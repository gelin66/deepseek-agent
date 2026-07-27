//! CLI entry point for DSE.

#![allow(clippy::uninlined_format_args)]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::borrow::Cow;
use std::io::{self, IsTerminal, Read, Write};
use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use dotenvy::dotenv;

use crate::dependencies::ExternalTool;
use dse_context::{project_context, prompts, skills as skill_context};
use dse_localization::{
    MessageId, ProductLanguage, process_language, process_language_is_set,
    resolve_product_language, set_process_language, tr, tr_in,
};
use dse_protocol::agent_runtime::RunPermissionMode;

mod audit;
mod config;
mod config_persistence;
mod dependencies;
mod error_taxonomy;
mod exec_lifecycle_stream;
mod exec_output;
mod exec_runtime;
mod execpolicy;
mod features;
mod hashing;
mod logging;
#[cfg(test)]
mod m22_streaming_delta_benchmark;
mod mcp;
mod model_failure_presentation;
mod palette;
mod plugins;
mod pricing;
mod route_budget;
mod runtime_log;
mod sandbox_backend;
#[allow(dead_code)]
mod session_diagnostics;
#[allow(dead_code)]
mod settings;
mod skills;
mod startup_trace;
#[cfg(test)]
mod test_support;
mod tls;
mod tui;
mod utils;
mod working_set;
mod workspace_discovery;

use crate::config::{Config, DEFAULT_TEXT_MODEL, MAX_SUBAGENTS, effective_home_dir};
use crate::exec_output::{ExecTerminalReceipt, RunTerminationReason};
use crate::features::render_feature_table;
use crate::mcp::{McpPool, McpServerConfig, McpServerOAuthConfig, McpWriteStatus};
use crate::tui::history::summarize_tool_output;

#[cfg(windows)]
fn configure_windows_console_utf8() {
    use windows::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};

    const CP_UTF8: u32 = 65001;
    unsafe {
        let _ = SetConsoleCP(CP_UTF8);
        let _ = SetConsoleOutputCP(CP_UTF8);
    }
}

#[cfg(not(windows))]
fn configure_windows_console_utf8() {}

fn install_rustls_crypto_provider() {
    crate::tls::ensure_rustls_crypto_provider();
}

#[derive(Parser, Debug)]
#[command(
    name = "dse-tui",
    bin_name = "dse-tui",
    author,
    version = env!("DSE_BUILD_VERSION"),
    about = "DSE terminal coding agent",
    long_about = "DeepSeek-only terminal coding agent.\n\nRun 'dse' to start."
)]
struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    feature_toggles: FeatureToggles,

    /// Initial prompt to submit in the interactive TUI. Use `exec` for non-interactive runs.
    #[arg(short, long, value_name = "PROMPT", num_args = 1..)]
    prompt: Vec<String>,

    /// Explicit startup override: use Full access for future Runs in this process.
    #[arg(long, hide = true)]
    yolo: bool,

    /// Maximum number of concurrent sub-agents (1-20)
    #[arg(long)]
    max_subagents: Option<usize>,

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Human interface language: en or zh-Hans.
    #[arg(long, value_name = "LANGUAGE")]
    language: Option<ProductLanguage>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Config profile name
    #[arg(long)]
    profile: Option<String>,

    /// Workspace directory for file operations
    #[arg(short, long)]
    workspace: Option<PathBuf>,

    /// Resume a canonical Agent run by exact Run ID
    #[arg(short, long)]
    resume: Option<String>,

    /// Resume the newest canonical Agent run in this workspace
    #[arg(short = 'c', long = "continue")]
    continue_session: bool,

    /// Deprecated compatibility flag; the interactive TUI always owns the
    /// alternate screen so terminal scrollback cannot hijack the viewport.
    #[arg(long = "no-alt-screen", hide = true)]
    no_alt_screen: bool,

    /// Enable TUI mouse capture for internal scrolling, scrollbar dragging,
    /// and supported secondary-surface interactions
    /// (default off on Windows)
    #[arg(long = "mouse-capture", conflicts_with = "no_mouse_capture")]
    mouse_capture: bool,

    /// Disable TUI mouse capture so terminal-native text selection works
    #[arg(long = "no-mouse-capture", conflicts_with = "mouse_capture")]
    no_mouse_capture: bool,

    /// Skip onboarding screens
    #[arg(long)]
    skip_onboarding: bool,

    /// Skip loading project-level config from $WORKSPACE/.dse/config.toml
    #[arg(long = "no-project-config")]
    no_project_config: bool,
}

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Run system diagnostics and check configuration
    Doctor(DoctorArgs),
    /// Summarize failure signals from a local JSONL session log without raw content
    SessionDiagnostics(SessionDiagnosticsArgs),
    /// Bootstrap MCP config and/or skills directories
    Setup(SetupArgs),
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Create default AGENTS.md in current directory
    Init,
    /// Save an API key to the shared user config
    Login {
        /// API key to store (otherwise read from stdin)
        #[arg(long)]
        api_key: Option<String>,
    },
    /// Remove the saved API key
    Logout,
    /// Run a non-interactive prompt. Use --auto for agent-with-tools mode.
    Exec(ExecArgs),
    /// Open the TUI pre-seeded with a GitHub PR's title, body, and diff
    Pr {
        /// PR number
        #[arg(value_name = "NUMBER")]
        number: u32,
        /// Repository in `owner/name` form. Defaults to the current
        /// workspace's `gh` config (i.e. the repo gh thinks you're in).
        #[arg(short = 'R', long)]
        repo: Option<String>,
        /// Skip `gh pr checkout` even if gh is available. By default
        /// the working tree is left as-is — checkout is opt-in via
        /// `--checkout` because dirty trees fail it loudly.
        #[arg(long, default_value_t = false)]
        checkout: bool,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Execpolicy tooling
    Execpolicy(ExecpolicyCommand),
    /// Inspect feature flags
    Features(FeaturesCli),
    /// Resume a canonical Agent run by exact Run ID (use --last for newest)
    Resume {
        /// Exact canonical Run ID
        #[arg(value_name = "RUN_ID")]
        session_id: Option<String>,
        /// Resume the newest canonical run in this workspace
        #[arg(long = "last", default_value_t = false, conflicts_with = "session_id")]
        last: bool,
    },
}

#[derive(Args, Debug, Clone)]
#[command(after_help = "\
Examples:
  dse exec \"explain this function\"
  dse exec --auto \"list crates/ with ls\"
  dse exec --auto --output-format stream-json \"fix the failing test\"

Plain `dse exec` is a one-shot model response. Use `--auto` for
non-interactive agent-with-tools execution with the Agent decides preset.
Host-classified critical calls still require approval and therefore fail
closed in headless execution. Only the explicit process-level `--yolo`
override selects Full access.
")]
struct ExecArgs {
    /// Override model for this run
    #[arg(long)]
    model: Option<String>,
    /// Retired provider selector. Kept as a fail-closed parser guard so the
    /// trailing prompt cannot reinterpret `--provider` as user text.
    #[arg(
        long = "provider",
        hide = true,
        value_name = "RETIRED",
        value_parser = reject_retired_provider_argument
    )]
    _retired_provider: Option<String>,
    /// Override reasoning/thinking effort for this run.
    /// Accepted values: off, low, medium, high, max.
    #[arg(long = "reasoning-effort", value_name = "EFFORT")]
    reasoning_effort: Option<String>,
    /// Enable tool-backed Agent mode. Host-classified high-risk operations
    /// still require approval and fail closed in this headless surface.
    #[arg(long, default_value_t = false)]
    auto: bool,
    /// Emit machine-readable JSON output
    #[arg(long, default_value_t = false, conflicts_with = "output_format")]
    json: bool,
    /// Resume a durable Agent run by its exact run ID
    #[arg(long, value_name = "RUN_ID", conflicts_with = "continue_session")]
    resume: Option<String>,
    /// Continue the most recent terminal root run with a new prompt
    #[arg(long = "continue", default_value_t = false, conflicts_with = "resume")]
    continue_session: bool,
    /// Output format for exec mode
    #[arg(long, value_enum, default_value_t = ExecOutputFormat::Text)]
    output_format: ExecOutputFormat,
    /// Comma-separated list of tools to allow (all others denied).
    /// Lowercase catalog names: read_file, write_file, exec_shell, grep_files, etc.
    #[arg(long, value_delimiter = ',')]
    allowed_tools: Option<Vec<String>>,
    /// Comma-separated list of tools to deny (deny wins over allow).
    #[arg(long, value_delimiter = ',')]
    disallowed_tools: Option<Vec<String>>,
    /// Maximum number of model steps (tool calls) before the run ends.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    max_turns: Option<u32>,
    /// Maximum number of real DeepSeek HTTP requests started by this run.
    #[arg(long, value_name = "COUNT")]
    max_api_requests: Option<NonZeroU32>,
    /// Hard wall-clock limit for the Headless Agent runtime.
    #[arg(long, value_name = "SECONDS")]
    max_runtime_secs: Option<NonZeroU64>,
    /// Extra text appended to the system prompt for this run.
    #[arg(long)]
    append_system_prompt: Option<String>,
    /// Prompt to send to the model; omitted only when resuming a durable run
    #[arg(
        value_name = "PROMPT",
        required_unless_present = "resume",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    prompt: Vec<String>,
}

fn reject_retired_provider_argument(_value: &str) -> Result<String, String> {
    Err(tr(MessageId::MainProviderRemoved).into_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ExecOutputFormat {
    Text,
    #[value(name = "stream-json")]
    StreamJson,
}

const DEFAULT_EXEC_MAX_RUNTIME_SECS: u64 = 30 * 60;
const MAX_EXEC_MAX_RUNTIME_SECS: u64 = 24 * 60 * 60;
const EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS: u64 = 30;
const EXEC_OUTPUT_QUEUE_CAPACITY: usize = 256;
const EXEC_OUTPUT_CLOSE_TIMEOUT_SECS: u64 = 2;

const DSE_TOOL_SURFACE_ENV: &str = "DSE_TOOL_SURFACE";
const SHELL_ONLY_EXEC_TOOLS: &[&str] = &["exec_shell", "exec_shell_wait", "exec_shell_interact"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecToolSurface {
    ShellOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecPermissionControls {
    permission_mode: RunPermissionMode,
    tool_mode: bool,
}

fn resolve_exec_permission_controls(
    cli_auto: bool,
    yolo: bool,
    explicit_tool_surface: bool,
) -> ExecPermissionControls {
    let permission_mode = if yolo {
        RunPermissionMode::FullAccess
    } else if cli_auto {
        RunPermissionMode::Agent
    } else {
        RunPermissionMode::Ask
    };
    ExecPermissionControls {
        permission_mode,
        // A permission preset never grants a tool surface by itself. Only an
        // explicit Agent/tool request may turn a one-shot completion into a
        // tool-using run.
        tool_mode: cli_auto || yolo || explicit_tool_surface,
    }
}

fn exec_tool_surface_from_env() -> Option<ExecToolSurface> {
    std::env::var(DSE_TOOL_SURFACE_ENV)
        .ok()
        .and_then(|value| {
            if should_warn_unknown_exec_tool_surface(&value) {
                eprintln!(
                    "warning: unrecognized {DSE_TOOL_SURFACE_ENV}; leaving exec tool surface unchanged. Use `shell-only`, `full`, or `native-tools`."
                );
            }
            parse_exec_tool_surface(&value)
        })
}

fn parse_exec_tool_surface(value: &str) -> Option<ExecToolSurface> {
    match value.trim().to_ascii_lowercase().as_str() {
        "shell-only" | "shell_only" | "shell" => Some(ExecToolSurface::ShellOnly),
        "full" | "native-tools" | "native_tools" | "" => None,
        _ => None,
    }
}

fn should_warn_unknown_exec_tool_surface(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !matches!(
        normalized.as_str(),
        "" | "shell-only" | "shell_only" | "shell" | "full" | "native-tools" | "native_tools"
    )
}

fn normalize_exec_tool_names(tools: &[String]) -> Vec<String> {
    tools
        .iter()
        .map(|name| name.to_ascii_lowercase().trim().to_string())
        .collect()
}

fn shell_only_exec_allowed_tools() -> Vec<String> {
    SHELL_ONLY_EXEC_TOOLS
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn resolve_exec_allowed_tools(
    cli_allowed_tools: Option<&[String]>,
    env_tool_surface: Option<ExecToolSurface>,
) -> Option<Vec<String>> {
    if let Some(tools) = cli_allowed_tools {
        return Some(normalize_exec_tool_names(tools));
    }

    env_tool_surface.map(|ExecToolSurface::ShellOnly| shell_only_exec_allowed_tools())
}

/// Spawn a tokio task that listens for terminating signals (SIGINT
/// always; SIGTERM and SIGHUP on Unix) and, on receipt, restores the
/// terminal modes and exits with the conventional 128 + signal code.
/// Multiple deliveries are tolerated: once the cleanup runs, a second
/// signal short-circuits to plain exit so a stuck cleanup can never
/// trap a frustrated user pressing Ctrl+C repeatedly.
///
/// See the call site in `main` for the rationale (#1583).
fn spawn_signal_cleanup_task() {
    tokio::spawn(async {
        let exit_code = wait_for_terminating_signal().await;
        // If we get here a fatal signal arrived. Restore the terminal
        // and exit. A second signal during cleanup re-enters this
        // path and aborts via `std::process::exit` directly.
        static CLEANED_UP: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !CLEANED_UP.swap(true, std::sync::atomic::Ordering::SeqCst) {
            crate::tui::ui::emergency_restore_terminal();
        }
        std::process::exit(exit_code);
    });
}

#[cfg(unix)]
async fn wait_for_terminating_signal() -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    // Failing to install any individual stream is non-fatal: we still
    // want the others to work. The fallback never-resolving future
    // keeps `select!` well-typed when a stream fails to register.
    let mut sigint = signal(SignalKind::interrupt()).ok();
    let mut sigterm = signal(SignalKind::terminate()).ok();
    let mut sighup = signal(SignalKind::hangup()).ok();
    tokio::select! {
        _ = async { match sigint.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 130,
        _ = async { match sigterm.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 143,
        _ = async { match sighup.as_mut() { Some(s) => { s.recv().await; }, None => std::future::pending::<()>().await, } } => 129,
    }
}

#[cfg(not(unix))]
async fn wait_for_terminating_signal() -> i32 {
    // Windows: tokio::signal::ctrl_c covers both Ctrl+C and Ctrl+Break
    // (CTRL_C_EVENT / CTRL_BREAK_EVENT). Console-close, logoff, and
    // shutdown events are not currently routed through tokio.
    let _ = tokio::signal::ctrl_c().await;
    130
}

fn spawn_exec_signal_controller() -> (
    tokio::sync::watch::Receiver<Option<i32>>,
    tokio::task::JoinHandle<()>,
    std::sync::Arc<std::sync::atomic::AtomicI32>,
) {
    use std::sync::atomic::{AtomicI32, Ordering};

    let (tx, rx) = tokio::sync::watch::channel(None);
    let phase = std::sync::Arc::new(AtomicI32::new(0));
    let controller_phase = std::sync::Arc::clone(&phase);
    let task = tokio::spawn(async move {
        let first = wait_for_terminating_signal().await;
        // 0 = execution is cancellable, positive = first signal won the
        // terminal race, -1 = terminal outcome has been committed. The CAS is
        // the single linearization point: a signal can never produce a success
        // receipt and a canceled process exit for the same execution.
        if controller_phase
            .compare_exchange(0, first, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let _ = tx.send(Some(first));
        }
        // The first signal is cooperatively latched for structured settlement.
        // A second signal is an explicit emergency escape from stuck cleanup
        // or output I/O and must not depend on the controller being polled.
        let second = wait_for_terminating_signal().await;
        crate::tui::ui::emergency_restore_terminal();
        std::process::exit(second);
    });
    (rx, task, phase)
}

fn commit_exec_terminal_signal(phase: &std::sync::atomic::AtomicI32) -> Option<i32> {
    use std::sync::atomic::Ordering;

    loop {
        let current = phase.load(Ordering::SeqCst);
        if current < 0 {
            return None;
        }
        if phase
            .compare_exchange(current, -1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return (current > 0).then_some(current);
        }
    }
}

#[cfg(test)]
mod exec_terminal_signal_tests {
    use std::sync::atomic::{AtomicI32, Ordering};

    use super::commit_exec_terminal_signal;

    #[test]
    fn runtime_terminal_claim_prevents_a_later_signal_exit() {
        let phase = AtomicI32::new(0);

        assert_eq!(commit_exec_terminal_signal(&phase), None);
        assert_eq!(phase.load(Ordering::SeqCst), -1);
        assert_eq!(commit_exec_terminal_signal(&phase), None);
    }

    #[test]
    fn an_already_latched_signal_suppresses_the_runtime_terminal_claim() {
        let phase = AtomicI32::new(143);

        assert_eq!(commit_exec_terminal_signal(&phase), Some(143));
        assert_eq!(phase.load(Ordering::SeqCst), -1);
    }
}

async fn recv_exec_signal(rx: &mut tokio::sync::watch::Receiver<Option<i32>>) -> Option<i32> {
    loop {
        if let Some(code) = *rx.borrow() {
            return Some(code);
        }
        if rx.changed().await.is_err() {
            return None;
        }
    }
}

async fn stop_exec_signal_controller(task: &mut Option<tokio::task::JoinHandle<()>>) {
    let Some(task) = task.take() else {
        return;
    };
    task.abort();
    let _ = task.await;
}

enum ExecOutputWait {
    Written,
    WatchdogTimeout,
    Signal(Option<i32>),
    Failed(String),
}

async fn wait_exec_output_until<F>(
    write: F,
    deadline: tokio::time::Instant,
    signal_rx: &mut tokio::sync::watch::Receiver<Option<i32>>,
) -> ExecOutputWait
where
    F: std::future::Future<Output = Result<(), crate::exec_output::ExecOutputError>>,
{
    tokio::select! {
        signal = recv_exec_signal(signal_rx) => ExecOutputWait::Signal(signal),
        result = tokio::time::timeout_at(deadline, write) => match result {
            Ok(Ok(())) => ExecOutputWait::Written,
            Ok(Err(error)) => ExecOutputWait::Failed(error.to_string()),
            Err(_) => ExecOutputWait::WatchdogTimeout,
        },
    }
}

async fn wait_terminal_output<F>(enqueue: F) -> Result<bool, String>
where
    F: std::future::Future<
            Output = Result<crate::exec_output::ExecOutputAck, crate::exec_output::ExecOutputError>,
        >,
{
    let acknowledgement =
        match tokio::time::timeout(Duration::from_secs(EXEC_OUTPUT_CLOSE_TIMEOUT_SECS), enqueue)
            .await
        {
            Ok(Ok(acknowledgement)) => acknowledgement,
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) => {
                return Err(format!(
                    "输出事件在 {} 秒内未能进入有界队列",
                    EXEC_OUTPUT_CLOSE_TIMEOUT_SECS
                ));
            }
        };

    match tokio::time::timeout(
        Duration::from_secs(EXEC_OUTPUT_CLOSE_TIMEOUT_SECS),
        acknowledgement.wait(),
    )
    .await
    {
        Ok(Ok(())) => Ok(true),
        Ok(Err(error)) => Err(error.to_string()),
        // The command is already owned by the writer. A later clean
        // close_and_join proves it was flushed; do not create a false
        // non-zero exit solely because its acknowledgement was slow.
        Err(_) => Ok(false),
    }
}

fn join_prompt_parts(parts: &[String]) -> String {
    parts.join(" ")
}

fn resolve_exec_model(config: &Config, explicit_model: Option<&str>) -> String {
    explicit_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .or_else(exec_model_env_override)
        .unwrap_or_else(|| config.default_model())
}

fn resolve_interactive_deepseek_model(config: &Config) -> Result<String> {
    let Some(configured_model) = config.default_text_model.as_deref() else {
        return Ok(crate::config::DEFAULT_TEXT_MODEL.to_owned());
    };
    let configured_model = configured_model.trim();
    crate::config::normalize_model_name(configured_model).ok_or_else(|| {
        anyhow!(
            "{}",
            tr(MessageId::MainInvalidInteractiveModel).replace("{model}", configured_model)
        )
    })
}

fn exec_model_env_override() -> Option<String> {
    ["DSE_MODEL", "DEEPSEEK_MODEL"].into_iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty())
    })
}

fn top_level_prompt_initial_input(parts: &[String]) -> Option<tui::InitialInput> {
    (!parts.is_empty()).then(|| tui::InitialInput::Submit(join_prompt_parts(parts)))
}

fn resolve_exec_run_launch(args: &ExecArgs) -> Result<exec_runtime::ExecRunLaunch> {
    if let Some(id) = args.resume.as_ref() {
        return Ok(exec_runtime::ExecRunLaunch::Resume(id.clone()));
    }
    if !args.continue_session {
        return Ok(exec_runtime::ExecRunLaunch::Fresh);
    }
    if args.prompt.is_empty() {
        bail!("{}", tr(MessageId::MainContinueNeedsInput));
    }
    Ok(exec_runtime::ExecRunLaunch::ContinueLatest)
}

#[derive(Args, Debug, Clone, Default)]
struct SetupArgs {
    /// Initialize MCP configuration at the configured path
    #[arg(long, default_value_t = false)]
    mcp: bool,
    /// Initialize skills directory and an example skill
    #[arg(long, default_value_t = false)]
    skills: bool,
    /// Initialize plugins directory with a self-describing example
    #[arg(long, default_value_t = false)]
    plugins: bool,
    /// Initialize MCP config, skills, and plugins
    #[arg(long, default_value_t = false)]
    all: bool,
    /// Create a local workspace skills directory (./skills)
    #[arg(long, default_value_t = false)]
    local: bool,
    /// Overwrite existing template files
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Print a compact, read-only status report (no network calls)
    #[arg(long, default_value_t = false, conflicts_with_all = ["mcp", "skills", "plugins", "all", "local"])]
    status: bool,
}

#[derive(Args, Debug, Clone, Default)]
struct DoctorArgs {
    /// Emit machine-readable JSON output (skips live API connectivity check)
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Args, Debug, Clone)]
struct SessionDiagnosticsArgs {
    /// JSONL session log to inspect
    #[arg(value_name = "JSONL")]
    path: PathBuf,
    /// Emit machine-readable JSON with redacted source handles
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Args, Debug, Default, Clone)]
struct FeatureToggles {
    /// Enable a feature (repeatable). Equivalent to `features.<name>=true`.
    #[arg(long = "enable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    enable: Vec<String>,

