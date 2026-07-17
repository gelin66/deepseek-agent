//! The single UI-independent Agent execution kernel.
//!
//! Concrete DeepSeek transport, tools, presentation, and persistence plug in
//! through the four small ports in this module. Runtime events are persisted
//! before clients can observe them; the store assigns their canonical order.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
pub use codewhale_protocol::agent_runtime::*;
use tokio::sync::Notify;

mod agent;
mod store;

pub use agent::{
    AgentControl, AgentRuntime, ControlError, RunReadyError, RuntimeJoinError, RuntimeRun,
};
pub use store::{
    AcquiredRun, CommandReceipt, CommittedContextCompaction, CreatedRun, CreationIntent,
    CreationReservation, DurableActionState, DurableCommand, InMemoryRunStore,
    PendingContextCompaction, PendingControl, PendingModelAction, PendingSteer, PendingToolAction,
    PendingUserInteraction, ReservedCreation, RootRunRecord, RunLease, RunReplay, RunSnapshot,
    StoppedContextCompactionFailure, StoppedModelFailure, apply_event, reduce_events,
    validate_continuation_request,
};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("model error {code}: {message}")]
pub struct ModelPortError {
    pub code: String,
    pub category: ModelErrorCategory,
    pub message: String,
    pub retryable: bool,
}

impl ModelPortError {
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        category: ModelErrorCategory,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            message: message.into(),
            retryable,
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("tool error {code}: {message}")]
pub struct ToolExecutionError {
    pub code: String,
    pub message: String,
}

impl ToolExecutionError {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RunStoreError {
    #[error("run {run_id} already exists")]
    AlreadyExists { run_id: RunId },
    #[error("run {run_id} does not exist")]
    NotFound { run_id: RunId },
    #[error("run {run_id} is owned by another live process")]
    AlreadyRunning { run_id: RunId },
    #[error("run {run_id} already reached its terminal event")]
    AlreadyTerminal { run_id: RunId },
    #[error("run {source_run_id} cannot be continued: {reason}")]
    InvalidContinuation {
        source_run_id: RunId,
        reason: ContinuationError,
    },
    #[error("run {run_id} lease epoch {epoch} is stale")]
    StaleLease { run_id: RunId, epoch: u64 },
    #[error("event id {event_id} for run {run_id} was reused with different content")]
    EventConflict {
        run_id: RunId,
        event_id: RuntimeEventId,
    },
    #[error("creation command id {command_id:?} was reused with different content")]
    CreationConflict { command_id: CommandId },
    #[error("run {run_id} has corrupt persisted state: {message}")]
    Corrupt { run_id: RunId, message: String },
    #[error(
        "run event schema version {found} is outside the supported range {minimum_supported}..={maximum_supported}"
    )]
    UnsupportedSchema {
        found: u32,
        minimum_supported: u32,
        maximum_supported: u32,
    },
    #[error("run store failed: {message}")]
    Backend { message: String },
}

#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum ContinuationError {
    #[error("source run is not terminal")]
    SourceNotTerminal,
    #[error("source run is a child Agent")]
    SourceIsChild,
    #[error("source run requires explicit recovery resolution")]
    RecoveryRequired,
    #[error("new run is not a root Agent")]
    NewRunIsNotRoot,
    #[error("source and continuation workspaces differ")]
    WorkspaceMismatch,
    #[error("continuation transcript is not the source canonical transcript")]
    TranscriptMismatch,
    #[error("continuation context projection is not the source projection")]
    ContextProjectionMismatch,
    #[error("continuation lineage is missing, cyclic, or internally inconsistent")]
    LineageCorrupt,
}

/// Pull-based model stream. Calling `next` only after the previous stored
/// event has reached the sink gives the runtime an explicit backpressure
/// boundary without choosing a transport-specific channel.
#[async_trait]
pub trait ModelStream: Send {
    async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>>;
}

#[async_trait]
pub trait ModelPort: Send + Sync {
    async fn stream(&self, request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError>;

    /// Return the physical request and billing truth owned by the backend.
    /// Child runs pass `false`; only a settled root run passes `true` and
    /// seals further admission before its unique terminal event is built.
    async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError>;
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.notify.notify_waiters();
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.state.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn definitions(&self) -> Vec<ToolDefinition>;

    /// Return a host-owned approval prompt for this exact invocation.
    /// Concrete tool implementations remain the only owner of their risk and
    /// policy rules; the runtime only persists and enforces the handshake.
    fn approval_prompt(
        &self,
        _invocation: &ToolInvocation,
    ) -> Result<Option<ToolApprovalPrompt>, ToolExecutionError> {
        Ok(None)
    }

    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutcome, ToolExecutionError>;
}

#[async_trait]
pub trait RuntimeEventSink: Send + Sync {
    async fn emit(&self, event: StoredRuntimeEvent);
}

#[async_trait]
pub trait RunStore: Send + Sync {
    /// Atomically reserve the durable identity of a Start/Continue/Compact
    /// command before composition can perform any external model request.
    async fn reserve_creation(
        &self,
        command_id: &CommandId,
        command_sha256: &str,
        proposed_run_id: RunId,
        intent: CreationIntent,
    ) -> Result<ReservedCreation, RunStoreError>;

    async fn creation(
        &self,
        command_id: &CommandId,
    ) -> Result<Option<CreationReservation>, RunStoreError>;

    async fn list_pending_creations(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<CreationReservation>, RunStoreError>;

    async fn create(&self, request: RunRequest) -> Result<CreatedRun, RunStoreError>;

    async fn acquire(&self, run_id: &RunId) -> Result<AcquiredRun, RunStoreError>;

    async fn append(
        &self,
        lease: &RunLease,
        event: PendingRuntimeEvent,
    ) -> Result<StoredRuntimeEvent, RunStoreError>;

    async fn load(&self, run_id: &RunId) -> Result<Option<RunReplay>, RunStoreError>;

    async fn events_after(
        &self,
        run_id: &RunId,
        sequence: u64,
    ) -> Result<Vec<StoredRuntimeEvent>, RunStoreError>;

    async fn release(&self, lease: &RunLease) -> Result<(), RunStoreError>;

    async fn list_root_runs(
        &self,
        workspace: &str,
        limit: u32,
    ) -> Result<Vec<RootRunRecord>, RunStoreError>;
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Default)]
pub struct NullEventSink;

#[async_trait]
impl RuntimeEventSink for NullEventSink {
    async fn emit(&self, _event: StoredRuntimeEvent) {}
}
