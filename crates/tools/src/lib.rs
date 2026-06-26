mod apply_patch;
mod bash;
mod context;
mod file_write;
mod glob;
mod grep;
mod invalid;
mod ls;
mod mcp_adapter;
mod orchestrator;
mod plan;
mod question;
mod read;
mod registry;
mod runtime;
mod shell_exec;
mod skill;
mod todo;
mod tool;
mod webfetch;
mod websearch;

pub use apply_patch::ApplyPatchTool;
pub use bash::BashTool;
pub use context::*;
pub use file_write::FileWriteTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use invalid::InvalidTool;
pub use ls::LsTool;
pub use mcp_adapter::{
    MCP_TOOL_PREFIX, McpToolAdapter, flatten_call_tool_result, register_mcp_tools,
};
pub use orchestrator::*;
pub use plan::PlanTool;
pub use question::QuestionTool;
pub use read::ReadTool;
pub use registry::*;
pub use runtime::*;
pub use skill::SkillTool;
pub use todo::TodoWriteTool;
pub use tool::{Tool, ToolOutput, ToolProgressEvent};
pub use webfetch::WebFetchTool;
pub use websearch::WebSearchTool;

use std::sync::Arc;

/// Register all built-in tools into a registry.
///
/// Subagent dispatch (`task`) and LSP (`lsp`) tools are not implemented; when
/// they land, add their tool impls and register them here.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(FileWriteTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(LsTool));
    registry.register(Arc::new(InvalidTool));
    registry.register(Arc::new(QuestionTool));
    registry.register(Arc::new(TodoWriteTool));
    registry.register(Arc::new(WebFetchTool));
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(SkillTool));
    registry.register(Arc::new(ApplyPatchTool));
    registry.register(Arc::new(PlanTool));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_builtin_tools_populates_registry() {
        let mut registry = ToolRegistry::new();
        register_builtin_tools(&mut registry);

        let expected = [
            "bash",
            "read",
            "write",
            "glob",
            "grep",
            "ls",
            "invalid",
            "question",
            "todowrite",
            "webfetch",
            "websearch",
            "skill",
            "apply_patch",
            "update_plan",
        ];
        for name in &expected {
            assert!(
                registry.get(name).is_some(),
                "expected builtin tool '{}' to be registered",
                name
            );
        }
        assert_eq!(registry.all().len(), expected.len());
    }

    #[test]
    fn unimplemented_tools_are_not_registered() {
        // `task` (subagent dispatch) and `lsp` are not implemented; guard against
        // them being registered before a real implementation exists.
        let mut registry = ToolRegistry::new();
        register_builtin_tools(&mut registry);
        assert!(registry.get("task").is_none());
        assert!(registry.get("lsp").is_none());
    }

    #[test]
    fn builtin_tools_have_nonempty_schemas() {
        let mut registry = ToolRegistry::new();
        register_builtin_tools(&mut registry);

        for tool in registry.all() {
            assert!(!tool.name().is_empty());
            assert!(!tool.description().is_empty());
            let schema = tool.input_schema();
            assert!(
                schema.is_object(),
                "tool '{}' schema should be an object",
                tool.name()
            );
        }
    }

    #[test]
    fn register_builtin_runtime_tools_populates_registry() {
        let registry = RuntimeToolRegistry::new();
        register_builtin_runtime_tools(&registry);

        let expected = [
            "shell_command",
            "bash",
            "read",
            "write",
            "glob",
            "grep",
            "ls",
            "invalid",
            "question",
            "todowrite",
            "webfetch",
            "websearch",
            "skill",
            "apply_patch",
            "update_plan",
        ];
        for name in &expected {
            assert!(
                registry.get(&ToolName((*name).into())).is_some(),
                "expected builtin runtime tool '{}' to be registered",
                name
            );
        }
        assert_eq!(registry.list().len(), expected.len());
    }
}
