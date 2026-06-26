use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lpa_core::{
    AppConfig, AppConfigLoader, FileSystemAppConfigLoader, LoggingBootstrap, LoggingRuntime,
    ModelCatalog, PresetModelCatalog, load_config, resolve_provider_settings,
};
use lpa_server::{ServerProcessArgs, run_server_process};
use lpa_utils::find_lpa_home;

mod agent;

use agent::run_agent;

/// Process exit codes. Documented here so scripts can branch on `lpagent`'s
/// result. `clap` already exits with `USAGE` (2) on argument-parse failures.
mod exit_codes {
    /// The command completed successfully.
    pub const SUCCESS: i32 = 0;
    /// The command ran but failed (turn error, provider failure, I/O error).
    pub const FAILURE: i32 = 1;
    /// The invocation was malformed (bad arguments / missing input).
    pub const USAGE: i32 = 2;
}

/// Top-level `lpagent` command that dispatches to interactive agent mode or one
/// of the supporting runtime subcommands.
#[derive(Debug, Parser)]
#[command(name = "lpagent", version, about = "Launchpad Agent CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Run a single prompt non-interactively and print the result (headless).
    ///
    /// Pass the prompt as the value (`-p "fix the bug"`). With no value the
    /// prompt is read from stdin, so `cat task.txt | lpagent -p` works.
    #[arg(short = 'p', long = "print", value_name = "PROMPT", num_args = 0..=1, default_missing_value = "", global = true)]
    print: Option<String>,

    /// Override the model used for this session.
    #[arg(long, global = true)]
    model: Option<String>,

    /// Replace the system prompt entirely (headless mode).
    #[arg(long = "system-prompt", global = true)]
    system_prompt: Option<String>,

    /// Append extra text to the system prompt (headless mode).
    #[arg(long = "append-system-prompt", global = true)]
    append_system_prompt: Option<String>,

    /// Restrict the model to these tools (comma-separated names, headless mode).
    #[arg(long = "allowed-tools", global = true, value_delimiter = ',')]
    allowed_tools: Vec<String>,

    /// Remove these tools from the model (comma-separated names, headless mode).
    #[arg(long = "disallowed-tools", global = true, value_delimiter = ',')]
    disallowed_tools: Vec<String>,

    /// Bypass all permission checks and the workspace sandbox (headless mode).
    #[arg(
        long = "dangerously-skip-permissions",
        global = true,
        default_value_t = false
    )]
    dangerously_skip_permissions: bool,

    /// Keep the UI in the main terminal buffer instead of switching to the alternate screen.
    #[arg(long = "no-alt-screen", default_value_t = false)]
    no_alt_screen: bool,

    /// Override the logging level for this process.
    #[arg(long = "log-level", global = true, value_enum)]
    log_level: Option<LogLevel>,

    /// Verbose logging (info level). Shortcut for `--log-level info`.
    #[arg(short = 'v', long = "verbose", global = true, default_value_t = false)]
    verbose: bool,

    /// Debug logging. Shortcut for `--log-level debug`.
    #[arg(long = "debug", global = true, default_value_t = false)]
    debug: bool,
}

impl Cli {
    /// Resolve the effective log level from the explicit `--log-level` plus the
    /// `--debug` / `--verbose` shortcuts (`--debug` wins, then `--verbose`).
    fn effective_log_level(&self) -> Option<LogLevel> {
        if self.debug {
            Some(LogLevel::Debug)
        } else if self.verbose {
            self.log_level.or(Some(LogLevel::Info))
        } else {
            self.log_level
        }
    }

