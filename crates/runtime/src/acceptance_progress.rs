//! Request-local acceptance progress derived from the canonical Run snapshot.
//!
//! This module does not persist progress. `RunSnapshot` remains the sole
//! replay truth; ContextBroker consumes this compact view for one model
//! request and the completion gate independently revalidates every receipt.

use codewhale_context::compaction::{
    AcceptanceEvidenceNeed, AcceptanceProgressItem, AcceptanceProgressProjection,
    AcceptanceProgressReason, AcceptanceProgressStatus, ContextInput,
};
use codewhale_context::system_prompt_uses_acceptance_progress;
use codewhale_protocol::agent_runtime::{
    ToolDefinition, ToolEvidenceStatus, ToolInvocationStatus, ToolOperationStatus, ToolOutcome,
    ToolSideEffectStatus, ToolTransportStatus, TranscriptEntry,
};
use codewhale_protocol::task::{
    AcceptanceId, CompletionRejection, CompletionRequiredTransition, EvidenceReceipt,
    EvidenceSealRejection, TaskAcceptance, TaskContract, VerifierEvidencePolicy,
    VerifierObservation, VerifierSpec, VerifierVerdict, WorkspaceRevision,
};

use crate::store::RunSnapshot;

/// One owned derivation plus the borrowed canonical facts needed by
/// ContextBroker. Agent execution and RunStore replay use this same builder.
pub(crate) struct RuntimeContextInput<'a> {
    snapshot: &'a RunSnapshot,
    tools: &'a [ToolDefinition],
    acceptance_progress: Option<AcceptanceProgressProjection>,
}

impl<'a> RuntimeContextInput<'a> {
    pub(crate) fn new(snapshot: &'a RunSnapshot, tools: &'a [ToolDefinition]) -> Self {
        let acceptance_progress =
            system_prompt_uses_acceptance_progress(&snapshot.request.system_prompt)
                .then(|| derive_acceptance_progress(snapshot))
                .flatten();
        Self {
            snapshot,
            tools,
            acceptance_progress,
        }
    }

    pub(crate) fn as_context_input(&self) -> ContextInput<'_> {
        ContextInput {
            transcript: &self.snapshot.transcript,
            projection: self.snapshot.context_projection.as_ref(),
            task_contract: self.snapshot.request.task_contract.as_ref(),
            workspace_state: &self.snapshot.workspace_state,
            acceptance_progress: self.acceptance_progress.as_ref(),
            evidence_receipts: &self.snapshot.evidence_receipts,
            last_completion_rejection: self.snapshot.last_completion_rejection.as_ref(),
            last_verifier_failure: self
                .snapshot
                .last_host_verification_failure
                .as_ref()
                .map(|failure| &failure.outcome),
            last_verifier_failure_workspace: self
                .snapshot
                .last_host_verification_failure
                .as_ref()
                .map(|failure| &failure.workspace_state),
            tools: self.tools,
        }
    }
}

