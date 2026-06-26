//! Headless (`-p` / `prompt`) execution.
//!
//! Headless drives the same `lpagent server` runtime the interactive TUI uses,
//! via [`StdioServerClient`]: it spawns a dedicated, single-tenant server
//! subprocess, starts a persisted session, runs one turn, prints the final
//! assistant message to stdout, and exits. Routing through the server (rather
//! than calling `lpa_core::query` directly) means headless runs persist a
//! rollout and become resumable/chainable — and reuses one implementation of
//! session/turn/persistence instead of duplicating it in the CLI.

use anyhow::{Context, Result};
use lpa_client::{StdioServerClient, StdioServerClientConfig};
use lpa_core::{ResolvedProviderSettings, resolve_provider_settings};
use lpa_protocol::{InputItem, ServerEvent, SessionStartParams, TurnStartParams, TurnStatus};

use crate::event_text::completed_agent_message_text;
use crate::server_env::server_env_overrides;

/// Resolved options for a single non-interactive (headless) run.
///
/// Logging is intentionally not part of this struct: `install_logging` in
/// `main` already installs the global subscriber (honoring `--verbose` /
/// `--debug`) before this runs.
pub struct HeadlessOptions {
    pub prompt: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub skip_permissions: bool,
}

impl HeadlessOptions {
    /// Builds the env overrides handed to the dedicated server subprocess:
    /// provider settings plus the headless-only flags (system prompt, tool
    /// filters) the server honors at bootstrap. `--dangerously-skip-permissions`
    /// is carried via the `session/start` params instead (see [`session_params`]).
    fn server_env(&self, resolved: &ResolvedProviderSettings) -> Vec<(String, String)> {
        let mut env = server_env_overrides(resolved);
        if let Some(system_prompt) = &self.system_prompt {
            env.push(("LPA_SYSTEM_PROMPT".to_string(), system_prompt.clone()));
        }
        if let Some(append) = &self.append_system_prompt {
            env.push(("LPA_APPEND_SYSTEM_PROMPT".to_string(), append.clone()));
        }
        if !self.allowed_tools.is_empty() {
            env.push((
                "LPA_ALLOWED_TOOLS".to_string(),
                self.allowed_tools.join(","),
            ));
        }
        if !self.disallowed_tools.is_empty() {
            env.push((
                "LPA_DISALLOWED_TOOLS".to_string(),
                self.disallowed_tools.join(","),
            ));
        }
        env
    }
}

/// Builds the `session/start` params for a headless run. `--dangerously-skip-
/// permissions` maps onto the existing per-session permission/sandbox modes
/// (`auto-approve` + `unrestricted`); otherwise the server applies its own
/// `[sandbox]` config and default approval policy.
fn session_params(
    cwd: std::path::PathBuf,
    model: String,
    skip_permissions: bool,
) -> SessionStartParams {
    let (permission_mode, sandbox_mode) = if skip_permissions {
        (
            Some("auto-approve".to_string()),
            Some("unrestricted".to_string()),
        )
    } else {
        (None, None)
    };
    SessionStartParams {
        cwd,
        ephemeral: false,
        title: None,
        model: Some(model),
        permission_mode,
        sandbox_mode,
    }
}

/// Runs a single headless turn through a dedicated server subprocess, printing
/// the final assistant message to stdout. Returns `Err` on turn failure so the
/// caller can map it to a non-zero exit code.
pub async fn run_headless(options: HeadlessOptions) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut resolved = resolve_provider_settings()
        .map_err(|error| anyhow::anyhow!("failed to resolve provider: {error}"))?;
    if let Some(model) = &options.model {
        resolved.model = model.clone();
    }

    let env = options.server_env(&resolved);
    let mut client = StdioServerClient::spawn(StdioServerClientConfig {
        program: std::env::current_exe().context("resolve current executable for server launch")?,
        workspace_root: Some(cwd.clone()),
        env,
        args: Vec::new(),
    })
    .await
    .context("spawn server subprocess")?;
    client.initialize().await.context("initialize server")?;

    let session = client
        .session_start(session_params(
            cwd,
            resolved.model.clone(),
            options.skip_permissions,
        ))
        .await
        .context("start session")?;

    client
        .turn_start(TurnStartParams {
            session_id: session.session_id,
            input: vec![InputItem::Text {
                text: options.prompt.clone(),
            }],
            model: None,
            thinking: None,
            sandbox: None,
            approval_policy: None,
            cwd: None,
        })
        .await
        .context("start turn")?;

    let outcome = drive_turn_to_completion(&mut client).await;
    client.shutdown().await.ok();
    let final_message = outcome?;

    match final_message {
        Some(text) => println!("{text}"),
        None => eprintln!("lpagent [prompt] empty response"),
    }
    Ok(())
}

/// Drains server notifications until the turn completes, accumulating the latest
/// assistant message. `turn/completed` carries only status — the assistant text
/// arrives via `item/completed` events — so the final message is captured from
/// the last completed agent-message item. Returns `Err` when the turn fails.
async fn drive_turn_to_completion(client: &mut StdioServerClient) -> Result<Option<String>> {
    let mut final_message: Option<String> = None;
    loop {
        match client.recv_event().await? {
            Some((_, ServerEvent::ItemCompleted(payload))) => {
                if let Some(text) = completed_agent_message_text(&payload) {
                    final_message = Some(text);
                }
            }
            Some((_, ServerEvent::TurnCompleted(payload))) => match payload.turn.status {
                TurnStatus::Completed => return Ok(final_message),
                TurnStatus::Failed => {
                    let detail = final_message.unwrap_or_else(|| "turn failed".to_string());
                    anyhow::bail!("prompt failed: {detail}");
                }
                other => anyhow::bail!("prompt ended with unexpected status {other:?}"),
            },
            Some(_) => {}
            None => anyhow::bail!("server closed before the turn completed"),
        }
    }
}
