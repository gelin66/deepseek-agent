use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use codewhale_context::compaction::{
    ContextCompactionPreparation, ContextInput, effective_context, estimate_projection_tokens,
    prepare_compaction,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedContextCompaction {
    pub source_projection_sha256: String,
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolAction {
    pub operation_id: OperationId,
    pub invocation: ToolInvocation,
    pub workspace_access: WorkspaceAccess,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingHostVerification {
    pub verification_id: VerificationId,
    pub candidate: CompletionCandidate,
    pub acceptance_id: AcceptanceId,
    pub verifier: VerifierSpec,
    pub workspace_state_before: WorkspaceState,
    pub state: DurableActionState,
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
    pub workspace_state: WorkspaceState,
    pub evidence_receipts: Vec<EvidenceReceipt>,
    pub pending_completion: Option<CompletionCandidate>,
    pub pending_host_verification: Option<PendingHostVerification>,
    pub last_completion_rejection: Option<CompletionRejection>,
    pub last_host_verification_failure: Option<HostVerificationFailure>,
    pub runtime_model_requests: u32,
    pub runtime_retries: u32,
    pub tool_calls: u32,
    pub local_turns: u32,
    pub last_sequence: u64,
    pub last_model_output: Option<ModelOutput>,
    /// Exact tool names advertised by the request that produced
    /// `last_model_output`. The request event remains the canonical source;
    /// this projection keeps response authorization crash-safe without
    /// duplicating the full request in every snapshot.
    #[serde(default)]
    pub last_model_advertised_tool_names: Vec<String>,
    pub last_model_response_sequence: Option<u64>,
    pub last_model_activity_sequence: Option<u64>,
    pub last_model_failure: Option<StoppedModelFailure>,
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
    let expected_facts = InheritedRunFacts {
        workspace_state: source.workspace_state.clone(),
        last_completion_rejection: source.last_completion_rejection.clone(),
        last_host_verification_failure: source.last_host_verification_failure.clone(),
    };
    if request.inherited_facts.as_ref() != Some(&expected_facts) {
        return Err(invalid(ContinuationError::InheritedFactsMismatch));
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
    if request.continued_from_run_id.is_none() && request.inherited_facts.is_some() {
        return Err(corrupt(
            &run_id,
            "only a root continuation may inherit prior run facts",
        ));
    }
    let contract = request
        .task_contract
        .as_ref()
        .ok_or_else(|| corrupt(&run_id, "Agent run has no frozen task contract"))?;
    contract
        .validate()
        .map_err(|message| corrupt(&run_id, message))?;
    if contract.generation_id.0 != run_id.0 {
        return Err(corrupt(
            &run_id,
            "task generation id does not match the run id",
        ));
    }
    let accounting = request.accounting_baseline.clone();

    let context_projection = request.context_projection.clone();
    let inherited_facts = request.inherited_facts.clone();
    let mut snapshot = RunSnapshot {
        transcript: initial_transcript(&request),
        context_projection,
        request,
        usage: Usage::default(),
        accounting,
        terminal: None,
        workspace_state: inherited_facts
            .as_ref()
            .map(|facts| facts.workspace_state.clone())
            .unwrap_or_else(|| WorkspaceState {
                generation: 0,
                revision: WorkspaceRevision::Unknown {
                    reason: "workspace has not been observed".to_owned(),
                },
            }),
        evidence_receipts: Vec::new(),
        pending_completion: None,
        pending_host_verification: None,
        last_completion_rejection: inherited_facts
            .as_ref()
            .and_then(|facts| facts.last_completion_rejection.clone()),
        last_host_verification_failure: inherited_facts
            .and_then(|facts| facts.last_host_verification_failure),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        local_turns: 0,
        last_sequence: 1,
        last_model_output: None,
        last_model_advertised_tool_names: Vec::new(),
        last_model_response_sequence: None,
        last_model_activity_sequence: None,
        last_model_failure: None,
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
        RuntimeEventKind::ContextCompactionCommitted {
            projection,
            tools,
            accounting,
            before_tokens,
            after_tokens,
        } => {
            validate_compaction_safe_boundary(snapshot, &run_id)?;
            let input = context_input(snapshot, tools);
            let current =
                effective_context(input).map_err(|error| corrupt(&run_id, error.to_string()))?;
            if projection.source_entry_count != current.source_entry_count
                || projection.source_projection_sha256 != current.sha256
            {
                return Err(corrupt(
                    &run_id,
                    "context compaction projection does not describe the current effective context",
                ));
            }
            if **accounting != snapshot.accounting {
                return Err(corrupt(
                    &run_id,
                    "deterministic context compaction changed model accounting",
                ));
            }
            let preparation = prepare_compaction(input, snapshot.request.context_policy)
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
                        "context compaction commit does not match the deterministic broker",
                    ));
                }
            }
            let expected_after = estimate_projection_tokens(input, projection)
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
            snapshot.last_context_compaction = Some(CommittedContextCompaction {
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
            if snapshot.pending_model.is_some() {
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
            validate_model_request_safe_boundary(snapshot, &run_id)?;
            let effective = effective_context(context_input(snapshot, &request.tools))
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
            let advertised_tool_names = pending
                .request
                .tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect();
            snapshot.pending_model = None;
            snapshot.usage.add_assign(output.usage);
            snapshot.accounting = (**accounting).clone();
            snapshot.last_model_output = Some((**output).clone());
            snapshot.last_model_advertised_tool_names = advertised_tool_names;
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
            workspace_access,
        } => {
            if snapshot.pending_tool.is_some() {
                return Err(corrupt(
                    &run_id,
                    "tool prepared while another durable action is pending",
                ));
            }
            snapshot.tool_calls = snapshot.tool_calls.saturating_add(1);
            snapshot.pending_tool = Some(PendingToolAction {
                operation_id: operation_id.clone(),
                invocation: invocation.clone(),
                workspace_access: *workspace_access,
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
            workspace_state,
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
            match (pending.workspace_access, workspace_state) {
                (WorkspaceAccess::ReadOnly, None) => {}
                (WorkspaceAccess::ReadOnly, Some(_)) => {
                    return Err(corrupt(
                        &run_id,
                        "read-only tool outcome cannot advance workspace state",
                    ));
                }
                (WorkspaceAccess::MayWrite, None)
                    if pending.state == DurableActionState::Prepared
                        && outcome.side_effect == ToolSideEffectStatus::NotApplied => {}
                (WorkspaceAccess::MayWrite, None) => {
                    return Err(corrupt(
                        &run_id,
                        "workspace-mutating tool outcome must settle workspace state",
                    ));
                }
                (WorkspaceAccess::MayWrite, Some(state)) => {
                    state
                        .validate()
                        .map_err(|message| corrupt(&run_id, message))?;
                    if state.generation != snapshot.workspace_state.generation.saturating_add(1) {
                        return Err(corrupt(
                            &run_id,
                            "workspace-mutating tool outcome did not advance one generation",
                        ));
                    }
                    snapshot.workspace_state = state.clone();
                }
            }
            snapshot.pending_tool = None;
            snapshot.transcript.entries.push(TranscriptEntry::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: outcome.clone(),
            });
        }
        RuntimeEventKind::WorkspaceObserved { workspace_state } => {
            workspace_state
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            let expected_generation = match (
                &snapshot.workspace_state.revision,
                &workspace_state.revision,
            ) {
                (
                    WorkspaceRevision::Known { sha256: previous },
                    WorkspaceRevision::Known { sha256: current },
                ) if previous == current => snapshot.workspace_state.generation,
                _ => snapshot.workspace_state.generation.saturating_add(1),
            };
            if workspace_state.generation != expected_generation {
                return Err(corrupt(
                    &run_id,
                    "workspace observation carries an invalid generation",
                ));
            }
            snapshot.workspace_state = workspace_state.clone();
        }
        RuntimeEventKind::CompletionProposed { candidate } => {
            candidate
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            let generation = &snapshot
                .request
                .task_contract
                .as_ref()
                .expect("Agent contract validated at run creation")
                .generation_id;
            if &candidate.generation_id != generation {
                return Err(corrupt(
                    &run_id,
                    "completion candidate belongs to another task generation",
                ));
            }
            snapshot.pending_completion = Some(candidate.clone());
        }
        RuntimeEventKind::HostVerificationPrepared {
            verification_id,
            candidate,
            acceptance_id,
            verifier,
            workspace_state_before,
        } => {
            if snapshot.pending_host_verification.is_some() {
                return Err(corrupt(&run_id, "Host verification prepared twice"));
            }
            if snapshot.pending_completion.as_ref() != Some(candidate) {
                return Err(corrupt(
                    &run_id,
                    "Host verification does not match the pending completion candidate",
                ));
            }
            let contract = snapshot
                .request
                .task_contract
                .as_ref()
                .expect("Agent contract validated at run creation");
            let matches_acceptance = contract.definition.acceptance.iter().any(|acceptance| {
                matches!(
                    acceptance,
                    TaskAcceptance::Verifier {
                        id,
                        verifier: expected,
                        ..
                    } if id == acceptance_id && expected == verifier
                )
            });
            if !matches_acceptance || workspace_state_before != &snapshot.workspace_state {
                return Err(corrupt(
                    &run_id,
                    "Host verification does not match the contract or current workspace",
                ));
            }
            snapshot.pending_host_verification = Some(PendingHostVerification {
                verification_id: verification_id.clone(),
                candidate: candidate.clone(),
                acceptance_id: acceptance_id.clone(),
                verifier: verifier.clone(),
                workspace_state_before: workspace_state_before.clone(),
                state: DurableActionState::Prepared,
            });
        }
        RuntimeEventKind::HostVerificationStarted { verification_id } => {
            let pending = snapshot
                .pending_host_verification
                .as_mut()
                .ok_or_else(|| corrupt(&run_id, "Host verification started before preparation"))?;
            if pending.verification_id != *verification_id
                || pending.state != DurableActionState::Prepared
                || pending.workspace_state_before != snapshot.workspace_state
            {
                return Err(corrupt(
                    &run_id,
                    "Host verification start does not match the prepared action",
                ));
            }
            pending.state = DurableActionState::InFlight;
        }
        RuntimeEventKind::HostVerificationCommitted {
            verification_id,
            outcome,
            receipt,
            workspace_state_after,
        } => {
            outcome
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            let pending = snapshot.pending_host_verification.as_ref().ok_or_else(|| {
                corrupt(&run_id, "Host verification committed before preparation")
            })?;
            if pending.verification_id != *verification_id
                || pending.state != DurableActionState::InFlight
            {
                return Err(corrupt(
                    &run_id,
                    "Host verification outcome does not match the in-flight action",
                ));
            }
            if workspace_state_after.generation
                != pending.workspace_state_before.generation.saturating_add(1)
            {
                return Err(corrupt(
                    &run_id,
                    "Host verification did not advance the workspace generation",
                ));
            }
            workspace_state_after
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            if let Some(receipt) = receipt {
                receipt
                    .validate()
                    .map_err(|message| corrupt(&run_id, message))?;
                let observation = outcome.verifier_observation.as_ref().ok_or_else(|| {
                    corrupt(
                        &run_id,
                        "Host verification receipt has no typed verifier observation",
                    )
                })?;
                let known_revision_matches = matches!(
                    (&observation.workspace_revision, &workspace_state_after.revision),
                    (
                        WorkspaceRevision::Known { sha256: observed },
                        WorkspaceRevision::Known { sha256: settled }
                    ) if observed == settled
                );
                let contract = snapshot
                    .request
                    .task_contract
                    .as_ref()
                    .expect("Agent contract validated at run creation");
                if !outcome.is_success()
                    || observation.verdict != VerifierVerdict::Passed
                    || observation.spec != pending.verifier
                    || !verifier_artifacts_are_available(outcome, observation)
                    || !known_revision_matches
                    || receipt.id
                        != EvidenceReceiptId::from(format!("receipt:{}", verification_id.0))
                    || receipt.generation_id != contract.generation_id
                    || receipt.acceptance_id != pending.acceptance_id
                    || receipt.verification_id != *verification_id
                    || receipt.verifier != pending.verifier
                    || receipt.workspace_state != *workspace_state_after
                    || receipt.artifact_ids != observation.artifact_ids
                {
                    return Err(corrupt(
                        &run_id,
                        "Host verification receipt does not match the exact observation",
                    ));
                }
                if snapshot
                    .evidence_receipts
                    .iter()
                    .any(|existing| existing.id == receipt.id)
                {
                    return Err(corrupt(&run_id, "evidence receipt id was committed twice"));
                }
                snapshot.evidence_receipts.push(receipt.clone());
                snapshot.last_completion_rejection = None;
                snapshot.last_host_verification_failure = None;
            } else {
                snapshot.last_host_verification_failure = Some(HostVerificationFailure {
                    outcome: (**outcome).clone(),
                    workspace_state: workspace_state_after.clone(),
                });
            }
            snapshot.workspace_state = workspace_state_after.clone();
            snapshot.pending_host_verification = None;
        }
        RuntimeEventKind::CompletionRejected { rejection } => {
            rejection
                .validate()
                .map_err(|message| corrupt(&run_id, message))?;
            if snapshot
                .pending_completion
                .as_ref()
                .is_none_or(|candidate| candidate.id != rejection.candidate_id)
            {
                return Err(corrupt(
                    &run_id,
                    "completion rejection does not match the pending candidate",
                ));
            }
            snapshot.last_completion_rejection = Some(rejection.clone());
            snapshot.pending_completion = None;
        }
        RuntimeEventKind::ChildStarted { child_run_id, .. } => {
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
            if let TerminalState::Completed { decision, .. } = &outcome.terminal {
                validate_completion_decision(snapshot, &run_id, decision)?;
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
    if let Some(contract) = &request.task_contract {
        transcript.entries.push(TranscriptEntry::User {
            content: contract.definition.model_message(),
        });
    }
    transcript
}

fn validate_completion_decision(
    snapshot: &RunSnapshot,
    run_id: &RunId,
    decision: &CompletionDecision,
) -> Result<(), RunStoreError> {
    decision
        .validate()
        .map_err(|message| corrupt(run_id, message))?;
    let candidate = snapshot
        .pending_completion
        .as_ref()
        .ok_or_else(|| corrupt(run_id, "completed terminal has no completion candidate"))?;
    let contract = snapshot
        .request
        .task_contract
        .as_ref()
        .ok_or_else(|| corrupt(run_id, "completed Agent has no task contract"))?;
    if decision.candidate_id != candidate.id
        || decision.generation_id != contract.generation_id
        || decision.workspace_state != snapshot.workspace_state
    {
        return Err(corrupt(
            run_id,
            "completion decision does not match the current candidate, generation, or workspace",
        ));
    }
    if decision.satisfied.len() != contract.definition.acceptance.len() {
        return Err(corrupt(
            run_id,
            "completion decision does not satisfy the whole task contract",
        ));
    }
    for acceptance in &contract.definition.acceptance {
        let Some(satisfaction) = decision
            .satisfied
            .iter()
            .find(|item| item.acceptance_id() == acceptance.id())
        else {
            return Err(corrupt(
                run_id,
                format!("acceptance '{}' is not satisfied", acceptance.id().0),
            ));
        };
        match (acceptance, satisfaction) {
            (TaskAcceptance::Host { .. }, AcceptanceSatisfaction::Host { .. }) => {}
            (
                TaskAcceptance::Verifier { verifier, .. },
                AcceptanceSatisfaction::Evidence { receipt_id, .. },
            ) => {
                let receipt = snapshot
                    .evidence_receipts
                    .iter()
                    .find(|receipt| &receipt.id == receipt_id)
                    .ok_or_else(|| corrupt(run_id, "completion references an unknown receipt"))?;
                if receipt.generation_id != contract.generation_id
                    || receipt.acceptance_id != *acceptance.id()
                    || receipt.verifier != *verifier
                    || receipt.workspace_state != snapshot.workspace_state
                {
                    return Err(corrupt(
                        run_id,
                        "completion receipt does not match the current contract or workspace",
                    ));
                }
            }
            _ => {
                return Err(corrupt(
                    run_id,
                    "completion satisfaction kind does not match task acceptance",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn verifier_artifacts_are_available(
    outcome: &ToolOutcome,
    observation: &VerifierObservation,
) -> bool {
    !observation.artifact_ids.is_empty()
        && outcome.evidence.status == ToolEvidenceStatus::Produced
        && outcome.evidence.references == observation.artifact_ids
        && observation.artifact_ids.iter().all(|artifact_id| {
            outcome.artifacts.iter().any(|artifact| {
                artifact.id == *artifact_id
                    && artifact.status == ToolArtifactStatus::Available
                    && artifact.sha256.is_some()
            })
        })
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
    if snapshot.pending_model.is_some()
        || snapshot.pending_tool.is_some()
        || snapshot.pending_completion.is_some()
        || snapshot.pending_host_verification.is_some()
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

fn validate_model_request_safe_boundary(
    snapshot: &RunSnapshot,
    run_id: &RunId,
) -> Result<(), RunStoreError> {
    if snapshot.pending_tool.is_some()
        || snapshot.pending_completion.is_some()
        || snapshot.pending_host_verification.is_some()
        || !snapshot.pending_children.is_empty()
        || !snapshot.pending_steers.is_empty()
        || snapshot.pending_control.is_some()
    {
        return Err(corrupt(
            run_id,
            "model request must begin at a settled durable action boundary",
        ));
    }
    Ok(())
}

fn context_input<'a>(snapshot: &'a RunSnapshot, tools: &'a [ToolDefinition]) -> ContextInput<'a> {
    ContextInput {
        transcript: &snapshot.transcript,
        projection: snapshot.context_projection.as_ref(),
        task_contract: snapshot.request.task_contract.as_ref(),
        workspace_state: &snapshot.workspace_state,
        evidence_receipts: &snapshot.evidence_receipts,
        last_completion_rejection: snapshot.last_completion_rejection.as_ref(),
        last_verifier_failure: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.outcome),
        last_verifier_failure_workspace: snapshot
            .last_host_verification_failure
            .as_ref()
            .map(|failure| &failure.workspace_state),
        tools,
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
