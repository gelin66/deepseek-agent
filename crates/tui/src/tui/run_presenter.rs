//! One-way presentation of canonical run effects into ephemeral TUI state.
//!
//! This module owns no runtime, persistence, transport, or side effects. It
//! consumes the exact canonical event carried by [`ProjectionEffect`] and
//! mutates only render state on [`App`].

use std::time::Instant;

use codewhale_protocol::agent_runtime::{
    DurableControlAction, InteractionId, ModelAccounting, ModelAttemptFailure, ModelErrorCategory,
    ModelOutput, ModelRetryDecision, ModelRetryStopReason,
    ReasoningEffort as CanonicalReasoningEffort, RuntimeEventKind, TerminalState, ToolArguments,
    ToolOutcome, TranscriptEntry, UserInteractionRequest, UserInteractionResponse,
};
use serde_json::Value;

use super::app::{App, ReasoningEffort};
use super::history::{
    GenericToolCell, HistoryCell, ToolStatus, output_looks_like_diff, summarize_tool_args,
    summarize_tool_output,
};
use super::run_projection::{ProjectionEffect, ProjectionEffectKind, UserTranscriptSource};

/// The only presentation decisions that still require the interactive loop.
///
/// Payloads remain the canonical protocol types; this is not a second event
/// schema.
#[derive(Debug, Clone, PartialEq)]
pub enum PresenterAction {
    ShowInteraction(UserInteractionRequest),
    InteractionResolved {
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    },
}

/// Apply one already-ordered projection effect to the TUI.
///
/// A canonical event may change status or transcript display, but it never
/// invokes a model, tool, hook, persistence writer, or workspace probe.
pub fn present_effect(app: &mut App, effect: ProjectionEffect) -> Option<PresenterAction> {
    let source_run_id = effect.run_id;

    match effect.kind {
        ProjectionEffectKind::UserTranscript {
            source: UserTranscriptSource::RunCreated,
            ..
        } if app.runtime_turn_status.as_deref() != Some("in_progress") => None,
        ProjectionEffectKind::UserTranscript { content, .. } => {
            app.add_message(HistoryCell::User { content });
            app.status_message = None;
            None
        }
        ProjectionEffectKind::Canonical(stored) => {
            let stored = *stored;
            debug_assert_eq!(stored.run_id, source_run_id);
            present_canonical_event(app, &source_run_id, stored.event)
        }
    }
}

fn present_reasoning_effort(effort: CanonicalReasoningEffort) -> ReasoningEffort {
    match effort {
        CanonicalReasoningEffort::Off => ReasoningEffort::Off,
        CanonicalReasoningEffort::Auto => ReasoningEffort::Auto,
        CanonicalReasoningEffort::Low => ReasoningEffort::Low,
        CanonicalReasoningEffort::Medium => ReasoningEffort::Medium,
        CanonicalReasoningEffort::High => ReasoningEffort::High,
        CanonicalReasoningEffort::Max => ReasoningEffort::Max,
    }
}