/// Derive one concise, revision-bound view from the current canonical facts.
#[must_use]
pub fn derive_acceptance_progress(snapshot: &RunSnapshot) -> Option<AcceptanceProgressProjection> {
    let contract = snapshot.request.task_contract.as_ref()?;
    let mut include_last_host_verifier_failure = false;
    let items = contract
        .definition
        .acceptance
        .iter()
        .map(|acceptance| match acceptance {
            TaskAcceptance::Host { id, .. } => pending(
                id,
                AcceptanceProgressReason::HostReviewPending,
                AcceptanceEvidenceNeed::HostCompletionReview,
            ),
            TaskAcceptance::Verifier {
                id,
                evidence_policy,
                verifier,
                ..
            } => {
                if let Some(receipt) =
                    current_receipt(snapshot, contract, id, *evidence_policy, verifier)
                {
                    satisfied(id, receipt)
                } else if snapshot
                    .temporal_evidence_progress
                    .as_ref()
                    .filter(|progress| {
                        progress.acceptance_id == *id && progress.verifier == *verifier
                    })
                    .and_then(|progress| progress.mutation.as_ref())
                    .is_some_and(|mutation| {
                        mutation.workspace_state_after == snapshot.workspace_state
                    })
                {
                    evidence_needed(
                        id,
                        AcceptanceProgressReason::TemporalMutationObserved,
                        AcceptanceEvidenceNeed::LatestHostVerifierPass,
                    )
                } else if let Some(rejection) = relevant_rejection(snapshot, contract, id) {
                    if workspace_changed_after_host_failure(snapshot, rejection) {
                        match evidence_policy {
                            VerifierEvidencePolicy::LatestPass => evidence_needed(
                                id,
                                AcceptanceProgressReason::WorkspaceMutationObserved,
                                AcceptanceEvidenceNeed::LatestHostVerifierPass,
                            ),
                            VerifierEvidencePolicy::FailedWritePass => evidence_needed(
                                id,
                                AcceptanceProgressReason::EvidenceLineageUnavailable,
                                AcceptanceEvidenceNeed::FailedVerifierObservation,
                            ),
                        }
                    } else {
                        include_last_host_verifier_failure |= snapshot
                            .last_host_verification_failure
                            .as_ref()
                            .is_some_and(|failure| failure.rejection == *rejection);
                        evidence_needed(
                            id,
                            rejection_reason(rejection.cause),
                            transition_need(rejection.required_transition),
                        )
                    }
                } else if let Some(progress) =
                    snapshot
                        .temporal_evidence_progress
                        .as_ref()
                        .filter(|progress| {
                            progress.acceptance_id == *id && progress.verifier == *verifier
                        })
                {
                    debug_assert!(
                        progress.mutation.as_ref().is_none_or(|mutation| {
                            mutation.workspace_state_after != snapshot.workspace_state
                        }),
                        "current temporal mutation was handled before the rejection"
                    );
                    evidence_needed(
                        id,
                        AcceptanceProgressReason::TemporalFailureObserved,
                        AcceptanceEvidenceNeed::EffectiveWorkspaceMutation,
                    )
                } else if let Some((outcome, observation)) =
                    latest_bound_observation(snapshot, verifier)
                {
                    match observation.verdict {
                        VerifierVerdict::Passed if outcome.is_success() => evidence_needed(
                            id,
                            AcceptanceProgressReason::VerifierPassObserved,
                            AcceptanceEvidenceNeed::HostCompletionReview,
                        ),
                        VerifierVerdict::Failed if canonical_failed_observation(outcome) => {
                            evidence_needed(
                                id,
                                AcceptanceProgressReason::VerifierFailureObserved,
                                AcceptanceEvidenceNeed::EffectiveWorkspaceMutation,
                            )
                        }
                        VerifierVerdict::Passed
                        | VerifierVerdict::Failed
                        | VerifierVerdict::Partial => evidence_needed(
                            id,
                            AcceptanceProgressReason::VerifierIncomplete,
                            AcceptanceEvidenceNeed::HostVerifierExecutionRepair,
                        ),
                    }
                } else if let Some(receipt) =
                    stale_receipt(snapshot, contract, id, *evidence_policy, verifier)
                {
                    invalidated(
                        id,
                        receipt,
                        match evidence_policy {
                            VerifierEvidencePolicy::LatestPass => {
                                AcceptanceEvidenceNeed::LatestHostVerifierPass
                            }
                            VerifierEvidencePolicy::FailedWritePass => {
                                AcceptanceEvidenceNeed::FailedVerifierObservation
                            }
                        },
                    )
                } else {
                    pending(
                        id,
                        AcceptanceProgressReason::VerifierPending,
                        match evidence_policy {
                            VerifierEvidencePolicy::LatestPass => {
                                AcceptanceEvidenceNeed::LatestHostVerifierPass
                            }
                            VerifierEvidencePolicy::FailedWritePass => {
                                AcceptanceEvidenceNeed::FailedVerifierObservation
                            }
                        },
                    )
                }
            }
        })
        .collect();
    Some(AcceptanceProgressProjection {
        items,
        include_last_host_verifier_failure,
    })
}

fn current_receipt<'a>(
    snapshot: &'a RunSnapshot,
    contract: &TaskContract,
    acceptance_id: &AcceptanceId,
    policy: VerifierEvidencePolicy,
    verifier: &VerifierSpec,
) -> Option<&'a EvidenceReceipt> {
    snapshot.evidence_receipts.iter().rev().find(|receipt| {
        matching_receipt(receipt, contract, acceptance_id, policy, verifier)
            && receipt.workspace_state == snapshot.workspace_state
    })
}

