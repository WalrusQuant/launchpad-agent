mod event_mapping;
mod history;
mod notifications;
mod provider_validate;
mod tool_render;

#[cfg(test)]
mod tests;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};

use lpa_core::{ProviderWireApi, SessionId, TurnId};
use lpa_protocol::{
    ApprovalDecisionValue, ApprovalRespondParams, ApprovalScopeValue, ProviderFamily,
};
use lpa_server::{
    InputItem, SessionListParams, SessionResumeParams, SessionStartParams,
    SessionTitleUpdateParams, SkillListParams, StdioServerClient, StdioServerClientConfig,
    TurnInterruptParams, TurnStartParams,
};

use crate::events::{SessionListEntry, WorkerEvent};

use history::project_history_items;
use notifications::{NotificationState, dispatch_server_notification};
use provider_validate::validate_provider_connection;
use tool_render::render_skill_list_body;

struct EnsureSessionOutcome {
    session_id: SessionId,
    resolved_model: Option<String>,
}

/// Immutable runtime configuration used to construct the background server client worker.
pub(crate) struct QueryWorkerConfig {
    /// Model identifier used for new turns.
    pub(crate) model: String,
    /// Working directory used for the server session.
    pub(crate) cwd: PathBuf,
    /// Environment overrides applied to the spawned server child process.
    pub(crate) server_env: Vec<(String, String)>,
    /// Optional log-level override forwarded to the server child process.
    pub(crate) server_log_level: Option<String>,
    /// Initial thinking mode used for new turns.
    pub(crate) thinking_selection: Option<String>,
}

/// Commands accepted by the background query worker.
enum OperationCommand {
    /// Submit a new user prompt to the session.
    SubmitPrompt(String),
    /// Update the model used for future turns.
    SetModel(String),
    /// Update the thinking mode used for future turns.
    SetThinking(Option<String>),
    /// Replace the provider connection settings and restart the server client.
    ReconfigureProvider {
        /// Provider wire protocol to use for future turns.
        wire_api: ProviderWireApi,
        /// Model identifier to use for future turns.
        model: String,
        /// Optional provider base URL override.
        base_url: Option<String>,
        /// Optional provider API key override.
        api_key: Option<String>,
    },
    /// Validates provider settings with a temporary probe request.
    ValidateProvider {
        provider: ProviderFamily,
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    },
    /// Request a session list from the server.
    ListSessions,
    /// Request a skills list from the server.
    ListSkills,
    /// Clear the active session so the next prompt starts a fresh one lazily.
    StartNewSession,
    /// Switch the active session to a persisted session identifier.
    SwitchSession(SessionId),
    /// Rename the current active session.
    RenameSession(String),
    /// Interrupt the active turn when one is running.
    InterruptTurn,
    /// Respond to a pending approval request.
    RespondApproval {
        session_id: SessionId,
        turn_id: TurnId,
        approval_id: smol_str::SmolStr,
        decision: ApprovalDecisionValue,
        scope: ApprovalScopeValue,
    },
    /// Stop the worker loop.
    Shutdown,
}

/// Handle used by the UI thread to interact with the background query worker.
pub(crate) struct QueryWorkerHandle {
    /// Sender used to submit commands to the worker.
    command_tx: mpsc::UnboundedSender<OperationCommand>,
    /// Receiver used by the UI to consume worker events.
    pub(crate) event_rx: mpsc::UnboundedReceiver<WorkerEvent>,
    /// Background task running the worker loop.
    join_handle: JoinHandle<()>,
}

