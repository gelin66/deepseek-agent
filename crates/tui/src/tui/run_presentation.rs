//! Replay-safe, read-only presentation of one canonical root Run.
//!
//! Runtime and RunStore remain the sole owners of every fact consumed here.
//! This reducer owns only ephemeral UI derivation: current phase, confirmed
//! workspace change, verification progress, and the frozen permission choice.

use std::collections::{BTreeSet, HashMap};

use dse_localization::{MessageId, ProductLanguage, tr_in};
use dse_protocol::agent_runtime::{
    AgentTaskId, RunId, RuntimeEventKind, StoredRuntimeEvent, TerminalState, ToolSideEffectStatus,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunPresentationPhase {
    #[default]
    Idle,
    Thinking,
    Executing,
    WaitingForUser,
    Verifying,
    Reworking,
    Completed,
    Blocked,
    Failed,
    Cancelled,
    Interrupted,
    RecoveryRequired,
}

impl RunPresentationPhase {
    #[must_use]
    pub fn localized_label(self, language: ProductLanguage) -> String {
        let id = match self {
            Self::Idle => MessageId::PhaseIdle,
            Self::Thinking => MessageId::WorkPhaseThinking,
            Self::Executing => MessageId::WorkPhaseExecuting,
            Self::WaitingForUser => MessageId::PhaseWaitingOnYou,
            Self::Verifying => MessageId::WorkPhaseVerifying,
            Self::Reworking => MessageId::WorkPhaseReworking,
            Self::Completed => MessageId::WorkPhaseCompleted,
            Self::Blocked => MessageId::WorkPhaseBlocked,
            Self::Failed => MessageId::PhaseFailed,
            Self::Cancelled => MessageId::WorkPhaseCancelled,
            Self::Interrupted => MessageId::WorkPhaseInterrupted,
            Self::RecoveryRequired => MessageId::WorkPhaseRecoveryRequired,
        };
        tr_in(language, id).into_owned()
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Thinking | Self::Executing | Self::Verifying | Self::Reworking
        )
    }

    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Completed)
    }

    #[must_use]
    pub const fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::WaitingForUser
                | Self::Blocked
                | Self::Failed
                | Self::Cancelled
                | Self::Interrupted
                | Self::RecoveryRequired
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerificationPresentation {
    #[default]
    NotStarted,
    Preparing,
    Running,
    Passed,
    Failed,
}

impl VerificationPresentation {
    #[must_use]
    pub fn localized_label(
        self,
        language: ProductLanguage,
        satisfied: usize,
        total: usize,
    ) -> String {
        let text = match self {
            Self::NotStarted => tr_in(language, MessageId::WorkVerificationNotStarted).into_owned(),
            Self::Preparing => tr_in(language, MessageId::WorkVerificationPreparing).into_owned(),
            Self::Running => tr_in(language, MessageId::WorkVerificationRunning).into_owned(),
            Self::Passed => tr_in(language, MessageId::WorkVerificationPassed).into_owned(),
            Self::Failed => tr_in(language, MessageId::WorkVerificationFailed).into_owned(),
        };
        text.replace("{satisfied}", &satisfied.to_string())
            .replace("{total}", &total.to_string())
    }
}

/// Ephemeral projection for the canonical root currently visible in the TUI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalRunPresentation {
    root_run_id: Option<RunId>,
    objective: Option<String>,
    acceptance_total: usize,
    phase: RunPresentationPhase,
    verification: VerificationPresentation,
    satisfied_acceptance_ids: BTreeSet<String>,
    workspace_change_confirmed: bool,
    changed_files: BTreeSet<String>,
    sealed_writer_files: HashMap<AgentTaskId, Vec<String>>,
    auto_approve: Option<bool>,
    reworking: bool,
}