#[allow(clippy::too_many_lines)]
fn present_canonical_event(
    app: &mut App,
    source_run_id: &codewhale_protocol::agent_runtime::RunId,
    event: RuntimeEventKind,
) -> Option<PresenterAction> {
    match event {
        RuntimeEventKind::RunCreated { request } => {
            app.is_loading = true;
            app.turn_started_at = Some(Instant::now());
            if request.parent_run_id.is_none() {
                app.child_agents.begin_root(source_run_id.clone());
                reset_run_display(app);
                rebuild_transcript(app, source_run_id, &request.transcript.entries);
                app.model = request.model.clone();
                app.reasoning_effort = present_reasoning_effort(request.reasoning_effort);
                app.runtime_turn_status = Some("in_progress".to_owned());
                app.status_message = Some("DeepSeek 正在处理…".to_owned());
            } else {
                app.runtime_turn_status = Some("child_in_progress".to_owned());
                app.status_message = Some("子 Agent 正在处理…".to_owned());
            }
            None
        }
        RuntimeEventKind::ContextCompactionCommitted {
            before_tokens,
            after_tokens,
            ..
        } => {
            app.status_message = Some(format!(
                "上下文压缩完成：{before_tokens} → {after_tokens} tokens"
            ));
            None
        }
        RuntimeEventKind::ModelRequestPrepared { .. } => {
            discard_uncommitted_streams(app);
            app.is_loading = true;
            app.runtime_turn_status = Some("in_progress".to_owned());
            app.status_message = Some("正在准备 DeepSeek 请求…".to_owned());
            None
        }
        RuntimeEventKind::ModelRequestInFlight { .. } => {
            app.is_loading = true;
            app.status_message = Some("等待 DeepSeek 响应…".to_owned());
            None
        }
        RuntimeEventKind::ModelRequestFailed { failure, retry, .. } => {
            discard_uncommitted_streams(app);
            app.status_message = Some(format!(
                "DeepSeek 请求失败：{}；{}",
                model_failure_label(&failure),
                model_retry_label(&retry)
            ));
            None
        }
        RuntimeEventKind::ModelResponseCommitted {
            output, accounting, ..
        } => {
            app.session.last_prompt_tokens = Some(narrow_u64(output.usage.input_tokens));
            project_accounting(app, &accounting);
            reconcile_model_output(app, &output);
            app.status_message = Some("DeepSeek 响应已确认".to_owned());
            None
        }
        RuntimeEventKind::ContentDelta { delta, .. } => {
            append_content_delta(app, &delta);
            None
        }
        RuntimeEventKind::ReasoningDelta { delta, .. } => {
            append_reasoning_delta(app, &delta);
            None
        }
        RuntimeEventKind::ToolPrepared { invocation, .. } => {
            present_tool_prepared(
                app,
                &invocation.call_id,
                &invocation.name,
                &invocation.arguments,
            );
            app.status_message = Some(format!("工具已准备：{}", invocation.name));
            None
        }
        RuntimeEventKind::ToolExecutionStarted { operation_id } => {
            app.status_message = Some(format!("工具正在执行：{}", operation_id.0));
            None
        }
        RuntimeEventKind::InteractionRequested { request } => {
            app.status_message = Some("需要你的确认或输入".to_owned());
            Some(PresenterAction::ShowInteraction(request))
        }
        RuntimeEventKind::InteractionResolved {
            interaction_id,
            response,
            ..
        } => {
            app.status_message = Some("交互已处理".to_owned());
            Some(PresenterAction::InteractionResolved {
                interaction_id,
                response,
            })
        }
        RuntimeEventKind::ToolOutcomeCommitted {
            call_id,
            name,
            outcome,
            ..
        } => {
            present_tool_outcome(app, &call_id, &name, &outcome);
            app.status_message = Some(if outcome.is_success() {
                format!("工具已完成：{name}")
            } else {
                format!("工具未成功：{name}")
            });
            None
        }
        RuntimeEventKind::WorkspaceObserved { .. } => None,
        RuntimeEventKind::CompletionProposed { .. } => {
            app.status_message = Some("DeepSeek 已提交完成候选，等待 Host 验收…".to_owned());
            None
        }
        RuntimeEventKind::HostVerificationPrepared { verifier, .. } => {
            app.status_message = Some(format!("Host 正在准备验收：{}", verifier.verifier_id));
            None
        }
        RuntimeEventKind::HostVerificationStarted { .. } => {
            app.status_message = Some("Host 正在执行确定性验收…".to_owned());
            None
        }
        RuntimeEventKind::HostVerificationCommitted {
            receipt, outcome, ..
        } => {
            app.status_message = Some(if receipt.is_some() {
                "Host 验收通过，证据回执已提交".to_owned()
            } else {
                format!("Host 验收未通过：{}", outcome.content)
            });
            None
        }
        RuntimeEventKind::CompletionRejected { rejection } => {
            app.status_message = Some(format!("完成候选被拒绝：{}", rejection.reason));
            None
        }
        RuntimeEventKind::ChildStarted {
            call_id,
            child_run_id,
            depth,
        } => {
            app.child_agents.record_started(
                source_run_id.clone(),
                call_id,
                child_run_id.clone(),
                depth,
            );
            app.add_message(HistoryCell::System {
                content: format!("子 Agent 已启动：{}（深度 {depth}）", child_run_id.0),
            });
            None
        }
        RuntimeEventKind::ChildFinished {
            call_id,
            outcome,
            handoff_content,
            ..
        } => {
            app.child_agents.record_finished(
                source_run_id.clone(),
                call_id,
                &outcome,
                &handoff_content,
            );
            let status = terminal_label(&outcome.terminal);
            let content = if handoff_content.trim().is_empty() {
                format!("子 Agent 已结束：{status}")
            } else {
                format!("子 Agent 已结束：{status}\n{handoff_content}")
            };
            app.add_message(HistoryCell::System { content });
            None
        }
        RuntimeEventKind::SteerQueued { .. } => {
            app.status_message = Some("追加指令已排队".to_owned());
            None
        }
        RuntimeEventKind::SteerApplied { .. } => {
            // The paired UserTranscript effect is the sole transcript write.
            app.status_message = Some("追加指令已应用".to_owned());
            None
        }
        RuntimeEventKind::ControlRequested { action, .. } => {
            app.status_message = Some(format!("控制请求已受理：{}", control_action_label(action)));
            None
        }
        RuntimeEventKind::Terminal { outcome } => {
            finish_terminal(app, &outcome.terminal, &outcome.accounting);
            None
        }
    }
}

fn reset_run_display(app: &mut App) {
    app.clear_history();
    app.tool_cells.clear();
    app.streaming_message_index = None;
}

fn rebuild_transcript(
    app: &mut App,
    source_run_id: &codewhale_protocol::agent_runtime::RunId,
    entries: &[TranscriptEntry],
) {
    for entry in entries {
        match entry {
            // The system prompt is execution context, not chat transcript.
            TranscriptEntry::System { .. } => {}
            TranscriptEntry::User { content } => {
                app.add_message(HistoryCell::User {
                    content: content.clone(),
                });
            }
            TranscriptEntry::Assistant {
                content,
                reasoning_content,
                tool_calls,
            } => {
                if let Some(reasoning) = reasoning_content
                    && !reasoning.is_empty()
                {
                    app.add_message(HistoryCell::Thinking {
                        content: reasoning.clone(),
                        streaming: false,
                    });
                }
                if let Some(content) = content
                    && !content.is_empty()
                {
                    app.add_message(HistoryCell::Assistant {
                        content: content.clone(),
                        streaming: false,
                    });
                }
                for call in tool_calls {
                    present_tool_prepared(app, &call.id, &call.name, &call.arguments);
                }
            }
            TranscriptEntry::Tool {
                call_id,
                name,
                outcome,
            } => present_tool_outcome(app, call_id, name, outcome),
            TranscriptEntry::ChildOutcome {
                call_id,
                child_run_id,
                outcome,
                handoff_content,
            } => {
                app.child_agents.record_finished(
                    source_run_id.clone(),
                    call_id.clone(),
                    outcome,
                    handoff_content,
                );
                let status = terminal_label(&outcome.terminal);
                let content = if handoff_content.trim().is_empty() {
                    format!("子 Agent {}：{status}", child_run_id.0)
                } else {
                    format!("子 Agent {}：{status}\n{handoff_content}", child_run_id.0)
                };
                app.add_message(HistoryCell::System { content });
            }
        }
    }
}

