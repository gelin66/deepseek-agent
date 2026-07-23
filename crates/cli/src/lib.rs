#![allow(clippy::uninlined_format_args)]

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use codewhale_app::{AgentApplication, ProductionApplicationConfig, ProductionPromptConfig};
use codewhale_app_server::{
    AppServerOptions, DEFAULT_MAX_BODY_BYTES, run as run_app_server,
    run_stdio as run_app_server_stdio,
};
use codewhale_config::{
    CliRuntimeOverrides, ConfigStore, ResolvedRuntimeOptions, RuntimeApiKeySource,
    canonical_deepseek_model, is_official_deepseek_base_url, load_prompt_preferences,
};
use codewhale_execpolicy::{AskForApproval, ExecPolicyContext, ExecPolicyEngine};
use codewhale_localization::{MessageId, tr};
use codewhale_protocol::run_api::{
    DEFAULT_RUN_LIST_LIMIT, MAX_RUN_LIST_LIMIT, RUN_API_SCHEMA_VERSION, RootRunSummary, RunCommand,
    RunCommandEnvelope, RunCommandResponse, RunCommandResult,
};
use codewhale_secrets::Secrets;
use codewhale_state::{StateStore, ThreadListFilters};

#[derive(Debug, Parser)]
#[command(
    name = "codewhale",
    version = env!("CODEWHALE_BUILD_VERSION"),
    bin_name = "codewhale",
    override_usage = "codewhale [OPTIONS] [PROMPT]\n       codewhale [OPTIONS] <COMMAND> [ARGS]"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long = "output-mode")]
    output_mode: Option<String>,
    #[arg(
        long = "verbosity",
        value_name = "LEVEL",
        help = "Controls transcript and output verbosity (normal, concise)"
    )]
    verbosity: Option<String>,
    #[arg(long = "log-level")]
    log_level: Option<String>,
    #[arg(long)]
    telemetry: Option<bool>,
    #[arg(long)]
    approval_policy: Option<String>,
    #[arg(long)]
    sandbox_mode: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    /// Workspace directory for TUI file tools
    #[arg(short = 'C', long = "workspace", alias = "cd", value_name = "DIR")]
    workspace: Option<PathBuf>,
    #[arg(long = "no-alt-screen", hide = true)]
    no_alt_screen: bool,
    #[arg(long = "mouse-capture", conflicts_with = "no_mouse_capture")]
    mouse_capture: bool,
    #[arg(long = "no-mouse-capture", conflicts_with = "mouse_capture")]
    no_mouse_capture: bool,
    #[arg(long = "skip-onboarding")]
    skip_onboarding: bool,
    /// Explicit startup override: enable Shell, automatic approval, and workspace-external trust.
    #[arg(long, hide = true)]
    yolo: bool,
    /// Continue the most recent interactive session for this workspace.
    #[arg(short = 'c', long = "continue")]
    continue_session: bool,
    #[arg(short = 'p', long = "prompt", value_name = "PROMPT")]
    prompt_flag: Option<String>,
    #[arg(value_name = "PROMPT")]
    prompt: Vec<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run CodeWhale diagnostics.
    Doctor(TuiPassthroughArgs),
    /// 列出当前工作区的 canonical Agent 运行。
    Runs(RunsArgs),
    /// 恢复指定 canonical Agent 运行，或使用 --last。
    Resume(TuiPassthroughArgs),
    /// Create a default AGENTS.md in the current directory.
    Init(TuiPassthroughArgs),
    /// Bootstrap MCP config and/or skills directories.
    Setup(TuiPassthroughArgs),
    /// Run a non-interactive prompt through the TUI runtime.
    #[command(after_help = "\
Examples:
  codewhale exec \"explain this function\"
  codewhale exec --auto \"list crates/ with ls\"
  codewhale exec --auto --output-format stream-json \"fix the failing test\"

Common forwarded flags:
  --auto                           Enable tool-backed agent mode with auto-approvals
  --json                           Emit summary JSON
  --resume <RUN_ID>                Resume one canonical Agent run
  --continue                       Continue the latest root run for this workspace
  --output-format <FORMAT>         Output format: text or stream-json
  --max-api-requests <COUNT>       Hard cap on real DeepSeek HTTP requests
  --max-runtime-secs <SECONDS>     Hard wall-clock cap for the Headless Agent

Plain `codewhale exec` is a one-shot model response. Use `--auto` for
non-interactive filesystem/shell tool use, matching the supported automation
path used by stream-json wrappers.
")]
    Exec(TuiPassthroughArgs),
    /// Manage durable Agent Fleet runs via the TUI runtime.
    Fleet(TuiPassthroughArgs),
    /// Internal detached-runtime output/receipt supervisor.
    #[command(name = "lane-log-proxy", hide = true)]
    LaneLogProxy(LaneLogProxyArgs),
    /// Manage running workflow instances (Lanes) and Runtime backends (#4176).
    #[command(after_help = "\
Examples:
  codewhale lane list
  codewhale lane status <lane-id>
  codewhale lane attach <lane-id>
  codewhale lane logs <lane-id>
  codewhale lane stop <lane-id>
  codewhale lane start --workflow stopship --fleet v0868-stopship --runtime tmux --issue 4090 -- echo hello

Lane records persist under $CODEWHALE_HOME/lanes/. tmux durability belongs to
Runtime, not Fleet.
")]
    Lane(LaneArgs),
    /// Manage TUI MCP servers.
    Mcp(TuiPassthroughArgs),
    /// Inspect TUI feature flags.
    Features(TuiPassthroughArgs),
    /// Generate shell completions for the TUI binary.
    Completions(TuiPassthroughArgs),
    /// 保存 DeepSeek API Key。
    Login(LoginArgs),
    /// 删除已保存的 DeepSeek 凭据。
    Logout,
    /// 查看或管理 DeepSeek 凭据。
    Auth(AuthArgs),
    /// Read/write/list config values.
    Config(ConfigArgs),
    /// 查看或设置 DeepSeek 模型。
    Model(ModelArgs),
    /// Manage thread/session metadata and resume/fork flows.
    Thread(ThreadArgs),
    /// Evaluate sandbox/approval policy decisions.
    Sandbox(SandboxArgs),
    /// Run the canonical local Run API over HTTP/SSE or stdio.
    #[command(after_help = "\
Transports:
  codewhale app-server                     HTTP/SSE Run API on 127.0.0.1:7878
  codewhale app-server --stdio             Canonical newline Run API on stdio

HTTP requires --auth-token (or CODEWHALE_APP_SERVER_TOKEN) unless the user
explicitly selects --insecure-no-auth on a loopback address.")]
    AppServer(AppServerArgs),
    /// Generate shell completions.
    #[command(after_help = r#"Examples:
  Bash (current shell only):
    source <(codewhale completion bash)

  Bash (persistent, Linux/bash-completion):
    mkdir -p ~/.local/share/bash-completion/completions
    codewhale completion bash > ~/.local/share/bash-completion/completions/codewhale
    # Requires bash-completion to be installed and loaded by your shell.

  Zsh:
    mkdir -p ~/.zfunc
    codewhale completion zsh > ~/.zfunc/_codewhale
    # Add to ~/.zshrc if needed:
    #   fpath=(~/.zfunc $fpath)
    #   autoload -Uz compinit && compinit

  Fish:
    mkdir -p ~/.config/fish/completions
    codewhale completion fish > ~/.config/fish/completions/codewhale.fish

  PowerShell (current shell only):
    codewhale completion powershell | Out-String | Invoke-Expression

The command prints the completion script to stdout; redirect it to a path your shell loads automatically."#)]
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Args)]
struct RunsArgs {
    /// 最多显示多少个用户 Agent 运行。
    #[arg(
        long,
        default_value_t = DEFAULT_RUN_LIST_LIMIT,
        value_parser = parse_run_list_limit
    )]
    limit: u32,
    /// 输出 versioned canonical Run API JSON。
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct TuiPassthroughArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Args)]
struct LaneLogProxyArgs {
    #[arg(long, value_name = "PATH")]
    log_path: PathBuf,
    #[arg(long, value_name = "PATH")]
    receipt_path: PathBuf,
    #[arg(long, value_name = "PATH")]
    receipt_tmp_path: PathBuf,
    #[arg(long, value_name = "PATH")]
    environment_path: Option<PathBuf>,
    #[arg(long)]
    lane_id: String,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    command: Vec<String>,
}

/// `codewhale lane …` — running workflow instances (#4176).
#[derive(Debug, Args)]
struct LaneArgs {
    #[command(subcommand)]
    command: LaneCommand,
}

