//! TUI tests — onboarding group. Helpers and imports live in the parent
//! `tests` module and are pulled in via `use super::*`.

use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn configure_openrouter_preset_opens_model_picker() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("openrouter");

    // URL comes from the preset, not a prompt.
    assert_eq!(
        app.onboarding_selected_base_url.as_deref(),
        Some("https://openrouter.ai/api/v1")
    );
    // The new flow shows a curated model list before asking for anything.
    assert!(app.is_onboarding_model_picker_open());
    assert!(!app.onboarding_api_key_pending);
    assert!(!app.onboarding_base_url_pending);
}

#[tokio::test]
async fn configure_openrouter_full_flow_reaches_validation() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("openrouter");

    // Pick a curated model from the list.
    select_model_row(&mut app, "z-ai/glm-4.6");
    app.try_accept_aux_panel_selection();

    // No saved key yet → the flow asks for the API key once.
    assert!(app.onboarding_api_key_pending);
    assert_eq!(
        app.onboarding_selected_model.as_deref(),
        Some("z-ai/glm-4.6")
    );

    // Entering the key triggers validation (which fails in the harness; we only
    // care that the model + key were captured and validation was attempted).
    let result = app.handle_submission("sk-or-v1-testkey".to_string());
    assert!(result.is_ok());
    assert_eq!(
        app.onboarding_selected_api_key.as_deref(),
        Some("sk-or-v1-testkey")
    );
    assert!(!app.onboarding_api_key_pending);
}

#[tokio::test]
async fn configure_openrouter_custom_model_row_shows_slug_hint() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("openrouter");

    select_model_row(&mut app, "__custom__");
    app.try_accept_aux_panel_selection();

    assert!(app.onboarding_custom_model_pending);
    let prompt = app.onboarding_prompt.as_deref().unwrap_or("");
    assert!(
        prompt.contains("anthropic/claude-3.5-sonnet"),
        "expected slug hint in prompt, got {prompt:?}"
    );
}

#[tokio::test]
async fn configure_ollama_preset_picks_model_without_api_key() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("ollama");

    // Ollama ships a curated model list and needs no key.
    assert!(app.is_onboarding_model_picker_open());
    assert!(!app.onboarding_api_key_pending);
    assert_eq!(
        app.onboarding_selected_base_url.as_deref(),
        Some("http://localhost:11434/v1")
    );

    // Selecting a model validates directly — no API key prompt.
    select_model_row(&mut app, "llama3.2");
    app.try_accept_aux_panel_selection();
    assert!(!app.onboarding_api_key_pending);
    assert_eq!(app.onboarding_selected_model.as_deref(), Some("llama3.2"));
}

#[tokio::test]
async fn configure_reuses_saved_api_key_for_provider() {
    let mut app = test_app();
    app.saved_models = vec![SavedModelEntry {
        model: "z-ai/glm-4.6".to_string(),
        provider: ProviderFamily::openai(),
        wire_api: lpa_core::ProviderWireApi::OpenAIChatCompletions,
        base_url: Some("https://openrouter.ai/api/v1".to_string()),
        api_key: Some("sk-or-saved".to_string()),
    }];
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("openrouter");

    // Pick a *different* model for the same provider — the saved key is reused,
    // so no API key prompt appears.
    select_model_row(&mut app, "openai/gpt-4o");
    app.try_accept_aux_panel_selection();

    assert!(!app.onboarding_api_key_pending);
    assert_eq!(
        app.onboarding_selected_api_key.as_deref(),
        Some("sk-or-saved")
    );
    assert_eq!(
        app.onboarding_selected_model.as_deref(),
        Some("openai/gpt-4o")
    );
}

#[tokio::test]
async fn configure_does_not_reuse_key_from_a_different_provider() {
    let mut app = test_app();
    // A saved key whose base URL is a *prefix* of another provider's URL must
    // not be lent out — identity is the exact base URL + wire API.
    app.saved_models = vec![SavedModelEntry {
        model: "some-model".to_string(),
        provider: ProviderFamily::openai(),
        wire_api: lpa_core::ProviderWireApi::OpenAIChatCompletions,
        base_url: Some("https://api.x.ai".to_string()),
        api_key: Some("sk-other".to_string()),
    }];
    app.handle_slash_command("/configure".to_string()).unwrap();
    // xAI's real base URL is https://api.x.ai/v1 — the saved https://api.x.ai
    // is a prefix but not an exact match, so it must be ignored.
    app.handle_preset_selected("xai");
    select_model_row(&mut app, "grok-4");
    app.try_accept_aux_panel_selection();

    // No matching saved key → the flow asks for one instead of reusing.
    assert!(app.onboarding_api_key_pending);
    assert_eq!(app.onboarding_selected_api_key, None);
}

#[tokio::test]
async fn configure_anthropic_preset_shows_model_catalog() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();
    app.handle_preset_selected("anthropic");

    // First-party presets still go through the builtin model catalog.
    assert!(app.is_onboarding_model_picker_open());
}