    /// Build the headless-run options when the invocation requested print mode,
    /// resolving the prompt text from the flag value or stdin.
    fn headless_options(&self, prompt: String) -> HeadlessOptions {
        HeadlessOptions {
            prompt,
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
            append_system_prompt: self.append_system_prompt.clone(),
            allowed_tools: self.allowed_tools.clone(),
            disallowed_tools: self.disallowed_tools.clone(),
            skip_permissions: self.dangerously_skip_permissions,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _logging = install_logging(&cli)?;

    // Headless dispatch: `-p/--print` or the `prompt` subcommand both run a
    // single turn and exit with a documented code instead of opening the TUI.
    if let Some(flag_prompt) = cli.print.clone() {
        return run_headless_and_exit(cli.headless_options(resolve_prompt_text(flag_prompt)?))
            .await;
    }
    if let Some(Command::Prompt { input }) = &cli.command {
        let prompt = resolve_prompt_text(input.clone().unwrap_or_default())?;
        return run_headless_and_exit(cli.headless_options(prompt)).await;
    }

    let log_level = cli.effective_log_level().map(LogLevel::as_str);
    match cli.command {
        Some(Command::Server(args)) => run_server_process(args).await,
        Some(Command::Onboard) => {
            run_agent(true, cli.no_alt_screen, log_level, cli.model.as_deref()).await
        }
        Some(Command::Prompt { .. }) => unreachable!("prompt handled by headless dispatch above"),
        Some(Command::Doctor) => run_doctor().await,
        None => run_agent(false, cli.no_alt_screen, log_level, cli.model.as_deref()).await,
    }
}

/// Resolve the headless prompt text: a non-empty flag value is used verbatim,
/// otherwise the prompt is read from stdin. Empty input is a usage error.
fn resolve_prompt_text(flag_value: String) -> Result<String> {
    if !flag_value.trim().is_empty() {
        return Ok(flag_value);
    }

    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .context("failed to read prompt from stdin")?;
    if buffer.trim().is_empty() {
        eprintln!("error: no prompt provided (pass text to -p or pipe it on stdin)");
        std::process::exit(exit_codes::USAGE);
    }
    Ok(buffer)
}

/// Run a headless turn and exit the process with a documented code.
async fn run_headless_and_exit(options: HeadlessOptions) -> Result<()> {
    match run_headless(options).await {
        Ok(()) => std::process::exit(exit_codes::SUCCESS),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(exit_codes::FAILURE);
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Launch the interactive onboarding flow to configure a model provider.
    Onboard,
    /// Start the transport-facing server runtime.
    Server(ServerProcessArgs),
    /// Send a single prompt to the model and print the response (non-interactive).
    ///
    /// Equivalent to `lpagent -p`. Omit the text to read the prompt from stdin.
    Prompt {
        /// The prompt text to send to the model (reads stdin when omitted).
        input: Option<String>,
    },
    /// Diagnose configuration, provider connectivity, and system health.
    Doctor,
}

fn install_logging(cli: &Cli) -> Result<LoggingRuntime> {
    let home_dir = find_lpa_home()?;
    let loader = FileSystemAppConfigLoader::new(home_dir.clone())
        .with_cli_overrides(cli_logging_overrides(cli));
    let current_dir = std::env::current_dir()?;
    let workspace_root = match &cli.command {
        Some(Command::Server(args)) => args.working_root.as_deref(),
        _ => Some(current_dir.as_path()),
    };
    let app_config = loader.load(workspace_root).unwrap_or_else(|err| {
        eprintln!("warning: failed to load app config for logging: {err}");
        AppConfig::default()
    });
    LoggingBootstrap {
        process_name: logging_process_name(&cli.command),
        config: app_config.logging,
        home_dir,
    }
    .install()
    .map_err(Into::into)
}

fn cli_logging_overrides(cli: &Cli) -> toml::Value {
    let Some(log_level) = cli.effective_log_level() else {
        return toml::Value::Table(Default::default());
    };

    toml::Value::Table(toml::map::Map::from_iter([(
        "logging".to_string(),
        toml::Value::Table(toml::map::Map::from_iter([(
            "level".to_string(),
            toml::Value::String(log_level.as_str().to_string()),
        )])),
    )]))
}

fn logging_process_name(command: &Option<Command>) -> &'static str {
    match command {
        Some(Command::Onboard) => "onboard",
        Some(Command::Server(_)) => "server",
        Some(Command::Prompt { .. }) => "prompt",
        Some(Command::Doctor) => "doctor",
        None => "cli",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// Resolved options for a single non-interactive (headless) run.
///
/// Logging is intentionally not part of this struct: `install_logging` in
/// `main` already installs the global subscriber (honoring `--verbose` /
/// `--debug` via [`Cli::effective_log_level`]) before this runs.
struct HeadlessOptions {
    prompt: String,
    model: Option<String>,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    allowed_tools: Vec<String>,
    disallowed_tools: Vec<String>,
    skip_permissions: bool,
}

async fn run_headless(options: HeadlessOptions) -> Result<()> {
    use lpa_core::{SessionConfig, SessionState, default_base_instructions};
    use lpa_tools::{ToolOrchestrator, ToolRegistry};

    let cwd = std::env::current_dir()?;
    let _stored_config = load_config().unwrap_or_default();
    let mut resolved = resolve_provider_settings()
        .map_err(|e| anyhow::anyhow!("failed to resolve provider: {e}"))?;

    if let Some(model) = &options.model {
        resolved.model = model.clone();
    }

    let home_dir = find_lpa_home()?;
    let provider =
        lpa_server::load_server_provider(&home_dir.join("config.toml"), Some(&resolved.model))?;

    // Resolve the merged app config (user + project) so the single-shot path
    // honors the same [sandbox] section as the interactive server path. When
    // the caller passes `--dangerously-skip-permissions` we drop the sandbox so
    // the run is fully unrestricted.
    let app_config = FileSystemAppConfigLoader::new(home_dir.clone())
        .load(Some(cwd.as_path()))
        .unwrap_or_default();
    let sandbox_policy = if options.skip_permissions {
        None
    } else {
        app_config.sandbox.to_policy_record()
    };
    if sandbox_policy.is_some() {
        eprintln!("lpagent [prompt] sandbox enabled (workspace-scoped)");
    }
    if options.skip_permissions {
        eprintln!("lpagent [prompt] permission checks and sandbox bypassed");
    }

    let session_config = SessionConfig {
        sandbox_policy,
        ..SessionConfig::default()
    };
    let mut session_state = SessionState::new(session_config, cwd.clone());
    session_state.push_message(lpa_core::Message::user(options.prompt.clone()));

    let registry = {
        let mut reg = ToolRegistry::new();
        lpa_tools::register_builtin_tools(&mut reg);
        apply_tool_filters(&mut reg, &options.allowed_tools, &options.disallowed_tools);
        std::sync::Arc::new(reg)
    };
    let orchestrator = ToolOrchestrator::new(std::sync::Arc::clone(&registry));
    let model_catalog = PresetModelCatalog::load()?;

    let mut model = model_catalog
        .get(&resolved.model)
        .cloned()
        .unwrap_or_else(|| lpa_core::Model {
            slug: resolved.model.clone(),
            base_instructions: default_base_instructions().to_string(),
            ..Default::default()
        });
    apply_system_prompt_overrides(
        &mut model.base_instructions,
        options.system_prompt.as_deref(),
        options.append_system_prompt.as_deref(),
    );

    let turn_config = lpa_core::TurnConfig {
        model,
        thinking_selection: None,
    };

    eprintln!("lpagent [prompt] model={} sending...", resolved.model);

    let result = lpa_core::query(
        &mut session_state,
        &turn_config,
        std::sync::Arc::clone(&provider.provider),
        registry,
        &orchestrator,
        None,
    )
    .await;

    match result {
        Ok(()) => {
            let reply = session_state.messages.iter().rev().find_map(|m| {
                if m.role != lpa_core::Role::Assistant {
                    return None;
                }
                m.content
                    .iter()
                    .filter_map(|block| match block {
                        lpa_core::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .next()
            });
            match reply {
                Some(text) => println!("{}", text),
                None => eprintln!("lpagent [prompt] empty response"),
            }
        }
        Err(e) => {
            anyhow::bail!("prompt failed: {e}");
        }
    }

    Ok(())
}

/// Apply `--system-prompt` (full replacement) then `--append-system-prompt`
/// (suffix) to the model's base instructions, in that order.
fn apply_system_prompt_overrides(
    base_instructions: &mut String,
    replacement: Option<&str>,
    append: Option<&str>,
) {
    if let Some(system_prompt) = replacement {
        *base_instructions = system_prompt.to_string();
    }
    if let Some(extra) = append {
        if !base_instructions.is_empty() {
            base_instructions.push_str("\n\n");
        }
        base_instructions.push_str(extra);
    }
}

/// Apply `--allowed-tools` / `--disallowed-tools` to the registry. When an
/// allow-list is present only those tools survive; the deny-list is then removed
/// from whatever remains.
fn apply_tool_filters(
    registry: &mut lpa_tools::ToolRegistry,
    allowed: &[String],
    disallowed: &[String],
) {
    if !allowed.is_empty() {
        registry.retain(|name| allowed.iter().any(|allowed| allowed == name));
    }
    if !disallowed.is_empty() {
        registry.retain(|name| !disallowed.iter().any(|denied| denied == name));
    }
}

async fn run_doctor() -> Result<()> {
    use colored::Colorize;
    use std::process::Command;

    println!("{}", "=== Launchpad Agent Doctor ===".bold());
    println!();

    let mut all_ok = true;

    println!("{} Rust toolchain:", "✓".green().bold());
    let rustc = Command::new("rustc").arg("--version").output();
    match rustc {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("  {}", version);
        }
        Err(e) => {
            println!("  {} rustc not found: {}", "✗".red(), e);
            all_ok = false;
        }
    }
    println!();

    println!("{} Config home (LPA_HOME):", "✓".green().bold());
    match find_lpa_home() {
        Ok(home) => {
            println!("  {}", home.display());
        }
        Err(e) => {
            println!("  {} {}", "✗".red(), e);
            all_ok = false;
        }
    }
    println!();

    println!("{} Config file:", "✓".green().bold());
    if let Ok(home) = find_lpa_home() {
        let config_path = home.join("config.toml");
        if config_path.exists() {
            println!("  {} {}", "found".green(), config_path.display());
            let content = std::fs::read_to_string(&config_path).unwrap_or_default();
            if content.contains("api_key") && content.contains("base_url") {
                println!("  {} api_key and base_url configured", "✓".green());
            } else {
                println!("  {} api_key or base_url missing", "!".yellow());
                all_ok = false;
            }
            let model_line = content.lines().find(|l| l.starts_with("model"));
            if let Some(line) = model_line {
                println!("  default model: {}", line.trim());
            } else {
                println!("  {} no default model set", "!".yellow());
            }
        } else {
            println!(
                "  {} not found at {}",
                "missing".yellow(),
                config_path.display()
            );
            println!("  Run `lpagent onboard` to create it.");
            all_ok = false;
        }
    }
    println!();

    println!("{} Provider resolution:", "✓".green().bold());
    match resolve_provider_settings() {
        Ok(resolved) => {
            println!("  provider:   {}", resolved.provider_id);
            println!("  model:      {}", resolved.model);
            println!(
                "  base_url:   {}",
                resolved.base_url.unwrap_or("default".into())
            );
            println!("  wire_api:   {:?}", resolved.wire_api);
            if resolved.api_key.is_some() {
                println!("  api_key:    {} (set)", "✓".green());
            } else {
                println!("  api_key:    {} (not set)", "✗".red());
                all_ok = false;
            }
        }
        Err(e) => {
            println!("  {} {}", "✗".red(), e);
            all_ok = false;
        }
    }
    println!();

    println!("{} Model catalog:", "✓".green().bold());
    match lpa_core::PresetModelCatalog::load() {
        Ok(catalog) => {
            let count = catalog.into_inner().len();
            println!("  {} builtin models loaded", count);
        }
        Err(e) => {
            println!("  {} failed to load: {}", "✗".red(), e);
            all_ok = false;
        }
    }
    println!();

    if all_ok {
        println!("{}", "All checks passed. Ready to use!".green().bold());
    } else {
        println!(
            "{}",
            "Some checks failed. See above for details.".yellow().bold()
        );
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        Cli, Command, LogLevel, ServerProcessArgs, apply_system_prompt_overrides,
        apply_tool_filters, cli_logging_overrides, logging_process_name,
    };
    use lpa_tools::{ToolRegistry, register_builtin_tools};

    /// A `Cli` with every field at its default; tests tweak the fields they care
    /// about so adding new flags does not churn every test.
    fn base_cli() -> Cli {
        Cli {
            command: None,
            print: None,
            model: None,
            system_prompt: None,
            append_system_prompt: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            dangerously_skip_permissions: false,
            no_alt_screen: false,
            log_level: None,
            verbose: false,
            debug: false,
        }
    }

    #[test]
    fn logging_process_name_defaults_to_cli() {
        assert_eq!(logging_process_name(&None), "cli");
    }

    #[test]
    fn logging_process_name_uses_server_for_server_subcommand() {
        assert_eq!(
            logging_process_name(&Some(Command::Server(ServerProcessArgs {
                working_root: None,
            }))),
            "server"
        );
    }

    #[test]
    fn logging_process_name_uses_onboard_for_onboard_subcommand() {
        assert_eq!(logging_process_name(&Some(Command::Onboard)), "onboard");
    }

    #[test]
    fn cli_logging_overrides_is_empty_without_log_level() {
        let cli = base_cli();

        assert_eq!(
            cli_logging_overrides(&cli),
            toml::Value::Table(Default::default())
        );
    }

    #[test]
    fn cli_logging_overrides_sets_logging_level() {
        let cli = Cli {
            log_level: Some(LogLevel::Debug),
            ..base_cli()
        };

        assert_eq!(
            cli_logging_overrides(&cli),
            toml::Value::Table(toml::map::Map::from_iter([(
                "logging".to_string(),
                toml::Value::Table(toml::map::Map::from_iter([(
                    "level".to_string(),
                    toml::Value::String("debug".to_string()),
                )])),
            )]))
        );
    }

    #[test]
    fn effective_log_level_prefers_debug_then_verbose() {
        assert_eq!(
            Cli {
                debug: true,
                verbose: true,
                log_level: Some(LogLevel::Error),
                ..base_cli()
            }
            .effective_log_level(),
            Some(LogLevel::Debug)
        );
        assert_eq!(
            Cli {
                verbose: true,
                ..base_cli()
            }
            .effective_log_level(),
            Some(LogLevel::Info)
        );
        assert_eq!(
            Cli {
                verbose: true,
                log_level: Some(LogLevel::Trace),
                ..base_cli()
            }
            .effective_log_level(),
            Some(LogLevel::Trace)
        );
        assert_eq!(base_cli().effective_log_level(), None);
    }

    #[test]
    fn system_prompt_replacement_then_append() {
        let mut base = "original".to_string();
        apply_system_prompt_overrides(&mut base, Some("replaced"), Some("extra"));
        assert_eq!(base, "replaced\n\nextra");
    }

    #[test]
    fn system_prompt_append_only_keeps_base() {
        let mut base = "original".to_string();
        apply_system_prompt_overrides(&mut base, None, Some("extra"));
        assert_eq!(base, "original\n\nextra");
    }

    #[test]
    fn allowed_tools_keeps_only_listed() {
        let mut registry = ToolRegistry::new();
        register_builtin_tools(&mut registry);
        apply_tool_filters(&mut registry, &["read".to_string(), "ls".to_string()], &[]);
        assert!(registry.get("read").is_some());
        assert!(registry.get("ls").is_some());
        assert!(registry.get("bash").is_none());
        assert_eq!(registry.all().len(), 2);
    }

    #[test]
    fn disallowed_tools_removes_listed() {
        let mut registry = ToolRegistry::new();
        register_builtin_tools(&mut registry);
        let before = registry.all().len();
        apply_tool_filters(&mut registry, &[], &["bash".to_string()]);
        assert!(registry.get("bash").is_none());
        assert_eq!(registry.all().len(), before - 1);
    }
}
