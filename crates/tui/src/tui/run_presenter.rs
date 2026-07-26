//! One-way presentation of canonical run effects into ephemeral TUI state.
//!
//! This module owns no runtime, persistence, transport, or side effects. It
//! consumes the exact canonical event carried by [`ProjectionEffect`] and
//! mutates only render state on [`App`].

use std::{borrow::Cow, time::Instant};

use dse_localization::{MessageId, ProductLanguage, tr_in};
use dse_protocol::agent_runtime::{
    DurableControlAction, InteractionId, ModelAccounting, ModelAttemptFailure, ModelErrorCategory,
    ModelOutput, ModelRetryDecision, ModelRetryStopReason,
    ReasoningEffort as CanonicalReasoningEffort, RuntimeEventKind, TerminalState, ToolArguments,
    ToolOutcome, TranscriptEntry, UserInteractionRequest, UserInteractionResponse,
    WriterCleanupResult, WriterIntegrationStatus,
};
#[cfg(test)]
use dse_protocol::agent_runtime::{ModelResponseEvidence, WriterCleanupMetadataState};
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
        } if app.run_presentation.root_run_id() != Some(&source_run_id) => None,
        ProjectionEffectKind::UserTranscript { content, .. } => {
            app.add_message(HistoryCell::User { content });
            app.status_message = None;
            None
        }
        ProjectionEffectKind::Canonical(stored) => {
            let stored = *stored;
            debug_assert_eq!(stored.run_id, source_run_id);
            app.run_presentation.apply(&stored);
            present_canonical_event(app, &source_run_id, stored.event)
        }
    }
}

fn present_reasoning_effort(effort: CanonicalReasoningEffort) -> ReasoningEffort {
    match effort {
        CanonicalReasoningEffort::Off => ReasoningEffort::Off,
        CanonicalReasoningEffort::Low => ReasoningEffort::Low,
        CanonicalReasoningEffort::Medium => ReasoningEffort::Medium,
        CanonicalReasoningEffort::High => ReasoningEffort::High,
        CanonicalReasoningEffort::Max => ReasoningEffort::Max,
    }
}

