//! Pure projection of canonical runtime events into TUI-facing effects.
//!
//! The canonical event remains the complete payload. This reducer adds only
//! the presentation boundary that is not itself an event: when user input
//! becomes transcript-visible. It owns no I/O, persistence, runtime loop, or
//! legacy `EngineEvent` conversion.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;

use codewhale_protocol::agent_runtime::{
    CommandId, InteractionId, RunId, RuntimeEventId, RuntimeEventKind, StoredRuntimeEvent,
};

/// One effect produced from one canonical stored event.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionEffect {
    pub run_id: RunId,
    pub sequence: u64,
    pub event_id: RuntimeEventId,
    pub kind: ProjectionEffectKind,
}

/// Minimal TUI effect surface.
///
/// `Canonical` deliberately carries the exact protocol event instead of a
/// mirrored TUI DTO. `UserTranscript` is emitted once for `RunCreated` and only
/// at `SteerApplied`, never at `SteerQueued`.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectionEffectKind {
    Canonical(Box<StoredRuntimeEvent>),
    UserTranscript {
        source: UserTranscriptSource,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserTranscriptSource {
    RunCreated,
    SteerApplied { command_id: CommandId },
}

/// Stateful cursor for projecting any number of independent run streams.
///
/// The state is limited to replay identity plus the interaction and steer
/// identities needed by the presentation contract. It is not a second runtime
/// snapshot.
#[derive(Debug, Default)]
pub struct CanonicalRunProjection {
    runs: HashMap<RunId, RunProjectionState>,
}

impl CanonicalRunProjection {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one exact stored runtime event.
    ///
    /// Exact replay is a no-op. Conflicting identities, sequence gaps, stale
    /// interaction resolutions, steer FIFO violations, and events after the
    /// canonical terminal are rejected without advancing the cursor.
    pub fn apply(
        &mut self,
        stored: StoredRuntimeEvent,
    ) -> Result<Vec<ProjectionEffect>, ProjectionError> {
        let run_id = stored.run_id.clone();
        let event_id = stored.event_id.clone();
        let sequence = stored.sequence;
        let event = &stored.event;
        let state = self.runs.entry(run_id.clone()).or_default();
        if state.classify(&stored)? == EventDisposition::Replay {
            return Ok(Vec::new());
        }
        state.validate_and_update(&run_id, sequence, event)?;
        state.record(stored.clone());

        let transcript = match event {
            RuntimeEventKind::RunCreated { request }
                if request.purpose == codewhale_protocol::agent_runtime::RunPurpose::Agent =>
            {
                Some((UserTranscriptSource::RunCreated, request.input.clone()))
            }
            RuntimeEventKind::SteerApplied {
                command_id,
                content,
            } => Some((
                UserTranscriptSource::SteerApplied {
                    command_id: command_id.clone(),
                },
                content.clone(),
            )),
            _ => None,
        };
        let mut effects = vec![ProjectionEffect {
            run_id: run_id.clone(),
            sequence,
            event_id: event_id.clone(),
            kind: ProjectionEffectKind::Canonical(Box::new(stored)),
        }];
        if let Some((source, content)) = transcript {
            effects.push(ProjectionEffect {
                run_id,
                sequence,
                event_id,
                kind: ProjectionEffectKind::UserTranscript { source, content },
            });
        }
        Ok(effects)
    }

    #[must_use]
    pub fn last_sequence(&self, run_id: &RunId) -> Option<u64> {
        self.runs.get(run_id).map(|state| state.last_sequence)
    }
}

#[derive(Debug, Default)]
struct RunProjectionState {
    last_sequence: u64,
    stored_events: HashMap<u64, StoredRuntimeEvent>,
    event_id_sequences: HashMap<String, u64>,
    pending_steers: VecDeque<PendingSteer>,
    pending_interaction: Option<InteractionId>,
    terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingSteer {
    command_id: CommandId,
    content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventDisposition {
    New,
    Replay,
}

impl RunProjectionState {
    fn classify(&self, stored: &StoredRuntimeEvent) -> Result<EventDisposition, ProjectionError> {
        let run_id = &stored.run_id;
        let sequence = stored.sequence;
        let event_id = &stored.event_id;
        if let Some(existing) = self.stored_events.get(&sequence) {
            return if existing == stored {
                Ok(EventDisposition::Replay)
            } else {
                Err(ProjectionError::SequenceConflict {
                    run_id: run_id.clone(),
                    sequence,
                    existing_event_id: existing.event_id.0.clone(),
                    incoming_event_id: event_id.0.clone(),
                })
            };
        }
        if let Some(existing_sequence) = self.event_id_sequences.get(&event_id.0) {
            return Err(ProjectionError::EventIdConflict {
                run_id: run_id.clone(),
                event_id: event_id.0.clone(),
                existing_sequence: *existing_sequence,
                incoming_sequence: sequence,
            });
        }
        let expected = self.last_sequence.saturating_add(1);
        if sequence != expected {
            return Err(ProjectionError::SequenceGap {
                run_id: run_id.clone(),
                expected,
                actual: sequence,
            });
        }
        if self.terminal {
            return Err(ProjectionError::EventAfterTerminal {
                run_id: run_id.clone(),
                sequence,
            });
        }
        Ok(EventDisposition::New)
    }

    fn record(&mut self, stored: StoredRuntimeEvent) {
        self.last_sequence = stored.sequence;
        self.event_id_sequences
            .insert(stored.event_id.0.clone(), stored.sequence);
        self.stored_events.insert(stored.sequence, stored);
    }

    fn validate_and_update(
        &mut self,
        run_id: &RunId,
        sequence: u64,
        event: &RuntimeEventKind,
    ) -> Result<(), ProjectionError> {
        if sequence == 1 && !matches!(event, RuntimeEventKind::RunCreated { .. }) {
            return Err(ProjectionError::FirstEventNotRunCreated {
                run_id: run_id.clone(),
            });
        }
        if sequence > 1 && matches!(event, RuntimeEventKind::RunCreated { .. }) {
            return Err(ProjectionError::RunCreatedAfterStart {
                run_id: run_id.clone(),
                sequence,
            });
        }

        match event {
            RuntimeEventKind::RunCreated { request }
                if request.run_id.as_ref().is_some_and(|id| id != run_id) =>
            {
                Err(ProjectionError::RunIdentityMismatch {
                    stored_run_id: run_id.clone(),
                    request_run_id: request.run_id.clone(),
                })
            }
            RuntimeEventKind::InteractionRequested { request } => {
                if let Some(pending) = &self.pending_interaction {
                    return Err(ProjectionError::InteractionAlreadyPending {
                        run_id: run_id.clone(),
                        pending_interaction_id: pending.clone(),
                        incoming_interaction_id: request.interaction_id.clone(),
                    });
                }
                self.pending_interaction = Some(request.interaction_id.clone());
                Ok(())
            }
            RuntimeEventKind::InteractionResolved { interaction_id, .. } => {
                let Some(pending) = self.pending_interaction.as_ref() else {
                    return Err(ProjectionError::InteractionNotPending {
                        run_id: run_id.clone(),
                        interaction_id: interaction_id.clone(),
                    });
                };
                if pending != interaction_id {
                    return Err(ProjectionError::InteractionMismatch {
                        run_id: run_id.clone(),
                        pending_interaction_id: pending.clone(),
                        resolved_interaction_id: interaction_id.clone(),
                    });
                }
                self.pending_interaction = None;
                Ok(())
            }
            RuntimeEventKind::SteerQueued {
                command_id,
                content,
            } => {
                self.pending_steers.push_back(PendingSteer {
                    command_id: command_id.clone(),
                    content: content.clone(),
                });
                Ok(())
            }
            RuntimeEventKind::SteerApplied {
                command_id,
                content,
            } => {
                let Some(pending) = self.pending_steers.front() else {
                    return Err(ProjectionError::SteerNotQueued {
                        run_id: run_id.clone(),
                        command_id: command_id.clone(),
                    });
                };
                if pending.command_id != *command_id || pending.content != *content {
                    return Err(ProjectionError::SteerFifoMismatch {
                        run_id: run_id.clone(),
                        expected_command_id: pending.command_id.clone(),
                        incoming_command_id: command_id.clone(),
                    });
                }
                self.pending_steers.pop_front();
                Ok(())
            }
            RuntimeEventKind::Terminal { .. } => {
                self.terminal = true;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    SequenceGap {
        run_id: RunId,
        expected: u64,
        actual: u64,
    },
    SequenceConflict {
        run_id: RunId,
        sequence: u64,
        existing_event_id: String,
        incoming_event_id: String,
    },
    EventIdConflict {
        run_id: RunId,
        event_id: String,
        existing_sequence: u64,
        incoming_sequence: u64,
    },
    FirstEventNotRunCreated {
        run_id: RunId,
    },
    RunCreatedAfterStart {
        run_id: RunId,
        sequence: u64,
    },
    RunIdentityMismatch {
        stored_run_id: RunId,
        request_run_id: Option<RunId>,
    },
    InteractionAlreadyPending {
        run_id: RunId,
        pending_interaction_id: InteractionId,
        incoming_interaction_id: InteractionId,
    },
    InteractionNotPending {
        run_id: RunId,
        interaction_id: InteractionId,
    },
    InteractionMismatch {
        run_id: RunId,
        pending_interaction_id: InteractionId,
        resolved_interaction_id: InteractionId,
    },
    SteerNotQueued {
        run_id: RunId,
        command_id: CommandId,
    },
    SteerFifoMismatch {
        run_id: RunId,
        expected_command_id: CommandId,
        incoming_command_id: CommandId,
    },
    EventAfterTerminal {
        run_id: RunId,
        sequence: u64,
    },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProjectionError {}

#[cfg(test)]
mod tests {
    use codewhale_protocol::agent_runtime::{
        AgentOutcome, ApprovalRisk, AttemptId, DurableControlAction, ModelAccounting,
        ModelFinishReason, ModelOutput, OperationId, RunRequest, TerminalState, ToolApprovalPrompt,
        ToolArguments, ToolInvocation, ToolOutcome, Usage, UserInteractionPrompt,
        UserInteractionRequest, UserInteractionResponse,
    };

    use super::*;

    fn stored(
        run_id: &RunId,
        sequence: u64,
        event_id: &str,
        event: RuntimeEventKind,
    ) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: 5,
            run_id: run_id.clone(),
            parent_run_id: None,
            event_id: RuntimeEventId(event_id.to_owned()),
            sequence,
            occurred_at_unix_ms: sequence,
            event,
        }
    }

    fn created(run_id: &RunId, input: &str) -> StoredRuntimeEvent {
        let mut request = RunRequest::new(input, "系统提示");
        request.run_id = Some(run_id.clone());
        stored(
            run_id,
            1,
            "run_created",
            RuntimeEventKind::RunCreated {
                request: Box::new(request),
            },
        )
    }

    fn model_output(content: &str) -> ModelOutput {
        ModelOutput {
            content: content.to_owned(),
            reasoning_content: Some("推理".to_owned()),
            tool_calls: Vec::new(),
            finish_reason: ModelFinishReason::Stop,
            usage: Usage::default(),
        }
    }

    fn outcome(run_id: &RunId, terminal: TerminalState) -> AgentOutcome {
        AgentOutcome {
            run_id: run_id.clone(),
            parent_run_id: None,
            terminal,
            accounting: ModelAccounting::default(),
            runtime_model_requests: 1,
            runtime_retries: 0,
            tool_calls: 0,
        }
    }

    fn canonical(effects: &[ProjectionEffect]) -> &StoredRuntimeEvent {
        let ProjectionEffectKind::Canonical(stored) = &effects[0].kind else {
            panic!("first effect must preserve the canonical event");
        };
        stored
    }

    #[test]
    fn fresh_projection_and_full_replay_are_idempotent() {
        let run_id = RunId::from("run");
        let events = vec![
            created(&run_id, "修复问题"),
            stored(
                &run_id,
                2,
                "content",
                RuntimeEventKind::ContentDelta {
                    attempt_id: AttemptId("attempt".to_owned()),
                    index: 0,
                    delta: "完成".to_owned(),
                },
            ),
            stored(
                &run_id,
                3,
                "reasoning",
                RuntimeEventKind::ReasoningDelta {
                    attempt_id: AttemptId("attempt".to_owned()),
                    index: 0,
                    delta: "先检查".to_owned(),
                },
            ),
            stored(
                &run_id,
                4,
                "model-committed",
                RuntimeEventKind::ModelResponseCommitted {
                    attempt_id: AttemptId("attempt".to_owned()),
                    output: Box::new(model_output("完成")),
                    accounting: Box::new(ModelAccounting::default()),
                },
            ),
        ];
        let mut projection = CanonicalRunProjection::new();
        let fresh = events
            .iter()
            .cloned()
            .flat_map(|event| projection.apply(event).expect("fresh projection"))
            .collect::<Vec<_>>();
        assert_eq!(
            fresh
                .iter()
                .filter(|effect| matches!(
                    effect.kind,
                    ProjectionEffectKind::UserTranscript {
                        source: UserTranscriptSource::RunCreated,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(fresh.iter().any(|effect| {
            let ProjectionEffectKind::Canonical(stored) = &effect.kind else {
                return false;
            };
            matches!(
                stored.event,
                RuntimeEventKind::ModelResponseCommitted { .. }
            )
        }));

        for event in events {
            assert!(projection.apply(event).expect("exact replay").is_empty());
        }
        assert_eq!(projection.last_sequence(&run_id), Some(4));
    }

    #[test]
    fn conflicting_sequence_or_event_id_never_advances_the_cursor() {
        let run_id = RunId::from("run");
        let mut projection = CanonicalRunProjection::new();
        projection.apply(created(&run_id, "任务")).unwrap();
        let content = || RuntimeEventKind::ContentDelta {
            attempt_id: AttemptId("attempt".to_owned()),
            index: 0,
            delta: "x".to_owned(),
        };
        assert!(matches!(
            projection
                .apply(stored(&run_id, 1, "other-id", content()))
                .unwrap_err(),
            ProjectionError::SequenceConflict { sequence: 1, .. }
        ));
        let mut changed_payload = created(&run_id, "另一份任务");
        changed_payload.event_id = RuntimeEventId("run_created".to_owned());
        assert!(matches!(
            projection.apply(changed_payload).unwrap_err(),
            ProjectionError::SequenceConflict { sequence: 1, .. }
        ));
        assert!(matches!(
            projection
                .apply(stored(&run_id, 2, "run_created", content()))
                .unwrap_err(),
            ProjectionError::EventIdConflict {
                existing_sequence: 1,
                incoming_sequence: 2,
                ..
            }
        ));
        assert_eq!(projection.last_sequence(&run_id), Some(1));
    }

    #[test]
    fn steer_is_fifo_and_only_applied_content_enters_the_transcript() {
        let run_id = RunId::from("run");
        let mut projection = CanonicalRunProjection::new();
        projection.apply(created(&run_id, "任务")).unwrap();
        let first = CommandId::from("steer-a");
        let second = CommandId::from("steer-b");
        for (sequence, command_id, content) in
            [(2, first.clone(), "先 A"), (3, second.clone(), "再 B")]
        {
            let effects = projection
                .apply(stored(
                    &run_id,
                    sequence,
                    &format!("queued-{sequence}"),
                    RuntimeEventKind::SteerQueued {
                        command_id,
                        content: content.to_owned(),
                    },
                ))
                .unwrap();
            assert_eq!(effects.len(), 1, "queued steer must not enter transcript");
        }
        assert!(matches!(
            projection
                .apply(stored(
                    &run_id,
                    4,
                    "early-b",
                    RuntimeEventKind::SteerApplied {
                        command_id: second.clone(),
                        content: "再 B".to_owned(),
                    },
                ))
                .unwrap_err(),
            ProjectionError::SteerFifoMismatch { .. }
        ));
        assert_eq!(projection.last_sequence(&run_id), Some(3));

        for (sequence, command_id, content) in [(4, first, "先 A"), (5, second, "再 B")] {
            let effects = projection
                .apply(stored(
                    &run_id,
                    sequence,
                    &format!("applied-{sequence}"),
                    RuntimeEventKind::SteerApplied {
                        command_id,
                        content: content.to_owned(),
                    },
                ))
                .unwrap();
            assert_eq!(effects.len(), 2);
            assert!(matches!(
                &effects[1].kind,
                ProjectionEffectKind::UserTranscript {
                    source: UserTranscriptSource::SteerApplied { .. },
                    content: projected,
                } if projected == content
            ));
        }
    }

    #[test]
    fn interaction_identity_survives_replay_and_rejects_stale_resolution() {
        let run_id = RunId::from("run");
        let interaction_id = InteractionId::from("interaction");
        let request = UserInteractionRequest {
            interaction_id: interaction_id.clone(),
            operation_id: OperationId::from("operation"),
            call_id: "call".to_owned(),
            tool_name: "apply_patch".to_owned(),
            prompt: UserInteractionPrompt::Approval {
                prompt: ToolApprovalPrompt {
                    title: "确认修改".to_owned(),
                    description: "将修改文件".to_owned(),
                    risk: ApprovalRisk::Elevated,
                },
                arguments: ToolArguments::parse(r#"{"patch":"*** Begin Patch"}"#)
                    .parsed
                    .expect("approval arguments"),
            },
        };
        let requested = stored(
            &run_id,
            2,
            "interaction-requested",
            RuntimeEventKind::InteractionRequested { request },
        );
        let mut projection = CanonicalRunProjection::new();
        projection.apply(created(&run_id, "任务")).unwrap();
        projection.apply(requested.clone()).unwrap();
        assert!(projection.apply(requested).unwrap().is_empty());
        assert!(matches!(
            projection
                .apply(stored(
                    &run_id,
                    3,
                    "stale-resolution",
                    RuntimeEventKind::InteractionResolved {
                        command_id: CommandId::from("resolve"),
                        interaction_id: InteractionId::from("stale"),
                        response: UserInteractionResponse::Approved,
                    },
                ))
                .unwrap_err(),
            ProjectionError::InteractionMismatch { .. }
        ));
        let effects = projection
            .apply(stored(
                &run_id,
                3,
                "interaction-resolved",
                RuntimeEventKind::InteractionResolved {
                    command_id: CommandId::from("resolve"),
                    interaction_id,
                    response: UserInteractionResponse::Approved,
                },
            ))
            .unwrap();
        assert!(matches!(
            &canonical(&effects).event,
            RuntimeEventKind::InteractionResolved { .. }
        ));
    }

    #[test]
    fn required_tool_child_and_control_events_keep_exact_payloads() {
        let run_id = RunId::from("run");
        let child_run_id = RunId::from("child");
        let invocation = ToolInvocation {
            run_id: run_id.clone(),
            call_id: "tool-call".to_owned(),
            name: "read_file".to_owned(),
            arguments: ToolArguments::parse(r#"{"path":"src/lib.rs"}"#),
        };
        let child_outcome = outcome(
            &child_run_id,
            TerminalState::Completed {
                message: "完成".to_owned(),
            },
        );
        let events = vec![
            RuntimeEventKind::ToolPrepared {
                operation_id: OperationId::from("operation"),
                invocation,
            },
            RuntimeEventKind::ToolExecutionStarted {
                operation_id: OperationId::from("operation"),
            },
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id: OperationId::from("operation"),
                call_id: "tool-call".to_owned(),
                name: "read_file".to_owned(),
                outcome: ToolOutcome::success("内容"),
            },
            RuntimeEventKind::ChildStarted {
                call_id: "agent-call".to_owned(),
                child_run_id,
                depth: 1,
            },
            RuntimeEventKind::ChildFinished {
                call_id: "agent-call".to_owned(),
                outcome: Box::new(child_outcome),
                accounting: Box::new(ModelAccounting::default()),
                handoff_content: "证据".to_owned(),
            },
            RuntimeEventKind::ControlRequested {
                command_id: CommandId::from("interrupt"),
                action: DurableControlAction::Interrupt,
            },
        ];
        let mut projection = CanonicalRunProjection::new();
        projection.apply(created(&run_id, "任务")).unwrap();
        for (offset, expected) in events.into_iter().enumerate() {
            let sequence = u64::try_from(offset).unwrap() + 2;
            let effects = projection
                .apply(stored(
                    &run_id,
                    sequence,
                    &format!("event-{sequence}"),
                    expected.clone(),
                ))
                .unwrap();
            assert_eq!(&canonical(&effects).event, &expected);
        }
    }

    #[test]
    fn terminal_is_authoritative_and_blocks_new_events_but_not_exact_replay() {
        let run_id = RunId::from("run");
        let mut projection = CanonicalRunProjection::new();
        projection.apply(created(&run_id, "任务")).unwrap();
        let terminal = stored(
            &run_id,
            2,
            "terminal",
            RuntimeEventKind::Terminal {
                outcome: Box::new(outcome(&run_id, TerminalState::Interrupted)),
            },
        );
        let effects = projection.apply(terminal.clone()).unwrap();
        assert!(matches!(
            &canonical(&effects).event,
            RuntimeEventKind::Terminal { .. }
        ));
        assert!(projection.apply(terminal).unwrap().is_empty());
        assert!(matches!(
            projection
                .apply(stored(
                    &run_id,
                    3,
                    "late-content",
                    RuntimeEventKind::ContentDelta {
                        attempt_id: AttemptId("attempt".to_owned()),
                        index: 1,
                        delta: "不应出现".to_owned(),
                    },
                ))
                .unwrap_err(),
            ProjectionError::EventAfterTerminal { sequence: 3, .. }
        ));
    }
}
