use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
};
use std::path::Path;

use textwrap::Options;

use crate::{
    app::TuiApp,
    events::{TranscriptItem, TranscriptItemKind},
};

use super::{markdown, theme};

pub(super) fn render(app: &TuiApp, area_width: u16, area_height: u16) -> Paragraph<'static> {
    let content = transcript_text(app, area_width.max(1));
    let max_scroll = rendered_line_count(content.clone(), area_width)
        .saturating_sub(area_height as usize) as u16;
    let paragraph = Paragraph::new(content).wrap(Wrap { trim: false });
    let scroll = if app.follow_output {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };

    paragraph.scroll((scroll, 0))
}

pub(super) fn line_count(app: &TuiApp, inner_width: u16) -> u16 {
    rendered_line_count(transcript_text(app, inner_width.max(1)), inner_width.max(1)) as u16
}

fn rendered_line_count(text: Text<'static>, width: u16) -> usize {
    let width = usize::from(width.max(1));
    text.lines
        .iter()
        .map(|line| {
            let rendered = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            if rendered.is_empty() {
                1
            } else {
                textwrap::wrap(&rendered, Options::new(width).break_words(false))
                    .len()
                    .max(1)
            }
        })
        .sum()
}

fn transcript_text(app: &TuiApp, inner_width: u16) -> Text<'static> {
    let mut lines = Vec::new();

    if app.transcript.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "No conversation yet. Ask Launchpad Agent to inspect code, explain behavior, or make changes.",
            theme::muted(),
        )]));
        return Text::from(lines);
    }

    let mut previous_kind = None;
    for item in &app.transcript {
        if previous_kind.is_some()
            && (matches!(item.kind, TranscriptItemKind::User)
                || matches!(previous_kind, Some(TranscriptItemKind::User))
                || matches!(item.kind, TranscriptItemKind::ToolCall)
                || matches!(item.kind, TranscriptItemKind::Error)
                || matches!(item.kind, TranscriptItemKind::System))
        {
            lines.push(Line::from(""));
        }
        append_transcript_item(
            &mut lines,
            item,
            app.spinner_index,
            inner_width,
            &app.cwd,
            app.show_reasoning,
        );
        previous_kind = Some(item.kind);
    }
    Text::from(lines)
}

fn append_transcript_item(
    lines: &mut Vec<Line<'static>>,
    item: &TranscriptItem,
    spinner_index: usize,
    inner_width: u16,
    cwd: &Path,
    show_reasoning: bool,
) {
    match item.kind {
        TranscriptItemKind::User => {
            append_user_bubble(lines, item, inner_width);
        }
        TranscriptItemKind::Assistant => {
            append_markdown_message(lines, item, cwd);
        }
        TranscriptItemKind::Reasoning => {
            if show_reasoning {
                append_wrapped_title(lines, &item.title, item.kind, inner_width);
                append_transcript_body(lines, item, inner_width);
            } else {
                // Collapsed: show a single compact placeholder so the viewport
                // stays stable while the model streams reasoning tokens.
                let chars = item.body.chars().count();
                let label = if chars == 0 {
                    "thinking…".to_string()
                } else {
                    format!("thought ({chars} chars) — /reasoning to show")
                };
                append_wrapped_styled_text(lines, &label, "∙ ", "  ", inner_width, theme::dim());
            }
        }
        TranscriptItemKind::System if item.title == "Thinking" => {
            let spinner = ["⠋", "⠙", "⠹", "⠸", "⠴", "⠦"][spinner_index % 6];
            append_wrapped_styled_text(
                lines,
                &format!("{spinner}  thinking"),
                "",
                "  ",
                inner_width,
                theme::accent(),
            );
        }
        TranscriptItemKind::System if item.title == "Interrupted" => {
            append_wrapped_styled_text(
                lines,
                "interrupted",
                "◼ ",
                "  ",
                inner_width,
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            );
        }
        TranscriptItemKind::ToolCall => {
            append_tool_call_title(lines, &item.title, inner_width);
        }
        TranscriptItemKind::ToolResult => {
            // Tool results render as a tree branch under the preceding call
            // so the reader can see call-and-response as one visual unit.
            append_tool_result(lines, item, inner_width);
        }
        TranscriptItemKind::System if item.title == "Welcome" => {
            append_welcome_banner(lines, item, inner_width);
        }
        TranscriptItemKind::System | TranscriptItemKind::Error => {
            append_wrapped_title(lines, &item.title, item.kind, inner_width);
            append_transcript_body(lines, item, inner_width);
        }
        TranscriptItemKind::ApprovalPrompt | TranscriptItemKind::ApprovalResolution => {
            append_wrapped_title(lines, &item.title, item.kind, inner_width);
            append_transcript_body(lines, item, inner_width);
        }
    }
}

