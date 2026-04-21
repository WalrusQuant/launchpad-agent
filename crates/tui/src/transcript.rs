use std::fmt::Write as _;
use std::path::{MAIN_SEPARATOR, Path};

use textwrap::{Options, wrap};

use crate::events::{TranscriptItem, TranscriptItemKind};

const TOOL_RESULT_PREVIEW_LINE_LIMIT: usize = 6;

pub(crate) fn format_session_history(width: u16, items: &[TranscriptItem]) -> String {
    items.iter().map(|item| format_item(width, item)).collect()
}

pub(crate) fn format_shell_command_echo(command: &str) -> String {
    format!("\n› {command}\n")
}

pub(crate) fn format_assistant_stream_chunk(
    width: u16,
    lines: &[String],
    include_header: bool,
) -> String {
    let mut out = String::new();
    if include_header {
        out.push('\n');
        out.push_str(&separator("assistant", width));
        out.push('\n');
    }

    let first_prefix = if include_header { "assistant> " } else { "  " };
    let continuation_prefix = "  ";
    let first_width = width.saturating_sub(first_prefix.len() as u16).max(1) as usize;
    let continuation_width = width
        .saturating_sub(continuation_prefix.len() as u16)
        .max(1) as usize;
    let mut first_visual_line = true;

    for logical_line in lines {
        let wrapped = wrap_token_safe_with_first_and_subsequent_widths(
            logical_line,
            if first_visual_line {
                first_width
            } else {
                continuation_width
            },
            continuation_width,
        );
        if wrapped.is_empty() {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            let _ = writeln!(out, "{prefix}");
            first_visual_line = false;
            continue;
        }

        for segment in wrapped {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            let _ = writeln!(out, "{prefix}{segment}");
            first_visual_line = false;
        }
    }

    out
}

pub(crate) fn split_assistant_pending_line(
    width: u16,
    pending: &str,
    include_header: bool,
) -> (Vec<String>, String) {
    let first_prefix = if include_header { "assistant> " } else { "  " };
    let continuation_prefix = "  ";
    let first_width = width.saturating_sub(first_prefix.len() as u16).max(1) as usize;
    let continuation_width = width
        .saturating_sub(continuation_prefix.len() as u16)
        .max(1) as usize;

    let wrapped =
        wrap_token_safe_with_first_and_subsequent_widths(pending, first_width, continuation_width);
    if wrapped.len() <= 1 {
        return (Vec::new(), pending.to_string());
    }

    let flushed = wrapped[..wrapped.len() - 1].to_vec();
    let remainder = wrapped.last().cloned().unwrap_or_default();
    (flushed, remainder)
}

pub(crate) fn format_item(width: u16, item: &TranscriptItem) -> String {
    match item.kind {
        TranscriptItemKind::User => format_block("user", &item.body, width, Some("> ")),
        TranscriptItemKind::Assistant => {
            format_block("assistant", &item.body, width, Some("assistant> "))
        }
        TranscriptItemKind::Reasoning => format_block(
            &block_title("reasoning", item.title.trim()),
            &item.body,
            width,
            None,
        ),
        TranscriptItemKind::ToolCall => format_block(
            &format!("tool: {}", item.title.trim()),
            &item.body,
            width,
            Some("tool> "),
        ),
        TranscriptItemKind::ToolResult => {
            let body = rendered_tool_result_body(item);
            format_block("tool output", &body, width, None)
        }
        TranscriptItemKind::Error => format_block(
            &block_title("error", item.title.trim()),
            &item.body,
            width,
            None,
        ),
        TranscriptItemKind::System => format_block(
            &block_title("system", item.title.trim()),
            &item.body,
            width,
            None,
        ),
        TranscriptItemKind::ApprovalPrompt => format_block(
            &block_title("approval", item.title.trim()),
            &item.body,
            width,
            None,
        ),
        TranscriptItemKind::ApprovalResolution => format_block(
            &block_title("approval", item.title.trim()),
            &item.body,
            width,
            None,
        ),
    }
}

