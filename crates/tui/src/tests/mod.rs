use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lpa_core::{Model, PresetModelCatalog, SessionId};
use lpa_protocol::ProviderFamily;
use pretty_assertions::assert_eq;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use crate::app::{AuxPanelContent, TuiApp};
use crate::{
    SavedModelEntry,
    events::{SessionListEntry, TranscriptItem, TranscriptItemKind, WorkerEvent},
    input::InputBuffer,
    render,
    worker::QueryWorkerHandle,
};

/// Redirects all config persistence to a throwaway temp directory so tests that
/// exercise the save path never clobber the developer's real
/// `~/.launchpad/agent/config.toml`. Idempotent — the first call pins the path.
fn isolate_test_home() {
    let dir = std::env::temp_dir().join("lpa-tui-test-home");
    let _ = std::fs::create_dir_all(&dir);
    lpa_utils::override_lpa_home(dir);
}

/// Points the open `ModelList` aux panel at the row with the given slug.
fn select_model_row(app: &mut TuiApp, slug: &str) {
    let index = app
        .aux_panel
        .as_ref()
        .and_then(|panel| match &panel.content {
            AuxPanelContent::ModelList(entries) => {
                entries.iter().position(|entry| entry.slug == slug)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("model row {slug:?} not found in panel"));
    app.aux_panel_selection = index;
}

fn test_app() -> TuiApp {
    isolate_test_home();
    TuiApp {
        model: "test-model".to_string(),
        provider: ProviderFamily::anthropic(),
        cwd: PathBuf::from("."),
        transcript: Vec::new(),
        input: InputBuffer::new(),
        status_message: "Ready".to_string(),
        busy: false,
        spinner_index: 0,
        scroll: 0,
        follow_output: true,
        turn_count: 3,
        total_input_tokens: 10,
        total_output_tokens: 20,
        slash_selection: 0,
        pending_status_index: None,
        pending_assistant_index: None,
        pending_reasoning_index: None,
        worker: QueryWorkerHandle::stub(),
        model_catalog: PresetModelCatalog::new(vec![Model {
            slug: "test-model".to_string(),
            display_name: "Test Model".to_string(),
            provider: ProviderFamily::anthropic(),
            thinking_capability: lpa_core::ThinkingCapability::Toggle,
            ..Model::default()
        }]),
        saved_models: vec![],
        show_model_onboarding: false,
        onboarding_announced: false,
        onboarding_custom_model_pending: false,
        onboarding_preset_id: None,
        onboarding_prompt: None,
        onboarding_prompt_history: Vec::new(),
        onboarding_base_url_pending: false,
        onboarding_api_key_pending: false,
        onboarding_selected_model: None,
        onboarding_selected_model_is_custom: false,
        onboarding_selected_base_url: None,
        onboarding_selected_api_key: None,
        aux_panel: None,
        aux_panel_selection: 0,
        thinking_selection: None,
        pending_tool_items: std::collections::HashMap::new(),
        last_ctrl_c_at: None,
        show_reasoning: false,
        turn_emitted_text: false,
        paste_burst: crate::paste_burst::PasteBurst::default(),
        should_quit: false,
        inline_mode: false,
        terminal_width: 80,
        inline_assistant_stream_open: false,
        inline_assistant_pending_line: String::new(),
        inline_assistant_header_emitted: false,
        pending_inline_history: Vec::new(),
        pending_approval: None,
        pending_validation_retry: None,
    }
}

fn render_inline_lines(backend: &TestBackend) -> Vec<String> {
    format!("{backend}")
        .lines()
        .map(|line| line.trim_matches('"').to_string())
        .collect()
}

mod inline;
mod onboarding;
mod slash;

#[tokio::test]
async fn assistant_text_deltas_append_to_same_item() {
    let mut app = test_app();
    app.handle_worker_event(WorkerEvent::TextDelta("hel".to_string()));
    app.handle_worker_event(WorkerEvent::TextDelta("lo".to_string()));

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::Assistant);
    assert_eq!(app.transcript[0].body, "hello");
}

#[tokio::test]
async fn reasoning_deltas_append_to_reasoning_item() {
    let mut app = test_app();
    app.handle_worker_event(WorkerEvent::ReasoningDelta("plan ".to_string()));
    app.handle_worker_event(WorkerEvent::ReasoningDelta("first".to_string()));

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::Reasoning);
    assert_eq!(app.transcript[0].body, "plan first");
}

#[tokio::test]
async fn completed_assistant_message_restores_final_text() {
    let mut app = test_app();
    app.handle_worker_event(WorkerEvent::AssistantMessageCompleted(
        "final response".to_string(),
    ));

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::Assistant);
    assert_eq!(app.transcript[0].body, "final response");
}

