//! Thin `codewhale exec` client for the production Agent application.
//!
//! This module projects CLI/config inputs into canonical Run commands, forwards
//! control commands, and renders canonical stored events. Production route,
//! prompt, model, tool, Store, resume, and runtime composition stay in
//! `codewhale-app`.

use std::collections::HashMap;
use std::num::{NonZeroU32, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use codewhale_app::{
    AgentApplication, DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
    ProductionPromptConfig, ProductionToolConfig, ShellPolicy, TransportRetryPolicy,
};
use codewhale_context::InstructionSource;
use codewhale_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunApiError, RunApiErrorCode, RunCommand, RunCommandEnvelope,
    RunCommandResult, RunProductControls, StartRunCommand,
};
use codewhale_runtime::{
    AgentOutcome, ApiSurface, CanonicalTranscript, ModelAccounting, ModelErrorCategory,
    ReasoningEffort, RunId, RunLimits, RuntimeEventKind, RuntimeFailure, RuntimeTimeoutPhase,
    StoredRuntimeEvent, TerminalState, ToolPolicy, TranscriptEntry,
};
use serde::Serialize;
use serde_json::Value;

use crate::config::{Config, MAX_SUBAGENTS};
use crate::core::termination::RunTerminationReason;
use crate::exec_output::ExecTerminalReceipt;

use super::{
    EXEC_OUTPUT_CLOSE_TIMEOUT_SECS, EXEC_OUTPUT_QUEUE_CAPACITY, EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS,
    ExecAccountingReceipt, ExecOutputFormat, ExecOutputWait, ExecStreamEvent,
    ExecStreamInputAnalysis, ExecStreamMeta, ExecSurfaceModelUsageBucket,
    commit_exec_terminal_signal, current_binary_sha256, exec_stream_line, exec_supports_provider,
    recv_exec_signal, stop_exec_signal_controller, wait_exec_output_until, wait_terminal_output,
    write_exec_stream_terminal,
};

