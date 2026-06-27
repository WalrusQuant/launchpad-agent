use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result};
use lpa_protocol::{
    ApprovalRespondParams, ApprovalRespondResult, ClientNotification, ClientRequest,
    ClientTransportKind, ErrorResponse, InitializeParams, InitializeResult, NotificationEnvelope,
    ProtocolErrorCode, ServerEvent, SessionCompactParams, SessionCompactResult,
    SessionContextClearParams, SessionContextClearResult, SessionForkParams, SessionForkResult,
    SessionListParams, SessionListResult, SessionResumeParams, SessionResumeResult,
    SessionStartParams, SessionStartResult, SessionTitleUpdateParams, SessionTitleUpdateResult,
    SkillChangedParams, SkillChangedResult, SkillListParams, SkillListResult, SuccessResponse,
    TurnInterruptParams, TurnInterruptResult, TurnStartParams, TurnStartResult, TurnSteerParams,
    TurnSteerResult,
};
use serde::de::DeserializeOwned;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::{Mutex, mpsc, oneshot},
    time::{Duration, timeout},
};

#[derive(Debug, Clone)]
pub struct StdioServerClientConfig {
    pub program: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ServerNotificationMessage {
    pub method: String,
    pub params: serde_json::Value,
}

/// Per-request response deadline for ordinary requests, once the server is up
/// and answering. These return promptly (turns stream over notifications, not
/// the request channel), so a tight bound surfaces a wedged server quickly.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Default deadline for the `initialize` handshake. This one is generous because
/// the server only starts answering after it has booted MCP servers and replayed
/// every persisted session from disk (see `run_server_process`), which can take
/// many seconds with a large session store or under load. Override with
/// `LPA_SERVER_INIT_TIMEOUT_SECS`.
const DEFAULT_INIT_TIMEOUT_SECS: u64 = 60;

pub struct StdioServerClient {
    child: Child,
    stdin: ChildStdin,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    next_request_id: AtomicU64,
    notifications_rx: mpsc::UnboundedReceiver<ServerNotificationMessage>,
    init_timeout: Duration,
}

impl StdioServerClient {
    pub async fn spawn(config: StdioServerClientConfig) -> Result<Self> {
        tracing::info!(
            program = %config.program.display(),
            workspace_root = ?config.workspace_root,
            env_override_count = config.env.len(),
            "spawning stdio server client"
        );
        let mut command = Command::new(&config.program);
        command.arg("server");
        for arg in config.args {
            command.arg(arg);
        }
        if let Some(workspace_root) = config.workspace_root {
            command.arg("--working-root").arg(workspace_root);
        }
        for (key, value) in config.env {
            command.env(key, value);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        // Guarantee the server child dies with this client even on paths that
        // skip the explicit `shutdown()` — a failed `initialize`, an early `?`,
        // a SIGPIPE on a truncated pipe, or a panic. Without this, those paths
        // orphan a running `lpagent server`, and accumulated orphans starve the
        // machine so later spawns time out during cold boot.
        command.kill_on_drop(true);

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn {}", config.program.display()))?;
        let stdin = child.stdin.take().context("capture server stdin")?;
        let stdout = child.stdout.take().context("capture server stdout")?;
        let stderr = child.stderr.take().context("capture server stderr")?;
        let pending = Arc::new(Mutex::new(
            HashMap::<u64, oneshot::Sender<serde_json::Value>>::new(),
        ));
        let (notifications_tx, notifications_rx) = mpsc::unbounded_channel();

        tokio::spawn(run_stdout_reader(
            BufReader::new(stdout).lines(),
            Arc::clone(&pending),
            notifications_tx,
        ));
        tokio::spawn(run_stderr_reader(BufReader::new(stderr).lines()));

        Ok(Self {
            child,
            stdin,
            pending,
            next_request_id: AtomicU64::new(1),
            notifications_rx,
            init_timeout: resolve_init_timeout(std::env::var("LPA_SERVER_INIT_TIMEOUT_SECS").ok()),
        })
    }

    pub async fn initialize(&mut self) -> Result<InitializeResult> {
        tracing::info!("initializing stdio server client");
        // `initialize` waits for the full server cold-boot (MCP + persisted
        // session replay), so it gets a longer deadline than ordinary requests.
        let result = self
            .request_with_timeout(
                "initialize",
                InitializeParams {
                    client_name: "lpagent".into(),
                    client_version: env!("CARGO_PKG_VERSION").into(),
                    transport: ClientTransportKind::Stdio,
                    supports_streaming: true,
                    supports_binary_images: false,
                    opt_out_notification_methods: Vec::new(),
                },
                self.init_timeout,
            )
            .await?;
        self.notify("initialized", serde_json::json!({})).await?;
        tracing::info!("stdio server client initialized");
        Ok(result)
    }

    pub async fn session_start(
        &mut self,
        params: SessionStartParams,
    ) -> Result<SessionStartResult> {
        self.request("session/start", params).await
    }

    pub async fn session_resume(
        &mut self,
        params: SessionResumeParams,
    ) -> Result<SessionResumeResult> {
        self.request("session/resume", params).await
    }

    pub async fn session_list(&mut self, params: SessionListParams) -> Result<SessionListResult> {
        self.request("session/list", params).await
    }

    pub async fn session_title_update(
        &mut self,
        params: SessionTitleUpdateParams,
    ) -> Result<SessionTitleUpdateResult> {
        self.request("session/title/update", params).await
    }

    pub async fn session_fork(&mut self, params: SessionForkParams) -> Result<SessionForkResult> {
        self.request("session/fork", params).await
    }

    pub async fn session_compact(
        &mut self,
        params: SessionCompactParams,
    ) -> Result<SessionCompactResult> {
        self.request("session/compact", params).await
    }

    pub async fn session_context_clear(
        &mut self,
        params: SessionContextClearParams,
    ) -> Result<SessionContextClearResult> {
        self.request("session/context/clear", params).await
    }

    pub async fn skills_list(&mut self, params: SkillListParams) -> Result<SkillListResult> {
        self.request("skills/list", params).await
    }

    pub async fn skills_changed(
        &mut self,
        params: SkillChangedParams,
    ) -> Result<SkillChangedResult> {
        self.request("skills/changed", params).await
    }

    pub async fn turn_start(&mut self, params: TurnStartParams) -> Result<TurnStartResult> {
        self.request("turn/start", params).await
    }

    pub async fn turn_interrupt(
        &mut self,
        params: TurnInterruptParams,
    ) -> Result<TurnInterruptResult> {
        self.request("turn/interrupt", params).await
    }

    pub async fn turn_steer(&mut self, params: TurnSteerParams) -> Result<TurnSteerResult> {
        self.request("turn/steer", params).await
    }

    /// Sends an approval decision to the server, unblocking the orchestrator's
    /// pending `Ask` branch. The server emits an `ApprovalDecisionItem` into
    /// the rollout journal and broadcasts `ServerRequestResolved` so other
    /// subscribers see the resolution.
    pub async fn approval_respond(
        &mut self,
        params: ApprovalRespondParams,
    ) -> Result<ApprovalRespondResult> {
        self.request("approval/respond", params).await
    }

    pub async fn recv_notification(&mut self) -> Option<ServerNotificationMessage> {
        self.notifications_rx.recv().await
    }

    pub async fn recv_event(&mut self) -> Result<Option<(String, ServerEvent)>> {
        let Some(notification) = self.recv_notification().await else {
            return Ok(None);
        };
        let event = serde_json::from_value(notification.params.clone()).with_context(|| {
            format!(
                "failed to decode server event for method {}",
                notification.method
            )
        })?;
        Ok(Some((notification.method, event)))
    }

    pub async fn shutdown(mut self) -> Result<()> {
        let _ = self.stdin.shutdown().await;
        self.child.kill().await.ok();
        let _ = self.child.wait().await;
        Ok(())
    }

    async fn request<P, R>(&mut self, method: &str, params: P) -> Result<R>
    where
        P: serde::Serialize,
        R: DeserializeOwned,
    {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
            .await
    }

    /// Sends one request and awaits its response under an explicit deadline.
    /// `initialize` passes a longer deadline than [`REQUEST_TIMEOUT`] because it
    /// races the server's cold boot.
    async fn request_with_timeout<P, R>(
        &mut self,
        method: &str,
        params: P,
        deadline: Duration,
    ) -> Result<R>
    where
        P: serde::Serialize,
        R: DeserializeOwned,
    {
        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        tracing::debug!(request_id, method, "sending client request");
        let (response_tx, response_rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id, response_tx);
        self.write_json(&ClientRequest {
            id: serde_json::json!(request_id),
            method: method.to_string(),
            params,
        })
        .await?;

        let response = timeout(deadline, response_rx)
            .await
            .with_context(|| {
                format!("timed out waiting for server response to request {request_id} ({method})")
            })?
            .with_context(|| format!("server dropped response for request {request_id}"))?;
        tracing::debug!(request_id, method, "received client response");
        if response.get("error").is_some() {
            let error: ErrorResponse =
                serde_json::from_value(response).context("decode error response from server")?;
            let data = if error.error.data.is_null() {
                String::new()
            } else {
                format!(" data={}", error.error.data)
            };
            anyhow::bail!(
                "server {}: {}{}",
                format_protocol_error_code(&error.error.code),
                error.error.message,
                data
            );
        }
        let success: SuccessResponse<R> =
            serde_json::from_value(response).context("decode success response from server")?;
        Ok(success.result)
    }

    async fn notify<P>(&mut self, method: &str, params: P) -> Result<()>
    where
        P: serde::Serialize,
    {
        self.write_json(&ClientNotification {
            method: method.to_string(),
            params,
        })
        .await
    }

    async fn write_json<T>(&mut self, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        let mut line = serde_json::to_vec(value).context("serialize client payload")?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .await
            .context("write client payload")?;
        self.stdin.flush().await.context("flush client payload")?;
        Ok(())
    }
}

async fn run_stdout_reader(
    mut lines: tokio::io::Lines<BufReader<ChildStdout>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    notifications_tx: mpsc::UnboundedSender<ServerNotificationMessage>,
) {
    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(message) => {
                if let Some(id) = message.get("id").and_then(serde_json::Value::as_u64) {
                    if let Some(tx) = pending.lock().await.remove(&id) {
                        let _ = tx.send(message);
                    }
                } else if let Ok(notification) =
                    serde_json::from_value::<NotificationEnvelope<serde_json::Value>>(message)
                {
                    let _ = notifications_tx.send(ServerNotificationMessage {
                        method: notification.method,
                        params: notification.params,
                    });
                }
            }
            Err(_) => {
                tracing::warn!(line = %line, "failed to parse JSON from server stdout");
            }
        }
    }
    tracing::warn!("server stdout reader stopped");
}