#[tokio::test]
async fn configure_prints_current_config_summary_on_start() {
    let mut app = test_app();
    app.handle_slash_command("/configure".to_string()).unwrap();

    // A "Current configuration" transcript item should be present.
    let has_summary = app.transcript.iter().any(|item| {
        let body = format!("{item:?}");
        body.contains("Current configuration")
    });
    assert!(has_summary, "expected current-config summary in transcript");
}

#[tokio::test]
async fn masked_api_key_shows_only_last_four_chars() {
    use crate::app::worker_events::mask_with_suffix;
    assert_eq!(mask_with_suffix("sk-or-v1-abcdefg"), "***defg");
    assert_eq!(mask_with_suffix("short"), "***hort");
    // Short tokens (<= 4 chars) are fully masked to avoid exposing them.
    assert_eq!(mask_with_suffix("abc"), "****");
    assert_eq!(mask_with_suffix(""), "****");
}

#[tokio::test]
async fn onboarding_model_panel_includes_custom_entry() {
    let mut app = test_app();
    app.show_model_onboarding = true;

    app.show_model_panel();

    assert!(app.aux_panel.as_ref().is_some_and(|panel| {
        matches!(
            &panel.content,
            AuxPanelContent::ModelList(entries)
                if entries.iter().any(|entry| entry.is_custom_mode)
        )
    }));
}

#[tokio::test]
async fn onboarding_model_picker_ignores_plain_typing() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.show_model_panel();

    app.handle_key(
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(app.input.is_blank());
    assert!(app.has_selectable_aux_panel());
}

#[tokio::test]
async fn onboarding_model_picker_allows_custom_shortcut() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.show_model_panel();

    app.handle_key(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(app.onboarding_custom_model_pending);
    assert_eq!(app.onboarding_prompt.as_deref(), Some("model name"));
    assert!(app.aux_panel.is_none());
}

#[tokio::test]
async fn onboarding_model_picker_enter_on_custom_row_starts_custom_flow() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.show_model_panel();
    app.aux_panel_selection = app
        .aux_panel
        .as_ref()
        .and_then(|panel| match &panel.content {
            AuxPanelContent::ModelList(entries) => {
                entries.iter().position(|entry| entry.is_custom_mode)
            }
            _ => None,
        })
        .expect("custom row should exist");

    app.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(app.onboarding_custom_model_pending);
    assert_eq!(app.onboarding_prompt.as_deref(), Some("model name"));
    assert!(app.aux_panel.is_none());
}

#[tokio::test]
async fn onboarding_model_picker_enter_on_builtin_row_prompts_for_connection() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.saved_models = vec![SavedModelEntry {
        model: "existing-model".to_string(),
        provider: ProviderFamily::anthropic(),
        wire_api: lpa_core::ProviderWireApi::AnthropicMessages,
        base_url: Some("https://example.invalid/v1".to_string()),
        api_key: Some("secret".to_string()),
    }];
    app.model_catalog = PresetModelCatalog::new(vec![Model {
        slug: "new-anthropic-model".to_string(),
        display_name: "New Anthropic Model".to_string(),
        provider: ProviderFamily::anthropic(),
        description: Some("test model".to_string()),
        ..Model::default()
    }]);
    app.show_model_panel();
    app.aux_panel_selection = app
        .aux_panel
        .as_ref()
        .and_then(|panel| match &panel.content {
            AuxPanelContent::ModelList(entries) => entries
                .iter()
                .position(|entry| entry.slug == "new-anthropic-model"),
            _ => None,
        })
        .expect("builtin row should exist");

    app.handle_key(
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(!app.onboarding_custom_model_pending);
    assert!(app.onboarding_base_url_pending);
    assert_eq!(
        app.onboarding_selected_model.as_deref(),
        Some("new-anthropic-model")
    );
    assert_eq!(app.onboarding_prompt.as_deref(), Some("base url"));
    assert!(app.aux_panel.is_none());
}

#[tokio::test]
async fn onboarding_rejects_base_url_without_http_scheme() {
    let mut app = test_app();
    app.onboarding_base_url_pending = true;
    app.onboarding_selected_model = Some("test-model".to_string());

    app.handle_submission("localhost:11434".to_string())
        .expect("submission should not crash");

    assert!(app.onboarding_base_url_pending);
    assert_eq!(app.onboarding_prompt.as_deref(), Some("base url"));
    assert_eq!(
        app.status_message,
        "Base URL must start with http:// or https://"
    );
}

#[tokio::test]
async fn onboarding_escape_steps_back_to_model_list() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.begin_custom_model_onboarding();

    app.handle_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(app.is_onboarding_model_picker_open());
    assert!(app.onboarding_prompt.is_none());
    assert!(!app.onboarding_custom_model_pending);
}

