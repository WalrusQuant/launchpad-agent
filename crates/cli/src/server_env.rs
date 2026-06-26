//! Shared builder for the env overrides passed to a spawned `lpagent server`
//! subprocess. Both the interactive TUI (`agent.rs`) and the headless driver
//! (`headless.rs`) spawn a dedicated, single-tenant server and hand it the
//! resolved provider settings through these env vars.

use lpa_core::{ProviderWireApi, ResolvedProviderSettings};

/// Builds the provider env overrides (`LPA_PROVIDER`, `LPA_WIRE_API`,
/// `LPA_MODEL`, and optionally `LPA_BASE_URL` / `LPA_API_KEY`) consumed by the
/// server subprocess during config resolution.
pub fn server_env_overrides(resolved: &ResolvedProviderSettings) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "LPA_PROVIDER".to_string(),
            resolved.provider.as_str().to_string(),
        ),
        (
            "LPA_WIRE_API".to_string(),
            match resolved.wire_api {
                ProviderWireApi::OpenAIChatCompletions => "openai_chat_completions".to_string(),
                ProviderWireApi::OpenAIResponses => "openai_responses".to_string(),
                ProviderWireApi::AnthropicMessages => "anthropic_messages".to_string(),
                ProviderWireApi::GoogleGenerateContent => "google_generate_content".to_string(),
            },
        ),
        ("LPA_MODEL".to_string(), resolved.model.clone()),
    ];
    if let Some(base_url) = &resolved.base_url {
        env.push(("LPA_BASE_URL".to_string(), base_url.clone()));
    }
    if let Some(api_key) = &resolved.api_key {
        env.push(("LPA_API_KEY".to_string(), api_key.clone()));
    }
    env
}