/// ASCII logo shown at startup and in the welcome message.
///
/// Figlet "standard" font rendering of "Launchpad" with "A G E N T" as a
/// subtitle on the final row. Six rows, ~54 columns wide so it fits in the
/// 76-column welcome box on every common terminal.
pub(crate) const LAUNCHPAD_LOGO: &str = r"  _                            _                   _
 | |    __ _ _   _ _ __   ___| |__  _ __   __ _  __| |
 | |   / _` | | | | '_ \ / __| '_ \| '_ \ / _` |/ _` |
 | |__| (_| | |_| | | | | (__| | | | |_) | (_| | (_| |
 |_____\__,_|\__,_|_| |_|\___|_| |_| .__/ \__,_|\__,_|
                                   |_|   A G E N T";

/// Renders the branded welcome screen shown at startup. Includes the
/// LAUNCHPAD AGENT ASCII logo, current model + cwd, and quick-start tips.
///
/// Structure:
/// ```text
///   ╭──────────────────────────────────────────────╮
///   │                                              │
///   │   [figlet logo]                              │
///   │                    A G E N T                 │
///   │                                              │
///   │   Any model. Any terminal.          v0.1.0   │
///   │                                              │
///   ╰──────────────────────────────────────────────╯
///
///   ◆  model       xiaomi/mimo-v2-flash
///   ◆  directory   ~/Code/code-agent
///
///   Get started
///     ❯ /configure   ...
///     ❯ /model       ...
///     ❯ /exit        ...
///
///   Or just type a message to begin.
/// ```
pub(crate) fn format_welcome_banner(model: &str, cwd: &Path, version: &str, width: u16) -> String {
    let inner_width = width.saturating_sub(2).clamp(56, 76) as usize;

    let mut out = String::new();
    out.push_str(&box_top(inner_width));
    out.push('\n');
    out.push_str(&box_blank_line(inner_width));
    out.push('\n');

    for line in LAUNCHPAD_LOGO.lines() {
        append_box_line(&mut out, &format!("   {line}"), inner_width);
    }

    append_box_line(&mut out, "", inner_width);

    // Right-align the version tag within the tagline row.
    let tagline = "Any model. Any terminal.";
    let version_tag = format!("v{version}");
    let padding =
        inner_width.saturating_sub(tagline.chars().count() + version_tag.chars().count() + 5);
    append_box_line(
        &mut out,
        &format!("   {tagline}{}{version_tag}", " ".repeat(padding.max(3))),
        inner_width,
    );

    out.push_str(&box_blank_line(inner_width));
    out.push('\n');
    out.push_str(&box_bottom(inner_width));
    out.push_str("\n\n");

    // Context — cyan diamonds before key/value rows read as bullets without
    // being loud.
    out.push_str(&format!("  ◆  model       {model}\n"));
    out.push_str(&format!("  ◆  directory   {}\n\n", abbreviate_cwd(cwd)));

    out.push_str("  Get started\n");
    out.push_str("    ❯ /configure   pick a provider (OpenRouter, OpenAI, Ollama, …)\n");
    out.push_str("    ❯ /model       switch between saved models\n");
    out.push_str("    ❯ /config      show the active config.toml\n");
    out.push_str("    ❯ /reasoning   toggle inline model reasoning\n");
    out.push_str("    ❯ /skills      browse available skills\n");
    out.push_str("    ❯ /exit        quit\n\n");
    out.push_str("  Or just type a message to begin.\n");

    out
}

fn format_block(title: &str, body: &str, width: u16, prefix: Option<&str>) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&separator(title, width));
    out.push('\n');

    let body = body.trim_end_matches('\n');
    if body.is_empty() {
        return out;
    }

    let prefix = prefix.unwrap_or("");
    append_wrapped_body(&mut out, body, width, prefix, "  ", None);
    out
}

fn append_wrapped_body(
    out: &mut String,
    body: &str,
    width: u16,
    first_prefix: &str,
    continuation_prefix: &str,
    logical_line_limit: Option<usize>,
) {
    let first_width = width.saturating_sub(first_prefix.len() as u16).max(1) as usize;
    let continuation_width = width
        .saturating_sub(continuation_prefix.len() as u16)
        .max(1) as usize;

    let logical_lines: Vec<&str> = body.split('\n').collect();
    let total_lines = logical_lines.len();
    let limit = logical_line_limit.unwrap_or(logical_lines.len());
    let mut first_visual_line = true;

    for (index, logical_line) in logical_lines.into_iter().enumerate() {
        if index >= limit {
            let remaining = total_lines.saturating_sub(limit);
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            let _ = writeln!(out, "{prefix}... {remaining} more lines folded");
            return;
        }

        let wrapped = wrap_token_safe_with_first_and_subsequent_widths(
            logical_line,
            if first_visual_line {
                first_width
            } else {
                continuation_width
            },
            continuation_width,
        );
        if wrapped.is_empty() {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            let _ = writeln!(out, "{prefix}");
            first_visual_line = false;
            continue;
        }

        for segment in wrapped {
            let prefix = if first_visual_line {
                first_prefix
            } else {
                continuation_prefix
            };
            let _ = writeln!(out, "{prefix}{segment}");
            first_visual_line = false;
        }
    }
}

fn block_title(kind: &str, title: &str) -> String {
    if title.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}: {title}")
    }
}

