use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use lpa_core::{
    EligibilitySelector, SessionConfig, SessionId, SessionTitleFinalSource, SessionTitleState,
    run_llm_compaction,
};
use lpa_safety::{SandboxMode, SandboxPolicyRecord, legacy_permissions::PermissionMode};

use crate::{
    ConnectionState, ProtocolErrorCode, ServerEvent, SessionCompactParams, SessionCompactResult,
    SessionContextClearParams, SessionContextClearResult, SessionEventPayload, SessionForkParams,
    SessionForkResult, SessionListParams, SessionListResult, SessionResumeParams,
    SessionResumeResult, SessionRuntimeStatus, SessionStartParams, SessionStartResult,
    SessionTitleUpdateParams, SessionTitleUpdateResult, SuccessResponse, execution::RuntimeSession,
    session::SessionSummary,
};

use super::ServerRuntime;

impl ServerRuntime {
    pub(super) async fn handle_initialize(
        &self,
        connection_id: u64,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let request_id = id.unwrap_or(serde_json::Value::Null);
        match serde_json::from_value::<crate::InitializeParams>(params) {
            Ok(params) => {
                let transport = params.transport.clone();
                let opt_out_notification_count = params.opt_out_notification_methods.len();
                if let Some(connection) = self.connections.lock().await.get_mut(&connection_id) {
                    connection.state = ConnectionState::Initializing;
                    connection.transport = params.transport;
                    connection.opt_out_notification_methods =
                        params.opt_out_notification_methods.into_iter().collect();
                }
                tracing::info!(
                    connection_id,
                    client_name = %params.client_name,
                    client_version = %params.client_version,
                    transport = ?transport,
                    supports_streaming = params.supports_streaming,
                    supports_binary_images = params.supports_binary_images,
                    opt_out_notification_count,
                    "accepted initialize request"
                );
                serde_json::to_value(SuccessResponse {
                    id: request_id,
                    result: self.metadata.clone(),
                })
                .expect("serialize initialize result")
            }
            Err(error) => self.error_response(
                request_id,
                ProtocolErrorCode::InvalidParams,
                format!("invalid initialize params: {error}"),
            ),
        }
    }

