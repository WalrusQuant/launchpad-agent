//! Builders that turn aux-panel entries into ratatui `ListItem`s. Split out of
//! `render/mod.rs` to separate data-to-widget transforms from layout/drawing.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::ListItem,
};

use super::theme;
use crate::app::TuiApp;
use crate::events::{ModelListEntry, ThinkingListEntry};

pub(super) fn session_items(entries: &[crate::events::SessionListEntry]) -> Vec<ListItem<'static>> {
    if entries.is_empty() {
        return vec![ListItem::new(Line::from(vec![Span::styled(
            "No saved sessions found.",
            theme::muted(),
        )]))];
    }

    entries
        .iter()
        .map(|entry| {
            let marker = if entry.is_active { "current" } else { "saved" };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(entry.title.clone(), theme::panel_title()),
                    Span::styled("  ", theme::muted()),
                    Span::styled(format!("[{marker}]"), theme::muted()),
                ]),
                Line::from(vec![
                    Span::styled(entry.session_id.to_string(), theme::muted()),
                    Span::styled("  ", theme::muted()),
                    Span::styled(entry.updated_at.clone(), theme::muted()),
                ]),
            ])
        })
        .collect()
}

pub(super) fn model_items(app: &TuiApp, entries: &[ModelListEntry]) -> Vec<ListItem<'static>> {
    if entries.is_empty() {
        return vec![ListItem::new(Line::from(vec![Span::styled(
            "No models available.",
            theme::muted(),
        )]))];
    }

    entries
        .iter()
        .map(|entry| {
            if app.show_model_onboarding {
                let description = entry
                    .description
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(if entry.is_custom_mode {
                        "Type a model name manually"
                    } else {
                        ""
                    });
                let title = if entry.is_current {
                    format!("{}  current", entry.display_name)
                } else {
                    entry.display_name.clone()
                };
                return ListItem::new(vec![
                    Line::from(vec![Span::styled(title, theme::panel_title())]),
                    Line::from(vec![Span::styled(description.to_string(), theme::muted())]),
                ]);
            }

            let description = entry
                .description
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(if entry.is_custom_mode {
                    "Open onboarding to add another model"
                } else {
                    "saved model"
                });
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        entry.display_name.clone(),
                        if entry.is_custom_mode {
                            theme::prompt()
                        } else if entry.is_current {
                            Style::new().add_modifier(Modifier::BOLD)
                        } else {
                            theme::panel_title()
                        },
                    ),
                    if entry.is_current {
                        Span::styled("  current", theme::muted())
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![Span::styled(description.to_string(), theme::muted())]),
            ])
        })
        .collect()
}

pub(super) fn preset_items(entries: &[crate::app::PresetListEntry]) -> Vec<ListItem<'static>> {
    if entries.is_empty() {
        return vec![ListItem::new(Line::from(vec![Span::styled(
            "No providers available.",
            theme::muted(),
        )]))];
    }

    entries
        .iter()
        .map(|entry| {
            let title_style = if entry.is_current {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                theme::panel_title()
            };
            let mut first_row = vec![Span::styled(entry.display_name.clone(), title_style)];
            if entry.is_current {
                first_row.push(Span::raw("  "));
                first_row.push(Span::styled("current", theme::muted()));
            }
            ListItem::new(vec![
                Line::from(first_row),
                Line::from(vec![Span::styled(
                    entry.description.clone(),
                    theme::muted(),
                )]),
            ])
        })
        .collect()
}

pub(super) fn thinking_items(entries: &[ThinkingListEntry]) -> Vec<ListItem<'static>> {
    if entries.is_empty() {
        return vec![ListItem::new(Line::from(vec![Span::styled(
            "No thinking options available.",
            theme::muted(),
        )]))];
    }

    entries
        .iter()
        .map(|entry| {
            let title = if entry.is_current {
                format!("{}  current", entry.label)
            } else {
                entry.label.clone()
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(title, theme::panel_title()),
                    Span::styled("  ", theme::muted()),
                    Span::styled(format!("[{}]", entry.value), theme::muted()),
                ]),
                Line::from(vec![Span::styled(
                    entry.description.clone(),
                    theme::muted(),
                )]),
            ])
        })
        .collect()
}
