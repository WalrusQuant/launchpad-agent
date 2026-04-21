use lpa_server::{SessionHistoryItem, SessionHistoryItemKind};

use crate::events::{TranscriptItem, TranscriptItemKind};

pub(super) fn project_history_items(items: &[SessionHistoryItem]) -> Vec<TranscriptItem> {
    let mut transcript = Vec::new();
    let mut index = 0usize;

    while index < items.len() {
        let item = &items[index];
        if item.kind == SessionHistoryItemKind::ToolCall
            && let Some(next) = items.get(index + 1)
            && matches!(
                next.kind,
                SessionHistoryItemKind::ToolResult | SessionHistoryItemKind::Error
            )
        {
            let merged = if next.kind == SessionHistoryItemKind::Error {
                TranscriptItem::tool_error(item.title.clone(), next.body.clone())
            } else {
                TranscriptItem::restored_tool_result(item.title.clone(), next.body.clone())
            };
            transcript.push(merged);
            index += 2;
            continue;
        }

        let kind = match item.kind {
            SessionHistoryItemKind::User => TranscriptItemKind::User,
            SessionHistoryItemKind::Assistant => TranscriptItemKind::Assistant,
            SessionHistoryItemKind::ToolCall => TranscriptItemKind::ToolCall,
            SessionHistoryItemKind::ToolResult => TranscriptItemKind::ToolResult,
            SessionHistoryItemKind::Error => TranscriptItemKind::Error,
        };
        let transcript_item = match item.kind {
            SessionHistoryItemKind::ToolCall => TranscriptItem::tool_call(item.title.clone()),
            SessionHistoryItemKind::ToolResult => {
                TranscriptItem::restored_tool_result(item.title.clone(), item.body.clone())
            }
            SessionHistoryItemKind::Error => {
                TranscriptItem::tool_error(item.title.clone(), item.body.clone())
            }
            SessionHistoryItemKind::User | SessionHistoryItemKind::Assistant => {
                TranscriptItem::new(kind, item.title.clone(), item.body.clone())
            }
        };
        transcript.push(transcript_item);
        index += 1;
    }

    transcript
}