fn rendered_tool_result_body(item: &TranscriptItem) -> String {
    let body = item.body.trim_end_matches('\n');
    let lines: Vec<&str> = body.lines().collect();
    if lines.len() <= TOOL_RESULT_PREVIEW_LINE_LIMIT {
        return body.to_string();
    }

    let mut rendered = lines[..TOOL_RESULT_PREVIEW_LINE_LIMIT].join("\n");
    let remaining = lines.len() - TOOL_RESULT_PREVIEW_LINE_LIMIT;
    let _ = writeln!(&mut rendered, "\n... {remaining} more lines folded");
    rendered
}

fn separator(title: &str, width: u16) -> String {
    let title = title.trim();
    let prefix = if title.is_empty() {
        "---".to_string()
    } else {
        format!("--- {title} ")
    };
    let desired_width = width.max(prefix.len() as u16) as usize;
    if desired_width <= prefix.len() {
        return prefix;
    }

    let fill = "-".repeat(desired_width.saturating_sub(prefix.len()));
    format!("{prefix}{fill}")
}

fn box_top(inner_width: usize) -> String {
    format!("╭{}╮", "─".repeat(inner_width))
}

fn box_bottom(inner_width: usize) -> String {
    format!("╰{}╯", "─".repeat(inner_width))
}

fn box_blank_line(inner_width: usize) -> String {
    format!("│{}│", " ".repeat(inner_width))
}

fn append_box_line(out: &mut String, text: &str, inner_width: usize) {
    let content_width = inner_width.max(1);
    let wrapped = wrap(text, Options::new(content_width).break_words(false));
    if wrapped.is_empty() {
        let _ = writeln!(out, "│{:inner_width$}│", "");
        return;
    }

    for segment in wrapped {
        let _ = writeln!(out, "│{segment:<inner_width$}│");
    }
}

fn wrap_token_safe_with_first_and_subsequent_widths(
    text: &str,
    first_width: usize,
    subsequent_width: usize,
) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = first_width.max(1);

    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };

        if current.is_empty() {
            current = candidate;
            continue;
        }

        if candidate.len() <= current_width {
            current = candidate;
            continue;
        }

        lines.push(current);
        current = word.to_string();
        current_width = subsequent_width.max(1);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn abbreviate_cwd(cwd: &Path) -> String {
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return cwd.display().to_string();
    };
    let home = Path::new(&home);
    let Ok(stripped) = cwd.strip_prefix(home) else {
        return cwd.display().to_string();
    };

    if stripped.as_os_str().is_empty() {
        "~".to_string()
    } else {
        format!("~{}{}", MAIN_SEPARATOR, stripped.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn formats_user_transcript_block() {
        let item = TranscriptItem::new(TranscriptItemKind::User, "You", "hello world");
        let rendered = format_item(80, &item);

        assert!(rendered.contains("--- user"));
        assert!(rendered.contains("> hello world"));
    }

    #[test]
    fn formats_folded_tool_result_preview() {
        let item = TranscriptItem::new(
            TranscriptItemKind::ToolResult,
            "Tool output",
            (1..=8)
                .map(|index| format!("line {index}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let rendered = format_item(80, &item);

        assert!(rendered.contains("--- tool output"));
        assert!(rendered.contains("line 1"));
        assert!(rendered.contains("line 6"));
        assert!(rendered.contains("more lines folded"));
    }

    #[test]
    fn formats_welcome_banner_with_logo_and_quickstart() {
        let rendered = format_welcome_banner(
            "gpt-5.4-mini high",
            &PathBuf::from(r"/Users/someone/projects/launchpad-agent"),
            "0.118.0",
            80,
        );

        // Box border.
        assert!(rendered.contains("╭"));
        // ASCII logo signature line (AGENT subtitle underneath "Launchpad").
        assert!(rendered.contains("A G E N T"));
        // Tagline + version.
        assert!(rendered.contains("Any model. Any terminal."));
        assert!(rendered.contains("v0.118.0"));
        // Context row.
        assert!(rendered.contains("model       gpt-5.4-mini high"));
        assert!(rendered.contains("directory   "));
        // Quick-start tips.
        assert!(rendered.contains("Get started"));
        assert!(rendered.contains("/configure"));
        assert!(rendered.contains("Or just type a message to begin."));
    }

    #[test]
    fn splits_long_assistant_pending_line_before_turn_end() {
        let (flushed, remainder) = split_assistant_pending_line(
            24,
            "this is a long assistant line without newline yet",
            true,
        );

        assert!(!flushed.is_empty());
        assert!(unicode_width::UnicodeWidthStr::width(flushed[0].as_str()) <= 13);
        assert!(!remainder.is_empty());
    }
}
