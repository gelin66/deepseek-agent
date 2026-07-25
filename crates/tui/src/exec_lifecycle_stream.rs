//! Exact machine-stream projection for canonical Agent lifecycle events.
//!
//! Exec does not maintain a second lifecycle DTO. Every root/child writer fact
//! is carried as the original stored event so identity, outcome, accounting,
//! and handoff fields cannot drift from the RunStore truth.

use dse_protocol::agent_runtime::{RuntimeEventKind, StoredRuntimeEvent};
use serde::Serialize;

pub(crate) const EXEC_STREAM_SCHEMA: &str = "codewhale.exec-stream";
pub(crate) const EXEC_STREAM_SCHEMA_VERSION: u32 = 3;

#[derive(Serialize)]
struct AgentLifecycleStreamEvent<'a> {
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
    let mut bytes = serde_json::to_vec(&AgentLifecycleStreamEvent {
        event_type: "agent_lifecycle",
        runtime_event: event,
        schema_version: EXEC_STREAM_SCHEMA_VERSION,
        schema: EXEC_STREAM_SCHEMA,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}
