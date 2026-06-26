//! Headless (`-p` / `prompt`) execution.
//!
//! Headless drives the same `lpagent server` runtime the interactive TUI uses,
//! via [`StdioServerClient`]: it spawns a dedicated, single-tenant server
//! subprocess, starts a persisted session, runs one turn, prints the final
//! assistant message to stdout, and exits. Routing through the server (rather
//! than calling `lpa_core::query` directly) means headless runs persist a
//! rollout and become resumable/chainable — and reuses one implementation of
//! session/turn/persistence instead of duplicating it in the CLI.

use std::path::Path;

use anyhow::{Context, Result};
use lpa_client::{StdioServerClient, StdioServerClientConfig};
use lpa_core::{ResolvedProviderSettings, resolve_provider_settings};
use lpa_protocol::{
    InputItem, ServerEvent, SessionId, SessionListParams, SessionResumeParams, SessionStartParams,
    SessionSummary, TurnStartParams, TurnStatus,
};

use crate::event_text::completed_agent_message_text;
use crate::server_env::server_env_overrides;

/// Which session a headless run targets, resolved from the `--resume` /
/// `--continue` / `--session-id` flags (mutually exclusive, enforced by clap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSelector {
    /// No selector flag: start a fresh persisted session.
    New,
    /// `--resume <id>`: resume an explicit session id (error if unknown).
    Resume(SessionId),
    /// `--continue`: resume the most-recently-updated session in the cwd.
    Continue,
    /// `--session-id <id>`: resume the id if it exists, else create it.
    Adopt(SessionId),
}

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
    /// `--resume <id>` (raw, parsed in [`HeadlessOptions::session_selector`]).
    pub resume: Option<String>,
    /// `--continue`.
    pub continue_session: bool,
    /// `--session-id <id>` (raw, parsed in [`HeadlessOptions::session_selector`]).
    pub session_id: Option<String>,
}

impl HeadlessOptions {
    /// Resolves the mutually-exclusive resume flags into a [`SessionSelector`],
    /// parsing any supplied session id. clap guarantees at most one flag is set;
    /// the match arms below assume that but stay total.
    fn session_selector(&self) -> Result<SessionSelector> {
        if let Some(id) = &self.resume {
            return Ok(SessionSelector::Resume(parse_session_id(id, "--resume")?));
        }
        if let Some(id) = &self.session_id {
            return Ok(SessionSelector::Adopt(parse_session_id(
                id,
                "--session-id",
            )?));
        }
        if self.continue_session {
            return Ok(SessionSelector::Continue);
        }
        Ok(SessionSelector::New)
    }

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
/// `[sandbox]` config and default approval policy. `session_id` is `Some` only
/// for the `--session-id` create case.
fn session_params(
    cwd: std::path::PathBuf,
    model: String,
    skip_permissions: bool,
    session_id: Option<SessionId>,
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
        session_id,
        permission_mode,
        sandbox_mode,
    }
}

/// Parses a CLI-supplied session id, attributing a parse failure to the flag.
fn parse_session_id(value: &str, flag: &str) -> Result<SessionId> {
    value
        .parse::<SessionId>()
        .map_err(|error| anyhow::anyhow!("invalid session id for {flag}: {error}"))
}

/// Picks the session to resume for `--continue`: the first session whose `cwd`
/// matches `target_cwd`. `sessions` is expected pre-sorted most-recent-first
/// (the server's `session/list` orders by `updated_at` descending), so the
/// first cwd match is the most recently updated one.
fn select_continue_session(sessions: &[SessionSummary], target_cwd: &Path) -> Option<SessionId> {
    sessions
        .iter()
        .find(|session| session.cwd == target_cwd)
        .map(|session| session.session_id)
}

/// Runs a single headless turn through a dedicated server subprocess, printing
/// the final assistant message to stdout. Returns `Err` on turn failure so the
/// caller can map it to a non-zero exit code.
pub async fn run_headless(options: HeadlessOptions) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let selector = options.session_selector()?;
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

    let outcome = run_turn_for_selector(&mut client, &options, &resolved, &cwd, selector).await;
    client.shutdown().await.ok();
    let final_message = outcome?;

    match final_message {
        Some(text) => println!("{text}"),
        None => eprintln!("lpagent [prompt] empty response"),
    }
    Ok(())
}