async fn run_stderr_reader(mut lines: tokio::io::Lines<BufReader<ChildStderr>>) {
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            tracing::warn!(server_stderr = %trimmed, "server child stderr");
        }
    }
    tracing::warn!("server stderr reader stopped");
}

fn format_protocol_error_code(code: &ProtocolErrorCode) -> &'static str {
    match code {
        ProtocolErrorCode::NotInitialized => "not_initialized",
        ProtocolErrorCode::InvalidParams => "invalid_params",
        ProtocolErrorCode::SessionNotFound => "session_not_found",
        ProtocolErrorCode::TurnNotFound => "turn_not_found",
        ProtocolErrorCode::TurnAlreadyRunning => "turn_already_running",
        ProtocolErrorCode::ApprovalNotFound => "approval_not_found",
        ProtocolErrorCode::PolicyDenied => "policy_denied",
        ProtocolErrorCode::ContextLimitExceeded => "context_limit_exceeded",
        ProtocolErrorCode::NoActiveTurn => "no_active_turn",
        ProtocolErrorCode::ExpectedTurnMismatch => "expected_turn_mismatch",
        ProtocolErrorCode::ActiveTurnNotSteerable => "active_turn_not_steerable",
        ProtocolErrorCode::EmptyInput => "empty_input",
        ProtocolErrorCode::InternalError => "internal_error",
    }
}

