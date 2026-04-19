use super::*;

impl TuiApp {
    pub(crate) fn handle_worker_event(&mut self, event: WorkerEvent) {
        // Worker events are intentionally reduced to UI state transitions here so the
        // rendering layer stays a pure projection of application state.
        match event {
            WorkerEvent::TurnStarted { model } => {
                self.model = model;
                self.busy = true;
                self.set_turn_status_line("Thinking");
                self.status_message = "Thinking".to_string();
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.close_inline_assistant_stream();
            }
            WorkerEvent::TextDelta(text) => {
                let index = self.ensure_assistant_item();
                self.transcript[index].body.push_str(&text);
                self.status_message = "Streaming response".to_string();
                if !text.trim().is_empty() {
                    self.turn_emitted_text = true;
                }
                if self.follow_output {
                    self.scroll = 0;
                }
                self.emit_inline_assistant_delta(&text);
            }
            WorkerEvent::ReasoningDelta(text) => {
                let index = self.ensure_reasoning_item();
                self.transcript[index].body.push_str(&text);
                self.status_message = "Thinking".to_string();
                if self.follow_output {
                    self.scroll = 0;
                }
            }
            WorkerEvent::AssistantMessageCompleted(text) => {
                if !text.trim().is_empty() {
                    self.turn_emitted_text = true;
                }
                let index = self.ensure_assistant_item();
                self.transcript[index].body = text;
                self.status_message = "Streaming response".to_string();
                if self.follow_output {
                    self.scroll = 0;
                }
            }
            WorkerEvent::ReasoningCompleted(text) => {
                let index = self.ensure_reasoning_item();
                self.transcript[index].body = text;
                self.status_message = "Thinking".to_string();
                if self.follow_output {
                    self.scroll = 0;
                }
            }
            WorkerEvent::ToolCall {
                tool_use_id,
                summary,
                detail: _detail,
            } => {
                self.close_inline_assistant_stream();
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.transcript
                    .push(TranscriptItem::tool_call(summary.clone()));
                let index = self.transcript.len() - 1;
                if self.follow_output {
                    self.scroll = 0;
                }
                self.pending_tool_items.insert(tool_use_id, index);
                if self.busy {
                    self.show_turn_status_line("Thinking");
                }
                self.status_message = format!("{summary}...");
            }
            WorkerEvent::ToolResult {
                tool_use_id,
                preview,
                is_error,
                truncated: _,
            } => {
                self.close_inline_assistant_stream();
                let kind = if is_error {
                    TranscriptItemKind::Error
                } else {
                    TranscriptItemKind::ToolResult
                };
                let body = preview.trim().to_string();
                let inline_body = body.clone();
                if let Some(index) = self.pending_tool_items.remove(&tool_use_id) {
                    if let Some(item) = self.transcript.get_mut(index) {
                        if kind == TranscriptItemKind::ToolResult {
                            *item = TranscriptItem::live_tool_result(item.title.clone(), body);
                        } else {
                            *item = TranscriptItem::tool_error(item.title.clone(), body);
                        }
                    }
                    if self.follow_output {
                        self.scroll = 0;
                    }
                } else if let Some(item) = self.transcript.last_mut() {
                    if item.kind == TranscriptItemKind::ToolCall {
                        if kind == TranscriptItemKind::ToolResult {
                            *item = TranscriptItem::live_tool_result(item.title.clone(), body);
                        } else {
                            *item = TranscriptItem::tool_error(item.title.clone(), body);
                        }
                        if self.follow_output {
                            self.scroll = 0;
                        }
                    } else {
                        let title = if is_error {
                            "Tool error"
                        } else {
                            "Tool output"
                        };
                        if kind == TranscriptItemKind::ToolResult {
                            self.transcript
                                .push(TranscriptItem::live_tool_result(title, body));
                            if self.follow_output {
                                self.scroll = 0;
                            }
                        } else {
                            self.transcript
                                .push(TranscriptItem::tool_error(title, body));
                            if self.follow_output {
                                self.scroll = 0;
                            }
                        }
                    }
                } else {
                    let title = if is_error {
                        "Tool error"
                    } else {
                        "Tool output"
                    };
                    if kind == TranscriptItemKind::ToolResult {
                        self.transcript
                            .push(TranscriptItem::live_tool_result(title, body));
                        if self.follow_output {
                            self.scroll = 0;
                        }
                    } else {
                        self.transcript
                            .push(TranscriptItem::tool_error(title, body));
                        if self.follow_output {
                            self.scroll = 0;
                        }
                    }
                }
                if self.inline_mode {
                    let title = if is_error {
                        "Tool error"
                    } else {
                        "Tool output"
                    };
                    let item = if is_error {
                        TranscriptItem::tool_error(title, inline_body)
                    } else {
                        TranscriptItem::live_tool_result(title, inline_body)
                    };
                    self.emit_inline_item(&item);
                }
                if self.busy {
                    self.show_turn_status_line("Thinking");
                }
                self.status_message = if is_error {
                    "Tool returned an error".to_string()
                } else {
                    "Tool completed".to_string()
                };
            }
            WorkerEvent::UsageUpdated {
                total_input_tokens,
                total_output_tokens,
            } => {
                self.total_input_tokens = total_input_tokens;
                self.total_output_tokens = total_output_tokens;
            }
            WorkerEvent::TurnFinished {
                stop_reason,
                turn_count,
                total_input_tokens,
                total_output_tokens,
            } => {
                self.close_inline_assistant_stream();
                self.busy = false;
                self.clear_turn_status_line();
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.pending_tool_items.clear();
                self.turn_count = turn_count;
                self.total_input_tokens = total_input_tokens;
                self.total_output_tokens = total_output_tokens;
                // Explicit markers for anything the user couldn't otherwise
                // infer. A normal turn with assistant text ends silently —
                // the composer lighting up signals readiness. But we DO
                // surface a marker when:
                //   - the turn was interrupted (Ctrl+C), or
                //   - the turn ended with no assistant text at all, which
                //     otherwise looks indistinguishable from the UI hanging.
                match stop_reason.as_str() {
                    "Interrupted" => {
                        self.push_item(TranscriptItemKind::System, "Interrupted", "");
                    }
                    "MaxTokens" | "length" => {
                        self.push_item(
                            TranscriptItemKind::System,
                            "Max tokens reached",
                            "The turn stopped because the model hit its output limit. \
                             Send another message to continue, or run /configure to \
                             pick a model with more output budget."
                                .to_string(),
                        );
                    }
                    _ if !self.turn_emitted_text => {
                        self.push_item(
                            TranscriptItemKind::System,
                            "No response",
                            format!(
                                "The agent ended its turn without a message \
                                 (reason: {stop_reason}). Send another message \
                                 to continue, or press Ctrl+C to reset.",
                            ),
                        );
                    }
                    _ => {}
                }
                self.status_message = format!("Turn completed ({stop_reason})");
            }
            WorkerEvent::TurnFailed {
                message,
                turn_count,
                total_input_tokens,
                total_output_tokens,
            } => {
                self.close_inline_assistant_stream();
                self.busy = false;
                self.clear_turn_status_line();
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.pending_tool_items.clear();
                self.turn_count = turn_count;
                self.total_input_tokens = total_input_tokens;
                self.total_output_tokens = total_output_tokens;
                self.push_item(TranscriptItemKind::Error, "Error", message);
                self.status_message = "Query failed; see error above".to_string();
            }
            WorkerEvent::ProviderValidationSucceeded { reply_preview } => {
                self.close_inline_assistant_stream();
                self.busy = false;
                self.push_item(
                    TranscriptItemKind::System,
                    "Configure",
                    format!("Validation reply: {reply_preview}"),
                );
                if let Err(error) = self.finish_onboarding_selection() {
                    self.push_item(
                        TranscriptItemKind::Error,
                        "Onboarding failed",
                        error.to_string(),
                    );
                    self.status_message = "Failed to save onboarding settings".to_string();
                    self.onboarding_api_key_pending = true;
                    self.onboarding_prompt = Some("api key".to_string());
                }
            }
            WorkerEvent::ProviderValidationFailed { message } => {
                self.close_inline_assistant_stream();
                self.busy = false;
                self.push_item(
                    TranscriptItemKind::Error,
                    "Validation failed",
                    message.clone(),
                );
                self.input.clear();
                self.status_message = format!("Validation failed: {message}");

                // Preset flow: the API key already worked to reach the server,
                // so the likely culprit is a bad model slug. Re-ask for the
                // model, preserving the key. Press Esc to fully restart.
                if let Some(preset_id) = self.onboarding_preset_id.clone() {
                    let label = lpa_core::preset_by_id(&preset_id)
                        .map(|p| p.display_name.to_string())
                        .unwrap_or(preset_id);
                    self.onboarding_api_key_pending = false;
                    self.onboarding_custom_model_pending = true;
                    self.onboarding_selected_model = None;
                    self.onboarding_prompt = Some(format!(
                        "retry model slug for {label} (Esc to cancel)"
                    ));
                    self.push_item(
                        TranscriptItemKind::System,
                        "Try a different model",
                        "Your API key reached the provider. Try a different \
                         model slug — or press Esc to start over."
                            .to_string(),
                    );
                } else {
                    // Legacy flow: re-ask for api key as before.
                    self.onboarding_api_key_pending = true;
                    self.onboarding_prompt = Some("api key".to_string());
                }
            }
            WorkerEvent::SessionsListed { sessions } => {
                self.close_inline_assistant_stream();
                if sessions.is_empty() {
                    self.show_aux_panel("Sessions", "No sessions found");
                } else {
                    self.show_session_panel(sessions);
                }
                self.status_message = "Sessions loaded".to_string();
            }
            WorkerEvent::SkillsListed { body } => {
                self.close_inline_assistant_stream();
                self.show_aux_panel("Skills", body);
                self.status_message = "Skills loaded".to_string();
            }
            WorkerEvent::NewSessionPrepared => {
                self.close_inline_assistant_stream();
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.pending_status_index = None;
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.pending_tool_items.clear();
                self.busy = false;
                self.total_input_tokens = 0;
                self.total_output_tokens = 0;
                self.transcript.clear();
                self.follow_output = true;
                self.scroll = 0;
                self.status_message = "New session ready; send a prompt to start it".to_string();
                self.emit_inline_system_note("New session ready; send a prompt to start it");
            }
            WorkerEvent::SessionSwitched {
                session_id,
                title,
                model,
                total_input_tokens,
                total_output_tokens,
                history_items,
                loaded_item_count,
            } => {
                if let Some(model) = model {
                    self.model = model;
                }
                self.total_input_tokens = total_input_tokens;
                self.total_output_tokens = total_output_tokens;
                self.aux_panel = None;
                self.aux_panel_selection = 0;
                self.pending_status_index = None;
                self.pending_assistant_index = None;
                self.pending_reasoning_index = None;
                self.busy = false;
                self.transcript = history_items;
                self.pending_tool_items.clear();
                self.follow_output = true;
                self.scroll = 0;
                self.status_message = format!("Active session: {session_id}");
                if self.transcript.is_empty() {
                    let message = format!(
                        "switched to {}\ntitle: {}\nloaded items: {}",
                        session_id,
                        title.unwrap_or_else(|| "(untitled)".to_string()),
                        loaded_item_count
                    );
                    self.push_item(TranscriptItemKind::System, "Session", message.clone());
                } else {
                    self.emit_inline_session_history();
                }
            }
            WorkerEvent::SessionRenamed { session_id, title } => {
                self.push_item(
                    TranscriptItemKind::System,
                    "Session",
                    format!("renamed {} to {}", session_id, title),
                );
                self.status_message = "Session renamed".to_string();
            }
            WorkerEvent::SessionTitleUpdated { session_id, title } => {
                if let Some(AuxPanel {
                    content: AuxPanelContent::SessionList(entries),
                    ..
                }) = self.aux_panel.as_mut()
                    && let Some(entry) = entries
                        .iter_mut()
                        .find(|entry| entry.session_id.to_string() == session_id)
                {
                    entry.title = title.clone();
                }
                self.status_message = format!("Session titled: {title}");
            }
            WorkerEvent::ApprovalRequest {
                session_id,
                turn_id,
                approval_id,
                action_summary,
                justification,
            } => {
                self.close_inline_assistant_stream();
                let body = if justification.trim().is_empty() {
                    format!("{action_summary}\n\nPress [y] approve, [n] deny, [Esc] cancel")
                } else {
                    format!(
                        "{action_summary}\n\n{justification}\n\nPress [y] approve, [n] deny, [Esc] cancel"
                    )
                };
                let transcript_index = self.push_item(
                    TranscriptItemKind::ApprovalPrompt,
                    "Approval required",
                    body,
                );
                self.pending_approval = Some(crate::app::PendingApprovalState {
                    session_id,
                    turn_id,
                    approval_id: smol_str::SmolStr::new(&approval_id),
                    transcript_index,
                });
                self.status_message = "Waiting for approval (y/n)".to_string();
            }
            WorkerEvent::ApprovalResolved {
                approval_id,
                outcome,
            } => {
                let note = format!("Resolved approval {approval_id} -> {outcome}");
                if let Some(state) = self.pending_approval.as_ref()
                    && state.approval_id.as_str() == approval_id
                {
                    let transcript_index = state.transcript_index;
                    if let Some(item) = self.transcript.get_mut(transcript_index) {
                        item.kind = TranscriptItemKind::ApprovalResolution;
                        item.title = format!("Approval {outcome}");
                        item.body = note.clone();
                    }
                    self.pending_approval = None;
                } else {
                    self.push_item(TranscriptItemKind::ApprovalResolution, "Approval", note);
                }
                self.status_message = format!("Approval {outcome}");
            }
        }
    }