fn append_markdown_message(lines: &mut Vec<Line<'static>>, item: &TranscriptItem, cwd: &Path) {
    let prefix_style = theme::transcript_prefix(TranscriptItemKind::Assistant);
    let rendered = markdown::render_markdown_lines(
        item.body.trim_end_matches('\n'),
        theme::transcript_body(item.kind),
        Some(cwd),
    );

    if rendered.is_empty() {
        lines.push(Line::from(vec![Span::styled("• ", prefix_style)]));
        return;
    }

    for (index, line) in rendered.into_iter().enumerate() {
        let prefix = if index == 0 { "• " } else { "  " };
        let mut spans = vec![Span::styled(prefix, prefix_style)];
        spans.extend(line.spans);
        lines.push(Line::from(spans).style(line.style));
    }
}

/// Renders a user message as a slate-backed "bubble" — each wrapped line gets
/// a consistent slate background across prefix, body, and right-edge padding
/// so the message reads as a visually distinct block.
fn append_user_bubble(lines: &mut Vec<Line<'static>>, item: &TranscriptItem, inner_width: u16) {
    const FIRST_PREFIX: &str = "❯ ";
    const CONTINUATION_PREFIX: &str = "  ";

    let body = item.body.trim_end_matches('\n');
    if body.is_empty() {
        return;
    }

    // Both prefixes are the same visual width (2 columns), so wrap against
    // a single width and alternate the prefix per line.
    let content_width = inner_width
        .saturating_sub(FIRST_PREFIX.chars().count() as u16)
        .max(1) as usize;
    let wrapped = textwrap::wrap(body, Options::new(content_width).break_words(false));

    for (index, segment) in wrapped.iter().enumerate() {
        let prefix = if index == 0 {
            FIRST_PREFIX
        } else {
            CONTINUATION_PREFIX
        };
        let segment_text: String = segment.as_ref().to_string();
        let prefix_width = prefix.chars().count();
        let segment_width = unicode_width::UnicodeWidthStr::width(segment_text.as_str());
        let used = prefix_width + segment_width;
        let fill = usize::from(inner_width).saturating_sub(used);

        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), theme::user_bubble_prefix()),
            Span::styled(segment_text, theme::user_bubble_body()),
            Span::styled(" ".repeat(fill), theme::user_bubble_fill()),
        ]));
    }
}

fn append_transcript_body(lines: &mut Vec<Line<'static>>, item: &TranscriptItem, inner_width: u16) {
    let body = rendered_transcript_body(item);
    if body.is_empty() {
        return;
    }

    append_wrapped_styled_text(
        lines,
        &body,
        "  └ ",
        "    ",
        inner_width,
        theme::transcript_body(item.kind),
    );
}

fn rendered_transcript_body(item: &TranscriptItem) -> String {
    match item.kind {
        TranscriptItemKind::ToolResult => match item.fold_stage {
            0 => item.body.trim_end_matches('\n').to_string(),
            1 => fold_tool_output(&item.body, 4),
            2 => fold_tool_output(&item.body, 1),
            _ => String::new(),
        },
        _ => item.body.trim_end_matches('\n').to_string(),
    }
}

fn fold_tool_output(body: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = body.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    if lines.len() <= max_lines {
        return body.trim_end_matches('\n').to_string();
    }

    let mut folded = lines[..max_lines].join("\n");
    folded.push_str("\n...");
    folded
}

/// Renders the welcome banner with per-line styling so the border and logo
/// pick up the cyan accent while context / quick-start rows stay in the
/// neutral slate body style. Splits on line prefix: anything starting with
/// `╭`, `│`, or `╰` is treated as the boxed banner and colored with the
/// cyan border accent; the ASCII-logo interior is brightened; the `❯`
/// bullets in quick-start inherit the accent glyph color.
fn append_welcome_banner(lines: &mut Vec<Line<'static>>, item: &TranscriptItem, _inner_width: u16) {
    for raw_line in item.body.lines() {
        let line = Line::from(style_welcome_line(raw_line));
        lines.push(line);
    }
}