#[derive(Debug, Subcommand)]
// Clap constructs this command enum once at process startup. Keeping the
// fields inline makes the generated CLI shape explicit; boxing them only to
// reduce this transient value would add indirection without runtime benefit.
#[allow(clippy::large_enum_variant)]
enum LaneCommand {
    /// List known lanes (newest first).
    List {
        /// Emit JSON.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Show one lane's status and attach metadata.
    Status {
        /// Lane id (e.g. `lane-a1b2c3d4`).
        lane_id: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Attach to a tmux-backed lane (prints attach command; execs when possible).
    Attach {
        lane_id: String,
        /// Only print the attach command; do not exec.
        #[arg(long, default_value_t = false)]
        print: bool,
    },
    /// Tail the lane stream-json / NDJSON journal.
    Logs {
        lane_id: String,
        /// Follow the log file (like `tail -f`).
        #[arg(long, short = 'f', default_value_t = false)]
        follow: bool,
        /// Number of trailing lines when not following (default 50).
        #[arg(long, default_value_t = 50)]
        tail: usize,
    },
    /// Stop a running lane.
    Stop { lane_id: String },
    /// Start a lane under a Runtime backend (tmux|inline|vm|ci).
    Start {
        /// Workflow name (e.g. `stopship`).
        #[arg(long)]
        workflow: Option<String>,
        /// Fleet roster name (e.g. `v0868-stopship`).
        #[arg(long)]
        fleet: Option<String>,
        /// Issue id binding.
        #[arg(long)]
        issue: Option<String>,
        /// Free-form goal text.
        #[arg(long)]
        goal: Option<String>,
        /// Runtime backend: tmux, inline, vm, or ci.
        #[arg(long, default_value = "tmux")]
        runtime: String,
        /// Command to run in the runtime (after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

struct LaneStartRequest {
    workflow: Option<String>,
    fleet: Option<String>,
    issue: Option<String>,
    goal: Option<String>,
    runtime: String,
    command: Vec<String>,
    environment: Vec<(String, String)>,
    cwd: Option<PathBuf>,
}

fn start_lane(request: LaneStartRequest) -> Result<()> {
    use codewhale_lane::{LaneRegistry, LaneStartSpec, RuntimeBackendKind, resolve_backend};

    let LaneStartRequest {
        workflow,
        fleet,
        issue,
        goal,
        runtime,
        command,
        environment,
        cwd,
    } = request;
    let kind = RuntimeBackendKind::parse(&runtime)?;
    let reg = LaneRegistry::open_default()?;
    let mut record = reg.create_pending(workflow, fleet, issue, goal, kind)?;
    let cmd = if command.is_empty() {
        vec![
            "sh".into(),
            "-c".into(),
            format!("echo lane {} started", record.id),
        ]
    } else {
        command
    };
    let spec = LaneStartSpec {
        command: cmd,
        cwd,
        environment,
        log_proxy: (kind == RuntimeBackendKind::Tmux)
            .then(std::env::current_exe)
            .transpose()
            .context("resolve current Codewhale executable for tmux log proxy")?,
    };
    let backend = resolve_backend(kind);
    backend.start(&reg, &mut record, &spec)?;
    println!("started {}", record.id);
    println!("status:  {}", record.status.as_str());
    println!("runtime: {}", record.runtime.as_str());
    println!("log:     {}", record.log_path.display());
    if let Some(attach) = backend.attach_command(&record) {
        println!("attach:  {attach}");
    }
    Ok(())
}

fn run_lane_command(args: LaneArgs) -> Result<()> {
    use codewhale_lane::{LaneRegistry, backend_for};
    use std::io::{BufRead, Seek, Write};
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    match args.command {
        LaneCommand::List { json } => {
            let reg = LaneRegistry::open_default()?;
            let mut lanes = reg.list()?;
            for lane in &mut lanes {
                if let Err(err) = backend_for(lane).reconcile(&reg, lane) {
                    eprintln!("warning: could not reconcile lane `{}`: {err:#}", lane.id);
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&lanes)?);
            } else if lanes.is_empty() {
                println!("No lanes under {}", reg.root().display());
            } else {
                println!(
                    "{:<16} {:<10} {:<12} {:<16} {:<10} STARTED",
                    "ID", "STATUS", "RUNTIME", "WORKFLOW", "ISSUE"
                );
                for lane in lanes {
                    println!(
                        "{:<16} {:<10} {:<12} {:<16} {:<10} {}",
                        lane.id,
                        lane.status.as_str(),
                        lane.runtime.as_str(),
                        lane.workflow.as_deref().unwrap_or("-"),
                        lane.issue.as_deref().unwrap_or("-"),
                        lane.started_at,
                    );
                }
            }
            Ok(())
        }
        LaneCommand::Status { lane_id, json } => {
            let reg = LaneRegistry::open_default()?;
            let mut lane = reg.load(&lane_id)?;
            backend_for(&lane).reconcile(&reg, &mut lane)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&lane)?);
            } else {
                println!("lane:     {}", lane.id);
                println!("status:   {}", lane.status.as_str());
                println!("runtime:  {}", lane.runtime.as_str());
                println!("workflow: {}", lane.workflow.as_deref().unwrap_or("-"));
                println!("fleet:    {}", lane.fleet.as_deref().unwrap_or("-"));
                println!("issue:    {}", lane.issue.as_deref().unwrap_or("-"));
                println!("goal:     {}", lane.goal.as_deref().unwrap_or("-"));
                println!("started:  {}", lane.started_at);
                println!("stopped:  {}", lane.stopped_at.as_deref().unwrap_or("-"));
                println!("tmux:     {}", lane.tmux_session.as_deref().unwrap_or("-"));
                println!(
                    "socket:   {}",
                    lane.tmux_socket
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
                println!("attach:   {}", lane.attach_target.as_deref().unwrap_or("-"));
                println!("log:      {}", lane.log_path.display());
            }
            Ok(())
        }
        LaneCommand::Attach { lane_id, print } => {
            let reg = LaneRegistry::open_default()?;
            let mut lane = reg.load(&lane_id)?;
            let backend = backend_for(&lane);
            backend.reconcile(&reg, &mut lane)?;
            let Some(attach) = backend.attach_command(&lane) else {
                if !lane.status.is_active() {
                    bail!(
                        "lane `{lane_id}` is {} and has no active attach target",
                        lane.status.as_str()
                    );
                }
                bail!(
                    "lane `{lane_id}` runtime `{}` has no attach target",
                    lane.runtime.as_str()
                );
            };
            if print {
                println!("{attach}");
                return Ok(());
            }
            if let Some(session) = lane.tmux_session.as_deref() {
                let socket = lane
                    .tmux_socket
                    .as_deref()
                    .context("tmux lane is missing its pinned server socket")?;
                let status = Command::new("tmux")
                    .arg("-S")
                    .arg(socket)
                    .args(["attach", "-t", session])
                    .status();
                match status {
                    Ok(s) if s.success() => Ok(()),
                    Ok(s) => bail!("tmux attach failed ({s}); command was: {attach}"),
                    Err(err) => {
                        eprintln!("could not exec tmux: {err}");
                        println!("{attach}");
                        bail!("tmux attach unavailable");
                    }
                }
            } else {
                println!("{attach}");
                Ok(())
            }
        }
        LaneCommand::Logs {
            lane_id,
            follow,
            tail,
        } => {
            let reg = LaneRegistry::open_default()?;
            let lane = reg.load(&lane_id)?;
            let path = lane.log_path;
            if !path.exists() {
                bail!("log file missing: {}", path.display());
            }
            let content = std::fs::read(&path)?;
            let lines: Vec<&[u8]> = content
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .collect();
            let start = lines.len().saturating_sub(tail);
            let mut stdout = std::io::stdout().lock();
            for line in &lines[start..] {
                stdout.write_all(String::from_utf8_lossy(line).as_bytes())?;
                stdout.write_all(b"\n")?;
            }
            stdout.flush()?;
            if !follow {
                return Ok(());
            }
            let mut file = std::fs::File::open(&path)?;
            file.seek(std::io::SeekFrom::End(0))?;
            let mut reader = std::io::BufReader::new(file);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => {
                        thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                    Ok(_) => {
                        let mut stdout = std::io::stdout().lock();
                        stdout.write_all(String::from_utf8_lossy(&line).as_bytes())?;
                        stdout.flush()?;
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }
        LaneCommand::Stop { lane_id } => {
            let reg = LaneRegistry::open_default()?;
            let mut lane = reg.load(&lane_id)?;
            let backend = backend_for(&lane);
            backend.stop(&reg, &mut lane)?;
            println!("stopped {}", lane.id);
            Ok(())
        }
        LaneCommand::Start {
            workflow,
            fleet,
            issue,
            goal,
            runtime,
            command,
        } => start_lane(LaneStartRequest {
            workflow,
            fleet,
            issue,
            goal,
            runtime,
            command,
            environment: Vec::new(),
            cwd: None,
        }),
    }
}

fn run_lane_log_proxy_command(args: LaneLogProxyArgs) -> Result<()> {
    let exit_code = codewhale_lane::run_lane_log_proxy(codewhale_lane::LaneLogProxySpec {
        command: args.command,
        log_path: args.log_path,
        receipt_path: args.receipt_path,
        receipt_tmp_path: args.receipt_tmp_path,
        environment_path: args.environment_path,
        lane_id: args.lane_id,
    })?;
    std::process::exit(exit_code);
}

#[derive(Debug, Args)]
struct LoginArgs {
    #[arg(long)]
    api_key: Option<String>,
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// 显示 DeepSeek 凭据来源，不显示凭据内容。
    Status,
    /// 保存 DeepSeek API Key。读取
    /// `--api-key`, `--api-key-stdin`, or prompts on stdin when
    /// neither is given. Does not echo the key.
    Set {
        /// Inline value (discouraged — appears in shell history).
        #[arg(long)]
        api_key: Option<String>,
        /// Read the key from stdin instead of prompting.
        #[arg(long = "api-key-stdin", default_value_t = false)]
        api_key_stdin: bool,
    },
    /// 显示 DeepSeek API Key 是否已配置。
    Get,
    /// 从配置文件和凭据存储中删除 DeepSeek API Key。
    Clear,
    /// Advanced: migrate config-file keys into a platform credential store.
    #[command(hide = true)]
    Migrate {
        /// Don't actually write anything; print what would change.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
    List,
    Path,
}

#[derive(Debug, Args)]
struct ModelArgs {
    #[command(subcommand)]
    command: ModelCommand,
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    List,
    Resolve {
        model: Option<String>,
    },
    /// Set the default model (e.g. "pro", "flash", "deepseek-v4-pro").
    Set {
        model: String,
    },
}

#[derive(Debug, Args)]
struct ThreadArgs {
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    List {
        #[arg(long, default_value_t = false)]
        all: bool,
        #[arg(long)]
        limit: Option<usize>,
    },
    Read {
        thread_id: String,
    },
    Resume {
        thread_id: String,
    },
    Fork {
        thread_id: String,
    },
    Archive {
        thread_id: String,
    },
    Unarchive {
        thread_id: String,
    },
    SetName {
        thread_id: String,
        name: String,
    },
    /// Remove the custom name from a thread, restoring the default
    /// `(unnamed)` rendering in `thread list`.
    ClearName {
        thread_id: String,
    },
}

#[derive(Debug, Args)]
struct SandboxArgs {
    #[command(subcommand)]
    command: SandboxCommand,
}

#[derive(Debug, Subcommand)]
enum SandboxCommand {
    Check {
        command: String,
        #[arg(long, value_enum, default_value_t = ApprovalModeArg::OnRequest)]
        ask: ApprovalModeArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ApprovalModeArg {
    UnlessTrusted,
    OnFailure,
    OnRequest,
    Never,
}

impl From<ApprovalModeArg> for AskForApproval {
    fn from(value: ApprovalModeArg) -> Self {
        match value {
            ApprovalModeArg::UnlessTrusted => AskForApproval::UnlessTrusted,
            ApprovalModeArg::OnFailure => AskForApproval::OnFailure,
            ApprovalModeArg::OnRequest => AskForApproval::OnRequest,
            ApprovalModeArg::Never => AskForApproval::Never,
        }
    }
}

#[derive(Debug, Args)]
struct AppServerArgs {
    /// Run the same canonical newline Run API over stdio instead of HTTP/SSE.
    #[arg(
        long,
        default_value_t = false,
        conflicts_with_all = [
            "host",
            "port",
            "auth_token",
            "insecure_no_auth",
            "cors_origin",
            "max_body_bytes"
        ]
    )]
    stdio: bool,
    /// HTTP bind host. Defaults to 127.0.0.1.
    #[arg(long)]
    host: Option<IpAddr>,
    /// HTTP bind port. Defaults to 7878.
    #[arg(long)]
    port: Option<u16>,
    /// Bearer token required by the HTTP Run API.
    #[arg(long = "auth-token")]
    auth_token: Option<String>,
    /// Allow unauthenticated HTTP only on a loopback bind address.
    #[arg(long, default_value_t = false)]
    insecure_no_auth: bool,
    /// Allowed browser origin. Repeat for more than one origin.
    #[arg(long = "cors-origin")]
    cors_origin: Vec<String>,
    /// Maximum accepted HTTP request body size.
    #[arg(long = "max-body-bytes")]
    max_body_bytes: Option<usize>,
    /// Maximum transport retries for each DeepSeek request.
    #[arg(long = "transport-max-retries", value_parser = clap::value_parser!(u32).range(0..=10))]
    transport_max_retries: Option<u32>,
}

fn install_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn run_cli() -> std::process::ExitCode {
    install_rustls_crypto_provider();

    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            // Use the full anyhow chain so callers see the underlying
            // cause (e.g. the actual TOML parse error with line/column)
            // instead of just the top-level context message. The bare
            // `{err}` Display impl drops the chain — see #767, where
            // users hit "failed to parse config at <path>" with no
            // hint that the real error was a stray BOM or unbalanced
            // quote a few lines down.
            eprintln!(
                "{}",
                tr(MessageId::CliErrorPrefix).replace("{error}", &err.to_string())
            );
            for cause in err.chain().skip(1) {
                eprintln!(
                    "  {}",
                    tr(MessageId::CliCausedByPrefix).replace("{error}", &cause.to_string())
                );
            }
            std::process::ExitCode::FAILURE
        }
    }
}

fn split_lane_log_proxy_command(
    command: Option<Commands>,
) -> (Option<LaneLogProxyArgs>, Option<Commands>) {
    match command {
        Some(Commands::LaneLogProxy(args)) => (Some(args), None),
        command => (None, command),
    }
}

fn reject_retired_command(cli: &Cli) -> Result<()> {
    if cli.prompt_flag.is_none() && cli.command.is_none() {
        match cli.prompt.first().map(String::as_str) {
            Some("sessions") => bail!(
                "命令 `codewhale sessions` 已删除；请使用 `codewhale runs` 查看当前工作区的 canonical Agent 运行"
            ),
            Some("fork") => bail!(
                "命令 `codewhale fork` 已删除；不再支持旧 TUI 会话分叉，请使用 `codewhale resume <RUN_ID>` 继续 canonical Agent 运行"
            ),
            Some("run") => bail!(
                "命令 `codewhale run` 已删除；请直接运行 `codewhale` 启动交互界面，或使用 `codewhale exec <PROMPT>` 执行非交互任务"
            ),
            Some("mcp-server") => bail!(
                "命令 `codewhale mcp-server` 已删除；如需本地 Agent 接口，请使用 canonical `codewhale app-server --stdio`"
            ),
            Some("update") => bail!(
                "命令 `codewhale update` 已删除；本项目不再内置自更新器，请通过当前安装渠道重新安装或升级"
            ),
            Some("metrics") => bail!(
                "命令 `codewhale metrics` 已删除；旧日志/会话扫描不是 canonical RunStore 指标来源"
            ),
            Some("workflow") => bail!(
                "命令 `codewhale workflow` 已删除；多 Agent 请使用 canonical `agent` 能力，写 Agent 的 worktree 由唯一 Orchestrator 管理"
            ),
            Some("workflow-tool") => {
                bail!("命令 `codewhale workflow-tool` 已删除；旧 Workflow 第二运行时不再提供")
            }
            Some("review") => {
                bail!("命令 `codewhale review` 已删除；请使用 canonical Agent 审查当前 git diff")
            }
            Some("speech") | Some("tts") => {
                bail!("命令 `codewhale speech` / `codewhale tts` 已删除")
            }
            _ => {}
        }
    }

    if let Some(Commands::Mcp(args)) = cli.command.as_ref()
        && args.args.first().is_some_and(|arg| arg == "add-self")
    {
        bail!("命令 `codewhale mcp add-self` 已删除；CodeWhale 不再把自身注册为 MCP 服务端");
    }

    Ok(())
}

fn cli_command_message(name: &str) -> Option<MessageId> {
    Some(match name {
        "doctor" => MessageId::CliCommandDoctor,
        "runs" => MessageId::CliCommandRuns,
        "resume" => MessageId::CliCommandResume,
        "init" => MessageId::CliCommandInit,
        "setup" => MessageId::CliCommandSetup,
        "exec" => MessageId::CliCommandExec,
        "fleet" => MessageId::CliCommandFleet,
        "lane" => MessageId::CliCommandLane,
        "mcp" => MessageId::CliCommandMcp,
        "features" => MessageId::CliCommandFeatures,
        "completions" => MessageId::CliCommandCompletions,
        "login" => MessageId::CliCommandLogin,
        "logout" => MessageId::CliCommandLogout,
        "auth" => MessageId::CliCommandAuth,
        "config" => MessageId::CliCommandConfig,
        "model" => MessageId::CliCommandModel,
        "thread" => MessageId::CliCommandThread,
        "sandbox" => MessageId::CliCommandSandbox,
        "app-server" => MessageId::CliCommandAppServer,
        "completion" => MessageId::CliCommandCompletion,
        _ => return None,
    })
}

fn localize_cli_command(command: &mut clap::Command) {
    let mut localized = command
        .clone()
        .help_template(tr(MessageId::CliHelpTemplate).into_owned())
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
            .help(tr(MessageId::CliArgHelp).into_owned()),
    );
    if command.get_version().is_some() {
        localized = localized.arg(
            clap::Arg::new("version")
                .short('V')
                .long("version")
                .action(clap::ArgAction::Version)
                .help(tr(MessageId::CliArgVersion).into_owned()),
        );
    }
    if command.get_name() == "codewhale" {
        localized = localized.about(tr(MessageId::CliAbout).into_owned());
    } else if let Some(message) = cli_command_message(command.get_name()) {
        localized = localized.about(tr(message).into_owned());
    }
    if command.get_name() == "exec" {
        localized = localized.after_help(tr(MessageId::CliExecAfterHelp).into_owned());
    } else if command.get_name() == "app-server" {
        localized = localized.after_help(tr(MessageId::CliAppServerAfterHelp).into_owned());
    }

    let arg_messages = [
        ("verbosity", MessageId::CliArgVerbosity),
        ("workspace", MessageId::CliArgWorkspace),
        ("continue_session", MessageId::CliArgContinue),
        ("prompt", MessageId::CliArgPrompt),
        ("prompt_flag", MessageId::CliArgPrompt),
        ("api_key", MessageId::CliArgApiKey),
        ("json", MessageId::CliArgJson),
        ("limit", MessageId::CliArgLimit),
        ("config", MessageId::CliArgConfig),
        ("profile", MessageId::CliArgProfile),
        ("model", MessageId::CliArgModel),
        ("output_mode", MessageId::CliArgOutputMode),
        ("log_level", MessageId::CliArgLogLevel),
        ("telemetry", MessageId::CliArgTelemetry),
        ("approval_policy", MessageId::CliArgApprovalPolicy),
        ("sandbox_mode", MessageId::CliArgSandboxMode),
        ("base_url", MessageId::CliArgBaseUrl),
        ("mouse_capture", MessageId::CliArgMouseCapture),
        ("no_mouse_capture", MessageId::CliArgNoMouseCapture),
        ("skip_onboarding", MessageId::CliArgSkipOnboarding),
        ("stdio", MessageId::CliArgStdio),
        ("host", MessageId::CliArgHost),
        ("port", MessageId::CliArgPort),
        ("auth_token", MessageId::CliArgAuthToken),
        ("insecure_no_auth", MessageId::CliArgInsecureNoAuth),
        ("cors_origin", MessageId::CliArgCorsOrigin),
        ("max_body_bytes", MessageId::CliArgMaxBodyBytes),
        (
            "transport_max_retries",
            MessageId::CliArgTransportMaxRetries,
        ),
    ];
    for (id, message) in arg_messages {
        if localized
            .get_arguments()
            .any(|argument| argument.get_id() == id)
        {
            localized = localized.mut_arg(id, |argument| argument.help(tr(message).into_owned()));
        }
    }

    *command = localized;
    for child in command.get_subcommands_mut() {
        localize_cli_command(child);
    }
}

fn localized_cli_command() -> clap::Command {
    let mut command = Cli::command();
    localize_cli_command(&mut command);
    command
}

fn parse_cli() -> Cli {
    let matches = match localized_cli_command().try_get_matches() {
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

fn run() -> Result<()> {
    let mut cli = parse_cli();
    // Clap intentionally accepts free-form root prompts. Reject retired
    // top-level command spellings before config, TUI, Store, or model setup so
    // an old command can never become an accidental paid prompt.
    reject_retired_command(&cli)?;

    // The detached log proxy must not depend on user config parsing: its job
    // is to frame child output and publish a terminal receipt even when the
    // delegated command's own config is malformed.
    let (proxy, command) = split_lane_log_proxy_command(cli.command.take());
    if let Some(args) = proxy {
        return run_lane_log_proxy_command(args);
    }

    let command = match command {
        Some(Commands::Completion { shell }) => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "codewhale", &mut io::stdout());
            return Ok(());
        }
        Some(Commands::Runs(args)) => return run_runs_command(&cli, args),
        command => command,
    };

    let mut store = ConfigStore::load(cli.config.clone())?;
    let runtime_overrides = CliRuntimeOverrides {
        model: cli.model.clone(),
        api_key: cli.api_key.clone(),
        base_url: cli.base_url.clone(),
        output_mode: cli.output_mode.clone(),
        log_level: cli.log_level.clone(),
        telemetry: cli.telemetry,
        approval_policy: cli.approval_policy.clone(),
        sandbox_mode: cli.sandbox_mode.clone(),
        yolo: Some(cli.yolo),
        verbosity: cli.verbosity.clone(),
    };
    match command {
        Some(Commands::Doctor(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("doctor", args))
        }
        Some(Commands::Runs(_)) => {
            unreachable!("canonical runs command dispatched before ConfigStore")
        }
        Some(Commands::Resume(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("resume", args))
        }
        Some(Commands::Init(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("init", args))
        }
        Some(Commands::Setup(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("setup", args))
        }
        Some(Commands::Exec(args)) => {
            reject_exec_global_flags(&args.args)?;
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_exec_to_tui(&cli, &resolved_runtime, tui_args("exec", args))
        }
        Some(Commands::Fleet(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("fleet", args))
        }
        Some(Commands::LaneLogProxy(_)) => unreachable!("lane log proxy dispatched above"),
        Some(Commands::Lane(args)) => run_lane_command(args),
        Some(Commands::Mcp(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("mcp", args))
        }
        Some(Commands::Features(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("features", args))
        }
        Some(Commands::Completions(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            delegate_to_tui(&cli, &resolved_runtime, tui_args("completions", args))
        }
        Some(Commands::Login(args)) => run_login_command(&mut store, args),
        Some(Commands::Logout) => run_logout_command(&mut store),
        Some(Commands::Auth(args)) => run_auth_command(&mut store, args.command),
        Some(Commands::Config(args)) => run_config_command(&mut store, args.command),
        Some(Commands::Model(args)) => run_model_command(&mut store, args.command),
        Some(Commands::Thread(args)) => run_thread_command(args.command),
        Some(Commands::Sandbox(args)) => run_sandbox_command(args.command),
        Some(Commands::AppServer(args)) => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            run_app_server_command(&resolved_runtime, args)
        }
        Some(Commands::Completion { .. }) => {
            unreachable!("completion command dispatched before ConfigStore")
        }
        None => {
            let resolved_runtime = resolve_runtime_for_dispatch(&mut store, &runtime_overrides)?;
            let forwarded = root_tui_passthrough(&cli)?;
            delegate_to_tui(&cli, &resolved_runtime, forwarded)
        }
    }
}

fn root_tui_passthrough(cli: &Cli) -> Result<Vec<String>> {
    let mut forwarded = Vec::new();
    if cli.continue_session {
        forwarded.push("--continue".to_string());
    }

    let prompt =
        cli.prompt_flag
            .iter()
            .chain(cli.prompt.iter())
            .fold(String::new(), |mut acc, part| {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(part);
                acc
            });
    if !prompt.is_empty() {
        if cli.continue_session {
            bail!("{}", tr(MessageId::CliContinueInteractiveConflict));
        }
        forwarded.push("--prompt".to_string());
        forwarded.push(prompt);
    }

    Ok(forwarded)
}

fn resolve_runtime_for_dispatch(
    store: &mut ConfigStore,
    runtime_overrides: &CliRuntimeOverrides,
) -> Result<ResolvedRuntimeOptions> {
    let runtime_secrets = Secrets::auto_detect();
    resolve_runtime_for_dispatch_with_secrets(store, runtime_overrides, &runtime_secrets)
}

fn resolve_runtime_for_dispatch_with_secrets(
    store: &mut ConfigStore,
    runtime_overrides: &CliRuntimeOverrides,
    secrets: &Secrets,
) -> Result<ResolvedRuntimeOptions> {
    let mut resolved = store
        .config
        .resolve_runtime_options_with_secrets(runtime_overrides, secrets)?;

    if resolved.api_key_source == Some(RuntimeApiKeySource::Keyring)
        && deepseek_config_api_key(store).is_none()
        && let Some(api_key) = resolved.api_key.clone()
    {
        write_deepseek_api_key_to_config(store, &api_key);
        match store.save() {
            Ok(()) => {
                eprintln!(
                    "信息：已从系统凭据存储恢复 DeepSeek API Key，并保存到 {}",
                    store.path().display()
                );
                resolved.api_key_source = Some(RuntimeApiKeySource::ConfigFile);
            }
            Err(err) => {
                eprintln!(
                    "警告：已从系统凭据存储恢复 DeepSeek API Key，但无法保存到 {}：{err}",
                    store.path().display()
                );
            }
        }
    }

    Ok(resolved)
}

fn tui_args(command: &str, args: TuiPassthroughArgs) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.args.len() + 1);
    forwarded.push(command.to_string());
    forwarded.extend(args.args);
    forwarded
}