fn append_content_delta(app: &mut App, delta: &str) {
    if delta.is_empty() {
        return;
    }
    let index = app.streaming_message_index.unwrap_or_else(|| {
        app.add_message(HistoryCell::Assistant {
            content: String::new(),
            streaming: true,
        });
        let index = app.history.len().saturating_sub(1);
        app.streaming_message_index = Some(index);
        index
    });
    if let Some(HistoryCell::Assistant { content, .. }) = app.history.get_mut(index) {
        content.push_str(delta);
        app.bump_history_cell(index);
    }
}

fn append_reasoning_delta(app: &mut App, delta: &str) {
    if delta.is_empty() {
        return;
    }
    if let Some(index) = app.history.iter().rposition(|cell| {
        matches!(
            cell,
            HistoryCell::Thinking {
                streaming: true,
                ..
            }
        )
    }) && let Some(HistoryCell::Thinking { content, .. }) = app.history.get_mut(index)
    {
        content.push_str(delta);
        app.bump_history_cell(index);
        return;
    }
    app.add_message(HistoryCell::Thinking {
        content: delta.to_owned(),
        streaming: true,
    });
}

fn discard_uncommitted_streams(app: &mut App) {
    if let Some(index) = app.streaming_message_index.take()
        && index == app.history.len().saturating_sub(1)
    {
        let _ = app.pop_history();
    }
    if matches!(
        app.history.last(),
        Some(HistoryCell::Thinking {
            streaming: true,
            ..
        })
    ) {
        let _ = app.pop_history();
    }
}

fn reconcile_model_output(app: &mut App, output: &ModelOutput) {
    discard_uncommitted_streams(app);
    if let Some(reasoning) = output.reasoning_content.as_ref()
        && !reasoning.is_empty()
    {
        app.add_message(HistoryCell::Thinking {
            content: reasoning.clone(),
            streaming: false,
        });
    }
    if !output.content.is_empty() {
        app.add_message(HistoryCell::Assistant {
            content: output.content.clone(),
            streaming: false,
        });
    }
}

fn tool_input(arguments: &ToolArguments) -> Value {
    arguments
        .parsed
        .clone()
        .unwrap_or_else(|| Value::String(arguments.raw.clone()))
}

fn present_tool_prepared(app: &mut App, id: &str, name: &str, arguments: &ToolArguments) {
    let input = tool_input(arguments);
    let index = app.history.len();
    app.add_message(HistoryCell::Tool(GenericToolCell {
        name: name.to_owned(),
        status: ToolStatus::Running,
        input_summary: summarize_tool_args(&input),
        output: None,
        prompts: None,
        output_summary: None,
        is_diff: false,
    }));
    app.tool_cells.insert(id.to_owned(), index);
}

fn present_tool_outcome(app: &mut App, id: &str, name: &str, outcome: &ToolOutcome) {
    let status = if outcome.is_success() {
        ToolStatus::Success
    } else {
        ToolStatus::Failed
    };
    let output = (!outcome.content.is_empty()).then(|| outcome.content.clone());
    let summary = output.as_deref().map(summarize_tool_output);
    let is_diff = output.as_deref().is_some_and(output_looks_like_diff);

    if let Some(index) = app.tool_cells.remove(id) {
        if let Some(HistoryCell::Tool(cell)) = app.history.get_mut(index) {
            cell.status = status;
            cell.output = output.clone();
            cell.output_summary = summary;
            cell.is_diff = is_diff;
            app.bump_history_cell(index);
        }
        return;
    }

    app.add_message(HistoryCell::Tool(GenericToolCell {
        name: name.to_owned(),
        status,
        input_summary: None,
        output: output.clone(),
        prompts: None,
        output_summary: summary,
        is_diff,
    }));
}

fn finish_terminal(app: &mut App, terminal: &TerminalState, accounting: &ModelAccounting) {
    finalize_streaming_cells(app);

    project_accounting(app, accounting);
    app.is_loading = false;
    app.turn_started_at = None;
    app.runtime_turn_status = Some(terminal_runtime_status(terminal).to_owned());
    app.status_message = Some(format!("运行已结束：{}", terminal_label(terminal)));
}

fn finalize_streaming_cells(app: &mut App) {
    if let Some(index) = app.streaming_message_index.take()
        && let Some(HistoryCell::Assistant { streaming, .. }) = app.history.get_mut(index)
    {
        *streaming = false;
        app.bump_history_cell(index);
    }
    if let Some(index) = app.history.iter().rposition(|cell| {
        matches!(
            cell,
            HistoryCell::Thinking {
                streaming: true,
                ..
            }
        )
    }) && let Some(HistoryCell::Thinking { streaming, .. }) = app.history.get_mut(index)
    {
        *streaming = false;
        app.bump_history_cell(index);
    }
}

fn project_accounting(app: &mut App, accounting: &ModelAccounting) {
    app.session.total_cost_usd = accounting.cost_nanousd as f64 / 1_000_000_000.0;
    app.session.total_cost_cny = accounting.cost_nanocny as f64 / 1_000_000_000.0;
}

fn narrow_u64(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn terminal_runtime_status(terminal: &TerminalState) -> &'static str {
    match terminal {
        TerminalState::Completed { .. } => "completed",
        TerminalState::Blocked { .. } => "blocked",
        TerminalState::Failed { .. } => "failed",
        TerminalState::Cancelled => "cancelled",
        TerminalState::Interrupted => "interrupted",
        TerminalState::RecoveryRequired { .. } => "recovery_required",
    }
}