/// Resolves the `initialize` deadline from the optional `LPA_SERVER_INIT_TIMEOUT_SECS`
/// value, falling back to [`DEFAULT_INIT_TIMEOUT_SECS`] when unset, unparseable,
/// or zero. Pure so the parsing is unit-tested without touching process env.
fn resolve_init_timeout(env_value: Option<String>) -> Duration {
    let secs = env_value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .unwrap_or(DEFAULT_INIT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_INIT_TIMEOUT_SECS, resolve_init_timeout};
    use pretty_assertions::assert_eq;
    use tokio::time::Duration;

    #[test]
    fn init_timeout_defaults_when_unset() {
        assert_eq!(
            resolve_init_timeout(None),
            Duration::from_secs(DEFAULT_INIT_TIMEOUT_SECS)
        );
    }

    #[test]
    fn init_timeout_honors_valid_override() {
        assert_eq!(
            resolve_init_timeout(Some("120".to_string())),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn init_timeout_rejects_zero_and_garbage() {
        assert_eq!(
            resolve_init_timeout(Some("0".to_string())),
            Duration::from_secs(DEFAULT_INIT_TIMEOUT_SECS)
        );
        assert_eq!(
            resolve_init_timeout(Some("not-a-number".to_string())),
            Duration::from_secs(DEFAULT_INIT_TIMEOUT_SECS)
        );
    }
}
