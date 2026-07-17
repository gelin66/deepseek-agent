use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use codewhale_context::compaction::{
    ContextCompactionPreparation, effective_context, estimate_projection_tokens,
    prepare_compaction, projection_from_summary,
};
use codewhale_protocol::run_api::{PendingCreationKind, RunCommand};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLease {
    pub run_id: RunId,
    pub epoch: u64,
    pub owner_id: String,
    pub owner_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatedRun {
    pub lease: RunLease,
    pub created: StoredRuntimeEvent,
    pub replay: RunReplay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcquiredRun {
    /// Terminal runs are immutable and need no writer lease.
    pub lease: Option<RunLease>,
    pub replay: RunReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRunRecord {
    pub run_id: RunId,
    pub purpose: RunPurpose,
    pub continued_from_run_id: Option<RunId>,
    pub workspace: String,
    pub last_sequence: u64,
    pub terminal: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreationIntent {
    pub kind: PendingCreationKind,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<RunId>,
    pub command: RunCommand,
}

impl CreationIntent {
    #[must_use]
    pub fn is_unknown_billing(&self) -> bool {
        matches!(&self.command, RunCommand::Start(command) if command.model.is_none())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreationReservation {
    pub command_id: CommandId,
    pub command_sha256: String,
    pub run_id: RunId,
    pub created_at_unix_ms: u64,
    /// Present only until this reservation's `RunCreated` is committed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<CreationIntent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReservedCreation {
    pub reservation: CreationReservation,
    pub newly_reserved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableActionState {
    Prepared,
    InFlight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingModelAction {
    pub attempt_id: AttemptId,
    pub request: ModelRequest,
    pub primary_failure: Option<ModelAttemptFailure>,
    pub state: DurableActionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoppedModelFailure {
    pub failure: ModelAttemptFailure,
    pub reason: ModelRetryStopReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingContextCompaction {
    pub compaction_id: ContextCompactionId,
    pub trigger: ContextCompactionTrigger,
    pub plan: ContextCompactionPlan,
    pub attempt_id: AttemptId,
    pub request: ModelRequest,
    pub state: DurableActionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoppedContextCompactionFailure {
    pub compaction_id: ContextCompactionId,
    pub trigger: ContextCompactionTrigger,
    pub source_projection_sha256: String,
    pub failure: ModelAttemptFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedContextCompaction {
    pub compaction_id: ContextCompactionId,
    pub trigger: ContextCompactionTrigger,
    pub source_projection_sha256: String,
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolAction {
    pub operation_id: OperationId,
    pub invocation: ToolInvocation,
    pub state: DurableActionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<PendingUserInteraction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingUserInteraction {
    pub request: UserInteractionRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<UserInteractionResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSteer {
    pub command_id: CommandId,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingControl {
    pub command_id: CommandId,
    pub action: DurableControlAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurableCommand {
    Steer {
        content: String,
    },
    Stop {
        action: DurableControlAction,
    },
    ResolveInteraction {
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub command_id: CommandId,
    pub sequence: u64,
    pub command: DurableCommand,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub request: RunRequest,
    pub transcript: CanonicalTranscript,
    #[serde(default)]
    pub context_projection: Option<ContextProjection>,
    pub usage: Usage,
    pub accounting: ModelAccounting,
    pub terminal: Option<AgentOutcome>,
    pub runtime_model_requests: u32,
    pub runtime_retries: u32,
    pub tool_calls: u32,
    pub local_turns: u32,
    pub last_sequence: u64,
    pub last_model_output: Option<ModelOutput>,
    pub last_model_response_sequence: Option<u64>,
    pub last_model_activity_sequence: Option<u64>,
    pub last_model_failure: Option<StoppedModelFailure>,
    #[serde(default)]
    pub pending_context_compaction: Option<PendingContextCompaction>,
    #[serde(default)]
    pub last_context_compaction_failure: Option<StoppedContextCompactionFailure>,
    #[serde(default)]
    pub last_context_compaction: Option<CommittedContextCompaction>,
    pub pending_model: Option<PendingModelAction>,
    pub pending_tool: Option<PendingToolAction>,
    pub pending_children: Vec<RunId>,
    pub pending_steers: Vec<PendingSteer>,
    pub pending_control: Option<PendingControl>,
    pub command_receipts: Vec<CommandReceipt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReplay {
    pub snapshot: RunSnapshot,
    pub events: Vec<StoredRuntimeEvent>,
}

/// Validate lineage facts that must hold atomically with continuation create.
///
/// Product composition owns the new model, prompt, tool catalog, fingerprint,
/// budget, and deadline. The Store owns source existence/lifecycle, root
/// hierarchy, workspace identity, and canonical transcript continuity.
pub fn validate_continuation_request(
    source: &RunSnapshot,
    request: &RunRequest,
) -> Result<(), RunStoreError> {
    let source_run_id = source.request.run_id.clone().unwrap_or_default();
    let invalid = |reason| RunStoreError::InvalidContinuation {
        source_run_id: source_run_id.clone(),
        reason,
    };
    if source.request.parent_run_id.is_some()
        || source.request.actor.kind != AgentActorKind::Root
        || source.request.actor.depth != 0
    {
        return Err(invalid(ContinuationError::SourceIsChild));
    }
    let Some(outcome) = &source.terminal else {
        return Err(invalid(ContinuationError::SourceNotTerminal));
    };
    if matches!(outcome.terminal, TerminalState::RecoveryRequired { .. }) {
        return Err(invalid(ContinuationError::RecoveryRequired));
    }
    if request.parent_run_id.is_some()
        || request.actor.kind != AgentActorKind::Root
        || request.actor.depth != 0
        || request.continued_from_run_id.as_ref() != Some(&source_run_id)
    {
        return Err(invalid(ContinuationError::NewRunIsNotRoot));
    }
    if request.environment.workspace != source.request.environment.workspace {
        return Err(invalid(ContinuationError::WorkspaceMismatch));
    }
    let mut expected_transcript = source.transcript.clone();
    match expected_transcript.entries.first_mut() {
        Some(TranscriptEntry::System { prompt }) => {
            *prompt = request.system_prompt.clone();
        }
        _ => {
            expected_transcript.entries.insert(
                0,
                TranscriptEntry::System {
                    prompt: request.system_prompt.clone(),
                },
            );
        }
    }
    if request.transcript != expected_transcript {
        return Err(invalid(ContinuationError::TranscriptMismatch));
    }
    if request.context_projection != source.context_projection {
        return Err(invalid(ContinuationError::ContextProjectionMismatch));
    }
    Ok(())
}

/// Reduce the append-only canonical log into the one resumable runtime view.
/// SQLite snapshots are an optimization only; both stores use this reducer as
/// the semantic authority.
pub fn reduce_events(events: &[StoredRuntimeEvent]) -> Result<RunSnapshot, RunStoreError> {
    let Some(first) = events.first() else {
        return Err(RunStoreError::Backend {
            message: "cannot reduce an empty run event log".to_owned(),
        });
    };
    let run_id = first.run_id.clone();
    validate_event_header(first, 1, &run_id)?;
    let RuntimeEventKind::RunCreated { request } = &first.event else {
        return Err(corrupt(&run_id, "first event is not run_created"));
    };
    let request = (**request).clone();
    if request.run_id.as_ref() != Some(&run_id) {
        return Err(corrupt(
            &run_id,
            "run_created request id does not match event run id",
        ));
    }
    if request.parent_run_id != first.parent_run_id {
        return Err(corrupt(
            &run_id,
            "run_created parent id does not match event parent id",
        ));
    }
    if request.parent_run_id.is_some() && request.continued_from_run_id.is_some() {
        return Err(corrupt(
            &run_id,
            "run cannot be both a child Agent and a root continuation",
        ));
    }
    match request.purpose {
        RunPurpose::Agent => {}
        RunPurpose::ContextCompaction => {
            if request.parent_run_id.is_some()
                || request.continued_from_run_id.is_none()
                || request.actor.kind != AgentActorKind::Root
                || request.actor.depth != 0
                || !request.input.is_empty()
            {
                return Err(corrupt(
                    &run_id,
                    "context-compaction run must be an input-free root continuation",
                ));
            }
        }
    }
    let accounting = request.accounting_baseline.clone();

    let context_projection = request.context_projection.clone();
    let mut snapshot = RunSnapshot {
        transcript: initial_transcript(&request),
        context_projection,
        request,
        usage: Usage::default(),
        accounting,
        terminal: None,
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        local_turns: 0,
        last_sequence: 1,
        last_model_output: None,
        last_model_response_sequence: None,
        last_model_activity_sequence: None,
        last_model_failure: None,
        pending_context_compaction: None,
        last_context_compaction_failure: None,
        last_context_compaction: None,
        pending_model: None,
        pending_tool: None,
        pending_children: Vec::new(),
        pending_steers: Vec::new(),
        pending_control: None,
        command_receipts: Vec::new(),
    };

    for stored in events.iter().skip(1) {
        apply_event(&mut snapshot, stored)?;
    }
    Ok(snapshot)
}

/// Apply one already ordered canonical event to a durable snapshot. SQLite
/// uses this same transition for O(1) append validation; full replay remains
/// the reopen/audit authority.
pub fn apply_event(
    snapshot: &mut RunSnapshot,
    stored: &StoredRuntimeEvent,
) -> Result<(), RunStoreError> {
    let run_id = snapshot
        .request
        .run_id
        .clone()
        .ok_or_else(|| RunStoreError::Backend {
            message: "run snapshot has no run id".to_owned(),
        })?;
    validate_event_header(stored, snapshot.last_sequence.saturating_add(1), &run_id)?;
    if snapshot.terminal.is_some() {
        return Err(corrupt(&run_id, "event appears after terminal"));
    }
    if snapshot.pending_control.is_some()
        && !matches!(
            &stored.event,
            RuntimeEventKind::ToolOutcomeCommitted { .. }
                | RuntimeEventKind::ChildFinished { .. }
                | RuntimeEventKind::Terminal { .. }
        )
    {
        return Err(corrupt(
            &run_id,
            "only settlement events may follow a durable terminal control request",
        ));
    }
    if snapshot.last_model_failure.is_some()
        && !matches!(&stored.event, RuntimeEventKind::Terminal { .. })
    {
        return Err(corrupt(
            &run_id,
            "event appears after the persisted model retry decision stopped the run",
        ));
    }
    match &stored.event {
        RuntimeEventKind::RunCreated { .. } => {
            return Err(corrupt(&run_id, "duplicate run_created event"));
        }
        RuntimeEventKind::ContextCompactionPrepared {
            compaction_id,
            attempt_id,
            trigger,
            plan,
            request,
        } => {
            let trigger_matches_purpose = matches!(
                (snapshot.request.purpose, *trigger),
                (
                    RunPurpose::ContextCompaction,
                    ContextCompactionTrigger::Manual
                ) | (
                    RunPurpose::Agent,
                    ContextCompactionTrigger::Threshold | ContextCompactionTrigger::PreflightLimit
                )
            );
            if !trigger_matches_purpose {
                return Err(corrupt(
                    &run_id,
                    "context compaction trigger does not match the run purpose",
                ));
            }
            validate_compaction_safe_boundary(snapshot, &run_id)?;
            if request.run_id != run_id {
                return Err(corrupt(
                    &run_id,
                    "prepared context compaction request belongs to another run",
                ));
            }
            if request.attempt != 0 || request.streaming || !request.tools.is_empty() {
                return Err(corrupt(
                    &run_id,
                    "initial context compaction request must be attempt zero, non-streaming, and tool-free",
                ));
            }
            let force = compaction_force(*trigger);
            let preparation = prepare_compaction(
                &snapshot.transcript,
                snapshot.context_projection.as_ref(),
                snapshot.request.context_policy,
                &compaction_request_template(snapshot),
                force,
            )
            .map_err(|error| corrupt(&run_id, error.to_string()))?;
            match preparation {
                ContextCompactionPreparation::Model {
                    plan: expected_plan,
                    request: expected_request,
                } if expected_plan == **plan && expected_request == **request => {}
                _ => {
                    return Err(corrupt(
                        &run_id,
                        "prepared context compaction does not match the deterministic planner",
                    ));
                }
            }
            snapshot.runtime_model_requests = snapshot.runtime_model_requests.saturating_add(1);
            snapshot.last_context_compaction_failure = None;
            snapshot.pending_context_compaction = Some(PendingContextCompaction {
                compaction_id: compaction_id.clone(),
                trigger: *trigger,
                plan: (**plan).clone(),
                attempt_id: attempt_id.clone(),
                request: (**request).clone(),
                state: DurableActionState::Prepared,
            });
        }
        RuntimeEventKind::ContextCompactionInFlight {
            compaction_id,
            attempt_id,
        } => {
            let pending =
                pending_context_compaction_mut(snapshot, &run_id, compaction_id, attempt_id)?;
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(
                    &run_id,
                    "context compaction request entered in_flight twice",
                ));
            }
            pending.state = DurableActionState::InFlight;
        }
        RuntimeEventKind::ContextCompactionAttemptFailed {
            compaction_id,
            attempt_id,
            failure,
            output,
            accounting,
            retry,
        } => {
            let pending = snapshot
                .pending_context_compaction
                .as_ref()
                .cloned()
                .ok_or_else(|| {
                    corrupt(
                        &run_id,
                        "context compaction failure has no prepared request",
                    )
                })?;
            if pending.compaction_id != *compaction_id || pending.attempt_id != *attempt_id {
                return Err(corrupt(
                    &run_id,
                    "context compaction failure identity does not match the pending request",
                ));
            }
            if pending.state != DurableActionState::InFlight {
                return Err(corrupt(
                    &run_id,
                    "context compaction failed before transport began",
                ));
            }
            if output.is_some() && !failure.actionable_output {
                return Err(corrupt(
                    &run_id,
                    "rejected context compaction output must be marked actionable",
                ));
            }
            validate_context_compaction_retry(snapshot, &run_id, &pending, failure, retry)?;
            snapshot.pending_context_compaction = None;
            if let Some(output) = output {
                snapshot.usage.add_assign(output.usage);
            }
            snapshot.accounting = (**accounting).clone();
            match retry {
                ModelRetryDecision::Stop { .. } => {
                    snapshot.last_context_compaction_failure =
                        Some(StoppedContextCompactionFailure {
                            compaction_id: compaction_id.clone(),
                            trigger: pending.trigger,
                            source_projection_sha256: pending.plan.source_projection_sha256.clone(),
                            failure: failure.clone(),
                        });
                }
                ModelRetryDecision::Retry { prepared } => {
                    snapshot.runtime_model_requests =
                        snapshot.runtime_model_requests.saturating_add(1);
                    snapshot.runtime_retries = snapshot.runtime_retries.saturating_add(1);
                    snapshot.last_context_compaction_failure = None;
                    snapshot.pending_context_compaction = Some(PendingContextCompaction {
                        compaction_id: compaction_id.clone(),
                        trigger: pending.trigger,
                        plan: pending.plan,
                        attempt_id: prepared.attempt_id.clone(),
                        request: (*prepared.request).clone(),
                        state: DurableActionState::Prepared,
                    });
                }
            }
        }
        RuntimeEventKind::ContextCompactionCommitted {
            compaction_id,
            trigger,
            projection,
            output,
            accounting,
            before_tokens,
            after_tokens,
        } => {
            let trigger_matches_purpose = matches!(
                (snapshot.request.purpose, *trigger),
                (
                    RunPurpose::ContextCompaction,
                    ContextCompactionTrigger::Manual
                ) | (
                    RunPurpose::Agent,
                    ContextCompactionTrigger::Threshold | ContextCompactionTrigger::PreflightLimit
                )
            );
            if !trigger_matches_purpose {
                return Err(corrupt(
                    &run_id,
                    "context compaction commit trigger does not match the run purpose",
                ));
            }
            let current =
                effective_context(&snapshot.transcript, snapshot.context_projection.as_ref())
                    .map_err(|error| corrupt(&run_id, error.to_string()))?;
            if projection.source_entry_count != current.source_entry_count
                || projection.source_projection_sha256 != current.sha256
            {
                return Err(corrupt(
                    &run_id,
                    "context compaction projection does not describe the current effective context",
                ));
            }
            match output {
                Some(output) => {
                    let pending = snapshot
                        .pending_context_compaction
                        .as_ref()
                        .cloned()
                        .ok_or_else(|| {
                            corrupt(
                                &run_id,
                                "model context compaction committed without a pending request",
                            )
                        })?;
                    if pending.compaction_id != *compaction_id
                        || pending.trigger != *trigger
                        || pending.state != DurableActionState::InFlight
                    {
                        return Err(corrupt(
                            &run_id,
                            "context compaction commit identity or state is invalid",
                        ));
                    }
                    if output.finish_reason != ModelFinishReason::Stop
                        || !output.tool_calls.is_empty()
                        || output.content.trim().is_empty()
                    {
                        return Err(corrupt(
                            &run_id,
                            "context compaction model output must be a non-empty tool-free stop response",
                        ));
                    }
                    let expected_projection =
                        projection_from_summary(&pending.plan, &output.content);
                    if expected_projection != **projection
                        || *before_tokens != pending.plan.before_tokens
                    {
                        return Err(corrupt(
                            &run_id,
                            "context compaction commit differs from the prepared plan",
                        ));
                    }
                    snapshot.usage.add_assign(output.usage);
                    snapshot.pending_context_compaction = None;
                }
                None => {
                    if snapshot.pending_context_compaction.is_some() {
                        return Err(corrupt(
                            &run_id,
                            "local context compaction cannot bypass a pending model request",
                        ));
                    }
                    if **accounting != snapshot.accounting {
                        return Err(corrupt(
                            &run_id,
                            "local context compaction changed model accounting",
                        ));
                    }
                    let preparation = prepare_compaction(
                        &snapshot.transcript,
                        snapshot.context_projection.as_ref(),
                        snapshot.request.context_policy,
                        &compaction_request_template(snapshot),
                        compaction_force(*trigger),
                    )
                    .map_err(|error| corrupt(&run_id, error.to_string()))?;
                    match preparation {
                        ContextCompactionPreparation::Local {
                            projection: expected_projection,
                            before_tokens: expected_before,
                            after_tokens: expected_after,
                        } if expected_projection == **projection
                            && expected_before == *before_tokens
                            && expected_after == *after_tokens => {}
                        _ => {
                            return Err(corrupt(
                                &run_id,
                                "local context compaction does not match the deterministic planner",
                            ));
                        }
                    }
                }
            }
            let expected_after = estimate_projection_tokens(&snapshot.transcript, projection)
                .map_err(|error| corrupt(&run_id, error.to_string()))?;
            if expected_after != *after_tokens || *before_tokens != current.estimated_tokens {
                return Err(corrupt(
                    &run_id,
                    "context compaction token estimates do not match the committed projection",
                ));
            }
            if *after_tokens >= *before_tokens
                || *after_tokens > u64::from(snapshot.request.context_policy.hard_input_tokens)
            {
                return Err(corrupt(
                    &run_id,
                    "context compaction must reduce input below the configured hard limit",
                ));
            }
            snapshot.context_projection = Some((**projection).clone());
            snapshot.accounting = (**accounting).clone();
            snapshot.last_context_compaction_failure = None;
            snapshot.last_context_compaction = Some(CommittedContextCompaction {
                compaction_id: compaction_id.clone(),
                trigger: *trigger,
                source_projection_sha256: projection.source_projection_sha256.clone(),
                before_tokens: *before_tokens,
                after_tokens: *after_tokens,
                sequence: stored.sequence,
            });
        }
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id,
            request,
        } => {
            if snapshot.request.purpose != RunPurpose::Agent {
                return Err(corrupt(
                    &run_id,
                    "context-compaction run cannot prepare an ordinary Agent model request",
                ));
            }
            if snapshot.pending_model.is_some() || snapshot.pending_context_compaction.is_some() {
                return Err(corrupt(
                    &run_id,
                    "model request prepared while another model request is pending",
                ));
            }
            if request.run_id != run_id {
                return Err(corrupt(
                    &run_id,
                    "prepared model request belongs to another run",
                ));
            }
            if request.attempt != 0 {
                return Err(corrupt(
                    &run_id,
                    "standalone model request prepared event must start at attempt zero",
                ));
            }
            let effective =
                effective_context(&snapshot.transcript, snapshot.context_projection.as_ref())
                    .map_err(|error| corrupt(&run_id, error.to_string()))?;
            if request.parent_run_id != snapshot.request.parent_run_id
                || request.actor != snapshot.request.actor
                || request.model != snapshot.request.model
                || request.system_prompt != effective.system_prompt
                || request.messages != effective.messages
                || request.reasoning_effort != snapshot.request.reasoning_effort
                || request.max_output_tokens != snapshot.request.max_output_tokens
                || request.streaming != snapshot.request.streaming
                || request.request_number != snapshot.local_turns.saturating_add(1)
            {
                return Err(corrupt(
                    &run_id,
                    "ordinary model request does not match the canonical run projection",
                ));
            }
            snapshot.local_turns = snapshot.local_turns.max(request.request_number);
            snapshot.runtime_model_requests = snapshot.runtime_model_requests.saturating_add(1);
            snapshot.last_model_activity_sequence = Some(stored.sequence);
            snapshot.last_model_failure = None;
            snapshot.pending_model = Some(PendingModelAction {
                attempt_id: attempt_id.clone(),
                request: (**request).clone(),
                primary_failure: None,
                state: DurableActionState::Prepared,
            });
        }
        RuntimeEventKind::ModelRequestInFlight { attempt_id } => {
            let pending = pending_model_mut(snapshot, &run_id, attempt_id)?;
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(&run_id, "model request entered in_flight twice"));
            }
            pending.state = DurableActionState::InFlight;
        }
        RuntimeEventKind::ModelRequestFailed {
            attempt_id,
            failure,
            accounting,
            retry,
        } => {
            let pending = snapshot
                .pending_model
                .as_ref()
                .cloned()
                .ok_or_else(|| corrupt(&run_id, "model event has no prepared request"))?;
            if pending.attempt_id != *attempt_id {
                return Err(corrupt(
                    &run_id,
                    "model event attempt id does not match pending request",
                ));
            }
            if pending.state != DurableActionState::InFlight {
                return Err(corrupt(
                    &run_id,
                    "model request failed before transport began",
                ));
            }
            let primary_failure = pending
                .primary_failure
                .clone()
                .unwrap_or_else(|| failure.clone());
            match retry {
                ModelRetryDecision::Stop { reason } => {
                    validate_retry_stop(
                        &run_id,
                        &pending,
                        failure,
                        &primary_failure,
                        *reason,
                        snapshot.request.limits.max_model_retries,
                    )?;
                }
                ModelRetryDecision::Retry { prepared } => {
                    validate_prepared_retry(
                        &run_id,
                        &pending,
                        failure,
                        &primary_failure,
                        prepared,
                        snapshot.request.limits.max_model_retries,
                    )?;
                }
            }

            snapshot.pending_model = None;
            snapshot.accounting = (**accounting).clone();
            snapshot.last_model_activity_sequence = Some(stored.sequence);
            match retry {
                ModelRetryDecision::Stop { reason } => {
                    snapshot.last_model_failure = Some(StoppedModelFailure {
                        failure: primary_failure,
                        reason: *reason,
                    });
                }
                ModelRetryDecision::Retry { prepared } => {
                    snapshot.runtime_model_requests =
                        snapshot.runtime_model_requests.saturating_add(1);
                    snapshot.runtime_retries = snapshot.runtime_retries.saturating_add(1);
                    snapshot.last_model_failure = None;
                    snapshot.pending_model = Some(PendingModelAction {
                        attempt_id: prepared.attempt_id.clone(),
                        request: (*prepared.request).clone(),
                        primary_failure: Some(primary_failure),
                        state: DurableActionState::Prepared,
                    });
                }
            }
        }
        RuntimeEventKind::ModelResponseCommitted {
            attempt_id,
            output,
            accounting,
        } => {
            let pending = pending_model_mut(snapshot, &run_id, attempt_id)?;
            if pending.state != DurableActionState::InFlight {
                return Err(corrupt(
                    &run_id,
                    "model response committed before transport began",
                ));
            }
            snapshot.pending_model = None;
            snapshot.usage.add_assign(output.usage);
            snapshot.accounting = (**accounting).clone();
            snapshot.last_model_output = Some((**output).clone());
            snapshot.last_model_response_sequence = Some(stored.sequence);
            snapshot.last_model_activity_sequence = Some(stored.sequence);
            snapshot.last_model_failure = None;
            snapshot
                .transcript
                .entries
                .push(TranscriptEntry::Assistant {
                    content: (!output.content.is_empty()).then(|| output.content.clone()),
                    reasoning_content: output.reasoning_content.clone(),
                    tool_calls: output.tool_calls.clone(),
                });
        }
        RuntimeEventKind::ContentDelta { attempt_id, .. }
        | RuntimeEventKind::ReasoningDelta { attempt_id, .. } => {
            let pending = pending_model_mut(snapshot, &run_id, attempt_id)?;
            if pending.state != DurableActionState::InFlight {
                return Err(corrupt(
                    &run_id,
                    "model delta appeared before transport began",
                ));
            }
        }
        RuntimeEventKind::ToolPrepared {
            operation_id,
            invocation,
        } => {
            if snapshot.request.purpose != RunPurpose::Agent {
                return Err(corrupt(
                    &run_id,
                    "context-compaction run cannot prepare a tool",
                ));
            }
            if snapshot.pending_tool.is_some() || snapshot.pending_context_compaction.is_some() {
                return Err(corrupt(
                    &run_id,
                    "tool prepared while another durable action is pending",
                ));
            }
            snapshot.tool_calls = snapshot.tool_calls.saturating_add(1);
            snapshot.pending_tool = Some(PendingToolAction {
                operation_id: operation_id.clone(),
                invocation: invocation.clone(),
                state: DurableActionState::Prepared,
                interaction: None,
            });
        }
        RuntimeEventKind::InteractionRequested { request } => {
            let interactive_root = snapshot.request.actor.kind == AgentActorKind::Root
                && snapshot.request.environment.interactive;
            let pending = pending_tool_mut(snapshot, &run_id, &request.operation_id)?;
            if pending.invocation.call_id != request.call_id
                || pending.invocation.name != request.tool_name
            {
                return Err(corrupt(
                    &run_id,
                    "interaction request identity does not match the prepared tool",
                ));
            }
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(
                    &run_id,
                    "interaction requested after tool execution began",
                ));
            }
            if pending.interaction.is_some() {
                return Err(corrupt(&run_id, "tool requested more than one interaction"));
            }
            match &request.prompt {
                UserInteractionPrompt::Approval { arguments, .. } => {
                    if request.tool_name == REQUEST_USER_INPUT_TOOL_NAME {
                        return Err(corrupt(
                            &run_id,
                            "request_user_input requires a user-input interaction",
                        ));
                    }
                    if pending.invocation.arguments.parsed.as_ref() != Some(arguments) {
                        return Err(corrupt(
                            &run_id,
                            "approval arguments do not match the prepared tool invocation",
                        ));
                    }
                }
                UserInteractionPrompt::UserInput {
                    request: user_input,
                } => {
                    if request.tool_name != REQUEST_USER_INPUT_TOOL_NAME || !interactive_root {
                        return Err(corrupt(
                            &run_id,
                            "user-input interaction is only valid for an interactive root request_user_input call",
                        ));
                    }
                    user_input
                        .validate()
                        .map_err(|message| corrupt(&run_id, message))?;
                    let arguments =
                        pending.invocation.arguments.parsed.clone().ok_or_else(|| {
                            corrupt(&run_id, "request_user_input requires parsed tool arguments")
                        })?;
                    let decoded: UserInputRequest =
                        serde_json::from_value(arguments).map_err(|error| {
                            corrupt(
                                &run_id,
                                format!(
                                    "request_user_input arguments are not a valid request: {error}"
                                ),
                            )
                        })?;
                    if decoded != *user_input {
                        return Err(corrupt(
                            &run_id,
                            "user-input request does not match the prepared tool invocation",
                        ));
                    }
                }
            }
            pending.interaction = Some(PendingUserInteraction {
                request: request.clone(),
                response: None,
            });
        }
        RuntimeEventKind::InteractionResolved {
            command_id,
            interaction_id,
            response,
        } => {
            let pending = snapshot
                .pending_tool
                .as_ref()
                .ok_or_else(|| corrupt(&run_id, "interaction resolved without a pending tool"))?;
            let interaction = pending
                .interaction
                .as_ref()
                .ok_or_else(|| corrupt(&run_id, "interaction resolved before it was requested"))?;
            if interaction.request.interaction_id != *interaction_id {
                return Err(corrupt(
                    &run_id,
                    "interaction resolution id does not match the pending request",
                ));
            }
            if interaction.response.is_some() {
                return Err(corrupt(&run_id, "interaction was resolved more than once"));
            }
            interaction
                .request
                .validate_response(response)
                .map_err(|message| corrupt(&run_id, message))?;
            record_command(
                snapshot,
                &run_id,
                command_id,
                stored.sequence,
                DurableCommand::ResolveInteraction {
                    interaction_id: interaction_id.clone(),
                    response: response.clone(),
                },
            )?;
            snapshot
                .pending_tool
                .as_mut()
                .and_then(|pending| pending.interaction.as_mut())
                .expect("validated pending interaction remains present")
                .response = Some(response.clone());
        }
        RuntimeEventKind::ToolExecutionStarted { operation_id } => {
            let pending = pending_tool_mut(snapshot, &run_id, operation_id)?;
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(&run_id, "tool execution entered in_flight twice"));
            }
            if let Some(interaction) = &pending.interaction
                && !matches!(
                    (&interaction.request.prompt, &interaction.response),
                    (
                        UserInteractionPrompt::Approval { .. },
                        Some(UserInteractionResponse::Approved)
                    )
                )
            {
                return Err(corrupt(
                    &run_id,
                    "tool execution began without an approved interaction",
                ));
            }
            pending.state = DurableActionState::InFlight;
        }
        RuntimeEventKind::ToolOutcomeCommitted {
            operation_id,
            call_id,
            name,
            outcome,
        } => {
            outcome
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            let pending = pending_tool_mut(snapshot, &run_id, operation_id)?;
            if pending.invocation.call_id != *call_id || pending.invocation.name != *name {
                return Err(corrupt(
                    &run_id,
                    "tool outcome identity does not match prepared call",
                ));
            }
            if let Some(interaction) = &pending.interaction {
                match (
                    &interaction.request.prompt,
                    &interaction.response,
                    pending.state,
                ) {
                    (_, None, _) if outcome.operation != ToolOperationStatus::Cancelled => {
                        return Err(corrupt(
                            &run_id,
                            "tool interaction was bypassed without cancellation",
                        ));
                    }
                    (
                        UserInteractionPrompt::Approval { .. },
                        Some(UserInteractionResponse::Approved),
                        DurableActionState::Prepared,
                    ) => {
                        return Err(corrupt(
                            &run_id,
                            "approved tool committed an outcome before execution began",
                        ));
                    }
                    (
                        UserInteractionPrompt::Approval { .. },
                        Some(
                            UserInteractionResponse::Denied { .. }
                            | UserInteractionResponse::Cancelled,
                        ),
                        _,
                    ) if outcome.invocation != ToolInvocationStatus::Rejected => {
                        return Err(corrupt(
                            &run_id,
                            "denied or cancelled approval must commit a rejected tool outcome",
                        ));
                    }
                    _ => {}
                }
            }
            snapshot.pending_tool = None;
            snapshot.transcript.entries.push(TranscriptEntry::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: outcome.clone(),
            });
        }
        RuntimeEventKind::ChildStarted { child_run_id, .. } => {
            if snapshot.request.purpose != RunPurpose::Agent {
                return Err(corrupt(
                    &run_id,
                    "context-compaction run cannot start a child Agent",
                ));
            }
            if snapshot.pending_children.contains(child_run_id) {
                return Err(corrupt(&run_id, "child run started twice"));
            }
            snapshot.pending_children.push(child_run_id.clone());
        }
        RuntimeEventKind::ChildFinished {
            call_id,
            outcome,
            accounting,
            handoff_content,
        } => {
            let Some(position) = snapshot
                .pending_children
                .iter()
                .position(|run| run == &outcome.run_id)
            else {
                return Err(corrupt(&run_id, "child finished without a matching start"));
            };
            snapshot.pending_children.remove(position);
            snapshot.runtime_model_requests = snapshot
                .runtime_model_requests
                .saturating_add(outcome.runtime_model_requests);
            snapshot.runtime_retries = snapshot
                .runtime_retries
                .saturating_add(outcome.runtime_retries);
            snapshot.tool_calls = snapshot.tool_calls.saturating_add(outcome.tool_calls);
            snapshot.accounting = (**accounting).clone();
            snapshot
                .transcript
                .entries
                .push(TranscriptEntry::ChildOutcome {
                    call_id: call_id.clone(),
                    child_run_id: outcome.run_id.clone(),
                    outcome: outcome.clone(),
                    handoff_content: handoff_content.clone(),
                });
        }
        RuntimeEventKind::SteerQueued {
            command_id,
            content,
        } => {
            if snapshot.request.purpose != RunPurpose::Agent {
                return Err(corrupt(
                    &run_id,
                    "context-compaction run cannot accept a steer",
                ));
            }
            record_command(
                snapshot,
                &run_id,
                command_id,
                stored.sequence,
                DurableCommand::Steer {
                    content: content.clone(),
                },
            )?;
            snapshot.pending_steers.push(PendingSteer {
                command_id: command_id.clone(),
                content: content.clone(),
            });
        }
        RuntimeEventKind::SteerApplied {
            command_id,
            content,
        } => {
            let Some(pending) = snapshot.pending_steers.first() else {
                return Err(corrupt(&run_id, "steer applied without a queued command"));
            };
            if pending.command_id != *command_id || pending.content != *content {
                return Err(corrupt(
                    &run_id,
                    "steers must be applied in queued order with unchanged content",
                ));
            }
            snapshot.pending_steers.remove(0);
            snapshot.transcript.entries.push(TranscriptEntry::User {
                content: content.clone(),
            });
            snapshot.last_model_activity_sequence = Some(stored.sequence);
        }
        RuntimeEventKind::ControlRequested { command_id, action } => {
            if snapshot.pending_control.is_some() {
                return Err(corrupt(
                    &run_id,
                    "more than one terminal control was requested",
                ));
            }
            record_command(
                snapshot,
                &run_id,
                command_id,
                stored.sequence,
                DurableCommand::Stop { action: *action },
            )?;
            snapshot.pending_control = Some(PendingControl {
                command_id: command_id.clone(),
                action: *action,
            });
        }
        RuntimeEventKind::Terminal { outcome } => {
            if outcome.run_id != run_id {
                return Err(corrupt(&run_id, "terminal outcome belongs to another run"));
            }
            if let Some(control) = &snapshot.pending_control {
                let matches_control = matches!(
                    (control.action, &outcome.terminal),
                    (
                        DurableControlAction::Interrupt,
                        TerminalState::Interrupted | TerminalState::RecoveryRequired { .. }
                    ) | (
                        DurableControlAction::Cancel,
                        TerminalState::Cancelled | TerminalState::RecoveryRequired { .. }
                    )
                );
                if !matches_control {
                    return Err(corrupt(
                        &run_id,
                        "terminal state does not match the durable control request",
                    ));
                }
            }
            snapshot.terminal = Some((**outcome).clone());
        }
    }
    snapshot.last_sequence = stored.sequence;
    Ok(())
}

fn validate_event_header(
    event: &StoredRuntimeEvent,
    expected_sequence: u64,
    run_id: &RunId,
) -> Result<(), RunStoreError> {
    if !(MIN_SUPPORTED_AGENT_RUNTIME_EVENT_SCHEMA_VERSION..=AGENT_RUNTIME_EVENT_SCHEMA_VERSION)
        .contains(&event.schema_version)
    {
        return Err(RunStoreError::UnsupportedSchema {
            found: event.schema_version,
            minimum_supported: MIN_SUPPORTED_AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            maximum_supported: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
        });
    }
    if &event.run_id != run_id {
        return Err(corrupt(run_id, "event log mixes run ids"));
    }
    if event.sequence != expected_sequence {
        return Err(corrupt(
            run_id,
            format!(
                "event sequence is not contiguous: expected {expected_sequence}, found {}",
                event.sequence
            ),
        ));
    }
    Ok(())
}

fn initial_transcript(request: &RunRequest) -> CanonicalTranscript {
    let mut transcript = request.transcript.clone();
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
    transcript
}

fn retry_policy_stop_reason(
    pending: &PendingModelAction,
    failure: &ModelAttemptFailure,
    primary_failure: &ModelAttemptFailure,
    max_model_retries: u32,
) -> Option<ModelRetryStopReason> {
    if failure.actionable_output {
        return Some(ModelRetryStopReason::ActionableOutput);
    }
    if !failure.retryable {
        return Some(ModelRetryStopReason::NotRetryable);
    }
    if failure.code != primary_failure.code || failure.category != primary_failure.category {
        return Some(ModelRetryStopReason::FailureChanged);
    }
    (pending.request.attempt >= max_model_retries)
        .then_some(ModelRetryStopReason::RetryLimitReached)
}

fn validate_retry_stop(
    run_id: &RunId,
    pending: &PendingModelAction,
    failure: &ModelAttemptFailure,
    primary_failure: &ModelAttemptFailure,
    reason: ModelRetryStopReason,
    max_model_retries: u32,
) -> Result<(), RunStoreError> {
    let policy_reason =
        retry_policy_stop_reason(pending, failure, primary_failure, max_model_retries);
    let valid = match policy_reason {
        Some(expected) => reason == expected,
        None => reason == ModelRetryStopReason::ModelRequestBudgetExceeded,
    };
    if !valid {
        return Err(corrupt(
            run_id,
            "persisted model retry stop reason disagrees with the retry policy",
        ));
    }
    Ok(())
}

fn validate_prepared_retry(
    run_id: &RunId,
    pending: &PendingModelAction,
    failure: &ModelAttemptFailure,
    primary_failure: &ModelAttemptFailure,
    prepared: &PreparedModelRetry,
    max_model_retries: u32,
) -> Result<(), RunStoreError> {
    if retry_policy_stop_reason(pending, failure, primary_failure, max_model_retries).is_some() {
        return Err(corrupt(
            run_id,
            "model retry was prepared even though the retry policy required stopping",
        ));
    }
    if prepared.attempt_id == pending.attempt_id {
        return Err(corrupt(run_id, "model retry reused the failed attempt id"));
    }
    let mut expected_request = pending.request.clone();
    expected_request.attempt = expected_request.attempt.saturating_add(1);
    if *prepared.request != expected_request {
        return Err(corrupt(
            run_id,
            "prepared model retry changed fields other than the attempt number",
        ));
    }
    Ok(())
}

fn validate_compaction_safe_boundary(
    snapshot: &RunSnapshot,
    run_id: &RunId,
) -> Result<(), RunStoreError> {
    if snapshot.pending_context_compaction.is_some()
        || snapshot.pending_model.is_some()
        || snapshot.pending_tool.is_some()
        || !snapshot.pending_children.is_empty()
        || !snapshot.pending_steers.is_empty()
        || snapshot.pending_control.is_some()
        || snapshot.last_model_failure.is_some()
    {
        return Err(corrupt(
            run_id,
            "context compaction must begin at a durable action boundary",
        ));
    }
    Ok(())
}

fn compaction_force(trigger: ContextCompactionTrigger) -> bool {
    !matches!(trigger, ContextCompactionTrigger::Threshold)
}

fn validate_context_compaction_retry(
    snapshot: &RunSnapshot,
    run_id: &RunId,
    pending: &PendingContextCompaction,
    failure: &ModelAttemptFailure,
    retry: &ModelRetryDecision,
) -> Result<(), RunStoreError> {
    let can_reserve_request =
        snapshot.runtime_model_requests < snapshot.request.limits.max_model_requests;
    let attempt = pending.request.attempt;
    let retry_limit = snapshot.request.context_policy.max_retries;
    let policy_reason = if failure.actionable_output {
        Some(ModelRetryStopReason::ActionableOutput)
    } else if !failure.retryable {
        Some(ModelRetryStopReason::NotRetryable)
    } else if attempt >= retry_limit {
        Some(ModelRetryStopReason::RetryLimitReached)
    } else if !can_reserve_request {
        Some(ModelRetryStopReason::ModelRequestBudgetExceeded)
    } else {
        None
    };
    match retry {
        ModelRetryDecision::Stop { reason } => {
            if policy_reason != Some(*reason) {
                return Err(corrupt(
                    run_id,
                    "context compaction retry stop reason disagrees with policy",
                ));
            }
            Ok(())
        }
        ModelRetryDecision::Retry { prepared } => {
            if policy_reason.is_some() {
                return Err(corrupt(
                    run_id,
                    "context compaction retried when policy required stopping",
                ));
            }
            if prepared.attempt_id == pending.attempt_id {
                return Err(corrupt(
                    run_id,
                    "context compaction retry reused the failed attempt id",
                ));
            }
            let mut expected_request = pending.request.clone();
            expected_request.attempt = attempt.saturating_add(1);
            if *prepared.request != expected_request {
                return Err(corrupt(
                    run_id,
                    "context compaction retry changed fields other than the persisted fallback or attempt number",
                ));
            }
            Ok(())
        }
    }
}

fn compaction_request_template(snapshot: &RunSnapshot) -> ModelRequest {
    ModelRequest {
        run_id: snapshot.request.run_id.clone().unwrap_or_default(),
        parent_run_id: snapshot.request.parent_run_id.clone(),
        actor: snapshot.request.actor,
        model: snapshot.request.model.clone(),
        system_prompt: snapshot.request.system_prompt.clone(),
        messages: Vec::new(),
        tools: Vec::new(),
        reasoning_effort: ReasoningEffort::Low,
        max_output_tokens: Some(
            snapshot
                .request
                .context_policy
                .summary_max_output_tokens
                .max(1),
        ),
        streaming: false,
        request_number: snapshot.local_turns.saturating_add(1),
        attempt: 0,
    }
}

fn pending_model_mut<'a>(
    snapshot: &'a mut RunSnapshot,
    run_id: &RunId,
    attempt_id: &AttemptId,
) -> Result<&'a mut PendingModelAction, RunStoreError> {
    let pending = snapshot
        .pending_model
        .as_mut()
        .ok_or_else(|| corrupt(run_id, "model event has no prepared request"))?;
    if &pending.attempt_id != attempt_id {
        return Err(corrupt(
            run_id,
            "model event attempt id does not match pending request",
        ));
    }
    Ok(pending)
}

fn pending_context_compaction_mut<'a>(
    snapshot: &'a mut RunSnapshot,
    run_id: &RunId,
    compaction_id: &ContextCompactionId,
    attempt_id: &AttemptId,
) -> Result<&'a mut PendingContextCompaction, RunStoreError> {
    let pending = snapshot
        .pending_context_compaction
        .as_mut()
        .ok_or_else(|| corrupt(run_id, "context compaction event has no prepared request"))?;
    if &pending.compaction_id != compaction_id || &pending.attempt_id != attempt_id {
        return Err(corrupt(
            run_id,
            "context compaction event identity does not match the pending request",
        ));
    }
    Ok(pending)
}

fn pending_tool_mut<'a>(
    snapshot: &'a mut RunSnapshot,
    run_id: &RunId,
    operation_id: &OperationId,
) -> Result<&'a mut PendingToolAction, RunStoreError> {
    let pending = snapshot
        .pending_tool
        .as_mut()
        .ok_or_else(|| corrupt(run_id, "tool event has no prepared invocation"))?;
    if &pending.operation_id != operation_id {
        return Err(corrupt(
            run_id,
            "tool event operation id does not match pending call",
        ));
    }
    Ok(pending)
}

fn record_command(
    snapshot: &mut RunSnapshot,
    run_id: &RunId,
    command_id: &CommandId,
    sequence: u64,
    command: DurableCommand,
) -> Result<(), RunStoreError> {
    if snapshot
        .command_receipts
        .iter()
        .any(|receipt| receipt.command_id == *command_id)
    {
        return Err(corrupt(run_id, "command id was committed more than once"));
    }
    snapshot.command_receipts.push(CommandReceipt {
        command_id: command_id.clone(),
        sequence,
        command,
    });
    Ok(())
}

fn corrupt(run_id: &RunId, message: impl Into<String>) -> RunStoreError {
    RunStoreError::Corrupt {
        run_id: run_id.clone(),
        message: message.into(),
    }
}

#[derive(Debug, Default)]
pub struct InMemoryRunStore {
    runs: Mutex<HashMap<RunId, InMemoryRun>>,
    creations: Mutex<HashMap<CommandId, CreationReservation>>,
}

#[derive(Debug)]
struct InMemoryRun {
    events: Vec<StoredRuntimeEvent>,
    lease: Option<RunLease>,
    next_epoch: u64,
}

#[async_trait]
impl RunStore for InMemoryRunStore {
    async fn reserve_creation(
        &self,
        command_id: &CommandId,
        command_sha256: &str,
        proposed_run_id: RunId,
        intent: CreationIntent,
    ) -> Result<ReservedCreation, RunStoreError> {
        let mut creations = self.creations.lock().await;
        if let Some(existing) = creations.get(command_id) {
            if existing.command_sha256 != command_sha256
                || existing
                    .intent
                    .as_ref()
                    .is_some_and(|stored| stored != &intent)
            {
                return Err(RunStoreError::CreationConflict {
                    command_id: command_id.clone(),
                });
            }
            return Ok(ReservedCreation {
                reservation: existing.clone(),
                newly_reserved: false,
            });
        }
        if creations
            .values()
            .any(|reservation| reservation.run_id == proposed_run_id)
        {
            return Err(RunStoreError::Backend {
                message: format!(
                    "creation reservation proposed duplicate run id {proposed_run_id}"
                ),
            });
        }
        let reservation = CreationReservation {
            command_id: command_id.clone(),
            command_sha256: command_sha256.to_owned(),
            run_id: proposed_run_id,
            created_at_unix_ms: now_unix_ms(),
            intent: Some(intent),
        };
        creations.insert(command_id.clone(), reservation.clone());
        Ok(ReservedCreation {
            reservation,
            newly_reserved: true,
        })
    }

    async fn creation(
        &self,
        command_id: &CommandId,
    ) -> Result<Option<CreationReservation>, RunStoreError> {
        Ok(self.creations.lock().await.get(command_id).cloned())
    }

    async fn list_pending_creations(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<CreationReservation>, RunStoreError> {
        let mut creations = self
            .creations
            .lock()
            .await
            .values()
            .filter(|reservation| {
                reservation
                    .intent
                    .as_ref()
                    .is_some_and(|intent| intent.workspace == workspace)
            })
            .cloned()
            .collect::<Vec<_>>();
        creations.sort_by(|left, right| {
            right
                .created_at_unix_ms
                .cmp(&left.created_at_unix_ms)
                .then_with(|| right.command_id.0.cmp(&left.command_id.0))
        });
        creations.truncate(limit as usize);
        Ok(creations)
    }

    async fn create(&self, mut request: RunRequest) -> Result<CreatedRun, RunStoreError> {
        let run_id = request.run_id.clone().unwrap_or_default();
        request.run_id = Some(run_id.clone());
        let mut runs = self.runs.lock().await;
        if runs.contains_key(&run_id) {
            return Err(RunStoreError::AlreadyExists { run_id });
        }
        if let Some(source_run_id) = request.continued_from_run_id.clone() {
            let source = runs
                .get(&source_run_id)
                .ok_or_else(|| RunStoreError::NotFound {
                    run_id: source_run_id.clone(),
                })?;
            let source = replay(source.events.clone())?;
            validate_continuation_request(&source.snapshot, &request)?;
            validate_in_memory_continuation_lineage(
                &runs,
                &source.snapshot,
                &request.environment.workspace,
            )?;
        }
        let lease = new_lease(run_id.clone(), 1);
        let created = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: run_id.clone(),
            parent_run_id: request.parent_run_id.clone(),
            event_id: RuntimeEventId::run_created(),
            sequence: 1,
            occurred_at_unix_ms: now_unix_ms(),
            event: RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        };
        let replay = replay(vec![created.clone()])?;
        runs.insert(
            run_id.clone(),
            InMemoryRun {
                events: vec![created.clone()],
                lease: Some(lease.clone()),
                next_epoch: 2,
            },
        );
        drop(runs);
        if let Some(reservation) = self
            .creations
            .lock()
            .await
            .values_mut()
            .find(|reservation| reservation.run_id == run_id)
        {
            reservation.intent = None;
        }
        Ok(CreatedRun {
            lease,
            created,
            replay,
        })
    }

    async fn acquire(&self, run_id: &RunId) -> Result<AcquiredRun, RunStoreError> {
        let mut runs = self.runs.lock().await;
        let run = runs
            .get_mut(run_id)
            .ok_or_else(|| RunStoreError::NotFound {
                run_id: run_id.clone(),
            })?;
        let replay = replay(run.events.clone())?;
        if replay.snapshot.terminal.is_some() {
            return Ok(AcquiredRun {
                lease: None,
                replay,
            });
        }
        if run.lease.is_some() {
            return Err(RunStoreError::AlreadyRunning {
                run_id: run_id.clone(),
            });
        }
        let lease = new_lease(run_id.clone(), run.next_epoch);
        run.next_epoch = run.next_epoch.saturating_add(1);
        run.lease = Some(lease.clone());
        Ok(AcquiredRun {
            lease: Some(lease),
            replay,
        })
    }

    async fn append(
        &self,
        lease: &RunLease,
        pending: PendingRuntimeEvent,
    ) -> Result<StoredRuntimeEvent, RunStoreError> {
        let mut runs = self.runs.lock().await;
        let run = runs
            .get_mut(&lease.run_id)
            .ok_or_else(|| RunStoreError::NotFound {
                run_id: lease.run_id.clone(),
            })?;
        if let Some(existing) = run
            .events
            .iter()
            .find(|event| event.event_id == pending.event_id)
        {
            if existing.event == pending.event {
                return Ok(existing.clone());
            }
            return Err(RunStoreError::EventConflict {
                run_id: lease.run_id.clone(),
                event_id: pending.event_id,
            });
        }
        if run
            .events
            .last()
            .is_some_and(|event| event.event.is_terminal())
        {
            return Err(RunStoreError::AlreadyTerminal {
                run_id: lease.run_id.clone(),
            });
        }
        if run.lease.as_ref() != Some(lease) {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id.clone(),
                epoch: lease.epoch,
            });
        }
        let first = run.events.first().expect("created runs have one event");
        let stored = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: lease.run_id.clone(),
            parent_run_id: first.parent_run_id.clone(),
            event_id: pending.event_id,
            sequence: u64::try_from(run.events.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1),
            occurred_at_unix_ms: now_unix_ms(),
            event: pending.event,
        };
        let mut candidate = run.events.clone();
        candidate.push(stored.clone());
        reduce_events(&candidate)?;
        if stored.event.is_terminal() {
            run.lease = None;
        }
        run.events.push(stored.clone());
        Ok(stored)
    }

    async fn load(&self, run_id: &RunId) -> Result<Option<RunReplay>, RunStoreError> {
        self.runs
            .lock()
            .await
            .get(run_id)
            .map(|run| replay(run.events.clone()))
            .transpose()
    }

    async fn events_after(
        &self,
        run_id: &RunId,
        sequence: u64,
    ) -> Result<Vec<StoredRuntimeEvent>, RunStoreError> {
        Ok(self
            .runs
            .lock()
            .await
            .get(run_id)
            .map(|run| {
                run.events
                    .iter()
                    .filter(|event| event.sequence > sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn release(&self, lease: &RunLease) -> Result<(), RunStoreError> {
        let mut runs = self.runs.lock().await;
        let run = runs
            .get_mut(&lease.run_id)
            .ok_or_else(|| RunStoreError::NotFound {
                run_id: lease.run_id.clone(),
            })?;
        if run.lease.as_ref() != Some(lease) {
            return Err(RunStoreError::StaleLease {
                run_id: lease.run_id.clone(),
                epoch: lease.epoch,
            });
        }
        run.lease = None;
        Ok(())
    }

    async fn list_root_runs(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<RootRunRecord>, RunStoreError> {
        let runs = self.runs.lock().await;
        let mut candidates = Vec::new();
        for (run_id, run) in runs.iter() {
            let snapshot = reduce_events(&run.events)?;
            if snapshot.request.parent_run_id.is_some()
                || snapshot.request.environment.workspace != workspace
            {
                continue;
            }
            let created_at_unix_ms = run
                .events
                .first()
                .ok_or_else(|| corrupt(run_id, "run has no creation event"))?
                .occurred_at_unix_ms;
            let updated_at_unix_ms = run
                .events
                .last()
                .ok_or_else(|| corrupt(run_id, "run has no events"))?
                .occurred_at_unix_ms;
            candidates.push(RootRunRecord {
                run_id: run_id.clone(),
                purpose: snapshot.request.purpose,
                continued_from_run_id: snapshot.request.continued_from_run_id.clone(),
                workspace: snapshot.request.environment.workspace,
                last_sequence: snapshot.last_sequence,
                terminal: snapshot.terminal.is_some(),
                created_at_unix_ms,
                updated_at_unix_ms,
            });
        }
        candidates.sort_by(|left, right| {
            right
                .updated_at_unix_ms
                .cmp(&left.updated_at_unix_ms)
                .then_with(|| right.run_id.0.cmp(&left.run_id.0))
        });
        candidates.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(candidates)
    }
}

fn validate_in_memory_continuation_lineage(
    runs: &HashMap<RunId, InMemoryRun>,
    source: &RunSnapshot,
    workspace: &str,
) -> Result<(), RunStoreError> {
    let source_run_id = source.request.run_id.clone().unwrap_or_default();
    let invalid = || RunStoreError::InvalidContinuation {
        source_run_id: source_run_id.clone(),
        reason: ContinuationError::LineageCorrupt,
    };
    let mut visited = HashSet::from([source_run_id.clone()]);
    let mut cursor = source.request.continued_from_run_id.clone();
    while let Some(run_id) = cursor {
        if !visited.insert(run_id.clone()) {
            return Err(invalid());
        }
        let run = runs.get(&run_id).ok_or_else(invalid)?;
        let replay = replay(run.events.clone())?;
        let snapshot = replay.snapshot;
        if snapshot.request.parent_run_id.is_some()
            || snapshot.request.actor.kind != AgentActorKind::Root
            || snapshot.request.actor.depth != 0
            || snapshot.request.environment.workspace != workspace
            || snapshot.terminal.is_none()
        {
            return Err(invalid());
        }
        cursor = snapshot.request.continued_from_run_id;
    }
    Ok(())
}

fn replay(events: Vec<StoredRuntimeEvent>) -> Result<RunReplay, RunStoreError> {
    let snapshot = reduce_events(&events)?;
    Ok(RunReplay { snapshot, events })
}

fn new_lease(run_id: RunId, epoch: u64) -> RunLease {
    RunLease {
        run_id,
        epoch,
        owner_id: RuntimeEventId::new().0,
        owner_pid: std::process::id(),
    }
}