#[allow(clippy::too_many_lines)]
fn present_canonical_event(
    app: &mut App,
    source_run_id: &dse_protocol::agent_runtime::RunId,
    event: RuntimeEventKind,
) -> Option<PresenterAction> {
    let language = app.language;
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
                app.status_message =
                    Some(tr_in(language, MessageId::RunDeepSeekProcessing).into_owned());
            } else {
                app.status_message =
                    Some(tr_in(language, MessageId::RunChildProcessing).into_owned());
            }
            None
        }
        RuntimeEventKind::ContextCompactionCommitted {
            before_tokens,
            after_tokens,
            ..
        } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunContextCompacted)
                    .replace("{before_tokens}", &before_tokens.to_string())
                    .replace("{after_tokens}", &after_tokens.to_string()),
            );
            None
        }
        RuntimeEventKind::ModelRequestPrepared { .. } => {
            discard_uncommitted_streams(app);
            app.is_loading = true;
            app.status_message =
                Some(tr_in(language, MessageId::RunModelRequestPreparing).into_owned());
            None
        }
        RuntimeEventKind::ModelRequestInFlight { .. } => {
            app.is_loading = true;
            app.status_message =
                Some(tr_in(language, MessageId::RunModelResponseWaiting).into_owned());
            None
        }
        RuntimeEventKind::ModelRequestFailed { failure, retry, .. } => {
            discard_uncommitted_streams(app);
            app.status_message = Some(
                tr_in(language, MessageId::RunModelRequestFailed)
                    .replace("{failure}", &model_failure_label(language, &failure))
                    .replace("{retry}", &model_retry_label(language, &retry)),
            );
            None
        }
        RuntimeEventKind::ModelResponseCommitted {
            output, accounting, ..
        } => {
            app.session.last_prompt_tokens = Some(narrow_u64(output.usage.input_tokens));
            project_accounting(app, &accounting);
            reconcile_model_output(app, &output);
            app.status_message =
                Some(tr_in(language, MessageId::RunModelResponseCommitted).into_owned());
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
            app.status_message = Some(
                tr_in(language, MessageId::RunToolPrepared).replace("{name}", &invocation.name),
            );
            None
        }
        RuntimeEventKind::ToolAuthorizationCommitted { .. } => None,
        RuntimeEventKind::ToolExecutionStarted { operation_id } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunToolExecuting)
                    .replace("{operation_id}", &operation_id.0),
            );
            None
        }
        RuntimeEventKind::InteractionRequested { request } => {
            app.status_message =
                Some(tr_in(language, MessageId::RunInteractionRequired).into_owned());
            Some(PresenterAction::ShowInteraction(request))
        }
        RuntimeEventKind::InteractionResolved {
            interaction_id,
            response,
            ..
        } => {
            app.status_message =
                Some(tr_in(language, MessageId::RunInteractionResolved).into_owned());
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
                tr_in(language, MessageId::RunToolCompleted).replace("{name}", &name)
            } else {
                tr_in(language, MessageId::RunToolFailed).replace("{name}", &name)
            });
            None
        }
        RuntimeEventKind::WorkspaceObserved { .. } => None,
        RuntimeEventKind::CompletionProposed { .. } => {
            app.status_message =
                Some(tr_in(language, MessageId::RunCompletionProposed).into_owned());
            None
        }
        RuntimeEventKind::HostVerificationPrepared { verifier, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunHostVerificationPreparing)
                    .replace("{verifier_id}", &verifier.verifier_id),
            );
            None
        }
        RuntimeEventKind::HostVerificationStarted { .. } => {
            app.status_message =
                Some(tr_in(language, MessageId::RunHostVerificationRunning).into_owned());
            None
        }
        RuntimeEventKind::HostVerificationCommitted {
            receipt, outcome, ..
        } => {
            app.status_message = Some(if receipt.is_some() {
                tr_in(language, MessageId::RunHostVerificationPassed).into_owned()
            } else {
                tr_in(language, MessageId::RunHostVerificationFailed)
                    .replace("{content}", &outcome.content)
            });
            None
        }
        RuntimeEventKind::CompletionRejected { rejection } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunCompletionRejected)
                    .replace("{reason}", &rejection.reason),
            );
            None
        }
        RuntimeEventKind::AgentTaskPrepared { task } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterTaskPrepared)
                    .replace("{task_id}", &task.task_id.to_string()),
            );
            None
        }
        RuntimeEventKind::AgentWorkspaceCreated {
            task_id,
            assignment,
            ..
        } => {
            let content = tr_in(language, MessageId::RunWriterWorktreeCreated)
                .replace("{path}", assignment.execution_workspace())
                .replace("{task_id}", &task_id.to_string());
            present_agent_progress(app, content);
            None
        }
        RuntimeEventKind::AgentSealPrepared { task_id, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterSealing)
                    .replace("{task_id}", &task_id.to_string()),
            );
            None
        }
        RuntimeEventKind::AgentSealCommitted {
            task_id,
            final_commit,
            changed_files,
            ..
        } => {
            let content = tr_in(language, MessageId::RunWriterSealed)
                .replace("{file_count}", &changed_files.len().to_string())
                .replace("{commit}", short_git_commit(&final_commit))
                .replace("{task_id}", &task_id.to_string());
            present_agent_progress(app, content);
            None
        }
        RuntimeEventKind::AgentResultCollected { task_id, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterResultCollected)
                    .replace("{task_id}", &task_id.to_string()),
            );
            None
        }
        RuntimeEventKind::AgentIntegrationPrepared { task_id, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterIntegrationPreparing)
                    .replace("{task_id}", &task_id.to_string()),
            );
            None
        }
        RuntimeEventKind::AgentIntegrationStarted { task_id, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterIntegrating)
                    .replace("{task_id}", &task_id.to_string()),
            );
            None
        }
        RuntimeEventKind::AgentIntegrationFailed {
            task_id, status, ..
        } => {
            let content =
                writer_integration_status_message(language, &task_id.to_string(), &status);
            present_agent_progress(app, content);
            None
        }
        RuntimeEventKind::AgentIntegrationCommitted {
            task_id,
            root_head_commit,
            ..
        } => {
            let content = tr_in(language, MessageId::RunWriterIntegrated)
                .replace("{commit}", short_git_commit(&root_head_commit))
                .replace("{task_id}", &task_id.to_string());
            present_agent_progress(app, content);
            None
        }
        RuntimeEventKind::AgentCleanupPrepared { task_id, plan } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunWriterCleaning)
                    .replace("{task_id}", &task_id.to_string())
                    .replace("{phase}", &cleanup_phase_label(language, plan.phase)),
            );
            None
        }
        RuntimeEventKind::AgentCleanupCommitted { task_id, result } => {
            let content = match result {
                WriterCleanupResult::Retained {
                    uncertainty_code, ..
                } => tr_in(language, MessageId::RunWriterRetained)
                    .replace("{task_id}", &task_id.to_string())
                    .replace(
                        "{reason}",
                        &cleanup_uncertainty_label(language, &uncertainty_code),
                    ),
                WriterCleanupResult::Removed { .. } | WriterCleanupResult::AlreadyAbsent => {
                    tr_in(language, MessageId::RunWriterCleaned)
                        .replace("{task_id}", &task_id.to_string())
                }
            };
            present_agent_progress(app, content);
            None
        }
        RuntimeEventKind::ChildStarted {
            call_id,
            child_run_id,
            depth,
            ..
        } => {
            app.child_agents.record_started(
                source_run_id.clone(),
                call_id,
                child_run_id.clone(),
                depth,
            );
            app.add_message(HistoryCell::System {
                content: tr_in(language, MessageId::RunChildStarted)
                    .replace("{child_id}", &child_run_id.0)
                    .replace("{depth}", &depth.to_string()),
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
            let status = terminal_label(language, &outcome.terminal);
            let content = if handoff_content.trim().is_empty() {
                tr_in(language, MessageId::RunChildEnded).replace("{status}", &status)
            } else {
                tr_in(language, MessageId::RunChildEndedHandoff)
                    .replace("{status}", &status)
                    .replace("{handoff}", &handoff_content)
            };
            app.add_message(HistoryCell::System { content });
            None
        }
        RuntimeEventKind::SteerQueued { .. } => {
            app.status_message = Some(tr_in(language, MessageId::RunSteerQueued).into_owned());
            None
        }
        RuntimeEventKind::SteerApplied { .. } => {
            // The paired UserTranscript effect is the sole transcript write.
            app.status_message = Some(tr_in(language, MessageId::RunSteerApplied).into_owned());
            None
        }
        RuntimeEventKind::ControlRequested { action, .. } => {
            app.status_message = Some(
                tr_in(language, MessageId::RunControlAccepted)
                    .replace("{action}", &control_action_label(language, action)),
            );
            None
        }
        RuntimeEventKind::Terminal { outcome } => {
            finish_terminal(app, &outcome.terminal, &outcome.accounting);
            None
        }
    }
}

