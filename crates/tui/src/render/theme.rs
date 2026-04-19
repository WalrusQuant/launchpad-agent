use ratatui::style::{Color, Modifier, Style};

use crate::{app::TuiApp, events::TranscriptItemKind};

// Slate ramp — cool-grey palette used for all structural content. Expanded
// so the renderer can layer emphasis (title > body > metadata) within a
// single coherent tone family.
const SLATE_100: Color = Color::Rgb(234, 238, 245); // brightest — focal text
const SLATE_200: Color = Color::Rgb(213, 219, 227); // primary body text
const SLATE_300: Color = Color::Rgb(175, 184, 200); // secondary body text
const SLATE_400: Color = Color::Rgb(138, 148, 163); // muted
const SLATE_500: Color = Color::Rgb(113, 124, 141); // dim / tree connectors
const SLATE_600: Color = Color::Rgb(82, 92, 108); // separators
// Cyan ramp — the signature accent. Bright for live/active elements, mid
// for tool-call glyphs, darker for subtle rim highlights on borders.
pub(super) const CYAN_200: Color = Color::Rgb(165, 231, 241);
pub(super) const CYAN_300: Color = Color::Rgb(112, 211, 228);
pub(super) const CYAN_400: Color = Color::Rgb(80, 180, 200);
pub(super) const CYAN_600: Color = Color::Rgb(48, 122, 140);
const AMBER_300: Color = Color::Rgb(245, 201, 117);
const RED_300: Color = Color::Rgb(244, 137, 137);
const SURFACE: Color = Color::Rgb(24, 28, 34);

pub(super) fn prompt() -> Style {
    Style::new().fg(CYAN_300).add_modifier(Modifier::BOLD)
}

pub(super) fn muted() -> Style {
    Style::new().fg(SLATE_500)
}

pub(super) fn dim() -> Style {
    Style::new().fg(SLATE_600)
}

pub(super) fn accent() -> Style {
    Style::new().fg(CYAN_300)
}

pub(super) fn accent_bright() -> Style {
    Style::new().fg(CYAN_200).add_modifier(Modifier::BOLD)
}

pub(super) fn accent_dim() -> Style {
    Style::new().fg(CYAN_400)
}

/// Slate background used to "bubble" user messages so they read as a
/// distinct block vs the flat assistant/tool-call stream around them.
pub(super) const USER_BUBBLE_BG: Color = Color::Rgb(40, 46, 56);

pub(super) fn user_bubble_prefix() -> Style {
    Style::new()
        .fg(CYAN_300)
        .bg(USER_BUBBLE_BG)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn user_bubble_body() -> Style {
    Style::new().fg(SLATE_100).bg(USER_BUBBLE_BG)
}

pub(super) fn user_bubble_fill() -> Style {
    Style::new().bg(USER_BUBBLE_BG)
}

pub(super) fn selected() -> Style {
    Style::new().fg(Color::Black).bg(SLATE_200)
}

pub(super) fn panel_title() -> Style {
    muted().add_modifier(Modifier::BOLD)
}

pub(super) fn composer_border(app: &TuiApp) -> Style {
    if app.busy {
        Style::new().fg(AMBER_300).add_modifier(Modifier::BOLD)
    } else if app.onboarding_prompt.is_some() {
        prompt()
    } else {
        Style::new().fg(CYAN_600)
    }
}

pub(super) fn overlay_border() -> Style {
    Style::new().fg(CYAN_600)
}

pub(super) fn menu_surface() -> Style {
    Style::new().bg(SURFACE)
}

pub(super) fn transcript_prefix(kind: TranscriptItemKind) -> Style {
    match kind {
        TranscriptItemKind::User => Style::new().fg(CYAN_300).add_modifier(Modifier::BOLD),
        TranscriptItemKind::Assistant => Style::new().fg(CYAN_400).add_modifier(Modifier::BOLD),
        TranscriptItemKind::Reasoning => Style::new().fg(SLATE_500),
        TranscriptItemKind::ToolCall => Style::new().fg(CYAN_300).add_modifier(Modifier::BOLD),
        TranscriptItemKind::ToolResult => Style::new().fg(SLATE_500),
        TranscriptItemKind::Error => Style::new().fg(RED_300).add_modifier(Modifier::BOLD),
        TranscriptItemKind::System => Style::new().fg(SLATE_500),
        TranscriptItemKind::ApprovalPrompt => {
            Style::new().fg(AMBER_300).add_modifier(Modifier::BOLD)
        }
        TranscriptItemKind::ApprovalResolution => Style::new().fg(SLATE_400),
    }
}

pub(super) fn transcript_title(kind: TranscriptItemKind) -> Style {
    match kind {
        TranscriptItemKind::ToolCall => Style::new().fg(SLATE_100).add_modifier(Modifier::BOLD),
        TranscriptItemKind::ToolResult => Style::new().fg(SLATE_300),
        TranscriptItemKind::Reasoning => Style::new().fg(SLATE_500),
        TranscriptItemKind::Error => Style::new().fg(RED_300).add_modifier(Modifier::BOLD),
        TranscriptItemKind::System => Style::new().fg(SLATE_300).add_modifier(Modifier::BOLD),
        TranscriptItemKind::User => Style::new().fg(CYAN_200).add_modifier(Modifier::BOLD),
        TranscriptItemKind::Assistant => Style::new().fg(SLATE_100),
        TranscriptItemKind::ApprovalPrompt => {
            Style::new().fg(AMBER_300).add_modifier(Modifier::BOLD)
        }
        TranscriptItemKind::ApprovalResolution => Style::new().fg(SLATE_300),
    }
}

pub(super) fn transcript_body(kind: TranscriptItemKind) -> Style {
    match kind {
        TranscriptItemKind::User => Style::new().fg(SLATE_100),
        TranscriptItemKind::Assistant => Style::new().fg(SLATE_100),
        TranscriptItemKind::Reasoning => Style::new().fg(SLATE_400),
        TranscriptItemKind::ToolCall => Style::new().fg(SLATE_300),
        TranscriptItemKind::ToolResult => Style::new().fg(SLATE_400),
        TranscriptItemKind::Error => Style::new().fg(RED_300),
        TranscriptItemKind::System => Style::new().fg(SLATE_400),
        TranscriptItemKind::ApprovalPrompt => Style::new().fg(SLATE_200),
        TranscriptItemKind::ApprovalResolution => Style::new().fg(SLATE_400),
    }
}