impl CanonicalRunPresentation {
    pub fn apply(&mut self, stored: &StoredRuntimeEvent) {
        if let RuntimeEventKind::RunCreated { request } = &stored.event {
            if request.parent_run_id.is_none() {
                self.begin_root(stored.run_id.clone(), request);
            }
            return;
        }
        if self.root_run_id.as_ref() != Some(&stored.run_id) {
            return;
        }

        match &stored.event {
            RuntimeEventKind::RunCreated { .. } => {}
            RuntimeEventKind::ContextCompactionCommitted { .. }
            | RuntimeEventKind::ModelRequestPrepared { .. }
            | RuntimeEventKind::ModelRequestInFlight { .. }
            | RuntimeEventKind::ModelRequestFailed { .. }
            | RuntimeEventKind::ModelResponseCommitted { .. }
            | RuntimeEventKind::ContentDelta { .. }
            | RuntimeEventKind::ReasoningDelta { .. }
            | RuntimeEventKind::SteerApplied { .. } => self.resume_model_work(),
            RuntimeEventKind::ToolPrepared { .. }
            | RuntimeEventKind::ToolExecutionStarted { .. }
            | RuntimeEventKind::AgentTaskPrepared { .. }
            | RuntimeEventKind::AgentWorkspaceCreated { .. }
            | RuntimeEventKind::AgentSealPrepared { .. }
            | RuntimeEventKind::AgentResultCollected { .. }
            | RuntimeEventKind::AgentIntegrationPrepared { .. }
            | RuntimeEventKind::AgentIntegrationStarted { .. }
            | RuntimeEventKind::AgentIntegrationFailed { .. }
            | RuntimeEventKind::AgentCleanupPrepared { .. }
            | RuntimeEventKind::AgentCleanupCommitted { .. }
            | RuntimeEventKind::ChildStarted { .. }
            | RuntimeEventKind::ChildFinished { .. } => self.resume_execution(),
            RuntimeEventKind::InteractionRequested { .. } => {
                self.phase = RunPresentationPhase::WaitingForUser;
            }
            RuntimeEventKind::InteractionResolved { .. } => self.resume_execution(),
            RuntimeEventKind::ToolOutcomeCommitted {
                outcome,
                workspace_state,
                ..
            } => {
                if outcome.side_effect == ToolSideEffectStatus::Applied && workspace_state.is_some()
                {
                    self.workspace_change_confirmed = true;
                }
                self.resume_model_work();
            }
            RuntimeEventKind::WorkspaceObserved { .. } => {}
            RuntimeEventKind::CompletionProposed { .. } => {
                self.phase = RunPresentationPhase::Verifying;
            }
            RuntimeEventKind::HostVerificationPrepared { .. } => {
                self.verification = VerificationPresentation::Preparing;
                self.phase = RunPresentationPhase::Verifying;
            }
            RuntimeEventKind::HostVerificationStarted { .. } => {
                self.verification = VerificationPresentation::Running;
                self.phase = RunPresentationPhase::Verifying;
            }
            RuntimeEventKind::HostVerificationCommitted { receipt, .. } => {
                if let Some(receipt) = receipt {
                    self.satisfied_acceptance_ids
                        .insert(receipt.acceptance_id.0.clone());
                    self.verification = VerificationPresentation::Passed;
                } else {
                    self.verification = VerificationPresentation::Failed;
                }
                self.phase = RunPresentationPhase::Verifying;
            }
            RuntimeEventKind::CompletionRejected { .. } => {
                self.reworking = true;
                self.verification = VerificationPresentation::Failed;
                self.phase = RunPresentationPhase::Reworking;
            }
            RuntimeEventKind::AgentSealCommitted {
                task_id,
                changed_files,
                ..
            } => {
                self.sealed_writer_files
                    .insert(task_id.clone(), changed_files.clone());
                self.resume_execution();
            }
            RuntimeEventKind::AgentIntegrationCommitted { task_id, .. } => {
                if let Some(files) = self.sealed_writer_files.remove(task_id) {
                    self.changed_files.extend(files);
                }
                self.workspace_change_confirmed = true;
                self.resume_execution();
            }
            RuntimeEventKind::SteerQueued { .. } | RuntimeEventKind::ControlRequested { .. } => {}
            RuntimeEventKind::Terminal { outcome } => {
                self.apply_terminal(&outcome.terminal);
            }
        }
    }

    fn begin_root(&mut self, run_id: RunId, request: &dse_protocol::agent_runtime::RunRequest) {
        *self = Self::default();
        self.root_run_id = Some(run_id);
        self.phase = RunPresentationPhase::Thinking;
        self.auto_approve = Some(request.environment.auto_approve);
        if let Some(contract) = &request.task_contract {
            self.objective = Some(contract.definition.objective.clone());
            self.acceptance_total = contract.definition.acceptance.len();
        }
    }

