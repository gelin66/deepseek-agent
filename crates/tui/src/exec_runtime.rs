//! Production composition and presentation for `codewhale exec`.
//!
//! This module is deliberately a thin client of `codewhale-runtime`: it
//! resolves the DeepSeek route, constructs the concrete ports, submits one
//! run, and projects canonical stored events. It never owns a model/tool loop,
//! accumulates usage, or decides whether a run completed successfully.

use std::collections::HashMap;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use codewhale_runtime::{
    AgentOutcome, AgentRuntime, ApiSurface, CanonicalTranscript, DurableActionState,
    ModelAccounting, ModelErrorCategory, ModelPort, ModelPortError, ModelRequest, ModelStream,
    PromptCacheControl, ReasoningEffort, RunEnvironment, RunId, RunLimits, RunReplay, RunRequest,
    RunStore, RuntimeEventKind, RuntimeEventSink, RuntimeFailure, RuntimeTimeoutPhase,
    StoredRuntimeEvent, SystemPrompt as RuntimeSystemPrompt, SystemPromptBlock, TerminalState,
    ToolExecutor, ToolPolicy, TranscriptEntry,
};
use codewhale_state::StateStore;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::agent_runtime_adapter::{
    DeepSeekModelPort, ProductionToolExecutor, canonical_system_prompt, model_accounting_snapshot,
};
use crate::client::DeepSeekClient;
use crate::client::request_budget::SharedApiRequestBudget;
use crate::config::{Config, MAX_SUBAGENTS};
use crate::core::termination::RunTerminationReason;
use crate::exec_output::ExecTerminalReceipt;
use crate::prompts::{InstructionSource, PromptSessionContext};
use crate::tools::spec::ToolContext;
use crate::tui::app::AppMode;

use super::{
    EXEC_OUTPUT_CLOSE_TIMEOUT_SECS, EXEC_OUTPUT_QUEUE_CAPACITY, EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS,
    ExecAccountingReceipt, ExecOutputFormat, ExecOutputWait, ExecStreamEvent,
    ExecStreamInputAnalysis, ExecStreamMeta, ExecSurfaceModelUsageBucket,
    commit_exec_terminal_signal, config_for_cli_route, current_binary_sha256,
    exec_sandbox_elevation_authorized, exec_stream_line, exec_supports_provider, recv_exec_signal,
    resolve_cli_auto_route, stop_exec_signal_controller, wait_exec_output_until,
    wait_terminal_output, write_exec_stream_terminal,
};

const RUNTIME_EVENT_CHANNEL_CAPACITY: usize = 256;