fn present_agent_progress(app: &mut App, content: String) {
    app.status_message = Some(content.clone());
    app.add_message(HistoryCell::System { content });
}

fn short_git_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn cleanup_phase_label(
    language: ProductLanguage,
    phase: dse_protocol::agent_runtime::WriterCleanupPhase,
) -> Cow<'static, str> {
    use dse_protocol::agent_runtime::WriterCleanupPhase;
    match phase {
        WriterCleanupPhase::Binding => tr_in(language, MessageId::RunCleanupPhaseBinding),
        WriterCleanupPhase::Child => tr_in(language, MessageId::RunCleanupPhaseChild),
        WriterCleanupPhase::Seal => tr_in(language, MessageId::RunCleanupPhaseSeal),
        WriterCleanupPhase::Integration => tr_in(language, MessageId::RunCleanupPhaseIntegration),
        WriterCleanupPhase::PostIntegration => {
            tr_in(language, MessageId::RunCleanupPhasePostIntegration)
        }
    }
}

fn cleanup_uncertainty_label(language: ProductLanguage, code: &str) -> Cow<'static, str> {
    match code {
        "writer_cleanup_scope_changed" => tr_in(language, MessageId::RunCleanupScopeChanged),
        "writer_cleanup_owner_changed" => tr_in(language, MessageId::RunCleanupOwnerChanged),
        "writer_cleanup_ownership_unknown" | "writer_cleanup_owner_unprovable" => {
            tr_in(language, MessageId::RunCleanupOwnerUnprovable)
        }
        _ => tr_in(language, MessageId::RunCleanupUncertain),
    }
}

