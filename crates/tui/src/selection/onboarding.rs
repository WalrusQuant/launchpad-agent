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

    fn existing_api_key_for_preset(&self, preset_base_url: Option<&str>) -> Option<String> {
        let preset_base_url = preset_base_url?;
        self.saved_models
            .iter()
            .find(|entry| {
                entry
                    .base_url
                    .as_deref()
                    .is_some_and(|url| url.starts_with(preset_base_url))
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

        match preset.id {
            "anthropic" | "openai" | "google" => {
                self.provider = match preset.id {
                    "anthropic" => lpa_protocol::ProviderFamily::anthropic(),
                    "openai" => lpa_protocol::ProviderFamily::openai(),
                    "google" => lpa_protocol::ProviderFamily::google(),
                    _ => unreachable!(),
                };
                self.show_onboarding_model_panel();
                self.onboarding_prompt = None;
                self.status_message = format!("Selected {}", preset.display_name);
            }
            "custom" => {
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.onboarding_custom_model_pending = true;
                self.onboarding_selected_model_is_custom = true;
                self.onboarding_prompt = Some("model name".to_string());
                self.status_message = format!("Selected {}", preset.display_name);
                self.input.clear();
            }
            _ => {
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.onboarding_custom_model_pending = false;
                self.onboarding_selected_model_is_custom = true;
                self.provider = lpa_protocol::ProviderFamily::openai();
                if preset.api_key_env_vars.is_empty() {
                    self.onboarding_selected_api_key = None;
                    self.onboarding_custom_model_pending = true;
                    self.onboarding_prompt =
                        Some(format!("model slug for {}", preset.display_name));
                } else {
                    self.onboarding_base_url_pending = false;
                    self.onboarding_api_key_pending = true;

                    let existing_key = self.existing_api_key_for_preset(preset.default_base_url);
                    self.onboarding_selected_api_key = existing_key.clone();

                    let env_var_hint = preset.api_key_env_vars.first();
                    let prompt = match (existing_key.is_some(), env_var_hint) {
                        (true, Some(var)) => format!(
                            "{} API key — Enter to keep saved, or paste new (env: ${var})",
                            preset.display_name
                        ),
                        (true, None) => format!(
                            "{} API key — Enter to keep saved, or paste new",
                            preset.display_name
                        ),
                        (false, Some(var)) => {
                            format!("{} API key (also read from ${var})", preset.display_name)
                        }
                        (false, None) => format!("{} API key", preset.display_name),
                    };
                    self.onboarding_prompt = Some(prompt);
                }
                self.status_message = format!("Selected {}", preset.display_name);
                self.input.clear();
            }
        }
    }

    pub(crate) fn handle_escape(&mut self) -> bool {
        if self.onboarding_api_key_pending {
            self.onboarding_api_key_pending = false;
            self.onboarding_prompt = Some("base url".to_string());
            self.input.clear();
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
            self.show_onboarding_model_panel();
            return true;
        }
        if self.is_onboarding_model_picker_open() {
            self.exit_onboarding();
            return true;
        }
        false
    }
}
