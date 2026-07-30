//! Machine-stream projection for selected canonical Runtime events.
//!
//! Lifecycle events carry the original stored event. Model failures use a
//! bounded DTO derived entirely from the same stored event so a retry update
//! cannot duplicate the full prompt, transcript, or tool catalog. Neither
//! projection owns retry decisions or persistence.

use dse_protocol::agent_runtime::{
    AttemptId, ModelAccounting, ModelAttemptFailure, ModelRetryDecision, ModelRetryStopReason,
    RunId, RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent,
};
use serde::Serialize;

pub(crate) const EXEC_STREAM_SCHEMA: &str = "dse.exec-stream";
pub(crate) const EXEC_STREAM_SCHEMA_VERSION: u32 = 7;

#[derive(Serialize)]
struct CanonicalRuntimeStreamEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    runtime_event: &'a StoredRuntimeEvent,
    schema_version: u32,
    schema: &'static str,
}

pub(crate) fn is_agent_lifecycle_event(event: &RuntimeEventKind) -> bool {
    matches!(
        event,
        RuntimeEventKind::AgentTaskPrepared { .. }
            | RuntimeEventKind::AgentWorkspaceCreated { .. }
            | RuntimeEventKind::AgentSealPrepared { .. }
            | RuntimeEventKind::AgentSealCommitted { .. }
            | RuntimeEventKind::ChildStarted { .. }
            | RuntimeEventKind::AgentResultCollected { .. }
            | RuntimeEventKind::AgentIntegrationPrepared { .. }
            | RuntimeEventKind::AgentIntegrationStarted { .. }
            | RuntimeEventKind::AgentIntegrationFailed { .. }
            | RuntimeEventKind::AgentIntegrationCommitted { .. }
            | RuntimeEventKind::AgentCleanupPrepared { .. }
            | RuntimeEventKind::AgentCleanupCommitted { .. }
            | RuntimeEventKind::ChildFinished { .. }
    )
}

pub(crate) fn agent_lifecycle_stream_line(
    event: &StoredRuntimeEvent,
) -> Result<Vec<u8>, serde_json::Error> {
    canonical_runtime_event_stream_line("agent_lifecycle", event)
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn model_request_failed_stream_line(
    event: &StoredRuntimeEvent,
) -> Result<Vec<u8>, serde_json::Error> {
    let RuntimeEventKind::ModelRequestFailed {
        attempt_id,
        failure,
        accounting,
        retry,
    } = &event.event
    else {
        debug_assert!(false, "model failure projection received another event");
        return serde_json::to_vec(&serde_json::Value::Null);
    };
    let retry = match retry {
        ModelRetryDecision::Retry { prepared } => ModelRetryStreamProjection::Retry {
            next_attempt_id: &prepared.attempt_id,
            next_request_number: prepared.request.request_number,
            next_attempt: prepared.request.attempt,
            max_retries: prepared.max_retries,
            decision_unix_ms: prepared.decision_unix_ms,
            backoff_ms: prepared.backoff_ms,
            not_before_unix_ms: prepared.not_before_unix_ms,
        },
        ModelRetryDecision::Stop { reason } => ModelRetryStreamProjection::Stop { reason: *reason },
    };
    let mut bytes = serde_json::to_vec(&ModelRequestFailedStreamEvent {
        event_type: "model_request_failed",
        run_id: &event.run_id,
        event_id: &event.event_id,
        sequence: event.sequence,
        occurred_at_unix_ms: event.occurred_at_unix_ms,
        attempt_id,
        failure,
        retry,
        accounting: ModelFailureAccountingProjection::from(accounting.as_ref()),
        schema_version: EXEC_STREAM_SCHEMA_VERSION,
        schema: EXEC_STREAM_SCHEMA,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Serialize)]
#[cfg_attr(test, allow(dead_code))]
struct ModelRequestFailedStreamEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    run_id: &'a RunId,
    event_id: &'a RuntimeEventId,
    sequence: u64,
    occurred_at_unix_ms: u64,
    attempt_id: &'a AttemptId,
    failure: &'a ModelAttemptFailure,
    retry: ModelRetryStreamProjection<'a>,
    accounting: ModelFailureAccountingProjection,
    schema_version: u32,
    schema: &'static str,
}

#[derive(Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[cfg_attr(test, allow(dead_code))]
enum ModelRetryStreamProjection<'a> {
    Retry {
        next_attempt_id: &'a AttemptId,
        next_request_number: u32,
        next_attempt: u32,
        max_retries: u32,
        decision_unix_ms: u64,
        backoff_ms: u64,
        not_before_unix_ms: u64,
    },
    Stop {
        reason: ModelRetryStopReason,
    },
}

#[derive(Serialize)]
#[cfg_attr(test, allow(dead_code))]
struct ModelFailureAccountingProjection {
    physical_started: u64,
    physical_completed: u64,
    physical_in_flight: u64,
    runtime_retries: u64,
    usage_complete: bool,
    billing_unknown: bool,
}

impl From<&ModelAccounting> for ModelFailureAccountingProjection {
    fn from(accounting: &ModelAccounting) -> Self {
        Self {
            physical_started: accounting.total_started(),
            physical_completed: accounting.total_completed(),
            physical_in_flight: accounting.total_in_flight(),
            runtime_retries: accounting.runtime_retries,
            usage_complete: accounting.usage_complete,
            billing_unknown: accounting.billing_unknown,
        }
    }
}

fn canonical_runtime_event_stream_line(
    event_type: &'static str,
    event: &StoredRuntimeEvent,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(&CanonicalRuntimeStreamEvent {
        event_type,
        runtime_event: event,
        schema_version: EXEC_STREAM_SCHEMA_VERSION,
        schema: EXEC_STREAM_SCHEMA,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}
