use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lpa_core::{
    AppConfig, AppConfigLoader, FileSystemAppConfigLoader, LoggingBootstrap, LoggingRuntime,
    resolve_provider_settings,
};
use lpa_server::{ServerProcessArgs, run_server_process};
use lpa_utils::find_lpa_home;

mod agent;
mod headless;
mod headless_output;
mod server_env;

use agent::run_agent;
use headless::{HeadlessOptions, run_headless};
use headless_output::OutputFormat;

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

    /// Output format for headless runs: `text` (default), `json`, or `stream-json`.
    #[arg(
        long = "output-format",
        global = true,
        value_enum,
        default_value_t = OutputFormat::Text
    )]
    output_format: OutputFormat,

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

    /// Resume a specific session by id, running the prompt as its next turn (headless).
    #[arg(
        short = 'r',
        long = "resume",
        value_name = "SESSION_ID",
        global = true,
        conflicts_with_all = ["continue_session", "session_id"]
    )]
    resume: Option<String>,

    /// Resume the most recent session in the current directory (headless).
    #[arg(
        short = 'c',
        long = "continue",
        global = true,
        default_value_t = false,
        conflicts_with_all = ["resume", "session_id"]
    )]
    continue_session: bool,

    /// Run under a specific session id, resuming it if it exists or creating it (headless).
    #[arg(
        long = "session-id",
        value_name = "UUID",
        global = true,
        conflicts_with_all = ["resume", "continue_session"]
    )]
    session_id: Option<String>,

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
            resume: self.resume.clone(),
            continue_session: self.continue_session,
            session_id: self.session_id.clone(),
            output_format: self.output_format,
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

    // The resume flags only take effect on the headless (`-p`/`prompt`) path
    // today. Supplied without a headless dispatch they would otherwise be
    // silently dropped and a fresh interactive session opened — fail loudly
    // instead so the caller does not believe they resumed.
    if cli.resume.is_some() || cli.continue_session || cli.session_id.is_some() {
        eprintln!(
            "error: --resume/--continue/--session-id require -p/--print or the `prompt` subcommand"
        );
        std::process::exit(exit_codes::USAGE);
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
        Cli, Command, LogLevel, OutputFormat, ServerProcessArgs, cli_logging_overrides,
        logging_process_name,
    };

    /// A `Cli` with every field at its default; tests tweak the fields they care
    /// about so adding new flags does not churn every test.
    fn base_cli() -> Cli {
        Cli {
            command: None,
            print: None,
            output_format: OutputFormat::Text,
            model: None,
            system_prompt: None,
            append_system_prompt: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            dangerously_skip_permissions: false,
            resume: None,
            continue_session: false,
            session_id: None,
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
}
