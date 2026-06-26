use std::collections::HashMap;
use std::sync::Arc;

use lpa_protocol::ToolDefinition;

use crate::Tool;

/// Central registry of available tools.
///
/// The registry owns all tool instances and provides lookup by name.
/// Tools are registered once at startup and remain immutable for the
/// lifetime of the session.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Return all tools for inclusion in the model request.
    pub fn all(&self) -> Vec<&Arc<dyn Tool>> {
        self.tools.values().collect()
    }

    /// Drop every registered tool whose name does not satisfy `keep`.
    ///
    /// Used by headless invocations to honor `--allowed-tools` /
    /// `--disallowed-tools` filters before the registry is frozen for the
    /// lifetime of the run.
    pub fn retain<F: FnMut(&str) -> bool>(&mut self, mut keep: F) {
        self.tools.retain(|name, _| keep(name));
    }

    /// Build tool definitions suitable for the model API.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Applies `--allowed-tools` / `--disallowed-tools` filters to a registry. When
/// `allowed` is non-empty only those tools survive; `disallowed` is then removed
/// from whatever remains. Shared by the headless CLI path and the server
/// bootstrap (which honors the `LPA_ALLOWED_TOOLS` / `LPA_DISALLOWED_TOOLS` env
/// vars set by a headless run) so both apply identical semantics.
pub fn apply_tool_filters(registry: &mut ToolRegistry, allowed: &[String], disallowed: &[String]) {
    if !allowed.is_empty() {
        registry.retain(|name| allowed.iter().any(|candidate| candidate == name));
    }
    if !disallowed.is_empty() {
        registry.retain(|name| !disallowed.iter().any(|denied| denied == name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    use crate::{ToolContext, ToolOutput};

    struct DummyTool {
        tool_name: &'static str,
        read_only: bool,
    }

    #[async_trait]
    impl crate::Tool for DummyTool {
        fn name(&self) -> &str {
            self.tool_name
        }
        fn description(&self) -> &str {
            "dummy"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        async fn execute(
            &self,
            _ctx: &ToolContext,
            _input: serde_json::Value,
        ) -> anyhow::Result<ToolOutput> {
            Ok(ToolOutput::success("ok"))
        }
        fn is_read_only(&self) -> bool {
            self.read_only
        }
    }

    #[test]
    fn register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool {
            tool_name: "test_tool",
            read_only: true,
        }));
        assert!(reg.get("test_tool").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn all_returns_registered_tools() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool {
            tool_name: "a",
            read_only: true,
        }));
        reg.register(Arc::new(DummyTool {
            tool_name: "b",
            read_only: false,
        }));
        assert_eq!(reg.all().len(), 2);
    }

    #[test]
    fn tool_definitions_maps_correctly() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool {
            tool_name: "my_tool",
            read_only: true,
        }));
        let defs = reg.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "my_tool");
        assert_eq!(defs[0].description, "dummy");
    }

    #[test]
    fn register_overwrites_duplicate_name() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool {
            tool_name: "same",
            read_only: true,
        }));
        reg.register(Arc::new(DummyTool {
            tool_name: "same",
            read_only: false,
        }));
        let tool = reg.get("same").unwrap();
        assert!(!tool.is_read_only());
    }

    #[test]
    fn default_creates_empty_registry() {
        let reg = ToolRegistry::default();
        assert!(reg.all().is_empty());
    }

    fn registry_with(names: &[&str]) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        for name in names {
            reg.register(Arc::new(DummyTool {
                tool_name: Box::leak(name.to_string().into_boxed_str()),
                read_only: true,
            }));
        }
        reg
    }

    #[test]
    fn apply_tool_filters_allow_keeps_only_listed() {
        let mut reg = registry_with(&["read", "ls", "bash"]);
        apply_tool_filters(&mut reg, &["read".to_string(), "ls".to_string()], &[]);
        assert!(reg.get("read").is_some());
        assert!(reg.get("ls").is_some());
        assert!(reg.get("bash").is_none());
        assert_eq!(reg.all().len(), 2);
    }

    #[test]
    fn apply_tool_filters_deny_removes_listed() {
        let mut reg = registry_with(&["read", "ls", "bash"]);
        apply_tool_filters(&mut reg, &[], &["bash".to_string()]);
        assert!(reg.get("bash").is_none());
        assert_eq!(reg.all().len(), 2);
    }

    #[test]
    fn apply_tool_filters_empty_is_noop() {
        let mut reg = registry_with(&["read", "ls"]);
        apply_tool_filters(&mut reg, &[], &[]);
        assert_eq!(reg.all().len(), 2);
    }
}