fn style_welcome_line(raw: &str) -> Vec<Span<'static>> {
    // Box border rows: the entire line is structural — color in dim cyan.
    if raw.starts_with('╭') || raw.starts_with('╰') {
        return vec![Span::styled(raw.to_string(), theme::accent_dim())];
    }

    // Interior of the box — left/right pipes stay dim cyan, content inside
    // is the bright accent on the ASCII logo and slate on tagline rows.
    if raw.starts_with('│') {
        let inner = raw.strip_prefix('│').unwrap_or(raw);
        let inner = inner.strip_suffix('│').unwrap_or(inner);
        // Detect whether the inner content looks like the figlet logo vs
        // plain text (the logo contains `|` or `_` runs at specific places).
        // Simpler: any line that contains `__` or `|_` treats as logo.
        let is_logo_line =
            inner.contains("__") || inner.contains("|_") || inner.contains("A G E N T");
        let inner_style = if is_logo_line {
            theme::accent_bright()
        } else {
            theme::transcript_title(TranscriptItemKind::Assistant)
        };
        return vec![
            Span::styled("│".to_string(), theme::accent_dim()),
            Span::styled(inner.to_string(), inner_style),
            Span::styled("│".to_string(), theme::accent_dim()),
        ];
    }

    // Context row: `  ◆  model       x` — color the ◆ cyan, rest neutral.
    if let Some(rest) = raw.strip_prefix("  ◆  ") {
        return vec![
            Span::styled("  ◆  ".to_string(), theme::accent()),
            Span::styled(rest.to_string(), theme::muted()),
        ];
    }

    // Quick-start headers (`  Get started`) in accent.
    if raw.trim() == "Get started" {
        return vec![Span::styled(raw.to_string(), theme::accent())];
    }

    // Quick-start bullets: `    ❯ /configure   ...` — color the ❯ cyan,
    // the slash command in slate_100, the description muted.
    if let Some(rest) = raw.strip_prefix("    ❯ ") {
        let mut spans = vec![
            Span::raw("    ".to_string()),
            Span::styled("❯ ".to_string(), theme::accent()),
        ];
        // Pull out the command (up to first two spaces or end).
        if let Some(idx) = rest.find("   ") {
            let (cmd, desc) = rest.split_at(idx);
            spans.push(Span::styled(
                cmd.to_string(),
                theme::transcript_title(TranscriptItemKind::Assistant),
            ));
            spans.push(Span::styled(desc.to_string(), theme::muted()));
        } else {
            spans.push(Span::styled(
                rest.to_string(),
                theme::transcript_title(TranscriptItemKind::Assistant),
            ));
        }
        return spans;
    }

    // Default: muted neutral.
    vec![Span::styled(raw.to_string(), theme::muted())]
}

/// Tool-call headers use an open-diamond glyph + tool-name highlighted in
/// slate_100 bold, with the rest (usually the command/args) in slate_300.
/// Example: `◇ bash  ls -la /Users/adam`
fn append_tool_call_title(lines: &mut Vec<Line<'static>>, title: &str, inner_width: u16) {
    let prefix = "◇ ";
    let continuation = "  ";
    let (tool_name, rest) = split_tool_call_title(title);

    let content_width = inner_width.saturating_sub(prefix.len() as u16).max(1) as usize;
    let combined = if rest.is_empty() {
        tool_name.to_string()
    } else {
        format!("{tool_name}  {rest}")
    };
    let wrapped = textwrap::wrap(&combined, Options::new(content_width).break_words(false));
    for (index, segment) in wrapped.iter().enumerate() {
        let prefix_text = if index == 0 { prefix } else { continuation };
        let segment_text = segment.to_string();
        // Split the first wrapped line to style the tool name separately.
        if index == 0 && !rest.is_empty() && segment_text.starts_with(tool_name) {
            let remainder = segment_text[tool_name.len()..].trim_start().to_string();
            lines.push(Line::from(vec![
                Span::styled(
                    prefix_text,
                    theme::transcript_prefix(TranscriptItemKind::ToolCall),
                ),
                Span::styled(
                    tool_name.to_string(),
                    theme::transcript_title(TranscriptItemKind::ToolCall),
                ),
                Span::raw("  "),
                Span::styled(
                    remainder,
                    theme::transcript_body(TranscriptItemKind::ToolCall),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    prefix_text,
                    theme::transcript_prefix(TranscriptItemKind::ToolCall),
                ),
                Span::styled(
                    segment_text,
                    theme::transcript_body(TranscriptItemKind::ToolCall),
                ),
            ]));
        }
    }
}