fn reject_exec_global_flags(args: &[String]) -> Result<()> {
    const GLOBAL_ONLY_FLAGS: &[&str] = &["--model", "--api-key", "--base-url"];

    for arg in args {
        if arg == "--" {
            break;
        }
        let flag = arg.split_once('=').map_or(arg.as_str(), |(flag, _)| flag);
        if GLOBAL_ONLY_FLAGS.contains(&flag) {
            bail!(
                "{}",
                tr(MessageId::CliExecFlagPlacement).replace("{flag}", flag)
            );
        }
    }

    Ok(())
}

fn run_login_command(store: &mut ConfigStore, args: LoginArgs) -> Result<()> {
    run_login_command_with_secrets(store, args, &Secrets::auto_detect())
}

fn run_login_command_with_secrets(
    store: &mut ConfigStore,
    args: LoginArgs,
    secrets: &Secrets,
) -> Result<()> {
    let api_key = match args.api_key {
        Some(v) => v,
        None => read_api_key_from_stdin()?,
    };
    write_deepseek_api_key_to_config(store, &api_key);
    let keyring_saved = secrets.set("deepseek", &api_key).is_ok();
    store.save()?;
    let destination = if keyring_saved {
        format!("{} and {}", store.path().display(), secrets.backend_name())
    } else {
        store.path().display().to_string()
    };
    println!("已保存 DeepSeek API Key：{destination}");
    Ok(())
}

