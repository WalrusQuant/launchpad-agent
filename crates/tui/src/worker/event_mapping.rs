use tokio::sync::mpsc;

use lpa_server::{ItemEnvelope, ItemEventPayload, ItemKind};

use crate::events::WorkerEvent;

use super::tool_render::{render_json_preview, render_json_value_text, summarize_tool_call};

pub(super) fn completed_agent_message_text(payload: &ItemEventPayload) -> Option<String> {
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

pub(super) fn handle_completed_item(
    payload: ItemEventPayload,
    event_tx: &mpsc::UnboundedSender<WorkerEvent>,
) {
    match payload.item {
        ItemEnvelope {
            item_kind: ItemKind::AgentMessage,
            payload,
            ..
        } => {
            let text = payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned);
            if let Some(text) = text {
                let _ = event_tx.send(WorkerEvent::AssistantMessageCompleted(text));
            }
        }
        ItemEnvelope {
            item_kind: ItemKind::Reasoning,
            payload,
            ..
        } => {
            let text = payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned);
            if let Some(text) = text {
                let _ = event_tx.send(WorkerEvent::ReasoningCompleted(text));
            }
        }
        ItemEnvelope {
            item_kind: ItemKind::ToolCall,
            payload,
            ..
        } => {
            let tool_use_id = payload
                .get("tool_use_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let summary = summarize_tool_call(&payload);
            let detail = payload
                .get("input")
                .map(render_json_preview)
                .filter(|detail| !detail.is_empty());
            let _ = event_tx.send(WorkerEvent::ToolCall {
                tool_use_id,
                summary,
                detail,
            });
        }
        ItemEnvelope {
            item_kind: ItemKind::ToolResult,
            payload,
            ..
        } => {
            let content = payload
                .get("content")
                .map(render_json_value_text)
                .unwrap_or_default();
            let is_error = payload
                .get("is_error")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let tool_use_id = payload
                .get("tool_use_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let _ = event_tx.send(WorkerEvent::ToolResult {
                tool_use_id,
                preview: content,
                is_error,
                truncated: false,
            });
        }
        _ => {}
    }
}