fn terminal_label(terminal: &TerminalState) -> &'static str {
    match terminal {
        TerminalState::Completed { .. } => "已完成",
        TerminalState::Blocked { .. } => "已阻塞",
        TerminalState::Failed { .. } => "失败",
        TerminalState::Cancelled => "已取消",
        TerminalState::Interrupted => "已中断",
        TerminalState::RecoveryRequired { .. } => "需要恢复",
    }
}

fn model_failure_label(failure: &ModelAttemptFailure) -> String {
    format!(
        "{}（代码：{}；类别：{}）",
        failure.message,
        failure.code,
        model_error_category_label(failure.category)
    )
}

fn model_error_category_label(category: ModelErrorCategory) -> &'static str {
    match category {
        ModelErrorCategory::Transport => "传输错误",
        ModelErrorCategory::Timeout => "请求超时",
        ModelErrorCategory::StreamStall => "流式响应停滞",
        ModelErrorCategory::RateLimit => "请求限流",
        ModelErrorCategory::Authentication => "身份验证失败",
        ModelErrorCategory::Protocol => "协议错误",
        ModelErrorCategory::Service => "服务错误",
        ModelErrorCategory::Cancelled => "请求已取消",
        ModelErrorCategory::Unknown => "未知错误",
    }
}

fn model_retry_label(retry: &ModelRetryDecision) -> String {
    match retry {
        ModelRetryDecision::Stop { reason } => {
            format!("停止重试：{}", model_retry_stop_reason_label(*reason))
        }
        ModelRetryDecision::Retry { prepared } => {
            format!("将重试（尝试 ID：{}）", prepared.attempt_id.0)
        }
    }
}

fn model_retry_stop_reason_label(reason: ModelRetryStopReason) -> &'static str {
    match reason {
        ModelRetryStopReason::ActionableOutput => "已收到可执行输出",
        ModelRetryStopReason::NotRetryable => "错误不可重试",
        ModelRetryStopReason::FailureChanged => "失败类型已变化",
        ModelRetryStopReason::RetryLimitReached => "已达到重试上限",
        ModelRetryStopReason::ModelRequestBudgetExceeded => "已超出模型请求预算",
    }
}

