//! Terminal input handling for the TUI app: key/mouse event dispatch,
//! paste-burst flushing, and Ctrl-C handling. Split out of `runtime.rs`.

use super::super::*;

impl TuiApp {
    pub(crate) fn handle_terminal_event(
        &mut self,
        event: Event,
        terminal_area: Rect,
    ) -> Result<()> {
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                // Flush buffered paste text before any navigation or command key so
                // mixed keyboard and paste input stays in the expected order.
                if !matches!(key.code, KeyCode::Char(_) | KeyCode::Enter) {
                    self.flush_pending_paste_burst(true);
                }
                self.handle_key(key, terminal_area)
            }
            Event::Paste(text) => {
                self.flush_pending_paste_burst(true);
                self.input.insert_str(&text);
                self.reset_slash_selection();
                self.aux_panel = None;
                self.aux_panel_selection = 0;
            }
            Event::Resize(_, _) => {}
            Event::Mouse(mouse) => {
                if self.inline_mode {
                    return Ok(());
                }
                self.flush_pending_paste_burst(true);
                use crossterm::event::MouseEventKind;
                match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        if self.follow_output {
                            self.scroll =
                                render::get_max_scroll(self, self.transcript_area(terminal_area));
                            self.follow_output = false;
                        }
                        self.scroll = self.scroll.saturating_add(1);
                    }
                    MouseEventKind::ScrollUp => {
                        if self.follow_output {
                            self.scroll =
                                render::get_max_scroll(self, self.transcript_area(terminal_area));
                            self.follow_output = false;
                        }
                        self.scroll = self.scroll.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent, terminal_area: Rect) {
        // If an approval is waiting and the composer is empty, y/n/Esc resolve the
        // prompt. When the composer has text, we fall through so typing is not
        // hijacked.
        if self.pending_approval.is_some() && self.input.is_blank() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.submit_pending_approval(lpa_protocol::ApprovalDecisionValue::Approve);
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.submit_pending_approval(lpa_protocol::ApprovalDecisionValue::Deny);
                    return;
                }
                KeyCode::Esc => {
                    self.submit_pending_approval(lpa_protocol::ApprovalDecisionValue::Cancel);
                    return;
                }
                _ => {}
            }
        }
        // If a validation-failure retry is pending and the composer is empty,
        // r/s/c/Esc drive the decision. Same composer rule as approvals: if
        // the user has typed something, don't hijack their input.
        if self.pending_validation_retry.is_some() && self.input.is_blank() {
            match key.code {
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    self.retry_validation();
                    return;
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.skip_validation_and_save();
                    return;
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    self.change_validation_inputs();
                    return;
                }
                KeyCode::Esc => {
                    self.push_item(
                        TranscriptItemKind::System,
                        "Configure",
                        "Onboarding cancelled.".to_string(),
                    );
                    self.pending_validation_retry = None;
                    self.exit_onboarding();
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_ctrl_c();
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.transcript.clear();
                self.pending_status_index = None;
                self.pending_assistant_index = None;
                self.status_message = "Transcript cleared".to_string();
                if self.inline_mode {
                    self.close_inline_assistant_stream();
                    self.pending_inline_history
                        .push("\n^L clear screen\n".to_string());
                    print!("\x1b[2J\x1b[H");
                    let mut stdout = std::io::stdout();
                    let _ = std::io::Write::flush(&mut stdout);
                }
            }
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                self.flush_pending_paste_burst(true);
                self.input.insert_char('\n');
            }
            KeyCode::Enter if !self.busy => {
                // Enter has three roles depending on current state:
                // accept a pasted multiline burst, execute a slash command, or submit.
                if self.paste_burst.push_newline(Instant::now()) {
                    return;
                }
                self.flush_pending_paste_burst(true);
                if self.has_slash_suggestions() && self.try_apply_slash_suggestion() {
                    let prompt = self.input.take();
                    if let Err(error) = self.handle_submission(prompt) {
                        self.push_item(
                            TranscriptItemKind::Error,
                            "Submit failed",
                            error.to_string(),
                        );
                        self.status_message = "Failed to submit prompt".to_string();
                    }
                    return;
                }
                if self.try_accept_aux_panel_selection() {
                    return;
                }
                let prompt = self.input.take();
                if let Err(error) = self.handle_submission(prompt) {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Submit failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to submit prompt".to_string();
                }
            }
            KeyCode::Backspace if self.has_selectable_aux_panel() && self.input.is_blank() => {}
            KeyCode::Backspace => {
                self.flush_pending_paste_burst(true);
                self.input.backspace();
                self.reset_slash_selection();
                self.aux_panel = None;
                self.aux_panel_selection = 0;
            }
            KeyCode::Delete if self.has_selectable_aux_panel() && self.input.is_blank() => {}
            KeyCode::Delete => {
                self.flush_pending_paste_burst(true);
                self.input.delete();
                self.reset_slash_selection();
                self.aux_panel = None;
                self.aux_panel_selection = 0;
            }
            KeyCode::Tab if self.try_apply_slash_suggestion() => {}
            KeyCode::Left => {
                self.flush_pending_paste_burst(true);
                self.input.move_left();
            }
            KeyCode::Right => {
                self.flush_pending_paste_burst(true);
                self.input.move_right();
            }
            KeyCode::Home => {
                self.flush_pending_paste_burst(true);
                self.input.move_home();
                self.scroll = 0;
                self.follow_output = false;
            }
            KeyCode::End => {
                self.flush_pending_paste_burst(true);
                self.input.move_end();
                self.follow_output = true;
            }
            KeyCode::Up => {
                if self.has_selectable_aux_panel() {
                    self.move_aux_panel_selection(-1);
                } else if self.has_slash_suggestions() {
                    self.move_slash_selection(-1);
                } else if !self.inline_mode {
                    if self.follow_output {
                        self.scroll =
                            render::get_max_scroll(self, self.transcript_area(terminal_area));
                        self.follow_output = false;
                    }
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if self.has_selectable_aux_panel() {
                    self.move_aux_panel_selection(1);
                } else if self.has_slash_suggestions() {
                    self.move_slash_selection(1);
                } else if !self.inline_mode {
                    if self.follow_output {
                        self.scroll =
                            render::get_max_scroll(self, self.transcript_area(terminal_area));
                        self.follow_output = false;
                    }
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            KeyCode::PageUp => {
                if !self.inline_mode {
                    if self.follow_output {
                        self.scroll =
                            render::get_max_scroll(self, self.transcript_area(terminal_area));
                        self.follow_output = false;
                    }
                    self.scroll = self.scroll.saturating_sub(10);
                }
            }
            KeyCode::PageDown => {
                if !self.inline_mode {
                    if self.follow_output {
                        self.scroll =
                            render::get_max_scroll(self, self.transcript_area(terminal_area));
                        self.follow_output = false;
                    }
                    self.scroll = self.scroll.saturating_add(10);
                }
            }
            KeyCode::Esc => {
                self.flush_pending_paste_burst(true);
                if self.has_slash_suggestions() {
                    self.dismiss_slash_popup();
                    self.dismiss_aux_panel();
                } else if !self.handle_escape() {
                    self.input.clear();
                    self.reset_slash_selection();
                    self.dismiss_aux_panel();
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.is_onboarding_model_picker_open()
                    && self.input.is_blank() =>
            {
                if matches!(ch, 'c' | 'C') {
                    self.begin_custom_model_onboarding();
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.paste_burst.push_char(ch, Instant::now()) {
                    return;
                }
                self.input.insert_char(ch);
                self.reset_slash_selection();
                self.dismiss_aux_panel();
            }
            _ => {}
        }
    }

    pub(crate) fn flush_pending_paste_burst(&mut self, force: bool) -> bool {
        let Some(text) = self.paste_burst.take_if_due(Instant::now(), force) else {
            return false;
        };
        // Insert the paste as one batch so a terminal paste behaves like a single
        // editing action instead of a sequence of character events.
        self.input.insert_str(&text);
        self.reset_slash_selection();
        self.aux_panel = None;
        self.aux_panel_selection = 0;
        true
    }

    pub(crate) fn handle_ctrl_c(&mut self) {
        const EXIT_CONFIRM_WINDOW: Duration = Duration::from_secs(2);

        let now = Instant::now();
        // The first Ctrl+C interrupts a running turn or arms exit confirmation.
        // A second press within the window exits the app.
        if self
            .last_ctrl_c_at
            .is_some_and(|previous| now.duration_since(previous) <= EXIT_CONFIRM_WINDOW)
        {
            self.should_quit = true;
            self.status_message = "Exiting".to_string();
            return;
        }

        self.last_ctrl_c_at = Some(now);
        if self.busy {
            if let Err(error) = self.worker.interrupt_turn() {
                self.push_item(
                    TranscriptItemKind::Error,
                    "Interrupt failed",
                    error.to_string(),
                );
                self.status_message = "Failed to interrupt active turn".to_string();
                return;
            }
            self.status_message =
                "Interrupt requested. Press Ctrl+C again within 2s to exit.".to_string();
        } else {
            self.status_message = "Press Ctrl+C again within 2s to exit.".to_string();
        }
    }
}
