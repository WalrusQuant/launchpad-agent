use super::super::*;
use crate::onboarding::save_last_used_model;
use lpa_core::ModelCatalog;

impl TuiApp {
    pub(crate) fn configure_preset_entries(&self) -> Vec<crate::app::PresetListEntry> {
        let current = self.active_preset_id();
        lpa_core::all_presets()
            .iter()
            .map(|preset| crate::app::PresetListEntry {
                id: preset.id.to_string(),
                display_name: preset.display_name.to_string(),
                description: preset.description.to_string(),
                default_base_url: preset.default_base_url.map(str::to_string),
                is_current: current.as_deref() == Some(preset.id),
            })
            .collect()
    }

    /// Best-effort lookup of the preset id that matches the active provider.
    ///
    /// Matches the saved base URL against every preset so OpenAI-compatible
    /// aggregators (OpenRouter, Groq, Together, Mistral, Ollama) don't all
    /// collapse into the generic "OpenAI" label.
    pub(crate) fn active_preset_id(&self) -> Option<String> {
        use lpa_protocol::ProviderFamily;

        if let Some(saved) = self.saved_model_entry(&self.model)
            && let Some(base_url) = saved.base_url.as_deref()
            && let Some(preset) = lpa_core::all_presets().iter().find(|p| {
                p.default_base_url
                    .is_some_and(|default| base_url.starts_with(default))
            })
        {
            return Some(preset.id.to_string());
        }

        match self.provider {
            ProviderFamily::Anthropic { .. } => Some("anthropic".to_string()),
            ProviderFamily::Openai { .. } => {
                let saved_has_custom_url = self
                    .saved_model_entry(&self.model)
                    .and_then(|s| s.base_url.as_deref())
                    .is_some();
                if saved_has_custom_url {
                    Some("custom".to_string())
                } else {
                    Some("openai".to_string())
                }
            }
            ProviderFamily::Google { .. } => Some("google".to_string()),
        }
    }

    pub(crate) fn show_configure_preset_panel(&mut self) {
        let entries = self.configure_preset_entries();
        self.aux_panel_selection = entries
            .iter()
            .position(|entry| entry.is_current)
            .unwrap_or(0);
        self.aux_panel = Some(AuxPanel {
            title: "Pick a provider".to_string(),
            content: AuxPanelContent::PresetList(entries),
        });
    }

    pub(crate) fn model_switch_entries(&self) -> Vec<ModelListEntry> {
        let mut entries = self
            .saved_models
            .iter()
            .map(|model| ModelListEntry {
                slug: model.model.clone(),
                display_name: model.model.clone(),
                provider: model.provider,
                description: model
                    .base_url
                    .as_ref()
                    .map(|base_url| format!("saved model from {base_url}")),
                is_current: model.model == self.model,
                is_builtin: false,
                is_custom_mode: false,
            })
            .collect::<Vec<_>>();

        if entries.is_empty() {
            entries.extend(
                self.model_catalog
                    .list_visible()
                    .iter()
                    .map(|model| ModelListEntry {
                        slug: model.slug.clone(),
                        display_name: model.display_name.clone(),
                        provider: model.provider_family(),
                        description: model.description.clone(),
                        is_current: model.slug == self.model,
                        is_builtin: true,
                        is_custom_mode: false,
                    }),
            );
        }

        if entries.is_empty() {
            entries.push(ModelListEntry {
                slug: self.model.clone(),
                display_name: self.model.clone(),
                provider: self.provider,
                description: Some("current model".to_string()),
                is_current: true,
                is_builtin: false,
                is_custom_mode: false,
            });
        }

        if !entries.iter().any(|entry| entry.is_current) {
            entries.insert(
                0,
                ModelListEntry {
                    slug: self.model.clone(),
                    display_name: self.model.clone(),
                    provider: self.provider,
                    description: Some("current model".to_string()),
                    is_current: true,
                    is_builtin: false,
                    is_custom_mode: false,
                },
            );
        }

        entries.push(ModelListEntry {
            slug: "__add_model__".to_string(),
            display_name: "Add model".to_string(),
            provider: self.provider,
            description: Some("Open onboarding to add another model".to_string()),
            is_current: false,
            is_builtin: false,
            is_custom_mode: true,
        });
        entries
    }