#[tokio::test]
async fn tool_results_create_separate_items() {
    let mut app = test_app();
    app.handle_worker_event(WorkerEvent::ToolResult {
        tool_use_id: "tool-1".to_string(),
        preview: "done".to_string(),
        is_error: false,
        truncated: false,
    });

    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::ToolResult);
    assert_eq!(app.transcript[0].body, "done");
}

#[tokio::test]
async fn tool_result_fold_progresses_to_hidden_compact_state() {
    let mut item = TranscriptItem::new(
        TranscriptItemKind::ToolResult,
        "Tool output",
        (1..=12)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .with_tool_fold();

    let first = item.fold_next_at.expect("fold deadline");
    assert!(!item.advance_fold(Instant::now()));
    assert!(item.advance_fold(first));
    assert_eq!(item.fold_stage, 1);

    let second = item.fold_next_at.expect("second fold deadline");
    assert!(item.advance_fold(second));
    assert_eq!(item.fold_stage, 2);

    let third = item.fold_next_at.expect("third fold deadline");
    assert!(item.advance_fold(third));
    assert_eq!(item.fold_stage, 3);
    assert!(item.fold_next_at.is_none());
    assert!(!item.advance_fold(third));
}

#[tokio::test]
async fn ctrl_c_requires_confirmation_when_idle() {
    let mut app = test_app();

    app.handle_ctrl_c();
    assert!(!app.should_quit);
    assert_eq!(app.status_message, "Press Ctrl+C again within 2s to exit.");

    app.handle_ctrl_c();
    assert!(app.should_quit);
}

#[tokio::test]
async fn ctrl_c_requests_interrupt_before_exit_when_busy() {
    let mut app = test_app();
    app.busy = true;

    app.handle_ctrl_c();
    assert!(!app.should_quit);
    assert_eq!(
        app.status_message,
        "Interrupt requested. Press Ctrl+C again within 2s to exit."
    );

    app.handle_ctrl_c();
    assert!(app.should_quit);
}

#[tokio::test]
async fn enter_executes_highlighted_slash_command() {
    let mut app = test_app();
    app.input.replace("/");
    app.slash_selection = app
        .slash_suggestions()
        .iter()
        .position(|suggestion| suggestion.name == "/exit")
        .expect("exit suggestion should exist");

    app.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(app.should_quit);
}

#[tokio::test]
async fn model_panel_selection_updates_model() {
    let mut app = test_app();

    app.handle_slash_command("/model".to_string())
        .expect("model command should succeed");
    app.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        Rect::default(),
    );

    assert_eq!(app.model, "test-model");
}

#[tokio::test]
async fn session_new_command_updates_status() {
    let mut app = test_app();

    app.handle_slash_command("/new".to_string())
        .expect("slash command should succeed");

    assert_eq!(
        app.status_message,
        "New session ready; send a prompt to start it"
    );
    assert_eq!(app.aux_panel, None);
}

#[tokio::test]
async fn turn_failed_uses_specific_error_status_message() {
    let mut app = test_app();
    app.busy = true;

    app.handle_worker_event(WorkerEvent::TurnFailed {
        message: "anthropic provider requires an API key".to_string(),
        turn_count: 3,
        total_input_tokens: 10,
        total_output_tokens: 20,
    });

    assert_eq!(
        app.transcript.last(),
        Some(&TranscriptItem::new(
            TranscriptItemKind::Error,
            "Error",
            "anthropic provider requires an API key"
        ))
    );
    assert_eq!(app.status_message, "Query failed; see error above");
}

#[tokio::test]
async fn new_session_prepared_clears_transcript_and_busy_state() {
    let mut app = test_app();
    app.busy = true;
    app.transcript.push(TranscriptItem::new(
        TranscriptItemKind::User,
        "You",
        "old session",
    ));
    app.pending_status_index = Some(0);

    app.handle_worker_event(WorkerEvent::NewSessionPrepared);

    assert!(app.transcript.is_empty());
    assert!(!app.busy);
    assert_eq!(
        app.status_message,
        "New session ready; send a prompt to start it"
    );
}

#[tokio::test]
async fn tool_call_breaks_assistant_stream_into_new_segment() {
    let mut app = test_app();
    app.handle_worker_event(WorkerEvent::TextDelta("before".to_string()));
    app.handle_worker_event(WorkerEvent::ToolCall {
        tool_use_id: "tool-1".to_string(),
        summary: "bash: date".to_string(),
        detail: Some("{\n  \"command\": \"date\"\n}".to_string()),
    });
    app.handle_worker_event(WorkerEvent::TextDelta("after".to_string()));

    assert_eq!(
        app.transcript,
        vec![
            TranscriptItem::new(TranscriptItemKind::Assistant, "Assistant", "before"),
            TranscriptItem::new(TranscriptItemKind::ToolCall, "bash: date", ""),
            TranscriptItem::new(TranscriptItemKind::Assistant, "Assistant", "after"),
        ]
    );
}