fn stale_receipt<'a>(
    snapshot: &'a RunSnapshot,
    contract: &TaskContract,
    acceptance_id: &AcceptanceId,
    policy: VerifierEvidencePolicy,
    verifier: &VerifierSpec,
) -> Option<&'a EvidenceReceipt> {
    snapshot.evidence_receipts.iter().rev().find(|receipt| {
        matching_receipt(receipt, contract, acceptance_id, policy, verifier)
            && receipt.workspace_state != snapshot.workspace_state
    })
}

fn matching_receipt(
    receipt: &EvidenceReceipt,
    contract: &TaskContract,
    acceptance_id: &AcceptanceId,
    policy: VerifierEvidencePolicy,
    verifier: &VerifierSpec,
) -> bool {
    receipt.generation_id == contract.generation_id
        && receipt.acceptance_id == *acceptance_id
        && receipt.verifier == *verifier
        && receipt.lineage.satisfies(policy)
}

fn relevant_rejection<'a>(
    snapshot: &'a RunSnapshot,
    contract: &TaskContract,
    acceptance_id: &AcceptanceId,
) -> Option<&'a CompletionRejection> {
    snapshot
        .last_completion_rejection
        .as_ref()
        .or_else(|| {
            snapshot
                .last_host_verification_failure
                .as_ref()
                .map(|failure| &failure.rejection)
        })
        .filter(|rejection| {
            rejection.generation_id == contract.generation_id
                && rejection.unmet_acceptance_ids.contains(acceptance_id)
        })
}

fn workspace_changed_after_host_failure(
    snapshot: &RunSnapshot,
    rejection: &CompletionRejection,
) -> bool {
    snapshot
        .last_host_verification_failure
        .as_ref()
        .filter(|failure| failure.rejection == *rejection)
        .is_some_and(|failure| {
            failure.workspace_state.generation < snapshot.workspace_state.generation
                && failure.workspace_state.revision != snapshot.workspace_state.revision
        })
}

fn latest_bound_observation<'a>(
    snapshot: &'a RunSnapshot,
    verifier: &VerifierSpec,
) -> Option<(&'a ToolOutcome, &'a VerifierObservation)> {
    snapshot.transcript.entries.iter().rev().find_map(|entry| {
        let TranscriptEntry::Tool { outcome, .. } = entry else {
            return None;
        };
        let outcome = outcome.as_ref();
        let observation = outcome.verifier_observation.as_ref()?;
        (observation.spec == *verifier
            && observation.workspace_revision == snapshot.workspace_state.revision
            && observation_is_bound(outcome, observation, &snapshot.workspace_state.revision))
        .then_some((outcome, observation))
    })
}

fn observation_is_bound(
    outcome: &ToolOutcome,
    observation: &VerifierObservation,
    revision: &WorkspaceRevision,
) -> bool {
    let WorkspaceRevision::Known { sha256 } = revision else {
        return false;
    };
    outcome.workspace_revision.as_deref() == Some(sha256)
        && !observation.artifact_ids.is_empty()
        && outcome.evidence.status == ToolEvidenceStatus::Produced
        && outcome.evidence.references == observation.artifact_ids
}

fn canonical_failed_observation(outcome: &ToolOutcome) -> bool {
    outcome.invocation == ToolInvocationStatus::Accepted
        && outcome.transport == ToolTransportStatus::Succeeded
        && outcome.operation == ToolOperationStatus::Failed
        && outcome.side_effect != ToolSideEffectStatus::Applied
}

fn transition_need(transition: CompletionRequiredTransition) -> AcceptanceEvidenceNeed {
    match transition {
        CompletionRequiredTransition::EffectiveWorkspaceMutation => {
            AcceptanceEvidenceNeed::EffectiveWorkspaceMutation
        }
        CompletionRequiredTransition::EvidenceLineageRepair => {
            AcceptanceEvidenceNeed::EvidenceLineageRepair
        }
        CompletionRequiredTransition::HostVerifierContractRepair => {
            AcceptanceEvidenceNeed::HostVerifierContractRepair
        }
        CompletionRequiredTransition::HostVerifierExecutionRepair => {
            AcceptanceEvidenceNeed::HostVerifierExecutionRepair
        }
    }
}

