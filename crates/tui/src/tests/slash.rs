//! TUI tests — slash group. Helpers and imports live in the parent
//! `tests` module and are pulled in via `use super::*`.

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn slash_status_shows_bottom_panel() {
    let mut app = test_app();

    app.handle_slash_command("/status".to_string())
        .expect("status command should succeed");

    assert!(app.transcript.is_empty());
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Status")
    );
    assert!(app.aux_panel.as_ref().is_some_and(
        |panel| matches!(&panel.content, AuxPanelContent::Text(body) if body.contains("turns: 3"))
    ));
}

#[tokio::test]
async fn slash_sessions_requests_listing() {
    let mut app = test_app();

    app.handle_slash_command("/sessions".to_string())
        .expect("sessions command should succeed");

    assert_eq!(app.status_message, "Listing sessions");
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Sessions")
    );
}

#[tokio::test]
async fn slash_skills_requests_listing() {
    let mut app = test_app();

    app.handle_slash_command("/skills".to_string())
        .expect("skills command should succeed");

    assert_eq!(app.status_message, "Listing skills");
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Skills")
    );
    assert!(app.aux_panel.as_ref().is_some_and(
        |panel| matches!(&panel.content, AuxPanelContent::Text(body) if body == "Loading skills...")
    ));
}

#[tokio::test]
async fn slash_new_requests_new_session() {
    let mut app = test_app();

    app.handle_slash_command("/new".to_string())
        .expect("new command should succeed");

    assert_eq!(
        app.status_message,
        "New session ready; send a prompt to start it"
    );
}

#[tokio::test]
async fn slash_model_shows_bottom_panel() {
    let mut app = test_app();

    app.handle_slash_command("/model".to_string())
        .expect("model command should succeed");

    assert!(app.transcript.is_empty());
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Models")
    );
    assert!(app
            .aux_panel
            .as_ref()
            .is_some_and(|panel| matches!(&panel.content, AuxPanelContent::ModelList(entries) if entries.iter().any(|entry| entry.slug == "test-model") && entries.iter().any(|entry| entry.is_custom_mode))));
}

#[tokio::test]
async fn slash_thinking_shows_bottom_panel() {
    let mut app = test_app();

    app.handle_slash_command("/thinking".to_string())
        .expect("thinking command should succeed");

    assert!(app.transcript.is_empty());
    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Thinking")
    );
    assert!(app.aux_panel.as_ref().is_some_and(
        |panel| matches!(&panel.content, AuxPanelContent::ThinkingList(entries) if !entries.is_empty())
    ));
}

#[tokio::test]
async fn slash_configure_starts_configure_flow() {
    let mut app = test_app();

    app.handle_slash_command("/configure".to_string())
        .expect("configure command should succeed");

    assert!(app.show_model_onboarding);
    // New flow: first step is the preset picker, not the model picker.
    assert!(app.is_preset_picker_open());
    assert_eq!(app.status_message, "Configuration started");
}

#[tokio::test]
async fn slash_onboard_alias_still_starts_flow() {
    let mut app = test_app();

    app.handle_slash_command("/onboard".to_string())
        .expect("onboard alias should succeed");

    assert!(app.show_model_onboarding);
}

#[tokio::test]
async fn slash_rename_requires_title() {
    let mut app = test_app();

    assert!(app.handle_slash_command("/rename".to_string()).is_err());
}

#[tokio::test]
async fn slash_exit_requests_shutdown() {
    let mut app = test_app();
    app.input.replace("/exit");

    app.handle_slash_command("/exit".to_string())
        .expect("exit command should succeed");

    assert!(app.should_quit);
    assert!(app.aux_panel.is_none());
    assert_eq!(app.input.text(), "");
}

#[tokio::test]
async fn slash_help_lists_commands() {
    let mut app = test_app();

    app.handle_slash_command("/help".to_string())
        .expect("help command should succeed");

    assert_eq!(
        app.aux_panel.as_ref().map(|panel| panel.title.as_str()),
        Some("Commands")
    );
    assert!(app.aux_panel.as_ref().is_some_and(|panel| matches!(
        &panel.content,
        AuxPanelContent::Text(body) if body.contains("/help") && body.contains("/exit")
    )));
}

