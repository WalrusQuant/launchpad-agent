//! Curated catalog of provider presets used by the TUI `/configure` flow.
//!
//! Each preset bundles the non-negotiable knobs needed to connect to one
//! provider family: the wire API, the default base URL (if any), and the
//! environment variable names we'll look for as a fallback. Each preset also
//! ships a curated `models` list so the picker can offer real, selectable model
//! slugs instead of asking the user to type one blind. A `slug_hint` gives a
//! format example for the always-available "Custom model…" escape hatch.

use crate::ProviderWireApi;

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

/// Convenience constructor keeping the large catalog literal readable.
const fn model(
    slug: &'static str,
    display_name: &'static str,
    description: &'static str,
) -> PresetModel {
    PresetModel {
        slug,
        display_name,
        description,
    }
}

/// Returns every preset in display order.
///
/// First-party providers come first, OpenAI-compatible aggregators follow,
/// local runtimes after that, and `Custom` sits at the bottom as an escape
/// hatch.
pub fn all_presets() -> &'static [ProviderPreset] {
    PRESETS
}

/// The curated preset catalog. Held in a `const` so the nested per-provider
/// model arrays promote to `'static` instead of becoming freed temporaries.
const PRESETS: &[ProviderPreset] = &[
        ProviderPreset {
            id: "anthropic",
            display_name: "Anthropic (Claude)",
            wire_api: ProviderWireApi::AnthropicMessages,
            default_base_url: Some("https://api.anthropic.com"),
            api_key_env_vars: &["ANTHROPIC_API_KEY", "LPA_API_KEY"],
            description: "Claude models — Sonnet, Opus, Haiku",
            is_custom: false,
            default_model: Some("claude-opus-4-8"),
            models: &[
                model("claude-opus-4-8", "Claude Opus 4.8", "Most capable Claude model"),
                model("claude-sonnet-4-6", "Claude Sonnet 4.6", "Balanced speed and capability"),
                model(
                    "claude-haiku-4-5-20251001",
                    "Claude Haiku 4.5",
                    "Fastest, most cost-effective Claude",
                ),
                model("claude-fable-5", "Claude Fable 5", "Creative-writing focused"),
            ],
            slug_hint: "e.g. claude-sonnet-4-6",
        },
        ProviderPreset {
            id: "openai",
            display_name: "OpenAI",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.openai.com"),
            api_key_env_vars: &["OPENAI_API_KEY", "LPA_API_KEY"],
            description: "GPT models — gpt-4o, gpt-5, o-series",
            is_custom: false,
            default_model: Some("gpt-4o"),
            models: &[
                model("gpt-5", "GPT-5", "Flagship general model"),
                model("gpt-4o", "GPT-4o", "Fast multimodal flagship"),
                model("gpt-4o-mini", "GPT-4o mini", "Small, cheap, fast"),
                model("o3", "o3", "Deep reasoning model"),
                model("o4-mini", "o4-mini", "Fast reasoning model"),
            ],
            slug_hint: "e.g. gpt-4o",
        },
        ProviderPreset {
            id: "google",
            display_name: "Google Gemini",
            wire_api: ProviderWireApi::GoogleGenerateContent,
            default_base_url: Some("https://generativelanguage.googleapis.com"),
            api_key_env_vars: &["GOOGLE_API_KEY", "GEMINI_API_KEY", "LPA_API_KEY"],
            description: "Gemini models — 2.5 Pro, 2.5 Flash",
            is_custom: false,
            default_model: Some("gemini-2.5-pro"),
            models: &[
                model("gemini-2.5-pro", "Gemini 2.5 Pro", "Flagship thinking model, 1M context"),
                model("gemini-2.5-flash", "Gemini 2.5 Flash", "Fast thinking model, 1M context"),
                model("gemini-2.0-flash", "Gemini 2.0 Flash", "Fast, lightweight, 1M context"),
            ],
            slug_hint: "e.g. gemini-2.5-pro",
        },
        ProviderPreset {
            id: "openrouter",
            display_name: "OpenRouter",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://openrouter.ai/api/v1"),
            api_key_env_vars: &["OPENROUTER_API_KEY", "LPA_API_KEY"],
            description: "Unified gateway — free + paid models from many vendors",
            is_custom: false,
            default_model: Some("z-ai/glm-4.6"),
            models: &[
                model("z-ai/glm-4.6", "GLM 4.6", "Z.ai flagship via OpenRouter"),
                model("anthropic/claude-3.5-sonnet", "Claude 3.5 Sonnet", "Anthropic via OpenRouter"),
                model("openai/gpt-4o", "GPT-4o", "OpenAI via OpenRouter"),
                model("google/gemini-2.5-pro", "Gemini 2.5 Pro", "Google via OpenRouter"),
                model(
                    "meta-llama/llama-3.3-70b-instruct",
                    "Llama 3.3 70B",
                    "Meta open weights via OpenRouter",
                ),
                model("deepseek/deepseek-chat", "DeepSeek V3", "DeepSeek via OpenRouter"),
            ],
            slug_hint: "vendor/model — e.g. anthropic/claude-3.5-sonnet",
        },
        ProviderPreset {
            id: "groq",
            display_name: "Groq",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.groq.com/openai/v1"),
            api_key_env_vars: &["GROQ_API_KEY", "LPA_API_KEY"],
            description: "Very fast inference — Llama, Mixtral, Gemma, Qwen",
            is_custom: false,
            default_model: Some("llama-3.3-70b-versatile"),
            models: &[
                model("llama-3.3-70b-versatile", "Llama 3.3 70B", "Versatile flagship"),
                model("llama-3.1-8b-instant", "Llama 3.1 8B", "Instant, low-latency"),
                model(
                    "deepseek-r1-distill-llama-70b",
                    "DeepSeek R1 Distill 70B",
                    "Reasoning, distilled",
                ),
                model("qwen-2.5-32b", "Qwen 2.5 32B", "Qwen general model"),
                model("gemma2-9b-it", "Gemma 2 9B", "Google open weights"),
            ],
            slug_hint: "e.g. llama-3.3-70b-versatile",
        },
        ProviderPreset {
            id: "together",
            display_name: "Together AI",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.together.xyz/v1"),
            api_key_env_vars: &["TOGETHER_API_KEY", "LPA_API_KEY"],
            description: "Open-weight models at scale — Llama, Qwen, DeepSeek",
            is_custom: false,
            default_model: Some("meta-llama/Llama-3.3-70B-Instruct-Turbo"),
            models: &[
                model(
                    "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                    "Llama 3.3 70B Turbo",
                    "Meta open weights",
                ),
                model(
                    "Qwen/Qwen2.5-72B-Instruct-Turbo",
                    "Qwen 2.5 72B Turbo",
                    "Qwen flagship",
                ),
                model("deepseek-ai/DeepSeek-V3", "DeepSeek V3", "DeepSeek flagship"),
                model(
                    "mistralai/Mixtral-8x7B-Instruct-v0.1",
                    "Mixtral 8x7B",
                    "Mistral MoE",
                ),
            ],
            slug_hint: "vendor/Model — e.g. Qwen/Qwen2.5-72B-Instruct-Turbo",
        },
        ProviderPreset {
            id: "mistral",
            display_name: "Mistral",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.mistral.ai/v1"),
            api_key_env_vars: &["MISTRAL_API_KEY", "LPA_API_KEY"],
            description: "Mistral models — Large, Small, Codestral",
            is_custom: false,
            default_model: Some("mistral-large-latest"),
            models: &[
                model("mistral-large-latest", "Mistral Large", "Flagship reasoning model"),
                model("mistral-small-latest", "Mistral Small", "Fast, cost-effective"),
                model("codestral-latest", "Codestral", "Code-specialized model"),
                model("ministral-8b-latest", "Ministral 8B", "Edge-sized model"),
            ],
            slug_hint: "e.g. mistral-large-latest",
        },
        ProviderPreset {
            id: "deepseek",
            display_name: "DeepSeek",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.deepseek.com"),
            api_key_env_vars: &["DEEPSEEK_API_KEY", "LPA_API_KEY"],
            description: "DeepSeek models — chat (V3) and reasoner (R1)",
            is_custom: false,
            default_model: Some("deepseek-chat"),
            models: &[
                model("deepseek-chat", "DeepSeek Chat (V3)", "Flagship general model"),
                model("deepseek-reasoner", "DeepSeek Reasoner (R1)", "Reasoning model"),
            ],
            slug_hint: "e.g. deepseek-chat",
        },
        ProviderPreset {
            id: "xai",
            display_name: "xAI (Grok)",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.x.ai/v1"),
            api_key_env_vars: &["XAI_API_KEY", "LPA_API_KEY"],
            description: "Grok models from xAI",
            is_custom: false,
            default_model: Some("grok-4"),
            models: &[
                model("grok-4", "Grok 4", "Flagship Grok model"),
                model("grok-3", "Grok 3", "Previous flagship"),
                model("grok-3-mini", "Grok 3 mini", "Fast, cheap"),
                model("grok-2-vision", "Grok 2 Vision", "Multimodal"),
            ],
            slug_hint: "e.g. grok-4",
        },
        ProviderPreset {
            id: "fireworks",
            display_name: "Fireworks AI",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.fireworks.ai/inference/v1"),
            api_key_env_vars: &["FIREWORKS_API_KEY", "LPA_API_KEY"],
            description: "Fast open-weight inference — Llama, Qwen, DeepSeek",
            is_custom: false,
            default_model: Some("accounts/fireworks/models/llama-v3p3-70b-instruct"),
            models: &[
                model(
                    "accounts/fireworks/models/llama-v3p3-70b-instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model(
                    "accounts/fireworks/models/qwen2p5-72b-instruct",
                    "Qwen 2.5 72B",
                    "Qwen flagship",
                ),
                model(
                    "accounts/fireworks/models/deepseek-v3",
                    "DeepSeek V3",
                    "DeepSeek flagship",
                ),
            ],
            slug_hint: "accounts/fireworks/models/<name>",
        },
        ProviderPreset {
            id: "cerebras",
            display_name: "Cerebras",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.cerebras.ai/v1"),
            api_key_env_vars: &["CEREBRAS_API_KEY", "LPA_API_KEY"],
            description: "Ultra-fast inference on Cerebras hardware",
            is_custom: false,
            default_model: Some("llama-3.3-70b"),
            models: &[
                model("llama-3.3-70b", "Llama 3.3 70B", "Meta open weights"),
                model("llama3.1-8b", "Llama 3.1 8B", "Small, fast"),
                model("qwen-3-32b", "Qwen 3 32B", "Qwen general model"),
            ],
            slug_hint: "e.g. llama-3.3-70b",
        },
        ProviderPreset {
            id: "perplexity",
            display_name: "Perplexity",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.perplexity.ai"),
            api_key_env_vars: &["PERPLEXITY_API_KEY", "LPA_API_KEY"],
            description: "Sonar models with built-in web search",
            is_custom: false,
            default_model: Some("sonar-pro"),
            models: &[
                model("sonar", "Sonar", "Fast search-grounded model"),
                model("sonar-pro", "Sonar Pro", "Advanced search-grounded model"),
                model("sonar-reasoning", "Sonar Reasoning", "Reasoning + search"),
                model("sonar-reasoning-pro", "Sonar Reasoning Pro", "Advanced reasoning + search"),
            ],
            slug_hint: "e.g. sonar-pro",
        },
        ProviderPreset {
            id: "moonshot",
            display_name: "Moonshot (Kimi)",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.moonshot.ai/v1"),
            api_key_env_vars: &["MOONSHOT_API_KEY", "LPA_API_KEY"],
            description: "Kimi models from Moonshot AI",
            is_custom: false,
            default_model: Some("kimi-k2.5"),
            models: &[
                model("kimi-k2.5", "Kimi K2.5", "Multimodal agentic flagship"),
                model("kimi-k2-0711-preview", "Kimi K2 (preview)", "Agentic model preview"),
                model("moonshot-v1-128k", "Moonshot v1 128k", "Long-context model"),
            ],
            slug_hint: "e.g. kimi-k2.5",
        },
        ProviderPreset {
            id: "deepinfra",
            display_name: "DeepInfra",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.deepinfra.com/v1/openai"),
            api_key_env_vars: &["DEEPINFRA_API_KEY", "LPA_API_KEY"],
            description: "Open-weight model gateway",
            is_custom: false,
            default_model: Some("meta-llama/Llama-3.3-70B-Instruct"),
            models: &[
                model(
                    "meta-llama/Llama-3.3-70B-Instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("Qwen/Qwen2.5-72B-Instruct", "Qwen 2.5 72B", "Qwen flagship"),
                model("deepseek-ai/DeepSeek-V3", "DeepSeek V3", "DeepSeek flagship"),
            ],
            slug_hint: "vendor/Model — e.g. meta-llama/Llama-3.3-70B-Instruct",
        },
        ProviderPreset {
            id: "nebius",
            display_name: "Nebius AI Studio",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.studio.nebius.ai/v1"),
            api_key_env_vars: &["NEBIUS_API_KEY", "LPA_API_KEY"],
            description: "Open-weight model gateway",
            is_custom: false,
            default_model: Some("meta-llama/Llama-3.3-70B-Instruct"),
            models: &[
                model(
                    "meta-llama/Llama-3.3-70B-Instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("Qwen/Qwen2.5-72B-Instruct", "Qwen 2.5 72B", "Qwen flagship"),
                model("deepseek-ai/DeepSeek-V3", "DeepSeek V3", "DeepSeek flagship"),
            ],
            slug_hint: "vendor/Model — e.g. Qwen/Qwen2.5-72B-Instruct",
        },
        ProviderPreset {
            id: "hyperbolic",
            display_name: "Hyperbolic",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.hyperbolic.xyz/v1"),
            api_key_env_vars: &["HYPERBOLIC_API_KEY", "LPA_API_KEY"],
            description: "Open-weight model gateway",
            is_custom: false,
            default_model: Some("meta-llama/Llama-3.3-70B-Instruct"),
            models: &[
                model(
                    "meta-llama/Llama-3.3-70B-Instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("Qwen/Qwen2.5-72B-Instruct", "Qwen 2.5 72B", "Qwen flagship"),
                model("deepseek-ai/DeepSeek-V3", "DeepSeek V3", "DeepSeek flagship"),
            ],
            slug_hint: "vendor/Model — e.g. meta-llama/Llama-3.3-70B-Instruct",
        },
        ProviderPreset {
            id: "novita",
            display_name: "Novita AI",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.novita.ai/v3/openai"),
            api_key_env_vars: &["NOVITA_API_KEY", "LPA_API_KEY"],
            description: "Open-weight model gateway",
            is_custom: false,
            default_model: Some("meta-llama/llama-3.3-70b-instruct"),
            models: &[
                model(
                    "meta-llama/llama-3.3-70b-instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("qwen/qwen-2.5-72b-instruct", "Qwen 2.5 72B", "Qwen flagship"),
                model("deepseek/deepseek_v3", "DeepSeek V3", "DeepSeek flagship"),
            ],
            slug_hint: "vendor/model — e.g. meta-llama/llama-3.3-70b-instruct",
        },
        ProviderPreset {
            id: "sambanova",
            display_name: "SambaNova",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.sambanova.ai/v1"),
            api_key_env_vars: &["SAMBANOVA_API_KEY", "LPA_API_KEY"],
            description: "Fast inference — Llama, DeepSeek",
            is_custom: false,
            default_model: Some("Meta-Llama-3.3-70B-Instruct"),
            models: &[
                model(
                    "Meta-Llama-3.3-70B-Instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("Llama-3.1-8B-Instruct", "Llama 3.1 8B", "Small, fast"),
                model("DeepSeek-R1", "DeepSeek R1", "Reasoning model"),
            ],
            slug_hint: "e.g. Meta-Llama-3.3-70B-Instruct",
        },
        ProviderPreset {
            id: "lambda",
            display_name: "Lambda",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.lambda.ai/v1"),
            api_key_env_vars: &["LAMBDA_API_KEY", "LPA_API_KEY"],
            description: "Lambda Inference API — open weights",
            is_custom: false,
            default_model: Some("llama-3.3-70b-instruct-fp8"),
            models: &[
                model(
                    "llama-3.3-70b-instruct-fp8",
                    "Llama 3.3 70B (fp8)",
                    "Meta open weights",
                ),
                model("deepseek-r1-671b", "DeepSeek R1 671B", "Reasoning flagship"),
                model(
                    "qwen25-coder-32b-instruct",
                    "Qwen 2.5 Coder 32B",
                    "Code-specialized",
                ),
            ],
            slug_hint: "e.g. llama-3.3-70b-instruct-fp8",
        },
        ProviderPreset {
            id: "nvidia",
            display_name: "Nvidia NIM",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://integrate.api.nvidia.com/v1"),
            api_key_env_vars: &["NVIDIA_API_KEY", "LPA_API_KEY"],
            description: "Nvidia-hosted open-weight models",
            is_custom: false,
            default_model: Some("meta/llama-3.3-70b-instruct"),
            models: &[
                model(
                    "meta/llama-3.3-70b-instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model(
                    "qwen/qwen2.5-coder-32b-instruct",
                    "Qwen 2.5 Coder 32B",
                    "Code-specialized",
                ),
                model("deepseek-ai/deepseek-r1", "DeepSeek R1", "Reasoning model"),
            ],
            slug_hint: "vendor/model — e.g. meta/llama-3.3-70b-instruct",
        },
        ProviderPreset {
            id: "github",
            display_name: "GitHub Models",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://models.github.ai/inference"),
            api_key_env_vars: &["GITHUB_TOKEN", "LPA_API_KEY"],
            description: "Models hosted by GitHub — uses your GitHub token",
            is_custom: false,
            default_model: Some("openai/gpt-4o"),
            models: &[
                model("openai/gpt-4o", "GPT-4o", "OpenAI via GitHub"),
                model("openai/gpt-4o-mini", "GPT-4o mini", "Small, cheap"),
                model(
                    "meta/Llama-3.3-70B-Instruct",
                    "Llama 3.3 70B",
                    "Meta open weights",
                ),
                model("deepseek/DeepSeek-V3-0324", "DeepSeek V3", "DeepSeek flagship"),
            ],
            slug_hint: "vendor/Model — e.g. openai/gpt-4o",
        },
        ProviderPreset {
            id: "zai_coding",
            display_name: "Z.ai (coding plan)",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("https://api.z.ai/api/coding/paas/v4"),
            api_key_env_vars: &["Z_AI_API_KEY", "LPA_API_KEY"],
            description: "Z.ai coding plan — GLM family (glm-5.2 flagship)",
            is_custom: false,
            default_model: Some("glm-5.2"),
            models: &[
                model("glm-5.2", "GLM 5.2", "Flagship coding model"),
                model("glm-4.6", "GLM 4.6", "Previous flagship"),
                model("glm-4.5-air", "GLM 4.5 Air", "Lightweight, fast"),
            ],
            slug_hint: "e.g. glm-5.2",
        },
        ProviderPreset {
            id: "ollama",
            display_name: "Ollama (local)",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("http://localhost:11434/v1"),
            api_key_env_vars: &[],
            description: "Run models locally — no API key needed",
            is_custom: false,
            default_model: Some("llama3.2"),
            models: &[
                model("llama3.2", "Llama 3.2", "Meta open weights"),
                model("llama3.1", "Llama 3.1", "Meta open weights"),
                model("qwen2.5-coder", "Qwen 2.5 Coder", "Code-specialized"),
                model("deepseek-r1", "DeepSeek R1", "Reasoning model"),
                model("mistral", "Mistral", "Mistral open weights"),
            ],
            slug_hint: "name of a pulled model — e.g. llama3.2",
        },
        ProviderPreset {
            id: "lmstudio",
            display_name: "LM Studio (local)",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: Some("http://localhost:1234/v1"),
            api_key_env_vars: &[],
            description: "Run models locally via LM Studio — no API key needed",
            is_custom: false,
            default_model: Some("qwen2.5-coder-7b-instruct"),
            models: &[
                model(
                    "qwen2.5-coder-7b-instruct",
                    "Qwen 2.5 Coder 7B",
                    "Code-specialized",
                ),
                model("llama-3.2-3b-instruct", "Llama 3.2 3B", "Small, fast"),
                model("deepseek-r1-distill-qwen-7b", "DeepSeek R1 Distill 7B", "Reasoning"),
            ],
            slug_hint: "the model identifier shown in LM Studio",
        },
        ProviderPreset {
            id: "custom",
            display_name: "Custom endpoint",
            wire_api: ProviderWireApi::OpenAIChatCompletions,
            default_base_url: None,
            api_key_env_vars: &["LPA_API_KEY"],
            description: "Any OpenAI-compatible endpoint",
            is_custom: true,
            default_model: None,
            models: &[],
            slug_hint: "any model slug your endpoint accepts",
        },
];

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