fn writer_integration_status_message(
    language: ProductLanguage,
    task_id: &str,
    status: &WriterIntegrationStatus,
) -> String {
    match status {
        WriterIntegrationStatus::Rejected { reason } => {
            tr_in(language, MessageId::RunIntegrationRejected)
                .replace("{reason}", reason)
                .replace("{task_id}", task_id)
        }
        WriterIntegrationStatus::Conflict { reason } => {
            tr_in(language, MessageId::RunIntegrationConflict)
                .replace("{reason}", reason)
                .replace("{task_id}", task_id)
        }
        WriterIntegrationStatus::RecoveryRequired { reason } => {
            tr_in(language, MessageId::RunIntegrationRecovery)
                .replace("{reason}", reason)
                .replace("{task_id}", task_id)
        }
        WriterIntegrationStatus::Integrated { writer_commit, .. } => {
            tr_in(language, MessageId::RunWriterIntegrated)
                .replace("{commit}", short_git_commit(writer_commit))
                .replace("{task_id}", task_id)
        }
        WriterIntegrationStatus::AwaitingHost => {
            tr_in(language, MessageId::RunIntegrationAwaitingHost).replace("{task_id}", task_id)
        }
        WriterIntegrationStatus::NotApplicable => {
            tr_in(language, MessageId::RunIntegrationNotApplicable).replace("{task_id}", task_id)
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
    source_run_id: &dse_protocol::agent_runtime::RunId,
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
                let status = terminal_label(app.language, &outcome.terminal);
                let content = if handoff_content.trim().is_empty() {
                    tr_in(app.language, MessageId::RunChildSummary)
                        .replace("{child_id}", &child_run_id.0)
                        .replace("{status}", &status)
                } else {
                    tr_in(app.language, MessageId::RunChildSummaryHandoff)
                        .replace("{child_id}", &child_run_id.0)
                        .replace("{status}", &status)
                        .replace("{handoff}", handoff_content)
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
    let projected_content = if outcome.is_success() {
        outcome.content.clone()
    } else {
        outcome.model_content()
    };
    let output = (!projected_content.is_empty()).then_some(projected_content);
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
    app.status_message = Some(
        tr_in(app.language, MessageId::RunTerminal)
            .replace("{terminal}", &terminal_label(app.language, terminal)),
    );
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

fn terminal_label(language: ProductLanguage, terminal: &TerminalState) -> Cow<'static, str> {
    match terminal {
        TerminalState::Completed { .. } => tr_in(language, MessageId::RunTerminalCompleted),
        TerminalState::Blocked { .. } => tr_in(language, MessageId::RunTerminalBlocked),
        TerminalState::Failed { .. } => tr_in(language, MessageId::RunTerminalFailed),
        TerminalState::Cancelled => tr_in(language, MessageId::RunTerminalCancelled),
        TerminalState::Interrupted => tr_in(language, MessageId::RunTerminalInterrupted),
        TerminalState::RecoveryRequired { .. } => {
            tr_in(language, MessageId::RunTerminalRecoveryRequired)
        }
    }
}

fn model_failure_label(language: ProductLanguage, failure: &ModelAttemptFailure) -> String {
    tr_in(language, MessageId::RunModelFailureDetail)
        .replace("{message}", &failure.message)
        .replace("{code}", &failure.code)
        .replace(
            "{category}",
            &model_error_category_label(language, failure.category),
        )
}

fn model_error_category_label(
    language: ProductLanguage,
    category: ModelErrorCategory,
) -> Cow<'static, str> {
    match category {
        ModelErrorCategory::Transport => tr_in(language, MessageId::RunModelCategoryTransport),
        ModelErrorCategory::Timeout => tr_in(language, MessageId::RunModelCategoryTimeout),
        ModelErrorCategory::StreamStall => tr_in(language, MessageId::RunModelCategoryStreamStall),
        ModelErrorCategory::RateLimit => tr_in(language, MessageId::RunModelCategoryRateLimit),
        ModelErrorCategory::Authentication => {
            tr_in(language, MessageId::RunModelCategoryAuthentication)
        }
        ModelErrorCategory::Protocol => tr_in(language, MessageId::RunModelCategoryProtocol),
        ModelErrorCategory::Service => tr_in(language, MessageId::RunModelCategoryService),
        ModelErrorCategory::Cancelled => tr_in(language, MessageId::RunModelCategoryCancelled),
        ModelErrorCategory::Unknown => tr_in(language, MessageId::RunModelCategoryUnknown),
    }
}

fn model_retry_label(language: ProductLanguage, retry: &ModelRetryDecision) -> String {
    match retry {
        ModelRetryDecision::Stop { reason } => tr_in(language, MessageId::RunRetryStopped).replace(
            "{reason}",
            &model_retry_stop_reason_label(language, *reason),
        ),
        ModelRetryDecision::Retry { prepared } => tr_in(language, MessageId::RunRetryPrepared)
            .replace("{attempt_id}", &prepared.attempt_id.0),
    }
}

fn model_retry_stop_reason_label(
    language: ProductLanguage,
    reason: ModelRetryStopReason,
) -> Cow<'static, str> {
    match reason {
        ModelRetryStopReason::ActionableOutput => {
            tr_in(language, MessageId::RunRetryActionableOutput)
        }
        ModelRetryStopReason::UnsafeReplay => tr_in(language, MessageId::RunRetryUnsafeReplay),
        ModelRetryStopReason::NotRetryable => tr_in(language, MessageId::RunRetryNotRetryable),
        ModelRetryStopReason::FailureChanged => tr_in(language, MessageId::RunRetryFailureChanged),
        ModelRetryStopReason::RetryLimitReached => tr_in(language, MessageId::RunRetryLimitReached),
        ModelRetryStopReason::ModelRequestBudgetExceeded => {
            tr_in(language, MessageId::RunRetryBudgetExceeded)
        }
    }
}

fn control_action_label(
    language: ProductLanguage,
    action: DurableControlAction,
) -> Cow<'static, str> {
    match action {
        DurableControlAction::Interrupt => tr_in(language, MessageId::RunControlInterrupt),
        DurableControlAction::Cancel => tr_in(language, MessageId::RunControlCancel),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dse_protocol::agent_runtime::{
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION, AgentActor, AgentOutcome, AgentTaskId,
        AgentWorkspaceAccess, AgentWorkspaceAssignment, AttemptId, CommandId, DurableControlAction,
        ModelAccounting, ModelFinishReason, ModelOutput, ModelRequest, ModelToolCall, OperationId,
        PreparedModelRetry, ReasoningEffort, RecoveryAmbiguity, RecoveryAmbiguityPhase, RunId,
        RunRequest, RuntimeEventId, RuntimeFailure, StoredRuntimeEvent, SystemPrompt,
        TerminalState, ToolFailureCode, ToolInvocation, ToolRetryDisposition, ToolSideEffectStatus,
        TranscriptEntry, Usage, WorkspaceAccess, WriterIntegrationStatus, WriterResourceState,
    };
    use dse_protocol::task::{
        AcceptanceId, AcceptanceSatisfaction, CompletionCandidateId, CompletionDecision,
        TaskContract, TaskDefinition, TaskGenerationId, WorkspaceRevision, WorkspaceState,
    };

    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
    use crate::tui::run_projection::CanonicalRunProjection;

    fn app_in(language: ProductLanguage) -> App {
        App::new(
            TuiOptions {
                model: "deepseek-v4-pro".to_owned(),
                language,
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

    fn app() -> App {
        app_in(ProductLanguage::SimplifiedChinese)
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
                    details: Default::default(),
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
            details: Default::default(),
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
    fn root_run_created_is_the_single_source_for_actual_route_display() {
        let run_id = RunId::from("actual-route");
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
        app.model = "deepseek-v4-flash".to_owned();
        app.reasoning_effort = crate::tui::app::ReasoningEffort::High;

        apply_events(&mut app, vec![event]);

        assert_eq!(app.model, "deepseek-v4-pro");
        assert_eq!(app.effective_model_for_budget(), "deepseek-v4-pro");
        assert_eq!(app.model_display_label(), "deepseek-v4-pro");
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
        assert_eq!(live.run_presentation, replay.run_presentation);
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
    fn failed_tool_projection_keeps_typed_recovery_in_live_replay_and_rebuild() {
        let run_id = RunId::from("run-failure");
        let operation_id = OperationId("operation-edit".to_owned());
        let call_id = "call-edit".to_owned();
        let name = "edit_file".to_owned();
        let arguments =
            ToolArguments::parse(r#"{"path":"src/lib.rs","search":"旧","replace":"新"}"#);
        let mut outcome = ToolOutcome::error("文件在读取后发生变化")
            .with_failure_code(ToolFailureCode::StaleRead);
        outcome.side_effect = ToolSideEffectStatus::NotApplied;
        outcome.retry = ToolRetryDisposition::AfterCorrection;
        outcome.validate().unwrap();
        let expected_output = outcome.model_content();
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
                    workspace_access: WorkspaceAccess::MayWrite,
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

        let rebuilt_run_id = RunId::from("continued-failure");
        let mut rebuilt_request = request(&rebuilt_run_id, "继续处理", "系统");
        rebuilt_request.transcript.entries = vec![
            TranscriptEntry::Assistant {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ModelToolCall {
                    id: call_id.clone(),
                    name: name.clone(),
                    arguments,
                }],
            },
            TranscriptEntry::Tool {
                call_id,
                name,
                outcome: Box::new(outcome),
            },
        ];
        let mut rebuilt = app();
        apply_events(
            &mut rebuilt,
            vec![stored(
                &rebuilt_run_id,
                1,
                RuntimeEventKind::RunCreated {
                    request: Box::new(rebuilt_request),
                },
            )],
        );

        let projected_output = |app: &App| {
            app.history.iter().find_map(|cell| match cell {
                HistoryCell::Tool(cell) if cell.name == "edit_file" => cell.output.clone(),
                _ => None,
            })
        };
        assert_eq!(projected_output(&live), Some(expected_output.clone()));
        assert_eq!(projected_output(&replay), Some(expected_output.clone()));
        assert_eq!(projected_output(&rebuilt), Some(expected_output.clone()));
        assert!(expected_output.contains("code=stale_read"));
        assert!(expected_output.contains("side_effect=not_applied"));
        assert!(expected_output.contains("先重新读取"));
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
            terminal_app.run_presentation.phase(),
            crate::tui::run_presentation::RunPresentationPhase::Completed
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
            retry_safe: true,
            actionable_output: false,
            response: ModelResponseEvidence::default(),
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
    fn retry_and_control_actions_follow_the_product_language() {
        let retry = ModelRetryDecision::Retry {
            prepared: PreparedModelRetry {
                attempt_id: AttemptId("attempt-retry-2".to_owned()),
                request: Box::new(ModelRequest {
                    run_id: RunId::from("retry-run"),
                    parent_run_id: None,
                    actor: AgentActor::default(),
                    model: "deepseek-v4-pro".to_owned(),
                    system_prompt: SystemPrompt::from_text("系统"),
                    messages: Vec::new(),
                    tools: Vec::new(),
                    reasoning_effort: ReasoningEffort::High,
                    max_output_tokens: None,
                    streaming: true,
                    request_number: 2,
                    attempt: 2,
                }),
            },
        };
        assert_eq!(
            model_retry_label(ProductLanguage::SimplifiedChinese, &retry),
            "将重试（尝试 ID：attempt-retry-2）"
        );
        assert_eq!(
            control_action_label(
                ProductLanguage::SimplifiedChinese,
                DurableControlAction::Interrupt
            ),
            "中断"
        );
        assert_eq!(
            control_action_label(
                ProductLanguage::SimplifiedChinese,
                DurableControlAction::Cancel
            ),
            "取消"
        );
        assert_eq!(
            model_retry_label(ProductLanguage::English, &retry),
            "Will retry (attempt ID: attempt-retry-2)"
        );
        assert_eq!(
            control_action_label(ProductLanguage::English, DurableControlAction::Cancel),
            "cancel"
        );
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
                        task_id: "task-child".into(),
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
    fn writer_lifecycle_events_render_only_canonical_chinese_progress() {
        let root = RunId::from("root");
        let task_id = AgentTaskId::from("writer-task");
        let mut app = app();
        let workspace_state = WorkspaceState {
            generation: 2,
            revision: WorkspaceRevision::Unknown {
                reason: "presentation fixture".to_owned(),
            },
        };

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentWorkspaceCreated {
                task_id: task_id.clone(),
                assignment: AgentWorkspaceAssignment {
                    access: AgentWorkspaceAccess::IsolatedWrite,
                    root_workspace: "/workspace/root".to_owned(),
                    base_commit: "a".repeat(40),
                    worktree_path: Some("/workspace/worktrees/writer-task".to_owned()),
                    root_branch: Some("deepseek-agent".to_owned()),
                    branch: Some("codex/writer-task".to_owned()),
                    allowed_paths: vec!["crates/tui".to_owned()],
                    owner_token: Some("owner-token".to_owned()),
                },
                writer_workspace_state: workspace_state.clone(),
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入工作树已创建：/workspace/worktrees/writer-task（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentSealCommitted {
                task_id: task_id.clone(),
                final_commit: "1234567890abcdef".to_owned(),
                diff_sha256: "c".repeat(64),
                changed_files: vec!["crates/tui/src/exec_runtime.rs".to_owned()],
                writer_workspace_state_after: workspace_state.clone(),
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入 Agent 变更已封存：1 个文件，提交 1234567890ab（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentIntegrationCommitted {
                task_id: task_id.clone(),
                integration_id: OperationId::from("integration"),
                root_head_commit: "abcdef1234567890".to_owned(),
                root_workspace_state_after: workspace_state.clone(),
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入结果已集成：提交 abcdef123456（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentIntegrationFailed {
                task_id: task_id.clone(),
                integration_id: OperationId::from("integration"),
                status: WriterIntegrationStatus::Conflict {
                    reason: "root HEAD 已变化".to_owned(),
                },
                root_workspace_state: workspace_state.clone(),
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入结果集成冲突：root HEAD 已变化（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentIntegrationFailed {
                task_id: task_id.clone(),
                integration_id: OperationId::from("integration"),
                status: WriterIntegrationStatus::RecoveryRequired {
                    reason: "集成结果不确定".to_owned(),
                },
                root_workspace_state: workspace_state,
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入结果集成需要恢复：集成结果不确定（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task_id.clone(),
                result: WriterCleanupResult::AlreadyAbsent,
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入工作树已清理（任务 writer-task）")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentCleanupCommitted {
                task_id,
                result: WriterCleanupResult::Retained {
                    worktree: WriterResourceState::Retained,
                    branch: WriterResourceState::Retained,
                    metadata: WriterCleanupMetadataState::Clear,
                    uncertainty_code: "writer_cleanup_conflict".to_owned(),
                },
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("写入工作树已保留（任务 writer-task；清理结果不确定，需要人工确认）")
        );
        assert!(
            matches!(
                app.history.last(),
                Some(HistoryCell::System { content })
                    if content == app.status_message.as_deref().unwrap_or_default()
            ),
            "关键进度必须由 canonical 事件即时呈现，不能依赖私有状态"
        );
    }

    #[test]
    fn writer_lifecycle_and_recovery_render_from_the_english_catalog() {
        let root = RunId::from("root");
        let task_id = AgentTaskId::from("writer-task");
        let mut app = app_in(ProductLanguage::English);
        let workspace_state = WorkspaceState {
            generation: 2,
            revision: WorkspaceRevision::Unknown {
                reason: "presentation fixture".to_owned(),
            },
        };

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentWorkspaceCreated {
                task_id: task_id.clone(),
                assignment: AgentWorkspaceAssignment {
                    access: AgentWorkspaceAccess::IsolatedWrite,
                    root_workspace: "/workspace/root".to_owned(),
                    base_commit: "a".repeat(40),
                    worktree_path: Some("/workspace/worktrees/writer-task".to_owned()),
                    root_branch: Some("deepseek-agent".to_owned()),
                    branch: Some("codex/writer-task".to_owned()),
                    allowed_paths: vec!["crates/tui".to_owned()],
                    owner_token: Some("owner-token".to_owned()),
                },
                writer_workspace_state: workspace_state.clone(),
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some("Writer worktree created: /workspace/worktrees/writer-task (task writer-task)")
        );

        let _ = present_canonical_event(
            &mut app,
            &root,
            RuntimeEventKind::AgentIntegrationFailed {
                task_id,
                integration_id: OperationId::from("integration"),
                status: WriterIntegrationStatus::RecoveryRequired {
                    reason: "integration result uncertain".to_owned(),
                },
                root_workspace_state: workspace_state,
            },
        );
        assert_eq!(
            app.status_message.as_deref(),
            Some(
                "Writer-result integration requires recovery: integration result uncertain (task writer-task)"
            )
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
                        task_id: "task-a".into(),
                        call_id: "call-a".to_owned(),
                        child_run_id: child_a.clone(),
                        depth: 1,
                    },
                ),
                stored(
                    &root,
                    3,
                    RuntimeEventKind::ChildStarted {
                        task_id: "task-b".into(),
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
                                failure: dse_protocol::agent_runtime::RuntimeFailure::Join {
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