fn rejection_reason(cause: EvidenceSealRejection) -> AcceptanceProgressReason {
    match cause {
        EvidenceSealRejection::VerifierObservationMissing => {
            AcceptanceProgressReason::VerifierObservationMissing
        }
        EvidenceSealRejection::VerifierSpecMismatch => {
            AcceptanceProgressReason::VerifierSpecMismatch
        }
        EvidenceSealRejection::VerifierWorkspaceUnstable => {
            AcceptanceProgressReason::VerifierWorkspaceUnstable
        }
        EvidenceSealRejection::VerifierArtifactUnavailable => {
            AcceptanceProgressReason::VerifierArtifactUnavailable
        }
        EvidenceSealRejection::VerifierFailed => AcceptanceProgressReason::VerifierFailed,
        EvidenceSealRejection::VerifierIncomplete => AcceptanceProgressReason::VerifierIncomplete,
        EvidenceSealRejection::VerifierOutcomeInconsistent => {
            AcceptanceProgressReason::VerifierOutcomeInconsistent
        }
        EvidenceSealRejection::EvidenceLineageUnavailable => {
            AcceptanceProgressReason::EvidenceLineageUnavailable
        }
        EvidenceSealRejection::EvidenceReceiptInvalid => {
            AcceptanceProgressReason::EvidenceReceiptInvalid
        }
    }
}

fn pending(
    acceptance_id: &AcceptanceId,
    reason: AcceptanceProgressReason,
    next_evidence: AcceptanceEvidenceNeed,
) -> AcceptanceProgressItem {
    AcceptanceProgressItem {
        acceptance_id: acceptance_id.0.clone(),
        status: AcceptanceProgressStatus::Pending,
        reason,
        next_evidence: Some(next_evidence),
        receipt_id: None,
        evidence_workspace_generation: None,
        evidence_workspace_revision: None,
    }
}

fn evidence_needed(
    acceptance_id: &AcceptanceId,
    reason: AcceptanceProgressReason,
    next_evidence: AcceptanceEvidenceNeed,
) -> AcceptanceProgressItem {
    AcceptanceProgressItem {
        status: AcceptanceProgressStatus::EvidenceNeeded,
        ..pending(acceptance_id, reason, next_evidence)
    }
}

fn satisfied(acceptance_id: &AcceptanceId, receipt: &EvidenceReceipt) -> AcceptanceProgressItem {
    receipt_item(
        acceptance_id,
        AcceptanceProgressStatus::Satisfied,
        AcceptanceProgressReason::CurrentReceipt,
        receipt,
        None,
    )
}

fn invalidated(
    acceptance_id: &AcceptanceId,
    receipt: &EvidenceReceipt,
    next_evidence: AcceptanceEvidenceNeed,
) -> AcceptanceProgressItem {
    receipt_item(
        acceptance_id,
        AcceptanceProgressStatus::Invalidated,
        AcceptanceProgressReason::StaleReceipt,
        receipt,
        Some(next_evidence),
    )
}