fn control_action_label(action: DurableControlAction) -> &'static str {
    match action {
        DurableControlAction::Interrupt => "中断",
        DurableControlAction::Cancel => "取消",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use codewhale_protocol::agent_runtime::{
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION, AgentActor, AgentOutcome, AttemptId, CommandId,
        DurableControlAction, ModelAccounting, ModelFinishReason, ModelOutput, ModelRequest,
        ModelToolCall, OperationId, PreparedModelRetry, ReasoningEffort, RecoveryAmbiguity,
        RecoveryAmbiguityPhase, RunId, RunRequest, RuntimeEventId, RuntimeFailure,
        StoredRuntimeEvent, SystemPrompt, TerminalState, ToolInvocation, TranscriptEntry, Usage,
        WorkspaceAccess,
    };
    use codewhale_protocol::task::{
        AcceptanceId, AcceptanceSatisfaction, CompletionCandidateId, CompletionDecision,
        TaskContract, TaskDefinition, TaskGenerationId, WorkspaceRevision, WorkspaceState,
    };

    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
    use crate::tui::run_projection::CanonicalRunProjection;

    fn app() -> App {
        App::new(
            TuiOptions {
                model: "deepseek-v4-pro".to_owned(),
                workspace: PathBuf::from("."),
                config_path: None,
                allow_shell: false,
                use_alt_screen: true,
                use_mouse_capture: false,
                use_bracketed_paste: true,
                max_subagents: 1,
                skills_dir: PathBuf::from("."),
                mcp_config_path: PathBuf::from("mcp.json"),
                skip_onboarding: false,
                yolo: false,
                resume_session_id: None,
                initial_input: None,
            },
            &Config::default(),
        )
    }

    fn stored(run_id: &RunId, sequence: u64, event: RuntimeEventKind) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: run_id.clone(),
            parent_run_id: None,
            event_id: RuntimeEventId(format!("event-{sequence}")),
            sequence,
            occurred_at_unix_ms: sequence,
            event,
        }
    }

    fn request(run_id: &RunId, objective: &str, system_prompt: &str) -> RunRequest {
        RunRequest::new(
            TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: TaskDefinition::host(objective),
            },
            system_prompt,
        )
    }

    fn completion_decision(run_id: &RunId) -> CompletionDecision {
        CompletionDecision {
            candidate_id: CompletionCandidateId::from(format!("candidate-{}", run_id.0)),
            generation_id: TaskGenerationId::from(run_id.0.clone()),
            workspace_state: WorkspaceState {
                generation: 0,
                revision: WorkspaceRevision::Unknown {
                    reason: "presentation fixture".to_owned(),
                },
            },
            satisfied: vec![AcceptanceSatisfaction::Host {
                acceptance_id: AcceptanceId::from("host"),
            }],
        }
    }

    fn created(run_id: &RunId, transcript: Vec<TranscriptEntry>) -> StoredRuntimeEvent {
        let mut request = request(run_id, "本轮输入", "系统");
        request.transcript.entries = transcript;
        stored(
            run_id,
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        )
    }

    fn committed(run_id: &RunId, sequence: u64, content: &str) -> StoredRuntimeEvent {
        stored(
            run_id,
            sequence,
            RuntimeEventKind::ModelResponseCommitted {
                attempt_id: AttemptId("attempt".to_owned()),
                output: Box::new(ModelOutput {
                    content: content.to_owned(),
                    reasoning_content: Some("最终推理".to_owned()),
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: Usage::default(),
                }),
                accounting: Box::new(ModelAccounting::default()),
            },
        )
    }

    fn terminal(run_id: &RunId, sequence: u64) -> StoredRuntimeEvent {
        terminal_with_state(
            run_id,
            sequence,
            TerminalState::Completed {
                message: "完成".to_owned(),
                decision: completion_decision(run_id),
            },
        )
    }

    fn terminal_with_state(
        run_id: &RunId,
        sequence: u64,
        terminal: TerminalState,
    ) -> StoredRuntimeEvent {
        stored(
            run_id,
            sequence,
            RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: run_id.clone(),
                    parent_run_id: None,
                    terminal,
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 1,
                    runtime_retries: 0,
                    tool_calls: 0,
                }),
            },
        )
    }

    fn child_outcome(
        parent_run_id: &RunId,
        child_run_id: &RunId,
        terminal: TerminalState,
    ) -> AgentOutcome {
        AgentOutcome {
            run_id: child_run_id.clone(),
            parent_run_id: Some(parent_run_id.clone()),
            terminal,
            accounting: ModelAccounting::default(),
            runtime_model_requests: 1,
            runtime_retries: 0,
            tool_calls: 0,
        }
    }

    fn apply_events(app: &mut App, events: Vec<StoredRuntimeEvent>) {
        let mut projection = CanonicalRunProjection::new();
        for event in events {
            for effect in projection.apply(event).expect("canonical projection") {
                let _ = present_effect(app, effect);
            }
        }
    }

    fn transcript(app: &App) -> Vec<String> {
        app.history
            .iter()
            .filter_map(|cell| match cell {
                HistoryCell::User { content } => Some(format!("user:{content}")),
                HistoryCell::Assistant { content, streaming } => {
                    Some(format!("assistant:{streaming}:{content}"))
                }
                HistoryCell::Thinking {
                    content, streaming, ..
                } => Some(format!("thinking:{streaming}:{content}")),
                HistoryCell::System { content } => Some(format!("system:{content}")),
                HistoryCell::Tool(cell) => Some(format!("tool:{:?}", cell.status)),
                HistoryCell::ArchivedContext { .. } => None,
            })
            .collect()
    }

    #[test]
    fn root_run_created_is_the_single_source_for_resolved_auto_route_display() {
        let run_id = RunId::from("auto-route");
        let mut request = request(&run_id, "实现功能", "系统");
        request.model = "deepseek-v4-pro".to_owned();
        request.reasoning_effort = CanonicalReasoningEffort::Max;
        let event = stored(
            &run_id,
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        );
        let mut app = app();
        app.auto_model = true;
        app.model = "auto".to_owned();
        app.reasoning_effort = crate::tui::app::ReasoningEffort::Auto;

        apply_events(&mut app, vec![event]);

        assert_eq!(app.model, "deepseek-v4-pro");
        assert_eq!(app.effective_model_for_budget(), "deepseek-v4-pro");
        assert_eq!(app.model_display_label(), "auto: deepseek-v4-pro");
        assert_eq!(app.reasoning_effort, crate::tui::app::ReasoningEffort::Max);
    }

    #[test]
    fn cold_replay_matches_live_projection() {
        let run_id = RunId::from("run");
        let events = vec![
            created(
                &run_id,
                vec![TranscriptEntry::User {
                    content: "上一轮".to_owned(),
                }],
            ),
            stored(
                &run_id,
                2,
                RuntimeEventKind::ReasoningDelta {
                    attempt_id: AttemptId("attempt".to_owned()),
                    index: 0,
                    delta: "临时推理".to_owned(),
                },
            ),
            stored(
                &run_id,
                3,
                RuntimeEventKind::ContentDelta {
                    attempt_id: AttemptId("attempt".to_owned()),
                    index: 0,
                    delta: "临时回答".to_owned(),
                },
            ),
            committed(&run_id, 4, "最终回答"),
            terminal(&run_id, 5),
        ];
        let mut live = app();
        let mut projection = CanonicalRunProjection::new();
        for event in events.iter().cloned() {
            for effect in projection.apply(event).unwrap() {
                let _ = present_effect(&mut live, effect);
            }
        }

        let mut replay = app();
        apply_events(&mut replay, events);
        assert_eq!(transcript(&live), transcript(&replay));
        assert_eq!(live.runtime_turn_status, replay.runtime_turn_status);
        assert_eq!(live.is_loading, replay.is_loading);
    }

    #[test]
    fn tool_projection_matches_live_replay_and_transcript_rebuild() {
        let run_id = RunId::from("run");
        let operation_id = OperationId("operation-read-1".to_owned());
        let call_id = "call-read-1".to_owned();
        let name = "read_file".to_owned();
        let arguments = ToolArguments::parse(r#"{"path":"src/lib.rs","line_end":200}"#);
        let output = "第一行\n第二行\n完整工具输出".to_owned();
        let outcome = ToolOutcome::success(output.clone());
        let events = vec![
            created(&run_id, Vec::new()),
            stored(
                &run_id,
                2,
                RuntimeEventKind::ToolPrepared {
                    operation_id: operation_id.clone(),
                    invocation: ToolInvocation {
                        run_id: run_id.clone(),
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    },
                    workspace_access: WorkspaceAccess::ReadOnly,
                },
            ),
            stored(
                &run_id,
                3,
                RuntimeEventKind::ToolOutcomeCommitted {
                    operation_id,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    outcome: Box::new(outcome.clone()),
                    workspace_state: None,
                },
            ),
        ];

        let mut live = app();
        apply_events(&mut live, events.clone());
        let mut replay = app();
        apply_events(&mut replay, events);

        let continued_run_id = RunId::from("continued-run");
        let mut rebuilt_request = request(&continued_run_id, "继续处理", "系统");
        rebuilt_request.transcript.entries = vec![
            TranscriptEntry::Assistant {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ModelToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }],
            },
            TranscriptEntry::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: Box::new(outcome),
            },
        ];
        let rebuilt_event = stored(
            &RunId::from("continued-run"),
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(rebuilt_request),
            },
        );
        let mut rebuilt = app();
        apply_events(&mut rebuilt, vec![rebuilt_event]);

        let snapshot = |app: &App| {
            let cell = app
                .history
                .iter()
                .find_map(|cell| match cell {
                    HistoryCell::Tool(cell) => Some(cell),
                    _ => None,
                })
                .expect("one canonical tool cell");
            (
                cell.name.clone(),
                cell.status,
                cell.input_summary.clone(),
                cell.output.clone(),
                cell.prompts.clone(),
                cell.output_summary.clone(),
                cell.is_diff,
            )
        };
        let expected_input_summary = summarize_tool_args(&tool_input(&arguments));
        let expected = (
            name.clone(),
            ToolStatus::Success,
            expected_input_summary,
            Some(output.clone()),
            None,
            Some(summarize_tool_output(&output)),
            false,
        );

        assert_eq!(snapshot(&live), expected);
        assert_eq!(snapshot(&replay), expected);
        assert_eq!(snapshot(&rebuilt), expected);
    }

    #[test]
    fn run_created_input_is_added_only_by_user_transcript_effect() {
        let run_id = RunId::from("run");
        let mut projection = CanonicalRunProjection::new();
        let effects = projection
            .apply(created(
                &run_id,
                vec![TranscriptEntry::User {
                    content: "历史输入".to_owned(),
                }],
            ))
            .unwrap();
        let mut app = app();

        let _ = present_effect(&mut app, effects[0].clone());
        assert_eq!(
            transcript(&app),
            vec!["user:历史输入"],
            "RunCreated must not duplicate the task objective"
        );
        let _ = present_effect(&mut app, effects[1].clone());
        assert_eq!(transcript(&app), vec!["user:历史输入", "user:本轮输入"]);
    }

    #[test]
    fn model_response_projects_context_size_and_aggregate_cost() {
        let run_id = RunId::from("run");
        let response_usage = Usage {
            input_tokens: 12_345,
            output_tokens: 678,
            cache_hit_tokens: 10_000,
            cache_miss_tokens: 2_345,
            reasoning_replay_tokens: 321,
            ..Usage::default()
        };
        let accounting = ModelAccounting {
            usage: Usage {
                input_tokens: 20_000,
                output_tokens: 1_000,
                cache_hit_tokens: 15_000,
                cache_miss_tokens: 5_000,
                reasoning_replay_tokens: 400,
                ..Usage::default()
            },
            cost_nanousd: 420_000_000,
            cost_nanocny: 2_500_000_000,
            ..ModelAccounting::default()
        };
        let event = stored(
            &run_id,
            2,
            RuntimeEventKind::ModelResponseCommitted {
                attempt_id: AttemptId("attempt".to_owned()),
                output: Box::new(ModelOutput {
                    content: "完成".to_owned(),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    finish_reason: ModelFinishReason::Stop,
                    usage: response_usage,
                }),
                accounting: Box::new(accounting),
            },
        );
        let mut projection = CanonicalRunProjection::new();
        let mut app = app();

        for stored in [created(&run_id, Vec::new()), event] {
            for effect in projection.apply(stored).unwrap() {
                let _ = present_effect(&mut app, effect);
            }
        }

        assert_eq!(app.session.last_prompt_tokens, Some(12_345));
        assert_eq!(app.session.total_cost_usd, 0.42);
        assert_eq!(app.session.total_cost_cny, 2.5);
    }

    #[test]
    fn queued_steer_is_status_only_and_applied_content_is_added_once() {
        let run_id = RunId::from("run");
        let command_id = CommandId::from("steer");
        let mut projection = CanonicalRunProjection::new();
        let mut app = app();
        for effect in projection.apply(created(&run_id, Vec::new())).unwrap() {
            let _ = present_effect(&mut app, effect);
        }
        let before = transcript(&app);
        for effect in projection
            .apply(stored(
                &run_id,
                2,
                RuntimeEventKind::SteerQueued {
                    command_id: command_id.clone(),
                    content: "补充要求".to_owned(),
                },
            ))
            .unwrap()
        {
            let _ = present_effect(&mut app, effect);
        }
        assert_eq!(transcript(&app), before);

        for effect in projection
            .apply(stored(
                &run_id,
                3,
                RuntimeEventKind::SteerApplied {
                    command_id,
                    content: "补充要求".to_owned(),
                },
            ))
            .unwrap()
        {
            let _ = present_effect(&mut app, effect);
        }
        assert_eq!(
            transcript(&app)
                .iter()
                .filter(|line| line.as_str() == "user:补充要求")
                .count(),
            1
        );
    }

    #[test]
    fn committed_output_replaces_deltas_instead_of_duplicating_them() {
        let run_id = RunId::from("run");
        let mut streamed = app();
        apply_events(
            &mut streamed,
            vec![
                created(&run_id, Vec::new()),
                stored(
                    &run_id,
                    2,
                    RuntimeEventKind::ReasoningDelta {
                        attempt_id: AttemptId("attempt".to_owned()),
                        index: 0,
                        delta: "临时推理".to_owned(),
                    },
                ),
                stored(
                    &run_id,
                    3,
                    RuntimeEventKind::ContentDelta {
                        attempt_id: AttemptId("attempt".to_owned()),
                        index: 0,
                        delta: "临时回答".to_owned(),
                    },
                ),
                committed(&run_id, 4, "最终回答"),
            ],
        );

        let other_run = RunId::from("other-run");
        let mut non_streamed = app();
        apply_events(
            &mut non_streamed,
            vec![
                created(&other_run, Vec::new()),
                committed(&other_run, 2, "最终回答"),
            ],
        );
        assert_eq!(transcript(&streamed), transcript(&non_streamed));
    }

    #[test]
    fn only_terminal_unlocks_the_composer() {
        let run_id = RunId::from("run");
        let mut non_terminal_app = app();
        apply_events(
            &mut non_terminal_app,
            vec![
                created(&run_id, Vec::new()),
                stored(
                    &run_id,
                    2,
                    RuntimeEventKind::ControlRequested {
                        command_id: CommandId::from("interrupt"),
                        action: DurableControlAction::Interrupt,
                    },
                ),
                committed(&run_id, 3, "完成"),
            ],
        );
        assert!(
            non_terminal_app.is_loading,
            "control and model commit are non-terminal"
        );

        let mut projection = CanonicalRunProjection::new();
        let events = vec![
            created(&run_id, Vec::new()),
            stored(
                &run_id,
                2,
                RuntimeEventKind::ControlRequested {
                    command_id: CommandId::from("interrupt"),
                    action: DurableControlAction::Interrupt,
                },
            ),
            committed(&run_id, 3, "完成"),
            terminal(&run_id, 4),
        ];
        let mut terminal_app = app();
        for event in events {
            for effect in projection.apply(event).unwrap() {
                let _ = present_effect(&mut terminal_app, effect);
            }
        }
        assert!(!terminal_app.is_loading);
        assert_eq!(
            terminal_app.runtime_turn_status.as_deref(),
            Some("completed")
        );
    }

    #[test]
    fn terminal_without_tool_outcome_does_not_invent_tool_failure() {
        let run_id = RunId::from("run");
        let terminal_states = [
            (
                "completed",
                TerminalState::Completed {
                    message: "完成".to_owned(),
                    decision: completion_decision(&run_id),
                },
            ),
            (
                "blocked",
                TerminalState::Blocked {
                    reason: "等待外部输入".to_owned(),
                },
            ),
            (
                "failed",
                TerminalState::Failed {
                    failure: RuntimeFailure::Join {
                        message: "worker failed".to_owned(),
                    },
                },
            ),
            ("cancelled", TerminalState::Cancelled),
            ("interrupted", TerminalState::Interrupted),
            (
                "recovery_required",
                TerminalState::RecoveryRequired {
                    ambiguity: RecoveryAmbiguity {
                        phase: RecoveryAmbiguityPhase::ToolExecution,
                        action_id: "operation-still-running".to_owned(),
                        message: "工具结果未知".to_owned(),
                    },
                },
            ),
        ];

        for (label, terminal_state) in terminal_states {
            let call_id = format!("call-still-running-{label}");
            let mut terminal_app = app();
            apply_events(
                &mut terminal_app,
                vec![
                    created(&run_id, Vec::new()),
                    stored(
                        &run_id,
                        2,
                        RuntimeEventKind::ToolPrepared {
                            operation_id: OperationId("operation-still-running".to_owned()),
                            invocation: ToolInvocation {
                                run_id: run_id.clone(),
                                call_id: call_id.clone(),
                                name: "read_file".to_owned(),
                                arguments: ToolArguments::parse(r#"{"path":"src/lib.rs"}"#),
                            },
                            workspace_access: WorkspaceAccess::ReadOnly,
                        },
                    ),
                    terminal_with_state(&run_id, 3, terminal_state),
                ],
            );

            let tool = terminal_app
                .history
                .iter()
                .find_map(|cell| match cell {
                    HistoryCell::Tool(tool) => Some(tool),
                    _ => None,
                })
                .expect("prepared tool remains visible");
            assert_eq!(tool.status, ToolStatus::Running, "terminal={label}");
            assert!(
                terminal_app.tool_cells.contains_key(&call_id),
                "terminal={label}"
            );
        }
    }

    #[test]
    fn model_failure_and_retry_status_do_not_expose_debug_enum_names() {
        let run_id = RunId::from("failure-run");
        let failure = ModelAttemptFailure {
            code: "deepseek_transport".to_owned(),
            category: ModelErrorCategory::Transport,
            message: "connection reset by peer".to_owned(),
            retryable: true,
            actionable_output: false,
        };
        let retry = ModelRetryDecision::Stop {
            reason: ModelRetryStopReason::RetryLimitReached,
        };
        let mut app = app();
        apply_events(
            &mut app,
            vec![
                created(&run_id, Vec::new()),
                stored(
                    &run_id,
                    2,
                    RuntimeEventKind::ModelRequestFailed {
                        attempt_id: AttemptId("attempt-1".to_owned()),
                        failure,
                        accounting: Box::new(ModelAccounting::default()),
                        retry,
                    },
                ),
            ],
        );

        let status = app.status_message.expect("失败状态");
        assert_eq!(
            status,
            "DeepSeek 请求失败：connection reset by peer（代码：deepseek_transport；类别：传输错误）；停止重试：已达到重试上限"
        );
        for leaked in [
            "ModelAttemptFailure",
            "Transport",
            "Stop",
            "RetryLimitReached",
        ] {
            assert!(!status.contains(leaked), "泄漏了协议枚举名：{leaked}");
        }
    }

    #[test]
    fn retry_and_control_actions_have_explicit_chinese_labels() {
        let retry = ModelRetryDecision::Retry {
            prepared: PreparedModelRetry {
                attempt_id: AttemptId("attempt-retry-2".to_owned()),
                request: Box::new(ModelRequest {
                    run_id: RunId::from("retry-run"),
                    parent_run_id: None,
                    actor: AgentActor::default(),
                    model: "deepseek-chat".to_owned(),
                    system_prompt: SystemPrompt::from_text("系统"),
                    messages: Vec::new(),
                    tools: Vec::new(),
                    reasoning_effort: ReasoningEffort::Auto,
                    max_output_tokens: None,
                    streaming: true,
                    request_number: 2,
                    attempt: 2,
                }),
            },
        };
        assert_eq!(
            model_retry_label(&retry),
            "将重试（尝试 ID：attempt-retry-2）"
        );
        assert_eq!(
            control_action_label(DurableControlAction::Interrupt),
            "中断"
        );
        assert_eq!(control_action_label(DurableControlAction::Cancel), "取消");
    }

    #[test]
    fn canonical_child_events_are_the_live_display_truth() {
        let root = RunId::from("root");
        let child = RunId::from("child");
        let mut app = app();
        apply_events(
            &mut app,
            vec![
                created(&root, Vec::new()),
                stored(
                    &root,
                    2,
                    RuntimeEventKind::ChildStarted {
                        call_id: "call-child".to_owned(),
                        child_run_id: child.clone(),
                        depth: 2,
                    },
                ),
            ],
        );

        assert_eq!(
            app.child_agents
                .rows()
                .iter()
                .filter(|row| row.is_active())
                .count(),
            1
        );
        let row = &app.child_agents.rows()[0];
        assert_eq!(row.parent_run_id, root);
        assert_eq!(row.call_id, "call-child");
        assert_eq!(row.child_run_id, child);
        assert_eq!(row.depth, 2);
        assert!(row.terminal.is_none());

        let child_note = app
            .history
            .iter()
            .find(|cell| matches!(cell, HistoryCell::System { .. }))
            .expect("canonical child event produces one system note");
        let note_lines = child_note.lines(80);
        assert_eq!(
            note_lines[0].spans[0].content.as_ref(),
            "说明",
            "canonical system chrome uses the fixed Chinese title"
        );
        assert!(
            note_lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.content.contains("child")),
            "the canonical child id remains unchanged"
        );

        apply_events(
            &mut app,
            vec![created(&RunId::from("unrelated-projection"), Vec::new())],
        );
        assert!(
            app.child_agents.rows().iter().all(|row| !row.is_active()),
            "a new root must not retain an active child from the previous root"
        );
    }

    #[test]
    fn concurrent_children_finish_by_identity_not_finish_order() {
        let root = RunId::from("root");
        let child_a = RunId::from("child-a");
        let child_b = RunId::from("child-b");
        let mut app = app();
        apply_events(
            &mut app,
            vec![
                created(&root, Vec::new()),
                stored(
                    &root,
                    2,
                    RuntimeEventKind::ChildStarted {
                        call_id: "call-a".to_owned(),
                        child_run_id: child_a.clone(),
                        depth: 1,
                    },
                ),
                stored(
                    &root,
                    3,
                    RuntimeEventKind::ChildStarted {
                        call_id: "call-b".to_owned(),
                        child_run_id: child_b.clone(),
                        depth: 1,
                    },
                ),
                stored(
                    &root,
                    4,
                    RuntimeEventKind::ChildFinished {
                        call_id: "call-b".to_owned(),
                        outcome: Box::new(child_outcome(
                            &root,
                            &child_b,
                            TerminalState::Completed {
                                message: "B 完成".to_owned(),
                                decision: completion_decision(&child_b),
                            },
                        )),
                        accounting: Box::new(ModelAccounting::default()),
                        handoff_content: "B 证据".to_owned(),
                    },
                ),
                stored(
                    &root,
                    5,
                    RuntimeEventKind::ChildFinished {
                        call_id: "call-a".to_owned(),
                        outcome: Box::new(child_outcome(
                            &root,
                            &child_a,
                            TerminalState::Failed {
                                failure: codewhale_protocol::agent_runtime::RuntimeFailure::Join {
                                    message: "A 失败".to_owned(),
                                },
                            },
                        )),
                        accounting: Box::new(ModelAccounting::default()),
                        handoff_content: "A 证据".to_owned(),
                    },
                ),
            ],
        );

        assert!(app.child_agents.rows().iter().all(|row| !row.is_active()));
        let rows = app.child_agents.rows();
        assert_eq!(
            rows.iter()
                .map(|row| row.child_run_id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["child-a", "child-b"],
            "stable start order must survive reverse completion"
        );
        assert!(matches!(
            rows[0].terminal,
            Some(TerminalState::Failed { .. })
        ));
        assert_eq!(rows[0].handoff_content.as_deref(), Some("A 证据"));
        assert!(matches!(
            rows[1].terminal,
            Some(TerminalState::Completed { .. })
        ));
        assert_eq!(rows[1].handoff_content.as_deref(), Some("B 证据"));
    }

    #[test]
    fn transcript_child_outcome_rebuilds_a_settled_continuation_row() {
        let continuation = RunId::from("continuation");
        let child = RunId::from("historical-child");
        let transcript = vec![TranscriptEntry::ChildOutcome {
            call_id: "historical-call".to_owned(),
            child_run_id: child.clone(),
            outcome: Box::new(child_outcome(
                &RunId::from("source-root"),
                &child,
                TerminalState::Completed {
                    message: "历史完成".to_owned(),
                    decision: completion_decision(&child),
                },
            )),
            handoff_content: "历史证据".to_owned(),
        }];
        let mut app = app();

        apply_events(&mut app, vec![created(&continuation, transcript)]);

        assert!(app.child_agents.rows().iter().all(|row| !row.is_active()));
        let row = &app.child_agents.rows()[0];
        assert_eq!(
            row.parent_run_id,
            RunId::from("source-root"),
            "the durable AgentOutcome owns the child parent identity"
        );
        assert_eq!(row.call_id, "historical-call");
        assert_eq!(row.child_run_id, child);
        assert!(matches!(
            row.terminal,
            Some(TerminalState::Completed { .. })
        ));
        assert_eq!(row.handoff_content.as_deref(), Some("历史证据"));
    }
}