/// Tool-result bodies render under an indented tree connector so call and
/// response read as a single unit. Uses a dim slate for both the branch and
/// the wrapped body so they visually sit behind the call title.
fn append_tool_result(lines: &mut Vec<Line<'static>>, item: &TranscriptItem, inner_width: u16) {
    let body = rendered_transcript_body(item);
    if body.is_empty() && item.title.is_empty() {
        return;
    }

    if !item.title.is_empty() && item.title != "Tool output" {
        lines.push(Line::from(vec![
            Span::styled(
                "  └ ",
                theme::transcript_prefix(TranscriptItemKind::ToolResult),
            ),
            Span::styled(
                item.title.clone(),
                theme::transcript_title(TranscriptItemKind::ToolResult),
            ),
        ]));
    }

    if body.is_empty() {
        return;
    }

    append_wrapped_styled_text(
        lines,
        &body,
        "  └ ",
        "    ",
        inner_width,
        theme::transcript_body(TranscriptItemKind::ToolResult),
    );
}

/// Splits a tool-call summary like `"bash: ls -la"` into `(tool_name, rest)`.
/// If no `:` is present, the whole string is treated as the tool name.
fn split_tool_call_title(title: &str) -> (&str, &str) {
    match title.find(':') {
        Some(idx) => (title[..idx].trim(), title[idx + 1..].trim()),
        None => (title.trim(), ""),
    }
}

fn append_wrapped_title(
    lines: &mut Vec<Line<'static>>,
    title: &str,
    kind: TranscriptItemKind,
    inner_width: u16,
) {
    let prefix = match kind {
        TranscriptItemKind::Error => "✕ ",
        TranscriptItemKind::ApprovalPrompt => "◆ ",
        TranscriptItemKind::ApprovalResolution => "◇ ",
        _ => "· ",
    };
    let continuation = "  ";
    let content_width = inner_width.saturating_sub(prefix.len() as u16).max(1) as usize;
    let wrapped = textwrap::wrap(title, Options::new(content_width).break_words(false));
    for (index, segment) in wrapped.iter().enumerate() {
        let prefix_text = if index == 0 { prefix } else { continuation };
        lines.push(Line::from(vec![
            Span::styled(prefix_text, theme::transcript_prefix(kind)),
            Span::styled(segment.to_string(), theme::transcript_title(kind)),
        ]));
    }
}

fn append_wrapped_styled_text(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    first_prefix: &'static str,
    continuation_prefix: &'static str,
    inner_width: u16,
    style: Style,
) {
    let prefix_kind = match first_prefix {
        "> " => TranscriptItemKind::User,
        "• " => TranscriptItemKind::Assistant,
        "  └ " => TranscriptItemKind::ToolResult,
        _ => TranscriptItemKind::System,
    };
    let prefix_style = theme::transcript_prefix(prefix_kind);
    if text.is_empty() {
        lines.push(Line::from(vec![Span::styled(first_prefix, prefix_style)]));
        return;
    }

    let first_width = inner_width.saturating_sub(first_prefix.len() as u16).max(1) as usize;
    let continuation_width = inner_width
        .saturating_sub(continuation_prefix.len() as u16)
        .max(1) as usize;
    let mut first_visual_line = true;

    for logical_line in text.split('\n') {
        let options = if first_visual_line {
            Options::new(first_width).break_words(false)
        } else {
            Options::new(continuation_width).break_words(false)
        };
        let wrapped = textwrap::wrap(logical_line, options);
        if wrapped.is_empty() {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            lines.push(Line::from(vec![Span::styled(prefix, prefix_style)]));
            first_visual_line = false;
            continue;
        }

        for segment in wrapped {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(segment.to_string(), style),
            ]));
            first_visual_line = false;
        }
    }
}