fn receipt_item(
    acceptance_id: &AcceptanceId,
    status: AcceptanceProgressStatus,
    reason: AcceptanceProgressReason,
    receipt: &EvidenceReceipt,
    next_evidence: Option<AcceptanceEvidenceNeed>,
) -> AcceptanceProgressItem {
    let WorkspaceRevision::Known { sha256 } = &receipt.workspace_state.revision else {
        unreachable!("RunStore rejects EvidenceReceipt with an unknown revision")
    };
    AcceptanceProgressItem {
        acceptance_id: acceptance_id.0.clone(),
        status,
        reason,
        next_evidence,
        receipt_id: Some(receipt.id.0.clone()),
        evidence_workspace_generation: Some(receipt.workspace_state.generation),
        evidence_workspace_revision: Some(sha256.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use codewhale_protocol::agent_runtime::{
        AGENT_RUNTIME_EVENT_SCHEMA_VERSION, HostVerificationFailure, RunId, RunRequest,
        RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent, TemporalEvidenceProgress,
        ToolArtifact, ToolEvidence, VerificationArtifactPayload,
    };
    use codewhale_protocol::task::{
        CompletionCandidateId, EvidenceLineage, EvidenceReceiptId, FailedVerifierEvidence,
        FailedVerifierSource, TaskDefinition, TaskGenerationId, VerificationId, VerifierPlan,
        VerifierStep, WorkspaceMutationEvidence, WorkspaceState,
    };

    use super::*;
    use crate::reduce_events;

    fn known(generation: u64, revision: &str) -> WorkspaceState {
        WorkspaceState {
            generation,
            revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
        }
    }

    fn verifier() -> VerifierSpec {
        VerifierSpec {
            verifier_id: "tests".to_owned(),
            parameters: serde_json::json!({"profile": "quick"}),
            plan: VerifierPlan {
                steps: vec![VerifierStep {
                    id: "tests".to_owned(),
                    program: "cargo".to_owned(),
                    args: vec!["test".to_owned(), "--locked".to_owned()],
                    cwd: String::new(),
                    env: BTreeMap::new(),
                    timeout_ms: 30_000,
                }],
            },
        }
    }

    fn snapshot(policy: VerifierEvidencePolicy) -> RunSnapshot {
        let run_id = RunId::from("acceptance-progress-run");
        let request = RunRequest::new(
            TaskContract {
                generation_id: TaskGenerationId::from(run_id.0.clone()),
                definition: TaskDefinition {
                    objective: "修复并验证".to_owned(),
                    constraints: Vec::new(),
                    non_goals: Vec::new(),
                    acceptance: vec![
                        TaskAcceptance::Host {
                            id: AcceptanceId::from("host"),
                            description: "Host 接受完成".to_owned(),
                        },
                        TaskAcceptance::Verifier {
                            id: AcceptanceId::from("tests"),
                            description: "冻结测试通过".to_owned(),
                            evidence_policy: policy,
                            verifier: verifier(),
                        },
                    ],
                },
            },
            "system",
        );
        let event = StoredRuntimeEvent {
            schema_version: AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id,
            parent_run_id: None,
            event_id: RuntimeEventId("created".to_owned()),
            sequence: 1,
            occurred_at_unix_ms: 1,
            event: RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        };
        let mut snapshot = reduce_events(&[event]).expect("base snapshot");
        snapshot.workspace_state = known(1, "sha256:one");
        snapshot
    }

    fn receipt(snapshot: &RunSnapshot, revision: WorkspaceState) -> EvidenceReceipt {
        EvidenceReceipt {
            id: EvidenceReceiptId::from(format!("receipt:{}", revision.generation)),
            generation_id: snapshot
                .request
                .task_contract
                .as_ref()
                .expect("contract")
                .generation_id
                .clone(),
            acceptance_id: AcceptanceId::from("tests"),
            verification_id: VerificationId::from(format!("verification:{}", revision.generation)),
            verifier: verifier(),
            workspace_state: revision,
            artifact_ids: vec!["artifact:tests".to_owned()],
            lineage: EvidenceLineage::LatestPass,
        }
    }

    fn rejection(snapshot: &RunSnapshot, cause: EvidenceSealRejection) -> CompletionRejection {
        CompletionRejection {
            candidate_id: CompletionCandidateId::from("candidate"),
            generation_id: snapshot
                .request
                .task_contract
                .as_ref()
                .expect("contract")
                .generation_id
                .clone(),
            unmet_acceptance_ids: vec![AcceptanceId::from("tests")],
            cause,
            required_transition: cause.required_transition(),
            reason: "typed rejection".to_owned(),
        }
    }

    fn verifier_outcome(verdict: VerifierVerdict, revision: &str) -> ToolOutcome {
        let payload = VerificationArtifactPayload {
            summary: "deterministic verifier".to_owned(),
            verifier: verifier(),
            verdict,
            workspace_revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
        };
        let artifact = ToolArtifact::inline_verification(payload);
        let artifact_id = artifact.id.clone();
        let mut outcome = match verdict {
            VerifierVerdict::Passed => ToolOutcome::success("passed"),
            VerifierVerdict::Failed | VerifierVerdict::Partial => ToolOutcome::error("failed"),
        };
        outcome.side_effect = ToolSideEffectStatus::NotApplied;
        outcome.workspace_revision = Some(revision.to_owned());
        outcome.evidence = ToolEvidence {
            status: ToolEvidenceStatus::Produced,
            references: vec![artifact_id.clone()],
        };
        outcome.artifacts = vec![artifact];
        outcome.verifier_observation = Some(VerifierObservation {
            spec: verifier(),
            verdict,
            workspace_revision: WorkspaceRevision::Known {
                sha256: revision.to_owned(),
            },
            artifact_ids: vec![artifact_id],
        });
        outcome
    }

    #[test]
    fn host_and_verifier_begin_pending_in_contract_order() {
        let snapshot = snapshot(VerifierEvidencePolicy::LatestPass);
        let projection = derive_acceptance_progress(&snapshot).expect("projection");
        assert_eq!(
            projection.items,
            vec![
                pending(
                    &AcceptanceId::from("host"),
                    AcceptanceProgressReason::HostReviewPending,
                    AcceptanceEvidenceNeed::HostCompletionReview,
                ),
                pending(
                    &AcceptanceId::from("tests"),
                    AcceptanceProgressReason::VerifierPending,
                    AcceptanceEvidenceNeed::LatestHostVerifierPass,
                ),
            ]
        );
        assert!(!projection.include_last_host_verifier_failure);
    }

    #[test]
    fn current_receipt_satisfies_and_workspace_change_invalidates() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::LatestPass);
        snapshot
            .evidence_receipts
            .push(receipt(&snapshot, snapshot.workspace_state.clone()));
        let current = derive_acceptance_progress(&snapshot).expect("current projection");
        assert_eq!(current.items[1].status, AcceptanceProgressStatus::Satisfied);
        assert_eq!(
            current.items[1].reason,
            AcceptanceProgressReason::CurrentReceipt
        );

        snapshot.workspace_state = known(2, "sha256:two");
        let invalidated = derive_acceptance_progress(&snapshot).expect("invalidated projection");
        assert_eq!(
            invalidated.items[1].status,
            AcceptanceProgressStatus::Invalidated
        );
        assert_eq!(
            invalidated.items[1].next_evidence,
            Some(AcceptanceEvidenceNeed::LatestHostVerifierPass)
        );
        assert_eq!(
            invalidated.items[1].evidence_workspace_revision.as_deref(),
            Some("sha256:one")
        );
    }

    #[test]
    fn current_rejection_exposes_only_the_typed_next_evidence() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::FailedWritePass);
        let rejection = rejection(&snapshot, EvidenceSealRejection::VerifierFailed);
        snapshot.last_completion_rejection = Some(rejection.clone());
        snapshot.last_host_verification_failure = Some(HostVerificationFailure {
            outcome: ToolOutcome::error("failed"),
            workspace_state: snapshot.workspace_state.clone(),
            rejection,
        });
        let projection = derive_acceptance_progress(&snapshot).expect("rejected projection");
        assert_eq!(
            projection.items[1].status,
            AcceptanceProgressStatus::EvidenceNeeded
        );
        assert_eq!(
            projection.items[1].reason,
            AcceptanceProgressReason::VerifierFailed
        );
        assert_eq!(
            projection.items[1].next_evidence,
            Some(AcceptanceEvidenceNeed::EffectiveWorkspaceMutation)
        );
        assert!(projection.include_last_host_verifier_failure);
    }

    #[test]
    fn workspace_change_after_host_failure_requests_policy_correct_evidence() {
        for (policy, expected_reason, expected_next) in [
            (
                VerifierEvidencePolicy::LatestPass,
                AcceptanceProgressReason::WorkspaceMutationObserved,
                AcceptanceEvidenceNeed::LatestHostVerifierPass,
            ),
            (
                VerifierEvidencePolicy::FailedWritePass,
                AcceptanceProgressReason::EvidenceLineageUnavailable,
                AcceptanceEvidenceNeed::FailedVerifierObservation,
            ),
        ] {
            let mut snapshot = snapshot(policy);
            let rejection = rejection(&snapshot, EvidenceSealRejection::VerifierFailed);
            snapshot.last_completion_rejection = Some(rejection.clone());
            snapshot.last_host_verification_failure = Some(HostVerificationFailure {
                outcome: ToolOutcome::error("failed"),
                workspace_state: snapshot.workspace_state.clone(),
                rejection,
            });
            snapshot.workspace_state = known(2, "sha256:fixed");

            let projection =
                derive_acceptance_progress(&snapshot).expect("post-mutation projection");
            assert_eq!(projection.items[1].reason, expected_reason);
            assert_eq!(projection.items[1].next_evidence, Some(expected_next));
            assert!(!projection.include_last_host_verifier_failure);
        }
    }

    #[test]
    fn temporal_failure_and_mutation_advance_without_a_plan_store() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::FailedWritePass);
        let failure_state = snapshot.workspace_state.clone();
        snapshot.temporal_evidence_progress = Some(TemporalEvidenceProgress {
            acceptance_id: AcceptanceId::from("tests"),
            verifier: verifier(),
            failure: FailedVerifierEvidence {
                source: FailedVerifierSource::Tool {
                    operation_id: "verify-failed".to_owned(),
                },
                workspace_state: failure_state.clone(),
                artifact_ids: vec!["artifact:failed".to_owned()],
            },
            mutation: None,
        });
        let failed = derive_acceptance_progress(&snapshot).expect("failed projection");
        assert_eq!(
            failed.items[1].reason,
            AcceptanceProgressReason::TemporalFailureObserved
        );

        let fixed = known(2, "sha256:fixed");
        snapshot.workspace_state = fixed.clone();
        snapshot
            .temporal_evidence_progress
            .as_mut()
            .expect("progress")
            .mutation = Some(WorkspaceMutationEvidence {
            operation_id: "write-fix".to_owned(),
            workspace_state_before: failure_state,
            workspace_state_after: fixed,
        });
        let mutated = derive_acceptance_progress(&snapshot).expect("mutated projection");
        assert_eq!(
            mutated.items[1].reason,
            AcceptanceProgressReason::TemporalMutationObserved
        );
        assert_eq!(
            mutated.items[1].next_evidence,
            Some(AcceptanceEvidenceNeed::LatestHostVerifierPass)
        );
    }

    #[test]
    fn latest_revision_tool_observation_changes_only_the_derived_view() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::LatestPass);
        snapshot.transcript.entries.push(TranscriptEntry::Tool {
            call_id: "verify".to_owned(),
            name: "run_verifiers".to_owned(),
            outcome: Box::new(verifier_outcome(VerifierVerdict::Passed, "sha256:one")),
        });
        let before = serde_json::to_vec(&snapshot).expect("snapshot bytes");
        let first = derive_acceptance_progress(&snapshot).expect("first projection");
        let second = derive_acceptance_progress(&snapshot).expect("second projection");
        assert_eq!(first, second);
        assert_eq!(
            first.items[1].reason,
            AcceptanceProgressReason::VerifierPassObserved
        );
        assert_eq!(
            first.items[1].next_evidence,
            Some(AcceptanceEvidenceNeed::HostCompletionReview)
        );
        assert_eq!(
            serde_json::to_vec(&snapshot).expect("snapshot bytes after"),
            before,
            "derivation must not persist or mutate a second progress truth"
        );
    }

    #[test]
    fn old_generation_rejection_is_not_inherited_as_current_progress() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::LatestPass);
        let mut old = rejection(&snapshot, EvidenceSealRejection::VerifierFailed);
        old.generation_id = TaskGenerationId::from("old-generation");
        snapshot.last_completion_rejection = Some(old);
        let projection = derive_acceptance_progress(&snapshot).expect("projection");
        assert_eq!(
            projection.items[1].reason,
            AcceptanceProgressReason::VerifierPending
        );
        assert!(!projection.include_last_host_verifier_failure);
    }

    #[test]
    fn stale_failed_write_pass_receipt_requires_a_new_failure_chain() {
        let mut snapshot = snapshot(VerifierEvidencePolicy::FailedWritePass);
        let mut stale = receipt(&snapshot, snapshot.workspace_state.clone());
        stale.lineage = EvidenceLineage::FailedWritePass {
            failure: FailedVerifierEvidence {
                source: FailedVerifierSource::Tool {
                    operation_id: "old-failure".to_owned(),
                },
                workspace_state: known(0, "sha256:broken"),
                artifact_ids: vec!["artifact:failure".to_owned()],
            },
            mutation: WorkspaceMutationEvidence {
                operation_id: "old-write".to_owned(),
                workspace_state_before: known(0, "sha256:broken"),
                workspace_state_after: known(1, "sha256:one"),
            },
        };
        snapshot.evidence_receipts.push(stale);
        snapshot.workspace_state = known(2, "sha256:later");
        let projection = derive_acceptance_progress(&snapshot).expect("projection");
        assert_eq!(
            projection.items[1].status,
            AcceptanceProgressStatus::Invalidated
        );
        assert_eq!(
            projection.items[1].next_evidence,
            Some(AcceptanceEvidenceNeed::FailedVerifierObservation)
        );
    }
}
