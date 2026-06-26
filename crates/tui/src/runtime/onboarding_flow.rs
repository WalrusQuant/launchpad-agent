//! Onboarding/approval submission flows: approval responses, validation
//! retry/skip/change, and the prompt-submission state machine. Split out of
//! `runtime.rs`.

use super::super::*;

impl TuiApp {
    /// Sends the given decision to the server for the currently pending approval.
    /// The UI optimistically updates the transcript while the worker round-trip
    /// completes; the server-emitted `ApprovalResolved` event is the source of
    /// truth and will rewrite the same transcript item if the outcomes differ.
    pub(crate) fn submit_pending_approval(
        &mut self,
        decision: lpa_protocol::ApprovalDecisionValue,
    ) {
        let Some(state) = self.pending_approval.take() else {
            return;
        };
        let Some(turn_id) = state.turn_id else {
            self.push_item(
                TranscriptItemKind::Error,
                "Approval failed",
                "no active turn recorded for pending approval".to_string(),
            );
            self.status_message = "Approval rejected: no active turn".to_string();
            return;
        };
        let outcome = match &decision {
            lpa_protocol::ApprovalDecisionValue::Approve => "approved",
            lpa_protocol::ApprovalDecisionValue::Deny => "denied",
            lpa_protocol::ApprovalDecisionValue::Cancel => "cancelled",
        };
        if let Some(item) = self.transcript.get_mut(state.transcript_index) {
            item.kind = TranscriptItemKind::ApprovalResolution;
            item.title = format!("Approval {outcome}");
            item.body = format!(
                "{}\n(sent {outcome} to server)",
                item.body.lines().next().unwrap_or("")
            );
        }
        if let Err(error) = self.worker.respond_approval(
            state.session_id,
            turn_id,
            state.approval_id,
            decision,
            lpa_protocol::ApprovalScopeValue::Once,
        ) {
            self.push_item(
                TranscriptItemKind::Error,
                "Approval failed",
                format!("worker refused approval response: {error}"),
            );
            self.status_message = "Approval submission failed".to_string();
            return;
        }
        self.status_message = format!("Approval {outcome}");
    }

    /// Re-runs the connection probe with the same inputs that failed last time.
    /// Surfaces the worker error (if dispatch itself fails) to the transcript.
    pub(crate) fn retry_validation(&mut self) {
        if self.pending_validation_retry.take().is_none() {
            return;
        }
        self.input.clear();
        if let Err(error) = self.begin_onboarding_validation() {
            self.push_item(
                TranscriptItemKind::Error,
                "Validation failed",
                error.to_string(),
            );
            self.status_message = format!("Validation retry failed: {error}");
        }
    }

    /// Persists the pending onboarding inputs without re-running the probe.
    /// Used when a user knows the endpoint is reachable despite a failed probe
    /// (for example a local Ollama that doesn't accept the default test payload).
    pub(crate) fn skip_validation_and_save(&mut self) {
        if self.pending_validation_retry.take().is_none() {
            return;
        }
        self.input.clear();
        self.push_item(
            TranscriptItemKind::System,
            "Configure",
            "Saving without validation. If requests fail later, re-run \
             /configure to change the provider settings."
                .to_string(),
        );
        if let Err(error) = self.finish_onboarding_selection() {
            self.push_item(
                TranscriptItemKind::Error,
                "Onboarding failed",
                error.to_string(),
            );
            self.status_message = "Failed to save onboarding settings".to_string();
        }
    }

    /// Returns the onboarding flow to input entry after a failure. The selected
    /// model and base URL are kept; the most common failure cause is a bad or
    /// expired key, so a key-bearing provider re-prompts for the API key, while
    /// a keyless provider (e.g. local runtimes) re-prompts for the model slug.
    pub(crate) fn change_validation_inputs(&mut self) {
        if self.pending_validation_retry.take().is_none() {
            return;
        }
        self.input.clear();
        let preset = self.current_preset();
        let label = preset.map(|p| p.display_name).unwrap_or("provider");
        let needs_key = preset
            .map(|p| !p.api_key_env_vars.is_empty())
            .unwrap_or(true);

        if needs_key {
            self.onboarding_selected_api_key = None;
            self.onboarding_api_key_pending = true;
            // Mirror the initial prompt so the env-var fallback stays discoverable.
            self.onboarding_prompt = Some(match preset.and_then(|p| p.api_key_env_vars.first()) {
                Some(var) => format!("{label} API key (also read from ${var})"),
                None => format!("{label} API key"),
            });
            self.status_message = format!("Enter a different API key for {label}");
        } else {
            let hint = preset.map(|p| p.slug_hint).unwrap_or("model slug");
            self.onboarding_custom_model_pending = true;
            self.onboarding_selected_model = None;
            self.onboarding_prompt = Some(format!("model slug for {label} — {hint}"));
            self.status_message = format!("Enter a different model slug for {label}");
        }
    }