    /// Disable a feature (repeatable). Equivalent to `features.<name>=false`.
    #[arg(long = "disable", value_name = "FEATURE", action = clap::ArgAction::Append, global = true)]
    disable: Vec<String>,
}

impl FeatureToggles {
    fn apply(&self, config: &mut Config) -> Result<()> {
        for feature in &self.enable {
            config.set_feature(feature, true)?;
        }
        for feature in &self.disable {
            config.set_feature(feature, false)?;
        }
        Ok(())
    }
}

#[derive(Subcommand, Debug, Clone)]
enum McpCommand {
    /// List configured MCP servers
    List,
    /// Create a template MCP config at the configured path
    Init {
        /// Overwrite an existing MCP config file
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Connect to MCP servers and report status
    Connect {
        /// Optional server name to connect to
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
    /// List tools discovered from MCP servers
    Tools {
        /// Optional server name to list tools for
        #[arg(value_name = "SERVER")]
        server: Option<String>,
    },
    /// Add an MCP server entry
    Add {
        /// Server name
        name: String,
        /// Command to launch stdio server
        #[arg(long, conflicts_with = "url")]
        command: Option<String>,
        /// URL for streamable HTTP/SSE server
        #[arg(long, conflicts_with = "command")]
        url: Option<String>,
        /// Explicit URL transport override. Use "sse" for legacy SSE endpoints.
        #[arg(long, requires = "url")]
        transport: Option<String>,
        /// Environment variable containing a bearer token for URL-based servers
        #[arg(long, requires = "url")]
        bearer_token_env_var: Option<String>,
        /// OAuth client ID for servers that do not support dynamic registration
        #[arg(long, requires = "url")]
        oauth_client_id: Option<String>,
        /// OAuth resource parameter to append to the authorization URL
        #[arg(long, requires = "url")]
        oauth_resource: Option<String>,
        /// OAuth scope to request during login. Repeat or comma-separate.
        #[arg(long = "scope", requires = "url", value_delimiter = ',')]
        scopes: Vec<String>,
        /// Arguments for command-based servers
        #[arg(long = "arg")]
        args: Vec<String>,
    },
    /// Authenticate to a URL-based MCP server using OAuth
    Login {
        /// Server name
        name: String,
        /// OAuth scope to request. Repeat or comma-separate; defaults to config/discovery.
        #[arg(long = "scope", value_delimiter = ',')]
        scopes: Vec<String>,
    },
    /// Delete stored OAuth credentials for a URL-based MCP server
    Logout {
        /// Server name
        name: String,
    },
    /// Remove an MCP server entry
    Remove {
        /// Server name
        name: String,
    },
    /// Enable an MCP server
    Enable {
        /// Server name
        name: String,
    },
    /// Disable an MCP server
    Disable {
        /// Server name
        name: String,
    },
    /// Validate MCP config and required servers
    Validate,
}

#[derive(Args, Debug, Clone)]
struct ExecpolicyCommand {
    #[command(subcommand)]
    command: ExecpolicySubcommand,
}

#[derive(Subcommand, Debug, Clone)]
enum ExecpolicySubcommand {
    /// Check execpolicy files against a command
    Check(execpolicy::ExecPolicyCheckCommand),
}

#[derive(Args, Debug, Clone)]
struct FeaturesCli {
    #[command(subcommand)]
    command: FeaturesSubcommand,
}

#[derive(Subcommand, Debug, Clone)]
enum FeaturesSubcommand {
    /// List known feature flags and their state
    List,
}

const DSE_MAIN_STACK_BYTES: usize = 16 * 1024 * 1024;

fn tui_command_message(name: &str) -> Option<MessageId> {
    Some(match name {
        "doctor" => MessageId::CliCommandDoctor,
        "session-diagnostics" => MessageId::CliCommandSessionDiagnostics,
        "setup" => MessageId::CliCommandSetup,
        "completions" => MessageId::CliCommandCompletions,
        "init" => MessageId::CliCommandInit,
        "login" => MessageId::CliCommandLogin,
        "logout" => MessageId::CliCommandLogout,
        "exec" => MessageId::CliCommandExec,
        "pr" => MessageId::CliCommandPr,
        "mcp" => MessageId::CliCommandMcp,
        "execpolicy" => MessageId::CliCommandExecPolicy,
        "features" => MessageId::CliCommandFeatures,
        "resume" => MessageId::CliCommandResume,
        _ => return None,
    })
}

fn localize_tui_command(command: &mut clap::Command) {
    localize_tui_command_in(command, process_language());
}

fn localize_tui_command_in(command: &mut clap::Command, language: ProductLanguage) {
    let mut localized = command
        .clone()
        .help_template(tr_in(language, MessageId::CliHelpTemplate).into_owned())
        .disable_help_subcommand(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .long_about(None)
        .after_help(None)
        .mut_args(|argument| {
            if argument.get_action().takes_values() {
                argument.hide_possible_values(true)
            } else {
                argument
            }
        });
    localized = localized.arg(
        clap::Arg::new("help")
            .short('h')
            .long("help")
            .action(clap::ArgAction::Help)
            .help(tr_in(language, MessageId::CliArgHelp).into_owned()),
    );
    if command.get_version().is_some() {
        localized = localized.arg(
            clap::Arg::new("version")
                .short('V')
                .long("version")
                .action(clap::ArgAction::Version)
                .help(tr_in(language, MessageId::CliArgVersion).into_owned()),
        );
    }
    if command.get_name() == "dse-tui" {
        localized = localized.about(tr_in(language, MessageId::CliAbout).into_owned());
    } else if let Some(message) = tui_command_message(command.get_name()) {
        localized = localized.about(tr_in(language, message).into_owned());
    }
    if command.get_name() == "exec" {
        localized = localized.after_help(tr_in(language, MessageId::CliExecAfterHelp).into_owned());
    }

    let arg_messages = [
        ("workspace", MessageId::CliArgWorkspace),
        ("continue_session", MessageId::CliArgContinue),
        ("prompt", MessageId::CliArgPrompt),
        ("api_key", MessageId::CliArgApiKey),
        ("json", MessageId::CliArgJson),
        ("enable", MessageId::CliArgEnableFeature),
        ("disable", MessageId::CliArgDisableFeature),
        ("max_subagents", MessageId::CliArgMaxSubagents),
        ("config", MessageId::CliArgConfig),
        ("language", MessageId::CliArgLanguage),
        ("verbose", MessageId::CliArgVerbose),
        ("profile", MessageId::CliArgProfile),
        ("resume", MessageId::CliArgResume),
        ("mouse_capture", MessageId::CliArgMouseCapture),
        ("no_mouse_capture", MessageId::CliArgNoMouseCapture),
        ("skip_onboarding", MessageId::CliArgSkipOnboarding),
        ("no_project_config", MessageId::CliArgNoProjectConfig),
        ("model", MessageId::CliArgModel),
        ("reasoning_effort", MessageId::CliArgReasoningEffort),
        ("auto", MessageId::CliArgAuto),
        ("output_format", MessageId::CliArgOutputFormat),
        ("allowed_tools", MessageId::CliArgAllowedTools),
        ("disallowed_tools", MessageId::CliArgDisallowedTools),
        ("max_turns", MessageId::CliArgMaxTurns),
        ("max_api_requests", MessageId::CliArgMaxApiRequests),
        ("max_runtime_secs", MessageId::CliArgMaxRuntimeSecs),
        ("append_system_prompt", MessageId::CliArgAppendSystemPrompt),
    ];
    for (id, message) in arg_messages {
        if localized
            .get_arguments()
            .any(|argument| argument.get_id() == id)
        {
            localized = localized.mut_arg(id, |argument| {
                argument.help(tr_in(language, message).into_owned())
            });
        }
    }

    *command = localized;
    for child in command.get_subcommands_mut() {
        localize_tui_command_in(child, language);
    }
}

fn localized_tui_command() -> clap::Command {
    let mut command = Cli::command();
    localize_tui_command(&mut command);
    command
}

fn parse_tui_cli() -> Cli {
    initialize_process_language_from_args();
    let matches = match localized_tui_command().try_get_matches() {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.exit()
        }
        Err(error) => {
            eprintln!("{}", tr(MessageId::CliArgumentError));
            error.exit()
        }
    };
    Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
}

fn initialize_process_language_from_args() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let (explicit, config_path) = startup_language_arguments(&arguments);
    let explicit = explicit
        .as_deref()
        .and_then(|value| value.parse::<ProductLanguage>().ok());
    let persisted = Config::load(config_path, None)
        .ok()
        .and_then(|config| config.ui_language().ok().flatten());
    let existing_dse_installation = dse_config::dse_home()
        .ok()
        .is_some_and(|home| home.join(".onboarded").is_file());
    let has_noninteractive_surface = arguments.iter().skip(1).any(|argument| {
        matches!(
            argument.to_str(),
            Some(
                "-h" | "--help"
                    | "-V"
                    | "--version"
                    | "doctor"
                    | "session-diagnostics"
                    | "setup"
                    | "completions"
                    | "init"
                    | "login"
                    | "logout"
                    | "exec"
                    | "mcp"
                    | "execpolicy"
                    | "features"
            )
        )
    });
    let defer_fresh_to_first_run_choice =
        !has_noninteractive_surface && io::stdin().is_terminal() && io::stdout().is_terminal();
    let language = resolve_product_language(
        explicit,
        persisted,
        existing_dse_installation,
        defer_fresh_to_first_run_choice,
    );
    if let Some(language) = language {
        let _ = set_process_language(language);
    }
}

fn startup_language_arguments(
    arguments: &[std::ffi::OsString],
) -> (Option<String>, Option<PathBuf>) {
    fn option_value(
        arguments: &[std::ffi::OsString],
        long_name: &str,
    ) -> Option<std::ffi::OsString> {
        let prefix = format!("{long_name}=");
        let mut index = 1;
        while index < arguments.len() {
            let value = arguments[index].to_string_lossy();
            if value == long_name {
                return arguments.get(index + 1).cloned();
            }
            if let Some(value) = value.strip_prefix(&prefix) {
                return Some(value.into());
            }
            index += 1;
        }
        None
    }

    let explicit =
        option_value(arguments, "--language").map(|value| value.to_string_lossy().into_owned());
    let config_path = option_value(arguments, "--config").map(PathBuf::from);
    (explicit, config_path)
}

fn initialize_first_run_language(cli: &Cli) -> Result<()> {
    if process_language_is_set() {
        return Ok(());
    }

    let opens_interactive_tui = cli.command.is_none()
        || matches!(
            &cli.command,
            Some(Commands::Pr { .. } | Commands::Resume { .. })
        );
    let can_ask = opens_interactive_tui
        && !cli.skip_onboarding
        && io::stdin().is_terminal()
        && io::stdout().is_terminal();
    let language = if can_ask {
        print!(
            "DSE · DeepSeek Engineer\n\
             Choose interface language / 选择界面语言\n\
             1) English\n\
             2) 简体中文\n\
             > "
        );
        io::stdout()
            .flush()
            .context("Failed to display language choice / 无法显示语言选择")?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .context("Failed to read language choice / 无法读取语言选择")?;
        parse_first_run_language_choice(&answer)?
    } else {
        ProductLanguage::English
    };

    set_process_language(language).map_err(|existing| {
        anyhow!(
            "Product language is already frozen as {existing}; cannot select {language} / \
             产品语言已固定为 {existing}，不能再选择 {language}"
        )
    })?;
    if can_ask {
        let mut store = dse_config::ConfigStore::load(cli.config.clone())?;
        store.config.set_value("ui.language", language.tag())?;
        store.save()?;
    }
    Ok(())
}

fn parse_first_run_language_choice(value: &str) -> Result<ProductLanguage> {
    match value.trim() {
        "" | "1" | "en" | "English" | "english" => Ok(ProductLanguage::English),
        "2" | "zh-Hans" | "简体中文" => Ok(ProductLanguage::SimplifiedChinese),
        other => bail!(
            "Invalid language choice {other:?}; enter 1 for English or 2 for 简体中文 / \
             无效语言选择 {other:?}；请输入 1 选择 English 或 2 选择简体中文"
        ),
    }
}

fn render_main_error(error: &anyhow::Error) -> String {
    let mut lines = vec![tr(MessageId::CliErrorPrefix).replace("{error}", &error.to_string())];
    lines.extend(error.chain().skip(1).map(|cause| {
        format!(
            "  {}",
            tr(MessageId::CliCausedByPrefix).replace("{error}", &cause.to_string())
        )
    }));
    lines.join("\n")
}

fn main() -> std::process::ExitCode {
    match run_main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", render_main_error(&error));
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_main() -> Result<()> {
    // Match the dispatcher entrypoint: Unix shells and supervisors may inherit
    // SIGPIPE ignored, which turns short pipelines such as `dse doctor |
    // head` into BrokenPipe panics once this delegated TUI binary prints.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    startup_trace::mark_process_start();
    configure_windows_console_utf8();
    install_rustls_crypto_provider();

    // ── Process hardening (#2183) ─────────────────────────────────────────
    // MUST run before Tokio is booted and before any threads are spawned.
    // See crates/tui/src/sandbox/process_hardening.rs for ordering rationale.
    dse_tools::sandbox::process_hardening::apply_process_hardening();

    // Set up process panic hook before anything else — writes crash dumps
    // to the canonical DSE crash directory before tokio is up,
    // and restores the terminal so a panicked TUI doesn't leave the user's
    // shell stuck in alt-screen mode.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore the terminal first so the panic message itself, plus the
        // user's shell after exit, are visible. Best-effort — we may not be
        // in raw / alt-screen mode if the panic happens pre-TUI. Shared
        // with the signal handler installed below so both exit paths leave
        // the terminal in the same well-defined state.
        crate::tui::ui::emergency_restore_terminal();

        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", panic_info.payload())
        };
        let location = panic_info
            .location()
            .map(|loc| loc.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        tracing::error!(target: "panic", "Process panicked at {location}: {msg}");
        // Write crash dump best-effort
        if let Ok(home) = dse_config::dse_home() {
            let crash_dir = home.join("crashes");
            let _ = std::fs::create_dir_all(&crash_dir);
            use chrono::Utc;
            let ts = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
            let path = crash_dir.join(format!("{ts}-process-panic.log"));
            let contents =
                format!("Process panicked\nLocation: {location}\nTimestamp: {ts}\nPanic: {msg}\n",);
            let _ = std::fs::write(&path, contents);
        }
        // Invoke the original hook (prints to stderr, etc.)
        orig_hook(panic_info);
    }));

    // The interactive runtime intentionally carries a large state machine:
    // terminal rendering, secondary-surface dispatch, DeepSeek authentication, and Agent execution
    // events all share one async owner. Debug builds retain enough stack
    // temporaries that nesting a secondary-surface event over the TUI loop can exceed the
    // platform main-thread default (8 MiB on macOS). Give that owner an
    // explicit stack while keeping process hardening and the global panic hook
    // above this boundary, before Tokio or any worker thread exists.
    let runtime_thread = std::thread::Builder::new()
        .name("dse-main".to_string())
        .stack_size(DSE_MAIN_STACK_BYTES)
        .spawn(run_async_main)
        .context(tr(MessageId::MainRuntimeThreadStartFailed).into_owned())?;
    match runtime_thread.join() {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| tr(MessageId::MainUnknownPanicPayload).into_owned());
            Err(anyhow!(
                "{}",
                tr(MessageId::MainRuntimeThreadPanicked).replace("{message}", &message)
            ))
        }
    }
}

