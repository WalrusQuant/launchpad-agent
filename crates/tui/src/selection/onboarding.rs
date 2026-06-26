use super::super::*;
use crate::slash::{SlashCommandSpec, matching_slash_commands};

impl TuiApp {
    pub(crate) fn slash_suggestions(&self) -> Vec<SlashCommandSpec> {
        matching_slash_commands(self.input.text())
    }

    pub(crate) fn has_slash_suggestions(&self) -> bool {
        !self.slash_suggestions().is_empty()
    }

    pub(crate) fn has_selectable_aux_panel(&self) -> bool {
        matches!(
            self.aux_panel.as_ref().map(|panel| &panel.content),
            Some(AuxPanelContent::SessionList(_) | AuxPanelContent::ModelList(_))
                | Some(AuxPanelContent::ThinkingList(_))
                | Some(AuxPanelContent::PresetList(_))
        )
    }

    #[cfg(test)]
    pub(crate) fn is_preset_picker_open(&self) -> bool {
        matches!(
            self.aux_panel.as_ref().map(|panel| &panel.content),
            Some(AuxPanelContent::PresetList(_))
        )
    }

    pub(crate) fn is_onboarding_model_picker_open(&self) -> bool {
        self.show_model_onboarding
            && matches!(
                self.aux_panel.as_ref().map(|panel| &panel.content),
                Some(AuxPanelContent::ModelList(_))
            )
    }

    pub(crate) fn begin_custom_model_onboarding(&mut self) {
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = true;
        self.onboarding_base_url_pending = false;
        self.onboarding_api_key_pending = false;
        self.onboarding_selected_model = None;
        self.onboarding_selected_model_is_custom = true;
        self.onboarding_selected_base_url = None;
        self.onboarding_selected_api_key = None;
        self.onboarding_prompt = Some("model name".to_string());
        self.status_message.clear();
        self.input.clear();
    }

    pub(crate) fn begin_model_credentials_onboarding(&mut self, model: String) {
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = false;
        self.onboarding_base_url_pending = true;
        self.onboarding_api_key_pending = false;
        self.onboarding_selected_model = Some(model);
        self.onboarding_selected_model_is_custom = false;
        self.onboarding_selected_base_url = None;
        self.onboarding_selected_api_key = None;
        self.onboarding_prompt = Some("base url".to_string());
        self.status_message.clear();
        self.input.clear();
    }

