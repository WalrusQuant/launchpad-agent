//! Structured output for headless runs: `--output-format text|json|stream-json`.
//!
//! `text` (default) prints only the final assistant message. `json` prints a
//! single result object when the turn finishes. `stream-json` emits one NDJSON
//! line per externally-visible event as it happens, then the same final result
//! object. The result object mirrors Claude Code's headless `result` shape so
//! existing tooling can consume it.

use lpa_protocol::{ItemDeltaKind, ItemKind, ServerEvent, SessionId, TurnUsage};
use serde_json::{Value, json};

/// Machine-output format for a headless run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Print only the final assistant message (default).
    #[default]
    Text,
    /// Print a single JSON result object when the turn finishes.
    Json,
    /// Stream newline-delimited JSON events, then a final result object.
    #[value(name = "stream-json")]
    StreamJson,
}

impl OutputFormat {
    /// Whether per-event NDJSON lines should be emitted during the turn.
    pub fn is_stream(self) -> bool {
        matches!(self, OutputFormat::StreamJson)
    }
}

/// The result of a headless turn, rendered per [`OutputFormat`].
pub struct TurnOutcome {
    pub session_id: SessionId,
    pub final_message: Option<String>,
    pub usage: Option<TurnUsage>,
    /// `Some` when the turn failed; the string is the failure detail.
    pub error: Option<String>,
}

/// Emits one NDJSON line for a streamed event under `stream-json`. Only
/// assistant text deltas and tool-call/result items are surfaced; other events
/// (reasoning, usage, lifecycle) are omitted to keep the stream focused on the
/// model's externally-visible actions. The terminal result object is emitted
/// separately by [`render_result`].
pub fn emit_stream_event(event: &ServerEvent) {
    let line = match event {
        ServerEvent::ItemDelta {
            delta_kind: ItemDeltaKind::AgentMessageDelta,
            payload,
        } => json!({ "type": "assistant", "text": payload.delta }),
        ServerEvent::ItemCompleted(payload) => match payload.item.item_kind {
            ItemKind::ToolCall => json!({ "type": "tool_use", "tool": payload.item.payload }),
            ItemKind::ToolResult => {
                json!({ "type": "tool_result", "result": payload.item.payload })
            }
            _ => return,
        },
        _ => return,
    };
    emit_line(&line);
}

/// Renders the terminal output for the turn and returns the process exit code
/// (0 success, 1 failure). `text` prints the message to stdout (errors to
/// stderr, matching the legacy contract); `json` / `stream-json` print a single
/// result object to stdout in all cases.
pub fn render_result(format: OutputFormat, outcome: &TurnOutcome, duration_ms: u128) -> i32 {
    match format {
        OutputFormat::Text => render_text(outcome),
        OutputFormat::Json | OutputFormat::StreamJson => {
            emit_line(&result_object(outcome, duration_ms));
            i32::from(outcome.error.is_some())
        }
    }
}

fn render_text(outcome: &TurnOutcome) -> i32 {
    if let Some(error) = &outcome.error {
        eprintln!("error: prompt failed: {error}");
        return 1;
    }
    match &outcome.final_message {
        Some(text) => println!("{text}"),
        None => eprintln!("lpagent [prompt] empty response"),
    }
    0
}

/// Builds the terminal `result` object common to `json` and `stream-json`.
pub fn result_object(outcome: &TurnOutcome, duration_ms: u128) -> Value {
    let (subtype, is_error, result) = match &outcome.error {
        Some(error) => ("error_during_execution", true, error.clone()),
        None => (
            "success",
            false,
            outcome.final_message.clone().unwrap_or_default(),
        ),
    };
    json!({
        "type": "result",
        "subtype": subtype,
        "is_error": is_error,
        "result": result,
        "session_id": outcome.session_id.to_string(),
        "duration_ms": duration_ms,
        "num_turns": 1,
        "usage": usage_object(outcome.usage.as_ref()),
    })
}

fn usage_object(usage: Option<&TurnUsage>) -> Value {
    let Some(usage) = usage else {
        return Value::Null;
    };
    let mut map = serde_json::Map::new();
    map.insert("input_tokens".into(), json!(usage.input_tokens));
    map.insert("output_tokens".into(), json!(usage.output_tokens));
    if let Some(tokens) = usage.cache_creation_input_tokens {
        map.insert("cache_creation_input_tokens".into(), json!(tokens));
    }
    if let Some(tokens) = usage.cache_read_input_tokens {
        map.insert("cache_read_input_tokens".into(), json!(tokens));
    }
    Value::Object(map)
}

fn emit_line(value: &Value) {
    // Serializing a serde_json::Value never fails.
    println!("{}", serde_json::to_string(value).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use lpa_protocol::{SessionId, TurnUsage};
    use pretty_assertions::assert_eq;

    use super::{OutputFormat, TurnOutcome, result_object};

    fn outcome(error: Option<&str>) -> TurnOutcome {
        TurnOutcome {
            session_id: SessionId::new(),
            final_message: Some("hello world".to_string()),
            usage: Some(TurnUsage {
                input_tokens: 12,
                output_tokens: 7,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(3),
            }),
            error: error.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn result_object_success_shape() {
        let value = result_object(&outcome(None), 42);
        assert_eq!(value["type"], "result");
        assert_eq!(value["subtype"], "success");
        assert_eq!(value["is_error"], false);
        assert_eq!(value["result"], "hello world");
        assert_eq!(value["num_turns"], 1);
        assert_eq!(value["duration_ms"], 42);
        assert_eq!(value["usage"]["input_tokens"], 12);
        assert_eq!(value["usage"]["output_tokens"], 7);
        assert_eq!(value["usage"]["cache_read_input_tokens"], 3);
        // Absent cache-creation tokens are omitted, not null.
        assert!(value["usage"].get("cache_creation_input_tokens").is_none());
    }

    #[test]
    fn result_object_error_shape() {
        let mut failed = outcome(Some("provider exploded"));
        failed.final_message = None;
        let value = result_object(&failed, 5);
        assert_eq!(value["subtype"], "error_during_execution");
        assert_eq!(value["is_error"], true);
        assert_eq!(value["result"], "provider exploded");
    }

    #[test]
    fn stream_format_is_stream() {
        assert!(OutputFormat::StreamJson.is_stream());
        assert!(!OutputFormat::Json.is_stream());
        assert!(!OutputFormat::Text.is_stream());
    }
}
