//! Extraction of human-facing text from server item events.

use lpa_protocol::{ItemEnvelope, ItemEventPayload, ItemKind};

/// Returns the trimmed text of a completed agent-message item, or `None` for
/// any other item kind or an empty message. Used by the headless driver to
/// capture the final assistant reply (`turn/completed` carries only status, so
/// the text must come from the item stream).
pub fn completed_agent_message_text(payload: &ItemEventPayload) -> Option<String> {
    match &payload.item {
        ItemEnvelope {
            item_kind: ItemKind::AgentMessage,
            payload,
            ..
        } => payload
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}