    fn resume_model_work(&mut self) {
        self.phase = if self.reworking {
            RunPresentationPhase::Reworking
        } else {
            RunPresentationPhase::Thinking
        };
    }

    fn resume_execution(&mut self) {
        self.phase = if self.reworking {
            RunPresentationPhase::Reworking
        } else {
            RunPresentationPhase::Executing
        };
    }

    fn apply_terminal(&mut self, terminal: &TerminalState) {
        self.phase = match terminal {
            TerminalState::Completed { decision, .. } => {
                self.satisfied_acceptance_ids.extend(
                    decision
                        .satisfied
                        .iter()
                        .map(|satisfaction| satisfaction.acceptance_id().0.clone()),
                );
                self.verification = VerificationPresentation::Passed;
                RunPresentationPhase::Completed
            }
            TerminalState::Blocked { .. } => RunPresentationPhase::Blocked,
            TerminalState::Failed { .. } => RunPresentationPhase::Failed,
            TerminalState::Cancelled => RunPresentationPhase::Cancelled,
            TerminalState::Interrupted => RunPresentationPhase::Interrupted,
            TerminalState::RecoveryRequired { .. } => RunPresentationPhase::RecoveryRequired,
        };
    }

    #[must_use]
    pub fn has_root(&self) -> bool {
        self.root_run_id.is_some()
    }

    #[must_use]
    pub fn root_run_id(&self) -> Option<&RunId> {
        self.root_run_id.as_ref()
    }

    #[must_use]
    pub fn objective(&self) -> Option<&str> {
        self.objective.as_deref()
    }

    #[must_use]
    pub const fn phase(&self) -> RunPresentationPhase {
        self.phase
    }

    #[must_use]
    pub const fn verification(&self) -> VerificationPresentation {
        self.verification
    }

    #[must_use]
    pub const fn acceptance_total(&self) -> usize {
        self.acceptance_total
    }

    #[must_use]
    pub fn satisfied_acceptance_count(&self) -> usize {
        self.satisfied_acceptance_ids.len()
    }

    #[must_use]
    pub const fn workspace_change_confirmed(&self) -> bool {
        self.workspace_change_confirmed
    }

    #[must_use]
    pub fn confirmed_changed_file_count(&self) -> usize {
        self.changed_files.len()
    }

    #[must_use]
    pub const fn auto_approve(&self) -> Option<bool> {
        self.auto_approve
    }
}

#[cfg(test)]
mod tests {
    use dse_protocol::{
        agent_runtime::{
            AgentOutcome, AgentResultDetails, ModelAccounting, OperationId, RunRequest,
            RuntimeEventId, ToolOutcome,
        },
        task::{
            AcceptanceId, AcceptanceSatisfaction, CompletionCandidateId, CompletionDecision,
            TaskContract, TaskDefinition, TaskGenerationId, WorkspaceRevision, WorkspaceState,
        },
    };

    use super::*;

    fn root_request(run_id: &str, auto_approve: bool) -> RunRequest {
        let mut request = RunRequest::new(
            TaskContract {
                generation_id: TaskGenerationId::from(run_id),
                definition: TaskDefinition::host("repair the parser"),
            },
            "system",
        );
        request.environment.auto_approve = auto_approve;
        request
    }

