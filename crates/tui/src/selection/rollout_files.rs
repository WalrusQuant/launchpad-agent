use super::super::*;
use lpa_core::SessionId;
use lpa_utils::find_lpa_home;
use std::io::{BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn read_redacted_config_toml() -> Result<String> {
    let path = find_lpa_home()?.join("config.toml");
    if !path.exists() {
        return Ok(format!(
            "(no config.toml at {})\n\nRun /configure to create one.",
            path.display()
        ));
    }
    let raw = std::fs::read_to_string(&path)?;
    let masked = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("api_key") || trimmed.starts_with("api_key ") {
                let leading_ws = &line[..line.len() - trimmed.len()];
                if let Some(eq_idx) = trimmed.find('=') {
                    let key_part = &trimmed[..=eq_idx];
                    let value_part = trimmed[eq_idx + 1..].trim();
                    let unquoted = value_part.trim_start_matches('"').trim_end_matches('"');
                    let masked_value = if unquoted.len() > 4 {
                        format!("\"***{}\"", &unquoted[unquoted.len().saturating_sub(4)..])
                    } else {
                        "\"****\"".to_string()
                    };
                    return format!("{leading_ws}{key_part} {masked_value}");
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("{}\n\n(path: {})", masked, path.display()))
}

pub(super) fn local_session_entries() -> Result<Vec<SessionListEntry>> {
    let root = find_lpa_home()?.join("sessions");
    let mut entries = Vec::new();
    if !root.exists() {
        return Ok(entries);
    }

    for path in walk_rollout_files(&root)? {
        if let Some(entry) = read_rollout_session_entry(&path)? {
            entries.push(entry);
        }
    }

    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(entries)
}

pub(super) fn walk_rollout_files(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            files.extend(walk_rollout_files(&path)?);
        } else if file_type.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
    Ok(files)
}

pub(super) fn read_rollout_session_entry(path: &std::path::Path) -> Result<Option<SessionListEntry>> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file).lines();
    let mut session_id = None;
    let mut title: Option<String> = None;
    let mut updated_at: Option<String> = None;

    for line in reader {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(line_value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        if let Some(meta) = line_value.get("SessionMeta") {
            if let Some(session) = meta.get("session") {
                session_id = session
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<SessionId>().ok());
                title = session
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned);
                updated_at = session
                    .get("updated_at")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned);
            }
            continue;
        }

        if let Some(updated) = line_value.get("SessionTitleUpdated") {
            title = updated
                .get("title")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            updated_at = updated
                .get("timestamp")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
        }
    }

    let session_id = session_id.unwrap_or_else(SessionId::new);
    let title = title.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.strip_prefix("rollout-").unwrap_or(stem).to_string())
            .unwrap_or_else(|| "(untitled)".to_string())
    });
    let updated_at = updated_at.unwrap_or_else(|| {
        path.metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(format_system_time)
            .unwrap_or_else(|| "(unknown)".to_string())
    });

    Ok(Some(SessionListEntry {
        session_id,
        title,
        updated_at,
        is_active: false,
    }))
}

pub(super) fn format_system_time(time: SystemTime) -> String {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("unix {}", duration.as_secs()),
        Err(_) => "(unknown)".to_string(),
    }
}