/// Resolves the target session for the selector (start / resume / continue /
/// adopt), runs one turn against it, and returns the final assistant message.
async fn run_turn_for_selector(
    client: &mut StdioServerClient,
    options: &HeadlessOptions,
    resolved: &ResolvedProviderSettings,
    cwd: &Path,
    selector: SessionSelector,
) -> Result<Option<String>> {
    let session_id = resolve_target_session(client, options, resolved, cwd, selector).await?;
    client
        .turn_start(TurnStartParams {
            session_id,
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
    drive_turn_to_completion(client).await
}

/// Maps a [`SessionSelector`] to a concrete session id, starting or resuming as
/// needed. Resume/continue/adopt all rely on the dedicated server having loaded
/// every persisted session at bootstrap, so `session/list` and `session/resume`
/// see the full on-disk set.
async fn resolve_target_session(
    client: &mut StdioServerClient,
    options: &HeadlessOptions,
    resolved: &ResolvedProviderSettings,
    cwd: &Path,
    selector: SessionSelector,
) -> Result<SessionId> {
    match selector {
        SessionSelector::New => Ok(start_session(client, options, resolved, cwd, None)
            .await
            .context("start session")?),
        SessionSelector::Resume(id) => {
            resume(client, id).await.context("resume session")?;
            Ok(id)
        }
        SessionSelector::Continue => {
            let sessions = client
                .session_list(SessionListParams::default())
                .await
                .context("list sessions")?
                .sessions;
            let id = select_continue_session(&sessions, cwd)
                .with_context(|| format!("no session to continue in {}", cwd.display()))?;
            resume(client, id).await.context("resume session")?;
            Ok(id)
        }
        SessionSelector::Adopt(id) => {
            let sessions = client
                .session_list(SessionListParams::default())
                .await
                .context("list sessions")?
                .sessions;
            if sessions.iter().any(|session| session.session_id == id) {
                resume(client, id).await.context("resume session")?;
            } else {
                start_session(client, options, resolved, cwd, Some(id))
                    .await
                    .context("start session")?;
            }
            Ok(id)
        }
    }
}

/// Starts a persisted session via `session/start`, returning its id.
async fn start_session(
    client: &mut StdioServerClient,
    options: &HeadlessOptions,
    resolved: &ResolvedProviderSettings,
    cwd: &Path,
    session_id: Option<SessionId>,
) -> Result<SessionId> {
    let result = client
        .session_start(session_params(
            cwd.to_path_buf(),
            resolved.model.clone(),
            options.skip_permissions,
            session_id,
        ))
        .await?;
    Ok(result.session_id)
}

/// Resumes a session by id, discarding the (replayed) result — the headless
/// driver only needs the id confirmed live before starting a turn.
async fn resume(client: &mut StdioServerClient, session_id: SessionId) -> Result<()> {
    client
        .session_resume(SessionResumeParams { session_id })
        .await
        .map(|_| ())
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use lpa_protocol::{SessionId, SessionRuntimeStatus, SessionSummary, SessionTitleState};
    use pretty_assertions::assert_eq;

    use super::{HeadlessOptions, SessionSelector, select_continue_session};

    fn opts() -> HeadlessOptions {
        HeadlessOptions {
            prompt: "hi".to_string(),
            model: None,
            system_prompt: None,
            append_system_prompt: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            skip_permissions: false,
            resume: None,
            continue_session: false,
            session_id: None,
        }
    }

    fn summary(id: SessionId, cwd: &str) -> SessionSummary {
        SessionSummary {
            session_id: id,
            cwd: PathBuf::from(cwd),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            title: None,
            title_state: SessionTitleState::Unset,
            ephemeral: false,
            resolved_model: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            status: SessionRuntimeStatus::Idle,
        }
    }

    #[test]
    fn selector_defaults_to_new() {
        assert_eq!(opts().session_selector().unwrap(), SessionSelector::New);
    }

    #[test]
    fn selector_continue() {
        let selector = HeadlessOptions {
            continue_session: true,
            ..opts()
        }
        .session_selector()
        .unwrap();
        assert_eq!(selector, SessionSelector::Continue);
    }

    #[test]
    fn selector_resume_parses_id() {
        let id = SessionId::new();
        let selector = HeadlessOptions {
            resume: Some(id.to_string()),
            ..opts()
        }
        .session_selector()
        .unwrap();
        assert_eq!(selector, SessionSelector::Resume(id));
    }

    #[test]
    fn selector_session_id_adopts() {
        let id = SessionId::new();
        let selector = HeadlessOptions {
            session_id: Some(id.to_string()),
            ..opts()
        }
        .session_selector()
        .unwrap();
        assert_eq!(selector, SessionSelector::Adopt(id));
    }

    #[test]
    fn selector_rejects_invalid_id() {
        let error = HeadlessOptions {
            resume: Some("not-a-uuid".to_string()),
            ..opts()
        }
        .session_selector()
        .unwrap_err()
        .to_string();
        assert!(error.contains("--resume"), "got: {error}");
    }

    #[test]
    fn continue_picks_first_cwd_match() {
        let here = SessionId::new();
        let elsewhere = SessionId::new();
        // Pre-sorted most-recent-first, as the server's session/list returns.
        let sessions = vec![summary(elsewhere, "/other/dir"), summary(here, "/work/dir")];
        assert_eq!(
            select_continue_session(&sessions, Path::new("/work/dir")),
            Some(here)
        );
    }

    #[test]
    fn continue_none_when_no_cwd_match() {
        let sessions = vec![summary(SessionId::new(), "/other/dir")];
        assert_eq!(
            select_continue_session(&sessions, Path::new("/work/dir")),
            None
        );
    }
}
