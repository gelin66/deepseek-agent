use std::collections::HashMap;

use async_trait::async_trait;
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
pub struct PendingToolAction {
    pub operation_id: OperationId,
    pub invocation: ToolInvocation,
    pub state: DurableActionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub request: RunRequest,
    pub transcript: CanonicalTranscript,
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
    pub pending_model: Option<PendingModelAction>,
    pub pending_tool: Option<PendingToolAction>,
    pub pending_children: Vec<RunId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReplay {
    pub snapshot: RunSnapshot,
    pub events: Vec<StoredRuntimeEvent>,
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
    let accounting = request.accounting_baseline.clone();

    let mut snapshot = RunSnapshot {
        transcript: initial_transcript(&request),
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
        pending_model: None,
        pending_tool: None,
        pending_children: Vec::new(),
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
        RuntimeEventKind::ModelRequestPrepared {
            attempt_id,
            request,
        } => {
            if snapshot.pending_model.is_some() {
                return Err(corrupt(
                    &run_id,
                    "model request prepared while another is pending",
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
            if snapshot.pending_tool.is_some() {
                return Err(corrupt(
                    &run_id,
                    "tool prepared while another tool is pending",
                ));
            }
            snapshot.tool_calls = snapshot.tool_calls.saturating_add(1);
            snapshot.pending_tool = Some(PendingToolAction {
                operation_id: operation_id.clone(),
                invocation: invocation.clone(),
                state: DurableActionState::Prepared,
            });
        }
        RuntimeEventKind::ToolExecutionStarted { operation_id } => {
            let pending = pending_tool_mut(snapshot, &run_id, operation_id)?;
            if pending.state != DurableActionState::Prepared {
                return Err(corrupt(&run_id, "tool execution entered in_flight twice"));
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
            snapshot.pending_tool = None;
            snapshot.transcript.entries.push(TranscriptEntry::Tool {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: outcome.clone(),
            });
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
        RuntimeEventKind::Steered { content } => {
            snapshot.transcript.entries.push(TranscriptEntry::User {
                content: content.clone(),
            });
        }
        RuntimeEventKind::Terminal { outcome } => {
            if outcome.run_id != run_id {
                return Err(corrupt(&run_id, "terminal outcome belongs to another run"));
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
    if event.schema_version != AGENT_RUNTIME_EVENT_SCHEMA_VERSION {
        return Err(RunStoreError::UnsupportedSchema {
            found: event.schema_version,
            supported: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
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
        None => reason == ModelRetryStopReason::RequestBudgetExceeded,
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

fn corrupt(run_id: &RunId, message: impl Into<String>) -> RunStoreError {
    RunStoreError::Corrupt {
        run_id: run_id.clone(),
        message: message.into(),
    }
}

#[derive(Debug, Default)]
pub struct InMemoryRunStore {
    runs: Mutex<HashMap<RunId, InMemoryRun>>,
}

#[derive(Debug)]
struct InMemoryRun {
    events: Vec<StoredRuntimeEvent>,
    lease: Option<RunLease>,
    next_epoch: u64,
}

#[async_trait]
impl RunStore for InMemoryRunStore {
    async fn create(&self, mut request: RunRequest) -> Result<CreatedRun, RunStoreError> {
        let run_id = request.run_id.clone().unwrap_or_default();
        request.run_id = Some(run_id.clone());
        let mut runs = self.runs.lock().await;
        if runs.contains_key(&run_id) {
            return Err(RunStoreError::AlreadyExists { run_id });
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
            run_id,
            InMemoryRun {
                events: vec![created.clone()],
                lease: Some(lease.clone()),
                next_epoch: 2,
            },
        );
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

    async fn latest_resumable_run(&self, workspace: &str) -> Result<Option<RunId>, RunStoreError> {
        let runs = self.runs.lock().await;
        let mut candidates = runs
            .iter()
            .filter_map(|(run_id, run)| {
                let snapshot = reduce_events(&run.events).ok()?;
                (snapshot.terminal.is_none()
                    && snapshot.request.parent_run_id.is_none()
                    && snapshot.request.environment.workspace == workspace)
                    .then_some((run.events.last()?.occurred_at_unix_ms, run_id.clone()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(time, _)| *time);
        Ok(candidates.pop().map(|(_, run_id)| run_id))
    }
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