#[tokio::test]
async fn slash_export_writes_transcript_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut app = test_app();
    app.cwd = dir.path().to_path_buf();
    app.push_item(TranscriptItemKind::User, "You", "hello there");

    app.handle_slash_command("/export".to_string())
        .expect("export command should succeed");

    let exported = std::fs::read_to_string(dir.path().join("lpagent-transcript.md"))
        .expect("export file written");
    assert!(exported.contains("# lpagent transcript"));
    assert!(exported.contains("hello there"));
}

#[tokio::test]
async fn hash_prefixed_input_appends_memory_line() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut app = test_app();
    app.cwd = dir.path().to_path_buf();

    app.handle_submission("# always run the tests".to_string())
        .expect("memory note should be accepted");

    let memory =
        std::fs::read_to_string(dir.path().join("AGENTS.md")).expect("memory file written");
    assert!(memory.contains("- always run the tests"));
}

#[tokio::test]
async fn slash_completion_applies_selected_command() {
    let mut app = test_app();
    app.input.replace("/exi");

    assert!(app.try_apply_slash_suggestion());
    assert_eq!(app.input.text(), "/exit");
}

#[tokio::test]
async fn slash_suggestions_include_configure() {
    let mut app = test_app();
    app.input.replace("/c");

    assert!(
        app.slash_suggestions()
            .iter()
            .any(|suggestion| suggestion.name == "/configure")
    );
}

#[tokio::test]
async fn slash_model_rejects_builtin_model_that_does_not_match_active_provider() {
    let mut app = test_app();
    app.model_catalog = PresetModelCatalog::new(vec![Model {
        slug: "gpt-5.4".to_string(),
        display_name: "GPT-5.4".to_string(),
        provider: ProviderFamily::openai(),
        ..Model::default()
    }]);

    app.handle_slash_command("/model gpt-5.4".to_string())
        .expect("model command should stay in tui");

    assert_eq!(app.model, "test-model");
    assert_eq!(app.status_message, "Failed to switch model");
    assert_eq!(
        app.transcript.last(),
        Some(&TranscriptItem::new(
            TranscriptItemKind::Error,
            "Model switch failed",
            "model `gpt-5.4` requires provider `openai`, but the active wire_api resolves to `anthropic`"
        ))
    );
}

#[tokio::test]
async fn slash_model_rejects_saved_model_with_mismatched_wire_api() {
    let mut app = test_app();
    app.model_catalog = PresetModelCatalog::new(vec![Model {
        slug: "gpt-5.4".to_string(),
        display_name: "GPT-5.4".to_string(),
        provider: ProviderFamily::openai(),
        ..Model::default()
    }]);
    app.saved_models = vec![SavedModelEntry {
        model: "gpt-5.4".to_string(),
        provider: ProviderFamily::anthropic(),
        wire_api: lpa_core::ProviderWireApi::AnthropicMessages,
        base_url: None,
        api_key: None,
    }];

    app.handle_slash_command("/model gpt-5.4".to_string())
        .expect("model command should stay in tui");

    assert_eq!(app.model, "test-model");
    assert_eq!(app.status_message, "Failed to switch model");
    assert_eq!(
        app.transcript.last(),
        Some(&TranscriptItem::new(
            TranscriptItemKind::Error,
            "Model switch failed",
            "model `gpt-5.4` requires provider `openai`, but the active wire_api resolves to `anthropic`"
        ))
    );
}

#[tokio::test]
async fn slash_selection_wraps_around() {
    let mut app = test_app();
    app.input.replace("/");

    app.move_slash_selection(-1);
    assert_eq!(app.slash_selection, app.slash_suggestions().len() - 1);

    app.move_slash_selection(1);
    assert_eq!(app.slash_selection, 0);
}
