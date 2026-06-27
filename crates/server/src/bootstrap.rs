use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use lpa_core::{
    AppConfigLoader, FileSystemAppConfigLoader, FileSystemSkillCatalog, ModelCatalog,
    PresetModelCatalog, SkillsConfig,
};
use lpa_mcp::{McpManager, StdMcpManager, TrustLevel};
use lpa_tools::{MCP_TOOL_PREFIX, ToolRegistry, apply_tool_filters, register_mcp_tools};
use lpa_utils::FileSystemConfigPathResolver;

use crate::{
    ListenTarget, ServerRuntime,
    execution::{BaseInstructionsOverride, ServerRuntimeDependencies},
    load_server_provider, resolve_listen_targets, run_listeners,
};

/// Command-line arguments accepted by the standalone server process entrypoint.
#[derive(Debug, Clone, Parser)]
#[command(name = "lpa-server", version, about)]
pub struct ServerProcessArgs {
    /// Optional workspace root used for project-level config resolution.
    #[arg(long)]
    pub working_root: Option<PathBuf>,
}

/// Starts the transport-facing server runtime using the resolved application
/// configuration and listener set.
pub async fn run_server_process(args: ServerProcessArgs) -> Result<()> {
    let resolver = FileSystemConfigPathResolver::from_env()?;
    let loader = FileSystemAppConfigLoader::new(resolver.user_config_dir());
    let config = loader.load(args.working_root.as_deref())?;
    let listen_targets = resolve_listen_targets(&config.server.listen)?;
    let effective_listen = listen_targets
        .iter()
        .map(|target| match target {
            ListenTarget::Stdio => "stdio://".to_string(),
            ListenTarget::WebSocket { bind_address } => format!("ws://{bind_address}"),
        })
        .collect::<Vec<_>>();

    tracing::info!(
        user_config = %resolver.user_config_file().display(),
        project_config = args
            .working_root
            .as_deref()
            .map(|root| resolver.project_config_file(root).display().to_string())
            .unwrap_or_else(|| "<none>".into()),
        configured_listen = ?config.server.listen,
        effective_listen = ?effective_listen,
        max_connections = config.server.max_connections,
        "loaded server config"
    );

    let mut registry = ToolRegistry::new();
    lpa_tools::register_builtin_tools(&mut registry);

    // Spin up MCP supervisors + register any discovered tools into the registry.
    // Hold on to the concrete `StdMcpManager` so we can call its eager-start
    // helper (not part of the trait) and the final `shutdown_all`.
    let concrete_mcp_manager = Arc::new(StdMcpManager::from_config(&config.mcp)?);
    let mcp_manager: Arc<dyn McpManager> = Arc::clone(&concrete_mcp_manager) as Arc<dyn McpManager>;
    if config.mcp.auto_start {
        concrete_mcp_manager.start_configured(&config.mcp).await?;
    }
    let statuses = mcp_manager.statuses().await.unwrap_or_default();
    register_mcp_tools(&mut registry, Arc::clone(&mcp_manager), &statuses);

    // Honor headless `--allowed-tools` / `--disallowed-tools`, carried into this
    // dedicated server subprocess as env vars. Applied after builtin + MCP
    // registration so the filter sees the full tool set. Process-global is
    // correct here: a headless server is single-tenant.
    let allowed_tools = env_csv("LPA_ALLOWED_TOOLS");
    let disallowed_tools = env_csv("LPA_DISALLOWED_TOOLS");
    if !allowed_tools.is_empty() || !disallowed_tools.is_empty() {
        apply_tool_filters(&mut registry, &allowed_tools, &disallowed_tools);
    }

    // Collect tool names exposed by servers configured with `trust_level = "trusted"`.
    // These are pre-seeded into every new session's approval cache so trusted-server
    // tools skip the approval flow. `exposed_name` mirrors `McpToolAdapter::exposed_name`.
    let trusted_server_ids: std::collections::HashSet<_> = config
        .mcp
        .servers
        .iter()
        .filter(|s| matches!(s.trust_level, TrustLevel::Trusted))
        .map(|s| s.id.clone())
        .collect();
    let trusted_mcp_tool_names: Vec<String> = statuses
        .iter()
        .filter(|s| trusted_server_ids.contains(&s.server_id))
        .flat_map(|s| {
            s.tools
                .iter()
                .map(|t| format!("{MCP_TOOL_PREFIX}{}__{}", s.server_id, t.name))
        })
        .collect();

    let provider = load_server_provider(&resolver.user_config_file(), None)?;
    let model_catalog: Arc<dyn ModelCatalog> = Arc::new(PresetModelCatalog::load()?);
    let skill_workspace_root = args.working_root.clone();
    let project_skill_base = skill_workspace_root
        .as_deref()
        .map(|root| resolver.project_config_dir(root));
    let user_skill_roots = config
        .skills
        .user_roots
        .iter()
        .cloned()
        .map(|root| {
            if root.is_absolute() {
                root
            } else {
                resolver.user_config_dir().join(root)
            }
        })
        .collect();
    let workspace_skill_roots = config
        .skills
        .workspace_roots
        .iter()
        .cloned()
        .filter_map(|root| {
            if root.is_absolute() {
                Some(root)
            } else {
                project_skill_base.as_ref().map(|base| base.join(root))
            }
        })
        .collect();
    let skill_catalog = Box::new(FileSystemSkillCatalog::new(SkillsConfig {
        enabled: config.skills.enabled,
        user_roots: user_skill_roots,
        workspace_roots: workspace_skill_roots,
        watch_for_changes: config.skills.watch_for_changes,
    }));
    let runtime = ServerRuntime::new(
        resolver.user_config_dir(),
        ServerRuntimeDependencies::new(
            provider.provider,
            Arc::new(registry),
            provider.default_model,
            model_catalog,
            skill_workspace_root,
            skill_catalog,
            Arc::clone(&mcp_manager),
            trusted_mcp_tool_names,
            config.sandbox.to_policy_record(),
        )
        .with_base_instructions_override(BaseInstructionsOverride {
            replace: env_nonempty("LPA_SYSTEM_PROMPT"),
            append: env_nonempty("LPA_APPEND_SYSTEM_PROMPT"),
        })
        .with_prompt_caching(config.caching.enabled),
    );
    tracing::info!("starting persisted session restore");
    runtime.load_persisted_sessions().await?;
    tracing::info!("persisted session restore completed");
    tracing::info!("server bootstrap completed; starting listeners");
    tokio::select! {
        result = run_listeners(runtime, &config.server.listen) => {
            result?;
        }
        result = tokio::signal::ctrl_c() => {
            result?;
            tracing::info!("server shutdown requested");
            concrete_mcp_manager.shutdown_all().await;
        }
    }
    Ok(())
}

/// Reads a comma-separated env var into a list of trimmed, non-empty entries.
/// Returns an empty vec when the var is unset or has no usable entries.
fn env_csv(key: &str) -> Vec<String> {
    split_csv(&std::env::var(key).unwrap_or_default())
}

/// Splits a comma-separated value into trimmed, non-empty entries. Pure helper
/// behind [`env_csv`] so the parsing is unit-tested without touching process env.
fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Reads an env var, returning `None` when it is unset or empty.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::split_csv;
    use pretty_assertions::assert_eq;

    #[test]
    fn split_csv_trims_and_drops_empties() {
        assert_eq!(
            split_csv(" read , ,bash,, ls "),
            vec!["read".to_string(), "bash".to_string(), "ls".to_string()]
        );
    }

    #[test]
    fn split_csv_empty_input_is_empty() {
        assert!(split_csv("").is_empty());
    }
}