    pub(crate) fn onboarding_model_picker_entries(&self) -> Vec<ModelListEntry> {
        let mut entries = Vec::new();

        for model in self.model_catalog.list_visible() {
            entries.push(ModelListEntry {
                slug: model.slug.clone(),
                display_name: model.display_name.clone(),
                provider: model.provider_family(),
                description: model.description.clone(),
                is_current: model.slug == self.model,
                is_builtin: true,
                is_custom_mode: false,
            });
        }

        if !self.show_model_onboarding && !entries.iter().any(|entry| entry.slug == self.model) {
            entries.insert(
                0,
                ModelListEntry {
                    slug: self.model.clone(),
                    display_name: self.model.clone(),
                    provider: self.provider,
                    description: Some("current model".to_string()),
                    is_current: true,
                    is_builtin: false,
                    is_custom_mode: false,
                },
            );
        }

        if self.show_model_onboarding {
            entries.push(ModelListEntry {
                slug: "__custom__".to_string(),
                display_name: "Custom model".to_string(),
                provider: self.provider,
                description: Some("enter a model name manually".to_string()),
                is_current: false,
                is_builtin: false,
                is_custom_mode: true,
            });
        }

        if entries.is_empty() {
            entries.push(ModelListEntry {
                slug: self.model.clone(),
                display_name: self.model.clone(),
                provider: self.provider,
                description: Some("current model".to_string()),
                is_current: true,
                is_builtin: false,
                is_custom_mode: false,
            });
        }

        entries
    }

    pub(crate) fn set_model(&mut self, model: String) -> Result<()> {
        self.validate_model_provider_selection(self.provider, &model)?;
        self.worker.set_model(model.clone())?;
        save_last_used_model(None, self.provider, &model)?;
        self.model = model;
        Ok(())
    }

    pub(crate) fn reconfigure_saved_model(
        &mut self,
        wire_api: lpa_core::ProviderWireApi,
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
    ) -> Result<()> {
        let provider = wire_api.provider_family();
        self.validate_model_provider_selection(provider, &model)?;
        if base_url.is_none() && api_key.is_none() {
            self.worker.set_model(model.clone())?;
            save_last_used_model(Some(wire_api), provider, &model)?;
            self.provider = provider;
            self.model = model;
            Ok(())
        } else {
            self.worker
                .reconfigure_provider(wire_api, model.clone(), base_url, api_key)?;
            save_last_used_model(Some(wire_api), provider, &model)?;
            self.provider = provider;
            self.model = model;
            Ok(())
        }
    }

    pub(crate) fn validate_model_provider_selection(
        &self,
        provider: ProviderFamily,
        model: &str,
    ) -> Result<()> {
        if let Some(catalog_model) = self.model_catalog.get(model)
            && catalog_model.provider_family() != provider
        {
            anyhow::bail!(
                "model `{model}` requires provider `{}`, but the active wire_api resolves to `{}`",
                catalog_model.provider_family(),
                provider
            );
        }
        Ok(())
    }

    pub(crate) fn saved_model_entry(&self, model: &str) -> Option<&SavedModelEntry> {
        self.saved_models.iter().find(|entry| entry.model == model)
    }

    pub(crate) fn onboarding_provider_for_model(
        &self,
        model: &str,
    ) -> lpa_protocol::ProviderFamily {
        if let Some(entry) = self.saved_model_entry(model) {
            return entry.provider;
        }
        if let Some(entry) = self.model_catalog.get(model) {
            return entry.provider_family();
        }
        self.provider
    }
}