#[tokio::test]
async fn tool_result_readds_thinking_while_turn_is_still_busy() {
    let mut app = test_app();
    app.busy = true;
    app.pending_status_index = Some(app.push_item(TranscriptItemKind::System, "Thinking", ""));

    app.handle_worker_event(WorkerEvent::ToolResult {
        tool_use_id: "tool-1".to_string(),
        preview: "2026-04-06 23:58:56".to_string(),
        is_error: false,
        truncated: false,
    });

    assert_eq!(app.transcript.len(), 2);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::ToolResult);
    assert_eq!(app.transcript[0].title, "Tool output");
    assert_eq!(app.transcript[0].body, "2026-04-06 23:58:56");
    // Tool output lands in the static preview stage (1) immediately — no
    // pop-in-then-vanish animation.
    assert_eq!(app.transcript[0].fold_stage, 1);
    assert!(app.transcript[0].fold_next_at.is_none());
    assert_eq!(
        app.transcript[1],
        TranscriptItem::new(TranscriptItemKind::System, "Thinking", "")
    );
}

#[tokio::test]
async fn submit_prompt_inserts_status_line_below_user_message() {
    let mut app = test_app();

    app.submit_prompt("hello".to_string())
        .expect("submit should succeed");

    assert_eq!(
        app.transcript,
        vec![
            TranscriptItem::new(TranscriptItemKind::User, "You", "hello"),
            TranscriptItem::new(TranscriptItemKind::System, "Thinking", ""),
        ]
    );
}

#[tokio::test]
async fn session_switched_event_updates_model_and_restores_transcript() {
    let mut app = test_app();

    app.handle_worker_event(WorkerEvent::SessionSwitched {
        session_id: "00000000-0000-0000-0000-000000000001".to_string(),
        title: Some("Saved session".to_string()),
        model: Some("restored-model".to_string()),
        total_input_tokens: 42,
        total_output_tokens: 7,
        history_items: vec![TranscriptItem::new(
            TranscriptItemKind::User,
            "You",
            "restored prompt",
        )],
        loaded_item_count: 7,
    });

    assert_eq!(app.model, "restored-model");
    assert_eq!(app.total_input_tokens, 42);
    assert_eq!(app.total_output_tokens, 7);
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(app.transcript[0].kind, TranscriptItemKind::User);
    assert_eq!(app.transcript[0].body, "restored prompt");
}

#[tokio::test]
async fn turn_started_event_updates_displayed_model() {
    let mut app = test_app();

    app.handle_worker_event(WorkerEvent::TurnStarted {
        model: "server-model".to_string(),
    });

    assert_eq!(app.model, "server-model");
    assert!(app.busy);
}

#[tokio::test]
async fn session_renamed_event_adds_transcript_note() {
    let mut app = test_app();

    app.handle_worker_event(WorkerEvent::SessionRenamed {
        session_id: "00000000-0000-0000-0000-000000000001".to_string(),
        title: "Renamed session".to_string(),
    });

    assert_eq!(app.status_message, "Session renamed");
    assert_eq!(app.transcript.len(), 1);
    assert!(app.transcript[0].body.contains("Renamed session"));
}

#[tokio::test]
async fn session_title_updated_event_refreshes_visible_session_list() {
    let mut app = test_app();
    let session_id = SessionId::new();
    app.show_session_panel(vec![SessionListEntry {
        session_id,
        title: "(untitled)".to_string(),
        updated_at: "2026-04-06 08:00:00 UTC".to_string(),
        is_active: true,
    }]);

    app.handle_worker_event(WorkerEvent::SessionTitleUpdated {
        session_id: session_id.to_string(),
        title: "Generated title".to_string(),
    });

    assert_eq!(app.status_message, "Session titled: Generated title");
    assert!(app.aux_panel.as_ref().is_some_and(|panel| {
        matches!(
            &panel.content,
            AuxPanelContent::SessionList(entries)
                if entries.iter().any(|entry| entry.title == "Generated title")
        )
    }));
}

#[tokio::test]
async fn sessions_listed_event_updates_bottom_panel_not_transcript() {
    let mut app = test_app();

    app.handle_worker_event(WorkerEvent::SessionsListed {
        sessions: vec![SessionListEntry {
            session_id: SessionId::new(),
            title: "Saved conversation".to_string(),
            updated_at: "2026-04-06 08:00:00 UTC".to_string(),
            is_active: true,
        }],
    });

    assert!(app.transcript.is_empty());
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Sessions")
    );
    assert!(app
            .aux_panel
            .as_ref()
            .is_some_and(|panel| matches!(&panel.content, AuxPanelContent::SessionList(entries) if entries.iter().any(|entry| entry.title == "Saved conversation"))));
}

