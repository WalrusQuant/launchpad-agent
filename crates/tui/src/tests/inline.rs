//! TUI tests — inline group. Helpers and imports live in the parent
//! `tests` module and are pulled in via `use super::*`.

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn inline_assistant_stream_flushes_to_pending_history_on_tool_call_boundary() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_worker_event(WorkerEvent::TextDelta("before".to_string()));
    assert!(app.pending_inline_history.is_empty());

    app.handle_worker_event(WorkerEvent::ToolCall {
        tool_use_id: "tool-1".to_string(),
        summary: "bash: date".to_string(),
        detail: None,
    });

    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("--- assistant"))
    );
    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("assistant> before"))
    );
}

#[tokio::test]
async fn inline_assistant_stream_flushes_completed_lines_before_turn_end() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_worker_event(WorkerEvent::TextDelta("line 1\nline 2".to_string()));

    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("assistant> line 1"))
    );
    assert_eq!(app.inline_assistant_pending_line, "line 2");
}

#[tokio::test]
async fn inline_assistant_stream_flushes_wrapped_visual_line_without_newline() {
    let mut app = test_app();
    app.inline_mode = true;
    app.terminal_width = 24;

    app.handle_worker_event(WorkerEvent::TextDelta(
        "this is a long assistant line without newline yet".to_string(),
    ));

    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("--- assistant"))
    );
    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("assistant> "))
    );
    assert!(!app.inline_assistant_pending_line.is_empty());
}

#[tokio::test]
async fn inline_assistant_stream_flushes_to_pending_history_when_turn_finishes() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_worker_event(WorkerEvent::TextDelta("hello".to_string()));
    app.handle_worker_event(WorkerEvent::TurnFinished {
        stop_reason: "completed".to_string(),
        turn_count: 1,
        total_input_tokens: 5,
        total_output_tokens: 7,
    });

    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("--- assistant"))
    );
    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("assistant> hello"))
    );
}

#[tokio::test]
async fn inline_slash_command_emits_shell_echo_to_history_queue() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_slash_command("/status".to_string())
        .expect("status command should succeed");

    assert!(
        app.pending_inline_history
            .iter()
            .any(|block| block.contains("› /status"))
    );
}

#[tokio::test]
async fn slash_sessions_in_inline_mode_opens_aux_panel() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_slash_command("/sessions".to_string())
        .expect("sessions command should succeed");

    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Sessions")
    );
    assert_eq!(app.status_message, "Listing sessions");
    assert!(app.transcript.is_empty());
}

#[tokio::test]
async fn slash_model_in_inline_mode_shows_bottom_panel() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_slash_command("/model".to_string())
        .expect("model command should succeed");

    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Models")
    );
    assert_eq!(app.status_message, "Model switcher shown");
    assert!(app.transcript.is_empty());
}

#[tokio::test]
async fn slash_thinking_in_inline_mode_shows_bottom_panel() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_slash_command("/thinking".to_string())
        .expect("thinking command should succeed");

    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Thinking")
    );
    assert!(app.aux_panel.as_ref().is_some_and(
        |panel| matches!(&panel.content, AuxPanelContent::ThinkingList(entries) if !entries.is_empty())
    ));
    assert!(app.transcript.is_empty());
}

#[tokio::test]
async fn slash_model_with_argument_in_inline_mode_updates_status_without_transcript_note() {
    let mut app = test_app();
    app.inline_mode = true;

    app.handle_slash_command("/model test-model".to_string())
        .expect("model command should succeed");

    assert_eq!(app.model, "test-model");
    assert_eq!(app.status_message, "Model set to test-model");
    assert!(app.transcript.is_empty());
}

#[tokio::test]
async fn inline_slash_popup_uses_reserved_bottom_area_and_restores_transcript() {
    let mut app = test_app();
    app.inline_mode = true;
    app.transcript.push(TranscriptItem::new(
        TranscriptItemKind::Assistant,
        "assistant",
        ["line 1", "line 2", "line 3", "line 4", "line 5"].join("\n"),
    ));
    app.input.insert_str("/mo");

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");
    terminal
        .draw(|frame| render::draw(frame, &app, true))
        .expect("draw popup");
    let popup_lines = render_inline_lines(terminal.backend());

    app.input.clear();
    terminal
        .draw(|frame| render::draw(frame, &app, true))
        .expect("draw restored transcript");
    let restored_lines = render_inline_lines(terminal.backend());

    assert!(!popup_lines.iter().any(|line| line.contains("line 1")));
    assert!(!popup_lines.iter().any(|line| line.contains("line 2")));
    assert!(!popup_lines.iter().any(|line| line.contains("line 3")));
    assert!(!popup_lines.iter().any(|line| line.contains("line 4")));
    assert!(!popup_lines.iter().any(|line| line.contains("line 5")));
    assert!(popup_lines.iter().any(|line| line.contains("› /mo")));
    assert!(popup_lines.iter().any(|line| line.contains("/model")));
    assert!(
        popup_lines
            .iter()
            .any(|line| line.contains("Show or change"))
    );
    assert!(
        restored_lines
            .iter()
            .any(|line| line.contains("Type a message or / for commands"))
    );
    assert!(!restored_lines.iter().any(|line| line.contains("/model")));
    assert!(!restored_lines.iter().any(|line| line.contains("line 1")));
}

#[tokio::test]
async fn inline_aux_panel_uses_reserved_bottom_area_and_restores_transcript() {
    let mut app = test_app();
    app.inline_mode = true;
    app.transcript.push(TranscriptItem::new(
        TranscriptItemKind::Assistant,
        "assistant",
        ["alpha", "beta", "gamma", "delta", "epsilon"].join("\n"),
    ));
    app.show_aux_panel("Status", "one\ntwo\nthree");

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");
    terminal
        .draw(|frame| render::draw(frame, &app, true))
        .expect("draw aux panel");
    let open_lines = render_inline_lines(terminal.backend());

    app.aux_panel = None;
    terminal
        .draw(|frame| render::draw(frame, &app, true))
        .expect("draw restored transcript");
    let closed_lines = render_inline_lines(terminal.backend());

    assert!(open_lines.iter().any(|line| line.contains("Status")));
    assert!(open_lines.iter().any(|line| line.contains("one")));
    assert!(open_lines.iter().any(|line| line.contains("two")));
    assert!(open_lines.iter().any(|line| line.contains("three")));
    assert!(
        !open_lines
            .iter()
            .any(|line| line.contains("┌") || line.contains("│"))
    );
    assert!(!open_lines.iter().any(|line| line.contains("alpha")));
    assert!(
        closed_lines
            .iter()
            .any(|line| line.contains("Type a message or / for commands"))
    );
    assert!(!closed_lines.iter().any(|line| line.contains("alpha")));
    assert!(!closed_lines.iter().any(|line| line.contains("epsilon")));
}

#[tokio::test]
async fn sessions_listed_event_updates_bottom_panel_in_inline_mode() {
    let mut app = test_app();
    app.inline_mode = true;

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
    assert!(app.aux_panel.as_ref().is_some_and(|panel| {
        matches!(
            &panel.content,
            AuxPanelContent::SessionList(entries)
                if entries.iter().any(|entry| entry.title == "Saved conversation")
        )
    }));
}
