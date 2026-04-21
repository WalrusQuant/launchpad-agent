use std::time::Duration;

use anyhow::{Context, Result};

use lpa_core::{Model, ModelCatalog, PresetModelCatalog, test_model_connection};
use lpa_protocol::ProviderFamily;
use lpa_provider::{
    ModelProviderSDK, anthropic::AnthropicProvider, google::GoogleProvider, openai::OpenAIProvider,
};

pub(super) async fn validate_provider_connection(
    provider: ProviderFamily,
    model: &str,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<String> {
    let validation_model = resolve_validation_model(provider, model)?;
    let validation_provider = build_validation_provider(provider, base_url, api_key)?;
    tokio::time::timeout(
        Duration::from_secs(20),
        test_model_connection(
            validation_provider.as_ref(),
            &validation_model,
            "Reply with OK only.",
        ),
    )
    .await
    .context("provider validation timed out after 20s")?
    .map_err(Into::into)
}

fn resolve_validation_model(provider: ProviderFamily, model: &str) -> Result<Model> {
    let catalog = PresetModelCatalog::load()?;
    if let Some(entry) = catalog.get(model) {
        return Ok(entry.clone());
    }
    Ok(Model {
        slug: model.to_string(),
        provider,
        ..Model::default()
    })
}

fn build_validation_provider(
    provider: ProviderFamily,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<std::sync::Arc<dyn ModelProviderSDK>> {
    match provider {
        ProviderFamily::Anthropic { .. } => {
            let api_key = api_key.context("anthropic provider requires an API key")?;
            let base_url = base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string());
            Ok(std::sync::Arc::new(
                AnthropicProvider::new(base_url).with_api_key(api_key),
            ))
        }
        ProviderFamily::Openai { .. } => {
            let base_url = normalize_openai_base_url(
                &base_url.unwrap_or_else(|| "https://api.openai.com".to_string()),
            );
            let provider = if let Some(api_key) = api_key {
                OpenAIProvider::new(base_url).with_api_key(api_key)
            } else {
                OpenAIProvider::new(base_url)
            };
            Ok(std::sync::Arc::new(provider))
        }
        ProviderFamily::Google { .. } => {
            let api_key = api_key.context("google provider requires an API key")?;
            let base_url =
                base_url.unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
            Ok(std::sync::Arc::new(
                GoogleProvider::new(base_url).with_api_key(api_key),
            ))
        }
    }
}

fn normalize_openai_base_url(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let Some(scheme_sep) = trimmed.find("://") else {
        return trimmed.to_string();
    };
    let has_explicit_path = trimmed[scheme_sep + 3..].contains('/');
    if has_explicit_path {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}