impl QueryWorkerHandle {
    /// Spawns the background worker and returns the UI-facing handle.
    pub(crate) fn spawn(config: QueryWorkerConfig) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let join_handle = tokio::spawn(run_worker(config, command_rx, event_tx));
        Self {
            command_tx,
            event_rx,
            join_handle,
        }
    }

    /// Submits one prompt to the worker.
    pub(crate) fn submit_prompt(&self, prompt: String) -> Result<()> {
        self.command_tx
            .send(OperationCommand::SubmitPrompt(prompt))
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Updates the active session model for future turns.
    pub(crate) fn set_model(&self, model: String) -> Result<()> {
        self.command_tx
            .send(OperationCommand::SetModel(model))
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Updates the thinking mode used for future turns.
    pub(crate) fn set_thinking(&self, thinking: Option<String>) -> Result<()> {
        self.command_tx
            .send(OperationCommand::SetThinking(thinking))
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Reconfigures the provider connection used by the background server client.
    pub(crate) fn reconfigure_provider(
        &self,
        wire_api: ProviderWireApi,
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<()> {
        self.command_tx
            .send(OperationCommand::ReconfigureProvider {
                wire_api,
                model,
                base_url,
                api_key,
            })
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Validates provider settings with a temporary probe request.
    pub(crate) fn validate_provider(
        &self,
        provider: ProviderFamily,
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<()> {
        self.command_tx
            .send(OperationCommand::ValidateProvider {
                provider,
                model,
                base_url,
                api_key,
            })
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Requests the current persisted session list from the background worker.
    pub(crate) fn list_sessions(&self) -> Result<()> {
        self.command_tx
            .send(OperationCommand::ListSessions)
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Requests the current skill list from the background worker.
    pub(crate) fn list_skills(&self) -> Result<()> {
        self.command_tx
            .send(OperationCommand::ListSkills)
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Clears the active session so the next submitted prompt starts a fresh one lazily.
    pub(crate) fn start_new_session(&self) -> Result<()> {
        self.command_tx
            .send(OperationCommand::StartNewSession)
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Switches the active session to a persisted session identifier.
    pub(crate) fn switch_session(&self, session_id: SessionId) -> Result<()> {
        self.command_tx
            .send(OperationCommand::SwitchSession(session_id))
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Renames the current active session.
    pub(crate) fn rename_session(&self, title: String) -> Result<()> {
        self.command_tx
            .send(OperationCommand::RenameSession(title))
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Interrupts the active turn when one exists.
    pub(crate) fn interrupt_turn(&self) -> Result<()> {
        self.command_tx
            .send(OperationCommand::InterruptTurn)
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Forwards an approval decision to the server.
    pub(crate) fn respond_approval(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
        approval_id: smol_str::SmolStr,
        decision: ApprovalDecisionValue,
        scope: ApprovalScopeValue,
    ) -> Result<()> {
        self.command_tx
            .send(OperationCommand::RespondApproval {
                session_id,
                turn_id,
                approval_id,
                decision,
                scope,
            })
            .map_err(|_| anyhow::anyhow!("interactive worker is no longer running"))
    }

    /// Stops the worker task and waits for it to finish.
    pub(crate) async fn shutdown(self) -> Result<()> {
        let _ = self.command_tx.send(OperationCommand::Shutdown);
        match self.join_handle.await {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(map_join_error(error)),
        }
    }
}

#[cfg(test)]
impl QueryWorkerHandle {
    /// Creates a lightweight stub worker handle for unit tests that exercise UI logic only.
    pub(crate) fn stub() -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (_event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            command_tx,
            event_rx,
            join_handle: tokio::spawn(async move { while command_rx.recv().await.is_some() {} }),
        }
    }
}

async fn run_worker(
    config: QueryWorkerConfig,
    mut command_rx: mpsc::UnboundedReceiver<OperationCommand>,
    event_tx: mpsc::UnboundedSender<WorkerEvent>,
) {
    if let Err(error) = run_worker_inner(config, &mut command_rx, &event_tx).await {
        let _ = event_tx.send(WorkerEvent::TurnFailed {
            message: error.to_string(),
            turn_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        });
    }
}

async fn run_worker_inner(
    config: QueryWorkerConfig,
    command_rx: &mut mpsc::UnboundedReceiver<OperationCommand>,
    event_tx: &mpsc::UnboundedSender<WorkerEvent>,
) -> Result<()> {
    // The worker owns the server client and translates UI commands into server
    // calls, then turns server notifications back into lightweight UI events.
    let mut server_env = config.server_env;
    let mut client = spawn_client(
        &config.cwd,
        server_env.clone(),
        config.server_log_level.clone(),
    )
    .await?;
    let _ = client.initialize().await?;
    let mut session_id: Option<SessionId> = None;
    let mut session_cwd = config.cwd.clone();
    let mut model = config.model;
    let mut thinking_selection = config.thinking_selection;
    let mut active_turn_id: Option<TurnId> = None;
    let mut turn_count = 0usize;
    let mut total_input_tokens = 0usize;
    let mut total_output_tokens = 0usize;
    let mut latest_completed_agent_message: Option<String> = None;

    loop {
        tokio::select! {
            maybe_command = command_rx.recv() => {
                match maybe_command {
                    Some(OperationCommand::SubmitPrompt(prompt)) => {
                        let session_start = ensure_session_started(
                            &mut client,
                            &config.cwd,
                            &model,
                            &mut session_id,
                        )
                        .await?;
                        if let Some(resolved_model) = session_start.resolved_model.clone() {
                            model = resolved_model;
                        }
                        let active_session_id = session_start.session_id;
                        let start_result = client.turn_start(TurnStartParams {
                            session_id: active_session_id,
                            input: vec![InputItem::Text { text: prompt }],
                            model: Some(model.clone()),
                            thinking: thinking_selection.clone(),
                            sandbox: None,
                            approval_policy: None,
                            cwd: None,
                        }).await;
                        match start_result {
                            Ok(result) => {
                                active_turn_id = Some(result.turn_id);
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: error.to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::SetModel(next_model)) => {
                        model = next_model;
                    }
                    Some(OperationCommand::SetThinking(next_thinking)) => {
                        thinking_selection = next_thinking;
                    }
                    Some(OperationCommand::ValidateProvider {
                        provider,
                        model: next_model,
                        base_url,
                        api_key,
                    }) => {
                        match validate_provider_connection(
                            provider,
                            &next_model,
                            base_url,
                            api_key,
                        ).await {
                            Ok(reply_preview) => {
                                let _ = event_tx.send(WorkerEvent::ProviderValidationSucceeded {
                                    reply_preview,
                                });
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::ProviderValidationFailed {
                                    message: error.to_string(),
                                });
                            }
                        }
                    }
                Some(OperationCommand::ReconfigureProvider {
                    wire_api,
                    model: next_model,
                    base_url,
                    api_key,
                }) => {
                        // Recreate the client so new provider credentials take effect
                        // without requiring the whole app to restart.
                        model = next_model;
                        apply_env_override(
                            &mut server_env,
                            "LPA_PROVIDER",
                            wire_api.provider_family().as_str(),
                        );
                        apply_env_override(
                            &mut server_env,
                            "LPA_WIRE_API",
                            match wire_api {
                                ProviderWireApi::OpenAIChatCompletions => {
                                    "openai_chat_completions"
                                }
                                ProviderWireApi::OpenAIResponses => "openai_responses",
                                ProviderWireApi::AnthropicMessages => "anthropic_messages",
                                ProviderWireApi::GoogleGenerateContent => {
                                    "google_generate_content"
                                }
                            },
                        );
                        apply_env_override(&mut server_env, "LPA_MODEL", &model);
                        apply_optional_env_override(&mut server_env, "LPA_BASE_URL", base_url);
                        apply_optional_env_override(&mut server_env, "LPA_API_KEY", api_key);
                        client.shutdown().await?;
                        client = spawn_client(
                            &config.cwd,
                            server_env.clone(),
                            config.server_log_level.clone(),
                        )
                        .await?;
                        client.initialize().await?;
                        session_id = None;
                        active_turn_id = None;
                    }
                    Some(OperationCommand::ListSessions) => {
                        match tokio::time::timeout(
                            Duration::from_secs(5),
                            client.session_list(SessionListParams::default()),
                        )
                        .await
                        {
                            Ok(Ok(result)) => {
                                let sessions = result
                                    .sessions
                                    .iter()
                                    .map(|session| SessionListEntry {
                                        session_id: session.session_id,
                                        title: session
                                            .title
                                            .clone()
                                            .unwrap_or_else(|| "(untitled)".to_string()),
                                        updated_at: session
                                            .updated_at
                                            .format("%Y-%m-%d %H:%M:%S UTC")
                                            .to_string(),
                                        is_active: Some(session.session_id) == session_id,
                                    })
                                    .collect();
                                let _ = event_tx.send(WorkerEvent::SessionsListed { sessions });
                            }
                            Ok(Err(error)) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: error.to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                            Err(_) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: "session list request timed out".to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::ListSkills) => {
                        match tokio::time::timeout(
                            Duration::from_secs(5),
                            client.skills_list(SkillListParams {
                                cwd: Some(session_cwd.clone()),
                            }),
                        )
                        .await
                        {
                            Ok(Ok(result)) => {
                                let body = render_skill_list_body(&result.skills);
                                let _ = event_tx.send(WorkerEvent::SkillsListed { body });
                            }
                            Ok(Err(error)) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: error.to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                            Err(_) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: "skills list request timed out".to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::StartNewSession) => {
                        active_turn_id = None;
                        session_id = None;
                        session_cwd = config.cwd.clone();
                        let _ = event_tx.send(WorkerEvent::NewSessionPrepared);
                    }
                    Some(OperationCommand::SwitchSession(next_session_id)) => {
                        match client
                            .session_resume(SessionResumeParams {
                                session_id: next_session_id,
                            })
                            .await
                        {
                            Ok(result) => {
                                active_turn_id = None;
                                session_id = Some(next_session_id);
                                session_cwd = result.session.cwd.clone();
                                let _ = event_tx.send(WorkerEvent::SessionSwitched {
                                    session_id: next_session_id.to_string(),
                                    title: result.session.title,
                                    model: result.session.resolved_model,
                                    total_input_tokens: result.session.total_input_tokens,
                                    total_output_tokens: result.session.total_output_tokens,
                                    history_items: project_history_items(&result.history_items),
                                    loaded_item_count: result.loaded_item_count,
                                });
                                total_input_tokens = result.session.total_input_tokens;
                                total_output_tokens = result.session.total_output_tokens;
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: error.to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::RenameSession(title)) => {
                        let Some(active_session_id) = session_id else {
                            let _ = event_tx.send(WorkerEvent::TurnFailed {
                                message: "no active session exists yet; send a prompt or switch to a saved session first".to_string(),
                                turn_count,
                                total_input_tokens,
                                total_output_tokens,
                            });
                            continue;
                        };
                        match client
                            .session_title_update(SessionTitleUpdateParams {
                                session_id: active_session_id,
                                title: title.clone(),
                            })
                            .await
                        {
                            Ok(result) => {
                                let _ = event_tx.send(WorkerEvent::SessionRenamed {
                                    session_id: active_session_id.to_string(),
                                    title: result
                                        .session
                                        .title
                                        .unwrap_or(title),
                                });
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: error.to_string(),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::InterruptTurn) => {
                        if let (Some(turn_id), Some(active_session_id)) = (active_turn_id, session_id)
                            && let Err(error) = client
                                .turn_interrupt(TurnInterruptParams {
                                    session_id: active_session_id,
                                    turn_id,
                                    reason: Some("user requested interrupt".to_string()),
                                })
                                .await
                        {
                            let _ = event_tx.send(WorkerEvent::TurnFailed {
                                message: error.to_string(),
                                turn_count,
                                total_input_tokens,
                                total_output_tokens,
                            });
                        }
                    }
                    Some(OperationCommand::RespondApproval {
                        session_id: approval_session_id,
                        turn_id: approval_turn_id,
                        approval_id,
                        decision,
                        scope,
                    }) => {
                        let outcome = match &decision {
                            ApprovalDecisionValue::Approve => "approved",
                            ApprovalDecisionValue::Deny => "denied",
                            ApprovalDecisionValue::Cancel => "cancelled",
                        };
                        let approval_id_display = approval_id.to_string();
                        match client
                            .approval_respond(ApprovalRespondParams {
                                session_id: approval_session_id,
                                turn_id: approval_turn_id,
                                approval_id: approval_id.clone(),
                                decision,
                                scope,
                            })
                            .await
                        {
                            Ok(_) => {
                                let _ = event_tx.send(WorkerEvent::ApprovalResolved {
                                    approval_id: approval_id_display,
                                    outcome: outcome.to_string(),
                                });
                            }
                            Err(error) => {
                                let _ = event_tx.send(WorkerEvent::TurnFailed {
                                    message: format!(
                                        "approval/respond failed for {approval_id_display}: {error}"
                                    ),
                                    turn_count,
                                    total_input_tokens,
                                    total_output_tokens,
                                });
                            }
                        }
                    }
                    Some(OperationCommand::Shutdown) | None => {
                        break;
                    }
                }
            }
            notification = client.recv_event() => {
                match notification? {
                    Some((method, event)) => {
                        let mut state = NotificationState {
                            active_turn_id: &mut active_turn_id,
                            model: &mut model,
                            latest_completed_agent_message: &mut latest_completed_agent_message,
                            turn_count: &mut turn_count,
                            total_input_tokens: &mut total_input_tokens,
                            total_output_tokens: &mut total_output_tokens,
                            event_tx,
                        };
                        dispatch_server_notification(&method, event, &mut state);
                    }
                    None => break,
                }
            }
        }
    }

    client.shutdown().await?;
    Ok(())
}

async fn ensure_session_started(
    client: &mut StdioServerClient,
    cwd: &Path,
    model: &str,
    session_id: &mut Option<SessionId>,
) -> Result<EnsureSessionOutcome> {
    if let Some(session_id) = session_id {
        return Ok(EnsureSessionOutcome {
            session_id: *session_id,
            resolved_model: Some(model.to_string()),
        });
    }

    let session = client
        .session_start(SessionStartParams {
            cwd: cwd.to_path_buf(),
            ephemeral: false,
            title: None,
            model: Some(model.to_string()),
        })
        .await?;
    *session_id = Some(session.session_id);
    Ok(EnsureSessionOutcome {
        session_id: session.session_id,
        resolved_model: session.resolved_model,
    })
}

async fn spawn_client(
    cwd: &Path,
    env: Vec<(String, String)>,
    server_log_level: Option<String>,
) -> Result<StdioServerClient> {
    StdioServerClient::spawn(StdioServerClientConfig {
        program: std::env::current_exe().context("resolve current executable for server launch")?,
        workspace_root: Some(cwd.to_path_buf()),
        env,
        args: server_log_level
            .into_iter()
            .flat_map(|level| ["--log-level".to_string(), level])
            .collect(),
    })
    .await
}

fn apply_env_override(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, existing)) = env.iter_mut().find(|(existing_key, _)| existing_key == key) {
        *existing = value.to_string();
    } else {
        env.push((key.to_string(), value.to_string()));
    }
}

fn apply_optional_env_override(env: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    match value {
        Some(value) => apply_env_override(env, key, &value),
        None => env.retain(|(existing_key, _)| existing_key != key),
    }
}

fn map_join_error(error: JoinError) -> anyhow::Error {
    if error.is_cancelled() {
        anyhow::anyhow!("interactive worker task was cancelled")
    } else if error.is_panic() {
        anyhow::anyhow!("interactive worker task panicked")
    } else {
        anyhow::Error::new(error)
    }
}