    pub(crate) fn ensure_assistant_item(&mut self) -> usize {
        if let Some(index) = self.pending_assistant_index {
            return index;
        }

        self.transcript.push(TranscriptItem::new(
            TranscriptItemKind::Assistant,
            "Assistant",
            String::new(),
        ));
        let index = self.transcript.len() - 1;
        self.pending_assistant_index = Some(index);
        index
    }

    pub(crate) fn ensure_reasoning_item(&mut self) -> usize {
        if let Some(index) = self.pending_reasoning_index {
            return index;
        }

        self.transcript.push(TranscriptItem::new(
            TranscriptItemKind::Reasoning,
            "Reasoning",
            String::new(),
        ));
        let index = self.transcript.len() - 1;
        self.pending_reasoning_index = Some(index);
        index
    }

    pub(crate) fn push_item(
        &mut self,
        kind: TranscriptItemKind,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> usize {
        let item = TranscriptItem::new(kind, title, body);
        if self.inline_mode {
            self.emit_inline_item(&item);
        }
        self.transcript.push(item);
        if self.follow_output {
            self.scroll = 0;
        }
        self.transcript.len() - 1
    }

    pub(crate) fn advance_transcript_folds(&mut self, now: Instant) -> bool {
        // Tool results compact over time so long outputs briefly stay readable before
        // collapsing to a smaller transcript footprint.
        let mut changed = false;
        for item in &mut self.transcript {
            if item.advance_fold(now) {
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn set_turn_status_line(&mut self, title: impl Into<String>) {
        if let Some(index) = self.pending_status_index
            && let Some(item) = self.transcript.get_mut(index)
        {
            item.title = title.into();
            item.body.clear();
        }
    }

    pub(crate) fn show_turn_status_line(&mut self, title: impl Into<String>) {
        self.clear_turn_status_line();
        self.pending_status_index =
            Some(self.push_item(TranscriptItemKind::System, title.into(), ""));
    }

    pub(crate) fn clear_turn_status_line(&mut self) {
        if let Some(index) = self.pending_status_index.take() {
            if index < self.transcript.len() {
                self.transcript.remove(index);
            }
            if let Some(pending_assistant_index) = self.pending_assistant_index {
                if pending_assistant_index > index {
                    self.pending_assistant_index = Some(pending_assistant_index - 1);
                } else if pending_assistant_index == index {
                    self.pending_assistant_index = None;
                }
            }
            if let Some(pending_reasoning_index) = self.pending_reasoning_index {
                if pending_reasoning_index > index {
                    self.pending_reasoning_index = Some(pending_reasoning_index - 1);
                } else if pending_reasoning_index == index {
                    self.pending_reasoning_index = None;
                }
            }
            for pending_tool_index in self.pending_tool_items.values_mut() {
                if *pending_tool_index > index {
                    *pending_tool_index -= 1;
                }
            }
        }
    }

    fn emit_inline_item(&mut self, item: &TranscriptItem) {
        if !self.inline_mode {
            return;
        }

        let rendered = crate::transcript::format_item(self.terminal_width.max(24), item);
        self.pending_inline_history.push(rendered);
    }

    fn emit_inline_system_note(&mut self, message: &str) {
        if self.inline_mode {
            let note = TranscriptItem::new(TranscriptItemKind::System, "Session", message);
            self.emit_inline_item(&note);
        }
    }

    fn emit_inline_session_history(&mut self) {
        if self.inline_mode {
            self.pending_inline_history
                .push(crate::transcript::format_session_history(
                    self.terminal_width.max(24),
                    &self.transcript,
                ));
        }
    }

    fn emit_inline_assistant_delta(&mut self, delta: &str) {
        if !self.inline_mode {
            return;
        }

        if !self.inline_assistant_stream_open {
            self.inline_assistant_stream_open = true;
        }
        self.inline_assistant_pending_line.push_str(delta);

        let mut completed_lines = Vec::new();
        while let Some(newline_index) = self.inline_assistant_pending_line.find('\n') {
            let line = self.inline_assistant_pending_line[..newline_index].to_string();
            completed_lines.push(line);
            self.inline_assistant_pending_line =
                self.inline_assistant_pending_line[newline_index + 1..].to_string();
        }

        if !completed_lines.is_empty() {
            self.pending_inline_history
                .push(crate::transcript::format_assistant_stream_chunk(
                    self.terminal_width.max(24),
                    &completed_lines,
                    !self.inline_assistant_header_emitted,
                ));
            self.inline_assistant_header_emitted = true;
        }

        let (wrapped_lines, remainder) = crate::transcript::split_assistant_pending_line(
            self.terminal_width.max(24),
            &self.inline_assistant_pending_line,
            !self.inline_assistant_header_emitted,
        );
        if !wrapped_lines.is_empty() {
            self.pending_inline_history
                .push(crate::transcript::format_assistant_stream_chunk(
                    self.terminal_width.max(24),
                    &wrapped_lines,
                    !self.inline_assistant_header_emitted,
                ));
            self.inline_assistant_header_emitted = true;
            self.inline_assistant_pending_line = remainder;
        }
    }

    pub(crate) fn close_inline_assistant_stream(&mut self) {
        if self.inline_mode && self.inline_assistant_stream_open {
            if !self.inline_assistant_pending_line.is_empty() {
                let trailing_line = std::mem::take(&mut self.inline_assistant_pending_line);
                self.pending_inline_history
                    .push(crate::transcript::format_assistant_stream_chunk(
                        self.terminal_width.max(24),
                        &[trailing_line],
                        !self.inline_assistant_header_emitted,
                    ));
                self.inline_assistant_header_emitted = true;
            } else if !self.inline_assistant_header_emitted
                && let Some(index) = self.pending_assistant_index
                && let Some(item) = self.transcript.get(index).cloned()
            {
                self.emit_inline_item(&item);
            }
        }
        self.inline_assistant_stream_open = false;
        self.inline_assistant_pending_line.clear();
        self.inline_assistant_header_emitted = false;
    }
}

pub(crate) fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        "(empty)".to_string()
    } else {
        "*".repeat(value.chars().count().min(8))
    }
}

/// Masks an API key showing only the last 4 characters for config summaries.
/// For keys <= 4 chars, returns `****` to avoid fully exposing short tokens.
pub(crate) fn mask_with_suffix(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len().max(4));
    }
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("***{suffix}")
}
