//! Pure geometry and sizing helpers for the render layer: overlay/popup
//! placement and aux-panel height calculations. Split out of `render/mod.rs`
//! to keep layout math separate from the drawing code. Overlay-size constants
//! and `inline_model_panel_height` live in the parent module and are reached
//! via `super::`.

use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    widgets::{Block, Borders, Padding},
};

use super::theme;
use super::{
    BRAND_HEADER_HEIGHT, MAX_LIST_OVERLAY_HEIGHT, MAX_ONBOARDING_LIST_OVERLAY_HEIGHT,
    MAX_OVERLAY_WIDTH, MAX_TEXT_OVERLAY_HEIGHT, MIN_OVERLAY_WIDTH, ONBOARDING_OVERLAY_WIDTH,
};
use crate::app::{AuxPanelContent, TuiApp};
use crate::events::ThinkingListEntry;

pub(super) fn overlay_block(title: &str, hide_title: bool) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::overlay_border())
        .padding(Padding::horizontal(1));
    if hide_title || title.is_empty() {
        block.title(" Esc back ")
    } else {
        block.title(format!(" {title} "))
    }
}

pub(super) fn bottom_popup_area(base_area: Rect, desired_height: u16, item_count: usize) -> Rect {
    let width = base_area.width.clamp(MIN_OVERLAY_WIDTH, MAX_OVERLAY_WIDTH);
    let height = desired_height
        .max(4)
        .min(base_area.height.saturating_sub(1).max(4))
        .min(if item_count == 0 {
            MAX_TEXT_OVERLAY_HEIGHT
        } else {
            MAX_LIST_OVERLAY_HEIGHT
        });
    Rect {
        x: base_area.x + base_area.width.saturating_sub(width),
        y: base_area.y + base_area.height.saturating_sub(height),
        width,
        height,
    }
}

pub(super) fn composer_popup_area(
    content_area: Rect,
    composer_area: Rect,
    desired_height: u16,
    item_count: usize,
) -> Rect {
    let width = composer_area
        .width
        .clamp(MIN_OVERLAY_WIDTH, MAX_OVERLAY_WIDTH);
    let height = desired_height
        .max(4)
        .min(content_area.height.saturating_sub(1).max(4))
        .min(if item_count == 0 {
            MAX_TEXT_OVERLAY_HEIGHT
        } else {
            MAX_LIST_OVERLAY_HEIGHT
        });
    Rect {
        x: composer_area.x,
        y: composer_area.y.saturating_sub(height),
        width,
        height,
    }
}

pub(super) fn centered_popup_area(base_area: Rect, desired_height: u16, item_count: usize) -> Rect {
    let width = base_area.width.clamp(MIN_OVERLAY_WIDTH, MAX_OVERLAY_WIDTH);
    let height = desired_height
        .max(4)
        .min(base_area.height.saturating_sub(2).max(4))
        .min(if item_count == 0 {
            MAX_TEXT_OVERLAY_HEIGHT
        } else {
            MAX_LIST_OVERLAY_HEIGHT
        });
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(base_area);
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    area
}

pub(super) fn onboarding_popup_area(base_area: Rect, desired_height: u16) -> Rect {
    let width = base_area.width.clamp(56, ONBOARDING_OVERLAY_WIDTH);
    let y = base_area.y.saturating_add(BRAND_HEADER_HEIGHT);
    let available_height = base_area
        .height
        .saturating_sub(BRAND_HEADER_HEIGHT)
        .saturating_sub(1)
        .max(8);
    let height = desired_height
        .max(8)
        .min(available_height)
        .min(MAX_ONBOARDING_LIST_OVERLAY_HEIGHT);
    Rect {
        x: base_area.x,
        y,
        width,
        height,
    }
}

pub(super) fn text_panel_height(body: &str) -> u16 {
    body.lines()
        .count()
        .saturating_add(2)
        .clamp(4, MAX_TEXT_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn session_panel_height(entries: &[crate::events::SessionListEntry]) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(2)
        .clamp(4, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn thinking_panel_height(entries: &[ThinkingListEntry]) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(2)
        .clamp(4, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn preset_panel_height(entries: &[crate::app::PresetListEntry]) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(2)
        .clamp(4, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn inline_preset_panel_height(
    entries: &[crate::app::PresetListEntry],
    title: &str,
) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(usize::from(!title.is_empty()))
        .clamp(2, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn inline_text_panel_height(body: &str, title: &str) -> u16 {
    body.lines()
        .count()
        .saturating_add(usize::from(!title.is_empty()))
        .clamp(2, MAX_TEXT_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn inline_session_panel_height(
    entries: &[crate::events::SessionListEntry],
    title: &str,
) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(usize::from(!title.is_empty()))
        .clamp(2, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn inline_thinking_panel_height(entries: &[ThinkingListEntry], title: &str) -> u16 {
    entries
        .len()
        .saturating_mul(2)
        .saturating_add(usize::from(!title.is_empty()))
        .clamp(2, MAX_LIST_OVERLAY_HEIGHT as usize) as u16
}

pub(super) fn inline_aux_panel_height(app: &TuiApp) -> u16 {
    let Some(panel) = app.aux_panel.as_ref() else {
        return 0;
    };
    if app.show_model_onboarding {
        return 0;
    }

    match &panel.content {
        AuxPanelContent::Text(body) => inline_text_panel_height(body, &panel.title),
        AuxPanelContent::SessionList(entries) => inline_session_panel_height(entries, &panel.title),
        AuxPanelContent::ThinkingList(entries) => {
            inline_thinking_panel_height(entries, &panel.title)
        }
        AuxPanelContent::ModelList(entries) => entries
            .len()
            .saturating_mul(2)
            .saturating_add(usize::from(!panel.title.is_empty()))
            .clamp(2, 8) as u16,
        AuxPanelContent::PresetList(entries) => inline_preset_panel_height(entries, &panel.title),
    }
}

pub(super) fn aux_panel_height(app: &TuiApp) -> u16 {
    let Some(panel) = app.aux_panel.as_ref() else {
        return 0;
    };
    if app.show_model_onboarding {
        return 0;
    }

    match &panel.content {
        AuxPanelContent::Text(body) => text_panel_height(body),
        AuxPanelContent::SessionList(entries) => session_panel_height(entries),
        AuxPanelContent::ThinkingList(entries) => thinking_panel_height(entries),
        AuxPanelContent::ModelList(entries) => super::inline_model_panel_height(entries),
        AuxPanelContent::PresetList(entries) => preset_panel_height(entries),
    }
}