fn run_logout_command(store: &mut ConfigStore) -> Result<()> {
    run_logout_command_with_secrets(store, &Secrets::auto_detect())
}

fn run_logout_command_with_secrets(store: &mut ConfigStore, secrets: &Secrets) -> Result<()> {
    clear_deepseek_api_key_from_config(store);
    let _ = secrets.delete("deepseek");
    store.save()?;
    println!("已删除 DeepSeek 凭据");
    Ok(())
}

#[cfg(test)]
fn no_keyring_secrets() -> Secrets {
    Secrets::new(std::sync::Arc::new(
        codewhale_secrets::InMemoryKeyringStore::new(),
    ))
}

fn write_deepseek_api_key_to_config(store: &mut ConfigStore, api_key: &str) {
    store.config.api_key = Some(api_key.to_string());
    if store.config.default_text_model.is_none() {
        store.config.default_text_model = Some("deepseek-v4-pro".to_string());
    }
}

fn clear_deepseek_api_key_from_config(store: &mut ConfigStore) {
    store.config.api_key = None;
}

fn deepseek_config_api_key(store: &ConfigStore) -> Option<&str> {
    store
        .config
        .api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
}

fn deepseek_keyring_api_key(secrets: &Secrets) -> Option<String> {
    secrets
        .get("deepseek")
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
}

