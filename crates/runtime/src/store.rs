use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use dse_context::compaction::{
    ContextCompactionPreparation, ContextInput, effective_context, estimate_projection_tokens,
    prepare_compaction,
};
use dse_protocol::run_api::{PendingCreationKind, RunCommand};
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreationReservation {
    pub command_id: CommandId,
    /// Identity of the canonical caller payload, before Host-owned resolution.
    pub command_sha256: String,
    pub run_id: RunId,
    pub created_at_unix_ms: u64,
    /// First Host-resolved durable launch truth. Present only until this
    /// reservation's `RunCreated` is committed.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<ToolAuthorizationDecision>,
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
pub struct AgentWorkspaceCreatedFact {
    pub assignment: AgentWorkspaceAssignment,
    pub writer_workspace_state: WorkspaceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChildStartedFact {
    pub call_id: String,
    pub child_run_id: RunId,
    pub depth: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSealCommittedFact {
    pub final_commit: String,
    pub diff_sha256: String,
    pub changed_files: Vec<String>,
    pub writer_workspace_state_after: WorkspaceState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSealLifecycle {
    pub base_commit: String,
    pub writer_workspace_state_before: WorkspaceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<AgentSealCommittedFact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentIntegrationCommittedFact {
    pub root_head_commit: String,
    pub root_workspace_state_after: WorkspaceState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentIntegrationFailureFact {
    pub status: WriterIntegrationStatus,
    pub root_workspace_state: WorkspaceState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentIntegrationLifecycle {
    pub integration_id: OperationId,
    pub base_commit: String,
    pub writer_commit: String,
    pub diff_sha256: String,
    pub expected_root_workspace_state: WorkspaceState,
    pub started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<AgentIntegrationCommittedFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<AgentIntegrationFailureFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCleanupLifecycle {
    pub plan: WriterCleanupPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<WriterCleanupResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentChildFinishedFact {
    pub call_id: String,
    pub outcome: AgentOutcome,
    pub accounting: ModelAccounting,
    pub handoff_content: String,
}

/// Durable orchestration truth for one Host-frozen child task.
///
/// The optional facts are a monotonic lifecycle, not independent flags. The
/// reducer enforces their order and identity. Keeping every prepared action
/// and committed result here lets recovery inspect the exact boundary without
/// deriving authority from presentation state or a model-authored prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTaskLifecycle {
    pub task: AgentTask,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_created: Option<AgentWorkspaceCreatedFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_started: Option<AgentChildStartedFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seal: Option<AgentSealLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<AgentOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration: Option<AgentIntegrationLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<AgentCleanupLifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished: Option<AgentChildFinishedFact>,
}

pub(crate) fn is_unintegrated_writer_recovery(lifecycle: &AgentTaskLifecycle) -> bool {
    lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
        && lifecycle
            .integration
            .as_ref()
            .is_none_or(|integration| integration.committed.is_none())
        && lifecycle.finished.as_ref().is_some_and(|finished| {
            matches!(
                finished.outcome.terminal,
                TerminalState::RecoveryRequired { .. }
            )
        })
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
    pub temporal_evidence_progress: Option<TemporalEvidenceProgress>,
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
    /// Canonical child/worktree lifecycle and recovery authority.
    pub agent_tasks: Vec<AgentTaskLifecycle>,
    pub pending_steers: Vec<PendingSteer>,
    pub pending_control: Option<PendingControl>,
    pub command_receipts: Vec<CommandReceipt>,
}

impl RunSnapshot {
    /// Derive unsettled child identities from the canonical task lifecycle,
    /// including tasks that failed between preparation and child start.
    #[must_use]
    pub fn pending_child_run_ids(&self) -> Vec<RunId> {
        self.agent_tasks
            .iter()
            .filter(|lifecycle| lifecycle.finished.is_none())
            .map(|lifecycle| lifecycle.task.child_run_id.clone())
            .collect()
    }
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
    request
        .validate_agent_task_binding()
        .map_err(|message| corrupt(&run_id, message))?;
    request
        .transcript
        .validate_complete_tool_history()
        .map_err(|message| corrupt(&run_id, message))?;
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
        temporal_evidence_progress: None,
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
        agent_tasks: Vec::new(),
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
    stored
        .event
        .validate_agent_lifecycle_payload()
        .map_err(|message| corrupt(&run_id, message))?;
    if snapshot.pending_control.is_some()
        && !matches!(
            &stored.event,
            RuntimeEventKind::ToolOutcomeCommitted { .. }
                | RuntimeEventKind::AgentWorkspaceCreated { .. }
                | RuntimeEventKind::AgentSealCommitted { .. }
                | RuntimeEventKind::AgentResultCollected { .. }
                | RuntimeEventKind::AgentIntegrationFailed { .. }
                | RuntimeEventKind::AgentIntegrationCommitted { .. }
                | RuntimeEventKind::AgentCleanupPrepared { .. }
                | RuntimeEventKind::AgentCleanupCommitted { .. }
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
            validate_model_failure_evidence(&run_id, failure)?;
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
            validate_tool_preparation_binding(snapshot, &run_id, invocation)?;
            snapshot.tool_calls = snapshot.tool_calls.saturating_add(1);
            snapshot.pending_tool = Some(PendingToolAction {
                operation_id: operation_id.clone(),
                invocation: invocation.clone(),
                workspace_access: *workspace_access,
                authorization: None,
                state: DurableActionState::Prepared,
                interaction: None,
            });
        }
        RuntimeEventKind::ToolAuthorizationCommitted {
            operation_id,
            decision,
        } => {
            let workspace_state = snapshot.workspace_state.clone();
            let permission_mode = snapshot.request.environment.permission_mode;
            let resolved_execution = {
                let pending = snapshot.pending_tool.as_ref().ok_or_else(|| {
                    corrupt(
                        &run_id,
                        "tool authorization committed without a prepared invocation",
                    )
                })?;
                resolve_tool_execution(snapshot.request.task_contract.as_ref(), &pending.invocation)
            };
            let execution_invocation = resolved_execution
                .invocation
                .map_err(|message| corrupt(&run_id, message))?;
            let pending = pending_tool_mut(snapshot, &run_id, operation_id)?;
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(
                    &run_id,
                    "tool authorization committed after execution began",
                ));
            }
            if pending.authorization.is_some() {
                return Err(corrupt(
                    &run_id,
                    "tool authorization committed more than once",
                ));
            }
            decision
                .validate_for(
                    &resolved_execution.grant,
                    &execution_invocation,
                    &workspace_state,
                )
                .map_err(|message| corrupt(&run_id, message))?;
            if decision.mode != permission_mode {
                return Err(corrupt(
                    &run_id,
                    "tool authorization mode differs from the frozen Run permission",
                ));
            }
            pending.authorization = Some(decision.clone());
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
                UserInteractionPrompt::Approval { prompt, arguments } => {
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
                    let Some(authorization) = pending.authorization.as_ref() else {
                        return Err(corrupt(
                            &run_id,
                            "approval interaction has no committed tool authorization",
                        ));
                    };
                    if authorization.disposition != ToolAuthorizationDisposition::Ask
                        || authorization.prompt.as_ref() != Some(prompt)
                    {
                        return Err(corrupt(
                            &run_id,
                            "approval interaction differs from committed authorization",
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
            if pending.invocation.name != AGENT_TOOL_NAME
                && pending.invocation.name != REQUEST_USER_INPUT_TOOL_NAME
            {
                let authorization = pending.authorization.as_ref().ok_or_else(|| {
                    corrupt(
                        &run_id,
                        "executable tool started without committed authorization",
                    )
                })?;
                match authorization.disposition {
                    ToolAuthorizationDisposition::Allow => {
                        if pending.interaction.is_some() {
                            return Err(corrupt(
                                &run_id,
                                "allowed tool unexpectedly carried an approval interaction",
                            ));
                        }
                    }
                    ToolAuthorizationDisposition::Ask => {
                        if !pending.interaction.as_ref().is_some_and(|interaction| {
                            matches!(
                                interaction.response,
                                Some(UserInteractionResponse::Approved)
                            )
                        }) {
                            return Err(corrupt(
                                &run_id,
                                "ask-authorized tool started without exact approval",
                            ));
                        }
                    }
                    ToolAuthorizationDisposition::Deny => {
                        return Err(corrupt(
                            &run_id,
                            "denied tool authorization reached execution start",
                        ));
                    }
                }
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
            let workspace_state_before = snapshot.workspace_state.clone();
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
            if pending.state == DurableActionState::InFlight
                && (outcome.invocation == ToolInvocationStatus::Rejected
                    || outcome.transport == ToolTransportStatus::NotStarted
                    || outcome.operation == ToolOperationStatus::NotStarted)
            {
                return Err(corrupt(
                    &run_id,
                    "an in-flight tool cannot commit a preflight rejection lifecycle",
                ));
            }
            if pending.state == DurableActionState::Prepared
                && pending.interaction.is_none()
                && outcome.invocation != ToolInvocationStatus::Rejected
            {
                return Err(corrupt(
                    &run_id,
                    "an unstarted tool can only commit a preflight rejection",
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
                    ) if outcome.failure_code != Some(ToolFailureCode::AuthorizationStale) => {
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
            let pending_workspace_access = pending.workspace_access;
            let pending_state = pending.state;
            if pending_workspace_access == WorkspaceAccess::ReadOnly
                && pending.invocation.name != AGENT_TOOL_NAME
                && pending.invocation.name != REQUEST_USER_INPUT_TOOL_NAME
                && matches!(
                    outcome.side_effect,
                    ToolSideEffectStatus::Applied | ToolSideEffectStatus::Indeterminate
                )
            {
                return Err(corrupt(
                    &run_id,
                    "read-only executable tool outcome cannot claim an applied or indeterminate workspace side effect",
                ));
            }
            let settles_agent_call = (pending.invocation.name == AGENT_TOOL_NAME)
                .then(|| pending.invocation.call_id.clone());
            let settled_agent_lifecycle = settles_agent_call.as_ref().and_then(|agent_call_id| {
                snapshot
                    .agent_tasks
                    .iter()
                    .find(|lifecycle| lifecycle.task.call_id == *agent_call_id)
            });
            if settles_agent_call.is_some() {
                match settled_agent_lifecycle {
                    None if outcome.operation != ToolOperationStatus::Succeeded
                        && outcome.side_effect == ToolSideEffectStatus::NotApplied => {}
                    None => {
                        return Err(corrupt(
                            &run_id,
                            "agent tool outcome without a lifecycle must prove preflight rejection without side effects",
                        ));
                    }
                    Some(lifecycle) => match lifecycle.task.workspace.access {
                        AgentWorkspaceAccess::ReadOnly
                            if lifecycle.child_started.is_none()
                                && lifecycle.finished.is_none() =>
                        {
                            return Err(corrupt(
                                &run_id,
                                "read-only agent launch outcome committed before child start",
                            ));
                        }
                        AgentWorkspaceAccess::IsolatedWrite if lifecycle.finished.is_none() => {
                            return Err(corrupt(
                                &run_id,
                                "writer agent tool outcome committed before its canonical child lifecycle finished",
                            ));
                        }
                        AgentWorkspaceAccess::ReadOnly | AgentWorkspaceAccess::IsolatedWrite => {}
                    },
                }
            }
            let rejected_before_lifecycle = settles_agent_call.is_some()
                && settled_agent_lifecycle.is_none()
                && outcome.operation != ToolOperationStatus::Succeeded
                && outcome.side_effect == ToolSideEffectStatus::NotApplied;
            let failed_writer_without_integration =
                settled_agent_lifecycle.is_some_and(|lifecycle| {
                    lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                        && lifecycle.integration.is_none()
                        && lifecycle.result.as_ref().is_some_and(|result| {
                            !matches!(result.terminal, TerminalState::Completed { .. })
                        })
                });
            let writer_integration_failed = settled_agent_lifecycle.is_some_and(|lifecycle| {
                lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                    && lifecycle
                        .integration
                        .as_ref()
                        .is_some_and(|integration| integration.failure.is_some())
            });
            let unintegrated_writer_recovery =
                settled_agent_lifecycle.is_some_and(is_unintegrated_writer_recovery);
            let integrated_writer_state = settled_agent_lifecycle
                .filter(|lifecycle| {
                    lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                })
                .and_then(|lifecycle| lifecycle.integration.as_ref())
                .and_then(|integration| integration.committed.as_ref())
                .map(|committed| committed.root_workspace_state_after.clone());
            match (pending_workspace_access, workspace_state) {
                (WorkspaceAccess::ReadOnly, None) => {}
                (WorkspaceAccess::ReadOnly, Some(_)) => {
                    return Err(corrupt(
                        &run_id,
                        "read-only tool outcome cannot advance workspace state",
                    ));
                }
                (WorkspaceAccess::MayWrite, None)
                    if (failed_writer_without_integration
                        || writer_integration_failed
                        || rejected_before_lifecycle)
                        && outcome.side_effect == ToolSideEffectStatus::NotApplied => {}
                (WorkspaceAccess::MayWrite, None)
                    if pending_state == DurableActionState::Prepared
                        && outcome.side_effect == ToolSideEffectStatus::NotApplied => {}
                (WorkspaceAccess::MayWrite, None)
                    if unintegrated_writer_recovery
                        && pending_state == DurableActionState::InFlight
                        && outcome.invocation == ToolInvocationStatus::Accepted
                        && outcome.transport == ToolTransportStatus::Indeterminate
                        && outcome.operation == ToolOperationStatus::Cancelled
                        && outcome.side_effect == ToolSideEffectStatus::Indeterminate
                        && outcome.retry == ToolRetryDisposition::Unsafe
                        && outcome.workspace_revision.is_none() => {}
                (WorkspaceAccess::MayWrite, None) => {
                    return Err(corrupt(
                        &run_id,
                        "workspace-mutating tool outcome must settle workspace state",
                    ));
                }
                (WorkspaceAccess::MayWrite, Some(state)) => {
                    if pending_state != DurableActionState::InFlight {
                        return Err(corrupt(
                            &run_id,
                            "unstarted workspace-mutating tool cannot settle workspace state",
                        ));
                    }
                    if settles_agent_call.is_some()
                        && integrated_writer_state
                            .as_ref()
                            .is_some_and(|integrated| integrated != state)
                    {
                        return Err(corrupt(
                            &run_id,
                            "writer agent tool outcome does not settle the integrated root state",
                        ));
                    }
                    if settles_agent_call.is_some()
                        && settled_agent_lifecycle.is_some_and(|lifecycle| {
                            lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                        })
                        && integrated_writer_state.is_none()
                    {
                        return Err(corrupt(
                            &run_id,
                            "unintegrated writer agent tool outcome cannot advance the root workspace",
                        ));
                    }
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
            let revision_changed =
                workspace_revision_changed(&workspace_state_before, &snapshot.workspace_state);
            if revision_changed && outcome.side_effect == ToolSideEffectStatus::NotApplied {
                return Err(corrupt(
                    &run_id,
                    "tool reporting no side effect cannot settle a changed workspace revision",
                ));
            }
            let is_temporal_verifier = snapshot
                .temporal_evidence_progress
                .as_ref()
                .is_some_and(|progress| name == &progress.verifier.verifier_id);
            let effective_mutation = pending_workspace_access == WorkspaceAccess::MayWrite
                && pending_state == DurableActionState::InFlight
                && outcome.invocation == ToolInvocationStatus::Accepted
                && outcome.transport == ToolTransportStatus::Succeeded
                && outcome.side_effect == ToolSideEffectStatus::Applied
                && !is_temporal_verifier
                && revision_changed;
            if revision_changed && !effective_mutation {
                snapshot.temporal_evidence_progress = None;
            } else if effective_mutation
                && let Some(progress) = snapshot.temporal_evidence_progress.as_mut()
            {
                let mutation = WorkspaceMutationEvidence {
                    operation_id: operation_id.0.clone(),
                    workspace_state_before: workspace_state_before.clone(),
                    workspace_state_after: snapshot.workspace_state.clone(),
                };
                mutation
                    .validate()
                    .map_err(|message| corrupt(&run_id, message))?;
                if progress.failure.workspace_state.generation
                    <= mutation.workspace_state_before.generation
                {
                    progress.mutation = Some(mutation);
                    progress
                        .validate()
                        .map_err(|message| corrupt(&run_id, message))?;
                }
            }
            let authorized_verifier_observation =
                outcome
                    .verifier_observation
                    .as_ref()
                    .is_none_or(|observation| {
                        pending_state == DurableActionState::InFlight
                            && name == &observation.spec.verifier_id
                            && outcome.invocation == ToolInvocationStatus::Accepted
                            && outcome.transport == ToolTransportStatus::Succeeded
                            && outcome.has_stable_verifier_revision(
                                &workspace_state_before.revision,
                                &snapshot.workspace_state.revision,
                            )
                            && verifier_artifacts_are_available(outcome, observation)
                            && matches!(
                                (observation.verdict, outcome.operation),
                                (VerifierVerdict::Passed, ToolOperationStatus::Succeeded)
                                    | (VerifierVerdict::Failed, ToolOperationStatus::Failed)
                            )
                    });
            if !authorized_verifier_observation {
                return Err(corrupt(
                    &run_id,
                    "tool verifier observation does not match an exact deterministic execution",
                ));
            }
            let deterministic_verifier_failure = pending_workspace_access
                == WorkspaceAccess::MayWrite
                && pending_state == DurableActionState::InFlight
                && outcome
                    .verifier_observation
                    .as_ref()
                    .is_some_and(|observation| name == &observation.spec.verifier_id);
            if deterministic_verifier_failure
                && let Some(progress) = failed_temporal_progress(
                    snapshot,
                    outcome,
                    FailedVerifierSource::Tool {
                        operation_id: operation_id.0.clone(),
                    },
                    &snapshot.workspace_state,
                    None,
                )
            {
                snapshot.temporal_evidence_progress = Some(progress);
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
            if snapshot.workspace_state.revision != workspace_state.revision {
                snapshot.temporal_evidence_progress = None;
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
            let pending = snapshot
                .pending_host_verification
                .as_ref()
                .cloned()
                .ok_or_else(|| {
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
            if pending.workspace_state_before != snapshot.workspace_state {
                return Err(corrupt(
                    &run_id,
                    "Host verification workspace changed after its exact action was prepared",
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
            let contract = snapshot
                .request
                .task_contract
                .as_ref()
                .expect("Agent contract validated at run creation");
            let generation_id = contract.generation_id.clone();
            let evidence_policy = contract
                .definition
                .acceptance
                .iter()
                .find_map(|acceptance| match acceptance {
                    TaskAcceptance::Verifier {
                        id,
                        evidence_policy,
                        verifier,
                        ..
                    } if *id == pending.acceptance_id && *verifier == pending.verifier => {
                        Some(*evidence_policy)
                    }
                    TaskAcceptance::Host { .. } | TaskAcceptance::Verifier { .. } => None,
                })
                .ok_or_else(|| {
                    corrupt(&run_id, "Host verification lost its verifier acceptance")
                })?;
            let expected = seal_evidence_receipt(
                snapshot,
                verification_id,
                &pending.acceptance_id,
                evidence_policy,
                &pending.verifier,
                outcome,
                workspace_state_after,
            );
            match (expected, receipt.as_deref()) {
                (Ok(expected), Some(receipt)) => {
                    if expected != *receipt {
                        return Err(corrupt(
                            &run_id,
                            "Host verification receipt does not equal the canonical seal result",
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
                    snapshot.temporal_evidence_progress = None;
                }
                (Ok(_), None) => {
                    return Err(corrupt(
                        &run_id,
                        "Host verification omitted a receipt that canonical sealing accepted",
                    ));
                }
                (Err(_), Some(_)) => {
                    return Err(corrupt(
                        &run_id,
                        "Host verification carried a receipt that canonical sealing rejected",
                    ));
                }
                (Err(cause), None) => {
                    if snapshot.workspace_state.revision != workspace_state_after.revision {
                        snapshot.temporal_evidence_progress = None;
                    }
                    let rejection = completion_rejection(
                        pending.candidate.id.clone(),
                        generation_id,
                        pending.acceptance_id.clone(),
                        &pending.verifier.verifier_id,
                        cause,
                    );
                    snapshot.last_host_verification_failure = Some(HostVerificationFailure {
                        outcome: (**outcome).clone(),
                        workspace_state: workspace_state_after.clone(),
                        rejection,
                    });
                    if let Some(progress) = failed_temporal_progress(
                        snapshot,
                        outcome,
                        FailedVerifierSource::Host {
                            verification_id: verification_id.clone(),
                        },
                        workspace_state_after,
                        Some((&pending.acceptance_id, &pending.verifier)),
                    ) {
                        snapshot.temporal_evidence_progress = Some(progress);
                    }
                }
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
            let contract = snapshot
                .request
                .task_contract
                .as_ref()
                .expect("Agent contract validated at run creation");
            if rejection.generation_id != contract.generation_id {
                return Err(corrupt(
                    &run_id,
                    "completion rejection belongs to another task generation",
                ));
            }
            if snapshot
                .last_host_verification_failure
                .as_ref()
                .is_none_or(|failure| failure.rejection != *rejection)
            {
                return Err(corrupt(
                    &run_id,
                    "completion rejection does not match the canonical Host verification failure",
                ));
            }
            snapshot.last_completion_rejection = Some(rejection.clone());
            snapshot.pending_completion = None;
        }
        RuntimeEventKind::AgentTaskPrepared { task } => {
            let pending = snapshot.pending_tool.as_ref().ok_or_else(|| {
                corrupt(
                    &run_id,
                    "Agent task prepared without an in-flight agent tool",
                )
            })?;
            let expected_access = match task.workspace.access {
                AgentWorkspaceAccess::ReadOnly => WorkspaceAccess::ReadOnly,
                AgentWorkspaceAccess::IsolatedWrite => WorkspaceAccess::MayWrite,
            };
            if task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                && snapshot.request.environment.write_execution_mode
                    != WriteExecutionMode::IsolatedWriter
            {
                return Err(corrupt(
                    &run_id,
                    "isolated writer task was prepared without explicit Writer admission",
                ));
            }
            if pending.state != DurableActionState::InFlight
                || pending.invocation.name != AGENT_TOOL_NAME
                || pending.invocation.call_id != task.call_id
                || pending.workspace_access != expected_access
            {
                return Err(corrupt(
                    &run_id,
                    "Agent task does not match the in-flight agent tool authority",
                ));
            }
            let expected_root_run_id = match snapshot.request.actor.kind {
                AgentActorKind::Root => &run_id,
                AgentActorKind::Child => {
                    &snapshot
                        .request
                        .agent_task
                        .as_ref()
                        .expect("child RunRequest binding validated at creation")
                        .root_run_id
                }
            };
            let expected_root_workspace = match snapshot.request.actor.kind {
                AgentActorKind::Root => snapshot.request.environment.workspace.as_str(),
                AgentActorKind::Child => snapshot
                    .request
                    .agent_task
                    .as_ref()
                    .expect("child RunRequest binding validated at creation")
                    .workspace
                    .root_workspace
                    .as_str(),
            };
            if task.parent_run_id != run_id
                || &task.root_run_id != expected_root_run_id
                || task.workspace.root_workspace != expected_root_workspace
            {
                return Err(corrupt(
                    &run_id,
                    "Agent task parent, root, or root workspace does not match this run",
                ));
            }
            if snapshot.agent_tasks.iter().any(|existing| {
                existing.task.task_id == task.task_id
                    || existing.task.child_run_id == task.child_run_id
                    || existing.task.call_id == task.call_id
            }) {
                return Err(corrupt(
                    &run_id,
                    "Agent task reused a task, child run, or tool call identity",
                ));
            }
            if task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                && snapshot.agent_tasks.iter().any(|existing| {
                    existing.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                })
            {
                return Err(corrupt(
                    &run_id,
                    "M6-A admits only one isolated writer task per root run",
                ));
            }
            snapshot.agent_tasks.push(AgentTaskLifecycle {
                task: (**task).clone(),
                workspace_created: None,
                child_started: None,
                seal: None,
                result: None,
                integration: None,
                cleanup: None,
                finished: None,
            });
        }
        RuntimeEventKind::AgentWorkspaceCreated {
            task_id,
            assignment,
            writer_workspace_state,
        } => {
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            if lifecycle.task.workspace.access != AgentWorkspaceAccess::IsolatedWrite
                || lifecycle.task.workspace != *assignment
                || lifecycle.workspace_created.is_some()
                || lifecycle.child_started.is_some()
            {
                return Err(corrupt(
                    &run_id,
                    "writer workspace creation does not match the prepared Agent task",
                ));
            }
            lifecycle.workspace_created = Some(AgentWorkspaceCreatedFact {
                assignment: assignment.clone(),
                writer_workspace_state: writer_workspace_state.clone(),
            });
        }
        RuntimeEventKind::ChildStarted {
            task_id,
            call_id,
            child_run_id,
            depth,
        } => {
            let expected_depth = snapshot.request.actor.depth.saturating_add(1);
            if snapshot.agent_tasks.iter().any(|lifecycle| {
                lifecycle.child_started.is_some()
                    && lifecycle.finished.is_none()
                    && lifecycle.task.child_run_id == *child_run_id
            }) {
                return Err(corrupt(&run_id, "child run started twice"));
            }
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let workspace_ready = match lifecycle.task.workspace.access {
                AgentWorkspaceAccess::ReadOnly => lifecycle.workspace_created.is_none(),
                AgentWorkspaceAccess::IsolatedWrite => lifecycle.workspace_created.is_some(),
            };
            if !workspace_ready
                || lifecycle.child_started.is_some()
                || lifecycle.task.call_id != *call_id
                || lifecycle.task.child_run_id != *child_run_id
                || *depth != expected_depth
            {
                return Err(corrupt(
                    &run_id,
                    "child start does not match its prepared task and workspace boundary",
                ));
            }
            lifecycle.child_started = Some(AgentChildStartedFact {
                call_id: call_id.clone(),
                child_run_id: child_run_id.clone(),
                depth: *depth,
            });
        }
        RuntimeEventKind::AgentSealPrepared {
            task_id,
            base_commit,
            writer_workspace_state_before,
        } => {
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let Some(created) = &lifecycle.workspace_created else {
                return Err(corrupt(
                    &run_id,
                    "writer seal prepared before workspace creation",
                ));
            };
            if lifecycle.task.workspace.access != AgentWorkspaceAccess::IsolatedWrite
                || lifecycle.child_started.is_none()
                || lifecycle.seal.is_some()
                || lifecycle.result.is_some()
                || base_commit != &lifecycle.task.workspace.base_commit
                || writer_workspace_state_before.generation
                    < created.writer_workspace_state.generation
            {
                return Err(corrupt(
                    &run_id,
                    "writer seal preparation does not match its started task",
                ));
            }
            lifecycle.seal = Some(AgentSealLifecycle {
                base_commit: base_commit.clone(),
                writer_workspace_state_before: writer_workspace_state_before.clone(),
                committed: None,
            });
        }
        RuntimeEventKind::AgentSealCommitted {
            task_id,
            final_commit,
            diff_sha256,
            changed_files,
            writer_workspace_state_after,
        } => {
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let seal = lifecycle
                .seal
                .as_mut()
                .ok_or_else(|| corrupt(&run_id, "writer seal committed before it was prepared"))?;
            if seal.committed.is_some()
                || lifecycle.result.is_some()
                || final_commit == &seal.base_commit
                || writer_workspace_state_after.generation
                    != seal
                        .writer_workspace_state_before
                        .generation
                        .saturating_add(1)
                || !paths_stay_within(changed_files, &lifecycle.task.workspace.allowed_paths)
            {
                return Err(corrupt(
                    &run_id,
                    "writer seal commit changed identity, scope, or generation",
                ));
            }
            seal.committed = Some(AgentSealCommittedFact {
                final_commit: final_commit.clone(),
                diff_sha256: diff_sha256.clone(),
                changed_files: changed_files.clone(),
                writer_workspace_state_after: writer_workspace_state_after.clone(),
            });
        }
        RuntimeEventKind::AgentResultCollected { task_id, outcome } => {
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let child_never_started = lifecycle.child_started.is_none();
            if lifecycle.result.is_some()
                || (child_never_started
                    && matches!(outcome.terminal, TerminalState::Completed { .. }))
            {
                return Err(corrupt(
                    &run_id,
                    "Agent result was collected more than once or claimed success before child start",
                ));
            }
            validate_collected_agent_result(&run_id, lifecycle, outcome)?;
            lifecycle.result = Some((**outcome).clone());
        }
        RuntimeEventKind::AgentIntegrationPrepared {
            task_id,
            integration_id,
            base_commit,
            writer_commit,
            diff_sha256,
            expected_root_workspace_state,
        } => {
            if expected_root_workspace_state != &snapshot.workspace_state {
                return Err(corrupt(
                    &run_id,
                    "writer integration expected state is not the current root workspace",
                ));
            }
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let seal = lifecycle
                .seal
                .as_ref()
                .and_then(|seal| seal.committed.as_ref())
                .ok_or_else(|| corrupt(&run_id, "writer integration prepared before sealing"))?;
            let result = lifecycle
                .result
                .as_ref()
                .ok_or_else(|| corrupt(&run_id, "writer integration prepared before result"))?;
            if lifecycle.integration.is_some()
                || !matches!(result.terminal, TerminalState::Completed { .. })
                || result.details.integration != WriterIntegrationStatus::AwaitingHost
                || base_commit != &lifecycle.task.workspace.base_commit
                || writer_commit != &seal.final_commit
                || diff_sha256 != &seal.diff_sha256
            {
                return Err(corrupt(
                    &run_id,
                    "writer integration preparation does not match the sealed successful result",
                ));
            }
            lifecycle.integration = Some(AgentIntegrationLifecycle {
                integration_id: integration_id.clone(),
                base_commit: base_commit.clone(),
                writer_commit: writer_commit.clone(),
                diff_sha256: diff_sha256.clone(),
                expected_root_workspace_state: expected_root_workspace_state.clone(),
                started: false,
                committed: None,
                failure: None,
            });
        }
        RuntimeEventKind::AgentIntegrationStarted {
            task_id,
            integration_id,
        } => {
            let current_root_workspace_state = snapshot.workspace_state.clone();
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let integration = lifecycle
                .integration
                .as_mut()
                .ok_or_else(|| corrupt(&run_id, "writer integration started before preparation"))?;
            if integration.integration_id != *integration_id
                || integration.started
                || integration.committed.is_some()
                || integration.failure.is_some()
                || integration.expected_root_workspace_state != current_root_workspace_state
            {
                return Err(corrupt(
                    &run_id,
                    "writer integration start does not match its prepared root state",
                ));
            }
            integration.started = true;
        }
        RuntimeEventKind::AgentIntegrationFailed {
            task_id,
            integration_id,
            status,
            root_workspace_state,
        } => {
            let current_root_workspace_state = snapshot.workspace_state.clone();
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let integration = lifecycle
                .integration
                .as_mut()
                .ok_or_else(|| corrupt(&run_id, "writer integration failed before preparation"))?;
            if integration.integration_id != *integration_id
                || !integration.started
                || integration.committed.is_some()
                || integration.failure.is_some()
                || integration.expected_root_workspace_state != current_root_workspace_state
                || integration.expected_root_workspace_state != *root_workspace_state
            {
                return Err(corrupt(
                    &run_id,
                    "writer integration failure does not match the in-flight expected root state",
                ));
            }
            integration.failure = Some(AgentIntegrationFailureFact {
                status: status.clone(),
                root_workspace_state: root_workspace_state.clone(),
            });
        }
        RuntimeEventKind::AgentIntegrationCommitted {
            task_id,
            integration_id,
            root_head_commit,
            root_workspace_state_after,
        } => {
            let current_root_workspace_state = snapshot.workspace_state.clone();
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let integration = lifecycle.integration.as_mut().ok_or_else(|| {
                corrupt(&run_id, "writer integration committed before preparation")
            })?;
            if integration.integration_id != *integration_id
                || !integration.started
                || integration.committed.is_some()
                || integration.failure.is_some()
                || integration.expected_root_workspace_state != current_root_workspace_state
                || root_head_commit != &integration.writer_commit
            {
                return Err(corrupt(
                    &run_id,
                    "writer integration commit does not match the in-flight action",
                ));
            }
            if root_workspace_state_after.generation
                != integration
                    .expected_root_workspace_state
                    .generation
                    .saturating_add(1)
            {
                return Err(corrupt(
                    &run_id,
                    "writer integration must advance exactly one generation",
                ));
            }
            integration.committed = Some(AgentIntegrationCommittedFact {
                root_head_commit: root_head_commit.clone(),
                root_workspace_state_after: root_workspace_state_after.clone(),
            });
        }
        RuntimeEventKind::AgentCleanupPrepared { task_id, plan } => {
            let tool_action_settled = snapshot.pending_tool.is_none();
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let result = lifecycle
                .result
                .as_ref()
                .ok_or_else(|| corrupt(&run_id, "writer cleanup prepared before result"))?;
            let successful = matches!(result.terminal, TerminalState::Completed { .. });
            let integration_committed = lifecycle
                .integration
                .as_ref()
                .is_some_and(|integration| integration.committed.is_some());
            let integration_failed = lifecycle
                .integration
                .as_ref()
                .is_some_and(|integration| integration.failure.is_some());
            let integration_settled = integration_committed ^ integration_failed;
            let cleanup_order_is_valid = if successful && integration_committed {
                lifecycle.finished.is_some() && tool_action_settled
            } else {
                lifecycle.finished.is_none()
            };
            if lifecycle.task.workspace.access != AgentWorkspaceAccess::IsolatedWrite
                || (lifecycle.workspace_created.is_none()
                    && plan.phase != WriterCleanupPhase::Binding)
                || lifecycle.cleanup.is_some()
                || (successful && !integration_settled)
                || (!successful && lifecycle.integration.is_some())
                || !cleanup_order_is_valid
            {
                return Err(corrupt(
                    &run_id,
                    "writer cleanup preparation does not match its owned completed path",
                ));
            }
            validate_writer_cleanup_plan(&run_id, lifecycle, plan)?;
            lifecycle.cleanup = Some(AgentCleanupLifecycle {
                plan: (**plan).clone(),
                committed: None,
            });
        }
        RuntimeEventKind::AgentCleanupCommitted { task_id, result } => {
            let lifecycle = agent_task_mut(snapshot, &run_id, task_id)?;
            let cleanup = lifecycle
                .cleanup
                .as_mut()
                .ok_or_else(|| corrupt(&run_id, "writer cleanup committed before preparation"))?;
            if cleanup.committed.is_some() {
                return Err(corrupt(
                    &run_id,
                    "writer cleanup commit does not match the prepared owned resources",
                ));
            }
            if let WriterCleanupMode::RetainForRecovery { uncertainty_code } = &cleanup.plan.mode {
                match result {
                    WriterCleanupResult::Retained {
                        uncertainty_code: result_code,
                        ..
                    } if result_code == uncertainty_code => {}
                    _ => {
                        return Err(corrupt(
                            &run_id,
                            "writer cleanup retention plan requires its exact uncertain result",
                        ));
                    }
                }
            }
            cleanup.committed = Some(result.clone());
        }
        RuntimeEventKind::ChildFinished {
            call_id,
            outcome,
            accounting,
            handoff_content,
        } => {
            let task_position = snapshot
                .agent_tasks
                .iter()
                .position(|lifecycle| lifecycle.task.child_run_id == outcome.run_id)
                .ok_or_else(|| corrupt(&run_id, "child finished without a prepared Agent task"))?;
            let lifecycle = &snapshot.agent_tasks[task_position];
            let expected_outcome = expected_finished_outcome(&run_id, lifecycle)?;
            if lifecycle.task.call_id != *call_id
                || expected_outcome != **outcome
                || lifecycle.finished.is_some()
            {
                return Err(corrupt(
                    &run_id,
                    "child finish does not match its collected and integrated result",
                ));
            }
            snapshot.agent_tasks[task_position].finished = Some(AgentChildFinishedFact {
                call_id: call_id.clone(),
                outcome: (**outcome).clone(),
                accounting: (**accounting).clone(),
                handoff_content: handoff_content.clone(),
            });
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
            if outcome.parent_run_id != snapshot.request.parent_run_id {
                return Err(corrupt(
                    &run_id,
                    "terminal outcome parent does not match the run",
                ));
            }
            let unsettled = snapshot
                .agent_tasks
                .iter()
                .filter(|lifecycle| lifecycle.finished.is_none())
                .collect::<Vec<_>>();
            let all_unsettled_are_read_only = !unsettled.is_empty()
                && unsettled.iter().all(|lifecycle| {
                    lifecycle.task.workspace.access == AgentWorkspaceAccess::ReadOnly
                });
            let exact_read_only_recovery = match &outcome.terminal {
                TerminalState::RecoveryRequired { ambiguity } if all_unsettled_are_read_only => {
                    match ambiguity.phase {
                        RecoveryAmbiguityPhase::ChildRun => unsettled
                            .iter()
                            .any(|lifecycle| lifecycle.task.child_run_id.0 == ambiguity.action_id),
                        RecoveryAmbiguityPhase::ToolExecution => {
                            snapshot.pending_tool.as_ref().is_some_and(|pending| {
                                pending.state == DurableActionState::InFlight
                                    && pending.invocation.name == AGENT_TOOL_NAME
                                    && pending.operation_id.0 == ambiguity.action_id
                                    && unsettled.iter().any(|lifecycle| {
                                        lifecycle.task.call_id == pending.invocation.call_id
                                            && lifecycle.child_started.is_some()
                                    })
                            })
                        }
                        RecoveryAmbiguityPhase::ModelRequest
                        | RecoveryAmbiguityPhase::HostVerification => false,
                    }
                }
                _ => false,
            };
            if !unsettled.is_empty() && !exact_read_only_recovery {
                return Err(corrupt(
                    &run_id,
                    "run reached terminal with an unsettled AgentTask lifecycle",
                ));
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
            validate_terminal_agent_cleanup(snapshot, &run_id, &outcome.terminal)?;
            if let TerminalState::Completed { decision, .. } = &outcome.terminal {
                validate_completion_decision(snapshot, &run_id, decision)?;
            }
            snapshot.accounting = outcome.accounting.clone();
            snapshot.terminal = Some((**outcome).clone());
        }
    }
    snapshot.last_sequence = stored.sequence;
    Ok(())
}

fn validate_writer_cleanup_plan(
    run_id: &RunId,
    lifecycle: &AgentTaskLifecycle,
    plan: &WriterCleanupPlan,
) -> Result<(), RunStoreError> {
    plan.validate()
        .map_err(|message| corrupt(run_id, format!("invalid writer cleanup plan: {message}")))?;
    let result = lifecycle
        .result
        .as_ref()
        .ok_or_else(|| corrupt(run_id, "writer cleanup plan has no collected result"))?;
    let successful = matches!(result.terminal, TerminalState::Completed { .. });
    let integration = lifecycle.integration.as_ref();
    let expected_phase = if successful {
        match integration {
            Some(integration) if integration.committed.is_some() => {
                WriterCleanupPhase::PostIntegration
            }
            Some(integration) if integration.failure.is_some() => WriterCleanupPhase::Integration,
            _ => {
                return Err(corrupt(
                    run_id,
                    "successful writer cleanup requires a settled integration",
                ));
            }
        }
    } else if lifecycle.child_started.is_none() {
        WriterCleanupPhase::Binding
    } else if lifecycle.seal.is_some() {
        WriterCleanupPhase::Seal
    } else {
        WriterCleanupPhase::Child
    };
    if plan.phase != expected_phase {
        return Err(corrupt(
            run_id,
            "writer cleanup phase does not match the durable lifecycle",
        ));
    }
    let expected_reason = match plan.phase {
        WriterCleanupPhase::PostIntegration => Some("writer_integrated"),
        WriterCleanupPhase::Integration => integration
            .and_then(|integration| integration.failure.as_ref())
            .map(|failure| match failure.status {
                WriterIntegrationStatus::Rejected { .. } => "writer_integration_rejected",
                WriterIntegrationStatus::Conflict { .. } => "writer_integration_conflict",
                WriterIntegrationStatus::RecoveryRequired { .. } => {
                    "writer_integration_recovery_required"
                }
                WriterIntegrationStatus::NotApplicable
                | WriterIntegrationStatus::AwaitingHost
                | WriterIntegrationStatus::Integrated { .. } => "writer_integration_invalid",
            }),
        WriterCleanupPhase::Binding => Some("writer_binding_failed"),
        WriterCleanupPhase::Seal => Some("writer_seal_failed"),
        WriterCleanupPhase::Child => Some(match &result.terminal {
            TerminalState::Blocked { .. } => "writer_child_blocked",
            TerminalState::Cancelled => "writer_child_cancelled",
            TerminalState::Interrupted => "writer_child_interrupted",
            TerminalState::RecoveryRequired { .. } => "writer_child_recovery_required",
            TerminalState::Failed { .. } | TerminalState::Completed { .. } => "writer_child_failed",
        }),
    };
    if expected_reason != Some(plan.reason_code.as_str()) {
        return Err(corrupt(
            run_id,
            "writer cleanup reason code does not match the durable failure disposition",
        ));
    }

    let committed_seal = lifecycle
        .seal
        .as_ref()
        .and_then(|seal| seal.committed.as_ref());
    match (&plan.artifact_state, committed_seal) {
        (
            WriterArtifactState::KnownHostSealed {
                final_commit,
                diff_sha256,
            },
            Some(seal),
        ) if final_commit == &seal.final_commit && diff_sha256 == &seal.diff_sha256 => {}
        (WriterArtifactState::KnownUnsealed, None) => {}
        (WriterArtifactState::Unknown, _)
            if matches!(plan.mode, WriterCleanupMode::RetainForRecovery { .. }) => {}
        _ => {
            return Err(corrupt(
                run_id,
                "writer cleanup artifact state does not match the durable seal",
            ));
        }
    }
    let expected_branch_commit = committed_seal
        .map(|seal| seal.final_commit.as_str())
        .unwrap_or(lifecycle.task.workspace.base_commit.as_str());
    match &plan.mode {
        WriterCleanupMode::RemoveExact {
            expected_branch_commit: actual,
        } if actual == expected_branch_commit => {}
        WriterCleanupMode::RetainForRecovery { uncertainty_code } => {
            if let WriterCleanupScope::Unknown {
                uncertainty_code: scope_code,
            } = &plan.scope
                && scope_code != uncertainty_code
            {
                return Err(corrupt(run_id, "writer cleanup retention codes disagree"));
            }
        }
        _ => {
            return Err(corrupt(
                run_id,
                "writer cleanup expected branch commit is not Host-frozen",
            ));
        }
    }
    Ok(())
}

fn agent_task_mut<'a>(
    snapshot: &'a mut RunSnapshot,
    run_id: &RunId,
    task_id: &AgentTaskId,
) -> Result<&'a mut AgentTaskLifecycle, RunStoreError> {
    snapshot
        .agent_tasks
        .iter_mut()
        .find(|lifecycle| lifecycle.task.task_id == *task_id)
        .ok_or_else(|| corrupt(run_id, "Agent lifecycle event has no prepared task"))
}

fn paths_stay_within(changed_files: &[String], allowed_paths: &[String]) -> bool {
    changed_files.iter().all(|changed| {
        allowed_paths.iter().any(|allowed| {
            changed == allowed
                || changed
                    .strip_prefix(allowed)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
    })
}

fn validate_collected_agent_result(
    run_id: &RunId,
    lifecycle: &AgentTaskLifecycle,
    outcome: &AgentOutcome,
) -> Result<(), RunStoreError> {
    if outcome.run_id != lifecycle.task.child_run_id
        || outcome.parent_run_id.as_ref() != Some(&lifecycle.task.parent_run_id)
    {
        return Err(corrupt(
            run_id,
            "collected Agent result does not match its child and parent identity",
        ));
    }
    if outcome
        .details
        .evidence
        .iter()
        .any(|receipt| receipt.generation_id != lifecycle.task.task_contract.generation_id)
    {
        return Err(corrupt(
            run_id,
            "collected Agent evidence belongs to another task generation",
        ));
    }
    match lifecycle.task.workspace.access {
        AgentWorkspaceAccess::ReadOnly => {
            if lifecycle.workspace_created.is_some()
                || lifecycle.seal.is_some()
                || lifecycle.integration.is_some()
                || lifecycle.cleanup.is_some()
                || outcome.details.workspace.as_ref() != Some(&lifecycle.task.workspace)
                || !matches!(
                    outcome.details.integration,
                    WriterIntegrationStatus::NotApplicable
                )
            {
                return Err(corrupt(
                    run_id,
                    "read-only Agent result carries writer lifecycle facts or a different workspace",
                ));
            }
        }
        AgentWorkspaceAccess::IsolatedWrite => {
            let successful = matches!(outcome.terminal, TerminalState::Completed { .. });
            let committed = lifecycle
                .seal
                .as_ref()
                .and_then(|seal| seal.committed.as_ref());
            if successful && committed.is_none() {
                return Err(corrupt(
                    run_id,
                    "successful writer result was collected before its seal committed",
                ));
            }
            match committed {
                Some(committed) => {
                    if outcome.details.workspace.as_ref() != Some(&lifecycle.task.workspace)
                        || outcome.details.workspace_state.as_ref()
                            != Some(&committed.writer_workspace_state_after)
                        || outcome.details.base_commit.as_deref()
                            != Some(lifecycle.task.workspace.base_commit.as_str())
                        || outcome.details.final_commit.as_deref()
                            != Some(committed.final_commit.as_str())
                        || outcome.details.diff_sha256.as_deref()
                            != Some(committed.diff_sha256.as_str())
                        || outcome.details.changed_files != committed.changed_files
                    {
                        return Err(corrupt(
                            run_id,
                            "writer result does not exactly match its sealed Host facts",
                        ));
                    }
                    if successful {
                        let expected_acceptance = lifecycle
                            .task
                            .task_contract
                            .definition
                            .acceptance
                            .iter()
                            .find_map(|acceptance| match acceptance {
                                TaskAcceptance::Verifier {
                                    id,
                                    evidence_policy,
                                    verifier,
                                    ..
                                } => Some((id, evidence_policy, verifier)),
                                TaskAcceptance::Host { .. } => None,
                            })
                            .ok_or_else(|| {
                                corrupt(
                                    run_id,
                                    "successful writer task has no exact verifier acceptance",
                                )
                            })?;
                        if outcome.details.evidence.len() != 1
                            || outcome.details.evidence.iter().any(|receipt| {
                                receipt.acceptance_id != *expected_acceptance.0
                                    || !receipt.lineage.satisfies(*expected_acceptance.1)
                                    || (*expected_acceptance.1
                                        == VerifierEvidencePolicy::FailedWritePass
                                        && !matches!(
                                            receipt.lineage,
                                            EvidenceLineage::FailedWritePass { .. }
                                        ))
                                    || receipt.verifier != *expected_acceptance.2
                                    || receipt.workspace_state
                                        != lifecycle
                                            .seal
                                            .as_ref()
                                            .expect("committed writer seal remains present")
                                            .writer_workspace_state_before
                            })
                        {
                            return Err(corrupt(
                                run_id,
                                "successful writer receipt does not verify the exact pre-seal child workspace",
                            ));
                        }
                        let receipt = &outcome.details.evidence[0];
                        let TerminalState::Completed { decision, .. } = &outcome.terminal else {
                            unreachable!("successful writer was matched above");
                        };
                        if decision.generation_id != lifecycle.task.task_contract.generation_id
                            || decision.workspace_state
                                != lifecycle
                                    .seal
                                    .as_ref()
                                    .expect("committed writer seal remains present")
                                    .writer_workspace_state_before
                            || decision.satisfied.as_slice()
                                != [AcceptanceSatisfaction::Evidence {
                                    acceptance_id: receipt.acceptance_id.clone(),
                                    receipt_id: receipt.id.clone(),
                                }]
                        {
                            return Err(corrupt(
                                run_id,
                                "successful writer completion decision is not backed by its exact pre-seal receipt",
                            ));
                        }
                    }
                    let valid_integration_state = if successful {
                        matches!(
                            outcome.details.integration,
                            WriterIntegrationStatus::AwaitingHost
                        )
                    } else {
                        matches!(
                            outcome.details.integration,
                            WriterIntegrationStatus::Rejected { .. }
                                | WriterIntegrationStatus::Conflict { .. }
                        )
                    };
                    if !valid_integration_state {
                        return Err(corrupt(
                            run_id,
                            "writer result has an invalid pre-integration disposition",
                        ));
                    }
                }
                None => {
                    if outcome.details.workspace.is_some()
                        || outcome.details.workspace_state.is_some()
                        || outcome.details.base_commit.is_some()
                        || outcome.details.final_commit.is_some()
                        || outcome.details.diff_sha256.is_some()
                        || !outcome.details.changed_files.is_empty()
                        || !matches!(
                            outcome.details.integration,
                            WriterIntegrationStatus::NotApplicable
                        )
                    {
                        return Err(corrupt(
                            run_id,
                            "unsealed failed writer result claims Host seal facts",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn expected_finished_outcome(
    run_id: &RunId,
    lifecycle: &AgentTaskLifecycle,
) -> Result<AgentOutcome, RunStoreError> {
    let mut expected = lifecycle
        .result
        .clone()
        .ok_or_else(|| corrupt(run_id, "child finished before its result was collected"))?;
    match lifecycle.task.workspace.access {
        AgentWorkspaceAccess::ReadOnly => {
            if lifecycle.workspace_created.is_some()
                || lifecycle.seal.is_some()
                || lifecycle.integration.is_some()
                || lifecycle.cleanup.is_some()
            {
                return Err(corrupt(
                    run_id,
                    "read-only child acquired a writer lifecycle",
                ));
            }
        }
        AgentWorkspaceAccess::IsolatedWrite => {
            let integration_committed = lifecycle
                .integration
                .as_ref()
                .is_some_and(|integration| integration.committed.is_some());
            let cleanup_must_precede_finish =
                !matches!(expected.terminal, TerminalState::Completed { .. })
                    || !integration_committed;
            if cleanup_must_precede_finish
                && lifecycle
                    .cleanup
                    .as_ref()
                    .and_then(|cleanup| cleanup.committed.as_ref())
                    .is_none()
            {
                return Err(corrupt(
                    run_id,
                    "writer child finished before owned workspace cleanup settled",
                ));
            }
            if matches!(expected.terminal, TerminalState::Completed { .. }) {
                let integration = lifecycle
                    .integration
                    .as_ref()
                    .ok_or_else(|| corrupt(run_id, "successful writer was not integrated"))?;
                match (&integration.committed, &integration.failure) {
                    (Some(committed), None) => {
                        expected.details.integration = WriterIntegrationStatus::Integrated {
                            integration_id: integration.integration_id.clone(),
                            writer_commit: integration.writer_commit.clone(),
                            root_workspace_state: committed.root_workspace_state_after.clone(),
                        };
                    }
                    (None, Some(failure)) => {
                        expected.details.integration = failure.status.clone();
                        match &failure.status {
                            WriterIntegrationStatus::Rejected { reason }
                            | WriterIntegrationStatus::Conflict { reason } => {
                                expected.terminal = TerminalState::Blocked {
                                    reason: reason.clone(),
                                };
                            }
                            WriterIntegrationStatus::RecoveryRequired { reason } => {
                                expected.terminal = TerminalState::RecoveryRequired {
                                    ambiguity: RecoveryAmbiguity {
                                        phase: RecoveryAmbiguityPhase::ChildRun,
                                        action_id: format!(
                                            "agent-integration:{}",
                                            integration.integration_id.0
                                        ),
                                        message: reason.clone(),
                                    },
                                };
                            }
                            WriterIntegrationStatus::NotApplicable
                            | WriterIntegrationStatus::AwaitingHost
                            | WriterIntegrationStatus::Integrated { .. } => {
                                return Err(corrupt(
                                    run_id,
                                    "writer integration failure has a non-failure status",
                                ));
                            }
                        }
                    }
                    (Some(_), Some(_)) => {
                        return Err(corrupt(
                            run_id,
                            "writer integration cannot both commit and fail",
                        ));
                    }
                    (None, None) => {
                        return Err(corrupt(
                            run_id,
                            "successful writer integration did not reach a durable outcome",
                        ));
                    }
                }
            } else if lifecycle.integration.is_some() {
                return Err(corrupt(
                    run_id,
                    "unsuccessful writer cannot carry an integration action",
                ));
            }
            if let Some(cleanup) = lifecycle
                .cleanup
                .as_ref()
                .and_then(|cleanup| cleanup.committed.as_ref())
                && cleanup.retained_for_recovery()
            {
                expected.terminal = TerminalState::RecoveryRequired {
                    ambiguity: RecoveryAmbiguity {
                        phase: RecoveryAmbiguityPhase::ChildRun,
                        action_id: format!("agent-cleanup:{}", lifecycle.task.task_id.0),
                        message: store_cleanup_uncertainty_code(cleanup)
                            .unwrap_or("writer_cleanup_uncertain")
                            .to_owned(),
                    },
                };
            }
        }
    }
    Ok(expected)
}

fn validate_terminal_agent_cleanup(
    snapshot: &RunSnapshot,
    run_id: &RunId,
    terminal: &TerminalState,
) -> Result<(), RunStoreError> {
    let recovery_terminal = matches!(terminal, TerminalState::RecoveryRequired { .. });
    for lifecycle in snapshot
        .agent_tasks
        .iter()
        .filter(|lifecycle| lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite)
    {
        let integrated = lifecycle
            .integration
            .as_ref()
            .is_some_and(|integration| integration.committed.is_some());
        let cleanup = lifecycle
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.committed.as_ref());
        if cleanup.is_none() {
            return Err(corrupt(
                run_id,
                if integrated {
                    "root terminal requires every integrated writer cleanup to commit"
                } else {
                    "root terminal requires every isolated writer cleanup to commit"
                },
            ));
        }
        let Some(cleanup) = cleanup else {
            continue;
        };
        if cleanup.retained_for_recovery() {
            let uncertainty_code =
                store_cleanup_uncertainty_code(cleanup).unwrap_or("writer_cleanup_uncertain");
            let exact_recovery = matches!(
                terminal,
                TerminalState::RecoveryRequired { ambiguity }
                    if ambiguity.phase == RecoveryAmbiguityPhase::ChildRun
                        && ambiguity.action_id
                            == format!("agent-cleanup:{}", lifecycle.task.task_id.0)
                        && ambiguity.message == uncertainty_code
            );
            if !recovery_terminal || !exact_recovery {
                return Err(corrupt(
                    run_id,
                    "retained writer cleanup requires its exact recovery-required root terminal",
                ));
            }
        }
    }
    Ok(())
}

fn store_cleanup_uncertainty_code(result: &WriterCleanupResult) -> Option<&str> {
    match result {
        WriterCleanupResult::Retained {
            uncertainty_code, ..
        } => Some(uncertainty_code),
        WriterCleanupResult::Removed { .. } | WriterCleanupResult::AlreadyAbsent => None,
    }
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
                TaskAcceptance::Verifier {
                    evidence_policy,
                    verifier,
                    ..
                },
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
                    || !receipt.lineage.satisfies(*evidence_policy)
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
                    && artifact
                        .validate_inline_verification()
                        .is_ok_and(|payload| {
                            payload.verifier == observation.spec
                                && payload.verdict == observation.verdict
                                && payload.workspace_revision == observation.workspace_revision
                        })
            })
        })
        && matches!(
            (&observation.workspace_revision, outcome.workspace_revision.as_deref()),
            (WorkspaceRevision::Known { sha256 }, Some(outcome_revision))
                if sha256 == outcome_revision
        )
}

fn workspace_revision_changed(before: &WorkspaceState, after: &WorkspaceState) -> bool {
    matches!(
        (&before.revision, &after.revision),
        (
            WorkspaceRevision::Known { sha256: before },
            WorkspaceRevision::Known { sha256: after }
        ) if before != after
    )
}

fn failed_temporal_progress(
    snapshot: &RunSnapshot,
    outcome: &ToolOutcome,
    source: FailedVerifierSource,
    workspace_state: &WorkspaceState,
    expected_acceptance: Option<(&AcceptanceId, &VerifierSpec)>,
) -> Option<TemporalEvidenceProgress> {
    let observation = outcome.verifier_observation.as_ref()?;
    if outcome.invocation != ToolInvocationStatus::Accepted
        || outcome.transport != ToolTransportStatus::Succeeded
        || outcome.operation != ToolOperationStatus::Failed
        || outcome.side_effect == ToolSideEffectStatus::Applied
        || observation.verdict != VerifierVerdict::Failed
        || !verifier_artifacts_are_available(outcome, observation)
        || observation.workspace_revision != workspace_state.revision
    {
        return None;
    }
    let contract = snapshot.request.task_contract.as_ref()?;
    let (acceptance_id, verifier) =
        contract
            .definition
            .acceptance
            .iter()
            .find_map(|acceptance| match acceptance {
                TaskAcceptance::Verifier {
                    id,
                    evidence_policy: VerifierEvidencePolicy::FailedWritePass,
                    verifier,
                    ..
                } if *verifier == observation.spec
                    && expected_acceptance.is_none_or(|(expected_id, expected_verifier)| {
                        id == expected_id && verifier == expected_verifier
                    }) =>
                {
                    Some((id, verifier))
                }
                TaskAcceptance::Host { .. } | TaskAcceptance::Verifier { .. } => None,
            })?;
    let failure = FailedVerifierEvidence {
        source,
        workspace_state: workspace_state.clone(),
        artifact_ids: observation.artifact_ids.clone(),
    };
    failure.validate().ok()?;
    let progress = TemporalEvidenceProgress {
        acceptance_id: acceptance_id.clone(),
        verifier: verifier.clone(),
        failure,
        mutation: None,
    };
    progress.validate().ok()?;
    Some(progress)
}

pub(crate) fn expected_evidence_lineage(
    snapshot: &RunSnapshot,
    acceptance_id: &AcceptanceId,
    verifier: &VerifierSpec,
    policy: VerifierEvidencePolicy,
) -> Option<EvidenceLineage> {
    match policy {
        VerifierEvidencePolicy::LatestPass => Some(EvidenceLineage::LatestPass),
        VerifierEvidencePolicy::FailedWritePass => {
            if let Some(progress) = &snapshot.temporal_evidence_progress {
                let mutation = progress.mutation.as_ref()?;
                if progress.acceptance_id != *acceptance_id
                    || progress.verifier != *verifier
                    || progress.failure.workspace_state.revision
                        == snapshot.workspace_state.revision
                    || mutation.workspace_state_after.generation
                        > snapshot.workspace_state.generation
                    || mutation.workspace_state_after.revision != snapshot.workspace_state.revision
                {
                    return None;
                }
                return Some(EvidenceLineage::FailedWritePass {
                    failure: progress.failure.clone(),
                    mutation: mutation.clone(),
                });
            }
            snapshot.agent_tasks.iter().rev().find_map(|lifecycle| {
                let integration = lifecycle.integration.as_ref()?;
                let committed = integration.committed.as_ref()?;
                if lifecycle.task.workspace.access != AgentWorkspaceAccess::IsolatedWrite
                    || committed.root_workspace_state_after != snapshot.workspace_state
                {
                    return None;
                }
                let child_receipt =
                    lifecycle
                        .result
                        .as_ref()?
                        .details
                        .evidence
                        .iter()
                        .find(|receipt| {
                            receipt.acceptance_id == *acceptance_id
                                && receipt.verifier == *verifier
                                && matches!(
                                    receipt.lineage,
                                    EvidenceLineage::FailedWritePass { .. }
                                )
                        })?;
                let integration_evidence = WorkspaceMutationEvidence {
                    operation_id: integration.integration_id.0.clone(),
                    workspace_state_before: integration.expected_root_workspace_state.clone(),
                    workspace_state_after: committed.root_workspace_state_after.clone(),
                };
                integration_evidence.validate().ok()?;
                Some(EvidenceLineage::DelegatedFailedWritePass {
                    child_run_id: lifecycle.task.child_run_id.0.clone(),
                    child_receipt_id: child_receipt.id.clone(),
                    integration: integration_evidence,
                })
            })
        }
    }
}

pub(crate) fn seal_evidence_receipt(
    snapshot: &RunSnapshot,
    verification_id: &VerificationId,
    acceptance_id: &AcceptanceId,
    evidence_policy: VerifierEvidencePolicy,
    verifier: &VerifierSpec,
    outcome: &ToolOutcome,
    workspace_state: &WorkspaceState,
) -> Result<EvidenceReceipt, EvidenceSealRejection> {
    let observation = outcome
        .verifier_observation
        .as_ref()
        .ok_or(EvidenceSealRejection::VerifierObservationMissing)?;
    if observation.spec != *verifier {
        return Err(EvidenceSealRejection::VerifierSpecMismatch);
    }
    if !outcome.has_stable_verifier_revision(
        &snapshot.workspace_state.revision,
        &workspace_state.revision,
    ) {
        return Err(EvidenceSealRejection::VerifierWorkspaceUnstable);
    }
    if !verifier_artifacts_are_available(outcome, observation) {
        return Err(EvidenceSealRejection::VerifierArtifactUnavailable);
    }
    match observation.verdict {
        VerifierVerdict::Passed if outcome.is_success() => {}
        VerifierVerdict::Failed
            if outcome.invocation == ToolInvocationStatus::Accepted
                && outcome.transport == ToolTransportStatus::Succeeded
                && outcome.operation == ToolOperationStatus::Failed =>
        {
            return Err(EvidenceSealRejection::VerifierFailed);
        }
        VerifierVerdict::Partial => {
            return Err(EvidenceSealRejection::VerifierIncomplete);
        }
        VerifierVerdict::Passed | VerifierVerdict::Failed => {
            return Err(EvidenceSealRejection::VerifierOutcomeInconsistent);
        }
    }
    let generation_id = snapshot
        .request
        .task_contract
        .as_ref()
        .ok_or(EvidenceSealRejection::EvidenceReceiptInvalid)?
        .generation_id
        .clone();
    let lineage = expected_evidence_lineage(snapshot, acceptance_id, verifier, evidence_policy)
        .ok_or(EvidenceSealRejection::EvidenceLineageUnavailable)?;
    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from(format!("receipt:{}", verification_id.0)),
        generation_id,
        acceptance_id: acceptance_id.clone(),
        verification_id: verification_id.clone(),
        verifier: verifier.clone(),
        workspace_state: workspace_state.clone(),
        artifact_ids: observation.artifact_ids.clone(),
        lineage,
    };
    receipt
        .validate()
        .map_err(|_| EvidenceSealRejection::EvidenceReceiptInvalid)?;
    Ok(receipt)
}

pub(crate) fn completion_rejection(
    candidate_id: CompletionCandidateId,
    generation_id: TaskGenerationId,
    acceptance_id: AcceptanceId,
    verifier_id: &str,
    cause: EvidenceSealRejection,
) -> CompletionRejection {
    let reason = match cause {
        EvidenceSealRejection::VerifierFailed => format!(
            "Host verifier '{verifier_id}' 确定性失败；必须先有效修改工作区，再在新 revision 提出完成"
        ),
        EvidenceSealRejection::VerifierSpecMismatch => format!(
            "Host verifier '{verifier_id}' 的实际执行规格与冻结 TaskContract 不一致；这是 Host 契约问题，禁止重复验证"
        ),
        EvidenceSealRejection::VerifierWorkspaceUnstable => format!(
            "Host verifier '{verifier_id}' 执行期间工作区 revision 不稳定；需要 Host 修复执行环境"
        ),
        EvidenceSealRejection::VerifierArtifactUnavailable => format!(
            "Host verifier '{verifier_id}' 没有产生完整、可校验的证据 artifact；需要 Host 修复执行链"
        ),
        EvidenceSealRejection::VerifierObservationMissing => format!(
            "Host verifier '{verifier_id}' 没有产生 typed verifier observation；需要 Host 修复执行链"
        ),
        EvidenceSealRejection::VerifierIncomplete => {
            format!("Host verifier '{verifier_id}' 只得到不完整结果；需要 Host 修复执行环境")
        }
        EvidenceSealRejection::VerifierOutcomeInconsistent => format!(
            "Host verifier '{verifier_id}' 的 verdict 与工具结果不一致；需要 Host 修复执行链"
        ),
        EvidenceSealRejection::EvidenceLineageUnavailable => format!(
            "Host verifier '{verifier_id}' 已通过，但缺少 TaskContract 要求的有序失败—修改—通过证据；禁止伪造完成"
        ),
        EvidenceSealRejection::EvidenceReceiptInvalid => format!(
            "Host verifier '{verifier_id}' 的 EvidenceReceipt 无法通过 canonical 校验；需要 Host 修复验收链"
        ),
    };
    CompletionRejection {
        candidate_id,
        generation_id,
        unmet_acceptance_ids: vec![acceptance_id],
        cause,
        required_transition: cause.required_transition(),
        reason,
    }
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
    if !failure.retry_safe {
        return Some(ModelRetryStopReason::UnsafeReplay);
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

fn validate_model_failure_evidence(
    run_id: &RunId,
    failure: &ModelAttemptFailure,
) -> Result<(), RunStoreError> {
    let response = failure.response;
    if failure.actionable_output != response.actionable_output()
        || failure.retry_safe != response.replay_safe()
    {
        return Err(corrupt(
            run_id,
            "persisted model failure summary disagrees with response evidence",
        ));
    }
    if (response.tool_call_id_observed
        || response.tool_call_name_observed
        || response.tool_call_arguments_observed)
        && !response.tool_call_observed
    {
        return Err(corrupt(
            run_id,
            "persisted model failure has tool fragments without a tool call",
        ));
    }
    if response.finish_reason_trusted
        && (!response.finish_reason_observed || !response.stream_done_received)
    {
        return Err(corrupt(
            run_id,
            "persisted model failure trusts a finish reason without [DONE]",
        ));
    }
    if (response.actionable_output()
        || response.finish_reason_observed
        || response.usage_received
        || response.stream_done_received)
        && !response.response_headers_received
    {
        return Err(corrupt(
            run_id,
            "persisted model response evidence exists before response headers",
        ));
    }
    Ok(())
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
        || snapshot
            .agent_tasks
            .iter()
            .any(|lifecycle| lifecycle.finished.is_none())
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
        || snapshot
            .agent_tasks
            .iter()
            .any(|lifecycle| lifecycle.finished.is_none())
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

fn validate_tool_preparation_binding(
    snapshot: &RunSnapshot,
    run_id: &RunId,
    invocation: &ToolInvocation,
) -> Result<(), RunStoreError> {
    if &invocation.run_id != run_id {
        return Err(corrupt(
            run_id,
            "prepared tool invocation run id does not match its canonical run",
        ));
    }
    let Some(assistant_index) = snapshot
        .transcript
        .entries
        .iter()
        .rposition(|entry| matches!(entry, TranscriptEntry::Assistant { .. }))
    else {
        return Err(corrupt(
            run_id,
            "prepared tool invocation has no assistant turn",
        ));
    };
    let TranscriptEntry::Assistant { tool_calls, .. } =
        &snapshot.transcript.entries[assistant_index]
    else {
        unreachable!("latest assistant index was selected above");
    };
    let matching_calls = tool_calls
        .iter()
        .filter(|call| call.id == invocation.call_id)
        .collect::<Vec<_>>();
    let [call] = matching_calls.as_slice() else {
        return Err(corrupt(
            run_id,
            "prepared tool invocation does not bind one unique call in the latest assistant turn",
        ));
    };
    if call.name != invocation.name || call.arguments != invocation.arguments {
        return Err(corrupt(
            run_id,
            "prepared tool invocation changed the latest assistant call identity or arguments",
        ));
    }
    if snapshot.transcript.entries[assistant_index + 1..]
        .iter()
        .any(|entry| {
            matches!(
                entry,
                TranscriptEntry::Tool { call_id, .. } if call_id == &invocation.call_id
            )
        })
    {
        return Err(corrupt(
            run_id,
            "prepared tool invocation repeats an already settled call in the latest assistant turn",
        ));
    }
    Ok(())
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
    snapshot: RunSnapshot,
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
            if existing.command_sha256 != command_sha256 {
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
            let source = replay_in_memory_run(source)?;
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
                snapshot: replay.snapshot.clone(),
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
        let replay = replay_in_memory_run(run)?;
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
        let mut candidate_snapshot = run.snapshot.clone();
        apply_event(&mut candidate_snapshot, &stored)?;
        if stored.event.is_terminal() {
            run.lease = None;
        }
        run.events.push(stored.clone());
        run.snapshot = candidate_snapshot;
        Ok(stored)
    }

    async fn load(&self, run_id: &RunId) -> Result<Option<RunReplay>, RunStoreError> {
        self.runs
            .lock()
            .await
            .get(run_id)
            .map(replay_in_memory_run)
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
            let replay = replay_in_memory_run(run)?;
            let snapshot = replay.snapshot;
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
        let replay = replay_in_memory_run(run)?;
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

fn replay_in_memory_run(run: &InMemoryRun) -> Result<RunReplay, RunStoreError> {
    let replay = replay(run.events.clone())?;
    if replay.snapshot != run.snapshot {
        let run_id = run.snapshot.request.run_id.clone().unwrap_or_default();
        return Err(corrupt(
            &run_id,
            "cached in-memory snapshot diverges from full event replay",
        ));
    }
    Ok(replay)
}

fn new_lease(run_id: RunId, epoch: u64) -> RunLease {
    RunLease {
        run_id,
        epoch,
        owner_id: RuntimeEventId::new().0,
        owner_pid: std::process::id(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const FINAL: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIFF: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    fn known(generation: u64, revision: &str) -> WorkspaceState {
        WorkspaceState {
            generation,
            revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
        }
    }

    fn root_request() -> RunRequest {
        let run_id = RunId::from("root");
        let mut request = RunRequest::new(
            TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: TaskDefinition::host("root task"),
            },
            "system",
        );
        request.run_id = Some(run_id);
        request.environment.workspace = "/repo".to_owned();
        request.environment.write_execution_mode = WriteExecutionMode::IsolatedWriter;
        request
    }

    fn writer_task() -> AgentTask {
        let child_run_id = RunId::from("child");
        AgentTask {
            task_id: AgentTaskId::from("task-1"),
            root_run_id: RunId::from("root"),
            parent_run_id: RunId::from("root"),
            child_run_id: child_run_id.clone(),
            call_id: "call-1".to_owned(),
            role: "writer".to_owned(),
            task_contract: TaskContract {
                generation_id: TaskGenerationId::from(child_run_id.0),
                definition: TaskDefinition {
                    objective: "write one file".to_owned(),
                    constraints: Vec::new(),
                    non_goals: Vec::new(),
                    acceptance: vec![TaskAcceptance::Verifier {
                        id: AcceptanceId::from("host"),
                        description: "exact child verifier".to_owned(),
                        evidence_policy: VerifierEvidencePolicy::LatestPass,
                        verifier: verifier(),
                    }],
                },
            },
            workspace: AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::IsolatedWrite,
                root_workspace: "/repo".to_owned(),
                root_branch: Some("deepseek-agent".to_owned()),
                base_commit: BASE.to_owned(),
                worktree_path: Some("/tmp/dse-writer".to_owned()),
                branch: Some("dse/writer/task-1".to_owned()),
                allowed_paths: vec!["src/lib.rs".to_owned()],
                owner_token: Some("owner-1".to_owned()),
            },
            model: "deepseek-v4-pro".to_owned(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: None,
            context_policy: ContextPolicy::default(),
            route: ModelRouteAudit {
                profile: ModelRouteProfile::Explicit,
                policy_version: "runtime_explicit_v1".to_owned(),
                reason_code: "explicit_model".to_owned(),
            },
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            deadline_unix_ms: None,
            expected_artifact: "sealed commit".to_owned(),
        }
    }

    fn verifier() -> VerifierSpec {
        VerifierSpec {
            verifier_id: "fixture".to_owned(),
            parameters: json!({}),
            plan: VerifierPlan {
                steps: vec![VerifierStep {
                    id: "check".to_owned(),
                    program: "true".to_owned(),
                    args: Vec::new(),
                    cwd: String::new(),
                    env: BTreeMap::new(),
                    timeout_ms: 1_000,
                }],
            },
        }
    }

    fn temporal_request() -> RunRequest {
        let mut request = root_request();
        request.environment.write_execution_mode = WriteExecutionMode::Root;
        request
            .task_contract
            .as_mut()
            .expect("root contract")
            .definition
            .acceptance = vec![TaskAcceptance::Verifier {
            id: AcceptanceId::from("temporal"),
            description: "failure then effective write then pass".to_owned(),
            evidence_policy: VerifierEvidencePolicy::FailedWritePass,
            verifier: verifier(),
        }];
        request
    }

    fn failed_verifier_outcome(revision: &str) -> ToolOutcome {
        let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
            summary: "deterministic verifier failure".to_owned(),
            verifier: verifier(),
            verdict: VerifierVerdict::Failed,
            workspace_revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
        });
        let artifact_id = artifact.id.clone();
        let mut outcome = ToolOutcome::error("deterministic verifier failure");
        outcome.side_effect = ToolSideEffectStatus::NotApplied;
        outcome.workspace_revision = Some(revision.to_owned());
        outcome.evidence = ToolEvidence {
            status: ToolEvidenceStatus::Produced,
            references: vec![artifact_id.clone()],
        };
        outcome.artifacts = vec![artifact];
        outcome.verifier_observation = Some(VerifierObservation {
            spec: verifier(),
            verdict: VerifierVerdict::Failed,
            workspace_revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
            artifact_ids: vec![artifact_id],
        });
        outcome
    }

    fn attach_failed_verifier_evidence(target: &mut ToolOutcome, source: &ToolOutcome) {
        target.evidence = source.evidence.clone();
        target.artifacts = source.artifacts.clone();
        target.workspace_revision = source.workspace_revision.clone();
        target.verifier_observation = source.verifier_observation.clone();
    }

    fn temporal_failure_kinds() -> Vec<RuntimeEventKind> {
        let operation_id = OperationId::from("failed-verifier");
        vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(temporal_request()),
            },
            RuntimeEventKind::WorkspaceObserved {
                workspace_state: known(1, "revision-a"),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: operation_id.clone(),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: "failed-verifier".to_owned(),
                    name: "fixture".to_owned(),
                    arguments: ToolArguments::from_value(json!({"verifier_id": "temporal"})),
                },
                workspace_access: WorkspaceAccess::MayWrite,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: operation_id.clone(),
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: "failed-verifier".to_owned(),
                name: "fixture".to_owned(),
                outcome: Box::new(failed_verifier_outcome("revision-a")),
                workspace_state: Some(known(2, "revision-a")),
            },
        ]
    }

    fn push_effective_write(
        kinds: &mut Vec<RuntimeEventKind>,
        operation: &str,
        workspace_after: WorkspaceState,
    ) {
        let operation_id = OperationId::from(operation);
        kinds.push(RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: RunId::from("root"),
                call_id: operation.to_owned(),
                name: "write".to_owned(),
                arguments: ToolArguments::from_value(json!({})),
            },
            workspace_access: WorkspaceAccess::MayWrite,
        });
        kinds.push(RuntimeEventKind::ToolExecutionStarted {
            operation_id: operation_id.clone(),
        });
        kinds.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id,
            call_id: operation.to_owned(),
            name: "write".to_owned(),
            outcome: Box::new(
                ToolOutcome::success("workspace changed")
                    .with_side_effect(ToolSideEffectStatus::Applied),
            ),
            workspace_state: Some(workspace_after),
        });
    }

    fn child_receipt() -> EvidenceReceipt {
        EvidenceReceipt {
            id: EvidenceReceiptId::from("child-receipt"),
            generation_id: TaskGenerationId::from("child"),
            acceptance_id: AcceptanceId::from("host"),
            verification_id: VerificationId::from("child-verification"),
            verifier: verifier(),
            workspace_state: known(1, "writer-dirty"),
            artifact_ids: vec!["child-artifact".to_owned()],
            lineage: EvidenceLineage::LatestPass,
        }
    }

    fn collected_writer_outcome() -> AgentOutcome {
        AgentOutcome {
            run_id: RunId::from("child"),
            parent_run_id: Some(RunId::from("root")),
            terminal: TerminalState::Completed {
                message: "done".to_owned(),
                decision: CompletionDecision {
                    candidate_id: CompletionCandidateId::from("child-candidate"),
                    generation_id: TaskGenerationId::from("child"),
                    workspace_state: known(1, "writer-dirty"),
                    satisfied: vec![AcceptanceSatisfaction::Evidence {
                        acceptance_id: AcceptanceId::from("host"),
                        receipt_id: EvidenceReceiptId::from("child-receipt"),
                    }],
                },
            },
            accounting: ModelAccounting::default(),
            runtime_model_requests: 1,
            runtime_retries: 0,
            tool_calls: 1,
            details: AgentResultDetails {
                summary: "changed one file".to_owned(),
                evidence: vec![child_receipt()],
                changed_files: vec!["src/lib.rs".to_owned()],
                checks: Vec::new(),
                unresolved: Vec::new(),
                artifacts: Vec::new(),
                workspace: Some(writer_task().workspace),
                workspace_state: Some(known(2, "writer-sealed")),
                base_commit: Some(BASE.to_owned()),
                final_commit: Some(FINAL.to_owned()),
                diff_sha256: Some(DIFF.to_owned()),
                integration: WriterIntegrationStatus::AwaitingHost,
            },
        }
    }

    fn integrated_writer_outcome() -> AgentOutcome {
        let mut outcome = collected_writer_outcome();
        outcome.details.integration = WriterIntegrationStatus::Integrated {
            integration_id: OperationId("integrate-1".to_owned()),
            writer_commit: FINAL.to_owned(),
            root_workspace_state: known(2, "root-integrated"),
        };
        outcome
    }

    fn read_only_task() -> AgentTask {
        let child_run_id = RunId::from("child");
        AgentTask {
            task_id: AgentTaskId::from("task-1"),
            root_run_id: RunId::from("root"),
            parent_run_id: RunId::from("root"),
            child_run_id: child_run_id.clone(),
            call_id: "call-1".to_owned(),
            role: "explorer".to_owned(),
            task_contract: TaskContract {
                generation_id: TaskGenerationId::from(child_run_id.0),
                definition: TaskDefinition::host("inspect one file"),
            },
            workspace: AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::ReadOnly,
                root_workspace: "/repo".to_owned(),
                root_branch: None,
                base_commit: BASE.to_owned(),
                worktree_path: None,
                branch: None,
                allowed_paths: Vec::new(),
                owner_token: None,
            },
            model: "deepseek-v4-flash".to_owned(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: None,
            context_policy: ContextPolicy::default(),
            route: ModelRouteAudit {
                profile: ModelRouteProfile::FixedActor,
                policy_version: "deepseek_fixed_actor_v1".to_owned(),
                reason_code: "fixed_read_only_investigation".to_owned(),
            },
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            deadline_unix_ms: None,
            expected_artifact: "analysis".to_owned(),
        }
    }

    fn read_only_outcome() -> AgentOutcome {
        AgentOutcome {
            run_id: RunId::from("child"),
            parent_run_id: Some(RunId::from("root")),
            terminal: TerminalState::Blocked {
                reason: "reported findings".to_owned(),
            },
            accounting: ModelAccounting::default(),
            runtime_model_requests: 1,
            runtime_retries: 0,
            tool_calls: 1,
            details: AgentResultDetails {
                summary: "read-only findings".to_owned(),
                workspace: Some(read_only_task().workspace),
                workspace_state: Some(known(0, "root-read-view")),
                ..AgentResultDetails::default()
            },
        }
    }

    fn event_kinds() -> Vec<RuntimeEventKind> {
        let task = writer_task();
        vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::WorkspaceObserved {
                workspace_state: known(1, "root-base"),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: task.call_id.clone(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::MayWrite,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("agent-operation".to_owned()),
            },
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(task.clone()),
            },
            RuntimeEventKind::AgentWorkspaceCreated {
                task_id: task.task_id.clone(),
                assignment: task.workspace.clone(),
                writer_workspace_state: known(0, "writer-created"),
            },
            RuntimeEventKind::ChildStarted {
                task_id: task.task_id.clone(),
                call_id: task.call_id.clone(),
                child_run_id: task.child_run_id.clone(),
                depth: 1,
            },
            RuntimeEventKind::AgentSealPrepared {
                task_id: task.task_id.clone(),
                base_commit: BASE.to_owned(),
                writer_workspace_state_before: known(1, "writer-dirty"),
            },
            RuntimeEventKind::AgentSealCommitted {
                task_id: task.task_id.clone(),
                final_commit: FINAL.to_owned(),
                diff_sha256: DIFF.to_owned(),
                changed_files: vec!["src/lib.rs".to_owned()],
                writer_workspace_state_after: known(2, "writer-sealed"),
            },
            RuntimeEventKind::AgentResultCollected {
                task_id: task.task_id.clone(),
                outcome: Box::new(collected_writer_outcome()),
            },
            RuntimeEventKind::AgentIntegrationPrepared {
                task_id: task.task_id.clone(),
                integration_id: OperationId("integrate-1".to_owned()),
                base_commit: BASE.to_owned(),
                writer_commit: FINAL.to_owned(),
                diff_sha256: DIFF.to_owned(),
                expected_root_workspace_state: known(1, "root-base"),
            },
            RuntimeEventKind::AgentIntegrationStarted {
                task_id: task.task_id.clone(),
                integration_id: OperationId("integrate-1".to_owned()),
            },
            RuntimeEventKind::AgentIntegrationCommitted {
                task_id: task.task_id.clone(),
                integration_id: OperationId("integrate-1".to_owned()),
                root_head_commit: FINAL.to_owned(),
                root_workspace_state_after: known(2, "root-integrated"),
            },
            RuntimeEventKind::ChildFinished {
                call_id: task.call_id.clone(),
                outcome: Box::new(integrated_writer_outcome()),
                accounting: Box::new(ModelAccounting::default()),
                handoff_content: "writer integrated".to_owned(),
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId("agent-operation".to_owned()),
                call_id: task.call_id.clone(),
                name: AGENT_TOOL_NAME.to_owned(),
                outcome: Box::new(
                    ToolOutcome::success("writer integrated")
                        .with_side_effect(ToolSideEffectStatus::Applied),
                ),
                workspace_state: Some(known(2, "root-integrated")),
            },
            RuntimeEventKind::AgentCleanupPrepared {
                task_id: task.task_id.clone(),
                plan: Box::new(test_cleanup_plan(WriterCleanupPhase::PostIntegration)),
            },
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task.task_id.clone(),
                result: WriterCleanupResult::Removed {
                    worktree: WriterRemovalState::Removed,
                    branch: WriterRemovalState::Removed,
                },
            },
        ]
    }

    fn test_cleanup_plan(phase: WriterCleanupPhase) -> WriterCleanupPlan {
        WriterCleanupPlan {
            phase,
            reason_code: "writer_integrated".to_owned(),
            ownership: WriterCleanupOwnership::Known {
                identity_sha256: "c".repeat(64),
            },
            artifact_state: WriterArtifactState::KnownHostSealed {
                final_commit: FINAL.to_owned(),
                diff_sha256: DIFF.to_owned(),
            },
            scope: WriterCleanupScope::Known {
                workspace_revision: WorkspaceRevision::Known {
                    sha256: "a".repeat(64),
                },
                changed_count: 1,
                in_scope_count: 1,
                out_of_scope_count: 0,
                path_set_sha256: "b".repeat(64),
            },
            mode: WriterCleanupMode::RemoveExact {
                expected_branch_commit: FINAL.to_owned(),
            },
        }
    }

    fn outcome_after_integration_failure(status: WriterIntegrationStatus) -> AgentOutcome {
        let mut outcome = collected_writer_outcome();
        outcome.details.integration = status.clone();
        outcome.terminal = match status {
            WriterIntegrationStatus::Rejected { reason }
            | WriterIntegrationStatus::Conflict { reason } => TerminalState::Blocked { reason },
            WriterIntegrationStatus::RecoveryRequired { reason } => {
                TerminalState::RecoveryRequired {
                    ambiguity: RecoveryAmbiguity {
                        phase: RecoveryAmbiguityPhase::ChildRun,
                        action_id: "agent-integration:integrate-1".to_owned(),
                        message: reason,
                    },
                }
            }
            WriterIntegrationStatus::NotApplicable
            | WriterIntegrationStatus::AwaitingHost
            | WriterIntegrationStatus::Integrated { .. } => {
                panic!("failure fixture requires a failure status")
            }
        };
        outcome
    }

    fn failed_integration_kinds(status: WriterIntegrationStatus) -> Vec<RuntimeEventKind> {
        let mut kinds = event_kinds();
        let cleanup_reason = match &status {
            WriterIntegrationStatus::Rejected { .. } => "writer_integration_rejected",
            WriterIntegrationStatus::Conflict { .. } => "writer_integration_conflict",
            WriterIntegrationStatus::RecoveryRequired { .. } => {
                "writer_integration_recovery_required"
            }
            WriterIntegrationStatus::NotApplicable
            | WriterIntegrationStatus::AwaitingHost
            | WriterIntegrationStatus::Integrated { .. } => {
                panic!("failure fixture requires a failure status")
            }
        };
        kinds[12] = RuntimeEventKind::AgentIntegrationFailed {
            task_id: AgentTaskId::from("task-1"),
            integration_id: OperationId("integrate-1".to_owned()),
            status: status.clone(),
            root_workspace_state: known(1, "root-base"),
        };
        let finished = kinds.remove(13);
        let tool_outcome = kinds.remove(13);
        assert!(matches!(
            tool_outcome,
            RuntimeEventKind::ToolOutcomeCommitted { .. }
        ));
        let RuntimeEventKind::AgentCleanupPrepared { plan, .. } = &mut kinds[13] else {
            panic!("fixture cleanup plan");
        };
        plan.phase = WriterCleanupPhase::Integration;
        plan.reason_code = cleanup_reason.to_owned();
        kinds.push(finished);
        let RuntimeEventKind::ChildFinished { outcome, .. } = &mut kinds[15] else {
            panic!("fixture child finish");
        };
        **outcome = outcome_after_integration_failure(status);
        kinds
    }

    fn stored_events(kinds: Vec<RuntimeEventKind>) -> Vec<StoredRuntimeEvent> {
        fn push(events: &mut Vec<StoredRuntimeEvent>, event: RuntimeEventKind) {
            let index = events.len();
            events.push(StoredRuntimeEvent {
                schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
                run_id: RunId::from("root"),
                parent_run_id: None,
                event_id: if index == 0 {
                    RuntimeEventId::run_created()
                } else {
                    RuntimeEventId(format!("event-{index}"))
                },
                sequence: u64::try_from(index).unwrap() + 1,
                occurred_at_unix_ms: 1,
                event,
            });
        }

        let mut events = Vec::new();
        for kind in kinds {
            let prepared = if let RuntimeEventKind::ToolPrepared {
                operation_id,
                invocation,
                ..
            } = &kind
            {
                let snapshot = reduce_events(&events).expect("canonical fixture prefix");
                let tools = vec![ToolDefinition {
                    name: invocation.name.clone(),
                    description: "fixture tool".to_owned(),
                    input_schema: json!({"type": "object"}),
                }];
                let context = effective_context(context_input(&snapshot, &tools))
                    .expect("fixture model context");
                let request_number = snapshot.local_turns.saturating_add(1);
                let attempt_id = AttemptId(format!("fixture-attempt-{request_number}"));
                let request = ModelRequest {
                    run_id: RunId::from("root"),
                    parent_run_id: snapshot.request.parent_run_id.clone(),
                    actor: snapshot.request.actor,
                    model: snapshot.request.model.clone(),
                    system_prompt: context.system_prompt,
                    messages: context.messages,
                    tools,
                    reasoning_effort: snapshot.request.reasoning_effort,
                    max_output_tokens: snapshot.request.max_output_tokens,
                    streaming: snapshot.request.streaming,
                    request_number,
                    attempt: 0,
                };
                push(
                    &mut events,
                    RuntimeEventKind::ModelRequestPrepared {
                        attempt_id: attempt_id.clone(),
                        request: Box::new(request),
                    },
                );
                push(
                    &mut events,
                    RuntimeEventKind::ModelRequestInFlight {
                        attempt_id: attempt_id.clone(),
                    },
                );
                push(
                    &mut events,
                    RuntimeEventKind::ModelResponseCommitted {
                        attempt_id,
                        output: Box::new(ModelOutput {
                            content: String::new(),
                            reasoning_content: None,
                            tool_calls: vec![ModelToolCall {
                                id: invocation.call_id.clone(),
                                name: invocation.name.clone(),
                                arguments: invocation.arguments.clone(),
                            }],
                            finish_reason: ModelFinishReason::ToolCalls,
                            usage: Usage::default(),
                        }),
                        accounting: Box::new(snapshot.accounting),
                    },
                );
                Some((operation_id.clone(), invocation.clone()))
            } else {
                None
            };
            push(&mut events, kind);
            if let Some((operation_id, invocation)) = prepared
                && invocation.name != AGENT_TOOL_NAME
                && invocation.name != REQUEST_USER_INPUT_TOOL_NAME
            {
                let snapshot =
                    reduce_events(&events).expect("prepared authorization fixture prefix");
                let resolved_execution =
                    resolve_tool_execution(snapshot.request.task_contract.as_ref(), &invocation);
                let execution_invocation = resolved_execution
                    .invocation
                    .expect("fixture verifier resolution");
                push(
                    &mut events,
                    RuntimeEventKind::ToolAuthorizationCommitted {
                        operation_id,
                        decision: ToolAuthorizationDecision {
                            mode: snapshot.request.environment.permission_mode,
                            execution_grant: resolved_execution.grant,
                            tool_name: execution_invocation.name.clone(),
                            arguments_sha256: execution_invocation.arguments_sha256(),
                            workspace_state: snapshot.workspace_state,
                            disposition: ToolAuthorizationDisposition::Allow,
                            risk: ApprovalRisk::Routine,
                            matched_rule: Some("runtime_store_fixture".to_owned()),
                            reason: "deterministic store fixture authorization".to_owned(),
                            prompt: None,
                        },
                    },
                );
            }
        }
        events
    }

    fn root_terminal(terminal: TerminalState) -> RuntimeEventKind {
        RuntimeEventKind::Terminal {
            outcome: Box::new(AgentOutcome {
                run_id: RunId::from("root"),
                parent_run_id: None,
                terminal,
                accounting: ModelAccounting::default(),
                runtime_model_requests: 0,
                runtime_retries: 0,
                tool_calls: 0,
                details: AgentResultDetails::default(),
            }),
        }
    }

    #[test]
    fn unowned_workspace_change_breaks_failure_to_write_lineage() {
        let mut kinds = temporal_failure_kinds();
        kinds.push(RuntimeEventKind::WorkspaceObserved {
            workspace_state: known(3, "revision-b"),
        });
        push_effective_write(&mut kinds, "write-b-to-c", known(4, "revision-c"));

        let snapshot = reduce_events(&stored_events(kinds)).unwrap();
        assert!(snapshot.temporal_evidence_progress.is_none());
        assert!(
            expected_evidence_lineage(
                &snapshot,
                &AcceptanceId::from("temporal"),
                &verifier(),
                VerifierEvidencePolicy::FailedWritePass,
            )
            .is_none()
        );
    }

    #[test]
    fn unstarted_write_cannot_claim_an_external_revision_change() {
        let mut kinds = temporal_failure_kinds();
        let operation_id = OperationId::from("rejected-write");
        kinds.push(RuntimeEventKind::ToolPrepared {
            operation_id: operation_id.clone(),
            invocation: ToolInvocation {
                run_id: RunId::from("root"),
                call_id: "rejected-write".to_owned(),
                name: "write".to_owned(),
                arguments: ToolArguments::from_value(json!({})),
            },
            workspace_access: WorkspaceAccess::MayWrite,
        });
        kinds.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id,
            call_id: "rejected-write".to_owned(),
            name: "write".to_owned(),
            outcome: Box::new(ToolOutcome::rejected(
                "approval denied",
                ToolRetryDisposition::AfterCorrection,
            )),
            workspace_state: Some(known(3, "revision-external")),
        });

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("unstarted workspace-mutating tool")
        ));
    }

    #[test]
    fn forged_verifier_observation_is_not_a_failure_fact() {
        let mut kinds = temporal_failure_kinds();
        let RuntimeEventKind::ToolPrepared { invocation, .. } = &mut kinds[2] else {
            unreachable!("temporal fixture has a prepared verifier")
        };
        invocation.name = "not-the-verifier".to_owned();
        let RuntimeEventKind::ToolOutcomeCommitted { name, .. } = &mut kinds[4] else {
            unreachable!("temporal fixture has a verifier outcome")
        };
        *name = "not-the-verifier".to_owned();

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("exact deterministic execution")
        ));
    }

    #[test]
    fn task_contract_verifier_authorization_rejects_a_forged_grant() {
        let mut events = stored_events(temporal_failure_kinds());
        let decision = events
            .iter_mut()
            .find_map(|event| match &mut event.event {
                RuntimeEventKind::ToolAuthorizationCommitted { decision, .. } => Some(decision),
                _ => None,
            })
            .expect("temporal fixture authorization");
        decision.execution_grant = ToolExecutionGrant::TaskContractVerifier {
            acceptance_id: AcceptanceId::from("temporal"),
            verifier_sha256: format!("sha256:{}", "0".repeat(64)),
        };

        assert!(matches!(
            reduce_events(&events),
            Err(RunStoreError::Corrupt { message, .. })
                if message.contains("does not bind the exact invocation")
        ));
    }

    #[test]
    fn returning_to_the_failed_revision_cannot_satisfy_temporal_evidence() {
        let mut kinds = temporal_failure_kinds();
        push_effective_write(&mut kinds, "write-a-to-b", known(3, "revision-b"));
        push_effective_write(&mut kinds, "write-b-to-a", known(4, "revision-a"));

        let snapshot = reduce_events(&stored_events(kinds)).unwrap();
        assert!(snapshot.temporal_evidence_progress.is_some());
        assert!(
            expected_evidence_lineage(
                &snapshot,
                &AcceptanceId::from("temporal"),
                &verifier(),
                VerifierEvidencePolicy::FailedWritePass,
            )
            .is_none()
        );
    }

    #[test]
    fn corrupt_inline_verification_artifact_is_rejected_on_replay() {
        let mut kinds = temporal_failure_kinds();
        let RuntimeEventKind::ToolOutcomeCommitted { outcome, .. } = &mut kinds[4] else {
            unreachable!("temporal fixture has a verifier outcome")
        };
        outcome.artifacts[0].byte_len = Some(0);

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("verification artifact metadata")
        ));
    }

    #[test]
    fn self_consistent_artifact_for_another_verdict_cannot_back_an_observation() {
        let mut kinds = temporal_failure_kinds();
        let RuntimeEventKind::ToolOutcomeCommitted { outcome, .. } = &mut kinds[4] else {
            unreachable!("temporal fixture has a verifier outcome")
        };
        let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
            summary: "claims pass".to_owned(),
            verifier: verifier(),
            verdict: VerifierVerdict::Passed,
            workspace_revision: WorkspaceRevision::Known {
                sha256: "revision-a".to_owned(),
            },
        });
        let artifact_id = artifact.id.clone();
        outcome.evidence.references = vec![artifact_id.clone()];
        outcome.artifacts = vec![artifact];
        outcome
            .verifier_observation
            .as_mut()
            .expect("failed verifier observation")
            .artifact_ids = vec![artifact_id];

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("exact deterministic execution")
        ));
    }

    #[test]
    fn unstable_host_verifier_observation_is_a_typed_execution_rejection() {
        for (label, settled_revision, side_effect) in [
            (
                "revision_changed",
                "revision-c",
                ToolSideEffectStatus::Indeterminate,
            ),
            (
                "side_effect_applied",
                "revision-b",
                ToolSideEffectStatus::Applied,
            ),
        ] {
            let mut kinds = temporal_failure_kinds();
            push_effective_write(&mut kinds, "repair", known(3, "revision-b"));
            let prefix = reduce_events(&stored_events(kinds.clone())).unwrap();
            let candidate = CompletionCandidate {
                id: CompletionCandidateId::from(format!("candidate-{label}")),
                generation_id: prefix
                    .request
                    .task_contract
                    .as_ref()
                    .expect("temporal contract")
                    .generation_id
                    .clone(),
                message: "claims completion".to_owned(),
            };
            let verification_id = VerificationId::from(format!("verification-{label}"));
            let workspace_state_after = known(4, settled_revision);
            let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
                summary: "claims deterministic pass".to_owned(),
                verifier: verifier(),
                verdict: VerifierVerdict::Passed,
                workspace_revision: workspace_state_after.revision.clone(),
            });
            let mut outcome = ToolOutcome::success("claims deterministic pass");
            outcome.side_effect = side_effect;
            outcome.workspace_revision = Some(settled_revision.to_owned());
            outcome.evidence = ToolEvidence {
                status: ToolEvidenceStatus::Produced,
                references: vec![artifact.id.clone()],
            };
            outcome.artifacts = vec![artifact];
            outcome.verifier_observation = Some(VerifierObservation {
                spec: verifier(),
                verdict: VerifierVerdict::Passed,
                workspace_revision: workspace_state_after.revision.clone(),
                artifact_ids: outcome.evidence.references.clone(),
            });
            kinds.extend([
                RuntimeEventKind::CompletionProposed {
                    candidate: candidate.clone(),
                },
                RuntimeEventKind::HostVerificationPrepared {
                    verification_id: verification_id.clone(),
                    candidate,
                    acceptance_id: AcceptanceId::from("temporal"),
                    verifier: verifier(),
                    workspace_state_before: prefix.workspace_state,
                },
                RuntimeEventKind::HostVerificationStarted {
                    verification_id: verification_id.clone(),
                },
                RuntimeEventKind::HostVerificationCommitted {
                    verification_id,
                    outcome: Box::new(outcome),
                    receipt: None,
                    workspace_state_after,
                },
            ]);

            let snapshot = reduce_events(&stored_events(kinds)).expect("typed rejection replay");
            let rejection = &snapshot
                .last_host_verification_failure
                .expect("Host failure")
                .rejection;
            assert_eq!(
                rejection.cause,
                EvidenceSealRejection::VerifierWorkspaceUnstable
            );
            assert_eq!(
                rejection.required_transition,
                CompletionRequiredTransition::HostVerifierExecutionRepair
            );
            assert!(snapshot.evidence_receipts.is_empty());
        }
    }

    #[test]
    fn only_an_exact_deterministic_failure_can_request_workspace_mutation() {
        let snapshot = reduce_events(&stored_events(temporal_failure_kinds()))
            .expect("temporal verifier snapshot");
        let workspace_after = known(3, "revision-a");
        let exact_failure = failed_verifier_outcome("revision-a");
        assert_eq!(
            seal_evidence_receipt(
                &snapshot,
                &VerificationId::from("exact-failure"),
                &AcceptanceId::from("temporal"),
                VerifierEvidencePolicy::FailedWritePass,
                &verifier(),
                &exact_failure,
                &workspace_after,
            ),
            Err(EvidenceSealRejection::VerifierFailed)
        );

        let mut rejected =
            ToolOutcome::rejected("invocation rejected", ToolRetryDisposition::AfterCorrection);
        attach_failed_verifier_evidence(&mut rejected, &exact_failure);
        let mut transport_failed = ToolOutcome::transport_failure("transport failed");
        attach_failed_verifier_evidence(&mut transport_failed, &exact_failure);
        let mut ambiguous = ToolOutcome::recovery_ambiguous("operation ambiguous");
        attach_failed_verifier_evidence(&mut ambiguous, &exact_failure);

        for (label, outcome) in [
            ("rejected", rejected),
            ("transport_failed", transport_failed),
            ("ambiguous", ambiguous),
        ] {
            outcome
                .validate()
                .unwrap_or_else(|error| panic!("{label} fixture is invalid: {error}"));
            assert_eq!(
                seal_evidence_receipt(
                    &snapshot,
                    &VerificationId::from(format!("not-deterministic-{label}")),
                    &AcceptanceId::from("temporal"),
                    VerifierEvidencePolicy::FailedWritePass,
                    &verifier(),
                    &outcome,
                    &workspace_after,
                ),
                Err(EvidenceSealRejection::VerifierOutcomeInconsistent),
                "{label} must not tell the model to modify code"
            );
        }
    }

    #[test]
    fn host_verification_commit_rejects_workspace_drift_after_prepare() {
        let mut kinds = temporal_failure_kinds();
        push_effective_write(&mut kinds, "repair", known(3, "revision-b"));
        let prefix = reduce_events(&stored_events(kinds.clone())).expect("write prefix");
        let candidate = CompletionCandidate {
            id: CompletionCandidateId::from("candidate-workspace-drift"),
            generation_id: prefix
                .request
                .task_contract
                .as_ref()
                .expect("temporal contract")
                .generation_id
                .clone(),
            message: "claims completion".to_owned(),
        };
        let verification_id = VerificationId::from("verification-workspace-drift");
        kinds.extend([
            RuntimeEventKind::CompletionProposed {
                candidate: candidate.clone(),
            },
            RuntimeEventKind::HostVerificationPrepared {
                verification_id: verification_id.clone(),
                candidate,
                acceptance_id: AcceptanceId::from("temporal"),
                verifier: verifier(),
                workspace_state_before: prefix.workspace_state,
            },
            RuntimeEventKind::HostVerificationStarted {
                verification_id: verification_id.clone(),
            },
            RuntimeEventKind::WorkspaceObserved {
                workspace_state: known(4, "revision-c"),
            },
            RuntimeEventKind::HostVerificationCommitted {
                verification_id,
                outcome: Box::new(failed_verifier_outcome("revision-c")),
                receipt: None,
                workspace_state_after: known(4, "revision-c"),
            },
        ]);

        assert!(matches!(
            reduce_events(&stored_events(kinds)),
            Err(RunStoreError::Corrupt { message, .. })
                if message.contains("workspace changed after its exact action was prepared")
        ));
    }

    #[test]
    fn writer_lifecycle_replays_full_recovery_facts_without_promoting_child_evidence() {
        let snapshot = reduce_events(&stored_events(event_kinds())).unwrap();
        let lifecycle = snapshot.agent_tasks.first().unwrap();
        assert!(lifecycle.finished.is_some());
        assert_eq!(
            lifecycle
                .integration
                .as_ref()
                .unwrap()
                .committed
                .as_ref()
                .unwrap()
                .root_workspace_state_after,
            known(2, "root-integrated")
        );
        assert!(snapshot.pending_child_run_ids().is_empty());
        assert!(
            snapshot.evidence_receipts.is_empty(),
            "a child receipt must never become root completion evidence"
        );
        assert_eq!(
            snapshot.workspace_state,
            known(2, "root-integrated"),
            "the agent ToolOutcome settles the integrated root revision exactly once"
        );
    }

    #[test]
    fn writer_root_generation_advances_only_when_the_agent_tool_outcome_commits() {
        let mut before_tool_outcome = event_kinds();
        before_tool_outcome.truncate(14);
        let snapshot = reduce_events(&stored_events(before_tool_outcome)).unwrap();
        assert_eq!(snapshot.workspace_state, known(1, "root-base"));
        assert!(snapshot.pending_tool.is_some());
        assert!(snapshot.pending_child_run_ids().is_empty());
        assert!(snapshot.agent_tasks[0].cleanup.is_none());

        let snapshot = reduce_events(&stored_events(event_kinds())).unwrap();
        assert_eq!(snapshot.workspace_state, known(2, "root-integrated"));
        assert!(snapshot.pending_tool.is_none());
    }

    #[test]
    fn successful_writer_cleanup_is_deferred_until_after_handoff_and_root_settlement() {
        let mut early_cleanup = event_kinds();
        let cleanup = early_cleanup.remove(15);
        early_cleanup.insert(13, cleanup);
        let error = reduce_events(&stored_events(early_cleanup)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("cleanup preparation does not match")
        ));

        let snapshot = reduce_events(&stored_events(event_kinds())).unwrap();
        let lifecycle = &snapshot.agent_tasks[0];
        assert!(lifecycle.finished.is_some());
        assert!(lifecycle.cleanup.as_ref().unwrap().committed.is_some());
    }

    #[test]
    fn root_terminal_rejects_an_integrated_writer_without_committed_cleanup() {
        let mut missing_cleanup = event_kinds();
        missing_cleanup.truncate(15);
        missing_cleanup.push(root_terminal(TerminalState::Failed {
            failure: RuntimeFailure::Join {
                message: "root stopped".to_owned(),
            },
        }));
        let error = reduce_events(&stored_events(missing_cleanup)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("integrated writer cleanup")
        ));

        let mut settled = event_kinds();
        settled.push(root_terminal(TerminalState::Failed {
            failure: RuntimeFailure::Join {
                message: "root stopped".to_owned(),
            },
        }));
        let snapshot = reduce_events(&stored_events(settled)).unwrap();
        assert!(matches!(
            snapshot.terminal.as_ref().map(|outcome| &outcome.terminal),
            Some(TerminalState::Failed { .. })
        ));
    }

    #[tokio::test]
    async fn in_memory_append_is_incremental_atomic_and_matches_full_replay() {
        let store = InMemoryRunStore::default();
        let created = store.create(root_request()).await.unwrap();
        store
            .append(
                &created.lease,
                PendingRuntimeEvent::new(RuntimeEventKind::WorkspaceObserved {
                    workspace_state: known(1, "root-base"),
                }),
            )
            .await
            .unwrap();

        let error = store
            .append(
                &created.lease,
                PendingRuntimeEvent::new(RuntimeEventKind::AgentCleanupPrepared {
                    task_id: AgentTaskId::from("missing-task"),
                    plan: Box::new(test_cleanup_plan(WriterCleanupPhase::PostIntegration)),
                }),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("no prepared task")
        ));

        {
            let runs = store.runs.lock().await;
            let run = runs.get(&RunId::from("root")).unwrap();
            assert_eq!(run.events.len(), 2);
            assert_eq!(run.snapshot.last_sequence, 2);
            assert_eq!(run.snapshot.workspace_state, known(1, "root-base"));
        }

        store
            .append(
                &created.lease,
                PendingRuntimeEvent::new(RuntimeEventKind::WorkspaceObserved {
                    workspace_state: known(2, "root-after-rejected-event"),
                }),
            )
            .await
            .unwrap();
        let replay = store.load(&RunId::from("root")).await.unwrap().unwrap();
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.snapshot.last_sequence, 3);
        assert_eq!(
            replay.snapshot.workspace_state,
            known(2, "root-after-rejected-event")
        );
        assert_eq!(replay.snapshot, reduce_events(&replay.events).unwrap());
    }

    #[test]
    fn read_only_child_uses_the_minimal_lifecycle_without_writer_facts() {
        let task = read_only_task();
        let outcome = read_only_outcome();
        let kinds = vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: task.call_id.clone(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::ReadOnly,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("agent-operation".to_owned()),
            },
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(task.clone()),
            },
            RuntimeEventKind::ChildStarted {
                task_id: task.task_id.clone(),
                call_id: task.call_id.clone(),
                child_run_id: task.child_run_id,
                depth: 1,
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId("agent-operation".to_owned()),
                call_id: task.call_id.clone(),
                name: AGENT_TOOL_NAME.to_owned(),
                outcome: Box::new(ToolOutcome::success("read-only child launched")),
                workspace_state: None,
            },
            RuntimeEventKind::AgentResultCollected {
                task_id: task.task_id,
                outcome: Box::new(outcome.clone()),
            },
            RuntimeEventKind::ChildFinished {
                call_id: task.call_id,
                outcome: Box::new(outcome),
                accounting: Box::new(ModelAccounting::default()),
                handoff_content: "read-only child settled".to_owned(),
            },
        ];
        let snapshot = reduce_events(&stored_events(kinds)).unwrap();
        let lifecycle = snapshot.agent_tasks.first().unwrap();
        assert!(lifecycle.workspace_created.is_none());
        assert!(lifecycle.seal.is_none());
        assert!(lifecycle.integration.is_none());
        assert!(lifecycle.cleanup.is_none());
        assert!(lifecycle.finished.is_some());
    }

    fn unfinished_read_only_child_kinds(launch_outcome_committed: bool) -> Vec<RuntimeEventKind> {
        let task = read_only_task();
        let mut kinds = vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: task.call_id.clone(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::ReadOnly,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("agent-operation".to_owned()),
            },
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(task.clone()),
            },
            RuntimeEventKind::ChildStarted {
                task_id: task.task_id.clone(),
                call_id: task.call_id.clone(),
                child_run_id: task.child_run_id,
                depth: 1,
            },
        ];
        if launch_outcome_committed {
            kinds.push(RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId("agent-operation".to_owned()),
                call_id: task.call_id,
                name: AGENT_TOOL_NAME.to_owned(),
                outcome: Box::new(
                    ToolOutcome::success("read-only child launched")
                        .with_side_effect(ToolSideEffectStatus::Applied),
                ),
                workspace_state: None,
            });
        }
        kinds
    }

    #[test]
    fn exact_recovery_terminal_may_preserve_unsettled_read_only_child_truth() {
        let cases = [
            (
                false,
                RecoveryAmbiguityPhase::ToolExecution,
                "agent-operation",
            ),
            (true, RecoveryAmbiguityPhase::ChildRun, "child"),
        ];
        for (launch_outcome_committed, phase, action_id) in cases {
            let mut kinds = unfinished_read_only_child_kinds(launch_outcome_committed);
            kinds.push(root_terminal(TerminalState::RecoveryRequired {
                ambiguity: RecoveryAmbiguity {
                    phase,
                    action_id: action_id.to_owned(),
                    message: "do not relaunch an ambiguous read-only child".to_owned(),
                },
            }));
            let snapshot = reduce_events(&stored_events(kinds)).unwrap();
            assert!(matches!(
                snapshot.terminal.as_ref().map(|outcome| &outcome.terminal),
                Some(TerminalState::RecoveryRequired { ambiguity })
                    if ambiguity.action_id == action_id
            ));
            assert_eq!(snapshot.agent_tasks.len(), 1);
            assert!(snapshot.agent_tasks[0].finished.is_none());
        }
    }

    #[test]
    fn non_matching_terminal_cannot_hide_an_unsettled_read_only_child() {
        let mut kinds = unfinished_read_only_child_kinds(true);
        kinds.push(root_terminal(TerminalState::Failed {
            failure: RuntimeFailure::Join {
                message: "generic failure".to_owned(),
            },
        }));
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("unsettled AgentTask")
        ));
    }

    #[test]
    fn writer_start_failure_requires_binding_cleanup_before_finish() {
        let task = writer_task();
        let mut outcome = AgentOutcome {
            run_id: task.child_run_id.clone(),
            parent_run_id: Some(task.parent_run_id.clone()),
            terminal: TerminalState::Failed {
                failure: RuntimeFailure::Join {
                    message: "child process did not start".to_owned(),
                },
            },
            accounting: ModelAccounting::default(),
            runtime_model_requests: 0,
            runtime_retries: 0,
            tool_calls: 0,
            details: AgentResultDetails::default(),
        };
        outcome.details.summary = "writer_binding_failed".to_owned();
        let prefix = vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: task.call_id.clone(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::MayWrite,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("agent-operation".to_owned()),
            },
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(task.clone()),
            },
            RuntimeEventKind::AgentResultCollected {
                task_id: task.task_id.clone(),
                outcome: Box::new(outcome.clone()),
            },
        ];
        let mut unfinished = prefix.clone();
        unfinished.push(RuntimeEventKind::ChildFinished {
            call_id: task.call_id.clone(),
            outcome: Box::new(outcome.clone()),
            accounting: Box::new(ModelAccounting::default()),
            handoff_content: "writer did not start".to_owned(),
        });
        let error = reduce_events(&stored_events(unfinished)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("before owned workspace cleanup settled")
        ));

        let mut kinds = prefix;
        kinds.extend([
            RuntimeEventKind::AgentCleanupPrepared {
                task_id: task.task_id.clone(),
                plan: Box::new(WriterCleanupPlan {
                    phase: WriterCleanupPhase::Binding,
                    reason_code: "writer_binding_failed".to_owned(),
                    ownership: WriterCleanupOwnership::Known {
                        identity_sha256: "c".repeat(64),
                    },
                    artifact_state: WriterArtifactState::KnownUnsealed,
                    scope: WriterCleanupScope::Known {
                        workspace_revision: WorkspaceRevision::Known {
                            sha256: "d".repeat(64),
                        },
                        changed_count: 0,
                        in_scope_count: 0,
                        out_of_scope_count: 0,
                        path_set_sha256: writer_path_set_sha256(&[]).unwrap(),
                    },
                    mode: WriterCleanupMode::RemoveExact {
                        expected_branch_commit: task.workspace.base_commit.clone(),
                    },
                }),
            },
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task.task_id.clone(),
                result: WriterCleanupResult::AlreadyAbsent,
            },
            RuntimeEventKind::ChildFinished {
                call_id: task.call_id,
                outcome: Box::new(outcome),
                accounting: Box::new(ModelAccounting::default()),
                handoff_content: "writer did not start".to_owned(),
            },
        ]);
        let snapshot = reduce_events(&stored_events(kinds)).unwrap();
        let lifecycle = snapshot.agent_tasks.first().unwrap();
        assert!(lifecycle.workspace_created.is_none());
        assert!(lifecycle.child_started.is_none());
        assert!(lifecycle.finished.is_some());
        assert!(snapshot.pending_child_run_ids().is_empty());
    }

    #[test]
    fn writer_result_before_seal_is_rejected() {
        let mut kinds = event_kinds();
        let result = kinds.remove(9);
        kinds.insert(7, result);
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("before its seal committed")
        ));
    }

    #[test]
    fn a_second_event_id_cannot_repeat_an_agent_lifecycle_stage() {
        let mut kinds = event_kinds();
        kinds.insert(
            5,
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(writer_task()),
            },
        );
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("reused a task")
        ));
    }

    #[test]
    fn a_root_can_never_freeze_a_second_writer_after_the_first_finished() {
        let mut kinds = event_kinds();
        let mut second = writer_task();
        second.task_id = AgentTaskId::from("task-2");
        second.child_run_id = RunId::from("child-2");
        second.call_id = "call-2".to_owned();
        second.task_contract.generation_id = TaskGenerationId::from("child-2");
        second.workspace.base_commit = FINAL.to_owned();
        second.workspace.worktree_path = Some("/tmp/dse-writer-2".to_owned());
        second.workspace.branch = Some("dse/writer/task-2".to_owned());
        second.workspace.owner_token = Some("owner-2".to_owned());
        kinds.extend([
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation-2".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: second.call_id.clone(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::MayWrite,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("agent-operation-2".to_owned()),
            },
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(second),
            },
        ]);

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("only one isolated writer task per root run")
        ));
    }

    #[test]
    fn agent_preflight_rejection_never_invents_a_child_lifecycle() {
        let prefix = vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("agent-operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: "call-1".to_owned(),
                    name: AGENT_TOOL_NAME.to_owned(),
                    arguments: ToolArguments::from_value(json!({})),
                },
                workspace_access: WorkspaceAccess::ReadOnly,
            },
        ];
        let mut rejected = prefix.clone();
        rejected.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: OperationId("agent-operation".to_owned()),
            call_id: "call-1".to_owned(),
            name: AGENT_TOOL_NAME.to_owned(),
            outcome: Box::new(ToolOutcome::rejected(
                "capacity unavailable",
                ToolRetryDisposition::NotRetryable,
            )),
            workspace_state: None,
        });
        let snapshot = reduce_events(&stored_events(rejected)).unwrap();
        assert!(snapshot.agent_tasks.is_empty());

        let mut fake_success = prefix;
        fake_success.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: OperationId("agent-operation".to_owned()),
            call_id: "call-1".to_owned(),
            name: AGENT_TOOL_NAME.to_owned(),
            outcome: Box::new(ToolOutcome::success("launched without lifecycle")),
            workspace_state: None,
        });
        let error = reduce_events(&stored_events(fake_success)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("unstarted tool can only commit a preflight rejection")
        ));
    }

    #[test]
    fn tool_history_rejects_orphan_duplicate_and_changed_invocations() {
        let mut orphan_request = root_request();
        orphan_request
            .transcript
            .entries
            .push(TranscriptEntry::Tool {
                call_id: "orphan".to_owned(),
                name: "read".to_owned(),
                outcome: Box::new(ToolOutcome::success("orphan")),
            });
        let error = reduce_events(&stored_events(vec![RuntimeEventKind::RunCreated {
            request: Box::new(orphan_request),
        }]))
        .unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("orphan or duplicate call id")
        ));

        let invocation = ToolInvocation {
            run_id: RunId::from("root"),
            call_id: "call-1".to_owned(),
            name: "read".to_owned(),
            arguments: ToolArguments::from_value(json!({"path": "src/lib.rs"})),
        };
        let mut changed = stored_events(vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("read-operation".to_owned()),
                invocation: invocation.clone(),
                workspace_access: WorkspaceAccess::ReadOnly,
            },
        ]);
        let prepared = changed
            .iter_mut()
            .find(|event| matches!(event.event, RuntimeEventKind::ToolPrepared { .. }))
            .expect("fixture ToolPrepared");
        let RuntimeEventKind::ToolPrepared {
            invocation: changed_invocation,
            ..
        } = &mut prepared.event
        else {
            unreachable!();
        };
        changed_invocation.arguments = ToolArguments::from_value(json!({"path": "src/other.rs"}));
        let error = reduce_events(&changed).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("changed the latest assistant call")
        ));

        let mut duplicate = stored_events(vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("read-operation".to_owned()),
                invocation: invocation.clone(),
                workspace_access: WorkspaceAccess::ReadOnly,
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId("read-operation".to_owned()),
                call_id: invocation.call_id.clone(),
                name: invocation.name.clone(),
                outcome: Box::new(ToolOutcome::rejected(
                    "fixture rejection",
                    ToolRetryDisposition::AfterCorrection,
                )),
                workspace_state: None,
            },
        ]);
        let index = duplicate.len();
        duplicate.push(StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: RunId::from("root"),
            parent_run_id: None,
            event_id: RuntimeEventId("duplicate-tool-prepared".to_owned()),
            sequence: u64::try_from(index).unwrap() + 1,
            occurred_at_unix_ms: 1,
            event: RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("duplicate-read-operation".to_owned()),
                invocation,
                workspace_access: WorkspaceAccess::ReadOnly,
            },
        });
        let error = reduce_events(&duplicate).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("repeats an already settled call")
        ));
    }

    #[test]
    fn in_flight_tool_cannot_claim_a_preflight_rejection() {
        let kinds = vec![
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request()),
            },
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId("operation".to_owned()),
                invocation: ToolInvocation {
                    run_id: RunId::from("root"),
                    call_id: "call-1".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: ToolArguments::from_value(json!({"path":"missing"})),
                },
                workspace_access: WorkspaceAccess::ReadOnly,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId("operation".to_owned()),
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId("operation".to_owned()),
                call_id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                outcome: Box::new(ToolOutcome::rejected(
                    "preflight failure after execution started",
                    ToolRetryDisposition::AfterCorrection,
                )),
                workspace_state: None,
            },
        ];
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("in-flight tool cannot commit a preflight rejection")
        ));
    }

    #[test]
    fn read_only_executable_outcome_cannot_claim_workspace_side_effect_ambiguity() {
        for side_effect in [
            ToolSideEffectStatus::Applied,
            ToolSideEffectStatus::Indeterminate,
        ] {
            let mut outcome = ToolOutcome::error("只读工具失败");
            outcome.side_effect = side_effect;
            let kinds = vec![
                RuntimeEventKind::RunCreated {
                    request: Box::new(root_request()),
                },
                RuntimeEventKind::ToolPrepared {
                    operation_id: OperationId("read-operation".to_owned()),
                    invocation: ToolInvocation {
                        run_id: RunId::from("root"),
                        call_id: "read-call".to_owned(),
                        name: "read_file".to_owned(),
                        arguments: ToolArguments::from_value(json!({"path":"src/lib.rs"})),
                    },
                    workspace_access: WorkspaceAccess::ReadOnly,
                },
                RuntimeEventKind::ToolExecutionStarted {
                    operation_id: OperationId("read-operation".to_owned()),
                },
                RuntimeEventKind::ToolOutcomeCommitted {
                    operation_id: OperationId("read-operation".to_owned()),
                    call_id: "read-call".to_owned(),
                    name: "read_file".to_owned(),
                    outcome: Box::new(outcome),
                    workspace_state: None,
                },
            ];

            let error = reduce_events(&stored_events(kinds)).unwrap_err();
            assert!(matches!(
                error,
                RunStoreError::Corrupt { message, .. }
                    if message.contains("read-only executable tool outcome")
            ));
        }
    }

    #[test]
    fn integration_rejects_a_stale_expected_root_generation() {
        let mut kinds = event_kinds();
        let RuntimeEventKind::AgentIntegrationPrepared {
            expected_root_workspace_state,
            ..
        } = &mut kinds[10]
        else {
            panic!("fixture integration preparation");
        };
        *expected_root_workspace_state = known(0, "stale-root");
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("current root workspace")
        ));
    }

    #[test]
    fn integration_commit_must_advance_exactly_one_generation() {
        let mut kinds = event_kinds();
        let RuntimeEventKind::AgentIntegrationCommitted {
            root_workspace_state_after,
            ..
        } = &mut kinds[12]
        else {
            panic!("fixture integration commit");
        };
        *root_workspace_state_after = known(3, "root-integrated");
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("advance exactly one generation")
        ));
    }

    #[test]
    fn retained_writer_cleanup_requires_a_recovery_required_root_terminal() {
        let mut kinds = event_kinds();
        let RuntimeEventKind::AgentCleanupCommitted { result, .. } = &mut kinds[16] else {
            panic!("fixture cleanup commit");
        };
        *result = WriterCleanupResult::Retained {
            worktree: WriterResourceState::Retained,
            branch: WriterResourceState::Retained,
            metadata: WriterCleanupMetadataState::Uncertain,
            uncertainty_code: "writer_cleanup_ownership_unknown".to_owned(),
        };

        let snapshot = reduce_events(&stored_events(kinds.clone())).unwrap();
        assert!(matches!(
            snapshot.agent_tasks[0]
                .finished
                .as_ref()
                .map(|finished| &finished.outcome.terminal),
            Some(TerminalState::Completed { .. })
        ));

        let mut normal_terminal = kinds.clone();
        normal_terminal.push(root_terminal(TerminalState::Failed {
            failure: RuntimeFailure::Join {
                message: "root stopped".to_owned(),
            },
        }));
        let error = reduce_events(&stored_events(normal_terminal)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("requires its exact recovery-required root terminal")
        ));

        kinds.push(root_terminal(TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ChildRun,
                action_id: "agent-cleanup:task-1".to_owned(),
                message: "writer_cleanup_ownership_unknown".to_owned(),
            },
        }));
        let snapshot = reduce_events(&stored_events(kinds)).unwrap();
        assert!(matches!(
            snapshot.terminal.as_ref().map(|outcome| &outcome.terminal),
            Some(TerminalState::RecoveryRequired { .. })
        ));
    }

    #[test]
    fn retain_for_recovery_plan_cannot_claim_cleanup_metadata_is_absent() {
        let uncertainty_code = "writer_cleanup_identity_unprovable".to_owned();
        let mut kinds = event_kinds();
        let RuntimeEventKind::AgentCleanupPrepared { plan, .. } = &mut kinds[15] else {
            panic!("fixture cleanup plan");
        };
        plan.ownership = WriterCleanupOwnership::Unknown {
            uncertainty_code: uncertainty_code.clone(),
        };
        plan.artifact_state = WriterArtifactState::Unknown;
        plan.scope = WriterCleanupScope::Unknown {
            uncertainty_code: uncertainty_code.clone(),
        };
        plan.mode = WriterCleanupMode::RetainForRecovery { uncertainty_code };
        let RuntimeEventKind::AgentCleanupCommitted { result, .. } = &mut kinds[16] else {
            panic!("fixture cleanup result");
        };
        *result = WriterCleanupResult::AlreadyAbsent;

        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("requires its exact uncertain result")
        ));
    }

    #[test]
    fn integration_conflict_is_a_single_durable_blocked_outcome() {
        let reason = "root branch moved".to_owned();
        let snapshot = reduce_events(&stored_events(failed_integration_kinds(
            WriterIntegrationStatus::Conflict {
                reason: reason.clone(),
            },
        )))
        .unwrap();
        let integration = snapshot.agent_tasks[0].integration.as_ref().unwrap();
        assert!(integration.committed.is_none());
        assert_eq!(
            integration.failure.as_ref().unwrap().status,
            WriterIntegrationStatus::Conflict {
                reason: reason.clone(),
            }
        );
        assert!(matches!(
            snapshot.agent_tasks[0]
                .finished
                .as_ref()
                .map(|finished| &finished.outcome.terminal),
            Some(TerminalState::Blocked { reason: blocked }) if blocked == &reason
        ));
        assert_eq!(snapshot.workspace_state, known(1, "root-base"));
    }

    #[test]
    fn integration_failure_and_commit_are_mutually_exclusive() {
        let mut kinds = failed_integration_kinds(WriterIntegrationStatus::Rejected {
            reason: "policy rejected".to_owned(),
        });
        kinds.insert(
            13,
            RuntimeEventKind::AgentIntegrationCommitted {
                task_id: AgentTaskId::from("task-1"),
                integration_id: OperationId("integrate-1".to_owned()),
                root_head_commit: FINAL.to_owned(),
                root_workspace_state_after: known(2, "root-integrated"),
            },
        );
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("does not match the in-flight action")
        ));
    }

    #[test]
    fn failed_integration_cannot_advance_the_root_tool_epoch() {
        let mut kinds = failed_integration_kinds(WriterIntegrationStatus::Conflict {
            reason: "root changed".to_owned(),
        });
        kinds.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: OperationId("agent-operation".to_owned()),
            call_id: "call-1".to_owned(),
            name: AGENT_TOOL_NAME.to_owned(),
            outcome: Box::new(
                ToolOutcome::success("must not advance")
                    .with_side_effect(ToolSideEffectStatus::Applied),
            ),
            workspace_state: Some(known(2, "root-integrated")),
        });
        let error = reduce_events(&stored_events(kinds)).unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("cannot advance the root workspace")
        ));

        let mut settled = failed_integration_kinds(WriterIntegrationStatus::Conflict {
            reason: "root changed".to_owned(),
        });
        settled.push(RuntimeEventKind::ToolOutcomeCommitted {
            operation_id: OperationId("agent-operation".to_owned()),
            call_id: "call-1".to_owned(),
            name: AGENT_TOOL_NAME.to_owned(),
            outcome: Box::new(
                ToolOutcome::error("integration conflict")
                    .with_side_effect(ToolSideEffectStatus::NotApplied),
            ),
            workspace_state: None,
        });
        let snapshot = reduce_events(&stored_events(settled)).unwrap();
        assert_eq!(snapshot.workspace_state, known(1, "root-base"));
        assert!(snapshot.pending_tool.is_none());
    }

    #[test]
    fn integration_recovery_and_cleanup_ambiguity_use_typed_child_recovery() {
        let integration_reason = "ff-only result is ambiguous".to_owned();
        let snapshot = reduce_events(&stored_events(failed_integration_kinds(
            WriterIntegrationStatus::RecoveryRequired {
                reason: integration_reason.clone(),
            },
        )))
        .unwrap();
        assert!(matches!(
            snapshot.agent_tasks[0]
                .finished
                .as_ref()
                .map(|finished| &finished.outcome.terminal),
            Some(TerminalState::RecoveryRequired { ambiguity })
                if ambiguity.action_id == "agent-integration:integrate-1"
                    && ambiguity.message == integration_reason
        ));

        let mut cleanup_ambiguous = failed_integration_kinds(WriterIntegrationStatus::Conflict {
            reason: "root changed".to_owned(),
        });
        let RuntimeEventKind::AgentCleanupCommitted { result, .. } = &mut cleanup_ambiguous[14]
        else {
            panic!("fixture cleanup commit");
        };
        *result = WriterCleanupResult::Retained {
            worktree: WriterResourceState::Retained,
            branch: WriterResourceState::Retained,
            metadata: WriterCleanupMetadataState::Clear,
            uncertainty_code: "writer_cleanup_ownership_unknown".to_owned(),
        };
        let RuntimeEventKind::ChildFinished { outcome, .. } = &mut cleanup_ambiguous[15] else {
            panic!("fixture child finish");
        };
        outcome.terminal = TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ChildRun,
                action_id: "agent-cleanup:task-1".to_owned(),
                message: "writer_cleanup_ownership_unknown".to_owned(),
            },
        };
        let snapshot = reduce_events(&stored_events(cleanup_ambiguous)).unwrap();
        assert!(matches!(
            snapshot.agent_tasks[0]
                .finished
                .as_ref()
                .map(|finished| &finished.outcome.terminal),
            Some(TerminalState::RecoveryRequired { ambiguity })
                if ambiguity.action_id == "agent-cleanup:task-1"
        ));
    }

    #[test]
    fn child_run_created_requires_an_exact_agent_task_binding() {
        let mut request = root_request();
        request.run_id = Some(RunId::from("child"));
        request.parent_run_id = Some(RunId::from("root"));
        request.actor = AgentActor {
            kind: AgentActorKind::Child,
            depth: 1,
        };
        request.task_contract = Some(writer_task().task_contract);
        let error = reduce_events(&[StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: RunId::from("child"),
            parent_run_id: Some(RunId::from("root")),
            event_id: RuntimeEventId::run_created(),
            sequence: 1,
            occurred_at_unix_ms: 1,
            event: RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        }])
        .unwrap_err();
        assert!(matches!(
            error,
            RunStoreError::Corrupt { message, .. }
                if message.contains("requires an AgentTask")
        ));
    }
}