    pub(super) async fn handle_session_start(
        &self,
        connection_id: u64,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionStartParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/start params: {error}"),
                );
            }
        };

        let now = Utc::now();
        let session_id = SessionId::new();
        let resolved_model = params
            .model
            .clone()
            .unwrap_or_else(|| self.deps.default_model.clone());
        let record = (!params.ephemeral).then(|| {
            self.rollout_store.create_session_record(
                session_id,
                now,
                params.cwd.clone(),
                params.title.clone(),
                Some(resolved_model.clone()),
                self.deps.provider.name().to_string(),
                None,
            )
        });
        let summary = SessionSummary {
            session_id,
            cwd: params.cwd.clone(),
            created_at: now,
            updated_at: now,
            title: params.title.clone(),
            title_state: params
                .title
                .as_ref()
                .map(|_| SessionTitleState::Final(SessionTitleFinalSource::ExplicitCreate))
                .unwrap_or(SessionTitleState::Unset),
            ephemeral: params.ephemeral,
            resolved_model: Some(resolved_model.clone()),
            total_input_tokens: 0,
            total_output_tokens: 0,
            status: SessionRuntimeStatus::Idle,
        };
        if let Some(record) = &record
            && let Err(error) = self.rollout_store.append_session_meta(record)
        {
            return self.error_response(
                request_id,
                ProtocolErrorCode::InternalError,
                format!("failed to persist session metadata: {error}"),
            );
        }
        // Build the SessionConfig incrementally so a request that only sets
        // `sandbox_mode` doesn't silently flip `permission_mode` to AutoApprove
        // (and vice versa). When the caller passes neither, leave `None` so
        // `new_session_state` falls back to the server default config.
        let permission_mode = params
            .permission_mode
            .as_deref()
            .and_then(PermissionMode::parse);
        let sandbox_policy = params.sandbox_mode.as_deref().and_then(parse_sandbox_mode);
        let session_config = if permission_mode.is_some() || sandbox_policy.is_some() {
            let defaults = SessionConfig::default();
            Some(SessionConfig {
                permission_mode: permission_mode.unwrap_or(defaults.permission_mode),
                // An explicit per-session `sandbox_mode` overrides the server's
                // configured [sandbox] baseline; otherwise inherit the baseline
                // so a request that only overrides `permission_mode` does not
                // silently drop the configured sandbox.
                sandbox_policy: sandbox_policy.or_else(|| self.deps.sandbox_policy.clone()),
                ..defaults
            })
        } else {
            None
        };
        let core_session =
            self.deps
                .new_session_state(session_id, params.cwd.clone(), session_config);
        let steering_queue = Arc::clone(&core_session.pending_user_prompts);
        self.sessions.lock().await.insert(
            session_id,
            RuntimeSession {
                record,
                summary: summary.clone(),
                core_session: Arc::new(Mutex::new(core_session)),
                active_turn: None,
                latest_turn: None,
                loaded_item_count: 0,
                history_items: Vec::new(),
                steering_queue,
                active_task: None,
                next_item_seq: 1,
                approval_cache: Arc::new(Mutex::new(self.deps.new_approval_cache())),
            }
            .shared(),
        );
        self.subscribe_connection_to_session(connection_id, session_id, None)
            .await;
        tracing::info!(
            connection_id,
            session_id = %session_id,
            cwd = %summary.cwd.display(),
            ephemeral = summary.ephemeral,
            resolved_model = ?summary.resolved_model,
            has_title = summary.title.is_some(),
            "started session"
        );
        self.broadcast_event(ServerEvent::SessionStarted(SessionEventPayload {
            session: summary.clone(),
        }))
        .await;

        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionStartResult {
                session_id,
                created_at: now,
                cwd: params.cwd,
                ephemeral: params.ephemeral,
                resolved_model: Some(resolved_model),
            },
        })
        .expect("serialize session/start response")
    }

    pub(super) async fn handle_session_list(
        &self,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        if let Err(error) = serde_json::from_value::<SessionListParams>(params) {
            return self.error_response(
                request_id,
                ProtocolErrorCode::InvalidParams,
                format!("invalid session/list params: {error}"),
            );
        }
        let sessions = self
            .sessions
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut summaries = Vec::with_capacity(sessions.len());
        for session in sessions {
            summaries.push(session.lock().await.summary.clone());
        }
        summaries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionListResult {
                sessions: summaries,
            },
        })
        .expect("serialize session/list response")
    }

    pub(super) async fn handle_session_title_update(
        &self,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionTitleUpdateParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/title/update params: {error}"),
                );
            }
        };
        let new_title = params.title.trim();
        if new_title.is_empty() {
            return self.error_response(
                request_id,
                ProtocolErrorCode::InvalidParams,
                "session title cannot be empty",
            );
        }
        let Some(session_arc) = self.sessions.lock().await.get(&params.session_id).cloned() else {
            return self.error_response(
                request_id,
                ProtocolErrorCode::SessionNotFound,
                "session does not exist",
            );
        };

        let summary = {
            let mut session = session_arc.lock().await;
            let previous_title = session.summary.title.clone();
            let updated_at = Utc::now();
            session.summary.title = Some(new_title.to_string());
            session.summary.title_state =
                SessionTitleState::Final(SessionTitleFinalSource::UserRename);
            session.summary.updated_at = updated_at;
            if let Some(record) = session.record.as_mut() {
                record.title = Some(new_title.to_string());
                record.title_state = SessionTitleState::Final(SessionTitleFinalSource::UserRename);
                record.updated_at = updated_at;
                if let Err(error) = self.rollout_store.append_title_update(
                    record,
                    new_title.to_string(),
                    record.title_state.clone(),
                    previous_title,
                ) {
                    return self.error_response(
                        request_id,
                        ProtocolErrorCode::InternalError,
                        format!("failed to persist session title update: {error}"),
                    );
                }
            }
            session.summary.clone()
        };
        self.broadcast_event(ServerEvent::SessionTitleUpdated(SessionEventPayload {
            session: summary.clone(),
        }))
        .await;

        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionTitleUpdateResult { session: summary },
        })
        .expect("serialize session/title/update response")
    }

    pub(super) async fn handle_session_resume(
        &self,
        connection_id: u64,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionResumeParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/resume params: {error}"),
                );
            }
        };
        let Some(session_arc) = self.sessions.lock().await.get(&params.session_id).cloned() else {
            return self.error_response(
                request_id,
                ProtocolErrorCode::SessionNotFound,
                "session does not exist",
            );
        };
        let session = session_arc.lock().await;
        let session_summary = session.summary.clone();
        let latest_turn = session.latest_turn.clone();
        let loaded_item_count = session.loaded_item_count;
        let history_items = session.history_items.clone();
        drop(session);
        self.subscribe_connection_to_session(connection_id, params.session_id, None)
            .await;
        tracing::info!(
            connection_id,
            session_id = %params.session_id,
            loaded_item_count,
            has_latest_turn = latest_turn.is_some(),
            "resumed session"
        );
        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionResumeResult {
                session: session_summary,
                latest_turn,
                loaded_item_count,
                history_items,
            },
        })
        .expect("serialize session/resume response")
    }

    pub(super) async fn handle_session_fork(
        &self,
        connection_id: u64,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionForkParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/fork params: {error}"),
                );
            }
        };
        let Some(source_arc) = self.sessions.lock().await.get(&params.session_id).cloned() else {
            return self.error_response(
                request_id,
                ProtocolErrorCode::SessionNotFound,
                "session does not exist",
            );
        };
        let source = source_arc.lock().await;
        let source_core_session = source.core_session.lock().await;
        let now = Utc::now();
        let forked_id = SessionId::new();
        let fork_cwd = params.cwd.unwrap_or_else(|| source.summary.cwd.clone());
        let fork_model = source
            .summary
            .resolved_model
            .clone()
            .unwrap_or_else(|| self.deps.default_model.clone());
        let summary = SessionSummary {
            session_id: forked_id,
            cwd: fork_cwd.clone(),
            created_at: now,
            updated_at: now,
            title: params.title.or_else(|| source.summary.title.clone()),
            title_state: source.summary.title_state.clone(),
            ephemeral: source.summary.ephemeral,
            resolved_model: Some(fork_model.clone()),
            total_input_tokens: source_core_session.total_input_tokens,
            total_output_tokens: source_core_session.total_output_tokens,
            status: SessionRuntimeStatus::Idle,
        };
        let mut core_session = self.deps.new_session_state(forked_id, fork_cwd, None);
        core_session.messages = source_core_session.messages.clone();
        core_session.turn_count = source_core_session.turn_count;
        core_session.total_input_tokens = source_core_session.total_input_tokens;
        core_session.total_output_tokens = source_core_session.total_output_tokens;
        core_session.total_cache_creation_tokens = source_core_session.total_cache_creation_tokens;
        core_session.total_cache_read_tokens = source_core_session.total_cache_read_tokens;
        core_session.last_input_tokens = source_core_session.last_input_tokens;
        let latest_turn = source.latest_turn.clone();
        let loaded_item_count = source.loaded_item_count;
        let history_items = source.history_items.clone();
        drop(source_core_session);
        drop(source);
        let steering_queue = Arc::clone(&core_session.pending_user_prompts);
        self.sessions.lock().await.insert(
            forked_id,
            RuntimeSession {
                record: None,
                summary: summary.clone(),
                core_session: Arc::new(Mutex::new(core_session)),
                active_turn: None,
                latest_turn,
                loaded_item_count,
                history_items,
                steering_queue,
                active_task: None,
                next_item_seq: loaded_item_count + 1,
                approval_cache: Arc::new(Mutex::new(self.deps.new_approval_cache())),
            }
            .shared(),
        );
        let sessions = self.sessions.lock().await;
        if let Some(forked_session) = sessions.get(&forked_id).cloned() {
            drop(sessions);
            let mut forked_session = forked_session.lock().await;
            if !forked_session.summary.ephemeral {
                let record = self.rollout_store.create_session_record(
                    forked_id,
                    now,
                    forked_session.summary.cwd.clone(),
                    forked_session.summary.title.clone(),
                    forked_session.summary.resolved_model.clone(),
                    self.deps.provider.name().to_string(),
                    Some(params.session_id),
                );
                if let Err(error) = self.rollout_store.append_session_meta(&record) {
                    return self.error_response(
                        request_id,
                        ProtocolErrorCode::InternalError,
                        format!("failed to persist forked session metadata: {error}"),
                    );
                }
                forked_session.record = Some(record);
            }
        } else {
            drop(sessions);
        }
        self.subscribe_connection_to_session(connection_id, forked_id, None)
            .await;
        tracing::info!(
            connection_id,
            source_session_id = %params.session_id,
            forked_session_id = %forked_id,
            cwd = %summary.cwd.display(),
            ephemeral = summary.ephemeral,
            resolved_model = ?summary.resolved_model,
            "forked session"
        );
        self.broadcast_event(ServerEvent::SessionStarted(SessionEventPayload {
            session: summary.clone(),
        }))
        .await;
        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionForkResult {
                session: summary,
                forked_from_session_id: params.session_id,
            },
        })
        .expect("serialize session/fork response")
    }

    /// Manually compact a session's context by summarizing its older turns,
    /// mirroring the automatic compaction the query loop performs when the token
    /// budget is exceeded.
    pub(super) async fn handle_session_compact(
        &self,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionCompactParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/compact params: {error}"),
                );
            }
        };
        let Some(session_arc) = self.sessions.lock().await.get(&params.session_id).cloned() else {
            return self.error_response(
                request_id,
                ProtocolErrorCode::SessionNotFound,
                "session does not exist",
            );
        };

        let mut session = session_arc.lock().await;
        let model_slug = session
            .summary
            .resolved_model
            .clone()
            .unwrap_or_else(|| self.deps.default_model.clone());
        let core_session = Arc::clone(&session.core_session);
        let outcome = {
            let mut core = core_session.lock().await;
            run_llm_compaction(
                &mut core,
                Arc::clone(&self.deps.provider),
                &model_slug,
                &EligibilitySelector::default(),
            )
            .await
        };

        match outcome {
            Ok(Some(outcome)) => {
                session.summary.updated_at = Utc::now();
                let summary = session.summary.clone();
                serde_json::to_value(SuccessResponse {
                    id: request_id,
                    result: SessionCompactResult {
                        session: summary,
                        messages_removed: outcome.replaced_prefix_len,
                        summary_chars: outcome.summary.summary_text.chars().count(),
                    },
                })
                .expect("serialize session/compact response")
            }
            Ok(None) => self.error_response(
                request_id,
                ProtocolErrorCode::InvalidParams,
                "nothing eligible to compact yet",
            ),
            Err(error) => self.error_response(
                request_id,
                ProtocolErrorCode::InternalError,
                format!("compaction failed: {error}"),
            ),
        }
    }

    /// Clear a session's conversation context while keeping the session itself
    /// alive — the message history and accumulated token counts are reset so the
    /// next prompt starts from an empty context window.
    pub(super) async fn handle_session_context_clear(
        &self,
        request_id: serde_json::Value,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let params: SessionContextClearParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return self.error_response(
                    request_id,
                    ProtocolErrorCode::InvalidParams,
                    format!("invalid session/context/clear params: {error}"),
                );
            }
        };
        let Some(session_arc) = self.sessions.lock().await.get(&params.session_id).cloned() else {
            return self.error_response(
                request_id,
                ProtocolErrorCode::SessionNotFound,
                "session does not exist",
            );
        };

        let mut session = session_arc.lock().await;
        let core_session = Arc::clone(&session.core_session);
        let messages_removed = {
            let mut core = core_session.lock().await;
            let removed = core.messages.len();
            core.messages.clear();
            core.turn_count = 0;
            core.total_input_tokens = 0;
            core.total_output_tokens = 0;
            core.total_cache_creation_tokens = 0;
            core.total_cache_read_tokens = 0;
            core.last_input_tokens = 0;
            core.active_compaction = None;
            removed
        };

        session.history_items.clear();
        session.summary.total_input_tokens = 0;
        session.summary.total_output_tokens = 0;
        session.summary.updated_at = Utc::now();
        let summary = session.summary.clone();

        tracing::info!(session_id = %params.session_id, messages_removed, "cleared session context");

        serde_json::to_value(SuccessResponse {
            id: request_id,
            result: SessionContextClearResult {
                session: summary,
                messages_removed,
            },
        })
        .expect("serialize session/context/clear response")
    }
}

/// Maps the user-facing `sandbox_mode` string from `SessionStartParams` to the
/// structured `SandboxPolicyRecord` the runtime consumes. Returns `None` for
/// unknown values so the server can fall back to the default policy.
fn parse_sandbox_mode(value: &str) -> Option<SandboxPolicyRecord> {
    match value.trim().to_ascii_lowercase().as_str() {
        "unrestricted" => Some(SandboxPolicyRecord {
            mode: SandboxMode::Unrestricted,
            workspace_write: true,
        }),
        "workspace-write" | "workspace_write" => Some(SandboxPolicyRecord {
            mode: SandboxMode::Restricted,
            workspace_write: true,
        }),
        "read-only" | "read_only" | "readonly" => Some(SandboxPolicyRecord {
            mode: SandboxMode::Restricted,
            workspace_write: false,
        }),
        _ => None,
    }
}
