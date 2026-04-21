use tokio::sync::mpsc;

use lpa_core::{TurnId, TurnStatus};
use lpa_server::{ServerEvent, TurnEventPayload};

use crate::events::WorkerEvent;

use super::event_mapping::{completed_agent_message_text, handle_completed_item};

/// Mutable worker state that the server-notification dispatcher updates in place.
pub(super) struct NotificationState<'a> {
    pub(super) active_turn_id: &'a mut Option<TurnId>,
    pub(super) model: &'a mut String,
    pub(super) latest_completed_agent_message: &'a mut Option<String>,
    pub(super) turn_count: &'a mut usize,
    pub(super) total_input_tokens: &'a mut usize,
    pub(super) total_output_tokens: &'a mut usize,
    pub(super) event_tx: &'a mpsc::UnboundedSender<WorkerEvent>,
}

/// Translates one server-side notification into the matching UI-facing
/// `WorkerEvent` and updates the caller's bookkeeping state.
pub(super) fn dispatch_server_notification(
    method: &str,
    event: ServerEvent,
    state: &mut NotificationState<'_>,
) {
    match method {
        "turn/started" => {
            if let ServerEvent::TurnStarted(payload) = event {
                *state.active_turn_id = Some(payload.turn.turn_id);
                *state.model = payload.turn.model_slug.clone();
                let _ = state.event_tx.send(WorkerEvent::TurnStarted {
                    model: payload.turn.model_slug,
                });
            }
            *state.latest_completed_agent_message = None;
        }
        "item/agentMessage/delta" => {
            if let ServerEvent::ItemDelta { payload, .. } = event {
                let _ = state.event_tx.send(WorkerEvent::TextDelta(payload.delta));
            }
        }
        "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
            if let ServerEvent::ItemDelta { payload, .. } = event {
                let _ = state
                    .event_tx
                    .send(WorkerEvent::ReasoningDelta(payload.delta));
            }
        }
        "item/completed" => {
            if let ServerEvent::ItemCompleted(payload) = event {
                if let Some(text) = completed_agent_message_text(&payload) {
                    *state.latest_completed_agent_message = Some(text);
                }
                // Completed tool items are mapped into compact UI events
                // with pre-rendered summaries and previews.
                handle_completed_item(payload, state.event_tx);
            }
        }
        "turn/completed" => {
            if let ServerEvent::TurnCompleted(payload) = event {
                *state.active_turn_id = None;
                let completed = payload.turn.status == TurnStatus::Completed
                    || payload.turn.status == TurnStatus::Interrupted;
                if completed {
                    *state.turn_count += 1;
                    if let Some(usage) = &payload.turn.usage {
                        *state.total_input_tokens = usage.input_tokens as usize;
                        *state.total_output_tokens = usage.output_tokens as usize;
                    }
                    let _ = state.event_tx.send(WorkerEvent::TurnFinished {
                        stop_reason: format!("{:?}", payload.turn.status),
                        turn_count: *state.turn_count,
                        total_input_tokens: *state.total_input_tokens,
                        total_output_tokens: *state.total_output_tokens,
                    });
                    *state.latest_completed_agent_message = None;
                }
            }
        }
        "turn/usage/updated" => {
            if let ServerEvent::TurnUsageUpdated(payload) = event {
                *state.total_input_tokens = payload.total_input_tokens;
                *state.total_output_tokens = payload.total_output_tokens;
                let _ = state.event_tx.send(WorkerEvent::UsageUpdated {
                    total_input_tokens: payload.total_input_tokens,
                    total_output_tokens: payload.total_output_tokens,
                });
            }
        }
        "turn/failed" => {
            if let ServerEvent::TurnFailed(TurnEventPayload { turn, .. }) = event {
                *state.active_turn_id = None;
                let message = state
                    .latest_completed_agent_message
                    .take()
                    .unwrap_or_else(|| format!("turn failed with status {:?}", turn.status));
                if let Some(usage) = &turn.usage {
                    *state.total_input_tokens = usage.input_tokens as usize;
                    *state.total_output_tokens = usage.output_tokens as usize;
                }
                let _ = state.event_tx.send(WorkerEvent::TurnFailed {
                    message,
                    turn_count: *state.turn_count,
                    total_input_tokens: *state.total_input_tokens,
                    total_output_tokens: *state.total_output_tokens,
                });
            }
        }
        "session/title/updated" => {
            if let ServerEvent::SessionTitleUpdated(payload) = event
                && let Some(title) = payload.session.title
            {
                let _ = state.event_tx.send(WorkerEvent::SessionTitleUpdated {
                    session_id: payload.session.session_id.to_string(),
                    title,
                });
            }
        }
        "approval/requested" => {
            if let ServerEvent::ApprovalRequested(payload) = event {
                let _ = state.event_tx.send(WorkerEvent::ApprovalRequest {
                    session_id: payload.request.session_id,
                    turn_id: payload.request.turn_id,
                    approval_id: payload.approval_id.to_string(),
                    action_summary: payload.action_summary,
                    justification: payload.justification,
                });
            }
        }
        _ => {}
    }
}