#[tokio::main]
async fn run_async_main() -> Result<()> {
    // Install signal handlers that restore the terminal before the
    // process exits. Without this, Ctrl+C delivered while raw mode /
    // kitty keyboard enhancement / alt-screen are active (or in the
    // brief windows around startup and teardown where they're being
    // toggled) leaves the user's shell receiving raw CSI sequences
    // like `^[[>5u` until they run `reset` (#1583).
    //
    // Once the TUI's raw mode is engaged the terminal driver delivers
    // Ctrl+C as the byte 0x03 rather than SIGINT, so the in-TUI key
    // handler — not this handler — is what processes user interrupts
    // during normal operation. This handler exists for the gaps:
    // pre-TUI subcommands (--version, doctor, login, …), the moments
    // around enable_raw_mode / disable_raw_mode, the external-editor
    // suspend path, and SIGTERM / SIGHUP from the OS.
    dotenv().ok();
    let cli = parse_tui_cli();
    initialize_first_run_language(&cli)?;
    // Engine-backed Headless exec installs its own structured controller.
    // Other commands retain the emergency terminal-restoration behavior.
    if !matches!(&cli.command, Some(Commands::Exec(_))) {
        spawn_signal_cleanup_task();
    }
    logging::set_verbose(cli.verbose || logging::env_requests_verbose_logging());

    // Install any user prompt overrides from the config directory before an
    // engine can compose a system prompt. The override cells are
    // first-call-wins; doing this once here keeps every downstream turn
    // consistent. Missing files are a no-op (bundled defaults). See #3638.
    crate::prompts::load_prompt_overrides_from_config_home();

    // Handle subcommands first
    if let Some(command) = cli.command.clone() {
        return match command {
            Commands::Doctor(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                if args.json {
                    run_doctor_json(&config, &workspace, cli.config.as_deref())
                } else {
                    run_doctor(&config, &workspace, cli.config.as_deref()).await;
                    Ok(())
                }
            }
            Commands::SessionDiagnostics(args) => run_session_diagnostics(args),
            Commands::Setup(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                run_setup(&config, &workspace, args)
            }
            Commands::Completions { shell } => {
                generate_completions(shell);
                Ok(())
            }
            Commands::Init => init_project(),
            Commands::Login { api_key } => run_login(api_key),
            Commands::Logout => run_logout(),
            Commands::Exec(args) => {
                let config = load_config_from_cli(&cli)?;
                let workspace = cli.workspace.clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                });
                let workspace = std::fs::canonicalize(&workspace).with_context(|| {
                    tr(MessageId::MainExecWorkspaceCanonicalizeFailed)
                        .replace("{path}", &workspace.display().to_string())
                })?;
                let mut config = config.clone();
                merge_user_workspace_config(&mut config, cli.config.clone(), &workspace);
                // Honour DEEPSEEK_BASE_URL forwarded by the CLI dispatcher from --base-url.
                if let Ok(env_url) = std::env::var("DEEPSEEK_BASE_URL") {
                    let trimmed = env_url.trim();
                    if !trimmed.is_empty() {
                        config.base_url = Some(trimmed.to_string());
                    }
                }
                if let Some(reasoning_arg) = args
                    .reasoning_effort
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    config.reasoning_effort = normalize_cli_reasoning_effort(reasoning_arg)?;
                }
                let model = resolve_exec_model(&config, args.model.as_deref());
                let prompt = join_prompt_parts(&args.prompt);
                let run_launch = resolve_exec_run_launch(&args)?;
                let yolo = cli.yolo;
                let env_tool_surface = exec_tool_surface_from_env();
                let max_subagents = cli.max_subagents.map_or_else(
                    || config.max_subagents(),
                    |value| value.clamp(1, MAX_SUBAGENTS),
                );
                // Positive authority enables tools; a deny-list can only
                // narrow an already-authorized surface and must never turn a
                // plain one-shot request into a filesystem-writing agent.
                let explicit_tool_surface =
                    args.allowed_tools.is_some() || env_tool_surface.is_some();
                let permission_controls =
                    resolve_exec_permission_controls(args.auto, yolo, explicit_tool_surface);
                let max_turns = args.max_turns.unwrap_or(100);
                let allowed_tools =
                    resolve_exec_allowed_tools(args.allowed_tools.as_deref(), env_tool_surface);
                let disallowed_tools = args
                    .disallowed_tools
                    .as_deref()
                    .map(normalize_exec_tool_names);
                exec_runtime::run_exec_runtime(
                    &config,
                    &model,
                    &prompt,
                    workspace,
                    max_subagents,
                    permission_controls.permission_mode,
                    permission_controls.tool_mode,
                    args.json,
                    run_launch,
                    args.output_format,
                    max_turns,
                    args.max_api_requests,
                    args.max_runtime_secs
                        .map_or(DEFAULT_EXEC_MAX_RUNTIME_SECS, NonZeroU64::get),
                    allowed_tools,
                    disallowed_tools,
                    args.append_system_prompt.clone(),
                )
                .await
            }
            Commands::Pr {
                number,
                repo,
                checkout,
            } => {
                let config = load_config_from_cli(&cli)?;
                run_pr(&cli, &config, number, repo.as_deref(), checkout).await
            }
            Commands::Mcp { command } => {
                let config = load_config_from_cli(&cli)?;
                let workspace = resolve_workspace(&cli);
                run_mcp_command(&config, &workspace, command).await
            }
            Commands::Execpolicy(command) => run_execpolicy_command(command),
            Commands::Features(command) => {
                let config = load_config_from_cli(&cli)?;
                run_features_command(&config, command)
            }
            Commands::Resume { session_id, last } => {
                let config = load_config_from_cli(&cli)?;
                let resume_id = if last {
                    "latest".to_owned()
                } else {
                    session_id.ok_or_else(|| anyhow!("{}", tr(MessageId::MainResumeIdRequired)))?
                };
                run_interactive(&cli, &config, Some(resume_id), None).await
            }
        };
    }

    // Top-level prompt mode: submit the initial prompt, then keep the TUI alive
    // for follow-up messages. Use `dse exec` for explicit non-interactive
    // one-shot behavior (#2370).
    let config = load_config_from_cli(&cli)?;
    crate::plugins::init_registry(&[]);
    if let Some(initial_input) = top_level_prompt_initial_input(&cli.prompt) {
        return run_interactive(&cli, &config, None, Some(initial_input)).await;
    }

    // Handle session resume. Plain `dse` starts fresh: interrupted
    // snapshots are preserved for explicit resume, but never auto-attached.
    let resume_session_id = if cli.continue_session {
        Some("latest".to_owned())
    } else {
        cli.resume.clone()
    };

    // Default: Interactive TUI
    // --yolo starts the interactive process with Full access and Shell enabled.
    run_interactive(&cli, &config, resume_session_id, None).await
}

/// Generate shell completions for the given shell
fn generate_completions(shell: Shell) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut io::stdout());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteStatus {
    Created,
    Overwritten,
    SkippedExists,
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            tr(MessageId::MainDirectoryCreateFailed)
                .replace("{path}", &parent.display().to_string())
        })?;
    }
    Ok(())
}

fn write_template_file(path: &Path, contents: &str, force: bool) -> Result<WriteStatus> {
    ensure_parent_dir(path)?;

    if path.exists() && !force {
        return Ok(WriteStatus::SkippedExists);
    }

    let status = if path.exists() {
        WriteStatus::Overwritten
    } else {
        WriteStatus::Created
    };

    std::fs::write(path, contents).with_context(|| {
        tr(MessageId::MainTemplateWriteFailed).replace("{path}", &path.display().to_string())
    })?;

    Ok(status)
}

fn skills_template(name: &str) -> String {
    format!(
        "\
---\n\
name: {name}\n\
description: Quick repo diagnostics and setup guidance\n\
allowed-tools: diagnostics, list_dir, read_file, grep_files, git_status, git_diff\n\
---\n\n\
When this skill is active:\n\
1. Run the diagnostics tool to report workspace and sandbox status.\n\
2. Skim key project files (README.md, Cargo.toml, AGENTS.md) before editing.\n\
3. Prefer small, validated changes and summarize what you verified.\n\
"
    )
}

fn init_skills_dir(skills_dir: &Path, force: bool) -> Result<(PathBuf, WriteStatus)> {
    std::fs::create_dir_all(skills_dir).with_context(|| {
        tr(MessageId::MainSkillsDirCreateFailed)
            .replace("{path}", &skills_dir.display().to_string())
    })?;

    let skill_name = "getting-started";
    let skill_path = skills_dir.join(skill_name).join("SKILL.md");
    ensure_parent_dir(&skill_path)?;

    let status = write_template_file(&skill_path, &skills_template(skill_name), force)?;
    Ok((skill_path, status))
}

fn plugins_readme_template() -> &'static str {
    "# Local plugins\n\n\
     Plugins are richer than tools: each one lives in its own subdirectory\n\
     with a `PLUGIN.md` describing what it does and how to enable it. The\n\
     directory is created so users have a documented place to drop\n\
     experiments without touching `~/.dse/skills/`.\n\n\
     A plugin layout looks like:\n\n\
     ```\n\
     plugins/\n\
       my-plugin/\n\
         PLUGIN.md   # frontmatter + body, same shape as SKILL.md\n\
         scripts/    # optional helpers invoked by the plugin\n\
     ```\n\n\
     Plugins are not loaded automatically. Wire them up through skills or\n\
     MCP servers when you want them active in a session.\n"
}

fn plugin_example_template() -> &'static str {
    "---\n\
     name: example\n\
     description: Placeholder plugin so /skills and doctor have something to show\n\
     status: example\n\
     ---\n\n\
     This is a starter plugin layout. Edit or replace it once you have a\n\
     real plugin. The agent does not load this file directly; reference it\n\
     from a skill or MCP wrapper if you want it active in a session.\n"
}

fn init_plugins_dir(
    plugins_dir: &Path,
    force: bool,
) -> Result<(PathBuf, PathBuf, WriteStatus, WriteStatus)> {
    std::fs::create_dir_all(plugins_dir).with_context(|| {
        tr(MessageId::MainPluginsDirCreateFailed)
            .replace("{path}", &plugins_dir.display().to_string())
    })?;

    let readme_path = plugins_dir.join("README.md");
    let readme_status = write_template_file(&readme_path, plugins_readme_template(), force)?;

    let example_path = plugins_dir.join("example").join("PLUGIN.md");
    ensure_parent_dir(&example_path)?;
    let example_status = write_template_file(&example_path, plugin_example_template(), force)?;

    Ok((readme_path, example_path, readme_status, example_status))
}

fn deepseek_home_dir() -> PathBuf {
    dse_config::dse_home().unwrap_or_else(|_| {
        dirs::home_dir().map_or_else(|| PathBuf::from(".dse"), |h| h.join(".dse"))
    })
}

/// Resolve the default plugins directory.
fn default_plugins_dir() -> PathBuf {
    deepseek_home_dir().join("plugins")
}

fn run_setup(config: &Config, workspace: &Path, args: SetupArgs) -> Result<()> {
    if args.status {
        return run_setup_status(config, workspace);
    }

    use crate::palette;
    use colored::Colorize;

    let (aqua_r, aqua_g, aqua_b) = palette::DSE_INFO_RGB;
    let (sky_r, sky_g, sky_b) = palette::DSE_INFO_RGB;

    let any_explicit = args.mcp || args.skills || args.plugins;
    let run_mcp = args.mcp || args.all || !any_explicit;
    let run_skills = args.skills || args.all || !any_explicit;
    let run_plugins = args.plugins || args.all;

    println!(
        "{}",
        tr(MessageId::MainSetupTitle)
            .truecolor(aqua_r, aqua_g, aqua_b)
            .bold()
    );
    println!("{}", "==============".truecolor(sky_r, sky_g, sky_b));
    println!(
        "{}",
        tr(MessageId::MainWorkspace).replace("{path}", &crate::utils::display_path(workspace))
    );

    if run_mcp {
        let mcp_path = config.mcp_config_path();
        let status = crate::mcp::init_config(&mcp_path, args.force)?;
        match status {
            McpWriteStatus::Created => {
                println!(
                    "{}",
                    tr(MessageId::MainCreated)
                        .replace("{label}", tr(MessageId::MainMcpConfigLabel).as_ref())
                        .replace("{path}", &mcp_path.display().to_string())
                );
            }
            McpWriteStatus::Overwritten => {
                println!(
                    "{}",
                    tr(MessageId::MainOverwritten)
                        .replace("{label}", tr(MessageId::MainMcpConfigLabel).as_ref())
                        .replace("{path}", &mcp_path.display().to_string())
                );
            }
            McpWriteStatus::SkippedExists => {
                println!(
                    "{}",
                    tr(MessageId::MainAlreadyExists)
                        .replace("{label}", tr(MessageId::MainMcpConfigLabel).as_ref())
                        .replace("{path}", &mcp_path.display().to_string())
                );
            }
        }
        println!("{}", tr(MessageId::MainSetupNextMcp));
    }

    if run_skills {
        let skills_dir = if args.local {
            workspace.join("skills")
        } else {
            config.skills_dir()
        };
        let (skill_path, status) = init_skills_dir(&skills_dir, args.force)?;
        report_write_status(
            tr(MessageId::MainExampleSkillLabel).as_ref(),
            &skill_path,
            status,
        );
        if args.local {
            println!(
                "{}",
                tr(MessageId::MainLocalSkillsDir)
                    .replace("{path}", &crate::utils::display_path(&skills_dir))
            );
        } else {
            println!(
                "{}",
                tr(MessageId::MainSkillsDir)
                    .replace("{path}", &crate::utils::display_path(&skills_dir))
            );
        }
        println!("{}", tr(MessageId::MainSetupNextSkills));
    }

    if run_plugins {
        let plugins_dir = default_plugins_dir();
        let (readme_path, example_path, readme_status, example_status) =
            init_plugins_dir(&plugins_dir, args.force)?;
        report_write_status(
            tr(MessageId::MainPluginsReadmeLabel).as_ref(),
            &readme_path,
            readme_status,
        );
        report_write_status(
            tr(MessageId::MainExamplePluginLabel).as_ref(),
            &example_path,
            example_status,
        );
        println!(
            "{}",
            tr(MessageId::MainPluginsDir)
                .replace("{path}", &crate::utils::display_path(&plugins_dir))
        );
        println!("{}", tr(MessageId::MainSetupNextPlugins));
    }

    let sandbox = dse_tools::sandbox::get_platform_sandbox();
    if let Some(kind) = sandbox {
        println!(
            "{}",
            tr(MessageId::MainSandboxAvailable).replace("{kind}", &kind.to_string())
        );
    } else {
        println!("{}", tr(MessageId::MainSandboxUnavailable));
    }

    Ok(())
}

fn report_write_status(label: &str, path: &Path, status: WriteStatus) {
    match status {
        WriteStatus::Created => {
            println!(
                "{}",
                tr(MessageId::MainCreated)
                    .replace("{label}", label)
                    .replace("{path}", &path.display().to_string())
            );
        }
        WriteStatus::Overwritten => {
            println!(
                "{}",
                tr(MessageId::MainOverwritten)
                    .replace("{label}", label)
                    .replace("{path}", &path.display().to_string())
            );
        }
        WriteStatus::SkippedExists => {
            println!(
                "{}",
                tr(MessageId::MainAlreadyExists)
                    .replace("{label}", label)
                    .replace("{path}", &path.display().to_string())
            );
        }
    }
}

/// Source of the resolved DeepSeek API key, used in status reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApiKeySource {
    Env,
    Config,
    Keyring,
    Missing,
}

fn resolve_api_key_source(config: &Config) -> ApiKeySource {
    if std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
    {
        match std::env::var("DEEPSEEK_API_KEY_SOURCE").ok().as_deref() {
            Some("config") => return ApiKeySource::Config,
            Some("keyring") => return ApiKeySource::Keyring,
            _ => {}
        }
    }

    let root_deepseek_key = config
        .api_key
        .as_ref()
        .is_some_and(|k| !k.trim().is_empty());

    if root_deepseek_key {
        ApiKeySource::Config
    } else if deepseek_env_key_source().is_some() {
        ApiKeySource::Env
    } else {
        ApiKeySource::Missing
    }
}

fn deepseek_env_key_source() -> Option<&'static str> {
    std::env::var(crate::config::DEEPSEEK_API_KEY_ENV)
        .is_ok_and(|value| !value.trim().is_empty())
        .then_some(crate::config::DEEPSEEK_API_KEY_ENV)
}

fn count_dir_entries(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(std::result::Result::ok).count())
        .unwrap_or(0)
}

fn skills_count_for(dir: &Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    crate::skill_context::SkillRegistry::discover(dir).len()
}

fn run_setup_status(config: &Config, workspace: &Path) -> Result<()> {
    use crate::palette;
    use colored::Colorize;

    let (aqua_r, aqua_g, aqua_b) = palette::DSE_INFO_RGB;
    let (sky_r, sky_g, sky_b) = palette::DSE_INFO_RGB;
    let (red_r, red_g, red_b) = palette::DSE_ERROR_RGB;

    println!(
        "{}",
        tr(MessageId::MainStatusTitle)
            .truecolor(aqua_r, aqua_g, aqua_b)
            .bold()
    );
    println!("{}", "===============".truecolor(sky_r, sky_g, sky_b));
    println!(
        "{}",
        tr(MessageId::MainWorkspace).replace("{path}", &workspace.display().to_string())
    );

    match resolve_api_key_source(config) {
        ApiKeySource::Env => {
            println!(
                "{}",
                tr(MessageId::MainStatusApiKeyEnv)
                    .replace("{icon}", &"✓".truecolor(aqua_r, aqua_g, aqua_b).to_string())
                    .replace("{env}", crate::config::DEEPSEEK_API_KEY_ENV)
            );
        }
        ApiKeySource::Keyring => println!(
            "{}",
            tr(MessageId::MainStatusApiKeyKeyring)
                .replace("{icon}", &"✓".truecolor(aqua_r, aqua_g, aqua_b).to_string())
        ),
        ApiKeySource::Config => println!(
            "{}",
            tr(MessageId::MainStatusApiKeyConfig)
                .replace("{icon}", &"✓".truecolor(aqua_r, aqua_g, aqua_b).to_string())
        ),
        ApiKeySource::Missing => {
            println!(
                "{}",
                tr(MessageId::MainStatusApiKeyMissing)
                    .replace("{icon}", &"✗".truecolor(red_r, red_g, red_b).to_string())
                    .replace("{env}", crate::config::DEEPSEEK_API_KEY_ENV)
            );
        }
    }
    println!(
        "{}",
        tr(MessageId::MainStatusBaseUrl).replace(
            "{url}",
            &crate::utils::redact_url_for_display(&config.deepseek_base_url())
        )
    );
    let model = config
        .default_text_model
        .clone()
        .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string());
    println!(
        "{}",
        tr(MessageId::MainStatusModel).replace("{model}", &model)
    );

    let mcp_path = config.mcp_config_path();
    let project_mcp_path = crate::mcp::workspace_mcp_config_path(workspace);
    let mcp_count = match crate::mcp::load_config_with_workspace(&mcp_path, workspace) {
        Ok(cfg) => cfg.servers.len(),
        Err(_) => 0,
    };
    let mcp_present = if mcp_path.exists() {
        Cow::Borrowed("")
    } else {
        tr(MessageId::MainStatusMissing)
    };
    let project_mcp_present = if project_mcp_path.exists() {
        Cow::Borrowed("")
    } else {
        tr(MessageId::MainStatusMissing)
    };
    println!(
        "{}",
        tr(MessageId::MainStatusMcpServers)
            .replace("{count}", &mcp_count.to_string())
            .replace("{global}", &mcp_path.display().to_string())
            .replace("{global_missing}", mcp_present.as_ref())
            .replace("{project}", &project_mcp_path.display().to_string())
            .replace("{project_missing}", project_mcp_present.as_ref())
    );

    let skills_dir = config.skills_dir();
    println!(
        "{}",
        tr(MessageId::MainStatusSkills)
            .replace("{count}", &skills_count_for(&skills_dir).to_string())
            .replace("{path}", &crate::utils::display_path(&skills_dir))
    );

    let plugins_dir = default_plugins_dir();
    let plugins_present = if plugins_dir.exists() {
        Cow::Borrowed("")
    } else {
        tr(MessageId::MainStatusPluginsMissing)
    };
    println!(
        "{}",
        tr(MessageId::MainStatusPlugins)
            .replace(
                "{count}",
                &if plugins_dir.exists() {
                    count_dir_entries(&plugins_dir)
                } else {
                    0
                }
                .to_string()
            )
            .replace("{path}", &crate::utils::display_path(&plugins_dir))
            .replace("{missing}", plugins_present.as_ref())
    );

    let sandbox = dse_tools::sandbox::get_platform_sandbox();
    match sandbox {
        Some(kind) => println!(
            "{}",
            tr(MessageId::MainStatusSandbox)
                .replace("{icon}", &"✓".truecolor(aqua_r, aqua_g, aqua_b).to_string())
                .replace("{kind}", &kind.to_string())
        ),
        None => println!(
            "{}",
            tr(MessageId::MainStatusSandboxUnavailable)
                .replace("{icon}", &"!".truecolor(sky_r, sky_g, sky_b).to_string())
        ),
    }

    println!("  {} {}", "·".dimmed(), dotenv_status_line(workspace));

    println!();
    println!("{}", tr(MessageId::MainDoctorJsonHint));
    Ok(())
}

fn dotenv_status_line(workspace: &Path) -> String {
    let dotenv = workspace.join(".env");
    if dotenv.exists() {
        return tr(MessageId::MainDotenvPresent).replace("{path}", &dotenv.display().to_string());
    }

    if workspace.join(".env.example").exists() {
        return tr(MessageId::MainDotenvExample).into_owned();
    }

    tr(MessageId::MainDotenvMissing).replace("{path}", &dotenv.display().to_string())
}

