use super::*;

#[path = "runtime/input_handler.rs"]
mod input_handler;
#[path = "runtime/onboarding_flow.rs"]
mod onboarding_flow;

impl TuiApp {
    /// Runs the interactive UI until the user exits.
    pub(crate) async fn run(config: InteractiveTuiConfig) -> Result<AppExit> {
        // Spawn the worker first.
        let worker = QueryWorkerHandle::spawn(QueryWorkerConfig {
            model: config.model.clone(),
            cwd: config.cwd.clone(),
            server_env: config.server_env,
            server_log_level: config.server_log_level,
            thinking_selection: config.thinking_selection.clone(),
        });

        let mut app = Self {
            model: config.model,
            provider: config.provider,
            cwd: config.cwd,
            transcript: Vec::new(),
            input: InputBuffer::new(),
            status_message: "Ready".to_string(),
            busy: false,
            spinner_index: 0,
            scroll: 0,
            follow_output: true,
            turn_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            slash_selection: 0,
            aux_panel: None,
            pending_status_index: None,
            pending_assistant_index: None,
            pending_reasoning_index: None,
            pending_tool_items: std::collections::HashMap::new(),
            thinking_selection: config.thinking_selection,
            worker,
            model_catalog: config.model_catalog,
            saved_models: config.saved_models,
            show_model_onboarding: config.show_model_onboarding,
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
            aux_panel_selection: 0,
            last_ctrl_c_at: None,
            show_reasoning: false,
            turn_emitted_text: false,
            paste_burst: PasteBurst::default(),
            should_quit: false,
            inline_mode: false,
            terminal_width: 80,
            inline_assistant_stream_open: false,
            inline_assistant_pending_line: String::new(),
            inline_assistant_header_emitted: false,
            pending_inline_history: Vec::new(),
            pending_approval: None,
            pending_validation_retry: None,
        };

        if let Err(error) = app.validate_model_provider_selection(app.provider, &app.model) {
            app.push_item(
                TranscriptItemKind::Error,
                "Model configuration error",
                error.to_string(),
            );
            app.status_message = "Configured model does not match the active provider".to_string();
        }

        if app.show_model_onboarding {
            app.show_configure_preset_panel();
            app.onboarding_prompt = None;
            app.status_message.clear();
        }

        let mut terminal = ManagedTerminal::new(config.terminal_mode)?;
        app.inline_mode = !terminal.uses_alternate_screen();
        app.terminal_width = terminal.area().width;
        if app.inline_mode {
            terminal.insert_history_block(&crate::transcript::format_welcome_banner(
                &app.model,
                &app.cwd,
                env!("CARGO_PKG_VERSION"),
                app.terminal_width,
            ))?;
        } else {
            // Fullscreen mode: show the welcome card as the first transcript
            // item so the user always sees the branded header + quick-start tips.
            let welcome = crate::transcript::format_welcome_banner(
                &app.model,
                &app.cwd,
                env!("CARGO_PKG_VERSION"),
                app.terminal_width,
            );
            app.push_item(TranscriptItemKind::System, "Welcome", welcome);
        }
        let mut event_stream = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(80));
        let mut needs_redraw = true;

        loop {
            // Only repaint after a state change; this keeps the UI responsive and
            // avoids unnecessary full-screen redraws.
            if needs_redraw {
                if app.inline_mode {
                    terminal.set_inline_viewport_height(render::inline_viewport_height(
                        &app,
                        terminal.area().width,
                    ))?;
                    terminal.flush_pending_inline_history(&mut app.pending_inline_history)?;
                }
                terminal
                    .terminal_mut()
                    .draw(|frame| render::draw(frame, &app, app.inline_mode))?;
                needs_redraw = false;
            }

            if app.should_quit {
                break;
            }

            tokio::select! {
                maybe_event = event_stream.next() => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            // Any terminal input can affect composer state, scrolling,
                            // or selection state, so accepted input invalidates the frame.
                            if let Event::Resize(width, _) = event {
                                app.terminal_width = width;
                            } else {
                                app.terminal_width = terminal.area().width;
                            }
                            app.handle_terminal_event(event, terminal.area())?;
                            needs_redraw = true;
                        }
                        Some(Err(error)) => {
                            app.push_item(
                                TranscriptItemKind::Error,
                                "Terminal error",
                                error.to_string(),
                            );
                            app.status_message = "Terminal input error".to_string();
                            needs_redraw = true;
                        }
                        None => break,
                    }
                }
                maybe_event = app.worker.event_rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            // Worker events are the source of transcript and session updates.
                            app.handle_worker_event(event);
                            needs_redraw = true;
                        }
                        None => {
                            app.status_message = "Background worker stopped".to_string();
                            break;
                        }
                    }
                }
                _ = tick.tick() => {
                    // The tick drives spinner animation, delayed fold transitions,
                    // and buffered paste flushes that are waiting on idle time.
                    let mut redraw = app.advance_transcript_folds(Instant::now());
                    if app.busy {
                        app.spinner_index = app.spinner_index.wrapping_add(1);
                        redraw = true;
                    }
                    if app.flush_pending_paste_burst(false) {
                        redraw = true;
                    }
                    if redraw {
                        needs_redraw = true;
                    }
                }
            }
        }

        app.worker.shutdown().await?;
        Ok(AppExit {
            turn_count: app.turn_count,
            total_input_tokens: app.total_input_tokens,
            total_output_tokens: app.total_output_tokens,
        })
    }

    pub(crate) fn transcript_area(&self, full_area: Rect) -> Rect {
        let content_area = render::centered_content_area(full_area);
        let composer_height = render::composer_height(self, content_area);
        let transcript_height = render::transcript_height(self, content_area);
        let [transcript_area, _, _, _] = Layout::vertical([
            Constraint::Length(transcript_height),
            Constraint::Length(1),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .areas(content_area);
        transcript_area
    }
}
