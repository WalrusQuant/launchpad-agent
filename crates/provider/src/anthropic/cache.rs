//! Prompt-caching primitives for the Anthropic provider.
//!
//! These types and helpers are the self-contained half of caching: the
//! `cache_control` marker, the dual-form `system` field, and usage-token
//! normalization. The breakpoint-placement walker that mutates the request
//! message AST lives next to those request types in [`super::messages`].

use serde::Serialize;
use serde_json::Value;

/// Marks a request element as a prompt-cache breakpoint. Everything up to and
/// including the marked element forms a cacheable prefix on the Anthropic side.
#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
}

impl CacheControl {
    pub(super) fn ephemeral() -> Self {
        Self { kind: "ephemeral" }
    }
}

/// The `system` field accepts either a plain string (no caching) or an array of
/// text blocks that can each carry a `cache_control` breakpoint. The untagged
/// representation keeps the string form byte-identical to the pre-caching output.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum AnthropicSystem {
    Text(String),
    Blocks(Vec<AnthropicSystemBlock>),
}

#[derive(Debug, Serialize)]
pub(super) struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

/// Builds the `system` field. With caching off (or no system prompt) this is the
/// plain-string form, byte-identical to the pre-caching output. With caching on
/// it becomes a single text block carrying a `cache_control` breakpoint, which
/// caches the whole static prefix (tools precede system in the cache order).
pub(super) fn build_system(system: Option<&str>, cache: bool) -> Option<AnthropicSystem> {
    let system = system?;
    if cache {
        Some(AnthropicSystem::Blocks(vec![AnthropicSystemBlock {
            kind: "text",
            text: system.to_string(),
            cache_control: Some(CacheControl::ephemeral()),
        }]))
    } else {
        Some(AnthropicSystem::Text(system.to_string()))
    }
}

/// Updates the streaming cache-token accumulators from a raw SSE `usage` object,
/// leaving them untouched when a field is absent so a later delta that omits the
/// cache counts does not clobber the values reported at `message_start`.
pub(super) fn read_stream_cache_usage(
    usage: &Value,
    cache_creation: &mut Option<usize>,
    cache_read: &mut Option<usize>,
) {
    if let Some(value) = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
    {
        *cache_creation = Some(value as usize);
    }
    if let Some(value) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
        *cache_read = Some(value as usize);
    }
}

/// Normalizes Anthropic's `input_tokens` (which reports only the *uncached*
/// remainder) to the full prompt size by adding the cache-creation and
/// cache-read counts. This matches the OpenAI/Gemini convention where the input
/// token count already includes cached tokens, so downstream budget and
/// compaction logic (`TokenBudget::should_compact`) stays provider-agnostic. The
/// cache fields are still surfaced separately as informational subsets.
pub(super) fn prompt_input_tokens(
    input: usize,
    cache_creation: Option<usize>,
    cache_read: Option<usize>,
) -> usize {
    input + cache_creation.unwrap_or(0) + cache_read.unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn build_system_off_path_is_plain_string() {
        let system = build_system(Some("hi"), false).expect("some system");
        assert_eq!(serde_json::to_value(&system).unwrap(), json!("hi"));
    }

    #[test]
    fn build_system_cached_emits_block_with_breakpoint() {
        let system = build_system(Some("hi"), true).expect("some system");
        let value = serde_json::to_value(&system).unwrap();
        assert_eq!(value[0]["type"], json!("text"));
        assert_eq!(value[0]["text"], json!("hi"));
        assert_eq!(value[0]["cache_control"]["type"], json!("ephemeral"));
    }

    #[test]
    fn read_stream_cache_usage_updates_only_present_fields() {
        let mut creation = None;
        let mut read = None;
        read_stream_cache_usage(
            &json!({"cache_creation_input_tokens": 100, "cache_read_input_tokens": 40}),
            &mut creation,
            &mut read,
        );
        assert_eq!(creation, Some(100));
        assert_eq!(read, Some(40));
        // A later delta that omits the cache counts must not clobber them.
        read_stream_cache_usage(&json!({"output_tokens": 12}), &mut creation, &mut read);
        assert_eq!(creation, Some(100));
        assert_eq!(read, Some(40));
    }

    #[test]
    fn prompt_input_tokens_sums_uncached_and_cache_counts() {
        assert_eq!(prompt_input_tokens(11, Some(3), Some(5)), 19);
        assert_eq!(prompt_input_tokens(11, None, None), 11);
    }
}