fn run_session_diagnostics(args: SessionDiagnosticsArgs) -> Result<()> {
    let contents = std::fs::read_to_string(&args.path).with_context(|| {
        format!(
            "read session diagnostic JSONL from {}",
            crate::utils::display_path(&args.path)
        )
    })?;
    let summary = crate::session_diagnostics::analyze_session_failure_jsonl(&contents);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!(
            "{}",
            crate::session_diagnostics::format_redacted_failure_summary(&summary)
        );
    }
    Ok(())
}

/// Run system diagnostics
async fn run_doctor(config: &Config, workspace: &Path, config_path_override: Option<&Path>) {
    use crate::palette;
    use colored::Colorize;

    let (accent_r, accent_g, accent_b) = palette::DSE_ACCENT_PRIMARY_RGB;
    let (sky_r, sky_g, sky_b) = palette::DSE_INFO_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::DSE_INFO_RGB;
    let (red_r, red_g, red_b) = palette::DSE_ERROR_RGB;

    println!(
        "{}",
        tr(MessageId::DoctorTitle)
            .truecolor(accent_r, accent_g, accent_b)
            .bold()
    );
    println!("{}", "==================".truecolor(sky_r, sky_g, sky_b));
    println!();

    // Version info
    println!("{}", tr(MessageId::DoctorSectionVersion).bold());
    println!("  dse-tui: {}", env!("DSE_BUILD_VERSION"));
    println!("  rust: {}", rustc_version());
    println!();

    println!("{}", tr(MessageId::DoctorSectionDelivery).bold());
    println!(
        "  · {}",
        tr(MessageId::DoctorInstalledBuild).replace("{version}", env!("DSE_BUILD_VERSION"))
    );
    println!("  · {}", tr(MessageId::DoctorUpdateDiscoveryDisabled));
    println!();

    // Configuration summary
    println!("{}", tr(MessageId::DoctorSectionConfiguration).bold());
    let config_path = config_path_override
        .map(PathBuf::from)
        .or_else(|| dse_config::resolve_config_path(None).ok())
        .unwrap_or_else(|| {
            dse_config::dse_home()
                .unwrap_or_else(|_| PathBuf::from(".dse"))
                .join("config.toml")
        });

    if config_path.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorConfigFound)
                .replace("{path}", &crate::utils::display_path(&config_path))
        );
    } else {
        println!(
            "  {} {}",
            "!".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorConfigNotFound)
                .replace("{path}", &crate::utils::display_path(&config_path))
        );
    }
    println!(
        "  {}",
        tr(MessageId::DoctorWorkspace).replace("{path}", &crate::utils::display_path(workspace))
    );
    println!("  {}", doctor_search_provider_line(config));

    // Canonical product state root
    println!();
    println!("{}", tr(MessageId::DoctorSectionStateRoot).bold());
    let code_home = dse_config::dse_home().unwrap_or_else(|_| PathBuf::from("~/.dse"));
    println!(
        "  {}",
        tr(MessageId::DoctorStateActive).replace("{path}", &crate::utils::display_path(&code_home))
    );

    let (setup_state, setup_source) = doctor_setup_state(config, workspace);
    print_doctor_setup_report(
        config,
        workspace,
        &setup_state,
        setup_source,
        (aqua_r, aqua_g, aqua_b),
        (sky_r, sky_g, sky_b),
    );

    // Check API keys
    println!();
    println!("{}", tr(MessageId::DoctorSectionApiKeys).bold());

    // DeepSeek state: env + config file only (no values printed).
    // Keep doctor/status prompt-free even for unsigned rebuilt binaries.
    let dispatcher_api_key_source = std::env::var("DEEPSEEK_API_KEY_SOURCE").ok();
    let in_env = deepseek_env_key_source().is_some();
    let injected_runtime_key = matches!(
        dispatcher_api_key_source.as_deref(),
        Some("keyring" | "env" | "cli")
    );
    let in_config = !injected_runtime_key && crate::config::has_config_api_key(config);
    let icon = if in_env || in_config {
        "✓".truecolor(aqua_r, aqua_g, aqua_b)
    } else {
        "·".dimmed()
    };
    println!(
        "  {} {}",
        icon,
        tr(MessageId::DoctorDeepseekKeyState)
            .replace(
                "{env}",
                if in_env {
                    tr(MessageId::MainDoctorYes)
                } else {
                    tr(MessageId::MainDoctorNo)
                }
                .as_ref(),
            )
            .replace(
                "{config}",
                if in_config {
                    tr(MessageId::MainDoctorYes)
                } else {
                    tr(MessageId::MainDoctorNo)
                }
                .as_ref(),
            )
    );
    println!("  · {}", tr(MessageId::DoctorCredentialPrecedence));

    let api_key_source = resolve_api_key_source(config);
    let has_api_key = if config.deepseek_api_key().is_ok() {
        let source_label: Cow<'static, str> = match api_key_source {
            ApiKeySource::Config => "config.toml".into(),
            ApiKeySource::Keyring => "OS keyring".into(),
            ApiKeySource::Env => "DEEPSEEK_API_KEY".into(),
            ApiKeySource::Missing => tr(MessageId::MainDoctorUnknownSource),
        };
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorActiveKeySource).replace("{source}", source_label.as_ref())
        );
        true
    } else {
        println!(
            "  {} {}",
            "✗".truecolor(red_r, red_g, red_b),
            tr(MessageId::DoctorActiveKeyMissing)
        );
        println!("    {}", tr(MessageId::DoctorSaveKeyHint));
        false
    };

    // API connectivity test
    println!();
    println!("{}", tr(MessageId::DoctorSectionApiConnectivity).bold());
    let api_target = doctor_api_target(config);
    println!(
        "  · {}",
        tr(MessageId::DoctorProvider).replace("{provider}", api_target.provider)
    );
    println!(
        "  · {}",
        tr(MessageId::DoctorBaseUrl).replace(
            "{base_url}",
            &crate::utils::redact_url_for_display(&api_target.base_url)
        )
    );
    println!(
        "  · {}",
        tr(MessageId::DoctorModel).replace("{model}", &api_target.model)
    );
    println!("  · {}", tr(MessageId::DoctorConnectivityScope));
    let tls_status = doctor_tls_status(config);
    if !tls_status.certificate_verification {
        println!(
            "  ! {}",
            tr(MessageId::DoctorTlsVerificationEnforced).replace("{provider}", tls_status.provider)
        );
    }
    if has_api_key {
        print!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorTestingConnection)
        );
        use std::io::Write;
        std::io::stdout().flush().ok();

        match test_api_connectivity(config).await {
            Ok(()) => {
                println!(
                    "\r  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorConnectionSuccessful)
                );
            }
            Err(e) => {
                let error_msg = e.to_string();
                println!(
                    "\r  {} {}",
                    "✗".truecolor(red_r, red_g, red_b),
                    tr(MessageId::DoctorConnectionFailed)
                );
                if error_msg.contains("401") || error_msg.contains("Unauthorized") {
                    println!("    {}", tr(MessageId::DoctorInvalidApiKey));
                    if matches!(api_key_source, ApiKeySource::Keyring) {
                        println!("    {}", tr(MessageId::DoctorRejectedKeyFromKeyring));
                        println!("    {}", tr(MessageId::DoctorInspectCredentialSources));
                    } else if matches!(api_key_source, ApiKeySource::Env) {
                        println!("    {}", tr(MessageId::DoctorRejectedKeyFromEnv));
                        println!("    {}", tr(MessageId::DoctorSaveConfigKeyOverridesEnv));
                    }
                } else if error_msg.contains("403") || error_msg.contains("Forbidden") {
                    println!("    {}", tr(MessageId::DoctorApiKeyPermissionDenied));
                } else if error_msg.contains("timeout") || error_msg.contains("Timeout") {
                    for line in doctor_timeout_recovery_lines(config) {
                        println!("    {line}");
                    }
                } else if error_msg.contains("dns") || error_msg.contains("resolve") {
                    println!("    {}", tr(MessageId::DoctorDnsFailure));
                } else if error_msg.contains("connect") {
                    println!("    {}", tr(MessageId::DoctorConnectFailure));
                } else {
                    println!(
                        "    {}",
                        tr(MessageId::DoctorRawError).replace("{error}", &error_msg)
                    );
                }
            }
        }
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorConnectionSkipped)
        );
    }

    // MCP configuration
    println!();
    println!("{}", tr(MessageId::DoctorSectionMcpServers).bold());
    let mcp_config_path = config.mcp_config_path();
    let project_mcp_config_path = crate::mcp::workspace_mcp_config_path(workspace);
    if mcp_config_path.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorMcpConfigFound)
                .replace("{path}", &crate::utils::display_path(&mcp_config_path))
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorMcpConfigMissing)
                .replace("{path}", &crate::utils::display_path(&mcp_config_path))
        );
    }
    if project_mcp_config_path.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorProjectMcpConfigFound).replace(
                "{path}",
                &crate::utils::display_path(&project_mcp_config_path)
            )
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorProjectMcpConfigMissing).replace(
                "{path}",
                &crate::utils::display_path(&project_mcp_config_path)
            )
        );
    }

    match crate::mcp::load_config_with_workspace(&mcp_config_path, workspace) {
        Ok(cfg) if cfg.servers.is_empty() => {
            println!(
                "  {} {}",
                "·".dimmed(),
                tr(MessageId::DoctorMcpMergedCount).replace("{count}", "0")
            );
            if !mcp_config_path.exists() && !project_mcp_config_path.exists() {
                println!("    {}", tr(MessageId::DoctorMcpInitHint));
            }
        }
        Ok(cfg) => {
            println!(
                "  {} {}",
                "·".dimmed(),
                tr(MessageId::DoctorMcpMergedCount)
                    .replace("{count}", &cfg.servers.len().to_string())
            );
            for (name, server) in &cfg.servers {
                let status = doctor_check_mcp_server(server);
                let icon = match status {
                    McpServerDoctorStatus::Ok(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "✓".truecolor(aqua_r, aqua_g, aqua_b),
                            detail
                        )
                    }
                    McpServerDoctorStatus::Warning(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "!".truecolor(sky_r, sky_g, sky_b),
                            detail
                        )
                    }
                    McpServerDoctorStatus::Error(ref detail) => {
                        format!(
                            "  {} {name}: {}",
                            "✗".truecolor(red_r, red_g, red_b),
                            detail
                        )
                    }
                };
                println!("{icon}");
                if !server.enabled {
                    println!("      ({})", tr(MessageId::DoctorMcpServerDisabled));
                }
            }
        }
        Err(err) => {
            println!(
                "  {} {}",
                "✗".truecolor(red_r, red_g, red_b),
                tr(MessageId::DoctorMcpConfigParseError).replace("{error}", &err.to_string())
            );
        }
    }

    // Skills configuration
    println!();
    println!("{}", tr(MessageId::DoctorSectionSkills).bold());
    let global_skills_dir = config.skills_dir();
    let agents_skills_dir = workspace.join(".agents").join("skills");
    let local_skills_dir = workspace.join("skills");
    let agents_global_skills_dir = crate::skill_context::agents_global_skills_dir();
    // #432: cross-tool skill discovery dirs. Presence is reported here
    // even though they sit lower in the precedence chain so users can
    // see at a glance whether a `.opencode/skills/`, `.claude/skills/`,
    // `.cursor/skills/`, or global agentskills.io directory is contributing
    // to the merged catalogue.
    let opencode_skills_dir = workspace.join(".opencode").join("skills");
    let claude_skills_dir = workspace.join(".claude").join("skills");
    let selected_skills_dir = if agents_skills_dir.exists() {
        agents_skills_dir.clone()
    } else if local_skills_dir.exists() {
        local_skills_dir.clone()
    } else if config.skills_dir.is_none()
        && let Some(global_agents) = agents_global_skills_dir.as_ref()
        && global_agents.exists()
    {
        global_agents.clone()
    } else {
        global_skills_dir.clone()
    };

    let describe_dir = |dir: &Path| -> usize {
        std::fs::read_dir(dir)
            .map(|entries| entries.filter_map(std::result::Result::ok).count())
            .unwrap_or(0)
    };

    if local_skills_dir.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorDirectoryFound)
                .replace("{label}", tr(MessageId::DoctorSkillWorkspaceLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&local_skills_dir))
                .replace("{count}", &describe_dir(&local_skills_dir).to_string())
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorDirectoryMissing)
                .replace("{label}", tr(MessageId::DoctorSkillWorkspaceLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&local_skills_dir))
        );
    }

    if agents_skills_dir.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorDirectoryFound)
                .replace("{label}", tr(MessageId::DoctorSkillAgentsLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&agents_skills_dir))
                .replace("{count}", &describe_dir(&agents_skills_dir).to_string())
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorDirectoryMissing)
                .replace("{label}", tr(MessageId::DoctorSkillAgentsLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&agents_skills_dir))
        );
    }

    if let Some(agents_global_skills_dir) = agents_global_skills_dir.as_ref() {
        if agents_global_skills_dir.exists() {
            println!(
                "  {} {}",
                "✓".truecolor(aqua_r, aqua_g, aqua_b),
                tr(MessageId::DoctorDirectoryFound)
                    .replace(
                        "{label}",
                        tr(MessageId::DoctorSkillGlobalAgentsLabel).as_ref(),
                    )
                    .replace(
                        "{path}",
                        &crate::utils::display_path(agents_global_skills_dir)
                    )
                    .replace(
                        "{count}",
                        &describe_dir(agents_global_skills_dir).to_string()
                    )
            );
        } else {
            println!(
                "  {} {}",
                "·".dimmed(),
                tr(MessageId::DoctorDirectoryMissing)
                    .replace(
                        "{label}",
                        tr(MessageId::DoctorSkillGlobalAgentsLabel).as_ref(),
                    )
                    .replace(
                        "{path}",
                        &crate::utils::display_path(agents_global_skills_dir)
                    )
            );
        }
    }

    if global_skills_dir.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorDirectoryFound)
                .replace("{label}", tr(MessageId::DoctorSkillGlobalLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&global_skills_dir))
                .replace("{count}", &describe_dir(&global_skills_dir).to_string())
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorDirectoryMissing)
                .replace("{label}", tr(MessageId::DoctorSkillGlobalLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&global_skills_dir))
        );
    }

    // #432: only print interop dirs when they're populated — empty
    // .opencode/.claude folders are common and would just clutter
    // the report with false-positive "absent" lines.
    if opencode_skills_dir.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorDirectoryFound)
                .replace("{label}", tr(MessageId::DoctorSkillOpencodeLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&opencode_skills_dir))
                .replace("{count}", &describe_dir(&opencode_skills_dir).to_string())
        );
    }
    if claude_skills_dir.exists() {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorDirectoryFound)
                .replace("{label}", tr(MessageId::DoctorSkillClaudeLabel).as_ref(),)
                .replace("{path}", &crate::utils::display_path(&claude_skills_dir))
                .replace("{count}", &describe_dir(&claude_skills_dir).to_string())
        );
    }

    println!(
        "  {} {}",
        "·".dimmed(),
        tr(MessageId::DoctorSelectedSkillsDirectory)
            .replace("{path}", &crate::utils::display_path(&selected_skills_dir))
    );
    if !agents_skills_dir.exists()
        && !local_skills_dir.exists()
        && !agents_global_skills_dir
            .as_ref()
            .is_some_and(|dir| dir.exists())
        && !global_skills_dir.exists()
    {
        println!("    {}", tr(MessageId::DoctorSkillsSetupHint));
    }

    // Plugins directory
    println!();
    println!("{}", tr(MessageId::DoctorSectionPlugins).bold());
    let plugins_dir = default_plugins_dir();
    if plugins_dir.exists() {
        let count = count_dir_entries(&plugins_dir);
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorPluginsFound)
                .replace("{path}", &crate::utils::display_path(&plugins_dir))
                .replace("{count}", &count.to_string())
        );
    } else {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorPluginsMissing)
                .replace("{path}", &crate::utils::display_path(&plugins_dir))
        );
        println!("    {}", tr(MessageId::DoctorPluginsHint));
    }

    // Tool dependencies — probe external binaries that individual
    // tools rely on (Python for code_execution, pdftotext for PDF
    // reading) so users see explicit ✓/✗ rather than the tool failing
    // at execution time with "program not found". New in v0.8.31.
    println!();
    println!("{}", tr(MessageId::DoctorSectionToolDependencies).bold());

    match crate::dependencies::resolve_python_interpreter() {
        Some(name) => println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorPythonAvailable).replace("{path}", &name)
        ),
        None => {
            println!(
                "  {} {}",
                "✗".truecolor(red_r, red_g, red_b),
                tr(MessageId::DoctorPythonMissing).replace(
                    "{candidates}",
                    &format!("{:?}", crate::dependencies::PYTHON_CANDIDATES)
                )
            );
            println!(
                "    {}",
                tr(MessageId::DoctorToolNotAdvertised).replace("{tool}", "code_execution")
            );
            println!(
                "    {}",
                tr(MessageId::DoctorInstallDependencyHint).replace("{dependency}", "Python 3")
            );
            match std::env::consts::OS {
                "macos" => println!("      {}", tr(MessageId::DoctorPythonMacInstall)),
                "linux" => println!("      {}", tr(MessageId::DoctorPythonLinuxInstall)),
                "windows" => println!("      {}", tr(MessageId::DoctorPythonWindowsInstall)),
                other => println!(
                    "      {}",
                    tr(MessageId::DoctorPythonOtherInstall).replace("{os}", other)
                ),
            }
        }
    }

    match crate::dependencies::resolve_pandoc() {
        Some(_) => println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorPandocAvailable)
        ),
        None => {
            println!("  {} {}", "·".dimmed(), tr(MessageId::DoctorPandocMissing));
            println!(
                "    {}",
                tr(MessageId::DoctorToolNotAdvertised).replace("{tool}", "pandoc_convert")
            );
            match std::env::consts::OS {
                "macos" => println!("      brew install pandoc"),
                "linux" => println!("      {}", tr(MessageId::DoctorPandocLinuxInstall)),
                "windows" => println!("      winget install JohnMacFarlane.Pandoc"),
                other => println!(
                    "      {}",
                    tr(MessageId::DoctorPandocOtherInstall).replace("{os}", other)
                ),
            }
        }
    }

    match dse_tools::resolve_tesseract() {
        Some(_) => {
            if cfg!(target_os = "macos") {
                println!(
                    "  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorOcrAvailable)
                );
            } else {
                println!(
                    "  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorTesseractAvailable)
                );
            }
        }
        None => {
            if cfg!(target_os = "macos") {
                println!(
                    "  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorOcrVisionFallback)
                );
                println!("    {}", tr(MessageId::DoctorTesseractOptionalMissing));
            } else {
                println!(
                    "  {} {}",
                    "·".dimmed(),
                    tr(MessageId::DoctorTesseractOptionalMissing)
                );
                println!("    {}", tr(MessageId::DoctorOcrNotAdvertised));
                match std::env::consts::OS {
                    "macos" => println!("      brew install tesseract"),
                    "linux" => {
                        println!("      {}", tr(MessageId::DoctorTesseractLinuxInstall))
                    }
                    "windows" => println!("      winget install UB-Mannheim.TesseractOCR"),
                    other => println!(
                        "      {}",
                        tr(MessageId::DoctorTesseractOtherInstall).replace("{os}", other)
                    ),
                }
            }
        }
    }

    // PDF reader: pure-Rust `pdf-extract` is the v0.8.32 default, so
    // `pdftotext` is no longer required for `read_file` to handle PDFs.
    // We still surface its presence (a) so users with column-heavy PDFs
    // know they can opt in via `prefer_external_pdftotext = true`, and
    // (b) so users who *did* opt in get a clean signal when the binary
    // is missing rather than discovering it on the next PDF read.
    let prefer_external = crate::settings::Settings::load()
        .map(|s| s.prefer_external_pdftotext)
        .unwrap_or(false);
    match crate::dependencies::resolve_pdftotext() {
        Some(_) => {
            if prefer_external {
                println!(
                    "  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorPdfExternalActive)
                );
            } else {
                println!(
                    "  {} {}",
                    "✓".truecolor(aqua_r, aqua_g, aqua_b),
                    tr(MessageId::DoctorPdftotextAvailable)
                );
                println!("    {}", tr(MessageId::DoctorPdfExternalHint));
            }
        }
        None => {
            if prefer_external {
                println!(
                    "  {} {}",
                    "✗".truecolor(red_r, red_g, red_b),
                    tr(MessageId::DoctorPdfExternalMissing)
                );
                println!("    {}", tr(MessageId::DoctorPdfExternalFallback));
                match std::env::consts::OS {
                    "macos" => println!("    {}", tr(MessageId::DoctorPopplerMacInstall)),
                    "linux" => println!("    {}", tr(MessageId::DoctorPopplerLinuxInstall)),
                    "windows" => {
                        println!("    {}", tr(MessageId::DoctorPopplerWindowsInstall))
                    }
                    _ => {}
                }
            } else {
                println!(
                    "  {} {}",
                    "·".dimmed(),
                    tr(MessageId::DoctorPdftotextMissing)
                );
                println!("    {}", tr(MessageId::DoctorPdftotextOptionalHint));
            }
        }
    }

    // Terminal-quirk overrides currently active. Mirrors the env
    // signals checked by `Settings::apply_env_overrides` so users
    // can see at a glance which a11y/compat overrides fired.
    println!();
    println!("{}", tr(MessageId::DoctorSectionTerminalQuirks).bold());
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let term_program_lc = term_program.to_ascii_lowercase();
    let mut any_quirk = false;
    if matches!(term_program.as_str(), "vscode" | "ghostty") {
        println!(
            "  {} {}",
            "•".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorTerminalLowMotion).replace("{term_program}", &term_program)
        );
        any_quirk = true;
    }
    if term_program == "Termius"
        || std::env::var_os("SSH_CLIENT").is_some_and(|v| !v.is_empty())
        || std::env::var_os("SSH_TTY").is_some_and(|v| !v.is_empty())
    {
        println!(
            "  {} {}",
            "•".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorTerminalSshLowMotion)
        );
        any_quirk = true;
    }
    if term_program_lc.contains("ptyxis")
        || std::env::var_os("PTYXIS_VERSION").is_some_and(|v| !v.is_empty())
    {
        println!(
            "  {} {}",
            "•".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorTerminalPtyxis)
        );
        any_quirk = true;
    }
    if crate::settings::detected_legacy_windows_console_host() {
        println!(
            "  {} {}",
            "•".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorTerminalLegacyWindows)
        );
        any_quirk = true;
    }
    if !any_quirk {
        println!(
            "  {} {}",
            "·".dimmed(),
            tr(MessageId::DoctorTerminalNoOverrides)
        );
    }

    // Platform and sandbox checks
    println!();
    println!("{}", tr(MessageId::DoctorSectionPlatform).bold());
    println!(
        "  {}",
        tr(MessageId::DoctorOs).replace("{os}", std::env::consts::OS)
    );
    println!(
        "  {}",
        tr(MessageId::DoctorArch).replace("{arch}", std::env::consts::ARCH)
    );

    let sandbox = dse_tools::sandbox::get_platform_sandbox();
    if let Some(kind) = sandbox {
        println!(
            "  {} {}",
            "✓".truecolor(aqua_r, aqua_g, aqua_b),
            tr(MessageId::DoctorSandboxAvailable).replace("{kind}", &kind.to_string())
        );
    } else {
        println!(
            "  {} {}",
            "!".truecolor(sky_r, sky_g, sky_b),
            tr(MessageId::DoctorSandboxUnavailable)
        );
    }

    println!();
    println!(
        "{}",
        tr(MessageId::DoctorComplete)
            .truecolor(aqua_r, aqua_g, aqua_b)
            .bold()
    );
}