    /// The provider preset chosen during onboarding, if any.
    pub(crate) fn current_preset(&self) -> Option<&'static lpa_core::ProviderPreset> {
        self.onboarding_preset_id
            .as_deref()
            .and_then(lpa_core::preset_by_id)
    }

    /// Finds a saved API key belonging to the same provider as `preset`.
    ///
    /// Identity is the preset's exact base URL paired with its wire API — not a
    /// prefix — so providers whose base URLs share a prefix can never lend each
    /// other a key. Returns `None` for presets without a default base URL (the
    /// custom/BYO endpoint), since there is nothing to match against.
    fn existing_api_key_for_preset(&self, preset: &lpa_core::ProviderPreset) -> Option<String> {
        let preset_base_url = preset.default_base_url?;
        self.saved_models
            .iter()
            .find(|entry| {
                entry.wire_api == preset.wire_api
                    && entry.base_url.as_deref() == Some(preset_base_url)
            })
            .and_then(|entry| entry.api_key.clone())
    }

    pub(crate) fn exit_onboarding(&mut self) {
        self.show_model_onboarding = false;
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = false;
        self.onboarding_prompt = None;
        self.onboarding_prompt_history.clear();
        self.onboarding_base_url_pending = false;
        self.onboarding_api_key_pending = false;
        self.onboarding_selected_model = None;
        self.onboarding_selected_model_is_custom = false;
        self.onboarding_selected_base_url = None;
        self.onboarding_selected_api_key = None;
        self.pending_validation_retry = None;
        self.input.clear();
        self.status_message = "Configuration dismissed".to_string();
    }

    pub(crate) fn start_onboarding(&mut self) {
        self.show_model_onboarding = true;
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = false;
        self.onboarding_prompt = None;
        self.onboarding_prompt_history.clear();
        self.onboarding_base_url_pending = false;
        self.onboarding_api_key_pending = false;
        self.onboarding_selected_model = None;
        self.onboarding_selected_model_is_custom = false;
        self.onboarding_selected_base_url = None;
        self.onboarding_selected_api_key = None;
        self.onboarding_preset_id = None;
        self.input.clear();

        let summary = self.current_config_summary();
        self.push_item(
            crate::events::TranscriptItemKind::System,
            "Current configuration",
            summary,
        );

        self.show_configure_preset_panel();
        self.status_message = "Configuration started".to_string();
    }

    pub(crate) fn current_config_summary(&self) -> String {
        let provider_label = self
            .active_preset_id()
            .and_then(|id| lpa_core::preset_by_id(&id).map(|p| p.display_name.to_string()))
            .unwrap_or_else(|| format!("{}", self.provider));
        let saved = self.saved_model_entry(&self.model);
        let base_url = saved
            .and_then(|s| s.base_url.clone())
            .unwrap_or_else(|| "(default)".to_string());
        let api_key = saved
            .and_then(|s| s.api_key.as_deref())
            .map(super::super::worker_events::mask_with_suffix)
            .unwrap_or_else(|| "(not set)".to_string());
        format!(
            "Provider: {provider_label}\nModel:    {}\nBase URL: {base_url}\nAPI key:  {api_key}",
            self.model
        )
    }

    pub(crate) fn handle_preset_selected(&mut self, preset_id: &str) {
        let Some(preset) = lpa_core::preset_by_id(preset_id) else {
            self.status_message = format!("Unknown preset: {preset_id}");
            return;
        };
        self.onboarding_preset_id = Some(preset.id.to_string());
        self.onboarding_selected_base_url = preset.default_base_url.map(str::to_string);
        self.onboarding_selected_api_key = None;
        self.onboarding_selected_model = None;
        self.onboarding_custom_model_pending = false;
        self.onboarding_base_url_pending = false;
        self.onboarding_api_key_pending = false;
        self.input.clear();

        // The wire API dictates the provider family for every preset; first-party
        // presets get their native family, OpenAI-compatible ones get OpenAI.
        self.provider = match preset.wire_api {
            lpa_core::ProviderWireApi::AnthropicMessages => {
                lpa_protocol::ProviderFamily::anthropic()
            }
            lpa_core::ProviderWireApi::GoogleGenerateContent => {
                lpa_protocol::ProviderFamily::google()
            }
            lpa_core::ProviderWireApi::OpenAIChatCompletions
            | lpa_core::ProviderWireApi::OpenAIResponses => lpa_protocol::ProviderFamily::openai(),
        };

        if preset.is_custom {
            // Bring-your-own endpoint: type a slug, then base URL + key.
            self.onboarding_custom_model_pending = true;
            self.onboarding_selected_model_is_custom = true;
            self.onboarding_prompt = Some(format!("model name — {}", preset.slug_hint));
            self.status_message = format!("Selected {}", preset.display_name);
            return;
        }

        // Every other provider ships a curated model list — show the picker.
        self.onboarding_selected_model_is_custom = false;
        self.onboarding_prompt = None;
        self.show_preset_model_panel();
        self.status_message = format!("Selected {} — choose a model", preset.display_name);
    }

    /// Advances onboarding once a model is known (picked from the curated list or
    /// typed). Reuses a saved API key for the provider when one exists so the
    /// user is not asked for it again; only prompts for a key the first time a
    /// provider that needs one is configured.
    pub(crate) fn proceed_with_preset_model(&mut self, model: String) -> Result<()> {
        self.onboarding_selected_model = Some(model);
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = false;
        self.input.clear();

        let preset = self.current_preset();

        if self.onboarding_selected_api_key.is_none()
            && let Some(preset) = preset
        {
            self.onboarding_selected_api_key = self.existing_api_key_for_preset(preset);
        }

        let needs_key = preset
            .map(|p| !p.api_key_env_vars.is_empty())
            .unwrap_or(true);
        if self.onboarding_selected_api_key.is_some() || !needs_key {
            self.onboarding_api_key_pending = false;
            self.onboarding_prompt = None;
            if self.onboarding_selected_api_key.is_some() {
                let label = preset.map(|p| p.display_name).unwrap_or("provider");
                self.status_message = format!("Reusing saved API key for {label}");
            }
            return self.begin_onboarding_validation();
        }

        // First time configuring this provider — ask for the key once.
        self.onboarding_api_key_pending = true;
        let label = preset.map(|p| p.display_name).unwrap_or("provider");
        self.onboarding_prompt = Some(match preset.and_then(|p| p.api_key_env_vars.first()) {
            Some(var) => format!("{label} API key (also read from ${var})"),
            None => format!("{label} API key"),
        });
        self.status_message = format!("Enter your {label} API key");
        Ok(())
    }

    /// Switches the preset model picker into manual-slug entry, keeping the
    /// preset's base URL and showing its format example as a hint.
    pub(crate) fn begin_preset_custom_model(&mut self) {
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = true;
        self.onboarding_selected_model = None;
        self.onboarding_selected_model_is_custom = true;
        let hint = self
            .current_preset()
            .map(|p| p.slug_hint)
            .unwrap_or("model slug");
        self.onboarding_prompt = Some(format!("model slug — {hint}"));
        self.input.clear();
        self.status_message = "Enter a model slug".to_string();
    }

    pub(crate) fn handle_escape(&mut self) -> bool {
        let preset_needs_model_list = self.current_preset().map(|p| !p.is_custom).unwrap_or(false);

        if self.onboarding_api_key_pending {
            self.onboarding_api_key_pending = false;
            self.input.clear();
            if preset_needs_model_list {
                self.onboarding_prompt = None;
                self.show_preset_model_panel();
            } else {
                self.onboarding_base_url_pending = true;
                self.onboarding_prompt = Some("base url".to_string());
            }
            return true;
        }
        if self.onboarding_base_url_pending {
            self.onboarding_base_url_pending = false;
            self.onboarding_selected_base_url = None;
            self.input.clear();
            if self.onboarding_selected_model_is_custom {
                self.onboarding_custom_model_pending = true;
                self.onboarding_prompt = Some("model name".to_string());
            } else {
                self.onboarding_prompt = None;
                self.show_onboarding_model_panel();
            }
            return true;
        }
        if self.onboarding_custom_model_pending {
            self.onboarding_custom_model_pending = false;
            self.onboarding_selected_model = None;
            self.onboarding_selected_model_is_custom = false;
            self.onboarding_prompt = None;
            self.input.clear();
            if preset_needs_model_list {
                self.show_preset_model_panel();
            } else {
                self.show_onboarding_model_panel();
            }
            return true;
        }
        if self.is_onboarding_model_picker_open() {
            // From a preset's model list, step back to the provider picker;
            // from the bare (no-preset) list, dismiss onboarding entirely.
            if self.onboarding_preset_id.is_some() {
                self.onboarding_preset_id = None;
                self.onboarding_selected_base_url = None;
                self.onboarding_selected_api_key = None;
                self.show_configure_preset_panel();
            } else {
                self.exit_onboarding();
            }
            return true;
        }
        false
    }
}