    fn stored(run_id: &str, sequence: u64, event: RuntimeEventKind) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: 25,
            run_id: RunId::from(run_id),
            parent_run_id: None,
            event_id: RuntimeEventId(format!("event-{sequence}")),
            sequence,
            occurred_at_unix_ms: sequence,
            event,
        }
    }

    fn workspace(generation: u64) -> WorkspaceState {
        WorkspaceState {
            generation,
            revision: WorkspaceRevision::Known {
                sha256: format!("revision-{generation}"),
            },
        }
    }

    #[test]
    fn root_flow_tracks_confirmed_change_and_terminal_verification() {
        let mut view = CanonicalRunPresentation::default();
        view.apply(&stored(
            "root",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request("root", false)),
            },
        ));
        assert_eq!(view.phase(), RunPresentationPhase::Thinking);
        assert_eq!(view.objective(), Some("repair the parser"));
        assert_eq!(view.acceptance_total(), 1);
        assert_eq!(view.auto_approve(), Some(false));

        view.apply(&stored(
            "root",
            2,
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId::from("op"),
                call_id: "call".to_owned(),
                name: "apply_patch".to_owned(),
                outcome: Box::new(ToolOutcome {
                    side_effect: ToolSideEffectStatus::Applied,
                    ..ToolOutcome::success("done")
                }),
                workspace_state: Some(workspace(2)),
            },
        ));
        assert!(view.workspace_change_confirmed());
        assert_eq!(view.phase(), RunPresentationPhase::Thinking);

        let decision = CompletionDecision {
            candidate_id: CompletionCandidateId::from("candidate"),
            generation_id: TaskGenerationId::from("root"),
            workspace_state: workspace(2),
            satisfied: vec![AcceptanceSatisfaction::Host {
                acceptance_id: AcceptanceId::from("host"),
            }],
        };
        view.apply(&stored(
            "root",
            3,
            RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: RunId::from("root"),
                    parent_run_id: None,
                    terminal: TerminalState::Completed {
                        message: "done".to_owned(),
                        decision,
                    },
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 1,
                    runtime_retries: 0,
                    tool_calls: 1,
                    details: AgentResultDetails::default(),
                }),
            },
        ));
        assert_eq!(view.phase(), RunPresentationPhase::Completed);
        assert_eq!(view.verification(), VerificationPresentation::Passed);
        assert_eq!(view.satisfied_acceptance_count(), 1);
    }

    #[test]
    fn child_run_cannot_replace_root_presentation() {
        let mut view = CanonicalRunPresentation::default();
        view.apply(&stored(
            "root",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request("root", false)),
            },
        ));
        let mut child = root_request("child", true);
        child.parent_run_id = Some(RunId::from("root"));
        view.apply(&stored(
            "child",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(child),
            },
        ));
        assert_eq!(view.root_run_id(), Some(&RunId::from("root")));
        assert_eq!(view.objective(), Some("repair the parser"));
        assert_eq!(view.auto_approve(), Some(false));
    }

    #[test]
    fn rejection_keeps_followup_activity_in_reworking_phase() {
        use dse_protocol::task::{
            CompletionRejection, CompletionRequiredTransition, EvidenceSealRejection,
        };

        let mut view = CanonicalRunPresentation::default();
        view.apply(&stored(
            "root",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request("root", false)),
            },
        ));
        view.apply(&stored(
            "root",
            2,
            RuntimeEventKind::CompletionRejected {
                rejection: CompletionRejection {
                    candidate_id: CompletionCandidateId::from("candidate"),
                    generation_id: TaskGenerationId::from("root"),
                    unmet_acceptance_ids: vec![AcceptanceId::from("host")],
                    cause: EvidenceSealRejection::VerifierFailed,
                    required_transition: CompletionRequiredTransition::EffectiveWorkspaceMutation,
                    reason: "verification failed".to_owned(),
                },
            },
        ));
        view.apply(&stored(
            "root",
            3,
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId::from("retry-op"),
            },
        ));
        assert_eq!(view.phase(), RunPresentationPhase::Reworking);
        assert_eq!(view.verification(), VerificationPresentation::Failed);
    }

    #[test]
    fn writer_files_are_confirmed_only_after_root_integration() {
        let mut view = CanonicalRunPresentation::default();
        view.apply(&stored(
            "root",
            1,
            RuntimeEventKind::RunCreated {
                request: Box::new(root_request("root", false)),
            },
        ));
        view.apply(&stored(
            "root",
            2,
            RuntimeEventKind::AgentSealCommitted {
                task_id: AgentTaskId::from("writer"),
                final_commit: "1234567890123456789012345678901234567890".to_owned(),
                diff_sha256: "diff".to_owned(),
                changed_files: vec!["src/lib.rs".to_owned(), "src/main.rs".to_owned()],
                writer_workspace_state_after: workspace(1),
            },
        ));
        assert_eq!(view.confirmed_changed_file_count(), 0);

        view.apply(&stored(
            "root",
            3,
            RuntimeEventKind::AgentIntegrationCommitted {
                task_id: AgentTaskId::from("writer"),
                integration_id: OperationId::from("integrate"),
                root_head_commit: "1234567890123456789012345678901234567890".to_owned(),
                root_workspace_state_after: workspace(2),
            },
        ));
        assert_eq!(view.confirmed_changed_file_count(), 2);
        assert!(view.workspace_change_confirmed());
    }
}