// Historical sidecar version still interpreted by Doctor and the prompt
// context loader. The retired TUI setup wizard no longer owns this value.
const LEGACY_SETUP_CHECKPOINT_VERSION: &str = "0.8.67";

fn doctor_setup_state(config: &Config, workspace: &Path) -> (dse_config::SetupState, &'static str) {
    if let Ok(Some(state)) = dse_config::SetupState::load() {
        return (state, "persisted");
    }

    (
        dse_config::SetupState::derive_inherited(&doctor_inherited_setup_facts(config, workspace)),
        "derived",
    )
}

fn doctor_inherited_setup_facts(
    config: &Config,
    workspace: &Path,
) -> dse_config::InheritedConfigFacts {
    let user_constitution = dse_config::UserConstitution::load().ok();
    let user_constitution_validity = user_constitution.as_ref().map_or(
        dse_config::ConstitutionValidity::Unknown,
        dse_config::UserConstitutionLoad::validity,
    );
    let has_user_constitution = user_constitution
        .as_ref()
        .is_some_and(|loaded| !matches!(loaded, dse_config::UserConstitutionLoad::Missing));
    let has_expert_override = dse_config::dse_home()
        .ok()
        .map(|home| home.join(Path::new(crate::prompts::CONSTITUTION_OVERRIDE_FILE)))
        .is_some_and(|path| path.exists());

    dse_config::InheritedConfigFacts {
        has_provider_route: !config.default_model().trim().is_empty(),
        has_credentials_or_local_runtime: doctor_has_credentials_or_local_runtime(config),
        trust_chosen: !crate::tui::onboarding::needs_trust(workspace),
        has_expert_override,
        has_user_constitution,
        user_constitution_validity,
    }
}

fn doctor_has_credentials_or_local_runtime(config: &Config) -> bool {
    resolve_api_key_source(config) != ApiKeySource::Missing
}

fn print_doctor_setup_report(
    config: &Config,
    workspace: &Path,
    state: &dse_config::SetupState,
    source: &str,
    ok_rgb: (u8, u8, u8),
    warn_rgb: (u8, u8, u8),
) {
    use colored::Colorize;

    let first_run_ready = state.first_run_ready();
    let update_ready = state.update_ready(LEGACY_SETUP_CHECKPOINT_VERSION);
    let first_run_icon = if first_run_ready {
        "✓".truecolor(ok_rgb.0, ok_rgb.1, ok_rgb.2)
    } else {
        "!".truecolor(warn_rgb.0, warn_rgb.1, warn_rgb.2)
    };
    let update_icon = if update_ready {
        "✓".truecolor(ok_rgb.0, ok_rgb.1, ok_rgb.2)
    } else {
        "!".truecolor(warn_rgb.0, warn_rgb.1, warn_rgb.2)
    };

    println!();
    println!("{}", tr(MessageId::DoctorSectionSetupState).bold());
    let source_label = match source {
        "persisted" => tr(MessageId::DoctorSourcePersisted),
        "derived" => tr(MessageId::DoctorSourceDerived),
        _ => Cow::Borrowed(source),
    };
    println!(
        "  · {}",
        tr(MessageId::DoctorSetupSource).replace("{source}", source_label.as_ref())
    );
    println!(
        "  {first_run_icon} {}",
        tr(MessageId::DoctorFirstRun)
            .replace("{status}", doctor_ready_label(first_run_ready).as_ref())
    );
    println!(
        "  {update_icon} {}",
        tr(MessageId::DoctorUpdateCheckpoint)
            .replace("{version}", LEGACY_SETUP_CHECKPOINT_VERSION)
            .replace("{status}", doctor_ready_label(update_ready).as_ref())
    );
    println!(
        "  · {}",
        tr(MessageId::DoctorConstitutionAutonomy).replace(
            "{value}",
            localized_autonomy_preference(doctor_constitution_autonomy_preference()).as_ref()
        )
    );
    println!(
        "  · {}",
        tr(MessageId::DoctorRuntimePosture)
            .replace("{value}", &doctor_runtime_posture_line(config, workspace))
    );
    let consistency = doctor_setup_consistency(state, source);
    if consistency["status"] == "inconsistent" {
        let issues = consistency["issues"]
            .as_array()
            .map(|issues| {
                issues
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        println!(
            "  {} {}",
            "!".truecolor(warn_rgb.0, warn_rgb.1, warn_rgb.2),
            tr(MessageId::DoctorSetupInconsistent)
                .replace("{issues}", &issues)
                .replace(
                    "{repair}",
                    consistency["repair"].as_str().unwrap_or("dse doctor")
                ),
        );
    }
    println!("  · {}", tr(MessageId::DoctorNextActions));
    for step in dse_config::SetupStep::ALL {
        let entry = state.steps.get(&step);
        let required = entry.is_some_and(|entry| entry.required);
        let version = entry.and_then(|entry| entry.version.as_deref());
        let result = entry.and_then(|entry| entry.result.as_deref());
        let required_label = if required {
            tr(MessageId::DoctorRequired)
        } else {
            tr(MessageId::DoctorOptional)
        };
        let version_label = version
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| tr(MessageId::DoctorUnversioned).into_owned());
        let result_label = result
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| tr(MessageId::DoctorNoResult).into_owned());
        println!(
            "    · {}: {} ({required_label}, {version_label}, {result_label})",
            setup_step_id(step),
            setup_status_id(state.status(step))
        );
    }
}

fn doctor_ready_label(ready: bool) -> Cow<'static, str> {
    if ready {
        tr(MessageId::DoctorReady)
    } else {
        tr(MessageId::DoctorNeedsAction)
    }
}

/// Detect half-applied setup persistence (#3410).
///
/// The setup transaction writes `constitution.json` and `setup_state.json`
/// together, so a persisted state that points at a user-global constitution
/// which is missing or unusable on disk means a write was interrupted or a
/// file was removed out-of-band. Stale `.tmp*` files in `$DSE_HOME`
/// are the other fingerprint of an interrupted atomic write.
fn doctor_setup_consistency(state: &dse_config::SetupState, source: &str) -> serde_json::Value {
    use serde_json::json;

    let mut issues: Vec<&'static str> = Vec::new();

    if source == "persisted"
        && matches!(
            state.constitution_source,
            dse_config::ConstitutionSource::UserGlobal
        )
    {
        match dse_config::UserConstitution::load() {
            Ok(dse_config::UserConstitutionLoad::Missing) => {
                issues.push("setup_state_points_at_missing_user_constitution");
            }
            Ok(dse_config::UserConstitutionLoad::Empty) => {
                issues.push("user_constitution_empty");
            }
            Ok(dse_config::UserConstitutionLoad::Invalid(_)) => {
                issues.push("user_constitution_invalid");
            }
            Ok(dse_config::UserConstitutionLoad::Unreadable(_)) | Err(_) => {
                issues.push("user_constitution_unreadable");
            }
            Ok(dse_config::UserConstitutionLoad::Loaded(_)) => {}
        }
    }

    if doctor_home_has_stale_setup_temp_files() {
        issues.push("stale_setup_temp_files_in_dse_home");
    }

    json!({
        "status": if issues.is_empty() { "consistent" } else { "inconsistent" },
        "issues": issues,
        "repair": "inspect ~/.dse configuration and rerun dse doctor",
    })
}

fn doctor_home_has_stale_setup_temp_files() -> bool {
    let Ok(home) = dse_config::dse_home() else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(&home) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_name().to_string_lossy().starts_with(".tmp")
            && entry.file_type().is_ok_and(|kind| kind.is_file())
    })
}

fn doctor_constitution_autonomy_preference() -> dse_config::AutonomyPreference {
    dse_config::UserConstitution::load()
        .ok()
        .and_then(|load| {
            load.constitution()
                .map(|constitution| constitution.autonomy_preference)
        })
        .unwrap_or(dse_config::AutonomyPreference::Unspecified)
}

fn doctor_constitution_autonomy_preference_id() -> &'static str {
    autonomy_preference_id(doctor_constitution_autonomy_preference())
}

fn autonomy_preference_id(preference: dse_config::AutonomyPreference) -> &'static str {
    match preference {
        dse_config::AutonomyPreference::Unspecified => "unspecified",
        dse_config::AutonomyPreference::Cautious => "cautious",
        dse_config::AutonomyPreference::Balanced => "balanced",
        dse_config::AutonomyPreference::Autonomous => "autonomous",
    }
}

fn localized_autonomy_preference(preference: dse_config::AutonomyPreference) -> Cow<'static, str> {
    match preference {
        dse_config::AutonomyPreference::Unspecified => tr(MessageId::DoctorAutonomyUnspecified),
        dse_config::AutonomyPreference::Cautious => tr(MessageId::DoctorAutonomyCautious),
        dse_config::AutonomyPreference::Balanced => tr(MessageId::DoctorAutonomyBalanced),
        dse_config::AutonomyPreference::Autonomous => tr(MessageId::DoctorAutonomyAutonomous),
    }
}

fn doctor_runtime_posture_line(config: &Config, workspace: &Path) -> String {
    let allow_shell = config.interactive_allow_shell();
    let allow_shell_source = if config.allow_shell.is_some() {
        tr(MessageId::DoctorSourceConfig)
    } else {
        tr(MessageId::DoctorSourceInteractiveDefault)
    };
    let trust = if crate::tui::onboarding::needs_trust(workspace) {
        tr(MessageId::DoctorWorkspaceNotTrusted)
    } else {
        tr(MessageId::DoctorWorkspaceTrusted)
    };

    format!(
        "permission=ask ({}), allow_shell={allow_shell} ({allow_shell_source}), workspace_trust={trust}",
        tr(MessageId::DoctorSourceDefault)
    )
}

fn doctor_task_graph_report_json(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let has_credentials_or_local = crate::config::has_api_key(config);
    let subagents_enabled = config.subagents_enabled();
    let disabled_reason = if subagents_enabled {
        None
    } else {
        Some(config.subagents_disabled_reason().unwrap_or("disabled"))
    };
    let max_subagents = config.max_subagents();

    json!({
        "ready": has_credentials_or_local && subagents_enabled,
        "owner": {
            "runtime": "AgentRuntime",
            "persistent_truth": "RunStore",
            "writer_workspace_side_effects": "ProductionAgentOrchestrator",
        },
        "provider": {
            "id": crate::config::DEEPSEEK_PROVIDER_ID,
            "auth": {
                "present_or_local": has_credentials_or_local,
                "source": doctor_api_key_source_label(resolve_api_key_source(config)),
            },
        },
        "actors": {
            "root": true,
            "read_only_child": subagents_enabled,
            "explicit_writer": subagents_enabled,
            "enabled": subagents_enabled,
            "disabled_reason": disabled_reason,
            "max_subagents": max_subagents,
        },
        "concurrency": {
            "max_subagents": max_subagents,
            "plan_limit_probed": false,
        },
        "remote_fleet": false,
        "multi_writer": false,
    })
}

fn doctor_provider_model_report_json(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let auth_source = resolve_api_key_source(config);
    let auth_present_or_local = crate::config::has_api_key(config);

    json!({
        "provider": {
            "id": crate::config::DEEPSEEK_PROVIDER_ID,
            "display": crate::config::DEEPSEEK_DISPLAY_NAME,
        },
        "model": {
            "resolved": config.default_model(),
        },
        "auth": {
            "present_or_local": auth_present_or_local,
            "source": doctor_api_key_source_label(auth_source),
            "env_vars": [crate::config::DEEPSEEK_API_KEY_ENV],
            "credential_url": crate::config::DEEPSEEK_CREDENTIAL_URL,
            "oauth_only": false,
        },
        "health": {
            "live_validation": false,
            "next_action": if auth_present_or_local {
                "dse auth status"
            } else {
                "dse auth set"
            },
        },
    })
}

fn doctor_setup_report_json(config: &Config, workspace: &Path) -> serde_json::Value {
    use serde_json::json;

    let (state, source) = doctor_setup_state(config, workspace);
    let allow_shell = config.interactive_allow_shell();
    let allow_shell_source = if config.allow_shell.is_some() {
        "config"
    } else {
        "interactive_default"
    };
    let workspace_trusted = !crate::tui::onboarding::needs_trust(workspace);
    let steps: Vec<_> = dse_config::SetupStep::ALL
        .into_iter()
        .map(|step| {
            let entry = state.steps.get(&step);
            json!({
                "step": setup_step_id(step),
                "status": setup_status_id(state.status(step)),
                "required": entry.is_some_and(|entry| entry.required),
                "version": entry.and_then(|entry| entry.version.clone()),
                "result": entry.and_then(|entry| entry.result.clone()),
            })
        })
        .collect();

    json!({
        "source": source,
        "schema_version": state.schema_version,
        "inherited": state.inherited,
        "checkpoint_version": LEGACY_SETUP_CHECKPOINT_VERSION,
        "first_run_ready": state.first_run_ready(),
        "update_ready": state.update_ready(LEGACY_SETUP_CHECKPOINT_VERSION),
        "constitution": {
            "choice": constitution_choice_id(state.constitution_choice),
            "source": constitution_source_id(state.constitution_source),
            "validity": constitution_validity_id(state.constitution_validity),
            "checkpoint_completed_for": state.constitution_checkpoint_completed_for.clone(),
            "preview_hash_present": state.constitution_preview_hash.is_some(),
            "preview_version": state.constitution_preview_version,
            "autonomy_preference": doctor_constitution_autonomy_preference_id(),
        },
        "runtime_posture_source": runtime_posture_source_id(state.runtime_posture_source),
        "runtime_posture": {
            "source": runtime_posture_source_id(state.runtime_posture_source),
            "permission": {"value": "ask", "source": "default"},
            "allow_shell": {
                "value": allow_shell,
                "source": allow_shell_source,
            },
            "workspace_trust": {
                "trusted": workspace_trusted,
                "source": "workspace",
            },
        },
        "provider_model": doctor_provider_model_report_json(config),
        "task_graph": doctor_task_graph_report_json(config),
        "consistency": doctor_setup_consistency(&state, source),
        "next_actions": {
            "authentication": "dse auth status/set",
            "configuration": "~/.dse/config.toml",
            "runs": "dse runs; dse resume <run-id>",
        },
        "steps": steps,
    })
}

fn setup_step_id(step: dse_config::SetupStep) -> &'static str {
    match step {
        dse_config::SetupStep::ProviderModel => "provider_model",
        dse_config::SetupStep::TrustSandbox => "trust_sandbox",
        dse_config::SetupStep::ToolsMcp => "tools_mcp",
        dse_config::SetupStep::Persistence => "persistence",
        dse_config::SetupStep::Constitution => "constitution",
        dse_config::SetupStep::Verification => "verification",
    }
}

fn setup_status_id(status: dse_config::StepStatus) -> &'static str {
    match status {
        dse_config::StepStatus::NotStarted => "not_started",
        dse_config::StepStatus::Recommended => "recommended",
        dse_config::StepStatus::Optional => "optional",
        dse_config::StepStatus::Deferred => "deferred",
        dse_config::StepStatus::InProgress => "in_progress",
        dse_config::StepStatus::Verified => "verified",
        dse_config::StepStatus::NeedsAction => "needs_action",
        dse_config::StepStatus::Failed => "failed",
        dse_config::StepStatus::Skipped => "skipped",
    }
}

fn constitution_choice_id(choice: dse_config::ConstitutionChoice) -> &'static str {
    match choice {
        dse_config::ConstitutionChoice::Unset => "unset",
        dse_config::ConstitutionChoice::Bundled => "bundled",
        dse_config::ConstitutionChoice::GuidedCustom => "guided_custom",
        dse_config::ConstitutionChoice::ExpertOverride => "expert_override",
        dse_config::ConstitutionChoice::Deferred => "deferred",
    }
}

fn constitution_source_id(source: dse_config::ConstitutionSource) -> &'static str {
    match source {
        dse_config::ConstitutionSource::Bundled => "bundled",
        dse_config::ConstitutionSource::UserGlobal => "user_global",
        dse_config::ConstitutionSource::ExpertOverride => "expert_override",
    }
}