fn deepseek_env_api_key() -> Option<String> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn deepseek_active_source(store: &ConfigStore, secrets: &Secrets) -> &'static str {
    if deepseek_config_api_key(store).is_some() {
        "配置文件"
    } else if deepseek_keyring_api_key(secrets).is_some() {
        "系统凭据存储"
    } else if deepseek_env_api_key().is_some() {
        "环境变量 DEEPSEEK_API_KEY"
    } else {
        "未配置"
    }
}

fn run_auth_command(store: &mut ConfigStore, command: AuthCommand) -> Result<()> {
    run_auth_command_with_secrets(store, command, &Secrets::auto_detect())
}

fn run_auth_command_with_secrets(
    store: &mut ConfigStore,
    command: AuthCommand,
    secrets: &Secrets,
) -> Result<()> {
    match command {
        AuthCommand::Status => {
            println!("Provider：deepseek");
            println!("凭据来源：{}", deepseek_active_source(store, secrets));
            println!("配置文件：{}", store.path().display());
            println!("查找顺序：命令行 -> 配置文件 -> 系统凭据存储 -> DEEPSEEK_API_KEY");
            Ok(())
        }
        AuthCommand::Set {
            api_key,
            api_key_stdin,
        } => {
            let api_key = match (api_key, api_key_stdin) {
                (Some(v), _) => v,
                (None, true) => read_api_key_from_stdin()?,
                (None, false) => prompt_api_key()?,
            };
            write_deepseek_api_key_to_config(store, &api_key);
            let keyring_saved = secrets.set("deepseek", &api_key).is_ok();
            store.save()?;
            if keyring_saved {
                println!(
                    "已将 DeepSeek API Key 保存到 {} 和 {}",
                    store.path().display(),
                    secrets.backend_name()
                );
            } else {
                println!("已将 DeepSeek API Key 保存到 {}", store.path().display());
            }
            Ok(())
        }
        AuthCommand::Get => {
            println!(
                "deepseek：{}（来源：{}）",
                if deepseek_active_source(store, secrets) == "未配置" {
                    "未配置"
                } else {
                    "已配置"
                },
                deepseek_active_source(store, secrets)
            );
            Ok(())
        }
        AuthCommand::Clear => {
            clear_deepseek_api_key_from_config(store);
            let _ = secrets.delete("deepseek");
            store.save()?;
            println!("已从配置文件和系统凭据存储中删除 DeepSeek API Key");
            Ok(())
        }
        AuthCommand::Migrate { dry_run } => run_auth_migrate(store, secrets, dry_run),
    }
}

fn prompt_api_key() -> Result<String> {
    use std::io::{IsTerminal, Write};
    eprint!("请输入 DeepSeek API Key：");
    io::stderr().flush().ok();
    if !io::stdin().is_terminal() {
        // Non-interactive: read directly without prompting twice.
        return read_api_key_from_stdin();
    }
    let mut buf = String::new();
    io::stdin()
        .read_line(&mut buf)
        .context(tr(MessageId::CliReadApiKeyFailed).into_owned())?;
    let key = buf.trim().to_string();
    if key.is_empty() {
        bail!("DeepSeek API Key 不能为空");
    }
    Ok(key)
}

fn run_auth_migrate(store: &mut ConfigStore, secrets: &Secrets, dry_run: bool) -> Result<()> {
    let Some(value) = deepseek_config_api_key(store).map(str::to_owned) else {
        println!("配置文件中没有可迁移的 DeepSeek API Key");
        return Ok(());
    };
    println!("系统凭据存储：{}", secrets.backend_name());
    if dry_run {
        println!("将迁移 DeepSeek API Key，并从配置文件中删除明文");
        return Ok(());
    }
    secrets
        .set("deepseek", &value)
        .context("无法写入系统凭据存储；配置文件未修改")?;
    clear_deepseek_api_key_from_config(store);
    store.save().context("无法更新配置文件")?;
    println!("已迁移 DeepSeek API Key，并从配置文件中删除明文");
    Ok(())
}

fn run_config_command(store: &mut ConfigStore, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Get { key } => {
            if let Some(value) = store.config.get_display_value(&key) {
                println!("{value}");
                return Ok(());
            }
            bail!(
                "{}",
                tr(MessageId::CliConfigKeyNotFound).replace("{key}", &key)
            );
        }
        ConfigCommand::Set { key, value } => {
            store.config.set_value(&key, &value)?;
            store.save()?;
            println!("{}", tr(MessageId::CliConfigSet).replace("{key}", &key));
            Ok(())
        }
        ConfigCommand::Unset { key } => {
            store.config.unset_value(&key)?;
            store.save()?;
            println!("{}", tr(MessageId::CliConfigUnset).replace("{key}", &key));
            Ok(())
        }
        ConfigCommand::List => {
            for (key, value) in store.config.list_values() {
                println!("{key} = {value}");
            }
            Ok(())
        }
        ConfigCommand::Path => {
            println!("{}", store.path().display());
            Ok(())
        }
    }
}

fn run_model_command(store: &mut ConfigStore, command: ModelCommand) -> Result<()> {
    match command {
        ModelCommand::List => {
            println!("deepseek-v4-pro");
            println!("deepseek-v4-flash");
            Ok(())
        }
        ModelCommand::Resolve { model } => {
            let requested = model
                .as_deref()
                .or(store.config.default_text_model.as_deref())
                .unwrap_or("deepseek-v4-pro");
            let resolved = canonical_deepseek_model(requested)?;
            println!("请求模型：{requested}");
            println!("实际模型：{resolved}");
            println!("Provider：deepseek");
            Ok(())
        }
        ModelCommand::Set { model } => {
            let canonical = canonical_deepseek_model(&model)?;
            store.config.default_text_model = Some(canonical.clone());
            store.save()?;
            println!("已将默认 DeepSeek 模型设为 `{canonical}`");
            Ok(())
        }
    }
}