fn protocol_label(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn execution_fingerprint_sha256(
    config: &Config,
    model: &str,
    context: &ToolContext,
    tool_catalog_sha256: Option<&str>,
) -> String {
    let provider = config.api_provider();
    let provider_config = config.provider_config_for(provider);
    let mut trusted_external_paths = context
        .trusted_external_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    trusted_external_paths.sort();
    let enabled_features = context
        .features
        .enabled_features()
        .into_iter()
        .map(|feature| feature.key())
        .collect::<Vec<_>>();
    let network = config.network.as_ref().map(|policy| {
        serde_json::json!({
            "default": &policy.default,
            "allow": &policy.allow,
            "deny": &policy.deny,
            "proxy": &policy.proxy,
            "audit": policy.audit,
        })
    });
    let sandbox_backend = match config
        .sandbox_backend
        .as_deref()
        .and_then(crate::sandbox::backend::SandboxKind::parse)
    {
        Some(crate::sandbox::backend::SandboxKind::OpenSandbox) => {
            let endpoint = config
                .sandbox_url
                .as_deref()
                .unwrap_or("http://localhost:8080");
            Some(serde_json::json!({
                "kind": "opensandbox",
                "endpoint_sha256": format!(
                    "sha256:{}",
                    crate::hashing::sha256_hex(endpoint.as_bytes())
                ),
            }))
        }
        Some(crate::sandbox::backend::SandboxKind::None) | None => None,
    };
    let value = serde_json::json!({
        "schema": 2,
        "binary_sha256": current_binary_sha256(),
        "provider": provider.as_str(),
        "model": model,
        "base_url_sha256": format!(
            "sha256:{}",
            crate::hashing::sha256_hex(config.deepseek_base_url().as_bytes())
        ),
        "deepseek_request_plan": {
            "strict_tool_mode": config.strict_tool_mode.unwrap_or(false),
            "path_suffix": provider_config.and_then(|provider| provider.path_suffix.as_deref()),
        },
        "deepseek_stream_decoder": {
            "reasoning_stream_style": provider_config
                .and_then(|provider| provider.reasoning_stream_style.as_deref()),
            "idle_timeout_secs": config.stream_chunk_timeout_secs(),
        },
        "workspace": context.workspace.display().to_string(),
        "auto_approve": context.auto_approve,
        "trust_mode": context.trust_mode,
        "shell_policy": context.shell_policy,
        "sandbox_policy": format!("{:?}", context.sandbox_policy),
        "elevated_sandbox_policy": &context.elevated_sandbox_policy,
        "sandbox_backend": sandbox_backend,
        "follow_symlinks": context.follow_symlinks,
        "trusted_external_paths": trusted_external_paths,
        "enabled_features": enabled_features,
        "network": network,
        "tool_catalog_sha256": tool_catalog_sha256,
    });
    let bytes = serde_json::to_vec(&value).expect("execution fingerprint value is serializable");
    format!("sha256:{}", crate::hashing::sha256_hex(&bytes))
}

#[derive(Clone, Copy)]
enum ExecStartupFailure {
    InvalidArguments,
    ProviderUnsupported,
    RunStore,
    ResumeNotFound,
    ResumeWorkspaceMismatch,
    ResumeProviderMismatch,
    ResumeToolCatalogMismatch,
    ResumeFingerprintMissing,
    ResumeFingerprintMismatch,
    Route,
    RouteTimeout,
    Client,
    ToolContext,
}

impl ExecStartupFailure {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidArguments => "exec_invalid_arguments",
            Self::ProviderUnsupported => "exec_provider_unsupported",
            Self::RunStore => "exec_run_store_failed",
            Self::ResumeNotFound => "exec_run_not_found",
            Self::ResumeWorkspaceMismatch => "exec_resume_workspace_mismatch",
            Self::ResumeProviderMismatch => "exec_resume_provider_mismatch",
            Self::ResumeToolCatalogMismatch => "exec_resume_tool_catalog_mismatch",
            Self::ResumeFingerprintMissing => "exec_resume_fingerprint_missing",
            Self::ResumeFingerprintMismatch => "exec_resume_fingerprint_mismatch",
            Self::Route => "exec_route_failed",
            Self::RouteTimeout => "exec_watchdog_timeout",
            Self::Client => "exec_client_startup_failed",
            Self::ToolContext => "exec_tool_startup_failed",
        }
    }

    fn category(self) -> &'static str {
        match self {
            Self::RouteTimeout => "timeout",
            Self::Route => "route",
            Self::Client | Self::ToolContext => "internal",
            Self::InvalidArguments
            | Self::ProviderUnsupported
            | Self::RunStore
            | Self::ResumeNotFound
            | Self::ResumeWorkspaceMismatch
            | Self::ResumeProviderMismatch
            | Self::ResumeToolCatalogMismatch
            | Self::ResumeFingerprintMissing
            | Self::ResumeFingerprintMismatch => "state",
        }
    }

    fn termination_reason(self) -> RunTerminationReason {
        match self {
            Self::RouteTimeout => RunTerminationReason::Timeout,
            Self::InvalidArguments
            | Self::ProviderUnsupported
            | Self::RunStore
            | Self::ResumeNotFound
            | Self::ResumeWorkspaceMismatch
            | Self::ResumeProviderMismatch
            | Self::ResumeToolCatalogMismatch
            | Self::ResumeFingerprintMissing
            | Self::ResumeFingerprintMismatch
            | Self::Route
            | Self::Client
            | Self::ToolContext => RunTerminationReason::InfrastructureError,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_exec_runtime(
    config: &Config,
    model: &str,
    prompt: &str,
    workspace: PathBuf,
    max_subagents: usize,
    auto_approve: bool,
    allow_sandbox_elevation: bool,
    explicit_sandbox: Option<&str>,
    trust_mode: bool,
    tool_mode: bool,
    json_output: bool,
    resume_run_id: Option<String>,
    output_format: ExecOutputFormat,
    max_turns: u32,
    max_api_requests: Option<NonZeroU32>,
    max_runtime_secs: u64,
    allowed_tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,
    append_system_prompt: Option<String>,
) -> Result<()> {
    let started = Instant::now();
    let deadline_origin = tokio::time::Instant::now();
    // The exact executable hash is part of both the resume fingerprint and
    // terminal receipt. In debug builds the binary is large and SHA-256 is
    // deliberately unoptimized, so warm the process-wide cache on a blocking
    // worker while route resolution performs network I/O. This preserves the
    // absolute wall-clock deadline instead of serializing two independent
    // startup costs on the async runtime thread.
    let _binary_sha256_warmup = tokio::task::spawn_blocking(current_binary_sha256);
    let absolute_deadline_unix_ms =
        unix_ms_now().saturating_add(max_runtime_secs.max(1).saturating_mul(1_000));
    let mut runtime_started = false;
    let mut startup_failure = ExecStartupFailure::InvalidArguments;
    let result: Result<()> = async {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    if max_runtime_secs > super::MAX_EXEC_MAX_RUNTIME_SECS {
        bail!(
            "--max-runtime-secs 不能超过 {} 秒",
            super::MAX_EXEC_MAX_RUNTIME_SECS
        );
    }
    startup_failure = ExecStartupFailure::ProviderUnsupported;
    if !exec_supports_provider(config.api_provider()) {
        bail!(
            "codewhale exec 是 DeepSeek 专用入口；当前 provider={}，请切换为 deepseek",
            config.api_provider().as_str()
        );
    }
    validate_budget_route(config, max_api_requests)?;

    startup_failure = ExecStartupFailure::RunStore;
    let store = Arc::new(StateStore::open(None).context("无法打开 canonical Agent RunStore")?);
    let resume_run_id = resume_run_id.map(RunId::from);
    let resume_replay = if let Some(run_id) = resume_run_id.as_ref() {
        let replay = store.load(run_id).await.map_err(|error| anyhow!(error))?;
        let Some(replay) = replay else {
            startup_failure = ExecStartupFailure::ResumeNotFound;
            bail!("exec_run_not_found：找不到可恢复运行 {run_id}");
        };
        Some(replay)
    } else {
        None
    };
    if let Some(replay) = resume_replay.as_ref() {
        let persisted = &replay.snapshot.request.environment;
        if persisted.workspace != workspace.display().to_string() {
            startup_failure = ExecStartupFailure::ResumeWorkspaceMismatch;
            bail!(
                "exec_resume_workspace_mismatch：运行绑定工作区 '{}'，当前工作区为 '{}'",
                persisted.workspace,
                workspace.display()
            );
        }
        if persisted.provider != config.api_provider().as_str() {
            startup_failure = ExecStartupFailure::ResumeProviderMismatch;
            bail!(
                "exec_resume_provider_mismatch：运行绑定 provider={}，当前 provider={}",
                persisted.provider,
                config.api_provider().as_str()
            );
        }
    }

    // The route classifier and every root/child request share this one
    // physical admission and billing owner.
    let (api_request_budget, resume_budget_exhausted) = if let Some(replay) = resume_replay.as_ref() {
        let physical_started = u32::try_from(replay.snapshot.accounting.total_started())
            .unwrap_or(u32::MAX);
        let remaining = replay
            .snapshot
            .request
            .limits
            .max_model_requests
            .saturating_sub(physical_started);
        (
            NonZeroU32::new(remaining).map_or_else(
                SharedApiRequestBudget::tracking_only,
                SharedApiRequestBudget::new,
            ),
            remaining == 0,
        )
    } else {
        (
            max_api_requests.map_or_else(
                SharedApiRequestBudget::tracking_only,
                SharedApiRequestBudget::new,
            ),
            false,
        )
    };
    let deadline = deadline_origin + Duration::from_secs(max_runtime_secs.max(1));
    let (mut signal_rx, signal_task, signal_phase) = super::spawn_exec_signal_controller();
    let mut signal_task = Some(signal_task);

    startup_failure = ExecStartupFailure::Route;
    let route = if let Some(replay) = resume_replay.as_ref() {
        super::CliAutoRoute {
            provider: config.api_provider(),
            model: replay.snapshot.request.model.clone(),
            reasoning_effort: None,
            auto_model: false,
        }
    } else {
        tokio::select! {
            biased;
            signal = recv_exec_signal(&mut signal_rx) => {
                stop_exec_signal_controller(&mut signal_task).await;
                if let Some(exit_code) = signal {
                    std::process::exit(exit_code);
                }
                bail!("Headless 信号控制器在 DeepSeek 路由阶段意外退出");
            }
            route = tokio::time::timeout_at(
                deadline,
                resolve_cli_auto_route(config, model, prompt, Some(&api_request_budget)),
            ) => match route {
                Ok(Ok(route)) => route,
                Ok(Err(error)) => {
                    stop_exec_signal_controller(&mut signal_task).await;
                    return Err(error).context("DeepSeek 路由解析失败");
                }
                Err(_) => {
                    startup_failure = ExecStartupFailure::RouteTimeout;
                    stop_exec_signal_controller(&mut signal_task).await;
                    bail!("exec_watchdog_timeout: DeepSeek 路由解析超过最大运行时间");
                }
            }
        }
    };
    startup_failure = ExecStartupFailure::ProviderUnsupported;
    if !exec_supports_provider(route.provider) {
        stop_exec_signal_controller(&mut signal_task).await;
        bail!(
            "Headless 自动路由选择了非 DeepSeek provider={}；codewhale exec 只允许 deepseek",
            route.provider.as_str()
        );
    }

    let execution_config = config_for_cli_route(config, &route);
    if let Err(error) = validate_budget_route(&execution_config, max_api_requests) {
        stop_exec_signal_controller(&mut signal_task).await;
        return Err(error);
    }
    let effective_model = route.model.clone();
    let route_source = if resume_replay.is_some() {
        "run_store_resume"
    } else if route.auto_model {
        "auto_resolver"
    } else {
        "explicit_or_configured"
    };
    let settings = crate::settings::Settings::load().unwrap_or_default();
    let (
        tool_policy,
        limits,
        system_prompt,
        reasoning_effort,
        effective_prompt,
        effective_deadline_unix_ms,
        effective_auto_approve,
        effective_trust_mode,
        effective_allow_sandbox_elevation,
        effective_sandbox,
    ) = if let Some(replay) = resume_replay.as_ref() {
        let request = &replay.snapshot.request;
        let environment = &request.environment;
        (
            request.tool_policy.clone(),
            request.limits,
            request.system_prompt.clone(),
            request.reasoning_effort,
            request.input.clone(),
            request.deadline_unix_ms,
            environment.auto_approve,
            environment.trust_mode,
            environment.allow_sandbox_elevation,
            environment.sandbox.clone(),
        )
    } else {
        let tool_policy = runtime_tool_policy(tool_mode, allowed_tools, disallowed_tools);
        let remaining_runtime_ms = absolute_deadline_unix_ms.saturating_sub(unix_ms_now());
        if remaining_runtime_ms == 0 {
            startup_failure = ExecStartupFailure::RouteTimeout;
            stop_exec_signal_controller(&mut signal_task).await;
            bail!("exec_watchdog_timeout: DeepSeek 路由已耗尽全局运行时间");
        }
        let limits = runtime_limits(
            &execution_config,
            route.provider,
            max_subagents,
            max_turns,
            max_api_requests,
            remaining_runtime_ms,
        );
        let system_prompt = runtime_system_prompt(
            &execution_config,
            &effective_model,
            &workspace,
            &settings,
            append_system_prompt,
            tool_mode,
        );
        let reasoning_effort = route
            .reasoning_effort
            .and_then(|effort| super::cli_reasoning_effort_value(&execution_config, effort))
            .as_deref()
            .map(runtime_reasoning_effort)
            .unwrap_or_default();
        (
            tool_policy,
            limits,
            system_prompt,
            reasoning_effort,
            prompt.to_owned(),
            Some(absolute_deadline_unix_ms),
            auto_approve,
            trust_mode,
            allow_sandbox_elevation,
            explicit_sandbox.map(str::to_owned),
        )
    };

    startup_failure = ExecStartupFailure::ToolContext;
    let tool_context = match production_tool_context(
        &execution_config,
        &workspace,
        &settings,
        effective_auto_approve,
        effective_trust_mode,
        effective_allow_sandbox_elevation,
        effective_sandbox.as_deref(),
    ) {
        Ok(context) => context,
        Err(error) => {
            stop_exec_signal_controller(&mut signal_task).await;
            return Err(error);
        }
    };
    let fingerprint_context = tool_context.clone();
    let accounting_baseline = if resume_replay.is_none() {
        model_accounting_snapshot(&api_request_budget)
    } else {
        Default::default()
    };
    let model_port: Arc<dyn ModelPort> = if resume_budget_exhausted
        || resume_replay
            .as_ref()
            .is_some_and(|replay| !resume_needs_live_model(replay))
    {
        Arc::new(ReplayOnlyModelPort)
    } else {
        startup_failure = ExecStartupFailure::Client;
        let client = match DeepSeekClient::new(&execution_config) {
            Ok(client) => client,
            Err(error) => {
                stop_exec_signal_controller(&mut signal_task).await;
                return Err(error);
            }
        };
        Arc::new(DeepSeekModelPort::new(client, api_request_budget))
    };
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(ProductionToolExecutor::new(tool_context));
    let (event_tx, mut event_rx) = mpsc::channel(RUNTIME_EVENT_CHANNEL_CAPACITY);
    let sink = Arc::new(ChannelEventSink { sender: event_tx });
    let runtime = Arc::new(AgentRuntime::new(
        model_port,
        tool_executor,
        sink,
        store.clone(),
    ));
    let tool_catalog = runtime.tool_definitions(&tool_policy, 0, limits.max_depth);
    let tool_catalog_sha256 = serde_json::to_vec(&tool_catalog)
        .ok()
        .map(|bytes| format!("sha256:{}", crate::hashing::sha256_hex(&bytes)));
    let execution_fingerprint_sha256 = execution_fingerprint_sha256(
        &execution_config,
        &effective_model,
        &fingerprint_context,
        tool_catalog_sha256.as_deref(),
    );

    if let Some(replay) = resume_replay.as_ref()
        && replay.snapshot.request.environment.tool_catalog_sha256 != tool_catalog_sha256
    {
        startup_failure = ExecStartupFailure::ResumeToolCatalogMismatch;
        stop_exec_signal_controller(&mut signal_task).await;
        bail!(
            "exec_resume_tool_catalog_mismatch：当前二进制的模型可见工具目录与持久运行不一致"
        );
    }
    if let Some(replay) = resume_replay.as_ref() {
        match replay
            .snapshot
            .request
            .environment
            .execution_fingerprint_sha256
            .as_deref()
        {
            None => {
                startup_failure = ExecStartupFailure::ResumeFingerprintMissing;
                stop_exec_signal_controller(&mut signal_task).await;
                bail!("exec_resume_fingerprint_missing：持久运行缺少执行环境指纹");
            }
            Some(persisted) if persisted != execution_fingerprint_sha256 => {
                startup_failure = ExecStartupFailure::ResumeFingerprintMismatch;
                stop_exec_signal_controller(&mut signal_task).await;
                bail!("exec_resume_fingerprint_mismatch：当前执行环境与持久运行不一致");
            }
            Some(_) => {}
        }
    }

    let run = if let Some(run_id) = resume_run_id {
        runtime.resume(run_id)
    } else {
        runtime.start(RunRequest {
            run_id: None,
            parent_run_id: None,
            model: effective_model.clone(),
            input: effective_prompt.clone(),
            system_prompt,
            transcript: CanonicalTranscript::default(),
            reasoning_effort,
            max_output_tokens: crate::models::max_output_tokens_for_model(&effective_model),
            streaming: tool_mode || output_format == ExecOutputFormat::StreamJson,
            actor: codewhale_runtime::AgentActor::default(),
            deadline_unix_ms: effective_deadline_unix_ms,
            tool_policy,
            limits,
            environment: RunEnvironment {
                workspace: workspace.display().to_string(),
                provider: route.provider.as_str().to_owned(),
                tool_catalog_sha256: tool_catalog_sha256.clone(),
                execution_fingerprint_sha256: Some(execution_fingerprint_sha256),
                auto_approve: effective_auto_approve,
                trust_mode: effective_trust_mode,
                allow_sandbox_elevation: effective_allow_sandbox_elevation,
                sandbox: effective_sandbox.clone(),
            },
            accounting_baseline,
        })
    };
    runtime_started = true;
    let root_run_id = run.run_id.clone();
    let control = run.control();
    let mut runtime_wait = Box::pin(run.wait());
    drop(runtime);

    let output = crate::exec_output::ExecOutput::new(
        NonZeroUsize::new(EXEC_OUTPUT_QUEUE_CAPACITY).expect("output queue capacity is non-zero"),
    );
    let mut summary = ExecSummary {
        mode: "runtime".to_owned(),
        model: effective_model.clone(),
        prompt: effective_prompt.clone(),
        ..ExecSummary::default()
    };
    let mut transcript = CanonicalTranscript::default();
    let mut tool_starts: HashMap<String, ToolStart> = HashMap::new();
    let mut terminal: Option<AgentOutcome> = None;
    let mut canonical_terminal = true;
    let mut runtime_joined = false;
    let mut drain_events = false;
    let mut drain_deadline = None;
    let mut output_failure = None;
    let mut signal_exit_code = None;

    while terminal.is_none() {
        enum Next {
            Event(Option<StoredRuntimeEvent>),
            Signal(Option<i32>),
            Runtime(Result<AgentOutcome, codewhale_runtime::RuntimeJoinError>),
            DrainTimeout,
        }

        let next = if drain_events {
            let settle_deadline = *drain_deadline.get_or_insert_with(|| {
                tokio::time::Instant::now() + Duration::from_secs(EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS)
            });
            tokio::select! {
                event = tokio::time::timeout_at(settle_deadline, event_rx.recv()) => match event {
                    Ok(event) => Next::Event(event),
                    Err(_) => Next::DrainTimeout,
                },
                result = &mut runtime_wait, if !runtime_joined => Next::Runtime(result),
            }
        } else {
            tokio::select! {
                biased;
                signal = recv_exec_signal(&mut signal_rx) => Next::Signal(signal),
                event = event_rx.recv() => Next::Event(event),
                result = &mut runtime_wait, if !runtime_joined => Next::Runtime(result),
            }
        };

        match next {
            Next::Signal(Some(exit_code)) => {
                signal_exit_code = Some(exit_code);
                let _ = control.cancel();
                drain_events = true;
            }
            Next::Signal(None) => {
                output_failure
                    .get_or_insert_with(|| "Headless 信号控制器在运行期间意外退出".to_owned());
                let _ = control.cancel();
                drain_events = true;
            }
            Next::Runtime(result) => {
                runtime_joined = true;
                match result {
                    Ok(outcome)
                        if matches!(
                            &outcome.terminal,
                            TerminalState::Failed {
                                failure: RuntimeFailure::Store { .. }
                            }
                        ) =>
                    {
                        canonical_terminal = false;
                        terminal = Some(outcome);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        output_failure.get_or_insert_with(|| error.to_string());
                    }
                }
            }
            Next::DrainTimeout => {
                std::process::exit(1);
            }
            // A RunStore acquisition failure has no durable event to publish,
            // so its sink closes just before the Runtime join produces the
            // typed fallback outcome. Resolve that join below instead of
            // racing the closed channel into an untyped process error.
            Next::Event(None) => break,
            Next::Event(Some(event)) => {
                if event.run_id != root_run_id {
                    continue;
                }
                if matches!(&event.event, RuntimeEventKind::Terminal { .. }) {
                    // Claim the terminal outcome before projecting any
                    // terminal-specific stdout. If a signal already won the
                    // CAS, suppress the runtime terminal receipt and preserve
                    // the signal exit; if Runtime wins, later signals cannot
                    // turn a published success into a non-zero signal exit.
                    let committed_signal = commit_exec_terminal_signal(&signal_phase);
                    signal_exit_code = signal_exit_code.or(committed_signal);
                    drain_events |= committed_signal.is_some();
                }
                let wait = RuntimeEventProjection {
                    summary: &mut summary,
                    transcript: &mut transcript,
                    tool_starts: &mut tool_starts,
                    output: &output,
                    format: output_format,
                    json_output,
                    render: !drain_events,
                }
                .project(&event)
                .await;
                if let Some(wait) = wait {
                    match wait_exec_output_until(wait, deadline, &mut signal_rx).await {
                        ExecOutputWait::Written => {}
                        ExecOutputWait::WatchdogTimeout => {
                            // Stop waiting on the blocked writer and drain the
                            // canonical sink so Runtime can publish its own
                            // typed timeout terminal.
                            drain_events = true;
                        }
                        ExecOutputWait::Signal(Some(exit_code)) => {
                            signal_exit_code = Some(exit_code);
                            let _ = control.cancel();
                            drain_events = true;
                        }
                        ExecOutputWait::Signal(None) => {
                            output_failure.get_or_insert_with(|| {
                                "Headless 信号控制器在输出期间意外退出".to_owned()
                            });
                            let _ = control.cancel();
                            drain_events = true;
                        }
                        ExecOutputWait::Failed(error) => {
                            output_failure.get_or_insert(error);
                            let _ = control.cancel();
                            drain_events = true;
                        }
                    }
                }
                if let RuntimeEventKind::Terminal { outcome } = event.event {
                    terminal = Some(*outcome);
                }
            }
        }
    }

    if !runtime_joined {
        match tokio::time::timeout(
            Duration::from_secs(EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS),
            &mut runtime_wait,
        )
        .await
        {
            Ok(Ok(outcome)) => {
                runtime_joined = true;
                if matches!(
                    &outcome.terminal,
                    TerminalState::Failed {
                        failure: RuntimeFailure::Store { .. }
                    }
                ) {
                    canonical_terminal = false;
                    terminal = Some(outcome);
                }
            }
            Ok(Err(error)) => {
                output_failure.get_or_insert_with(|| error.to_string());
            }
            Err(_) => std::process::exit(1),
        };
    }
    let Some(outcome) = terminal else {
        let report = output
            .close_and_join(Duration::from_secs(EXEC_OUTPUT_CLOSE_TIMEOUT_SECS))
            .await;
        stop_exec_signal_controller(&mut signal_task).await;
        if report.unjoined {
            std::process::exit(1);
        }
        bail!(
            "exec runtime ended without a canonical terminal event{}",
            output_failure
                .as_deref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default()
        );
    };
    let terminal_projection = project_terminal(&outcome.terminal);
    summary.terminal = Some(terminal_projection.receipt);
    summary.accounting = accounting_receipt(&outcome.accounting);
    summary.error = terminal_projection.error.clone();
    summary.error_category = terminal_projection
        .error
        .as_ref()
        .map(|_| terminal_projection.category.to_owned());

    if !drain_events {
        emit_terminal_output(
            &output,
            &summary,
            &transcript,
            &outcome,
            &terminal_projection,
            TerminalMetadata {
                receipt_kind: if canonical_terminal {
                    "terminal"
                } else {
                    "runtime_failure"
                },
                provider: route.provider.as_str(),
                model: &effective_model,
                route_source,
                started,
                approval_posture: if effective_auto_approve { "auto_tools" } else { "ask" },
                sandbox_posture: effective_sandbox.as_deref().unwrap_or("configured_default"),
                prompt: &effective_prompt,
                tool_catalog_sha256,
                workspace: &workspace,
                run_id: &root_run_id,
            },
            output_format,
            json_output,
        )
        .await
        .unwrap_or_else(|error| {
            output_failure.get_or_insert(error);
        });
    }

    if json_output && output_format != ExecOutputFormat::StreamJson && !drain_events {
        let mut bytes = serde_json::to_vec_pretty(&summary)?;
        bytes.push(b'\n');
        if let Err(error) = wait_terminal_output(output.enqueue_stdout(bytes)).await {
            output_failure.get_or_insert(error);
        }
    }

    let report = output
        .close_and_join(Duration::from_secs(EXEC_OUTPUT_CLOSE_TIMEOUT_SECS))
        .await;
    stop_exec_signal_controller(&mut signal_task).await;
    if let Some(exit_code) = signal_exit_code {
        std::process::exit(exit_code);
    }
    if !runtime_joined || report.unjoined {
        std::process::exit(1);
    }
    if let Some(error) = report
        .write_error
        .map(|error| error.to_string())
        .or(report.join_error)
    {
        output_failure.get_or_insert(error);
    }
    if let Some(error) = output_failure {
        bail!("exec output failed: {error}");
    }
    if let Some(error) = terminal_projection.error {
        bail!("exec runtime failed: {error}");
    }
    Ok(())
    }
    .await;

    if let Err(error) = &result
        && !runtime_started
        && output_format == ExecOutputFormat::StreamJson
        && let Err(output_error) = emit_startup_stream_failure(
            config,
            model,
            prompt,
            &workspace,
            started,
            auto_approve,
            explicit_sandbox,
            startup_failure,
            &error.to_string(),
        )
        .await
    {
        return Err(anyhow!("{error}; startup output failed: {output_error}"));
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn emit_startup_stream_failure(
    config: &Config,
    model: &str,
    prompt: &str,
    workspace: &Path,
    started: Instant,
    auto_approve: bool,
    explicit_sandbox: Option<&str>,
    failure: ExecStartupFailure,
    message: &str,
) -> std::result::Result<(), String> {
    let output = crate::exec_output::ExecOutput::new(
        NonZeroUsize::new(EXEC_OUTPUT_QUEUE_CAPACITY).expect("output queue capacity is non-zero"),
    );
    let terminal = ExecTerminalReceipt::from_reason(failure.termination_reason());
    let meta = ExecStreamMeta {
        receipt_kind: "startup_failure",
        provider: config.api_provider().as_str().to_owned(),
        model: model.to_owned(),
        route_source: "startup".to_owned(),
        accounting: ExecAccountingReceipt::default(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        approval_posture: if auto_approve { "auto_tools" } else { "ask" }.to_owned(),
        sandbox_posture: explicit_sandbox.unwrap_or("configured_default").to_owned(),
        binary_sha256: current_binary_sha256(),
        config_sha256: None,
        prompt_sha256: format!("sha256:{}", crate::hashing::sha256_hex(prompt.as_bytes())),
        tool_catalog_sha256: None,
        input_analysis: ExecStreamInputAnalysis::default(),
        visible_final_answer_chars: 0,
        run_id: String::new(),
        resume_command: String::new(),
        workspace: workspace.display().to_string(),
        message_count: 0,
        terminal,
        error_category: Some(failure.category().to_owned()),
    };
    let writes = async {
        write_exec_stream_terminal(
            &output,
            &ExecStreamEvent::Metadata {
                meta: Box::new(meta),
            },
        )
        .await?;
        write_exec_stream_terminal(
            &output,
            &ExecStreamEvent::Error {
                error: message.to_owned(),
                code: failure.code().to_owned(),
                category: failure.category().to_owned(),
                recoverable: false,
                termination_reason: failure.termination_reason(),
            },
        )
        .await?;
        write_exec_stream_terminal(&output, &ExecStreamEvent::Done).await
    }
    .await;
    let report = output
        .close_and_join(Duration::from_secs(EXEC_OUTPUT_CLOSE_TIMEOUT_SECS))
        .await;
    if report.unjoined {
        return Err("startup output writer did not settle".to_owned());
    }
    if let Some(error) = report
        .write_error
        .map(|error| error.to_string())
        .or(report.join_error)
    {
        return Err(error);
    }
    writes.map(|_| ())
}

fn validate_budget_route(config: &Config, limit: Option<NonZeroU32>) -> Result<()> {
    if limit.is_none() {
        return Ok(());
    }
    let provider = config.api_provider();
    let base_url = config.deepseek_base_url();
    let path_suffix = config
        .provider_config_for(provider)
        .and_then(|provider| provider.path_suffix.as_deref());
    if !crate::client::deepseek::owns_route(provider, &base_url, path_suffix) {
        bail!(
            "--max-api-requests 当前只支持 DeepSeek 官方路由；请使用 deepseek、官方 API 地址且不要配置 path_suffix"
        );
    }
    Ok(())
}

fn runtime_tool_policy(
    enabled: bool,
    allowed: Option<Vec<String>>,
    denied: Option<Vec<String>>,
) -> ToolPolicy {
    let mut denied = denied.unwrap_or_default();
    if !denied.iter().any(|name| name == "workflow") {
        denied.push("workflow".to_owned());
    }
    denied.sort();
    denied.dedup();
    ToolPolicy {
        enabled,
        allowed,
        denied,
    }
}

fn runtime_limits(
    config: &Config,
    provider: crate::config::ApiProvider,
    requested_subagents: usize,
    max_turns: u32,
    hard_requests: Option<NonZeroU32>,
    remaining_runtime_ms: u64,
) -> RunLimits {
    let subagents_enabled = config.subagents_enabled_for_provider(provider);
    let max_subagents = if subagents_enabled {
        requested_subagents
            .min(config.max_subagents_for_provider(provider))
            .clamp(1, MAX_SUBAGENTS)
    } else {
        0
    };
    let tree_width = u32::try_from(max_subagents)
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    RunLimits {
        max_turns,
        max_model_requests: hard_requests
            .map(NonZeroU32::get)
            .unwrap_or_else(|| max_turns.saturating_mul(tree_width).max(max_turns)),
        max_model_retries: 2,
        max_tool_calls: max_turns.saturating_mul(tree_width).saturating_mul(4),
        max_depth: if subagents_enabled {
            u8::try_from(config.subagent_max_spawn_depth_for_provider(provider)).unwrap_or(u8::MAX)
        } else {
            0
        },
        max_concurrent_children: u32::try_from(max_subagents).unwrap_or(u32::MAX),
        model_event_idle_ms: Some(config.stream_chunk_timeout_secs().saturating_mul(1_000)),
        wall_time_ms: Some(remaining_runtime_ms.max(1)),
    }
}

fn runtime_system_prompt(
    config: &Config,
    model: &str,
    workspace: &Path,
    settings: &crate::settings::Settings,
    append_system_prompt: Option<String>,
    tool_mode: bool,
) -> RuntimeSystemPrompt {
    let locale = crate::localization::resolve_locale(&settings.locale);
    let mut instructions = config
        .instructions_paths()
        .into_iter()
        .map(Into::into)
        .collect::<Vec<InstructionSource>>();
    if let Some(content) = append_system_prompt {
        instructions.push(InstructionSource::Inline {
            name: "cli:append-system-prompt".to_owned(),
            content,
        });
    }
    let prompt = crate::prompts::system_prompt_for_mode_with_context_skills_and_session(
        workspace,
        None,
        Some(&config.skills_dir()),
        Some(&instructions),
        PromptSessionContext {
            user_memory_block: None,
            goal_objective: None,
            project_context_pack_enabled: config.project_context_pack_enabled(),
            locale_tag: locale.tag(),
            translation_enabled: false,
            model_id: model,
            context_window_override: None,
            show_thinking: settings.show_thinking,
            verbosity: config.verbosity.as_deref(),
            skills_scan_codewhale_only: config.skills_config().scan_codewhale_only(),
        },
    );
    let mut runtime = canonical_system_prompt(&prompt);
    runtime.blocks.push(SystemPromptBlock {
        text: if tool_mode {
            "你正在唯一 AgentRuntime 中执行编码任务。只使用本次请求实际提供的工具；先读取再修改，修改后运行最相关验证。`agent` 会启动同一 Runtime 的后台子 Agent，运行时会自动等待并把结构化结果回注；不要轮询或调用不存在的等待工具。".to_owned()
        } else {
            "本次是无工具执行。直接给出准确、简洁、可操作的最终答案，不要声称执行了文件或命令操作。".to_owned()
        },
        cache_control: PromptCacheControl::Volatile,
    });
    runtime
}

fn runtime_reasoning_effort(value: &str) -> ReasoningEffort {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" => ReasoningEffort::Off,
        "low" => ReasoningEffort::Low,
        "medium" => ReasoningEffort::Medium,
        "high" => ReasoningEffort::High,
        "max" => ReasoningEffort::Max,
        _ => ReasoningEffort::Auto,
    }
}

#[allow(clippy::too_many_arguments)]
fn production_tool_context(
    config: &Config,
    workspace: &Path,
    settings: &crate::settings::Settings,
    auto_approve: bool,
    trust_mode: bool,
    allow_sandbox_elevation: bool,
    explicit_sandbox: Option<&str>,
) -> Result<ToolContext> {
    let mode = if auto_approve {
        AppMode::Yolo
    } else {
        AppMode::Agent
    };
    let shell_policy =
        crate::core::authority::shell_policy_for_mode(mode, auto_approve || config.allow_shell());
    let trusted = crate::workspace_trust::WorkspaceTrust::load_for(workspace);
    let mut context = ToolContext::with_auto_approve(
        workspace.to_path_buf(),
        trust_mode,
        config.notes_path(),
        config.mcp_config_path(),
        auto_approve,
    )
    .with_features(config.features())
    .with_shell_policy(shell_policy)
    .with_trusted_external_paths(trusted.paths().to_vec())
    .with_follow_symlinks(settings.workspace_follow_symlinks)
    .with_elevated_sandbox_policy(
        if exec_sandbox_elevation_authorized(allow_sandbox_elevation, explicit_sandbox) {
            crate::sandbox::SandboxPolicy::DangerFullAccess
        } else {
            effective_sandbox_policy(config, mode, workspace)
        },
    );
    if let Some(network) = config.network.clone() {
        context = context.with_network_policy(
            crate::network_policy::NetworkPolicyDecider::with_default_audit(network.into_runtime()),
        );
    }
    if let Some(backend) = crate::sandbox::backend::create_backend(config)? {
        context = context.with_sandbox_backend(Arc::from(backend));
    }
    context.search_provider = config.search_provider();
    context.search_api_key = config
        .search
        .as_ref()
        .and_then(|search| search.api_key.clone());
    context.search_base_url = config
        .search
        .as_ref()
        .and_then(|search| search.base_url.clone());
    Ok(context)
}

fn effective_sandbox_policy(
    config: &Config,
    mode: AppMode,
    workspace: &Path,
) -> crate::sandbox::SandboxPolicy {
    match config.sandbox_mode.as_deref() {
        Some("read-only") => crate::sandbox::SandboxPolicy::ReadOnly,
        Some("workspace-write") => crate::sandbox::SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![workspace.to_path_buf()],
            network_access: true,
            exclude_tmpdir: false,
            exclude_slash_tmp: false,
        },
        Some("danger-full-access") => crate::sandbox::SandboxPolicy::DangerFullAccess,
        Some("external-sandbox") => crate::sandbox::SandboxPolicy::ExternalSandbox {
            network_access: true,
        },
        _ => crate::core::authority::sandbox_policy_for_mode(mode, workspace),
    }
}

#[derive(Clone)]
struct ChannelEventSink {
    sender: mpsc::Sender<StoredRuntimeEvent>,
}

#[async_trait]
impl RuntimeEventSink for ChannelEventSink {
    async fn emit(&self, event: StoredRuntimeEvent) {
        let _ = self.sender.send(event).await;
    }
}

/// Terminal replay and fail-closed recovery are state-store operations. They
/// must remain inspectable after a credential is removed or rotated, and the
/// runtime conformance contract guarantees that neither path opens a model
/// request.
struct ReplayOnlyModelPort;

#[async_trait]
impl ModelPort for ReplayOnlyModelPort {
    async fn stream(&self, _request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        Err(ModelPortError::new(
            "resume_model_access_forbidden",
            ModelErrorCategory::Protocol,
            "纯恢复路径不得发起 DeepSeek 请求",
            false,
        ))
    }

    async fn accounting_snapshot(&self, _seal: bool) -> Result<ModelAccounting, ModelPortError> {
        Ok(ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        })
    }
}

fn resume_needs_live_model(replay: &RunReplay) -> bool {
    if replay.snapshot.terminal.is_some()
        || replay.snapshot.last_model_failure.is_some()
        || !replay.snapshot.pending_children.is_empty()
    {
        return false;
    }
    if replay
        .snapshot
        .pending_model
        .as_ref()
        .is_some_and(|pending| pending.state == DurableActionState::InFlight)
        || replay
            .snapshot
            .pending_tool
            .as_ref()
            .is_some_and(|pending| pending.state == DurableActionState::InFlight)
    {
        return false;
    }
    true
}

#[derive(Default, Serialize)]
struct ExecSummary {
    mode: String,
    model: String,
    prompt: String,
    output: String,
    tools: Vec<ExecToolEntry>,
    outcomes: Vec<ExecOutcome>,
    #[serde(flatten)]
    terminal: Option<ExecTerminalReceipt>,
    error_category: Option<String>,
    error: Option<String>,
    #[serde(flatten)]
    accounting: ExecAccountingReceipt,
}

#[derive(Serialize)]
struct ExecToolEntry {
    name: String,
    success: bool,
    output: String,
}

#[derive(Serialize)]
struct ExecOutcome {
    kind: String,
    outcome: String,
    tool_name: String,
    reason: String,
}

struct ToolStart {
    occurred_at_unix_ms: u64,
    started_at: String,
}

struct RuntimeEventProjection<'a> {
    summary: &'a mut ExecSummary,
    transcript: &'a mut CanonicalTranscript,
    tool_starts: &'a mut HashMap<String, ToolStart>,
    output: &'a crate::exec_output::ExecOutput,
    format: ExecOutputFormat,
    json_output: bool,
    render: bool,
}

impl<'a> RuntimeEventProjection<'a> {
    async fn project(
        self,
        event: &StoredRuntimeEvent,
    ) -> Option<
        impl std::future::Future<Output = Result<(), crate::exec_output::ExecOutputError>> + 'a,
    > {
        let Self {
            summary,
            transcript,
            tool_starts,
            output,
            format,
            json_output,
            render,
        } = self;
        let bytes = match &event.event {
            RuntimeEventKind::RunCreated { request } => {
                *transcript = request.transcript.clone();
                if !matches!(
                    transcript.entries.first(),
                    Some(TranscriptEntry::System { .. })
                ) {
                    transcript.entries.insert(
                        0,
                        TranscriptEntry::System {
                            prompt: request.system_prompt.clone(),
                        },
                    );
                }
                if !request.input.is_empty() {
                    transcript.entries.push(TranscriptEntry::User {
                        content: request.input.clone(),
                    });
                }
                None
            }
            RuntimeEventKind::ModelResponseCommitted { output: model, .. } => {
                transcript.entries.push(TranscriptEntry::Assistant {
                    content: (!model.content.is_empty()).then(|| model.content.clone()),
                    reasoning_content: model.reasoning_content.clone(),
                    tool_calls: model.tool_calls.clone(),
                });
                None
            }
            RuntimeEventKind::ContentDelta { delta, .. } => {
                summary.output.push_str(delta);
                if format == ExecOutputFormat::StreamJson {
                    exec_stream_line(&ExecStreamEvent::Content {
                        content: delta.clone(),
                    })
                    .ok()
                } else if !json_output {
                    Some(delta.as_bytes().to_vec())
                } else {
                    None
                }
            }
            RuntimeEventKind::ToolPrepared { invocation, .. } => {
                let started_at = timestamp(event.occurred_at_unix_ms);
                tool_starts.insert(
                    invocation.call_id.clone(),
                    ToolStart {
                        occurred_at_unix_ms: event.occurred_at_unix_ms,
                        started_at: started_at.clone(),
                    },
                );
                let input = invocation
                    .arguments
                    .parsed
                    .clone()
                    .unwrap_or_else(|| Value::String(invocation.arguments.raw.clone()));
                if format == ExecOutputFormat::StreamJson {
                    exec_stream_line(&ExecStreamEvent::ToolUse {
                        name: invocation.name.clone(),
                        id: invocation.call_id.clone(),
                        input,
                        started_at,
                    })
                    .ok()
                } else if !json_output {
                    Some(format!("tool: {}\n", invocation.name).into_bytes())
                } else {
                    None
                }
            }
            RuntimeEventKind::ToolOutcomeCommitted {
                call_id,
                name,
                outcome,
                ..
            } => {
                let start = tool_starts.remove(call_id).unwrap_or_else(|| ToolStart {
                    occurred_at_unix_ms: event.occurred_at_unix_ms,
                    started_at: timestamp(event.occurred_at_unix_ms),
                });
                summary.tools.push(ExecToolEntry {
                    name: name.clone(),
                    success: outcome.is_success(),
                    output: outcome.content.clone(),
                });
                if format == ExecOutputFormat::StreamJson {
                    exec_stream_line(&ExecStreamEvent::ToolResult {
                        id: call_id.clone(),
                        name: name.clone(),
                        output: outcome.content.clone(),
                        status: if outcome.is_success() {
                            "success"
                        } else {
                            "error"
                        }
                        .to_owned(),
                        started_at: start.started_at,
                        completed_at: timestamp(event.occurred_at_unix_ms),
                        duration_ms: event
                            .occurred_at_unix_ms
                            .saturating_sub(start.occurred_at_unix_ms),
                        side_effect_status: protocol_label(&outcome.side_effect),
                        error_category: (!outcome.is_success()).then(|| {
                            format!(
                                "transport_{}_operation_{}",
                                protocol_label(&outcome.transport),
                                protocol_label(&outcome.operation)
                            )
                        }),
                        truncated: None,
                        artifact: outcome
                            .artifacts
                            .first()
                            .and_then(|artifact| serde_json::to_value(artifact).ok()),
                        result_metadata: outcome.metadata.clone(),
                    })
                    .ok()
                } else if !json_output {
                    Some(
                        format!(
                            "tool {name} {}: {}\n",
                            if outcome.is_success() {
                                "completed"
                            } else {
                                "failed"
                            },
                            super::summarize_tool_output(&outcome.content)
                        )
                        .into_bytes(),
                    )
                } else {
                    None
                }
            }
            RuntimeEventKind::ChildFinished {
                call_id,
                outcome,
                handoff_content,
                ..
            } => {
                transcript.entries.push(TranscriptEntry::ChildOutcome {
                    call_id: call_id.clone(),
                    child_run_id: outcome.run_id.clone(),
                    outcome: outcome.clone(),
                    handoff_content: handoff_content.clone(),
                });
                None
            }
            RuntimeEventKind::Steered { content } => {
                transcript.entries.push(TranscriptEntry::User {
                    content: content.clone(),
                });
                None
            }
            RuntimeEventKind::Terminal { outcome } => {
                if summary.output.is_empty()
                    && let TerminalState::Completed { message } = &outcome.terminal
                {
                    summary.output.push_str(message);
                    if format != ExecOutputFormat::StreamJson && !json_output {
                        Some(format!("{message}\n").into_bytes())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            RuntimeEventKind::ModelRequestPrepared { .. }
            | RuntimeEventKind::ModelRequestInFlight { .. }
            | RuntimeEventKind::ModelRequestFailed { .. }
            | RuntimeEventKind::ReasoningDelta { .. }
            | RuntimeEventKind::ToolExecutionStarted { .. }
            | RuntimeEventKind::ChildStarted { .. } => None,
        };
        if !render {
            return None;
        }
        bytes.map(|bytes| output.write_stdout(bytes))
    }
}

struct TerminalProjection {
    receipt: ExecTerminalReceipt,
    error: Option<String>,
    code: &'static str,
    category: &'static str,
    recoverable: bool,
}

fn project_terminal(terminal: &TerminalState) -> TerminalProjection {
    let (reason, error, code, category, recoverable) = match terminal {
        TerminalState::Completed { .. } => (RunTerminationReason::Resolved, None, "", "", false),
        TerminalState::Blocked { reason } => (
            RunTerminationReason::Unresolved,
            Some(reason.clone()),
            "runtime_blocked",
            "state",
            false,
        ),
        TerminalState::RecoveryRequired { ambiguity } => (
            RunTerminationReason::Unresolved,
            Some(ambiguity.message.clone()),
            "runtime_recovery_ambiguous",
            "state",
            false,
        ),
        TerminalState::Cancelled | TerminalState::Interrupted => (
            RunTerminationReason::Canceled,
            Some("Headless 执行已取消".to_owned()),
            "exec_cancelled",
            "state",
            false,
        ),
        TerminalState::Failed { failure } => project_failure(failure),
    };
    TerminalProjection {
        receipt: ExecTerminalReceipt::from_reason(reason),
        error,
        code,
        category,
        recoverable,
    }
}

fn project_failure(
    failure: &RuntimeFailure,
) -> (
    RunTerminationReason,
    Option<String>,
    &'static str,
    &'static str,
    bool,
) {
    match failure {
        RuntimeFailure::RequestBudgetExceeded { limit } => (
            RunTerminationReason::BudgetExhausted,
            Some(format!("DeepSeek API 请求预算已用尽（上限：{limit}）")),
            "llm_api_request_budget_exhausted",
            "state",
            false,
        ),
        RuntimeFailure::TurnBudgetExceeded { limit } => (
            RunTerminationReason::BudgetExhausted,
            Some(format!("Agent turn 预算已用尽（上限：{limit}）")),
            "runtime_turn_budget_exhausted",
            "state",
            false,
        ),
        RuntimeFailure::ToolBudgetExceeded { limit } => (
            RunTerminationReason::BudgetExhausted,
            Some(format!("工具调用预算已用尽（上限：{limit}）")),
            "runtime_tool_budget_exhausted",
            "state",
            false,
        ),
        RuntimeFailure::DepthLimit { limit } => (
            RunTerminationReason::BudgetExhausted,
            Some(format!("子 Agent 深度超过上限：{limit}")),
            "runtime_depth_limit",
            "state",
            false,
        ),
        RuntimeFailure::Timeout { phase, .. } => (
            RunTerminationReason::Timeout,
            Some(format!(
                "Headless 执行超时（阶段：{}）",
                timeout_phase(*phase)
            )),
            "exec_watchdog_timeout",
            "timeout",
            false,
        ),
        RuntimeFailure::IncompleteModelStream => (
            RunTerminationReason::ModelError,
            Some("DeepSeek stream 在完整终止帧前关闭".to_owned()),
            "llm_stream_incomplete",
            "parse",
            false,
        ),
        RuntimeFailure::Model {
            code,
            category,
            message,
            retryable,
        } => (
            RunTerminationReason::ModelError,
            Some(message.clone()),
            if code == "deepseek_stream_incomplete" {
                "llm_stream_incomplete"
            } else if code == "stream_stall" {
                "stream_stall"
            } else {
                "deepseek_model_error"
            },
            model_error_category(*category),
            *retryable,
        ),
        RuntimeFailure::AccountingIncomplete { message } => (
            RunTerminationReason::InfrastructureError,
            Some(message.clone()),
            "llm_accounting_incomplete",
            "internal",
            false,
        ),
        RuntimeFailure::Store { message } => (
            RunTerminationReason::InfrastructureError,
            Some(message.clone()),
            "runtime_store_failed",
            "internal",
            false,
        ),
        RuntimeFailure::Join { message } => (
            RunTerminationReason::InfrastructureError,
            Some(message.clone()),
            "runtime_join_failed",
            "internal",
            false,
        ),
        RuntimeFailure::EmptyModelOutput => (
            RunTerminationReason::Unresolved,
            Some("DeepSeek 返回了空结果".to_owned()),
            "llm_empty_output",
            "state",
            false,
        ),
        RuntimeFailure::OutputLimit => (
            RunTerminationReason::Unresolved,
            Some("DeepSeek 输出达到长度上限".to_owned()),
            "llm_output_limit",
            "state",
            false,
        ),
        RuntimeFailure::ContentFiltered => (
            RunTerminationReason::ModelError,
            Some("DeepSeek 输出被内容策略终止".to_owned()),
            "llm_content_filtered",
            "state",
            false,
        ),
        RuntimeFailure::InsufficientSystemResource => (
            RunTerminationReason::ModelError,
            Some("DeepSeek 服务资源不足".to_owned()),
            "llm_insufficient_system_resource",
            "network",
            true,
        ),
        RuntimeFailure::InvalidModelOutput { message } => (
            RunTerminationReason::ModelError,
            Some(message.clone()),
            "llm_invalid_output",
            "parse",
            false,
        ),
    }
}

fn timeout_phase(phase: RuntimeTimeoutPhase) -> &'static str {
    match phase {
        RuntimeTimeoutPhase::Model => "model",
        RuntimeTimeoutPhase::Tool => "tool",
        RuntimeTimeoutPhase::Run => "run",
    }
}

fn model_error_category(category: ModelErrorCategory) -> &'static str {
    match category {
        ModelErrorCategory::Timeout | ModelErrorCategory::StreamStall => "timeout",
        ModelErrorCategory::Transport | ModelErrorCategory::Service => "network",
        ModelErrorCategory::RateLimit => "rate_limit",
        ModelErrorCategory::Authentication => "authentication",
        ModelErrorCategory::Protocol => "parse",
        ModelErrorCategory::Cancelled => "state",
        ModelErrorCategory::Unknown => "internal",
    }
}

fn accounting_receipt(accounting: &ModelAccounting) -> ExecAccountingReceipt {
    let mut standard = 0_u64;
    let mut strict = 0_u64;
    let mut fim = 0_u64;
    let buckets = accounting
        .surface_usage
        .iter()
        .map(|bucket| {
            match bucket.surface {
                ApiSurface::StandardChat => {
                    standard = standard.saturating_add(bucket.response_count)
                }
                ApiSurface::StrictChat => strict = strict.saturating_add(bucket.response_count),
                ApiSurface::Fim => fim = fim.saturating_add(bucket.response_count),
            }
            ExecSurfaceModelUsageBucket {
                model: bucket.model.clone(),
                api_surface: match bucket.surface {
                    ApiSurface::StandardChat => "standard_chat",
                    ApiSurface::StrictChat => "strict_chat",
                    ApiSurface::Fim => "fim",
                },
                response_count: u32_saturating(bucket.response_count),
                usage_response_count: u32_saturating(bucket.usage_response_count),
                input_tokens: u32_saturating(bucket.usage.input_tokens),
                output_tokens: u32_saturating(bucket.usage.output_tokens),
                prompt_cache_hit_tokens: Some(u32_saturating(bucket.usage.cache_hit_tokens)),
                prompt_cache_miss_tokens: Some(u32_saturating(bucket.usage.cache_miss_tokens)),
                prompt_cache_write_tokens: Some(u32_saturating(bucket.usage.cache_write_tokens)),
                reasoning_tokens: Some(u32_saturating(bucket.usage.reasoning_tokens)),
                reasoning_replay_tokens: Some(u32_saturating(bucket.usage.reasoning_replay_tokens)),
                total_tokens: bucket.usage.total_tokens(),
                cost_usd: nano_to_unit(bucket.cost_nanousd),
                cost_cny: nano_to_unit(bucket.cost_nanocny),
            }
        })
        .collect();
    ExecAccountingReceipt {
        input_tokens: Some(u32_saturating(accounting.usage.input_tokens)),
        output_tokens: Some(u32_saturating(accounting.usage.output_tokens)),
        prompt_cache_hit_tokens: Some(u32_saturating(accounting.usage.cache_hit_tokens)),
        prompt_cache_miss_tokens: Some(u32_saturating(accounting.usage.cache_miss_tokens)),
        prompt_cache_write_tokens: Some(u32_saturating(accounting.usage.cache_write_tokens)),
        reasoning_tokens: Some(u32_saturating(accounting.usage.reasoning_tokens)),
        reasoning_replay_tokens: Some(u32_saturating(accounting.usage.reasoning_replay_tokens)),
        total_tokens: Some(accounting.usage.total_tokens()),
        cost_usd: Some(nano_to_unit(accounting.cost_nanousd)),
        cost_cny: Some(nano_to_unit(accounting.cost_nanocny)),
        usage_response_count: Some(u32_saturating(accounting.usage_responses)),
        standard_chat_response_count: Some(u32_saturating(standard)),
        strict_chat_response_count: Some(u32_saturating(strict)),
        fim_response_count: Some(u32_saturating(fim)),
        surface_model_usage_buckets: Some(buckets),
        usage_complete: Some(accounting.usage_complete),
        cost_complete: Some(
            !accounting.usage_missing
                && !accounting.usage_incomplete
                && !accounting.billing_unknown
                && !accounting.unpriced
                && accounting.records_after_seal == 0,
        ),
        usage_missing_responses: Some(u32_saturating(accounting.usage_missing_responses)),
        usage_incomplete_responses: Some(u32_saturating(accounting.incomplete_responses)),
        billing_unknown_attempts: Some(u32_saturating(accounting.billing_unknown_attempts)),
        unpriced_usage_responses: Some(u32_saturating(accounting.unpriced_usage_responses)),
        usage_records_after_seal_observed: Some(u32_saturating(accounting.records_after_seal)),
        transport_retry_count: Some(u32_saturating(accounting.transport_retries)),
        api_request_count: Some(u32_saturating(accounting.total_started())),
        api_request_completed: Some(u32_saturating(accounting.total_completed())),
        api_request_in_flight: Some(u32_saturating(accounting.total_in_flight())),
        api_request_root_started: Some(u32_saturating(accounting.root.started)),
        api_request_root_completed: Some(u32_saturating(accounting.root.completed)),
        api_request_root_in_flight: Some(u32_saturating(accounting.root.in_flight)),
        api_request_child_started: Some(u32_saturating(accounting.child.started)),
        api_request_child_completed: Some(u32_saturating(accounting.child.completed)),
        api_request_child_in_flight: Some(u32_saturating(accounting.child.in_flight)),
        api_request_limit: accounting.hard_request_limit,
        api_request_budget_exhausted: Some(accounting.budget_exhausted),
        api_request_rejected_exhausted: Some(u32_saturating(accounting.exhausted_denied)),
        api_request_rejected_after_seal_observed: Some(u32_saturating(accounting.sealed_denied)),
    }
}

fn u32_saturating(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn nano_to_unit(value: u64) -> f64 {
    value as f64 / 1_000_000_000.0
}

struct TerminalMetadata<'a> {
    receipt_kind: &'static str,
    provider: &'a str,
    model: &'a str,
    route_source: &'a str,
    started: Instant,
    approval_posture: &'a str,
    sandbox_posture: &'a str,
    prompt: &'a str,
    tool_catalog_sha256: Option<String>,
    workspace: &'a Path,
    run_id: &'a RunId,
}

#[allow(clippy::too_many_arguments)]
async fn emit_terminal_output(
    output: &crate::exec_output::ExecOutput,
    summary: &ExecSummary,
    transcript: &CanonicalTranscript,
    outcome: &AgentOutcome,
    terminal: &TerminalProjection,
    metadata: TerminalMetadata<'_>,
    format: ExecOutputFormat,
    json_output: bool,
) -> Result<(), String> {
    if format == ExecOutputFormat::StreamJson {
        let meta = ExecStreamMeta {
            receipt_kind: metadata.receipt_kind,
            provider: metadata.provider.to_owned(),
            model: metadata.model.to_owned(),
            route_source: metadata.route_source.to_owned(),
            accounting: summary.accounting.clone(),
            duration_ms: u64::try_from(metadata.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            approval_posture: metadata.approval_posture.to_owned(),
            sandbox_posture: metadata.sandbox_posture.to_owned(),
            binary_sha256: current_binary_sha256(),
            config_sha256: None,
            prompt_sha256: format!(
                "sha256:{}",
                crate::hashing::sha256_hex(metadata.prompt.as_bytes())
            ),
            tool_catalog_sha256: metadata.tool_catalog_sha256,
            input_analysis: runtime_input_analysis(transcript),
            visible_final_answer_chars: summary.output.chars().count(),
            run_id: metadata.run_id.to_string(),
            resume_command: format!("codewhale exec --resume {}", metadata.run_id),
            workspace: metadata.workspace.display().to_string(),
            message_count: transcript
                .entries
                .iter()
                .filter(|entry| !matches!(entry, TranscriptEntry::System { .. }))
                .count(),
            terminal: terminal.receipt,
            error_category: summary.error_category.clone(),
        };
        write_exec_stream_terminal(
            output,
            &ExecStreamEvent::Metadata {
                meta: Box::new(meta),
            },
        )
        .await?;
        if let Some(error) = terminal.error.as_ref() {
            write_exec_stream_terminal(
                output,
                &ExecStreamEvent::Error {
                    error: error.clone(),
                    code: terminal.code.to_owned(),
                    category: terminal.category.to_owned(),
                    recoverable: terminal.recoverable,
                    termination_reason: terminal.receipt.termination_reason(),
                },
            )
            .await?;
        }
        write_exec_stream_terminal(output, &ExecStreamEvent::Done).await?;
    } else if !json_output {
        let accounting = &outcome.accounting;
        let request_line = if let Some(limit) = accounting.hard_request_limit {
            format!(
                "DeepSeek API 请求：{}/{limit}\n",
                accounting.total_started()
            )
        } else {
            format!("DeepSeek API 请求：{}\n", accounting.total_started())
        };
        wait_terminal_output(output.enqueue_stderr(request_line.into_bytes())).await?;
        wait_terminal_output(
            output.enqueue_stderr(
                format!(
                    "DeepSeek 用量：输入 {}，输出 {}；估算费用 ${:.6} / ¥{:.6}\n",
                    accounting.usage.input_tokens,
                    accounting.usage.output_tokens,
                    nano_to_unit(accounting.cost_nanousd),
                    nano_to_unit(accounting.cost_nanocny),
                )
                .into_bytes(),
            ),
        )
        .await?;
        if let Some(error) = terminal.error.as_ref() {
            wait_terminal_output(output.enqueue_stderr(format!("错误：{error}\n").into_bytes()))
                .await?;
        }
    }
    Ok(())
}

fn runtime_input_analysis(transcript: &CanonicalTranscript) -> ExecStreamInputAnalysis {
    let mut analysis = ExecStreamInputAnalysis::default();
    for entry in &transcript.entries {
        match entry {
            TranscriptEntry::System { prompt } => {
                for block in &prompt.blocks {
                    add_text(
                        &block.text,
                        &mut analysis.text_chars,
                        &mut analysis.text_estimated_tokens,
                    );
                    analysis.estimated_system_tokens =
                        analysis.estimated_system_tokens.saturating_add(
                            crate::compaction::estimate_text_tokens_conservative(&block.text),
                        );
                }
            }
            TranscriptEntry::User { content } => {
                analysis.user_message_count += 1;
                add_text(
                    content,
                    &mut analysis.text_chars,
                    &mut analysis.text_estimated_tokens,
                );
            }
            TranscriptEntry::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => {
                analysis.assistant_message_count += 1;
                if let Some(content) = content {
                    add_text(
                        content,
                        &mut analysis.text_chars,
                        &mut analysis.text_estimated_tokens,
                    );
                }
                if let Some(reasoning) = reasoning_content {
                    add_text(
                        reasoning,
                        &mut analysis.thinking_chars,
                        &mut analysis.thinking_estimated_tokens,
                    );
                }
                for call in tool_calls {
                    analysis.tool_use_count += 1;
                    add_text(
                        &call.arguments.raw,
                        &mut analysis.tool_use_input_chars,
                        &mut analysis.tool_use_input_estimated_tokens,
                    );
                }
            }
            TranscriptEntry::Tool { outcome, .. } => {
                analysis.tool_message_count += 1;
                analysis.tool_result_count += 1;
                add_text(
                    &outcome.content,
                    &mut analysis.tool_result_chars,
                    &mut analysis.tool_result_estimated_tokens,
                );
            }
            TranscriptEntry::ChildOutcome {
                handoff_content, ..
            } => {
                analysis.user_message_count += 1;
                add_text(
                    handoff_content,
                    &mut analysis.tool_result_chars,
                    &mut analysis.tool_result_estimated_tokens,
                );
            }
        }
    }
    let messages = analysis
        .user_message_count
        .saturating_add(analysis.assistant_message_count)
        .saturating_add(analysis.tool_message_count);
    analysis.estimated_message_content_tokens = analysis
        .text_estimated_tokens
        .saturating_add(analysis.thinking_estimated_tokens)
        .saturating_add(analysis.tool_use_input_estimated_tokens)
        .saturating_add(analysis.tool_result_estimated_tokens);
    analysis.estimated_framing_tokens = messages.saturating_mul(12).saturating_add(48);
    analysis.estimated_request_tokens = analysis
        .estimated_system_tokens
        .saturating_add(analysis.estimated_message_content_tokens)
        .saturating_add(analysis.estimated_framing_tokens);
    analysis
}

fn add_text(text: &str, chars: &mut usize, tokens: &mut usize) {
    *chars = chars.saturating_add(text.chars().count());
    *tokens = tokens.saturating_add(crate::compaction::estimate_text_tokens_conservative(text));
}

fn timestamp(unix_ms: u64) -> String {
    i64::try_from(unix_ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|timestamp| timestamp.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339())
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_execution_fingerprint(config: &Config) -> String {
        execution_fingerprint_sha256(
            config,
            "deepseek-v4-pro",
            &ToolContext::new(std::env::temp_dir().join("codewhale-exec-fingerprint")),
            Some("sha256:test-tool-catalog"),
        )
    }

    #[test]
    fn execution_fingerprint_binds_deepseek_wire_decoder_and_tool_backend_without_secrets() {
        let mut baseline = Config {
            provider: Some("deepseek".to_owned()),
            api_key: Some("deepseek-secret-a".to_owned()),
            base_url: Some("https://api.deepseek.com".to_owned()),
            strict_tool_mode: Some(false),
            sandbox_backend: Some("opensandbox".to_owned()),
            sandbox_url: Some("https://sandbox-a.example".to_owned()),
            sandbox_api_key: Some("sandbox-secret-a".to_owned()),
            providers: Some(crate::config::ProvidersConfig::default()),
            tui: Some(crate::config::TuiConfig {
                stream_chunk_timeout_secs: Some(900),
                ..crate::config::TuiConfig::default()
            }),
            ..Config::default()
        };
        baseline
            .providers
            .as_mut()
            .expect("providers")
            .deepseek
            .reasoning_stream_style = Some("separate_field".to_owned());
        let expected = test_execution_fingerprint(&baseline);

        let mut strict = baseline.clone();
        strict.strict_tool_mode = Some(true);
        assert_ne!(test_execution_fingerprint(&strict), expected);

        let mut suffix = baseline.clone();
        suffix
            .providers
            .as_mut()
            .expect("providers")
            .deepseek
            .path_suffix = Some("v1/custom-chat".to_owned());
        assert_ne!(test_execution_fingerprint(&suffix), expected);

        let mut decoder = baseline.clone();
        decoder
            .providers
            .as_mut()
            .expect("providers")
            .deepseek
            .reasoning_stream_style = Some("inline_tags".to_owned());
        assert_ne!(test_execution_fingerprint(&decoder), expected);

        let mut idle_timeout = baseline.clone();
        idle_timeout
            .tui
            .as_mut()
            .expect("tui")
            .stream_chunk_timeout_secs = Some(901);
        assert_ne!(test_execution_fingerprint(&idle_timeout), expected);

        let mut sandbox_endpoint = baseline.clone();
        sandbox_endpoint.sandbox_url = Some("https://sandbox-b.example".to_owned());
        assert_ne!(test_execution_fingerprint(&sandbox_endpoint), expected);

        let mut rotated_credentials = baseline;
        rotated_credentials.api_key = Some("deepseek-secret-b".to_owned());
        rotated_credentials.sandbox_api_key = Some("sandbox-secret-b".to_owned());
        assert_eq!(test_execution_fingerprint(&rotated_credentials), expected);
    }

    #[test]
    fn disabled_subagents_collapse_exec_limits_to_the_root_actor() {
        let config: Config =
            toml::from_str("[subagents]\nenabled = false\n").expect("disabled subagent config");

        let limits = runtime_limits(
            &config,
            crate::config::ApiProvider::Deepseek,
            4,
            10,
            None,
            30_000,
        );

        assert_eq!(limits.max_depth, 0);
        assert_eq!(limits.max_concurrent_children, 0);
        assert_eq!(limits.max_model_requests, 10);
        assert_eq!(limits.max_tool_calls, 40);
    }
}
