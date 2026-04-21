use chrono::Utc;
use pretty_assertions::assert_eq;

use lpa_core::{SessionId, SessionTitleState};
use lpa_server::{SessionRuntimeStatus, SessionSummary};

use super::history::project_history_items;
use super::tool_render::{normalize_display_output, summarize_tool_call, truncate_tool_output};
use crate::events::SessionListEntry;
use crate::events::TranscriptItem;
use lpa_server::{SessionHistoryItem, SessionHistoryItemKind};

#[test]
fn bash_tool_summary_uses_command_text() {
    let payload = serde_json::json!({
        "tool_name": "bash",
        "input": {
            "command": "Get-Date -Format \"yyyy-MM-dd\""
        }
    });

    assert_eq!(
        summarize_tool_call(&payload),
        "bash: Get-Date -Format \"yyyy-MM-dd\""
    );
}

#[test]
fn tool_output_preview_truncates_large_content() {
    let content = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        truncate_tool_output(&content),
        "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n… "
    );
}

#[test]
fn session_list_entries_keep_title_before_identifier() {
    let active_session_id = SessionId::new();
    let summary = SessionSummary {
        session_id: active_session_id,
        cwd: ".".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        title: Some("Saved conversation".to_string()),
        title_state: SessionTitleState::Provisional,
        ephemeral: false,
        resolved_model: Some("test-model".to_string()),
        total_input_tokens: 0,
        total_output_tokens: 0,
        status: SessionRuntimeStatus::Idle,
    };
    let entry = SessionListEntry {
        session_id: summary.session_id,
        title: summary.title.clone().unwrap_or_default(),
        updated_at: summary
            .updated_at
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
        is_active: true,
    };

    assert_eq!(entry.title, "Saved conversation");
    assert!(entry.updated_at.contains("UTC"));
}

#[test]
fn session_list_entries_mark_inactive_sessions() {
    let summary = SessionSummary {
        session_id: SessionId::new(),
        cwd: ".".into(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        title: Some("Saved conversation".to_string()),
        title_state: SessionTitleState::Provisional,
        ephemeral: false,
        resolved_model: Some("test-model".to_string()),
        total_input_tokens: 0,
        total_output_tokens: 0,
        status: SessionRuntimeStatus::Idle,
    };
    let entry = SessionListEntry {
        session_id: summary.session_id,
        title: summary.title.clone().unwrap_or_default(),
        updated_at: summary
            .updated_at
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
        is_active: false,
    };

    assert!(!entry.is_active);
}

#[test]
fn display_output_normalization_trims_crlf_padding() {
    assert_eq!(
        normalize_display_output("\r\n\r\nhello\r\nworld\r\n\r\n"),
        "hello\nworld"
    );
}

#[test]
fn project_history_merges_tool_call_and_result() {
    let items = vec![
        SessionHistoryItem {
            kind: SessionHistoryItemKind::ToolCall,
            title: "Ran powershell -Command \"Get-Date\"".to_string(),
            body: String::new(),
        },
        SessionHistoryItem {
            kind: SessionHistoryItemKind::ToolResult,
            title: "Tool output".to_string(),
            body: "2026-04-09".to_string(),
        },
    ];

    assert_eq!(
        project_history_items(&items),
        vec![TranscriptItem::restored_tool_result(
            "Ran powershell -Command \"Get-Date\"",
            "2026-04-09"
        )]
    );
}