    pub(crate) fn handle_submission(&mut self, prompt: String) -> Result<()> {
        // Onboarding states consume input locally; only normal prompts reach the worker.
        if self.onboarding_custom_model_pending {
            let model = prompt.trim();
            if model.is_empty() {
                self.onboarding_prompt = Some("model name".to_string());
                return Ok(());
            }

            self.onboarding_custom_model_pending = false;
            self.onboarding_selected_model = Some(model.to_string());
            self.onboarding_selected_model_is_custom = true;
            self.input.clear();
            self.onboarding_prompt_history
                .push(format!("model> {model}"));

            let is_custom_preset = self.current_preset().map(|p| p.is_custom).unwrap_or(false);

            // A curated preset already knows its base URL — resolve/ask for the
            // key (reusing a saved one) and validate.
            if self.onboarding_preset_id.is_some() && !is_custom_preset {
                return self.proceed_with_preset_model(model.to_string());
            }

            // Custom / BYO endpoint: model first, then base_url + api_key.
            self.onboarding_base_url_pending = true;
            self.aux_panel = None;
            self.aux_panel_selection = 0;
            self.onboarding_prompt = Some("base url".to_string());
            self.status_message.clear();
            return Ok(());
        }

        if self.onboarding_base_url_pending {
            let base_url = prompt.trim();
            if !(base_url.is_empty()
                || base_url.starts_with("http://")
                || base_url.starts_with("https://"))
            {
                self.status_message = "Base URL must start with http:// or https://".to_string();
                self.onboarding_prompt = Some("base url".to_string());
                return Ok(());
            }
            self.onboarding_base_url_pending = false;
            self.onboarding_api_key_pending = true;
            self.onboarding_selected_base_url = if base_url.is_empty() {
                None
            } else {
                Some(base_url.to_string())
            };
            self.onboarding_prompt_history.push(format!(
                "base url> {}",
                self.onboarding_selected_base_url.as_deref().unwrap_or("")
            ));
            if let Some(model) = self.onboarding_selected_model.clone() {
                self.push_item(
                    TranscriptItemKind::System,
                    "Configure",
                    format!(
                        "base url> {}",
                        self.onboarding_selected_base_url
                            .as_deref()
                            .unwrap_or("(empty)")
                    ),
                );
                self.status_message = format!("Base URL saved for {model}");
            }
            self.input.clear();
            self.onboarding_prompt = Some("api key".to_string());
            return Ok(());
        }

        if self.onboarding_api_key_pending {
            let api_key = prompt.trim();
            self.onboarding_api_key_pending = false;
            // Empty input preserves any pre-populated key (set by the preset
            // picker when a saved key exists); a non-empty entry always
            // replaces it.
            if !api_key.is_empty() {
                self.onboarding_selected_api_key = Some(api_key.to_string());
            }
            self.onboarding_prompt_history.push(format!(
                "api key> {}",
                self.onboarding_selected_api_key
                    .as_deref()
                    .map(super::worker_events::mask_secret)
                    .unwrap_or_default()
            ));

            // A model was already chosen (curated pick, custom preset, or legacy
            // flow) — validate now.
            if self.onboarding_selected_model.is_some() {
                return self.begin_onboarding_validation();
            }

            // Fallback: a preset still needs a model slug.
            if let Some(preset) = self.current_preset() {
                let label = preset.display_name;
                let hint = preset.slug_hint;
                self.onboarding_custom_model_pending = true;
                self.input.clear();
                self.onboarding_prompt = Some(format!("model slug for {label} — {hint}"));
                self.status_message = format!("API key saved for {label}");
                return Ok(());
            }

            // Legacy flow — model was captured earlier; validate now.
            self.begin_onboarding_validation()
        } else if let Some(note) = prompt.trim_start().strip_prefix('#') {
            self.append_memory_line(note)
        } else if prompt.trim_start().starts_with('/') {
            self.handle_slash_command(prompt)
        } else {
            self.submit_prompt(prompt)
        }
    }

    /// Triggers the connection-test path once the preset flow has collected
    /// base URL (optional), API key (optional), and model slug.
    pub(crate) fn begin_onboarding_validation(&mut self) -> Result<()> {
        let Some(model) = self.onboarding_selected_model.clone() else {
            anyhow::bail!("onboarding model selection was lost before validation");
        };
        let provider = self.onboarding_provider_for_model(&model);
        self.busy = true;
        self.status_message = "Validating provider connection".to_string();
        self.worker.validate_provider(
            provider,
            model,
            self.onboarding_selected_base_url.clone(),
            self.onboarding_selected_api_key.clone(),
        )?;
        Ok(())
    }

    /// Append a `#`-prefixed note as a bullet to the project memory file.
    ///
    /// Prefers an existing `AGENTS.md`, then `CLAUDE.md`, and otherwise creates
    /// `AGENTS.md` in the working directory. Mirrors Claude Code's `#` shortcut
    /// for jotting a durable instruction without leaving the composer.
    pub(crate) fn append_memory_line(&mut self, note: &str) -> Result<()> {
        let note = note.trim();
        if note.is_empty() {
            self.status_message = "Nothing to add to memory".to_string();
            return Ok(());
        }

        self.emit_inline_command_echo(&format!("# {note}"));

        let target = ["AGENTS.md", "CLAUDE.md"]
            .into_iter()
            .map(|name| self.cwd.join(name))
            .find(|path| path.exists())
            .unwrap_or_else(|| self.cwd.join("AGENTS.md"));

        use std::io::Write as _;
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .and_then(|mut file| writeln!(file, "- {note}"));

        match result {
            Ok(()) => {
                self.push_item(
                    TranscriptItemKind::System,
                    "Memory",
                    format!("Added to {}: {note}", target.display()),
                );
                self.status_message = "Memory note added".to_string();
            }
            Err(error) => {
                self.push_item(
                    TranscriptItemKind::Error,
                    "Memory",
                    format!("Failed to update {}: {error}", target.display()),
                );
                self.status_message = "Failed to add memory note".to_string();
            }
        }
        Ok(())
    }

    pub(crate) fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        if self.input.is_blank() && prompt.trim().is_empty() {
            return Ok(());
        }

        self.close_inline_assistant_stream();
        self.push_item(TranscriptItemKind::User, "You", prompt.clone());
        self.pending_status_index =
            Some(self.push_item(TranscriptItemKind::System, "Thinking", ""));
        self.follow_output = true;
        self.busy = true;
        self.reset_slash_selection();
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.pending_assistant_index = None;
        self.turn_emitted_text = false;
        self.worker.submit_prompt(prompt)
    }
}
