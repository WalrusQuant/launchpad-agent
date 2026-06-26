//! Curated catalog of provider presets used by the TUI `/configure` flow.
//!
//! Each preset bundles the non-negotiable knobs needed to connect to one
//! provider family: the wire API, the default base URL (if any), and the
//! environment variable names we'll look for as a fallback. Each preset also
//! ships a curated `models` list so the picker can offer real, selectable model
//! slugs instead of asking the user to type one blind. A `slug_hint` gives a
//! format example for the always-available "Custom model…" escape hatch.

use crate::ProviderWireApi;

mod catalog;

/// One selectable model bundled with a provider preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetModel {
    /// The exact slug sent to the provider on the wire.
    pub slug: &'static str,
    /// Human-readable label shown in the model picker.
    pub display_name: &'static str,
    /// Short one-line description rendered beneath the label.
    pub description: &'static str,
}

/// One curated provider entry shown in the `/configure` provider picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    /// Stable identifier used as the key in `model_providers` config.
    pub id: &'static str,
    /// Human-readable label shown in the picker.
    pub display_name: &'static str,
    /// Which wire API this preset speaks.
    pub wire_api: ProviderWireApi,
    /// Default base URL. `None` means "user must supply their own".
    pub default_base_url: Option<&'static str>,
    /// Environment variables we'll check as fallback API-key sources.
    pub api_key_env_vars: &'static [&'static str],
    /// Short help line shown under the display name.
    pub description: &'static str,
    /// Whether this preset represents the "custom/BYO endpoint" sentinel.
    pub is_custom: bool,
    /// Suggested default model slug, pre-selected when the user picks this
    /// preset in onboarding. `None` for `custom` (the user must type one).
    pub default_model: Option<&'static str>,
    /// Curated, selectable models for this provider. May be empty for
    /// `custom`, where the user always types a slug.
    pub models: &'static [PresetModel],
    /// Format example shown when the user opts to type a custom slug, e.g.
    /// `vendor/model — anthropic/claude-3.5-sonnet`.
    pub slug_hint: &'static str,
}

impl ProviderPreset {
    /// Returns the curated default model, falling back to the first entry in
    /// the curated `models` list when no explicit default is declared.
    pub fn preferred_model(&self) -> Option<&'static str> {
        self.default_model
            .or_else(|| self.models.first().map(|model| model.slug))
    }
}

/// Returns every preset in display order.
///
/// First-party providers come first, OpenAI-compatible aggregators follow,
/// local runtimes after that, and `Custom` sits at the bottom as an escape
/// hatch.
pub fn all_presets() -> &'static [ProviderPreset] {
    catalog::PRESETS
}

/// Looks up a preset by its stable id.
pub fn preset_by_id(id: &str) -> Option<&'static ProviderPreset> {
    all_presets().iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn catalog_contains_all_expected_presets() {
        let ids: Vec<_> = all_presets().iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            vec![
                "anthropic",
                "openai",
                "google",
                "openrouter",
                "groq",
                "together",
                "mistral",
                "deepseek",
                "xai",
                "fireworks",
                "cerebras",
                "perplexity",
                "moonshot",
                "deepinfra",
                "nebius",
                "hyperbolic",
                "novita",
                "sambanova",
                "lambda",
                "nvidia",
                "github",
                "zai_coding",
                "ollama",
                "lmstudio",
                "custom",
            ]
        );
    }

    #[test]
    fn every_non_custom_preset_declares_a_default_model() {
        for preset in all_presets() {
            if preset.is_custom {
                continue;
            }
            assert!(
                preset.default_model.is_some(),
                "preset {} should declare a default_model",
                preset.id
            );
        }
    }

    #[test]
    fn every_non_custom_preset_offers_a_selectable_model_list() {
        for preset in all_presets() {
            if preset.is_custom {
                continue;
            }
            assert!(
                !preset.models.is_empty(),
                "preset {} should offer a curated model list",
                preset.id
            );
        }
    }

    #[test]
    fn every_preset_provides_a_slug_hint() {
        for preset in all_presets() {
            assert!(
                !preset.slug_hint.trim().is_empty(),
                "preset {} should provide a slug hint",
                preset.id
            );
        }
    }

    #[test]
    fn preferred_model_falls_back_to_first_curated_model() {
        let preset = preset_by_id("deepseek").expect("deepseek preset");
        assert_eq!(preset.preferred_model(), Some("deepseek-chat"));
    }

    #[test]
    fn zai_coding_preset_targets_glm_on_zai_endpoint() {
        let preset = preset_by_id("zai_coding").expect("zai_coding preset");
        assert_eq!(
            preset.default_base_url,
            Some("https://api.z.ai/api/coding/paas/v4")
        );
        assert_eq!(preset.default_model, Some("glm-5.2"));
        assert!(preset.api_key_env_vars.contains(&"Z_AI_API_KEY"));
        assert!(!preset.is_custom);
    }

    #[test]
    fn custom_preset_has_no_default_model() {
        let preset = preset_by_id("custom").expect("custom preset");
        assert!(preset.default_model.is_none());
        assert!(preset.models.is_empty());
    }

    #[test]
    fn openrouter_preset_has_expected_defaults() {
        let preset = preset_by_id("openrouter").expect("openrouter preset");
        assert_eq!(
            preset.default_base_url,
            Some("https://openrouter.ai/api/v1")
        );
        assert_eq!(preset.wire_api, ProviderWireApi::OpenAIChatCompletions);
        assert!(preset.api_key_env_vars.contains(&"OPENROUTER_API_KEY"));
        assert!(!preset.is_custom);
    }

    #[test]
    fn custom_preset_has_no_default_base_url() {
        let preset = preset_by_id("custom").expect("custom preset");
        assert!(preset.default_base_url.is_none());
        assert!(preset.is_custom);
    }

    #[test]
    fn ollama_preset_has_no_api_key_env_vars() {
        let preset = preset_by_id("ollama").expect("ollama preset");
        assert!(preset.api_key_env_vars.is_empty());
        assert!(preset.default_base_url.unwrap().starts_with("http://"));
    }

    #[test]
    fn newly_added_providers_are_present() {
        for id in ["deepseek", "xai", "fireworks", "cerebras", "perplexity"] {
            assert!(preset_by_id(id).is_some(), "expected preset {id}");
        }
    }

    #[test]
    fn preset_by_id_returns_none_for_unknown() {
        assert!(preset_by_id("does-not-exist").is_none());
    }
}
