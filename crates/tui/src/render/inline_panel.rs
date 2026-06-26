//! Inline-mode aux-panel rendering: turns the active aux panel into stacked
//! `Line` rows drawn above the composer. Split out of `render/mod.rs`;
//! `inline_aux_panel_height` lives in the sibling `geometry` module and is
//! reached via `super::`.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::theme;
use crate::app::{AuxPanelContent, TuiApp};

pub(super) struct InlineAuxPanel<'a> {
    title: &'a str,
    content: &'a AuxPanelContent,
    pub(super) height: u16,
}

pub(super) fn inline_aux_panel(app: &TuiApp) -> Option<InlineAuxPanel<'_>> {
    let panel = app.aux_panel.as_ref()?;
    let height = super::inline_aux_panel_height(app);
    if height == 0 {
        return None;
    }

    Some(InlineAuxPanel {
        title: panel.title.as_str(),
        content: &panel.content,
        height,
    })
}

pub(super) fn render_inline_aux_panel(
    frame: &mut Frame,
    area: Rect,
    app: &TuiApp,
    panel: InlineAuxPanel<'_>,
) {
    let rows = inline_aux_panel_rows(app, &panel);
    let start_y = area
        .y
        .saturating_add(area.height.saturating_sub(rows.len() as u16));

    for (index, line) in rows.into_iter().enumerate() {
        let y = start_y.saturating_add(index as u16);
        if y >= area.y.saturating_add(area.height) {
            break;
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: area.x.saturating_add(2),
                y,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }
}

fn inline_aux_panel_rows(app: &TuiApp, panel: &InlineAuxPanel<'_>) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if !panel.title.is_empty() {
        rows.push(Line::from(vec![Span::styled(
            format!("  {}", panel.title),
            theme::muted(),
        )]));
    }

    match panel.content {
        AuxPanelContent::Text(body) => {
            rows.extend(body.lines().map(|line| Line::from(format!("  {line}"))));
        }
        AuxPanelContent::SessionList(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                let marker = if index == app.aux_panel_selection {
                    "› "
                } else {
                    "  "
                };
                let tag = if entry.is_active { "current" } else { "saved" };
                rows.push(Line::from(vec![
                    Span::styled(
                        marker,
                        if marker == "› " {
                            theme::prompt()
                        } else {
                            theme::muted()
                        },
                    ),
                    Span::styled(entry.title.clone(), theme::panel_title()),
                    Span::raw("  "),
                    Span::styled(format!("[{tag}]"), theme::muted()),
                ]));
                rows.push(Line::from(vec![
                    "  ".into(),
                    Span::styled(entry.session_id.to_string(), theme::muted()),
                    Span::raw("  "),
                    Span::styled(entry.updated_at.clone(), theme::muted()),
                ]));
            }
        }
        AuxPanelContent::ModelList(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                let marker = if index == app.aux_panel_selection {
                    "› "
                } else {
                    "  "
                };
                let mut first_row = vec![
                    Span::styled(
                        marker,
                        if marker == "› " {
                            theme::prompt()
                        } else {
                            theme::muted()
                        },
                    ),
                    Span::styled(
                        entry.display_name.clone(),
                        if entry.is_current {
                            Style::new().add_modifier(Modifier::BOLD)
                        } else {
                            theme::panel_title()
                        },
                    ),
                ];
                if entry.is_current {
                    first_row.push(Span::raw("  "));
                    first_row.push(Span::styled("current", theme::muted()));
                }
                rows.push(Line::from(first_row));
                if let Some(description) = entry.description.as_deref()
                    && !description.trim().is_empty()
                {
                    rows.push(Line::from(vec![
                        "  ".into(),
                        Span::styled(description.to_string(), theme::muted()),
                    ]));
                }
            }
        }
        AuxPanelContent::ThinkingList(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                let marker = if index == app.aux_panel_selection {
                    "› "
                } else {
                    "  "
                };
                rows.push(Line::from(vec![
                    Span::styled(
                        marker,
                        if marker == "› " {
                            theme::prompt()
                        } else {
                            theme::muted()
                        },
                    ),
                    Span::styled(entry.label.clone(), theme::panel_title()),
                    Span::raw("  "),
                    Span::styled(format!("[{}]", entry.value), theme::muted()),
                ]));
                rows.push(Line::from(vec![
                    "  ".into(),
                    Span::styled(entry.description.clone(), theme::muted()),
                ]));
            }
        }
        AuxPanelContent::PresetList(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                let marker = if index == app.aux_panel_selection {
                    "› "
                } else {
                    "  "
                };
                let mut first_row = vec![
                    Span::styled(
                        marker,
                        if marker == "› " {
                            theme::prompt()
                        } else {
                            theme::muted()
                        },
                    ),
                    Span::styled(
                        entry.display_name.clone(),
                        if entry.is_current {
                            Style::new().add_modifier(Modifier::BOLD)
                        } else {
                            theme::panel_title()
                        },
                    ),
                ];
                if entry.is_current {
                    first_row.push(Span::raw("  "));
                    first_row.push(Span::styled("current", theme::muted()));
                }
                rows.push(Line::from(first_row));
                if !entry.description.is_empty() {
                    rows.push(Line::from(vec![
                        "  ".into(),
                        Span::styled(entry.description.clone(), theme::muted()),
                    ]));
                }
            }
        }
    }

    rows
}