fn protocol_label(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
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
    // The exact executable hash is diagnostic receipt evidence only. In debug
    // builds the binary is large and SHA-256 is deliberately unoptimized, so
    // warm the process-wide cache on a blocking worker while route resolution
    // performs network I/O. Resume compatibility uses the shared composition
    // build revision instead, allowing exec and app-server to resume the same
    // run even though they are different executables.
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

        let deadline = deadline_origin + Duration::from_secs(max_runtime_secs.max(1));
        let (mut signal_rx, signal_task, signal_phase) = super::spawn_exec_signal_controller();
        let mut signal_task = Some(signal_task);
        let settings = crate::settings::Settings::load().unwrap_or_default();
        startup_failure = ExecStartupFailure::ToolContext;
        let application_config = production_application_config(
            config,
            &workspace,
            &settings,
            auto_approve,
            trust_mode,
            append_system_prompt,
        )?;
        startup_failure = ExecStartupFailure::RunStore;
        let application = AgentApplication::production(application_config)
            .context("无法创建 production AgentApplication")?;

        let is_resume = resume_run_id.is_some();
        let requested_auto_model = !is_resume && model.trim().eq_ignore_ascii_case("auto");
        let route_source = if is_resume {
            "run_store_resume"
        } else if requested_auto_model {
            "auto_resolver"
        } else {
            "explicit_or_configured"
        };
        let remaining_runtime_ms = absolute_deadline_unix_ms.saturating_sub(unix_ms_now());
        if remaining_runtime_ms == 0 {
            startup_failure = ExecStartupFailure::RouteTimeout;
            stop_exec_signal_controller(&mut signal_task).await;
            bail!("exec_watchdog_timeout: AgentApplication 启动前已耗尽全局运行时间");
        }
        let command = if let Some(run_id) = resume_run_id {
            RunCommand::Resume {
                run_id: RunId::from(run_id),
                expected_workspace: Some(workspace.display().to_string()),
            }
        } else {
            RunCommand::Start(StartRunCommand {
                input: prompt.to_owned(),
                workspace: workspace.display().to_string(),
                model: (!requested_auto_model).then(|| model.to_owned()),
                reasoning_effort: config
                    .reasoning_effort
                    .as_deref()
                    .map(runtime_reasoning_effort)
                    .unwrap_or_default(),
                // `exec` deliberately requests the complete official 384K
                // output allowance. Other app clients retain their own policy.
                max_output_tokens: Some(384_000),
                max_api_requests,
                streaming: tool_mode || output_format == ExecOutputFormat::StreamJson,
                tool_policy: runtime_tool_policy(tool_mode, allowed_tools, disallowed_tools),
                limits: runtime_limits(
                    config,
                    config.api_provider(),
                    max_subagents,
                    max_turns,
                    max_api_requests,
                    remaining_runtime_ms,
                ),
                controls: RunProductControls {
                    auto_approve,
                    trust_mode,
                    allow_sandbox_elevation,
                    interactive: false,
                    sandbox: explicit_sandbox
                        .map(str::to_owned)
                        .or_else(|| config.sandbox_mode.clone()),
                },
            })
        };
        startup_failure = if requested_auto_model {
            ExecStartupFailure::Route
        } else {
            ExecStartupFailure::InvalidArguments
        };
        let response = tokio::select! {
            biased;
            signal = recv_exec_signal(&mut signal_rx) => {
                stop_exec_signal_controller(&mut signal_task).await;
                if let Some(exit_code) = signal {
                    std::process::exit(exit_code);
                }
                bail!("Headless 信号控制器在 AgentApplication 启动阶段意外退出");
            }
            response = tokio::time::timeout_at(
                deadline,
                application.execute(run_envelope("exec-launch", command)),
            ) => match response {
                Ok(response) => response,
                Err(_) => {
                    startup_failure = ExecStartupFailure::RouteTimeout;
                    stop_exec_signal_controller(&mut signal_task).await;
                    bail!("exec_watchdog_timeout: AgentApplication 启动超过最大运行时间");
                }
            }
        };
        let run = match response.result {
            RunCommandResult::Run { run } => *run,
            RunCommandResult::Error { error } => {
                if is_resume && error.code == RunApiErrorCode::RunAlreadyRunning {
                    runtime_started = true;
                    startup_failure = ExecStartupFailure::RunStore;
                    stop_exec_signal_controller(&mut signal_task).await;
                    if output_format == ExecOutputFormat::StreamJson {
                        emit_exec_stream_failure(
                            config,
                            model,
                            prompt,
                            &workspace,
                            started,
                            auto_approve,
                            explicit_sandbox,
                            "runtime_failure",
                            "run_store_resume",
                            error.run_id.as_ref(),
                            startup_failure,
                            "runtime_store_failed",
                            &error.message,
                        )
                        .await
                        .map_err(anyhow::Error::msg)?;
                    }
                    bail!("runtime_store_failed：{}", error.message);
                }
                startup_failure = startup_failure_for_run_api(&error);
                stop_exec_signal_controller(&mut signal_task).await;
                bail!("{}：{}", startup_failure.code(), error.message);
            }
            other => {
                stop_exec_signal_controller(&mut signal_task).await;
                bail!("AgentApplication 启动返回了非 Run 结果: {other:?}");
            }
        };
        runtime_started = true;
        let root_run_id = run.run_id.clone();
        let mut effective_model = run.model;
        let mut effective_prompt = prompt.to_owned();
        let mut effective_auto_approve = auto_approve;
        let mut effective_sandbox = explicit_sandbox
            .map(str::to_owned)
            .or_else(|| config.sandbox_mode.clone());
        let mut run_provider = "deepseek".to_owned();
        let mut run_workspace = workspace.clone();
        let mut tool_catalog_sha256 = None;

        let output = crate::exec_output::ExecOutput::new(
            NonZeroUsize::new(EXEC_OUTPUT_QUEUE_CAPACITY)
                .expect("output queue capacity is non-zero"),
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
        let mut after_sequence = 0_u64;
        let mut drain_events = false;
        let mut ignore_signals = false;
        let mut drain_deadline = None;
        let mut output_failure = None;
        let mut signal_exit_code = None;

        while terminal.is_none() {
            enum Next {
                Events(RunCommandResult),
                Signal(Option<i32>),
                DrainTimeout,
            }

            let next = if drain_events {
                let settle_deadline = *drain_deadline.get_or_insert_with(|| {
                    tokio::time::Instant::now()
                        + Duration::from_secs(EXEC_TOTAL_SHUTDOWN_TIMEOUT_SECS)
                });
                match tokio::time::timeout_at(
                    settle_deadline,
                    application.wait_events(&root_run_id, after_sequence),
                )
                .await
                {
                    Ok(events) => Next::Events(events),
                    Err(_) => Next::DrainTimeout,
                }
            } else if ignore_signals {
                Next::Events(
                    application
                        .wait_events(&root_run_id, after_sequence)
                        .await,
                )
            } else {
                tokio::select! {
                    biased;
                    signal = recv_exec_signal(&mut signal_rx) => Next::Signal(signal),
                    events = application.wait_events(&root_run_id, after_sequence) => Next::Events(events),
                }
            };

            let events = match next {
                Next::Signal(Some(exit_code)) => {
                    let response = application
                        .execute(run_envelope(
                            "exec-signal-cancel",
                            RunCommand::Cancel {
                                run_id: root_run_id.clone(),
                            },
                        ))
                        .await;
                    if signal_cancel_won(&response.result, &signal_phase) {
                        signal_exit_code = Some(exit_code);
                        drain_events = true;
                    } else {
                        ignore_signals = true;
                    }
                    continue;
                }
                Next::Signal(None) => {
                    output_failure.get_or_insert_with(|| {
                        "Headless 信号控制器在运行期间意外退出".to_owned()
                    });
                    let response = application
                        .execute(run_envelope(
                            "exec-controller-cancel",
                            RunCommand::Cancel {
                                run_id: root_run_id.clone(),
                            },
                        ))
                        .await;
                    if signal_cancel_won(&response.result, &signal_phase) {
                        drain_events = true;
                    } else {
                        ignore_signals = true;
                    }
                    continue;
                }
                Next::DrainTimeout => std::process::exit(1),
                Next::Events(RunCommandResult::Events { events, .. }) => events,
                Next::Events(RunCommandResult::Error { error }) => {
                    output_failure.get_or_insert(error.message);
                    break;
                }
                Next::Events(other) => {
                    output_failure.get_or_insert_with(|| {
                        format!("AgentApplication events 返回了意外结果: {other:?}")
                    });
                    break;
                }
            };

            for event in events {
                if event.run_id != root_run_id || event.sequence <= after_sequence {
                    continue;
                }
                after_sequence = event.sequence;
                if let RuntimeEventKind::RunCreated { request } = &event.event {
                    effective_model.clone_from(&request.model);
                    effective_prompt.clone_from(&request.input);
                    effective_auto_approve = request.environment.auto_approve;
                    effective_sandbox.clone_from(&request.environment.sandbox);
                    run_provider.clone_from(&request.environment.provider);
                    run_workspace = PathBuf::from(&request.environment.workspace);
                    tool_catalog_sha256.clone_from(&request.environment.tool_catalog_sha256);
                    summary.model.clone_from(&request.model);
                    summary.prompt.clone_from(&request.input);
                }
                if matches!(&event.event, RuntimeEventKind::Terminal { .. }) {
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
                    if ignore_signals {
                        if let Err(error) = wait.await {
                            output_failure.get_or_insert_with(|| error.to_string());
                        }
                    } else {
                        match wait_exec_output_until(wait, deadline, &mut signal_rx).await {
                        ExecOutputWait::Written => {}
                        ExecOutputWait::WatchdogTimeout => drain_events = true,
                        ExecOutputWait::Signal(Some(exit_code)) => {
                            let response = application
                                .execute(run_envelope(
                                    "exec-output-cancel",
                                    RunCommand::Cancel {
                                        run_id: root_run_id.clone(),
                                    },
                                ))
                                .await;
                            if signal_cancel_won(&response.result, &signal_phase) {
                                signal_exit_code = Some(exit_code);
                                drain_events = true;
                            } else {
                                ignore_signals = true;
                            }
                        }
                        ExecOutputWait::Signal(None) => {
                            output_failure.get_or_insert_with(|| {
                                "Headless 信号控制器在输出期间意外退出".to_owned()
                            });
                            let response = application
                                .execute(run_envelope(
                                    "exec-output-controller-cancel",
                                    RunCommand::Cancel {
                                        run_id: root_run_id.clone(),
                                    },
                                ))
                                .await;
                            if signal_cancel_won(&response.result, &signal_phase) {
                                drain_events = true;
                            } else {
                                ignore_signals = true;
                            }
                        }
                        ExecOutputWait::Failed(error) => {
                            output_failure.get_or_insert(error);
                            let _ = application
                                .execute(run_envelope(
                                    "exec-output-failure-cancel",
                                    RunCommand::Cancel {
                                        run_id: root_run_id.clone(),
                                    },
                                ))
                                .await;
                            drain_events = true;
                        }
                        }
                    }
                }
                if let RuntimeEventKind::Terminal { outcome } = event.event {
                    terminal = Some(*outcome);
                    break;
                }
            }
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
                "AgentApplication ended without a canonical terminal event{}",
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
                    receipt_kind: "terminal",
                    provider: &run_provider,
                    model: &effective_model,
                    route_source,
                    started,
                    approval_posture: if effective_auto_approve {
                        "auto_tools"
                    } else {
                        "ask"
                    },
                    sandbox_posture: effective_sandbox
                        .as_deref()
                        .unwrap_or("configured_default"),
                    prompt: &effective_prompt,
                    tool_catalog_sha256,
                    workspace: &run_workspace,
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
        if report.unjoined {
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
        && let Err(output_error) = emit_exec_stream_failure(
            config,
            model,
            prompt,
            &workspace,
            started,
            auto_approve,
            explicit_sandbox,
            "startup_failure",
            "startup",
            None,
            startup_failure,
            startup_failure.code(),
            &error.to_string(),
        )
        .await
    {
        return Err(anyhow!("{error}; startup output failed: {output_error}"));
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn emit_exec_stream_failure(
    config: &Config,
    model: &str,
    prompt: &str,
    workspace: &Path,
    started: Instant,
    auto_approve: bool,
    explicit_sandbox: Option<&str>,
    receipt_kind: &'static str,
    route_source: &str,
    run_id: Option<&RunId>,
    failure: ExecStartupFailure,
    error_code: &str,
    message: &str,
) -> std::result::Result<(), String> {
    let output = crate::exec_output::ExecOutput::new(
        NonZeroUsize::new(EXEC_OUTPUT_QUEUE_CAPACITY).expect("output queue capacity is non-zero"),
    );
    let terminal = ExecTerminalReceipt::from_reason(failure.termination_reason());
    let meta = ExecStreamMeta {
        receipt_kind,
        provider: config.api_provider().as_str().to_owned(),
        model: model.to_owned(),
        route_source: route_source.to_owned(),
        accounting: ExecAccountingReceipt::default(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        approval_posture: if auto_approve { "auto_tools" } else { "ask" }.to_owned(),
        sandbox_posture: explicit_sandbox
            .or(config.sandbox_mode.as_deref())
            .unwrap_or("configured_default")
            .to_owned(),
        binary_sha256: current_binary_sha256(),
        config_sha256: None,
        prompt_sha256: format!("sha256:{}", crate::hashing::sha256_hex(prompt.as_bytes())),
        tool_catalog_sha256: None,
        input_analysis: ExecStreamInputAnalysis::default(),
        visible_final_answer_chars: 0,
        run_id: run_id.map(ToString::to_string).unwrap_or_default(),
        resume_command: run_id
            .map(|run_id| format!("codewhale exec --resume {run_id}"))
            .unwrap_or_default(),
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
                code: error_code.to_owned(),
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

fn run_envelope(request_id: &str, command: RunCommand) -> RunCommandEnvelope {
    RunCommandEnvelope {
        schema_version: RUN_API_SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        command,
    }
}

/// Settle the process signal against the canonical control result.
///
/// `RunTerminal` means the Store terminal was already authoritative before
/// Cancel reached the active control. In that case suppress the latched signal
/// so the canonical terminal receipt and its normal exit status win. Every
/// other result keeps the signal as the cooperative shutdown owner.
fn signal_cancel_won(result: &RunCommandResult, phase: &AtomicI32) -> bool {
    if matches!(
        result,
        RunCommandResult::Error { error }
            if error.code == RunApiErrorCode::RunTerminal
    ) {
        phase.store(-1, Ordering::SeqCst);
        false
    } else {
        true
    }
}

fn startup_failure_for_run_api(error: &RunApiError) -> ExecStartupFailure {
    let message = error.message.as_str();
    if message.starts_with("run_resume_workspace_mismatch：") {
        return ExecStartupFailure::ResumeWorkspaceMismatch;
    }
    if message.starts_with("run_resume_provider_mismatch：") {
        return ExecStartupFailure::ResumeProviderMismatch;
    }
    if message.starts_with("run_resume_tool_catalog_mismatch：") {
        return ExecStartupFailure::ResumeToolCatalogMismatch;
    }
    if message.starts_with("run_resume_fingerprint_missing：") {
        return ExecStartupFailure::ResumeFingerprintMissing;
    }
    if message.starts_with("run_resume_fingerprint_mismatch：") {
        return ExecStartupFailure::ResumeFingerprintMismatch;
    }
    match error.code {
        RunApiErrorCode::RunNotFound => ExecStartupFailure::ResumeNotFound,
        RunApiErrorCode::RunStoreFailed | RunApiErrorCode::RunAlreadyRunning => {
            ExecStartupFailure::RunStore
        }
        RunApiErrorCode::InvalidRequest if message.starts_with("deepseek_auto_route_") => {
            ExecStartupFailure::Route
        }
        RunApiErrorCode::InvalidRequest if message.starts_with("deepseek_credential_missing：") => {
            ExecStartupFailure::Client
        }
        RunApiErrorCode::InvalidRequest => ExecStartupFailure::InvalidArguments,
        _ => ExecStartupFailure::InvalidArguments,
    }
}

fn production_application_config(
    config: &Config,
    workspace: &Path,
    settings: &crate::settings::Settings,
    auto_approve: bool,
    trust_mode: bool,
    append_system_prompt: Option<String>,
) -> Result<ProductionApplicationConfig> {
    let provider = config.api_provider();
    let path_suffix = config
        .provider_config_for(provider)
        .and_then(|provider| provider.path_suffix.as_deref());
    if path_suffix.is_some() {
        bail!("DeepSeek production AgentApplication 不支持 path_suffix 路由改写");
    }
    let base_url = config.deepseek_base_url();
    let endpoint = if codewhale_deepseek::official_root(&base_url).is_some() {
        DeepSeekEndpoint::Official
    } else {
        DeepSeekEndpoint::loopback_fixture(crate::client::versioned_base_url(&base_url))?
    };
    let retry = config.retry_policy();
    let connection = DeepSeekConnectionConfig {
        endpoint,
        strict_tools: config.strict_tool_mode.unwrap_or(false),
        response_header_timeout: Duration::from_secs(45),
        stream_idle_timeout: Duration::from_secs(config.stream_chunk_timeout_secs()),
        retry: TransportRetryPolicy {
            max_retries: if retry.enabled { retry.max_retries } else { 0 },
            initial_delay: Duration::from_secs_f64(retry.initial_delay.clamp(0.0, 300.0)),
            max_delay: Duration::from_secs_f64(retry.max_delay.clamp(0.0, 300.0)),
            exponential_base: retry.exponential_base,
        },
    };

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
    let prompt = ProductionPromptConfig {
        preferences: settings.prompt_preferences(),
        instructions,
        skills_dir: Some(config.skills_dir()),
        project_context_pack_enabled: config.project_context_pack_enabled(),
        verbosity: config.verbosity.clone(),
        skills_scan_codewhale_only: config.skills_config().scan_codewhale_only(),
        shell_binary: codewhale_tools::shell_dispatcher::global_dispatcher()
            .kind()
            .binary()
            .to_owned(),
    };

    let trusted = crate::workspace_trust::WorkspaceTrust::load_for(workspace);
    let shell_policy = if auto_approve || config.allow_shell() {
        ShellPolicy::Full
    } else {
        ShellPolicy::None
    };
    let mut tools = ProductionToolConfig::new(workspace.to_path_buf())
        .with_trust_mode(trust_mode)
        .with_trusted_external_paths(trusted.paths().to_vec())
        .with_follow_symlinks(settings.workspace_follow_symlinks)
        .with_auto_approve(auto_approve)
        .with_shell_policy(shell_policy)
        .with_prefer_external_pdftotext(settings.prefer_external_pdftotext);
    if let Some(backend) = crate::sandbox_backend::create_backend(config)? {
        tools = tools.with_sandbox_backend(Arc::from(backend));
    }
    let exec_policy = if config
        .features()
        .enabled(crate::features::Feature::ExecPolicy)
    {
        crate::execpolicy::load_default_policy()?.map(|policy| policy.production_snapshot())
    } else {
        None
    };
    tools = tools.with_exec_policy(exec_policy);

    let mut application = ProductionApplicationConfig::official()
        .with_deepseek_connection(connection)
        .with_tool_config(tools)
        .with_prompt(prompt)
        .with_default_max_api_requests(
            NonZeroU32::new(u32::MAX).expect("exec request tracking limit is non-zero"),
        );
    if let Ok(api_key) = config.deepseek_api_key()
        && !api_key.trim().is_empty()
    {
        application = application.with_api_key(api_key)?;
    }
    Ok(application)
}

fn runtime_tool_policy(
    enabled: bool,
    allowed: Option<Vec<String>>,
    denied: Option<Vec<String>>,
) -> ToolPolicy {
    let mut allowed = allowed;
    if let Some(allowed) = allowed.as_mut() {
        allowed.sort();
        allowed.dedup();
    }
    let mut denied = denied.unwrap_or_default();
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
            RuntimeEventKind::InteractionRequested { .. }
            | RuntimeEventKind::InteractionResolved { .. }
            | RuntimeEventKind::SteerQueued { .. }
            | RuntimeEventKind::ControlRequested { .. } => None,
            RuntimeEventKind::SteerApplied { content, .. } => {
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

    #[test]
    fn canonical_terminal_rejects_an_already_latched_process_signal() {
        let phase = AtomicI32::new(143);
        let result = RunCommandResult::Error {
            error: RunApiError {
                code: RunApiErrorCode::RunTerminal,
                message: "run already terminal".to_owned(),
                run_id: Some(RunId::from("run-terminal")),
                terminal: None,
            },
        };

        assert!(!signal_cancel_won(&result, &phase));
        assert_eq!(phase.load(Ordering::SeqCst), -1);
        assert_eq!(commit_exec_terminal_signal(&phase), None);
    }

    #[test]
    fn accepted_cancel_keeps_the_latched_process_signal_authoritative() {
        let phase = AtomicI32::new(143);
        let result = RunCommandResult::Accepted {
            run_id: RunId::from("run-active"),
            last_sequence: 2,
        };

        assert!(signal_cancel_won(&result, &phase));
        assert_eq!(commit_exec_terminal_signal(&phase), Some(143));
    }
}
