use std::sync::Arc;

use async_trait::async_trait;

use crate::{AgentTask, AgentTaskId, AgentWorkspaceAssignment, ToolExecutor, WorkspaceState};

/// Read-only input used to freeze an isolated writer assignment before any
/// worktree side effect is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterPreparation {
    pub task_id: AgentTaskId,
    pub root_workspace: String,
    pub allowed_paths: Vec<String>,
}

/// Host-derived workspace identity persisted in `AgentTaskPrepared`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterPlan {
    pub assignment: AgentWorkspaceAssignment,
}

/// Exact writer workspace binding used by the child AgentRuntime.
///
/// The executor must be a fresh instance scoped to `assignment`; sharing the
/// root executor would silently turn worktree isolation into presentation
/// metadata.
#[derive(Clone)]
pub struct WriterBinding {
    pub assignment: AgentWorkspaceAssignment,
    pub writer_workspace_state: WorkspaceState,
    pub tools: Arc<dyn ToolExecutor>,
}

impl std::fmt::Debug for WriterBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WriterBinding")
            .field("assignment", &self.assignment)
            .field("writer_workspace_state", &self.writer_workspace_state)
            .field("tools", &"<isolated ToolExecutor>")
            .finish()
    }
}

/// Host-sealed immutable writer result. The concrete Git owner reconstructs
/// these facts during recovery rather than trusting model output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterSeal {
    pub base_commit: String,
    pub final_commit: String,
    pub diff_sha256: String,
    pub changed_files: Vec<String>,
    pub writer_workspace_state: WorkspaceState,
}

/// Result of guarded, idempotent integration into the canonical root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterIntegration {
    pub root_head_commit: String,
    pub root_workspace_state: WorkspaceState,
}

/// Exact-owned cleanup result. Retention is explicit so ambiguous state is
/// never made to look like successful cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterCleanup {
    pub worktree_removed: bool,
    pub branch_removed: bool,
    pub retained_for_recovery: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOrchestrationErrorKind {
    Rejected,
    Conflict,
    RecoveryRequired,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct AgentOrchestrationError {
    pub kind: AgentOrchestrationErrorKind,
    pub code: String,
    pub message: String,
}

impl AgentOrchestrationError {
    #[must_use]
    pub fn new(
        kind: AgentOrchestrationErrorKind,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Runtime-side boundary for the single canonical writer lifecycle.
///
/// Implementations own only Git/worktree side effects and the construction
/// of an isolated tool executor. AgentRuntime remains the sole model loop,
/// RunStore writer, lifecycle-event owner, and terminal-state authority.
#[async_trait]
pub trait AgentOrchestrator: Send + Sync {
    /// Prove the root is a supported clean Git checkout and derive the exact
    /// assignment. This method must not create a branch, worktree, or file.
    async fn prepare_writer(
        &self,
        request: WriterPreparation,
    ) -> Result<WriterPlan, AgentOrchestrationError>;

    /// Create or exactly recover the writer workspace after
    /// `AgentTaskPrepared` has committed. `sealed` distinguishes an unsealed
    /// allocation from a Host-sealed recovery.
    async fn bind_writer(
        &self,
        task: &AgentTask,
        sealed: Option<&WriterSeal>,
    ) -> Result<WriterBinding, AgentOrchestrationError>;

    /// Seal the current writer result after `AgentSealPrepared` commits.
    async fn seal_writer(&self, task: &AgentTask) -> Result<WriterSeal, AgentOrchestrationError>;

    /// Fast-forward or prove an already-applied exact integration.
    async fn integrate_writer(
        &self,
        task: &AgentTask,
        seal: &WriterSeal,
        expected_root: &WorkspaceState,
    ) -> Result<WriterIntegration, AgentOrchestrationError>;

    /// Remove only the exact resources owned by `task`.
    async fn cleanup_writer(
        &self,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
    ) -> Result<WriterCleanup, AgentOrchestrationError>;
}
