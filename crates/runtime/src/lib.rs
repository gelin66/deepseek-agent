//! The single UI-independent Agent execution kernel.
//!
//! Concrete DeepSeek transport, tools, presentation, and persistence plug in
//! through the four small ports in this module. Runtime events are persisted
//! before clients can observe them; the store assigns their canonical order.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
pub use dse_protocol::agent_runtime::*;
pub use dse_protocol::task::*;
use tokio::sync::Notify;

mod agent;
mod orchestration;
mod store;

pub use agent::{
    AgentControl, AgentRuntime, ChildRouteContext, ChildRunRoutePolicy, ChildRunRouteSelection,
    ControlError, ModelToolAuthority, RunReadyError, RuntimeJoinError, RuntimeRun,
};
pub use orchestration::{
    AgentOrchestrationError, AgentOrchestrationErrorKind, AgentOrchestrator, WriterBinding,
    WriterIntegration, WriterPlan, WriterPreparation, WriterSeal,
};
pub use store::{
    AcquiredRun, AgentChildFinishedFact, AgentChildStartedFact, AgentCleanupLifecycle,
    AgentIntegrationCommittedFact, AgentIntegrationFailureFact, AgentIntegrationLifecycle,
    AgentSealCommittedFact, AgentSealLifecycle, AgentTaskLifecycle, AgentWorkspaceCreatedFact,
    CommandReceipt, CommittedContextCompaction, CreatedRun, CreationIntent, CreationReservation,
    DurableActionState, DurableCommand, InMemoryRunStore, PendingControl, PendingHostVerification,
    PendingModelAction, PendingSteer, PendingToolAction, PendingUserInteraction, ReservedCreation,
    RootRunRecord, RunLease, RunReplay, RunSnapshot, StoppedModelFailure, apply_event,
    reduce_events, validate_continuation_request,
};

/// SHA-256 of the exact ordered model-visible tool definitions.
///
/// The Runtime owns this identity because it owns actor filtering, named
/// verifier specialization, the agent definition, and terminal empty
/// catalogs. Callers must not derive a child catalog identity from its root.
#[must_use]
pub fn canonical_tool_catalog_sha256(catalog: &[ToolDefinition]) -> String {
    use sha2::{Digest, Sha256};

    let bytes = serde_json::to_vec(catalog).expect("canonical tool catalog is serializable");
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

pub(crate) fn named_verifier_acceptance(
    task: &TaskDefinition,
) -> Option<(&AcceptanceId, &VerifierSpec)> {
    task.acceptance
        .iter()
        .find_map(|acceptance| match acceptance {
            TaskAcceptance::Verifier { id, verifier, .. } => Some((id, verifier)),
            TaskAcceptance::Host { .. } => None,
        })
}

/// Resolve the model-visible named-verifier handle into the exact frozen Host
/// invocation. Both authorization and execution use this one derivation so
/// the durable decision cannot bind only the abbreviated model arguments.
pub(crate) fn resolve_named_verifier_invocation(
    contract: Option<&TaskContract>,
    invocation: &ToolInvocation,
) -> Result<ToolInvocation, String> {
    let Some((acceptance_id, verifier)) = contract
        .map(|contract| &contract.definition)
        .and_then(named_verifier_acceptance)
    else {
        return Ok(invocation.clone());
    };
    if invocation.name != verifier.verifier_id {
        return Ok(invocation.clone());
    }
    let arguments = invocation
        .arguments
        .parsed
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "冻结 verifier 只接受包含 verifier_id 的 JSON 对象".to_owned())?;
    if arguments.len() != 1
        || arguments
            .get("verifier_id")
            .and_then(serde_json::Value::as_str)
            != Some(acceptance_id.0.as_str())
    {
        return Err(format!(
            "只接受冻结 verifier ID '{}'，不得提交或覆盖完整 verifier 参数",
            acceptance_id.0
        ));
    }
    Ok(ToolInvocation {
        run_id: invocation.run_id.clone(),
        call_id: invocation.call_id.clone(),
        name: verifier.verifier_id.clone(),
        arguments: ToolArguments::from_value(verifier.parameters.clone()),
    })
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("model error {code}: {message}")]
pub struct ModelPortError {
    pub code: String,
    pub category: ModelErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub response: ModelResponseEvidence,
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
            response: ModelResponseEvidence::default(),
        }
    }

    #[must_use]
    pub fn with_response(mut self, response: ModelResponseEvidence) -> Self {
        self.response = response;
        self
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
    #[error("continuation inherited facts do not match the source run")]
    InheritedFactsMismatch,
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

    /// Conservatively classify a tool definition before model arguments
    /// exist. Catalog filtering uses this as a first capability boundary;
    /// exact invocation classification remains the execution-time authority.
    fn definition_workspace_access(&self, _name: &str) -> WorkspaceAccess {
        WorkspaceAccess::MayWrite
    }

    /// Classify whether this invocation can mutate the workspace. The
    /// conservative default protects embedders until they provide a narrower
    /// classification.
    fn workspace_access(&self, _invocation: &ToolInvocation) -> WorkspaceAccess {
        WorkspaceAccess::MayWrite
    }

    /// Reject malformed or schema-invalid arguments before execution starts.
    /// The concrete executor remains the sole owner of argument semantics;
    /// Runtime only commits the returned canonical outcome.
    fn preflight(&self, _invocation: &ToolInvocation) -> Option<ToolOutcome> {
        None
    }

    /// Observe the current canonical workspace revision. Runtime owns the
    /// monotonic workspace generation and never trusts a tool-supplied epoch.
    async fn observe_workspace_revision(&self) -> Result<String, ToolExecutionError> {
        Err(ToolExecutionError::new(
            "workspace_revision_unavailable",
            "tool executor cannot observe the workspace revision",
        ))
    }

    /// Return the Host-owned authorization decision for this exact invocation
    /// and workspace state. Runtime persists the decision before any approval
    /// interaction or side effect and never reclassifies it after reopen.
    fn authorize(
        &self,
        mode: RunPermissionMode,
        invocation: &ToolInvocation,
        workspace_state: &WorkspaceState,
    ) -> Result<ToolAuthorizationDecision, ToolExecutionError> {
        Ok(ToolAuthorizationDecision {
            mode,
            tool_name: invocation.name.clone(),
            arguments_sha256: invocation.arguments_sha256(),
            workspace_state: workspace_state.clone(),
            disposition: ToolAuthorizationDisposition::Allow,
            risk: ApprovalRisk::Routine,
            matched_rule: Some("executor_default".to_owned()),
            reason: "tool executor allows this invocation".to_owned(),
            prompt: None,
        })
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
    /// Atomically reserve the durable identity of a Start/Continue command
    /// before composition can perform any external model request.
    ///
    /// `command_sha256` identifies the canonical caller payload. `intent` is
    /// the first Host-resolved launch payload. A retry with the same digest
    /// must return that original intent even when re-resolution would now
    /// differ; a different digest for the same command id is a conflict.
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