fn constitution_validity_id(validity: dse_config::ConstitutionValidity) -> &'static str {
    match validity {
        dse_config::ConstitutionValidity::Unknown => "unknown",
        dse_config::ConstitutionValidity::Valid => "valid",
        dse_config::ConstitutionValidity::Invalid => "invalid",
        dse_config::ConstitutionValidity::Empty => "empty",
        dse_config::ConstitutionValidity::Unreadable => "unreadable",
    }
}

fn runtime_posture_source_id(source: dse_config::RuntimePostureSource) -> &'static str {
    match source {
        dse_config::RuntimePostureSource::Unset => "unset",
        dse_config::RuntimePostureSource::Inherited => "inherited",
        dse_config::RuntimePostureSource::Confirmed => "confirmed",
    }
}

/// Machine-readable counterpart to `run_doctor`. Skips the live API call so it
/// is safe to run in CI and from non-interactive scripts.
fn run_doctor_json(
    config: &Config,
    workspace: &Path,
    config_path_override: Option<&Path>,
) -> Result<()> {
    use serde_json::json;

    let config_path = config_path_override
        .map(PathBuf::from)
        .or_else(|| dse_config::resolve_config_path(None).ok())
        .unwrap_or_else(|| {
            dse_config::dse_home()
                .unwrap_or_else(|_| PathBuf::from(".dse"))
                .join("config.toml")
        });

    let api_key_state = match resolve_api_key_source(config) {
        ApiKeySource::Env => "env",
        ApiKeySource::Config => "config",
        ApiKeySource::Keyring => "keyring",
        ApiKeySource::Missing => "missing",
    };

    let mcp_config_path = config.mcp_config_path();
    let project_mcp_config_path = crate::mcp::workspace_mcp_config_path(workspace);
    let mcp_present = mcp_config_path.exists();
    let project_mcp_present = project_mcp_config_path.exists();
    let mcp_summary = match crate::mcp::load_config_with_workspace(&mcp_config_path, workspace) {
        Ok(cfg) => {
            let servers: Vec<serde_json::Value> = cfg
                .servers
                .iter()
                .map(|(name, server)| {
                    let status = doctor_check_mcp_server(server);
                    let (kind, detail) = match &status {
                        McpServerDoctorStatus::Ok(d) => ("ok", d.clone()),
                        McpServerDoctorStatus::Warning(d) => ("warning", d.clone()),
                        McpServerDoctorStatus::Error(d) => ("error", d.clone()),
                    };
                    json!({
                        "name": name,
                        "enabled": server.enabled && !server.disabled,
                        "status": kind,
                        "detail": detail,
                    })
                })
                .collect();
            json!({
                "config_path": mcp_config_path.display().to_string(),
                "present": mcp_present,
                "project_config_path": project_mcp_config_path.display().to_string(),
                "project_present": project_mcp_present,
                "servers": servers,
            })
        }
        Err(err) => json!({
            "config_path": mcp_config_path.display().to_string(),
            "present": mcp_present,
            "project_config_path": project_mcp_config_path.display().to_string(),
            "project_present": project_mcp_present,
            "servers": [],
            "error": err.to_string(),
        }),
    };

    let global_skills_dir = config.skills_dir();
    let agents_skills_dir = workspace.join(".agents").join("skills");
    let local_skills_dir = workspace.join("skills");
    let agents_global_skills_dir = crate::skill_context::agents_global_skills_dir();
    // #432: cross-tool skill discovery dirs surface in the JSON
    // report so external dashboards can see whether any
    // `.opencode/skills/`, `.claude/skills/`, `.cursor/skills/`, or
    // global agentskills.io content is contributing to the merged catalogue.
    let opencode_skills_dir = workspace.join(".opencode").join("skills");
    let claude_skills_dir = workspace.join(".claude").join("skills");
    let selected_skills_dir = if agents_skills_dir.exists() {
        agents_skills_dir.clone()
    } else if local_skills_dir.exists() {
        local_skills_dir.clone()
    } else if config.skills_dir.is_none()
        && let Some(global_agents) = agents_global_skills_dir.as_ref()
        && global_agents.exists()
    {
        global_agents.clone()
    } else {
        global_skills_dir.clone()
    };
    let agents_global_summary = agents_global_skills_dir
        .as_ref()
        .map(|path| {
            json!({
                "path": path.display().to_string(),
                "present": path.exists(),
                "count": skills_count_for(path),
            })
        })
        .unwrap_or_else(|| {
            json!({
                "path": null,
                "present": false,
                "count": 0,
            })
        });

    let plugins_dir = default_plugins_dir();

    let api_target = doctor_api_target(config);
    let tls_status = doctor_tls_status(config);
    let code_home = dse_config::dse_home().unwrap_or_else(|_| PathBuf::from("~/.dse"));

    let report = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "config_path": config_path.display().to_string(),
        "config_present": config_path.exists(),
        "workspace": workspace.display().to_string(),
        "state_root": code_home.display().to_string(),
        "setup": doctor_setup_report_json(config, workspace),
        "api_key": {
            "source": api_key_state,
        },
        "base_url": crate::utils::redact_url_for_display(&api_target.base_url),
        "default_text_model": api_target.model,
        "route": doctor_route_report(config),
        "tls": {
            "certificate_verification": tls_status.certificate_verification,
            "insecure_skip_tls_verify": tls_status.insecure_skip_tls_verify,
            "provider": tls_status.provider,
            "message": tls_status.message,
        },
        "search_provider": doctor_search_provider_json(config),
        "mcp": mcp_summary,
        "skills": {
            "selected": selected_skills_dir.display().to_string(),
            "global": {
                "path": global_skills_dir.display().to_string(),
                "present": global_skills_dir.exists(),
                "count": skills_count_for(&global_skills_dir),
            },
            "agents": {
                "path": agents_skills_dir.display().to_string(),
                "present": agents_skills_dir.exists(),
                "count": skills_count_for(&agents_skills_dir),
            },
            "agents_global": agents_global_summary,
            "local": {
                "path": local_skills_dir.display().to_string(),
                "present": local_skills_dir.exists(),
                "count": skills_count_for(&local_skills_dir),
            },
            "opencode": {
                "path": opencode_skills_dir.display().to_string(),
                "present": opencode_skills_dir.exists(),
                "count": skills_count_for(&opencode_skills_dir),
            },
            "claude": {
                "path": claude_skills_dir.display().to_string(),
                "present": claude_skills_dir.exists(),
                "count": skills_count_for(&claude_skills_dir),
            },
        },
        "plugins": {
            "path": plugins_dir.display().to_string(),
            "present": plugins_dir.exists(),
            "count": if plugins_dir.exists() { count_dir_entries(&plugins_dir) } else { 0 },
        },
        "sandbox": match dse_tools::sandbox::get_platform_sandbox() {
            Some(kind) => json!({"available": true, "kind": kind.to_string()}),
            None => json!({"available": false, "kind": null}),
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        },
        "api_connectivity": {
            "checked": false,
            "note": "Skipped in --json mode; run `dse doctor` for a non-inference official host and credential reachability check.",
        },
        "capability": deepseek_capability_report(config),
    });

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Build the `capability` section for the machine-readable doctor report.
///
/// Returns a JSON value with the fixed DeepSeek identity, resolved model, context
/// window, max output, thinking support, cache telemetry support, and request
/// payload mode.
fn deepseek_capability_report(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let model = config.default_model();

    let cap = crate::config::deepseek_capability(&model);

    json!({
        "resolved_provider": crate::config::DEEPSEEK_PROVIDER_ID,
        "resolved_model": cap.resolved_model,
        "context_window": cap.context_window,
        "max_output": cap.max_output,
        "thinking_supported": cap.thinking_supported,
        "cache_telemetry_supported": cap.cache_telemetry_supported,
        "request_payload_mode": serde_json::to_value(cap.request_payload_mode).unwrap_or_default(),
    })
}

fn doctor_route_report(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let target = doctor_api_target(config);
    let redacted_base_url = crate::utils::redact_url_for_display(&target.base_url);

    json!({
        "provider": target.provider,
        "provider_source": "fixed_deepseek",
        "provider_config_table": "root",
        "model": target.model,
        "wire_protocol": doctor_wire_protocol(),
        "base_url": {
            "redacted": redacted_base_url,
            "class": doctor_base_url_class(&target.base_url),
            "fingerprint": crate::utils::redacted_identifier_for_log(&target.base_url),
        },
        "auth": {
            "scheme": doctor_auth_scheme(config),
            "source": doctor_api_key_source_label(resolve_api_key_source(config)),
        },
    })
}

fn doctor_wire_protocol() -> &'static str {
    "chat_completions"
}

fn doctor_base_url_class(base_url: &str) -> &'static str {
    let normalized = base_url.trim_end_matches('/').to_ascii_lowercase();
    if normalized.starts_with("http://localhost")
        || normalized.starts_with("http://127.0.0.1")
        || normalized.starts_with("http://[::1]")
    {
        return "local";
    }
    if normalized
        == crate::config::DEFAULT_DEEPSEEK_BASE_URL
            .trim_end_matches('/')
            .to_ascii_lowercase()
    {
        "default"
    } else {
        "custom"
    }
}

fn doctor_auth_scheme(_config: &Config) -> &'static str {
    "bearer"
}

fn doctor_api_key_source_label(source: ApiKeySource) -> &'static str {
    match source {
        ApiKeySource::Env => "env",
        ApiKeySource::Config => "config",
        ApiKeySource::Keyring => "keyring",
        ApiKeySource::Missing => "missing",
    }
}

fn doctor_search_provider_line(config: &Config) -> String {
    let search_provider = config.search_provider_resolution();
    let switch_hint = if matches!(
        (search_provider.provider, search_provider.source),
        (
            crate::config::SearchProvider::DuckDuckGo,
            crate::config::SearchProviderSource::Default
        )
    ) {
        tr(MessageId::DoctorSearchProviderSwitchHint)
    } else {
        Cow::Borrowed("")
    };

    tr(MessageId::DoctorSearchProvider)
        .replace("{provider}", search_provider.provider.as_str())
        .replace("{source}", search_provider.source.as_str())
        .replace("{hint}", switch_hint.as_ref())
}