#[tokio::test]
async fn onboarding_escape_from_root_dismisses_onboarding() {
    let mut app = test_app();
    app.show_model_onboarding = true;
    app.show_model_panel();

    app.handle_key(
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        Rect::default(),
    );

    assert!(!app.show_model_onboarding);
    assert!(app.aux_panel.is_none());
    assert_eq!(app.status_message, "Configuration dismissed");
}

#[tokio::test]
async fn validation_failure_enters_retry_state() {
    let mut app = test_app();
    app.busy = true;
    app.onboarding_selected_model = Some("test-model".to_string());
    app.onboarding_selected_api_key = Some("sk-test".to_string());

    app.handle_worker_event(WorkerEvent::ProviderValidationFailed {
        message: "connection refused".to_string(),
    });

    assert!(!app.busy);
    assert!(app.pending_validation_retry.is_some());
    assert_eq!(
        app.pending_validation_retry
            .as_ref()
            .unwrap()
            .failure_message,
        "connection refused"
    );
    // Retry panel owns the composer — no individual step should be pending.
    assert!(!app.onboarding_api_key_pending);
    assert!(!app.onboarding_custom_model_pending);
    assert!(!app.onboarding_base_url_pending);
    assert!(app.status_message.contains("connection refused"));
    // Both the error and the "What next?" explainer should be in the transcript.
    assert!(
        app.transcript
            .iter()
            .any(|item| item.title == "Validation failed")
    );
    assert!(app.transcript.iter().any(|item| item.title == "What next?"));
}

#[tokio::test]
async fn validation_failure_allows_retry_without_losing_input() {
    let mut app = test_app();
    app.busy = true;
    app.onboarding_selected_model = Some("test-model".to_string());
    app.onboarding_selected_base_url = Some("https://example.test".to_string());
    app.onboarding_selected_api_key = Some("sk-test".to_string());

    app.handle_worker_event(WorkerEvent::ProviderValidationFailed {
        message: "connection refused".to_string(),
    });
    assert!(app.pending_validation_retry.is_some());

    app.retry_validation();

    // Retry clears the decision flag and re-enters the validating state.
    assert!(app.pending_validation_retry.is_none());
    assert!(app.busy);
    assert_eq!(app.status_message, "Validating provider connection");
    // Inputs the user typed must survive the retry so the second probe uses
    // exactly the same parameters.
    assert_eq!(app.onboarding_selected_model.as_deref(), Some("test-model"));
    assert_eq!(
        app.onboarding_selected_base_url.as_deref(),
        Some("https://example.test")
    );
    assert_eq!(app.onboarding_selected_api_key.as_deref(), Some("sk-test"));
}

#[tokio::test]
async fn validation_skip_pushes_save_without_probe_notice() {
    let mut app = test_app();
    app.onboarding_selected_model = Some("test-model".to_string());
    app.onboarding_selected_api_key = Some("sk-test".to_string());

    app.handle_worker_event(WorkerEvent::ProviderValidationFailed {
        message: "connection refused".to_string(),
    });

    app.skip_validation_and_save();

    assert!(app.pending_validation_retry.is_none());
    assert!(
        app.transcript
            .iter()
            .any(|item| item.kind == TranscriptItemKind::System
                && item.body.contains("Saving without validation")),
        "expected a System transcript item noting the skip, got {:?}",
        app.transcript
    );
}

#[tokio::test]
async fn validation_change_reprompts_for_api_key_for_keyed_provider() {
    let mut app = test_app();
    app.onboarding_preset_id = Some("openrouter".to_string());
    app.onboarding_selected_model = Some("z-ai/glm-4.6".to_string());
    app.onboarding_selected_api_key = Some("sk-bad".to_string());

    app.handle_worker_event(WorkerEvent::ProviderValidationFailed {
        message: "invalid api key".to_string(),
    });

    app.change_validation_inputs();

    assert!(app.pending_validation_retry.is_none());
    // A bad/expired key is the usual cause — re-prompt for the key, keep the model.
    assert!(app.onboarding_api_key_pending);
    assert!(app.onboarding_selected_api_key.is_none());
    assert_eq!(
        app.onboarding_selected_model.as_deref(),
        Some("z-ai/glm-4.6")
    );
}

#[tokio::test]
async fn validation_change_reprompts_for_model_for_keyless_provider() {
    let mut app = test_app();
    app.onboarding_preset_id = Some("ollama".to_string());
    app.onboarding_selected_model = Some("bad-slug".to_string());

    app.handle_worker_event(WorkerEvent::ProviderValidationFailed {
        message: "unknown model".to_string(),
    });

    app.change_validation_inputs();

    assert!(app.pending_validation_retry.is_none());
    // Keyless provider (local runtime) → re-ask for the model slug instead.
    assert!(app.onboarding_custom_model_pending);
    assert!(app.onboarding_selected_model.is_none());
}
