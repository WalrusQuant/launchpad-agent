use super::super::*;
use crate::onboarding::{save_onboarding_config, save_thinking_selection};
use lpa_core::ModelCatalog;

impl TuiApp {
    pub(crate) fn try_accept_aux_panel_selection(&mut self) -> bool {
        let Some(panel) = self.aux_panel.as_ref() else {
            return false;
        };
        if !self.input.is_blank() {
            return false;
        }

        match &panel.content {
            AuxPanelContent::SessionList(sessions) => {
                if sessions.is_empty() {
                    return false;
                }
                let selected =
                    sessions[self.aux_panel_selection.min(sessions.len() - 1)].session_id;
                if let Err(error) = self.worker.switch_session(selected) {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Switch failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to switch session".to_string();
                } else {
                    self.status_message = format!("Switching to session {selected}");
                }
                true
            }
            AuxPanelContent::ModelList(models) => {
                if models.is_empty() {
                    return false;
                }
                let selected = models[self.aux_panel_selection.min(models.len() - 1)].clone();
                if selected.is_custom_mode {
                    if self.show_model_onboarding {
                        self.begin_custom_model_onboarding();
                    } else {
                        self.start_onboarding();
                    }
                    return true;
                }
                if self.show_model_onboarding && self.saved_model_entry(&selected.slug).is_none() {
                    self.begin_model_credentials_onboarding(selected.slug.clone());
                    return true;
                }
                let Some(saved_model) = self.saved_model_entry(&selected.slug).cloned() else {
                    if let Err(error) = self.set_model(selected.slug.clone()) {
                        self.push_item(
                            TranscriptItemKind::Error,
                            "Model switch failed",
                            error.to_string(),
                        );
                        self.status_message = "Failed to switch model".to_string();
                    } else {
                        self.status_message = format!("Model set to {}", self.model);
                    }
                    self.aux_panel = None;
                    self.aux_panel_selection = 0;
                    return true;
                };
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.onboarding_custom_model_pending = false;
                self.onboarding_selected_model_is_custom = false;
                self.onboarding_base_url_pending = false;
                self.onboarding_api_key_pending = false;
                self.onboarding_selected_model = None;
                self.onboarding_selected_base_url = None;
                self.onboarding_selected_api_key = None;
                self.onboarding_prompt = None;
                if let Err(error) = self.reconfigure_saved_model(
                    saved_model.wire_api,
                    saved_model.model.clone(),
                    saved_model.base_url.clone(),
                    saved_model.api_key.clone(),
                ) {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Model switch failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to switch model".to_string();
                } else {
                    self.status_message = format!("Model set to {}", self.model);
                }
                true
            }
            AuxPanelContent::PresetList(presets) => {
                if presets.is_empty() {
                    return false;
                }
                let selected = presets[self.aux_panel_selection.min(presets.len() - 1)].clone();
                self.handle_preset_selected(&selected.id);
                true
            }
            AuxPanelContent::ThinkingList(thinking) => {
                if thinking.is_empty() {
                    return false;
                }
                let selected = thinking[self.aux_panel_selection.min(thinking.len() - 1)].clone();
                self.thinking_selection = Some(selected.value.clone());
                if let Err(error) = self.worker.set_thinking(self.thinking_selection.clone()) {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Thinking update failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to update thinking mode".to_string();
                } else if let Err(error) =
                    save_thinking_selection(self.thinking_selection.as_deref())
                {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Thinking update failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to persist thinking mode".to_string();
                } else {
                    self.status_message = format!("Thinking set to {}", selected.label);
                }
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                true
            }
            AuxPanelContent::Text(_) => false,
        }
    }

    pub(crate) fn thinking_entries(&self) -> Vec<ThinkingListEntry> {
        let Some(model) = self.model_catalog.get(&self.model) else {
            return Vec::new();
        };
        let capability = model.effective_thinking_capability();
        let options = capability.options();
        let current = self
            .thinking_selection
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_else(|| model.default_thinking_selection().unwrap_or_default());

        options
            .into_iter()
            .map(|option| ThinkingListEntry {
                is_current: option.value == current || option.label.to_lowercase() == current,
                label: option.label,
                description: option.description,
                value: option.value,
            })
            .collect()
    }

    pub(crate) fn finish_onboarding_selection(&mut self) -> Result<()> {
        let Some(model) = self.onboarding_selected_model.take() else {
            return Ok(());
        };
        let base_url = self.onboarding_selected_base_url.take();
        let api_key = self.onboarding_selected_api_key.take();
        let provider = self.onboarding_provider_for_model(&model);
        let wire_api = lpa_core::ProviderWireApi::default_for_provider(&provider);
        self.validate_model_provider_selection(provider, &model)?;

        save_onboarding_config(provider, &model, base_url.as_deref(), api_key.as_deref())?;
        self.worker.reconfigure_provider(
            wire_api,
            model.clone(),
            base_url.clone(),
            api_key.clone(),
        )?;
        self.provider = provider;
        self.model = model.clone();

        let entry = crate::events::SavedModelEntry {
            model: model.clone(),
            provider,
            wire_api,
            base_url: base_url.clone(),
            api_key: api_key.clone(),
        };
        if let Some(existing) = self
            .saved_models
            .iter_mut()
            .find(|m| m.model == entry.model)
        {
            *existing = entry;
        } else {
            self.saved_models.push(entry);
        }

        self.aux_panel = None;
        self.aux_panel_selection = 0;
        self.onboarding_custom_model_pending = false;
        self.onboarding_selected_model_is_custom = false;
        self.onboarding_preset_id = None;
        self.onboarding_prompt = None;
        self.onboarding_prompt_history.clear();
        self.onboarding_base_url_pending = false;
        self.onboarding_api_key_pending = false;
        self.status_message = format!("Configuration saved. Model set to {model}");
        if self.show_model_onboarding && !self.onboarding_announced {
            self.push_item(
                TranscriptItemKind::System,
                "Configure",
                "Configuration saved. Run `/configure` any time to change provider or model.",
            );
            self.onboarding_announced = true;
            self.show_model_onboarding = false;
        }
        Ok(())
    }
}