fn doctor_search_provider_json(config: &Config) -> serde_json::Value {
    use serde_json::json;

    let search_provider = config.search_provider_resolution();
    json!({
        "provider": search_provider.provider.as_str(),
        "source": search_provider.source.as_str(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorApiTarget {
    provider: &'static str,
    base_url: String,
    model: String,
}

fn doctor_api_target(config: &Config) -> DoctorApiTarget {
    DoctorApiTarget {
        provider: crate::config::DEEPSEEK_PROVIDER_ID,
        base_url: config.deepseek_base_url(),
        model: config.default_model(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorTlsStatus {
    certificate_verification: bool,
    insecure_skip_tls_verify: bool,
    provider: &'static str,
    message: String,
}

fn doctor_tls_status(config: &Config) -> DoctorTlsStatus {
    let provider = crate::config::DEEPSEEK_PROVIDER_ID;
    let insecure_skip_tls_verify = config.insecure_skip_tls_verify();
    DoctorTlsStatus {
        certificate_verification: true,
        insecure_skip_tls_verify,
        provider,
        message: if insecure_skip_tls_verify {
            format!(
                "TLS certificate verification cannot be disabled for provider {provider}; use SSL_CERT_FILE with a trusted custom CA bundle"
            )
        } else {
            "TLS certificate verification enabled".to_string()
        },
    }
}

fn doctor_timeout_recovery_lines(config: &Config) -> Vec<String> {
    let target = doctor_api_target(config);
    let mut lines = vec![tr(MessageId::DoctorTimeout).replace("{base_url}", &target.base_url)];

    if target.base_url.contains("api.deepseek.com") {
        lines.push(tr(MessageId::DoctorTimeoutOfficialHint).into_owned());
    } else {
        lines.push(tr(MessageId::DoctorTimeoutFixtureHint).into_owned());
    }

    lines.push(tr(MessageId::DoctorTimeoutReportHint).into_owned());
    lines
}

fn run_execpolicy_command(command: ExecpolicyCommand) -> Result<()> {
    match command.command {
        ExecpolicySubcommand::Check(cmd) => cmd.run(),
    }
}

fn run_features_command(config: &Config, command: FeaturesCli) -> Result<()> {
    match command.command {
        FeaturesSubcommand::List => {
            print!("{}", render_feature_table(&config.features()));
            Ok(())
        }
    }
}

/// Test official API host/auth reachability without making a model request.
async fn test_api_connectivity(config: &Config) -> Result<()> {
    use dse_deepseek::DeepSeekCredential;

    let connection = crate::exec_runtime::deepseek_connection_config(config)?;
    crate::tls::ensure_rustls_crypto_provider();

    // Keep a bounded whole-probe deadline in addition to the canonical
    // response-header timeout so a malformed peer cannot hold Doctor open
    // while sending the small account response body.
    let timeout_duration = std::time::Duration::from_secs(15);
    match tokio::time::timeout(
        timeout_duration,
        connection.probe_account(
            reqwest::Client::builder().build()?,
            DeepSeekCredential::new(config.deepseek_api_key()?)?,
        ),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => anyhow::bail!("{}", tr(MessageId::MainDoctorProbeTimeout)),
    }
}

fn rustc_version() -> String {
    let Some(mut cmd) = crate::dependencies::RustC::command() else {
        return "unknown".to_string();
    };
    let Ok(output) = cmd.arg("--version").output() else {
        return "unknown".to_string();
    };
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Initialize a new project with AGENTS.md
fn init_project() -> Result<()> {
    use crate::palette;
    use colored::Colorize;
    use project_context::create_default_agents_md;

    let (sky_r, sky_g, sky_b) = palette::DSE_INFO_RGB;
    let (aqua_r, aqua_g, aqua_b) = palette::DSE_INFO_RGB;
    let (red_r, red_g, red_b) = palette::DSE_ERROR_RGB;

    let workspace = std::env::current_dir()?;
    let agents_path = workspace.join("AGENTS.md");

    if agents_path.exists() {
        println!(
            "{}",
            tr(MessageId::MainAgentsExists)
                .replace("{icon}", &"!".truecolor(sky_r, sky_g, sky_b).to_string())
                .replace("{path}", &agents_path.display().to_string())
        );
        return Ok(());
    }

    match create_default_agents_md(&workspace) {
        Ok(path) => {
            println!(
                "{}",
                tr(MessageId::MainAgentsCreated)
                    .replace("{icon}", &"✓".truecolor(aqua_r, aqua_g, aqua_b).to_string())
                    .replace("{path}", &path.display().to_string())
            );
            println!();
            println!("{}", tr(MessageId::MainAgentsEdit));
            println!("{}", tr(MessageId::MainAgentsLoaded));
        }
        Err(e) => {
            println!(
                "{}",
                tr(MessageId::MainAgentsCreateFailed)
                    .replace("{icon}", &"✗".truecolor(red_r, red_g, red_b).to_string())
                    .replace("{error}", &e.to_string())
            );
        }
    }

    Ok(())
}

fn resolve_workspace(cli: &Cli) -> PathBuf {
    cli.workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn load_config_from_cli(cli: &Cli) -> Result<Config> {
    let profile = cli
        .profile
        .clone()
        .or_else(|| std::env::var("DSE_PROFILE").ok());
    let mut config = Config::load(cli.config.clone(), profile.as_deref())?;
    cli.feature_toggles.apply(&mut config)?;
    Ok(config)
}

fn read_api_key_from_stdin() -> Result<String> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        bail!("{}", tr(MessageId::MainApiKeyInputRequired));
    }
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;
    let api_key = buffer.trim().to_string();
    if api_key.is_empty() {
        bail!("{}", tr(MessageId::MainApiKeyStdinRequired));
    }
    Ok(api_key)
}

fn run_login(api_key: Option<String>) -> Result<()> {
    let api_key = match api_key {
        Some(key) => key,
        None => read_api_key_from_stdin()?,
    };
    let saved = config::save_api_key(&api_key)?;
    println!(
        "{}",
        tr(MessageId::MainApiKeySaved).replace("{destination}", &saved.describe())
    );
    Ok(())
}

fn run_logout() -> Result<()> {
    config::clear_api_key()?;
    println!("{}", tr(MessageId::MainApiKeyCleared));
    Ok(())
}

/// `dse pr <N>` (#451) — fetch a GitHub PR via `gh`, format
/// title + body + diff as the composer's first message, and launch
/// the interactive TUI. Falls back gracefully if `gh` is missing.
async fn run_pr(
    cli: &Cli,
    config: &Config,
    number: u32,
    repo: Option<&str>,
    checkout: bool,
) -> Result<()> {
    if !is_command_available("gh") {
        bail!("{}", tr(MessageId::MainGhMissing));
    }

    let view = run_gh_pr_view(number, repo)?;
    let diff = run_gh_pr_diff(number, repo)?;

    if checkout {
        match run_gh_pr_checkout(number, repo) {
            Ok(()) => eprintln!(
                "{}",
                tr(MessageId::MainPrCheckedOut).replace("{number}", &number.to_string())
            ),
            Err(err) => eprintln!(
                "{}",
                tr(MessageId::MainPrCheckoutFailed)
                    .replace("{number}", &number.to_string())
                    .replace("{error}", &err.to_string())
            ),
        }
    }

    let prompt = format_pr_prompt(number, &view, &diff);
    let resume_session_id = if cli.continue_session {
        Some("latest".to_owned())
    } else {
        cli.resume.clone()
    };
    run_interactive(
        cli,
        config,
        resume_session_id,
        Some(tui::InitialInput::Prefill(prompt)),
    )
    .await
}

/// Return true if `name` resolves to an executable on the current `PATH`.
///
/// Walks `$PATH` directly instead of probing with `--version`. The
/// previous implementation invoked `Command::new(name).arg("--version")`,
/// which fails on the Ubuntu CI runner because `/bin/sh` is `dash` —
/// `dash --version` exits with status 2 ("invalid option") even though
/// `sh` is plainly on PATH. macOS happens to ship bash as `sh`, which
/// does honor `--version`, so the bug was invisible locally and only
/// surfaced in CI logs.
///
/// Windows: also checks the `.exe` extension when `name` doesn't have
/// one, matching the platform's PATHEXT lookup behavior for the common
/// case.
fn is_command_available(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            // PATHEXT gives `.exe`/`.cmd`/`.bat` etc. priority — we only
            // probe `.exe` because that's the case that actually trips
            // up the negative case (`gh` resolves as `gh.exe`).
            if candidate.extension().is_none() && candidate.with_extension("exe").is_file() {
                return true;
            }
        }
    }
    false
}

#[derive(Debug, Clone, Default)]
struct GhPullRequest {
    title: String,
    body: String,
    base: String,
    head: String,
    url: String,
}

fn run_gh_pr_view(number: u32, repo: Option<&str>) -> Result<GhPullRequest> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("{}", tr(MessageId::MainGhMissing)))?;
    cmd.arg("pr").arg("view").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    cmd.arg("--json")
        .arg("title,body,baseRefName,headRefName,url");
    let output = cmd.output().map_err(|error| {
        anyhow::anyhow!(
            "{}",
            tr(MessageId::MainGhRunFailed)
                .replace("{command}", "gh pr view")
                .replace("{error}", &error.to_string())
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{}",
            tr(MessageId::MainGhCommandFailed)
                .replace("{command}", "gh pr view")
                .replace("{number}", &number.to_string())
                .replace("{stderr}", &stderr)
        );
    }
    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        anyhow::anyhow!(
            "{}",
            tr(MessageId::MainGhJsonFailed).replace("{error}", &error.to_string())
        )
    })?;
    let pick = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Ok(GhPullRequest {
        title: pick("title"),
        body: pick("body"),
        base: pick("baseRefName"),
        head: pick("headRefName"),
        url: pick("url"),
    })
}

fn run_gh_pr_diff(number: u32, repo: Option<&str>) -> Result<String> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("{}", tr(MessageId::MainGhMissing)))?;
    cmd.arg("pr").arg("diff").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    let output = cmd.output().map_err(|error| {
        anyhow::anyhow!(
            "{}",
            tr(MessageId::MainGhRunFailed)
                .replace("{command}", "gh pr diff")
                .replace("{error}", &error.to_string())
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{}",
            tr(MessageId::MainGhCommandFailed)
                .replace("{command}", "gh pr diff")
                .replace("{number}", &number.to_string())
                .replace("{stderr}", &stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_gh_pr_checkout(number: u32, repo: Option<&str>) -> Result<()> {
    let mut cmd = crate::dependencies::Gh::command()
        .ok_or_else(|| anyhow::anyhow!("{}", tr(MessageId::MainGhMissing)))?;
    cmd.arg("pr").arg("checkout").arg(number.to_string());
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    let output = cmd.output().map_err(|error| {
        anyhow::anyhow!(
            "{}",
            tr(MessageId::MainGhRunFailed)
                .replace("{command}", "gh pr checkout")
                .replace("{error}", &error.to_string())
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{}",
            tr(MessageId::MainGhCommandFailed)
                .replace("{command}", "gh pr checkout")
                .replace("{number}", &number.to_string())
                .replace("{stderr}", &stderr)
        );
    }
    Ok(())
}

/// Format the PR review prompt that lands in the composer. Caps the
/// diff at 200 KiB so a massive PR doesn't blow the model's context
/// window before the user even hits Enter — they can always ask the
/// model to fetch more via `gh pr diff #N` from inside the session.
fn format_pr_prompt(number: u32, view: &GhPullRequest, diff: &str) -> String {
    const MAX_DIFF_BYTES: usize = 200 * 1024;
    let diff_section = if diff.len() > MAX_DIFF_BYTES {
        let cut = (0..=MAX_DIFF_BYTES)
            .rev()
            .find(|&i| diff.is_char_boundary(i))
            .unwrap_or(0);
        format!(
            "{}\n\n[…diff truncated at {} KiB; ask me to fetch more if needed]\n",
            &diff[..cut],
            MAX_DIFF_BYTES / 1024
        )
    } else {
        diff.to_string()
    };
    let body = if view.body.trim().is_empty() {
        "(no description)".to_string()
    } else {
        view.body.trim().to_string()
    };
    let title = if view.title.trim().is_empty() {
        format!("(PR #{number})")
    } else {
        view.title.trim().to_string()
    };
    let branches = match (view.base.is_empty(), view.head.is_empty()) {
        (false, false) => format!("{} ← {}", view.base, view.head),
        (false, true) => view.base.clone(),
        (true, false) => view.head.clone(),
        _ => "(unknown)".to_string(),
    };
    format!(
        "Review PR #{number} — {title}\n\
         \n\
         URL: {url}\n\
         Branches: {branches}\n\
         \n\
         ## Description\n\
         \n\
         {body}\n\
         \n\
         ## Diff\n\
         \n\
         ```diff\n\
         {diff_section}\n\
         ```\n",
        url = if view.url.is_empty() {
            "(unavailable)"
        } else {
            view.url.as_str()
        },
    )
}

async fn run_mcp_command(config: &Config, workspace: &Path, command: McpCommand) -> Result<()> {
    let config_path = config.mcp_config_path();
    match command {
        McpCommand::Init { force } => {
            let status = crate::mcp::init_config(&config_path, force)?;
            match status {
                McpWriteStatus::Created => {
                    println!(
                        "{}",
                        tr(MessageId::MainCreated)
                            .replace("{label}", tr(MessageId::MainMcpConfigLabel).as_ref())
                            .replace("{path}", &config_path.display().to_string())
                    );
                }
                McpWriteStatus::Overwritten => {
                    println!(
                        "{}",
                        tr(MessageId::MainOverwritten)
                            .replace("{label}", tr(MessageId::MainMcpConfigLabel).as_ref())
                            .replace("{path}", &config_path.display().to_string())
                    );
                }
                McpWriteStatus::SkippedExists => {
                    println!(
                        "{}",
                        tr(MessageId::MainMcpConfigExistsForce)
                            .replace("{path}", &config_path.display().to_string())
                    );
                }
            }
            println!("{}", tr(MessageId::MainSetupNextMcp));
            Ok(())
        }
        McpCommand::List => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            if cfg.servers.is_empty() {
                println!(
                    "{}",
                    tr(MessageId::MainMcpNoServers)
                        .replace("{global}", &config_path.display().to_string())
                        .replace(
                            "{project}",
                            &crate::mcp::workspace_mcp_config_path(workspace)
                                .display()
                                .to_string()
                        )
                );
                return Ok(());
            }
            println!(
                "{}",
                tr(MessageId::MainMcpServersTitle)
                    .replace("{count}", &cfg.servers.len().to_string())
            );
            for (name, server) in cfg.servers {
                let status = if server.enabled && !server.disabled {
                    tr(MessageId::MainMcpEnabled)
                } else {
                    tr(MessageId::MainMcpDisabled)
                };
                let auth_status = crate::mcp::oauth::auth_status_for_server(&name, &server).await;
                let auth = if auth_status == crate::mcp::oauth::McpAuthStatus::Unsupported {
                    String::new()
                } else {
                    let label = match auth_status {
                        crate::mcp::oauth::McpAuthStatus::Unsupported => unreachable!(),
                        crate::mcp::oauth::McpAuthStatus::NotLoggedIn => {
                            tr(MessageId::MainMcpAuthNotLoggedIn)
                        }
                        crate::mcp::oauth::McpAuthStatus::BearerToken => {
                            tr(MessageId::MainMcpAuthBearerToken)
                        }
                        crate::mcp::oauth::McpAuthStatus::OAuth => tr(MessageId::MainMcpAuthOauth),
                    };
                    format!(" auth={label}")
                };
                let args = if server.args.is_empty() {
                    "".to_string()
                } else {
                    format!(" {}", server.args.join(" "))
                };
                let cmd_str = if let Some(cmd) = server.command {
                    format!("{cmd}{args}")
                } else if let Some(url) = server.url {
                    url
                } else {
                    tr(MessageId::MainMcpUnknown).into_owned()
                };
                let required = if server.required {
                    tr(MessageId::MainMcpRequired)
                } else {
                    Cow::Borrowed("")
                };
                println!("  - {name} [{status}{required}{auth}] {cmd_str}");
            }
            Ok(())
        }
        McpCommand::Connect { server } => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            if let Some(name) = server {
                if let Err(err) = pool.get_or_connect(&name).await {
                    if crate::mcp::oauth::error_looks_auth_required(&err) {
                        let hint = crate::mcp::oauth::auth_required_login_hint(&name);
                        return Err(err).context(hint);
                    }
                    return Err(err);
                }
                println!(
                    "{}",
                    tr(MessageId::MainMcpConnected).replace("{name}", &name)
                );
            } else {
                let errors = pool.connect_all().await;
                if errors.is_empty() {
                    println!("{}", tr(MessageId::MainMcpAllConnected));
                } else {
                    for (name, err) in errors {
                        eprintln!(
                            "{}",
                            tr(MessageId::MainMcpConnectFailed)
                                .replace("{name}", &name)
                                .replace("{error}", &format!("{err:#}"))
                        );
                        if crate::mcp::oauth::error_looks_auth_required(&err) {
                            eprintln!("  {}", crate::mcp::oauth::auth_required_login_hint(&name));
                        }
                    }
                }
            }
            Ok(())
        }
        McpCommand::Tools { server } => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            if let Some(name) = server {
                let conn = match pool.get_or_connect(&name).await {
                    Ok(conn) => conn,
                    Err(err) => {
                        if crate::mcp::oauth::error_looks_auth_required(&err) {
                            let hint = crate::mcp::oauth::auth_required_login_hint(&name);
                            return Err(err).context(hint);
                        }
                        return Err(err);
                    }
                };
                if conn.tools().is_empty() {
                    println!(
                        "{}",
                        tr(MessageId::MainMcpNoToolsServer).replace("{name}", &name)
                    );
                } else {
                    println!(
                        "{}",
                        tr(MessageId::MainMcpToolsFor).replace("{name}", &name)
                    );
                    for tool in conn.tools() {
                        println!(
                            "  - {}{}",
                            tool.name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            } else {
                let errors = pool.connect_all().await;
                for (name, err) in errors {
                    eprintln!(
                        "{}",
                        tr(MessageId::MainMcpConnectFailed)
                            .replace("{name}", &name)
                            .replace("{error}", &format!("{err:#}"))
                    );
                    if crate::mcp::oauth::error_looks_auth_required(&err) {
                        eprintln!("  {}", crate::mcp::oauth::auth_required_login_hint(&name));
                    }
                }
                let tools = pool.all_tools();
                if tools.is_empty() {
                    println!("{}", tr(MessageId::MainMcpNoTools));
                } else {
                    println!("{}", tr(MessageId::MainMcpToolsTitle));
                    for (name, tool) in tools {
                        println!(
                            "  - {}{}",
                            name,
                            tool.description
                                .as_ref()
                                .map_or(String::new(), |d| format!(": {d}"))
                        );
                    }
                }
            }
            Ok(())
        }
        McpCommand::Add {
            name,
            command,
            url,
            transport,
            bearer_token_env_var,
            oauth_client_id,
            oauth_resource,
            scopes,
            args,
        } => {
            let added_server = McpServerConfig {
                command,
                args,
                env: std::collections::HashMap::new(),
                cwd: None,
                url,
                transport,
                connect_timeout: None,
                execute_timeout: None,
                read_timeout: None,
                disabled: false,
                enabled: true,
                required: false,
                enabled_tools: Vec::new(),
                disabled_tools: Vec::new(),
                headers: std::collections::HashMap::new(),
                env_headers: std::collections::HashMap::new(),
                bearer_token_env_var,
                scopes,
                oauth: oauth_client_id.map(|client_id| McpServerOAuthConfig {
                    client_id: Some(client_id),
                }),
                oauth_resource,
            };
            let can_suggest_oauth = added_server.url.is_some()
                && added_server.bearer_token_env_var.is_none()
                && added_server
                    .headers
                    .keys()
                    .all(|key| !key.trim().eq_ignore_ascii_case("authorization"))
                && added_server
                    .env_headers
                    .keys()
                    .all(|key| !key.trim().eq_ignore_ascii_case("authorization"));
            crate::mcp::add_server_config(&config_path, name.clone(), added_server.clone())?;
            println!(
                "{}",
                tr(MessageId::MainMcpAdded)
                    .replace("{name}", &name)
                    .replace("{path}", &config_path.display().to_string())
            );
            if can_suggest_oauth
                && crate::mcp::oauth::oauth_login_support(&added_server)
                    .await
                    .is_ok_and(|support| support.is_some())
            {
                println!(
                    "{}",
                    tr(MessageId::MainMcpOauthAvailable).replace("{name}", &name)
                );
            }
            Ok(())
        }
        McpCommand::Login { name, scopes } => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            let server = cfg.servers.get(&name).ok_or_else(|| {
                anyhow!(
                    "{}",
                    tr(MessageId::MainMcpServerNotFound).replace("{name}", &name)
                )
            })?;
            let explicit_scopes = (!scopes.is_empty()).then_some(scopes);
            crate::mcp::oauth::perform_oauth_login_for_server(
                &name,
                server,
                explicit_scopes,
                config.mcp_oauth_callback_port,
                config.mcp_oauth_callback_url.as_deref(),
            )
            .await?;
            println!(
                "{}",
                tr(MessageId::MainMcpOauthStored).replace("{name}", &name)
            );
            Ok(())
        }
        McpCommand::Logout { name } => {
            let cfg = crate::mcp::load_config_with_workspace(&config_path, workspace)?;
            let server = cfg.servers.get(&name).ok_or_else(|| {
                anyhow!(
                    "{}",
                    tr(MessageId::MainMcpServerNotFound).replace("{name}", &name)
                )
            })?;
            if crate::mcp::oauth::delete_oauth_tokens_for_server(&name, server)? {
                println!(
                    "{}",
                    tr(MessageId::MainMcpOauthDeleted).replace("{name}", &name)
                );
            } else {
                println!(
                    "{}",
                    tr(MessageId::MainMcpOauthMissing).replace("{name}", &name)
                );
            }
            Ok(())
        }
        McpCommand::Remove { name } => {
            crate::mcp::remove_server_config(&config_path, &name)?;
            println!("{}", tr(MessageId::MainMcpRemoved).replace("{name}", &name));
            Ok(())
        }
        McpCommand::Enable { name } => {
            crate::mcp::set_server_enabled(&config_path, &name, true)?;
            println!(
                "{}",
                tr(MessageId::MainMcpEnabledNotice).replace("{name}", &name)
            );
            Ok(())
        }
        McpCommand::Disable { name } => {
            crate::mcp::set_server_enabled(&config_path, &name, false)?;
            println!(
                "{}",
                tr(MessageId::MainMcpDisabledNotice).replace("{name}", &name)
            );
            Ok(())
        }
        McpCommand::Validate => {
            let mut pool = McpPool::from_config_path_with_workspace(&config_path, workspace)?;
            let errors = pool.connect_all().await;
            if errors.is_empty() {
                println!("{}", tr(MessageId::MainMcpValid));
                return Ok(());
            }
            eprintln!("{}", tr(MessageId::MainMcpValidationFailed));
            for (name, err) in errors {
                eprintln!("  - {name}: {err:#}");
            }
            bail!("{}", tr(MessageId::MainMcpValidationError));
        }
    }
}

/// Diagnostic status for an MCP server entry.
#[derive(Debug)]
enum McpServerDoctorStatus {
    Ok(String),
    Warning(String),
    Error(String),
}

fn is_relative_stdio_path_arg(value: &str) -> bool {
    if value.is_empty() || value.starts_with('-') || value.contains("://") || value.starts_with('~')
    {
        return false;
    }
    let looks_like_path = value.contains('/') || value.contains('\\');
    if !looks_like_path {
        return false;
    }
    let bytes = value.as_bytes();
    let windows_absolute = value.starts_with("\\\\")
        || (bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/'));
    !Path::new(value).is_absolute() && !windows_absolute
}

/// Check an MCP server config entry for common issues.
fn doctor_check_mcp_server(server: &McpServerConfig) -> McpServerDoctorStatus {
    // No command or URL — incomplete entry.
    if server.command.is_none() && server.url.is_none() {
        return McpServerDoctorStatus::Error(tr(MessageId::DoctorMcpNoCommand).into_owned());
    }

    // URL-based server — just report the URL.
    if let Some(ref url) = server.url {
        return McpServerDoctorStatus::Ok(tr(MessageId::DoctorMcpHttpServer).replace("{url}", url));
    }

    // Command-based: validate command path exists.
    let cmd = server.command.as_deref().unwrap_or("");
    if cmd.is_empty() {
        return McpServerDoctorStatus::Error(tr(MessageId::DoctorMcpEmptyCommand).into_owned());
    }

    let cmd_path = Path::new(cmd);
    // Also accept Unix-style `/` prefix on Windows, where Path::is_absolute()
    // requires a drive letter.
    let is_absolute = cmd_path.is_absolute() || cmd.starts_with('/');

    if is_absolute && !cmd_path.exists() {
        return McpServerDoctorStatus::Error(
            tr(MessageId::DoctorMcpCommandNotFound).replace("{command}", cmd),
        );
    }

    if server.cwd.is_none() {
        if is_relative_stdio_path_arg(cmd) {
            return McpServerDoctorStatus::Warning(
                tr(MessageId::DoctorMcpRelativeCommand).replace("{command}", cmd),
            );
        }
        if let Some(arg) = server
            .args
            .iter()
            .find(|arg| is_relative_stdio_path_arg(arg))
        {
            return McpServerDoctorStatus::Warning(
                tr(MessageId::DoctorMcpRelativeArg).replace("{arg}", arg),
            );
        }
    }

    let args_str = server.args.join(" ");
    let command = if args_str.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {args_str}")
    };
    McpServerDoctorStatus::Ok(tr(MessageId::DoctorMcpStdioServer).replace("{command}", &command))
}

fn should_use_alt_screen(_cli: &Cli, _config: &Config) -> bool {
    true
}

fn should_use_mouse_capture(cli: &Cli, config: &Config, use_alt_screen: bool) -> bool {
    let terminal_emulator = std::env::var("TERMINAL_EMULATOR").ok();
    let wt_session = std::env::var("WT_SESSION").ok().filter(|s| !s.is_empty());
    let conemu_pid = std::env::var("ConEmuPID").ok().filter(|s| !s.is_empty());
    should_use_mouse_capture_with(
        cli,
        config,
        use_alt_screen,
        terminal_emulator.as_deref(),
        wt_session.as_deref(),
        conemu_pid.as_deref(),
    )
}

fn should_use_mouse_capture_with(
    cli: &Cli,
    config: &Config,
    use_alt_screen: bool,
    terminal_emulator: Option<&str>,
    wt_session: Option<&str>,
    conemu_pid: Option<&str>,
) -> bool {
    if !use_alt_screen || cli.no_mouse_capture {
        return false;
    }
    if cli.mouse_capture {
        return true;
    }
    config
        .tui
        .as_ref()
        .and_then(|tui| tui.mouse_capture)
        .unwrap_or_else(|| default_mouse_capture_enabled(terminal_emulator, wt_session, conemu_pid))
}

/// Whether to enable terminal mouse capture by default for this platform/host.
///
/// On Windows the default depends on the host: Windows Terminal (which sets
/// `WT_SESSION`) and ConEmu/Cmder (which set `ConEmuPID`) handle mouse-mode
/// reporting cleanly, so default-on there gives users in-app text selection
/// and keeps the application's selection clamped to the transcript area
/// (#1169). Legacy conhost (CMD without either env var) stays default-off
/// because its mouse-mode reporting can leak SGR escape sequences as raw
/// text into the composer (#878 / #898).
///
/// Off elsewhere only for JetBrains' JediTerm, which advertises mouse
/// support but forwards the same SGR escape sequences as raw input. The
/// user can still opt back in with `[tui] mouse_capture = true` in
/// `~/.dse/config.toml` or `--mouse-capture`.
fn default_mouse_capture_enabled(
    terminal_emulator: Option<&str>,
    wt_session: Option<&str>,
    conemu_pid: Option<&str>,
) -> bool {
    if cfg!(windows) {
        return wt_session.is_some() || conemu_pid.is_some();
    }
    if matches!(terminal_emulator, Some(t) if t.eq_ignore_ascii_case("JetBrains-JediTerm")) {
        return false;
    }
    true
}

/// Apply the remaining non-authority project configuration.
fn merge_project_config(config: &mut Config, workspace: &Path) {
    // When the workspace is the user's home directory, the project-scope
    // config file is also the global config file. Skip the merge to avoid
    // redundant processing and a misleading "project-scope config key
    // ignored" warning on every launch from ~.
    if let Some(home) = effective_home_dir()
        && let (Ok(w), Ok(h)) = (
            std::fs::canonicalize(workspace),
            std::fs::canonicalize(&home),
        )
        && w == h
    {
        return;
    }

    let path = workspace.join(dse_config::DSE_APP_DIR).join("config.toml");
    let raw = match read_project_config_file(&path) {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(err) => {
            eprintln!(
                "warning: failed to read project-scope config {}: {err}",
                path.display()
            );
            return;
        }
    };
    let project: toml::Value = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return,
    };
    let table = match project.as_table() {
        Some(t) => t,
        None => return,
    };

    // #417: dangerous keys are denied at project scope. A malicious
    // `<workspace>/.dse/config.toml` could otherwise:
    // * `api_key` / `base_url` / `provider` — exfiltrate prompts to a
    //   look-alike endpoint by swapping the user's credentials and
    //   target host with project-controlled values.
    // * `mcp_config_path` — point the loader at an MCP config that
    //   spawns arbitrary stdio servers under the user's identity.
    // * `mcp_oauth_callback_*` — choose local OAuth redirect listener
    //   behavior for user-owned MCP credentials.
    //
    // The overlay path is non-interactive; users can't visually
    // confirm a rogue project config is hijacking these. We surface
    // a stderr warning on first encounter so a user who *did* expect
    // the override has a chance to notice the deny instead of silent
    // discard.
    const DENY_AT_PROJECT_SCOPE: &[&str] = &[
        "api_key",
        "base_url",
        "provider",
        "mcp_config_path",
        "mcp_oauth_callback_port",
        "mcp_oauth_callback_url",
    ];
    for key in DENY_AT_PROJECT_SCOPE {
        if table.contains_key(*key) {
            eprintln!(
                "warning: project-scope config key `{key}` is ignored — \
                 set it in `~/.dse/config.toml` instead. \
                 (See #417 for the deny-list rationale.)"
            );
        }
    }

    // String fields a project may legitimately override (model and reasoning effort).
    for (key, field) in [
        ("model", &mut config.default_text_model),
        ("reasoning_effort", &mut config.reasoning_effort),
    ] {
        if let Some(v) = table.get(key).and_then(toml::Value::as_str)
            && !v.is_empty()
        {
            *field = Some(v.to_string());
        }
    }

    // Numeric / bool fields that benefit from per-project overrides.
    if let Some(v) = table.get("max_subagents").and_then(toml::Value::as_integer)
        && v > 0
    {
        config.max_subagents = Some((v as usize).clamp(1, crate::config::MAX_SUBAGENTS));
    }
    if let Some(v) = table.get("allow_shell").and_then(toml::Value::as_bool) {
        if v {
            eprintln!(
                "warning: project-scope `allow_shell = true` is ignored — \
                 enable shell from user config for this workspace instead. \
                 (See #417.)"
            );
        } else {
            config.allow_shell = Some(false);
        }
    }

    if table.contains_key("instructions") {
        eprintln!(
            "warning: project-scope `instructions` is ignored — \
             configure instruction files from user config instead. \
             (See #417.)"
        );
    }
}

fn read_project_config_file(path: &Path) -> io::Result<Option<String>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project-scope config must not be a symlink",
        ));
    }
    if !file_type.is_file() {
        return Ok(None);
    }

    let mut file = open_project_config_file(path)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(Some(raw))
}

