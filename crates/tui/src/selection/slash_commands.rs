use std::fmt::Write as _;

use super::super::*;
use super::rollout_files::{local_session_entries, read_redacted_config_toml};
use lpa_core::SessionId;

/// Where users are pointed for bug reports, feedback, and release notes.
const ISSUES_URL: &str = "https://github.com/WalrusQuant/launchpad-agent/issues";
const RELEASES_URL: &str = "https://github.com/WalrusQuant/launchpad-agent/releases";

impl TuiApp {
    pub(crate) fn handle_slash_command(&mut self, prompt: String) -> Result<()> {
        let trimmed = prompt.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let command = parts.next().unwrap_or_default();
        let argument = parts.next().map(str::trim).unwrap_or_default();

        match command {
            "/exit" => {
                self.emit_inline_command_echo(trimmed);
                self.dismiss_aux_panel();
                self.dismiss_slash_popup();
                self.reset_slash_selection();
                self.busy = false;
                self.last_ctrl_c_at = None;
                self.status_message = "Exiting".to_string();
                self.should_quit = true;
                Ok(())
            }
            "/help" => {
                self.emit_inline_command_echo(trimmed);
                let mut body = String::new();
                for spec in crate::slash::SLASH_COMMANDS {
                    let _ = writeln!(body, "{:<16}{}", spec.name, spec.description);
                }
                self.show_aux_panel("Commands", body.trim_end().to_string());
                self.status_message = "Slash commands shown".to_string();
                Ok(())
            }
            "/export" => {
                self.emit_inline_command_echo(trimmed);
                let target = if argument.is_empty() {
                    self.cwd.join("lpagent-transcript.md")
                } else {
                    let candidate = std::path::PathBuf::from(argument);
                    if candidate.is_absolute() {
                        candidate
                    } else {
                        self.cwd.join(candidate)
                    }
                };
                let body =
                    crate::transcript::export_transcript_markdown(&self.model, &self.transcript);
                match std::fs::write(&target, body) {
                    Ok(()) => {
                        self.show_aux_panel(
                            "Export",
                            format!("Transcript written to {}", target.display()),
                        );
                        self.status_message = "Transcript exported".to_string();
                    }
                    Err(error) => {
                        self.show_aux_panel(
                            "Export",
                            format!("Failed to write {}: {error}", target.display()),
                        );
                        self.status_message = "Export failed".to_string();
                    }
                }
                Ok(())
            }
            "/bug" | "/feedback" => {
                self.emit_inline_command_echo(trimmed);
                self.show_aux_panel(
                    "Feedback",
                    format!("Report bugs and share feedback at:\n{ISSUES_URL}"),
                );
                self.status_message = "Feedback link shown".to_string();
                Ok(())
            }
            "/release-notes" => {
                self.emit_inline_command_echo(trimmed);
                self.show_aux_panel(
                    "Release notes",
                    format!(
                        "lpagent v{}\n\nRelease notes:\n{RELEASES_URL}",
                        env!("CARGO_PKG_VERSION")
                    ),
                );
                self.status_message = "Release notes shown".to_string();
                Ok(())
            }
            "/status" => {
                self.emit_inline_command_echo(trimmed);
                self.show_aux_panel(
                    "Status",
                    format!(
                        "turns: {}\nmodel: {}\ntokens: {} in / {} out\nbusy: {}",
                        self.turn_count,
                        self.model,
                        self.total_input_tokens,
                        self.total_output_tokens,
                        self.busy
                    ),
                );
                self.status_message = "Session status shown".to_string();
                Ok(())
            }
            "/configure" | "/onboard" => {
                self.emit_inline_command_echo(trimmed);
                self.start_onboarding();
                Ok(())
            }
            "/sessions" => {
                self.emit_inline_command_echo(trimmed);
                let sessions = local_session_entries().unwrap_or_default();
                if sessions.is_empty() {
                    self.show_aux_panel("Sessions", "No sessions found");
                } else {
                    self.show_session_panel(sessions);
                }
                self.status_message = "Listing sessions".to_string();
                self.worker.list_sessions()?;
                Ok(())
            }
            "/skills" => {
                self.emit_inline_command_echo(trimmed);
                self.show_aux_panel("Skills", "Loading skills...");
                self.status_message = "Listing skills".to_string();
                self.worker.list_skills()?;
                Ok(())
            }
            "/thinking" => {
                self.emit_inline_command_echo(trimmed);
                self.show_thinking_panel();
                self.status_message = "Thinking options shown".to_string();
                Ok(())
            }
            "/reasoning" => {
                self.emit_inline_command_echo(trimmed);
                self.show_reasoning = !self.show_reasoning;
                self.status_message = if self.show_reasoning {
                    "Reasoning blocks expanded".to_string()
                } else {
                    "Reasoning blocks collapsed".to_string()
                };
                Ok(())
            }
            "/config" => {
                self.emit_inline_command_echo(trimmed);
                match read_redacted_config_toml() {
                    Ok(body) => {
                        self.show_aux_panel("Config", body);
                        self.status_message = "Config shown (keys masked)".to_string();
                    }
                    Err(error) => {
                        self.show_aux_panel(
                            "Config",
                            format!("Failed to read config.toml: {error}"),
                        );
                        self.status_message = "Failed to read config".to_string();
                    }
                }
                Ok(())
            }
            "/new" => {
                self.emit_inline_command_echo(trimmed);
                self.worker.start_new_session()?;
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.status_message = "New session ready; send a prompt to start it".to_string();
                Ok(())
            }
            "/compact" => {
                self.emit_inline_command_echo(trimmed);
                if self.busy {
                    anyhow::bail!("cannot compact while a turn is running");
                }
                self.worker.compact_session()?;
                self.status_message = "Compacting context...".to_string();
                Ok(())
            }
            "/clear" => {
                self.emit_inline_command_echo(trimmed);
                if self.busy {
                    anyhow::bail!("cannot clear context while a turn is running");
                }
                self.worker.clear_context()?;
                self.status_message = "Clearing context...".to_string();
                Ok(())
            }
            "/rename" => {
                self.emit_inline_command_echo(trimmed);
                if argument.is_empty() {
                    anyhow::bail!("usage: /rename <new title>");
                }
                self.worker.rename_session(argument.to_string())?;
                self.status_message = "Renaming current session".to_string();
                Ok(())
            }
            "/session" => {
                self.emit_inline_command_echo(trimmed);
                if argument.is_empty() || argument == "list" {
                    let sessions = local_session_entries().unwrap_or_default();
                    if sessions.is_empty() {
                        self.show_aux_panel("Sessions", "No sessions found");
                    } else {
                        self.show_session_panel(sessions);
                    }
                    self.status_message = "Listing sessions".to_string();
                    self.worker.list_sessions()?;
                    return Ok(());
                }

                let mut session_parts = argument.splitn(2, char::is_whitespace);
                let subcommand = session_parts.next().unwrap_or_default();
                let rest = session_parts.next().map(str::trim).unwrap_or_default();

                match subcommand {
                    "new" => {
                        self.worker.start_new_session()?;
                        self.aux_panel = None;
                        self.aux_panel_selection = 0;
                        self.status_message =
                            "New session ready; send a prompt to start it".to_string();
                        Ok(())
                    }
                    "rename" => {
                        if rest.is_empty() {
                            anyhow::bail!("usage: /session rename <new title>");
                        }
                        self.worker.rename_session(rest.to_string())?;
                        self.status_message = "Renaming current session".to_string();
                        Ok(())
                    }
                    "switch" => {
                        if rest.is_empty() {
                            anyhow::bail!("usage: /session switch <session_id>");
                        }
                        let session_id = rest.parse::<SessionId>().map_err(|error| {
                            anyhow::anyhow!("invalid session id `{rest}`: {error}")
                        })?;
                        self.worker.switch_session(session_id)?;
                        self.status_message = format!("Switching to session {rest}");
                        Ok(())
                    }
                    _ => {
                        let session_id = argument.parse::<SessionId>().map_err(|error| {
                            anyhow::anyhow!("invalid session command `{argument}`: {error}")
                        })?;
                        self.worker.switch_session(session_id)?;
                        self.status_message = format!("Switching to session {argument}");
                        Ok(())
                    }
                }
            }
            "/model" => {
                self.emit_inline_command_echo(trimmed);
                if argument.is_empty() {
                    self.show_model_switch_panel();
                    self.status_message = "Model switcher shown".to_string();
                    return Ok(());
                }

                if let Some(model) = self
                    .saved_models
                    .iter()
                    .find(|entry| entry.model == argument)
                    .cloned()
                {
                    if let Err(error) = self.reconfigure_saved_model(
                        model.wire_api,
                        model.model,
                        model.base_url,
                        model.api_key,
                    ) {
                        self.push_item(
                            TranscriptItemKind::Error,
                            "Model switch failed",
                            error.to_string(),
                        );
                        self.status_message = "Failed to switch model".to_string();
                        return Ok(());
                    }
                } else {
                    if let Err(error) = self.set_model(argument.to_string()) {
                        self.push_item(
                            TranscriptItemKind::Error,
                            "Model switch failed",
                            error.to_string(),
                        );
                        self.status_message = "Failed to switch model".to_string();
                        return Ok(());
                    }
                }
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.status_message = format!("Model set to {}", self.model);
                Ok(())
            }
            _ => self.submit_prompt(prompt),
        }
    }
}