fn run_thread_command(command: ThreadCommand) -> Result<()> {
    let state = StateStore::open(None)?;
    match command {
        ThreadCommand::List { all, limit } => {
            let threads = state.list_threads(ThreadListFilters {
                include_archived: all,
                limit,
            })?;
            for thread in threads {
                println!(
                    "{} | {} | {} | {}",
                    thread.id,
                    thread
                        .name
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_string()),
                    thread.model_provider,
                    thread.cwd.display()
                );
            }
            Ok(())
        }
        ThreadCommand::Read { thread_id } => {
            let thread = state.get_thread(&thread_id)?;
            println!("{}", serde_json::to_string_pretty(&thread)?);
            Ok(())
        }
        ThreadCommand::Resume { thread_id } => {
            let args = vec!["resume".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Fork { thread_id } => {
            let args = vec!["fork".to_string(), thread_id];
            delegate_simple_tui(args)
        }
        ThreadCommand::Archive { thread_id } => {
            state.mark_archived(&thread_id)?;
            println!(
                "{}",
                tr(MessageId::CliThreadArchived).replace("{thread_id}", &thread_id)
            );
            Ok(())
        }
        ThreadCommand::Unarchive { thread_id } => {
            state.mark_unarchived(&thread_id)?;
            println!(
                "{}",
                tr(MessageId::CliThreadUnarchived).replace("{thread_id}", &thread_id)
            );
            Ok(())
        }
        ThreadCommand::SetName { thread_id, name } => {
            let mut thread = state.get_thread(&thread_id)?.with_context(|| {
                tr(MessageId::CliThreadNotFound).replace("{thread_id}", &thread_id)
            })?;
            thread.name = Some(name);
            thread.updated_at = chrono::Utc::now().timestamp();
            state.upsert_thread(&thread)?;
            println!(
                "{}",
                tr(MessageId::CliThreadRenamed).replace("{thread_id}", &thread_id)
            );
            Ok(())
        }
        ThreadCommand::ClearName { thread_id } => {
            let mut thread = state.get_thread(&thread_id)?.with_context(|| {
                tr(MessageId::CliThreadNotFound).replace("{thread_id}", &thread_id)
            })?;
            thread.name = None;
            thread.updated_at = chrono::Utc::now().timestamp();
            state.upsert_thread(&thread)?;
            println!(
                "{}",
                tr(MessageId::CliThreadNameCleared).replace("{thread_id}", &thread_id)
            );
            Ok(())
        }
    }
}

fn run_sandbox_command(command: SandboxCommand) -> Result<()> {
    match command {
        SandboxCommand::Check { command, ask } => {
            let engine = ExecPolicyEngine::new(Vec::new(), vec!["rm -rf".to_string()]);
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let decision = engine.check(ExecPolicyContext {
                command: &command,
                cwd: &cwd.display().to_string(),
                tool: Some("exec_shell"),
                path: None,
                ask_for_approval: ask.into(),
                sandbox_mode: Some("workspace-write"),
            })?;
            println!("{}", serde_json::to_string_pretty(&decision)?);
            Ok(())
        }
    }
}

fn parse_run_list_limit(raw: &str) -> std::result::Result<u32, String> {
    let limit = raw
        .parse::<u32>()
        .map_err(|_| format!("`--limit` 必须是 1 到 {MAX_RUN_LIST_LIMIT} 之间的整数"))?;
    if !(1..=MAX_RUN_LIST_LIMIT).contains(&limit) {
        return Err(format!(
            "`--limit` 必须是 1 到 {MAX_RUN_LIST_LIMIT} 之间的整数"
        ));
    }
    Ok(limit)
}

fn run_runs_command(cli: &Cli, args: RunsArgs) -> Result<()> {
    let workspace = canonical_runs_workspace(cli.workspace.as_deref())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("无法创建 canonical Run 查询运行时")?;
    let application = AgentApplication::production(ProductionApplicationConfig::official())
        .context("无法打开 canonical RunStore")?;
    let response = runtime.block_on(application.execute(RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: "cli-runs".to_owned(),
        command: RunCommand::ListRoots {
            workspace: workspace.clone(),
            limit: MAX_RUN_LIST_LIMIT,
        },
    }));

    let RunCommandResponse {
        schema_version,
        request_id,
        result,
    } = response;
    let (response_workspace, runs) = match result {
        RunCommandResult::Runs { workspace, runs } => (workspace, runs),
        RunCommandResult::Error { error } => {
            bail!(
                "无法列出当前工作区的 Agent 运行（{:?}）：{}",
                error.code,
                error.message
            );
        }
        other => bail!("canonical Run API 返回了意外结果：{other:?}"),
    };
    let runs = runs
        .into_iter()
        .take(args.limit as usize)
        .collect::<Vec<_>>();

    let mut output = io::stdout().lock();
    if args.json {
        let response = RunCommandResponse {
            schema_version,
            request_id,
            result: RunCommandResult::Runs {
                workspace: response_workspace,
                runs,
            },
        };
        serde_json::to_writer_pretty(&mut output, &response)
            .context("无法编码 canonical Run API JSON")?;
        writeln!(output)?;
    } else {
        write_runs_human(&mut output, &response_workspace, &runs)?;
    }
    Ok(())
}

fn canonical_runs_workspace(configured: Option<&Path>) -> Result<String> {
    let workspace = match configured {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("无法读取当前工作区")?,
    };
    let canonical = workspace
        .canonicalize()
        .with_context(|| format!("无法解析工作区路径 {}", workspace.display()))?;
    canonical
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow!("canonical Run API 要求工作区路径是有效 UTF-8"))
}

fn write_runs_human(
    output: &mut dyn Write,
    workspace: &str,
    runs: &[RootRunSummary],
) -> io::Result<()> {
    if runs.is_empty() {
        writeln!(output, "当前工作区还没有 Agent 运行。")?;
        return Ok(());
    }

    writeln!(output, "当前工作区 Agent 运行：{workspace}")?;
    for run in runs {
        let state = if run.terminal {
            "已结束"
        } else {
            "进行中"
        };
        write!(
            output,
            "- {} ｜ {} ｜ 更新时间 {} ｜ 事件序号 {}",
            run.run_id,
            state,
            format_run_timestamp(run.updated_at_unix_ms),
            run.last_sequence
        )?;
        if let Some(source) = run.continued_from_run_id.as_ref() {
            write!(output, " ｜ 延续自 {source}")?;
        }
        writeln!(output)?;
    }
    Ok(())
}

fn format_run_timestamp(unix_ms: u64) -> String {
    i64::try_from(unix_ms)
        .ok()
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| unix_ms.to_string())
}

fn run_app_server_command(
    resolved_runtime: &ResolvedRuntimeOptions,
    args: AppServerArgs,
) -> Result<()> {
    // Match exec and the interactive TUI: install the single context-owned
    // config-home override before AgentApplication composes any system prompt.
    codewhale_context::prompts::load_prompt_overrides_from_config_home();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context(tr(MessageId::CliAppServerRuntimeFailed).into_owned())?;
    let application = Arc::new(AgentApplication::production(
        production_application_config(resolved_runtime, args.transport_max_retries)?,
    )?);
    if args.stdio {
        return runtime
            .block_on(run_app_server_stdio(application))
            .context(tr(MessageId::CliAppServerStdioFailed).into_owned());
    }

    let listen = SocketAddr::new(
        args.host.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        args.port.unwrap_or(7878),
    );
    runtime
        .block_on(run_app_server(
            application,
            AppServerOptions {
                listen,
                auth_token: args.auth_token.or_else(app_server_token_from_env),
                insecure_no_auth: args.insecure_no_auth,
                cors_origins: args.cors_origin,
                max_body_bytes: args.max_body_bytes.unwrap_or(DEFAULT_MAX_BODY_BYTES),
                ..AppServerOptions::default()
            },
        ))
        .with_context(|| {
            tr(MessageId::CliAppServerHttpFailed).replace("{listen}", &listen.to_string())
        })
}

fn production_application_config(
    resolved_runtime: &ResolvedRuntimeOptions,
    transport_max_retries: Option<u32>,
) -> Result<ProductionApplicationConfig> {
    let base_url = resolved_runtime.base_url.trim_end_matches('/');
    if !is_official_deepseek_base_url(base_url) {
        bail!(
            "{}",
            tr(MessageId::CliAppServerOfficialEndpointOnly).replace("{base_url}", base_url)
        );
    }

    let prompt = ProductionPromptConfig {
        preferences: load_prompt_preferences()
            .context(tr(MessageId::CliPromptPreferencesFailed).into_owned())?,
        verbosity: resolved_runtime.verbosity.clone(),
        ..ProductionPromptConfig::default()
    };
    let mut config = ProductionApplicationConfig::official().with_prompt(prompt);
    if let Some(max_retries) = transport_max_retries {
        config = config.with_transport_max_retries(max_retries);
    }
    if let Some(api_key) = resolved_runtime.api_key.clone() {
        config = config.with_api_key(api_key)?;
    }
    Ok(config)
}