#[cfg(unix)]
fn open_project_config_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_project_config_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}

fn merge_user_workspace_config(
    config: &mut Config,
    config_path: Option<PathBuf>,
    workspace: &Path,
) {
    let allow_shell_before = config.allow_shell;
    let allow_shell_from_env = std::env::var_os("DSE_ALLOW_SHELL").is_some();
    let Some(path) = crate::config::resolve_load_config_path(config_path) else {
        return;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(doc) = toml::from_str::<toml::Value>(&raw) else {
        return;
    };
    merge_user_workspace_config_from_doc(config, &doc, workspace);
    if allow_shell_from_env {
        config.allow_shell = allow_shell_before;
    }
}

fn merge_user_workspace_config_from_doc(config: &mut Config, doc: &toml::Value, workspace: &Path) {
    for table_name in ["workspace", "projects"] {
        let Some(entries) = doc.get(table_name).and_then(toml::Value::as_table) else {
            continue;
        };
        for (raw_path, entry) in entries {
            if !workspace_config_path_matches(raw_path, workspace) {
                continue;
            }
            if let Some(allow_shell) = entry.get("allow_shell").and_then(toml::Value::as_bool) {
                config.allow_shell = Some(allow_shell);
            }
        }
    }
}

fn workspace_config_path_matches(raw_path: &str, workspace: &Path) -> bool {
    let configured = crate::config::expand_path(raw_path);
    let configured = configured.canonicalize().unwrap_or(configured);
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    paths_equal_for_config(&configured, &workspace)
}

#[cfg(windows)]
fn paths_equal_for_config(left: &Path, right: &Path) -> bool {
    normalize_windows_config_path_for_compare(left)
        == normalize_windows_config_path_for_compare(right)
}

#[cfg(not(windows))]
fn paths_equal_for_config(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn normalize_windows_config_path_for_compare(path: &Path) -> String {
    normalize_windows_config_path_str(&path.to_string_lossy())
}

#[cfg(windows)]
fn normalize_windows_config_path_str(path: &str) -> String {
    let mut normalized = path.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\UNC\") {
        normalized = format!("\\\\{rest}");
    } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        normalized = rest.to_string();
    }
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized.to_ascii_lowercase()
}

fn interactive_tui_allow_shell(yolo: bool, config: &Config) -> bool {
    yolo || config.interactive_allow_shell()
}

async fn run_interactive(
    cli: &Cli,
    config: &Config,
    resume_session_id: Option<String>,
    initial_input: Option<tui::InitialInput>,
) -> Result<()> {
    let workspace = cli
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Merge project-level config from $WORKSPACE/.dse/config.toml unless
    // --no-project-config was passed (#485).
    let mut merged_config = config.clone();
    merge_user_workspace_config(&mut merged_config, cli.config.clone(), &workspace);
    if !cli.no_project_config {
        merge_project_config(&mut merged_config, &workspace);
    }
    let config = &merged_config;

    if !cli.skip_onboarding {
        match crate::config::ensure_config_file_exists(cli.config.clone()) {
            Ok(Some(path)) => logging::info(format!(
                "Created first-run config file at {}",
                path.display()
            )),
            Ok(None) => {}
            Err(err) => logging::warn(format!("Failed to create first-run config file: {err}")),
        }
    }

    let model = resolve_interactive_deepseek_model(config)?;
    let max_subagents = cli.max_subagents.map_or_else(
        || config.max_subagents(),
        |value| value.clamp(1, MAX_SUBAGENTS),
    );
    let use_alt_screen = should_use_alt_screen(cli, config);
    let use_mouse_capture = should_use_mouse_capture(cli, config, use_alt_screen);
    let use_bracketed_paste = crate::settings::Settings::load()
        .map(|s| s.effective_bracketed_paste())
        .unwrap_or_else(|_| !crate::settings::detected_legacy_windows_console_host());

    // Auto-install bundled system skills (e.g. skill-creator) on first launch.
    // Errors are non-fatal: log a warning and continue.
    let skills_dir = config.skills_dir();
    if let Err(e) = crate::skills::install_system_skills(&skills_dir) {
        logging::warn(format!("Failed to install system skills: {e}"));
    }

    startup_trace::mark("interactive_config");

    let yolo = cli.yolo;

    tui::run_tui(
        config,
        tui::TuiOptions {
            model,
            language: cli
                .language
                .or(config.ui_language()?)
                .unwrap_or_else(dse_localization::process_language),
            workspace,
            config_path: cli.config.clone(),
            allow_shell: interactive_tui_allow_shell(yolo, config),
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            skills_dir,
            mcp_config_path: config.mcp_config_path(),
            skip_onboarding: cli.skip_onboarding,
            yolo, // Explicit process-local Full access; never persisted.
            resume_session_id,
            initial_input,
            max_subagents,
        },
    )
    .await
}

fn normalize_cli_reasoning_effort(value: &str) -> Result<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let normalized = match trimmed.to_ascii_lowercase().as_str() {
        "inherit" | "parent" | "same" | "current" | "default" | "unset" => return Ok(None),
        "off" | "disabled" | "none" | "false" => "off",
        "low" | "minimal" => "low",
        "medium" | "mid" => "medium",
        "high" => "high",
        "max" | "maximum" | "xhigh" | "ultracode" => "max",
        _ => bail!(
            "Unrecognized --reasoning-effort {trimmed:?}. Expected: off, low, medium, high, max, or default."
        ),
    };
    Ok(Some(normalized.to_string()))
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
struct ExecSurfaceModelUsageBucket {
    model: String,
    api_surface: &'static str,
    response_count: u32,
    usage_response_count: u32,
    input_tokens: u32,
    output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_hit_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_miss_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_write_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_replay_tokens: Option<u32>,
    total_tokens: u64,
    cost_usd: f64,
    cost_cny: f64,
}

/// One stable Headless projection of the shared physical-request and provider
/// usage ledgers. Both JSON summary and stream metadata flatten this exact
/// receipt so startup, normal, and abnormal terminals cannot drift apart.
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
struct ExecAccountingReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_hit_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_miss_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_write_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_replay_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_cny: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_response_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    standard_chat_response_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict_chat_response_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    surface_model_usage_buckets: Option<Vec<ExecSurfaceModelUsageBucket>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_missing_responses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_incomplete_responses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    billing_unknown_attempts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unpriced_usage_responses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_records_after_seal_observed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_retry_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_completed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_in_flight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_root_started: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_root_completed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_root_in_flight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_child_started: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_child_completed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_child_in_flight: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_budget_exhausted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_rejected_exhausted: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_request_rejected_after_seal_observed: Option<u32>,
}

#[derive(serde::Serialize)]
struct ExecStreamMeta {
    receipt_kind: &'static str,
    provider: String,
    model: String,
    route_source: String,
    #[serde(flatten)]
    accounting: ExecAccountingReceipt,
    duration_ms: u64,
    approval_posture: String,
    sandbox_posture: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    binary_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_sha256: Option<String>,
    prompt_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_catalog_sha256: Option<String>,
    input_analysis: ExecStreamInputAnalysis,
    visible_final_answer_chars: usize,
    run_id: String,
    resume_command: String,
    workspace: String,
    message_count: usize,
    #[serde(flatten)]
    terminal: ExecTerminalReceipt,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_category: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Serialize, PartialEq, Eq)]
struct ExecStreamInputAnalysis {
    estimated_request_tokens: usize,
    estimated_message_content_tokens: usize,
    estimated_system_tokens: usize,
    estimated_framing_tokens: usize,
    user_message_count: usize,
    assistant_message_count: usize,
    tool_message_count: usize,
    tool_use_count: usize,
    tool_result_count: usize,
    text_chars: usize,
    thinking_chars: usize,
    tool_use_input_chars: usize,
    tool_result_chars: usize,
    text_estimated_tokens: usize,
    thinking_estimated_tokens: usize,
    tool_use_input_estimated_tokens: usize,
    tool_result_estimated_tokens: usize,
}

#[derive(serde::Serialize)]
#[serde(tag = "type")]
// Keep receipts flat for stable JSONL consumers. Boxing the whole tool_result
// payload would introduce a nested object and break the stream schema.
#[allow(clippy::large_enum_variant)]
enum ExecStreamEvent {
    #[serde(rename = "content")]
    Content { content: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        id: String,
        input: serde_json::Value,
        started_at: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        name: String,
        output: String,
        status: String,
        started_at: String,
        completed_at: String,
        duration_ms: u64,
        invocation_status: String,
        transport_status: String,
        operation_status: String,
        side_effect_status: String,
        retry_disposition: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        failure_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        truncated: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        artifact: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result_metadata: Option<serde_json::Value>,
    },
    #[serde(rename = "metadata")]
    Metadata { meta: Box<ExecStreamMeta> },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error {
        error: String,
        code: String,
        category: String,
        recoverable: bool,
        termination_reason: RunTerminationReason,
    },
}

fn exec_stream_line(event: &ExecStreamEvent) -> Result<Vec<u8>> {
    let mut value = serde_json::to_vec(&exec_stream_value(event)?)?;
    value.push(b'\n');
    Ok(value)
}

async fn write_exec_stream_terminal(
    output: &crate::exec_output::ExecOutput,
    event: &ExecStreamEvent,
) -> Result<bool, String> {
    let bytes = exec_stream_line(event).map_err(|error| format!("{error:#}"))?;
    wait_terminal_output(output.enqueue_stdout(bytes)).await
}

fn exec_stream_value(event: &ExecStreamEvent) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(event)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "schema_version".to_string(),
            serde_json::json!(crate::exec_lifecycle_stream::EXEC_STREAM_SCHEMA_VERSION),
        );
        object.insert(
            "schema".to_string(),
            serde_json::json!(crate::exec_lifecycle_stream::EXEC_STREAM_SCHEMA),
        );
    }
    Ok(value)
}

fn current_binary_sha256() -> Option<String> {
    static CURRENT_BINARY_SHA256: OnceLock<Option<String>> = OnceLock::new();
    CURRENT_BINARY_SHA256
        .get_or_init(|| {
            let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
            Some(format!("sha256:{}", crate::hashing::sha256_hex(&bytes)))
        })
        .clone()
}

#[cfg(test)]
mod m8a_deepseek_only_entry_tests {
    use super::*;

    #[test]
    fn exec_rejects_retired_provider_flag_at_parse_boundary() {
        let error = Cli::try_parse_from(["dse-tui", "exec", "--provider", "openrouter", "hello"])
            .expect_err("retired --provider must not parse");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(error.to_string().contains("仅使用官方 DeepSeek"));
    }

    #[test]
    fn exec_accepts_only_model_and_reasoning_overrides() {
        let cli = Cli::try_parse_from([
            "dse-tui",
            "exec",
            "--model",
            "deepseek-v4-flash",
            "--reasoning-effort",
            "high",
            "hello",
        ])
        .expect("DeepSeek exec arguments");
        let Some(Commands::Exec(args)) = cli.command else {
            panic!("expected exec command");
        };
        assert_eq!(args.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(args.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn interactive_model_rejects_foreign_model() {
        let config = Config {
            default_text_model: Some("gpt-5".to_string()),
            ..Config::default()
        };
        let error = resolve_interactive_deepseek_model(&config)
            .expect_err("foreign model must fail before runtime launch");
        assert!(error.to_string().contains("DeepSeek"));
    }

    #[test]
    fn doctor_route_is_official_deepseek_shape() {
        let config = Config::default();
        let route = doctor_route_report(&config);
        assert_eq!(route["provider"], "deepseek");
        assert_eq!(route["wire_protocol"], "chat_completions");
        assert_eq!(route["auth"]["scheme"], "bearer");
    }

    #[test]
    fn canonical_cli_has_no_fleet_or_direct_sandbox_shell() {
        let commands = Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(commands.len(), 13, "{commands:?}");
        assert!(!commands.iter().any(|command| command == "fleet"));
        assert!(!commands.iter().any(|command| command == "sandbox"));
    }
}

#[cfg(test)]
mod m8c_fixed_zh_hans_help_tests {
    use super::*;

    fn help_for(argv: &[&str]) -> String {
        localized_tui_command()
            .try_get_matches_from(argv)
            .expect_err("--help must stop parsing")
            .to_string()
    }

    #[test]
    fn direct_tui_help_uses_the_shared_fixed_catalog() {
        let help = help_for(&["dse-tui", "--help"]);
        assert!(help.contains("面向官方 DeepSeek API 的本地终端编码 Agent"));
        assert!(help.contains("用法："));
        assert!(help.contains("检查本地配置、凭据、运行环境与恢复建议"));
        for stable in ["doctor", "exec", "resume", "--workspace", "--prompt"] {
            assert!(help.contains(stable), "stable identity missing: {stable}");
        }
        for leak in [
            "DSE terminal coding agent",
            "Run system diagnostics",
            "Run a non-interactive prompt",
            "Resume a canonical Agent run",
            "possible values",
        ] {
            assert!(!help.contains(leak), "English product text leaked: {leak}");
        }

        let exec = help_for(&["dse-tui", "exec", "--help"]);
        assert!(exec.contains("本次运行的 thinking 强度"));
        assert!(exec.contains("Headless Agent 的最大运行秒数"));
        assert!(exec.contains("普通 `dse exec` 是一次性模型响应"));
        for leak in [
            "Override model for this run",
            "Enable agent-with-tools mode",
            "Maximum number of model steps",
            "Plain `dse exec` is a one-shot model response",
        ] {
            assert!(!exec.contains(leak), "English exec help leaked: {leak}");
        }
    }

    #[test]
    fn direct_tui_failure_localizes_framework_and_preserves_raw_detail() {
        let error = anyhow::anyhow!("raw-provider-sentinel").context("Headless 启动失败");
        let rendered = render_main_error(&error);

        assert!(rendered.starts_with("错误：Headless 启动失败"));
        assert!(rendered.contains("原因：raw-provider-sentinel"));
        assert!(!rendered.starts_with("Error:"));
    }

    #[test]
    fn doctor_mcp_status_localizes_host_chrome_and_preserves_raw_values() {
        let missing: McpServerConfig =
            serde_json::from_value(serde_json::json!({"command": null, "url": null}))
                .expect("minimal missing MCP config");
        let url: McpServerConfig = serde_json::from_value(serde_json::json!({
            "command": null,
            "url": "https://fixture.invalid/raw-path"
        }))
        .expect("minimal URL MCP config");
        let relative: McpServerConfig = serde_json::from_value(serde_json::json!({
            "command": "./raw-server",
            "url": null
        }))
        .expect("minimal relative MCP config");

        let McpServerDoctorStatus::Error(missing_detail) = doctor_check_mcp_server(&missing) else {
            panic!("missing MCP route must be an error");
        };
        assert_eq!(missing_detail, "尚未配置 command 或 url");

        let McpServerDoctorStatus::Ok(url_detail) = doctor_check_mcp_server(&url) else {
            panic!("URL MCP route must be accepted");
        };
        assert_eq!(
            url_detail,
            "HTTP/SSE 服务器：https://fixture.invalid/raw-path"
        );

        let McpServerDoctorStatus::Warning(relative_detail) = doctor_check_mcp_server(&relative)
        else {
            panic!("relative MCP route without cwd must warn");
        };
        assert!(relative_detail.contains("相对 command“./raw-server”"));
        assert!(relative_detail.contains("cwd"));
    }

    #[test]
    fn fixed_zh_hans_help_respects_80_and_120_column_widths() {
        use unicode_width::UnicodeWidthStr;

        for width in [80usize, 120] {
            for args in [
                ["dse-tui", "--help"].as_slice(),
                ["dse-tui", "exec", "--help"].as_slice(),
            ] {
                let help = localized_tui_command()
                    .term_width(width)
                    .try_get_matches_from(args)
                    .expect_err("--help must stop parsing")
                    .to_string();
                assert!(!help.contains('\u{fffd}'));
                for line in help.lines() {
                    assert!(
                        line.width() <= width,
                        "rendered help width {} exceeds {width}: {line:?}",
                        line.width()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod m17d_bilingual_entry_tests {
    use super::*;
    use clap::error::ErrorKind;
    use std::ffi::OsString;
    use unicode_width::UnicodeWidthStr;

    fn help_for(language: ProductLanguage, argv: &[&str], width: usize) -> String {
        let mut command = Cli::command();
        localize_tui_command_in(&mut command, language);
        let error = command
            .term_width(width)
            .try_get_matches_from(argv)
            .expect_err("--help must stop parsing");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        error.to_string()
    }

    #[test]
    fn direct_tui_english_help_is_complete_and_width_safe() {
        for width in [80usize, 120] {
            let help = help_for(ProductLanguage::English, &["dse-tui", "--help"], width);
            assert!(help.contains("A local terminal coding agent"));
            assert!(help.contains("Check local configuration"));
            assert!(help.contains("--language"));
            assert!(!help.contains("面向官方"));
            for line in help.lines() {
                assert!(
                    line.width() <= width,
                    "rendered help width {} exceeds {width}: {line:?}",
                    line.width()
                );
            }
        }
    }

    #[test]
    fn language_precedence_scanner_and_first_run_choice_are_strict() {
        let arguments = ["dse-tui", "--config", "/tmp/dse.toml", "--language=zh-Hans"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (language, config) = startup_language_arguments(&arguments);
        assert_eq!(language.as_deref(), Some("zh-Hans"));
        assert_eq!(config.as_deref(), Some(Path::new("/tmp/dse.toml")));

        assert_eq!(
            parse_first_run_language_choice("").unwrap(),
            ProductLanguage::English
        );
        assert_eq!(
            parse_first_run_language_choice("2").unwrap(),
            ProductLanguage::SimplifiedChinese
        );
        assert!(parse_first_run_language_choice("fr").is_err());
        assert!(Cli::try_parse_from(["dse-tui", "--language", "en-US"]).is_err());
    }
}