#[tokio::test]
async fn session_panel_selection_moves_with_up_and_down() {
    let mut app = test_app();
    app.show_session_panel(vec![
        SessionListEntry {
            session_id: SessionId::new(),
            title: "First".to_string(),
            updated_at: "2026-04-06 08:00:00 UTC".to_string(),
            is_active: true,
        },
        SessionListEntry {
            session_id: SessionId::new(),
            title: "Second".to_string(),
            updated_at: "2026-04-06 09:00:00 UTC".to_string(),
            is_active: false,
        },
    ]);

    app.move_aux_panel_selection(1);
    assert_eq!(app.aux_panel_selection, 1);

    app.move_aux_panel_selection(-1);
    assert_eq!(app.aux_panel_selection, 0);

    app.move_aux_panel_selection(-1);
    assert_eq!(app.aux_panel_selection, 1);

    app.move_aux_panel_selection(1);
    assert_eq!(app.aux_panel_selection, 0);
}

#[tokio::test]
async fn escape_dismisses_slash_popup_and_clears_input() {
    let mut app = test_app();
    app.input.replace("/mo");

    app.handle_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        Rect::new(0, 0, 80, 24),
    );

    assert_eq!(app.input.text(), "");
    assert!(!app.has_slash_suggestions());
}

#[tokio::test]
async fn typing_with_aux_panel_open_dismisses_panel_and_starts_input() {
    let mut app = test_app();
    app.show_aux_panel("Status", "details");

    app.handle_key(
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        Rect::new(0, 0, 80, 24),
    );

    assert!(app.aux_panel.is_none());
    assert_eq!(app.input.text(), "h");
}

#[tokio::test]
async fn interrupted_turn_adds_status_line_to_transcript() {
    let mut app = test_app();
    app.pending_status_index = Some(app.push_item(TranscriptItemKind::System, "Thinking", ""));
    app.busy = true;

    app.handle_worker_event(WorkerEvent::TurnFinished {
        stop_reason: "Interrupted".to_string(),
        turn_count: 1,
        total_input_tokens: 0,
        total_output_tokens: 0,
    });

    assert_eq!(
        app.transcript,
        vec![TranscriptItem::new(
            TranscriptItemKind::System,
            "Interrupted",
            "",
        )]
    );
}

#[tokio::test]
async fn completed_turn_with_text_leaves_no_end_marker() {
    let mut app = test_app();
    app.pending_status_index = Some(app.push_item(TranscriptItemKind::System, "Thinking", ""));
    app.busy = true;
    // Simulate the assistant emitting text during the turn.
    app.handle_worker_event(WorkerEvent::AssistantMessageCompleted("hi".to_string()));

    app.handle_worker_event(WorkerEvent::TurnFinished {
        stop_reason: "Completed".to_string(),
        turn_count: 1,
        total_input_tokens: 0,
        total_output_tokens: 0,
    });

    // Successful turn with text ends silently — the composer lighting back
    // up is the end-of-turn signal. No "Complete" or "No response" marker.
    assert!(!app.busy);
    assert!(
        !app.transcript
            .iter()
            .any(|item| item.title == "Complete" || item.title == "No response"),
        "should not emit end-of-turn markers when text was produced",
    );
}

#[tokio::test]
async fn empty_turn_pushes_no_response_marker() {
    let mut app = test_app();
    app.pending_status_index = Some(app.push_item(TranscriptItemKind::System, "Thinking", ""));
    app.busy = true;
    // No assistant text emitted — simulate the "silent turn" bug the user hit.

    app.handle_worker_event(WorkerEvent::TurnFinished {
        stop_reason: "Completed".to_string(),
        turn_count: 1,
        total_input_tokens: 0,
        total_output_tokens: 0,
    });

    assert!(
        app.transcript
            .iter()
            .any(|item| item.title == "No response"),
        "empty turn should surface an explicit marker",
    );
}

#[tokio::test]
async fn interrupted_turn_still_pushes_marker() {
    let mut app = test_app();
    app.pending_status_index = Some(app.push_item(TranscriptItemKind::System, "Thinking", ""));
    app.busy = true;

    app.handle_worker_event(WorkerEvent::TurnFinished {
        stop_reason: "Interrupted".to_string(),
        turn_count: 1,
        total_input_tokens: 0,
        total_output_tokens: 0,
    });

    assert!(
        app.transcript
            .iter()
            .any(|item| item.title == "Interrupted"),
        "interrupted turn should surface a marker"
    );
}

#[tokio::test]
async fn reasoning_toggle_flips_show_reasoning_flag() {
    let mut app = test_app();
    assert!(!app.show_reasoning);
    app.handle_slash_command("/reasoning".to_string())
        .expect("reasoning command");
    assert!(app.show_reasoning);
    app.handle_slash_command("/reasoning".to_string())
        .expect("reasoning command");
    assert!(!app.show_reasoning);
}