fn app_server_token_from_env() -> Option<String> {
    std::env::var("CODEWHALE_APP_SERVER_TOKEN").ok()
}

fn delegate_to_tui(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let mut cmd = build_tui_command(cli, resolved_runtime, passthrough)?;
    let tui = PathBuf::from(cmd.get_program());
    let status = cmd
        .status()
        .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
    exit_with_tui_status(status)
}

/// Replace the Unix dispatcher process with the Headless runtime so an
/// orchestrator-visible PID is the actual Agent owner. Signals, process-group
/// control, and exit status therefore cannot stop at an intermediate parent
/// while the model/tool child continues running.
fn delegate_exec_to_tui(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<()> {
    let mut cmd = build_tui_command(cli, resolved_runtime, passthrough)?;
    let tui = PathBuf::from(cmd.get_program());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let error = cmd.exec();
        Err(anyhow!("{}", tui_spawn_error(&tui, &error)))
    }
    #[cfg(not(unix))]
    {
        // Windows has no exec(2). Keep the existing synchronous delegation;
        // the TUI process owns Ctrl+C handling and the dispatcher propagates
        // its final status. The Windows job-object migration remains isolated
        // to this platform-specific branch.
        let status = cmd
            .status()
            .map_err(|error| anyhow!("{}", tui_spawn_error(&tui, &error)))?;
        exit_with_tui_status(status)
    }
}

fn build_tui_command(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
) -> Result<Command> {
    build_tui_command_with_paths(
        cli,
        resolved_runtime,
        passthrough,
        cli.config.as_deref(),
        cli.workspace.as_deref(),
    )
}

fn build_tui_command_with_paths(
    cli: &Cli,
    resolved_runtime: &ResolvedRuntimeOptions,
    passthrough: Vec<String>,
    config_path: Option<&Path>,
    workspace_path: Option<&Path>,
) -> Result<Command> {
    let tui = locate_sibling_tui_binary()?;
    let mut verbosity = if cli.profile.is_some() {
        cli.verbosity.clone()
    } else {
        resolved_runtime.verbosity.clone()
    };
    if verbosity.is_none() && passthrough.iter().any(|arg| arg == "exec") {
        verbosity = Some("concise".to_string());
    }

    let mut cmd = Command::new(&tui);
    if let Some(config) = config_path {
        cmd.arg("--config").arg(config);
    }
    if let Some(profile) = cli.profile.as_ref() {
        cmd.arg("--profile").arg(profile);
    }
    if let Some(workspace) = workspace_path {
        cmd.arg("--workspace").arg(workspace);
    }
    // Accepted for older scripts, but no longer forwarded: the interactive TUI
    // always owns the alternate screen to avoid host scrollback hijacking.
    let _ = cli.no_alt_screen;
    if cli.mouse_capture {
        cmd.arg("--mouse-capture");
    }
    if cli.no_mouse_capture {
        cmd.arg("--no-mouse-capture");
    }
    if cli.skip_onboarding {
        cmd.arg("--skip-onboarding");
    }
    cmd.args(passthrough);

    let keyring_bridge_api_key = resolved_runtime.api_key.as_ref();
    let keyring_bridge_source = resolved_runtime.api_key_source;

    if matches!(keyring_bridge_source, Some(RuntimeApiKeySource::Keyring))
        && let Some(api_key) = keyring_bridge_api_key
    {
        // TUI routine startup stays prompt-free and does not query the platform
        // keyring. Bridge only the recovered DeepSeek secret.
        cmd.env("DEEPSEEK_API_KEY", api_key);
        cmd.env(
            "DEEPSEEK_API_KEY_SOURCE",
            RuntimeApiKeySource::Keyring.as_env_value(),
        );
    }

    if let Some(model) = cli.model.as_ref() {
        cmd.env("DEEPSEEK_MODEL", model);
    }
    if let Some(output_mode) = cli.output_mode.as_ref() {
        cmd.env("CODEWHALE_OUTPUT_MODE", output_mode);
    }
    if let Some(v) = verbosity.as_ref() {
        cmd.env("CODEWHALE_VERBOSITY", v);
        cmd.env("CODEWHALE_VERBOSITY", v);
    }
    if let Some(log_level) = cli.log_level.as_ref() {
        cmd.env("CODEWHALE_LOG_LEVEL", log_level);
    }
    if let Some(telemetry) = cli.telemetry {
        cmd.env("CODEWHALE_TELEMETRY", telemetry.to_string());
    }
    if let Some(policy) = cli.approval_policy.as_ref() {
        cmd.env("CODEWHALE_APPROVAL_POLICY", policy);
    }
    if let Some(mode) = cli.sandbox_mode.as_ref() {
        cmd.env("CODEWHALE_SANDBOX_MODE", mode);
    }
    if cli.yolo {
        cmd.env("CODEWHALE_YOLO", "true");
    }
    if let Some(api_key) = cli.api_key.as_ref() {
        // Carry the explicit DeepSeek secret through the source-marked slot so
        // the TUI applies the same precedence without persisting it.
        cmd.env("CODEWHALE_CLI_API_KEY", api_key);
        cmd.env("DEEPSEEK_API_KEY", api_key);
        cmd.env("DEEPSEEK_API_KEY_SOURCE", "cli");
    }
    if let Some(base_url) = cli.base_url.as_ref() {
        cmd.env("DEEPSEEK_BASE_URL", base_url);
    }

    Ok(cmd)
}

fn tui_child_exit_code(status: std::process::ExitStatus) -> Option<i32> {
    if let Some(code) = status.code() {
        return Some(code);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        status.signal().map(|signal| 128 + signal)
    }

    #[cfg(not(unix))]
    {
        None
    }
}

fn exit_with_tui_status(status: std::process::ExitStatus) -> Result<()> {
    if let Some(code) = tui_child_exit_code(status) {
        std::process::exit(code);
    }
    bail!("{}", tr(MessageId::CliTuiNoExitCode))
}

fn delegate_simple_tui(args: Vec<String>) -> Result<()> {
    let tui = locate_sibling_tui_binary()?;
    let status = Command::new(&tui)
        .args(args)
        .status()
        .map_err(|err| anyhow!("{}", tui_spawn_error(&tui, &err)))?;
    exit_with_tui_status(status)
}

fn tui_spawn_error(tui: &Path, err: &io::Error) -> String {
    format!(
        "failed to spawn companion TUI binary at {}: {err}\n\
\n\
The `codewhale` dispatcher found a `codewhale-tui` file, but the OS refused \
to execute it. Common fixes:\n\
  - Reinstall both binaries through the same installation channel.\n\
  - On Windows, run `where codewhale` and `where codewhale-tui`; both should \
come from the same install directory.\n\
  - If you downloaded release assets manually, keep both `codewhale` and \
`codewhale-tui` binaries together and make sure the TUI binary is executable.\n\
  - Set CODEWHALE_TUI_BIN to the absolute path of a working `codewhale-tui` \
binary.",
        tui.display()
    )
}

