use crate::{Tool, ToolContext, ToolOutput};
use async_trait::async_trait;
use serde_json::json;
use tracing::debug;

const DESCRIPTION: &str = include_str!("ls.txt");

/// List the entries of a single directory (non-recursive).
pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: cwd)"
                }
            }
        })
    }

    async fn execute(
        &self,
        ctx: &ToolContext,
        input: serde_json::Value,
    ) -> anyhow::Result<ToolOutput> {
        let base = match input["path"].as_str() {
            Some(p) => {
                let pb = std::path::PathBuf::from(p);
                if pb.is_absolute() {
                    pb
                } else {
                    ctx.cwd.join(pb)
                }
            }
            None => ctx.cwd.clone(),
        };

        debug!(base = %base.display(), "ls directory");

        let read_dir = match std::fs::read_dir(&base) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                return Ok(ToolOutput::error(format!(
                    "cannot read directory {}: {error}",
                    base.display()
                )));
            }
        };

        let mut entries: Vec<(String, bool)> = Vec::new();
        for entry in read_dir.flatten() {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            entries.push((entry.file_name().to_string_lossy().to_string(), is_dir));
        }

        // Directories first, then files; alphabetical within each group.
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        if entries.is_empty() {
            return Ok(ToolOutput::success("(empty directory)"));
        }

        let lines: Vec<String> = entries
            .iter()
            .map(|(name, is_dir)| {
                if *is_dir {
                    format!("{name}/")
                } else {
                    name.clone()
                }
            })
            .collect();

        Ok(ToolOutput::success(lines.join("\n")))
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lpa_safety::legacy_permissions::{PermissionMode, RuleBasedPolicy};
    use std::sync::Arc;

    fn ctx(cwd: std::path::PathBuf) -> ToolContext {
        ToolContext {
            cwd,
            permissions: Arc::new(RuleBasedPolicy::new(PermissionMode::AutoApprove)),
            session_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn lists_directories_before_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir(dir.path().join("zsub")).unwrap();
        std::fs::write(dir.path().join("afile.txt"), "x").unwrap();

        let output = LsTool
            .execute(&ctx(dir.path().to_path_buf()), json!({}))
            .await
            .expect("ls executes");
        let lines: Vec<&str> = output.content.lines().collect();
        assert_eq!(lines, vec!["zsub/", "afile.txt"]);
    }

    #[tokio::test]
    async fn reports_empty_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let output = LsTool
            .execute(&ctx(dir.path().to_path_buf()), json!({}))
            .await
            .expect("ls executes");
        assert_eq!(output.content, "(empty directory)");
    }

    #[tokio::test]
    async fn errors_on_missing_directory() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let output = LsTool
            .execute(
                &ctx(dir.path().to_path_buf()),
                json!({ "path": "does-not-exist" }),
            )
            .await
            .expect("ls executes");
        assert!(output.is_error);
        assert!(output.content.contains("cannot read directory"));
    }
}
