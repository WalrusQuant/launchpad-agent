use lpa_server::SkillSource;

pub(super) fn render_skill_list_body(skills: &[lpa_server::SkillRecord]) -> String {
    if skills.is_empty() {
        return "No skills found".to_string();
    }

    skills
        .iter()
        .map(|skill| {
            let status = if skill.enabled { "enabled" } else { "disabled" };
            format!(
                "{} ({status})\n{}\nsource: {}\npath: {}",
                skill.name,
                skill.description,
                render_skill_source(&skill.source),
                skill.path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn render_skill_source(source: &SkillSource) -> String {
    match source {
        SkillSource::User => "user".to_string(),
        SkillSource::Workspace { cwd } => format!("workspace ({})", cwd.display()),
        SkillSource::Plugin { plugin_id } => format!("plugin ({plugin_id})"),
    }
}

pub(super) fn summarize_tool_call(payload: &serde_json::Value) -> String {
    let tool_name = payload
        .get("tool_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tool");
    let input = payload.get("input").unwrap_or(&serde_json::Value::Null);
    let detail = summarize_tool_input(tool_name, input);
    if detail.is_empty() {
        tool_name.to_string()
    } else {
        format!("{tool_name}: {detail}")
    }
}

fn summarize_tool_input(tool_name: &str, input: &serde_json::Value) -> String {
    let candidate = match tool_name {
        "bash" => input
            .get("command")
            .and_then(serde_json::Value::as_str)
            .or_else(|| input.get("cmd").and_then(serde_json::Value::as_str)),
        "read" => input
            .get("filePath")
            .and_then(serde_json::Value::as_str)
            .or_else(|| input.get("path").and_then(serde_json::Value::as_str)),
        "write" | "edit" | "apply_patch" => input
            .get("path")
            .and_then(serde_json::Value::as_str)
            .or_else(|| input.get("filePath").and_then(serde_json::Value::as_str)),
        "webfetch" | "websearch" => input
            .get("url")
            .and_then(serde_json::Value::as_str)
            .or_else(|| input.get("query").and_then(serde_json::Value::as_str)),
        _ => None,
    };

    candidate
        .map(|text| compact_tool_summary(text, 96))
        .unwrap_or_else(|| compact_tool_summary(&render_json_preview(input), 96))
}

fn compact_tool_summary(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = compact.chars().count() > max_chars;
    let mut out = compact.chars().take(max_chars).collect::<String>();
    if truncated {
        out.push('…');
    }
    out
}

pub(super) fn render_json_preview(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => truncate_tool_output(text),
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
            truncate_tool_output(&pretty)
        }
        _ => truncate_tool_output(&value.to_string()),
    }
}

pub(super) fn render_json_value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

pub(super) fn truncate_tool_output(content: &str) -> String {
    const MAX_LINES: usize = 8;
    const MAX_CHARS: usize = 1200;
    let content = normalize_display_output(content);
    let content = content.as_str();

    let mut lines = Vec::new();
    let mut chars = 0usize;
    for line in content.lines() {
        if lines.len() >= MAX_LINES || chars >= MAX_CHARS {
            break;
        }
        let remaining = MAX_CHARS.saturating_sub(chars);
        if line.chars().count() > remaining {
            let preview = line.chars().take(remaining).collect::<String>();
            lines.push(preview);
            break;
        }
        chars += line.chars().count();
        lines.push(line.to_string());
    }

    if lines.is_empty() && !content.is_empty() {
        let preview = content.chars().take(MAX_CHARS).collect::<String>();
        return if preview == content {
            preview
        } else {
            format!("{preview}\n… ")
        };
    }

    let preview = lines.join("\n");
    if preview == content {
        preview
    } else if preview.is_empty() {
        "… ".to_string()
    } else {
        format!("{preview}\n… ")
    }
}

pub(super) fn normalize_display_output(content: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_matches('\n')
        .to_string()
}