/// Resolve the sibling `codewhale-tui` executable next to the running
/// dispatcher. Honours the platform executable suffix (`.exe` on Windows).
///
/// `CODEWHALE_TUI_BIN` is consulted first as an explicit override for custom
/// installs and CI test layouts.
fn locate_sibling_tui_binary() -> Result<PathBuf> {
    if let Ok(override_path) = std::env::var("CODEWHALE_TUI_BIN") {
        let candidate = PathBuf::from(override_path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        bail!(
            "CODEWHALE_TUI_BIN points at {}, which is not a regular file.",
            candidate.display()
        );
    }

    let current =
        std::env::current_exe().context(tr(MessageId::CliCurrentExecutableFailed).into_owned())?;
    if let Some(found) = sibling_tui_candidate(&current) {
        return Ok(found);
    }

    // Build a stable error path so the user sees the platform-correct
    // expected name, not "codewhale-tui" on Windows.
    let expected = current.with_file_name(format!("codewhale-tui{}", std::env::consts::EXE_SUFFIX));
    bail!(
        "Companion `codewhale-tui` binary not found at {}.\n\
\n\
The `codewhale` dispatcher delegates interactive sessions to a sibling \
`codewhale-tui` binary. Reinstall the checksum-verified CodeWhale package so \
both binaries are activated from the same release directory.\n\
\n\
Or set CODEWHALE_TUI_BIN to the absolute path of an existing `codewhale-tui` binary.",
        expected.display()
    );
}

/// Return the first existing sibling-binary path under any of the names
/// `codewhale-tui` might use on this platform. Pure function to keep
/// `locate_sibling_tui_binary` testable.
fn sibling_tui_candidate(dispatcher: &Path) -> Option<PathBuf> {
    // Primary: platform-correct name. EXE_SUFFIX is "" on Unix and ".exe"
    // on Windows.
    let primary =
        dispatcher.with_file_name(format!("codewhale-tui{}", std::env::consts::EXE_SUFFIX));
    if primary.is_file() {
        return Some(primary);
    }
    // Windows fallback: a user who manually renamed `.exe` away (per the
    // workaround in #247) still launches successfully under the new code.
    if cfg!(windows) {
        let suffixless = dispatcher.with_file_name("codewhale-tui");
        if suffixless.is_file() {
            return Some(suffixless);
        }
    }
    None
}

fn read_api_key_from_stdin() -> Result<String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context(tr(MessageId::CliReadApiKeyFailed).into_owned())?;
    let key = input.trim().to_string();
    if key.is_empty() {
        bail!("{}", tr(MessageId::CliEmptyApiKey));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, OnceLock};

    fn parse_ok(argv: &[&str]) -> Cli {
        Cli::try_parse_from(argv).unwrap_or_else(|err| panic!("解析失败 {argv:?}: {err}"))
    }

    fn help_for(argv: &[&str]) -> String {
        let err = localized_cli_command()
            .try_get_matches_from(argv)
            .expect_err("--help 应终止解析");
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    struct ScopedEnv {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl ScopedEnv {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: every test in this module that mutates process environment
            // holds env_lock for the guard lifetime.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            // SAFETY: caller still holds env_lock while guards are dropped.
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn command_env(cmd: &Command, name: &str) -> Option<String> {
        let name = OsStr::new(name);
        cmd.get_envs().find_map(|(key, value)| {
            (key == name)
                .then(|| value.map(|value| value.to_string_lossy().into_owned()))
                .flatten()
        })
    }

    #[test]
    fn m8a_cli_help_is_deepseek_only() {
        let help = help_for(&["codewhale", "--help"]);
        for retained in ["doctor", "exec", "app-server", "login", "auth", "model"] {
            assert!(help.contains(retained), "{retained}");
        }
        assert!(!help.contains("--provider"));
        assert!(!help.contains("List live provider API models"));
        assert!(!help.contains("xai-device"));

        let auth = help_for(&["codewhale", "auth", "--help"]);
        assert!(auth.contains("status"));
        assert!(auth.contains("set"));
        assert!(auth.contains("get"));
        assert!(auth.contains("clear"));
        assert!(!auth.contains("--provider"));
        assert!(!auth.contains("list"));
    }

    #[test]
    fn m8c_cli_help_uses_fixed_zh_hans_without_changing_command_ids() {
        let help = help_for(&["codewhale", "--help"]);
        assert!(help.contains("面向官方 DeepSeek API 的本地终端编码 Agent"));
        assert!(help.contains("用法："));
        assert!(help.contains("检查本地配置、凭据、运行环境与恢复建议"));
        for stable in ["doctor", "exec", "app-server", "--workspace", "--verbosity"] {
            assert!(
                help.contains(stable),
                "stable CLI identity missing: {stable}"
            );
        }
        for leak in [
            "Run CodeWhale diagnostics",
            "Run a non-interactive prompt",
            "Run the canonical local Run API",
            "Controls transcript and output verbosity",
            "possible values",
        ] {
            assert!(!help.contains(leak), "English product text leaked: {leak}");
        }
        assert!(help.contains("是否启用本地遥测：true 或 false"));

        let app_server = help_for(&["codewhale", "app-server", "--help"]);
        assert!(app_server.contains("HTTP 监听地址"));
        assert!(app_server.contains("通过标准输入/输出运行同一个"));
        assert!(app_server.contains("HTTP 默认要求 --auth-token"));
        for leak in [
            "HTTP bind host",
            "Run the same canonical newline",
            "HTTP requires --auth-token",
        ] {
            assert!(
                !app_server.contains(leak),
                "English app-server help leaked: {leak}"
            );
        }
    }

    #[test]
    fn m8a_foreign_provider_flag_fails_during_parsing() {
        let error = Cli::try_parse_from(["codewhale", "--provider", "openai", "exec", "hi"])
            .expect_err("generic provider flag must be absent");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn m8a_auth_and_model_commands_have_no_provider_selector() {
        assert!(matches!(
            parse_ok(&["codewhale", "auth", "status"]).command,
            Some(Commands::Auth(AuthArgs {
                command: AuthCommand::Status
            }))
        ));
        assert!(
            Cli::try_parse_from([
                "codewhale",
                "auth",
                "set",
                "--provider",
                "openai",
                "--api-key",
                "secret"
            ])
            .is_err()
        );
        assert!(matches!(
            parse_ok(&["codewhale", "model", "resolve", "flash"]).command,
            Some(Commands::Model(ModelArgs {
                command: ModelCommand::Resolve { model: Some(_) }
            }))
        ));
    }

    #[test]
    fn m8a_login_writes_only_deepseek_slots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut store = ConfigStore::load(Some(path.clone())).unwrap();
        run_login_command_with_secrets(
            &mut store,
            LoginArgs {
                api_key: Some("test-deepseek-secret".to_string()),
            },
            &no_keyring_secrets(),
        )
        .unwrap();

        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("test-deepseek-secret"));
        assert!(saved.contains("api_key = \"test-deepseek-secret\""));
        assert!(!saved.contains("[providers"));
        assert!(!saved.contains("openai"));
    }

    #[test]
    fn m8a_foreign_config_fails_before_dispatch() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "provider = \"openai\"\n").unwrap();
        let error = ConfigStore::load(Some(path))
            .expect_err("foreign provider must fail during config admission");
        assert!(error.to_string().contains("配置不兼容"));
    }

    #[test]
    fn m8a_model_resolution_is_deepseek_only() {
        assert_eq!(canonical_deepseek_model("pro").unwrap(), "deepseek-v4-pro");
        assert_eq!(
            canonical_deepseek_model("deepseek-chat").unwrap(),
            "deepseek-v4-flash"
        );
        assert_eq!(
            canonical_deepseek_model("deepseek-future").unwrap(),
            "deepseek-future"
        );
        assert!(canonical_deepseek_model("  ").is_err());
    }

    #[test]
    fn m8a_tui_child_receives_only_deepseek_runtime_identity() {
        let _lock = env_lock();
        let directory = tempfile::tempdir().unwrap();
        let tui = directory
            .path()
            .join(format!("codewhale-tui{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&tui, b"").unwrap();
        let _binary = ScopedEnv::set("CODEWHALE_TUI_BIN", &tui);

        let cli = parse_ok(&[
            "codewhale",
            "--api-key",
            "explicit-secret",
            "--model",
            "deepseek-v4-pro",
            "doctor",
        ]);
        let mut store = ConfigStore::load(Some(directory.path().join("config.toml"))).unwrap();
        let overrides = CliRuntimeOverrides {
            api_key: cli.api_key.clone(),
            model: cli.model.clone(),
            ..CliRuntimeOverrides::default()
        };
        let resolved = resolve_runtime_for_dispatch_with_secrets(
            &mut store,
            &overrides,
            &no_keyring_secrets(),
        )
        .unwrap();
        let command =
            build_tui_command_with_paths(&cli, &resolved, vec!["doctor".into()], None, None)
                .unwrap();

        assert_eq!(
            command_env(&command, "DEEPSEEK_API_KEY").as_deref(),
            Some("explicit-secret")
        );
        assert_eq!(
            command_env(&command, "DEEPSEEK_MODEL").as_deref(),
            Some("deepseek-v4-pro")
        );
        assert!(command_env(&command, "DEEPSEEK_PROVIDER").is_none());
        assert!(command_env(&command, "OPENAI_API_KEY").is_none());
        assert!(command_env(&command, "XAI_API_KEY").is_none());
    }

    #[test]
    fn m8a_app_server_accepts_only_official_deepseek_route() {
        let directory = tempfile::tempdir().unwrap();
        let mut resolved = ConfigStore::load(Some(directory.path().join("config.toml")))
            .unwrap()
            .config
            .resolve_runtime_options(&CliRuntimeOverrides::default())
            .unwrap();
        resolved.base_url = "https://example.com/v1".to_string();
        let error = match production_application_config(&resolved, None) {
            Ok(_) => panic!("foreign endpoint must fail before app construction"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("官方 DeepSeek endpoint"));
    }

    #[test]
    fn canonical_run_limit_remains_stable() {
        assert_eq!(parse_run_list_limit("1").unwrap(), 1);
        assert_eq!(
            parse_run_list_limit(&MAX_RUN_LIST_LIMIT.to_string()).unwrap(),
            MAX_RUN_LIST_LIMIT
        );
        assert!(parse_run_list_limit("0").is_err());
    }

    #[test]
    fn clap_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
