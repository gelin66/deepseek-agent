use std::collections::{BTreeSet, HashSet};
use std::path::Component;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use codewhale_context::compaction::{
    ContextCompactionPreparation, ContextInput, effective_context, prepare_compaction,
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::*;

enum RunLaunch {
    Create(Box<RunRequest>),
    Resume(RunId),
}

struct ResumeAccountingBootstrap {
    expected_sequence: u64,
    accounting: ModelAccounting,
}

struct RunLaunchOptions {
    budget: Option<Arc<RuntimeBudget>>,
    terminal_model_request: Option<ModelRequestPermit>,
    model_accounting_includes_baseline: bool,
    resume_accounting: Option<ResumeAccountingBootstrap>,
}

#[derive(Clone)]
pub struct AgentRuntime {
    model: Arc<dyn ModelPort>,
    tools: Arc<dyn ToolExecutor>,
    sink: Arc<dyn RuntimeEventSink>,
    store: Arc<dyn RunStore>,
    orchestrator: Option<Arc<dyn AgentOrchestrator>>,
}

impl AgentRuntime {
    #[must_use]
    pub fn new(
        model: Arc<dyn ModelPort>,
        tools: Arc<dyn ToolExecutor>,
        sink: Arc<dyn RuntimeEventSink>,
        store: Arc<dyn RunStore>,
    ) -> Self {
        Self {
            model,
            tools,
            sink,
            store,
            orchestrator: None,
        }
    }

    #[must_use]
    pub fn with_orchestrator(mut self, orchestrator: Arc<dyn AgentOrchestrator>) -> Self {
        self.orchestrator = Some(orchestrator);
        self
    }

    #[must_use]
    pub fn store(&self) -> Arc<dyn RunStore> {
        self.store.clone()
    }

    fn with_tools(&self, tools: Arc<dyn ToolExecutor>) -> Arc<Self> {
        Arc::new(Self {
            model: self.model.clone(),
            tools,
            sink: self.sink.clone(),
            store: self.store.clone(),
            orchestrator: self.orchestrator.clone(),
        })
    }

    /// The exact, deterministic catalog sent to the model. The built-in
    /// `agent` definition has one owner here and is never duplicated by a
    /// concrete tool adapter.
    #[must_use]
    pub fn tool_definitions(
        &self,
        policy: &ToolPolicy,
        depth: u8,
        max_depth: u8,
        interactive: bool,
    ) -> Vec<ToolDefinition> {
        if !policy.enabled {
            return Vec::new();
        }
        let mut definitions = self
            .tools
            .definitions()
            .into_iter()
            .filter(|definition| {
                definition.name != AGENT_TOOL_NAME && policy.permits(&definition.name)
            })
            .collect::<Vec<_>>();
        if depth < max_depth && policy.permits(AGENT_TOOL_NAME) {
            definitions.push(agent_tool_definition());
        }
        if depth == 0 && interactive && policy.permits(REQUEST_USER_INPUT_TOOL_NAME) {
            definitions.push(request_user_input_tool_definition());
        }
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        definitions.dedup_by(|left, right| left.name == right.name);
        definitions
    }

    #[must_use]
    pub fn start(self: &Arc<Self>, mut request: RunRequest) -> RuntimeRun {
        request.parent_run_id = None;
        request.actor = AgentActor::default();
        let budget = Arc::new(RuntimeBudget::new(request.limits, 0, 0));
        self.start_inner(request, budget, None, true)
    }

    /// Reopen one canonical run. The persisted request, transcript, counters,
    /// and terminal state win over all caller-local state.
    #[must_use]
    pub fn resume(self: &Arc<Self>, run_id: RunId) -> RuntimeRun {
        self.resume_inner(run_id, None)
    }

    /// Resume one root from a newer cumulative physical-accounting checkpoint.
    ///
    /// A writer child can persist the shared DeepSeek ledger after the root's
    /// latest event. This override affects only the acquired live root epoch;
    /// the next ordinary accounting event remains the sole durable Store write.
    #[must_use]
    pub fn resume_with_accounting_baseline(
        self: &Arc<Self>,
        run_id: RunId,
        expected_sequence: u64,
        accounting: ModelAccounting,
    ) -> RuntimeRun {
        self.resume_inner_with_accounting(
            run_id,
            None,
            Some(ResumeAccountingBootstrap {
                expected_sequence,
                accounting,
            }),
        )
    }

    fn resume_inner(
        self: &Arc<Self>,
        run_id: RunId,
        budget: Option<Arc<RuntimeBudget>>,
    ) -> RuntimeRun {
        self.resume_inner_with_accounting(run_id, budget, None)
    }

    fn resume_inner_with_accounting(
        self: &Arc<Self>,
        run_id: RunId,
        budget: Option<Arc<RuntimeBudget>>,
        accounting: Option<ResumeAccountingBootstrap>,
    ) -> RuntimeRun {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let control = AgentControl { sender };
        let runtime = self.clone();
        let task_run_id = run_id.clone();
        let join = tokio::spawn(Box::pin(runtime.run_launch(
            RunLaunch::Resume(task_run_id),
            RunLaunchOptions {
                budget,
                terminal_model_request: None,
                model_accounting_includes_baseline: false,
                resume_accounting: accounting,
            },
            receiver,
            ready_sender,
        )));
        RuntimeRun {
            run_id,
            control,
            join,
            ready: Some(ready_receiver),
        }
    }

    fn start_inner(
        self: &Arc<Self>,
        mut request: RunRequest,
        budget: Arc<RuntimeBudget>,
        terminal_model_request: Option<ModelRequestPermit>,
        model_accounting_includes_baseline: bool,
    ) -> RuntimeRun {
        if request.deadline_unix_ms.is_none() {
            request.deadline_unix_ms = request
                .limits
                .wall_time_ms
                .map(|duration| now_unix_ms().saturating_add(duration));
        }
        let run_id = request.run_id.take().unwrap_or_default();
        request.run_id = Some(run_id.clone());
        let (sender, receiver) = mpsc::unbounded_channel();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let control = AgentControl { sender };
        let runtime = self.clone();
        let join = tokio::spawn(Box::pin(runtime.run_launch(
            RunLaunch::Create(Box::new(request)),
            RunLaunchOptions {
                budget: Some(budget),
                terminal_model_request,
                model_accounting_includes_baseline,
                resume_accounting: None,
            },
            receiver,
            ready_sender,
        )));
        RuntimeRun {
            run_id,
            control,
            join,
            ready: Some(ready_receiver),
        }
    }

    async fn run_launch(
        self: Arc<Self>,
        launch: RunLaunch,
        options: RunLaunchOptions,
        mut control: mpsc::UnboundedReceiver<ControlCommand>,
        ready: oneshot::Sender<Result<(), RunStoreError>>,
    ) -> AgentOutcome {
        let RunLaunchOptions {
            budget,
            terminal_model_request,
            model_accounting_includes_baseline,
            resume_accounting,
        } = options;
        let mut ready = Some(ready);
        let resumed = matches!(&launch, RunLaunch::Resume(_));
        let (lease, replay) = match launch {
            RunLaunch::Create(request) => {
                let run_id = request.run_id.clone().unwrap_or_default();
                let parent_run_id = request.parent_run_id.clone();
                match self.store.create(*request).await {
                    Ok(created) => {
                        self.sink.emit(created.created).await;
                        (created.lease, created.replay)
                    }
                    Err(error) => {
                        signal_ready(&mut ready, Err(error.clone()));
                        return store_start_failure(error, run_id, parent_run_id);
                    }
                }
            }
            RunLaunch::Resume(run_id) => match self.store.acquire(&run_id).await {
                Ok(acquired) => {
                    for event in &acquired.replay.events {
                        self.sink.emit(event.clone()).await;
                    }
                    if let Some(outcome) = acquired.replay.snapshot.terminal.clone() {
                        signal_ready(&mut ready, Ok(()));
                        return outcome;
                    }
                    let Some(lease) = acquired.lease else {
                        let error = RunStoreError::Corrupt {
                            run_id: run_id.clone(),
                            message: "non-terminal resume returned no writer lease".to_owned(),
                        };
                        signal_ready(&mut ready, Err(error.clone()));
                        return store_start_failure(error, run_id, None);
                    };
                    if let Some(bootstrap) = resume_accounting.as_ref() {
                        let error = if acquired.replay.snapshot.request.actor.kind
                            != AgentActorKind::Root
                        {
                            Some(RunStoreError::Corrupt {
                                run_id: run_id.clone(),
                                message:
                                    "accounting baseline override is only valid for a root resume"
                                        .to_owned(),
                            })
                        } else if acquired.replay.snapshot.last_sequence
                            != bootstrap.expected_sequence
                        {
                            Some(RunStoreError::Backend {
                                message: format!(
                                    "run_resume_replay_advanced：run {} advanced from sequence {} to {} before accounting recovery acquired it",
                                    run_id,
                                    bootstrap.expected_sequence,
                                    acquired.replay.snapshot.last_sequence,
                                ),
                            })
                        } else {
                            None
                        };
                        if let Some(error) = error {
                            let _ = self.store.release(&lease).await;
                            signal_ready(&mut ready, Err(error.clone()));
                            return store_start_failure(error, run_id, None);
                        }
                    }
                    (lease, acquired.replay)
                }
                Err(error) => {
                    signal_ready(&mut ready, Err(error.clone()));
                    return store_start_failure(error, run_id, None);
                }
            },
        };
        signal_ready(&mut ready, Ok(()));
        let snapshot = replay.snapshot;
        let deadline = effective_deadline(&snapshot.request);
        let budget = budget.unwrap_or_else(|| {
            Arc::new(RuntimeBudget::new(
                snapshot.request.limits,
                snapshot.runtime_model_requests,
                snapshot.tool_calls,
            ))
        });
        let response_is_current = snapshot.last_model_response_sequence.is_some()
            && snapshot.last_model_response_sequence == snapshot.last_model_activity_sequence;
        let accounting_epoch_baseline = resume_accounting
            .map(|bootstrap| bootstrap.accounting)
            .unwrap_or_else(|| snapshot.accounting.clone());
        let recovery_model = snapshot.pending_model.clone();
        let recovery_tool = snapshot.pending_tool.clone();
        let recovery_output = response_is_current
            .then_some(snapshot.last_model_output.clone())
            .flatten();
        let recovery_failure = resumed
            .then_some(snapshot.last_model_failure.clone())
            .flatten();
        let recovered_child_ids = snapshot.pending_child_run_ids();
        let terminal_request_is_already_admitted = recovery_model
            .as_ref()
            .is_some_and(|pending| pending.request.tools.is_empty())
            || recovery_output.is_some() && snapshot.last_model_advertised_tool_names.is_empty();
        let terminal_model_request = terminal_model_request.or_else(|| {
            (!terminal_request_is_already_admitted)
                .then(|| budget.reserve_terminal_model_request())
                .flatten()
        });
        let mut state = RunState {
            snapshot,
            lease,
            accounting_epoch_baseline,
            model_accounting_includes_baseline,
            started_unix_ms: replay
                .events
                .first()
                .map_or_else(now_unix_ms, |event| event.occurred_at_unix_ms),
            pending_children: Vec::new(),
            recovery_model,
            recovery_tool,
            recovery_output,
            recovery_failure,
            recovered_child_ids,
            terminal_model_request,
        };

        let terminal =
            Box::pin(self.run_until_terminal(&mut state, &budget, &mut control, deadline, resumed))
                .await;
        self.finalize(&mut state, terminal, &budget).await
    }

    async fn run_until_terminal(
        self: &Arc<Self>,
        state: &mut RunState,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        resumed: bool,
    ) -> TerminalState {
        if resumed {
            if let Some(ambiguity) = state.recovery_ambiguity() {
                return TerminalState::RecoveryRequired { ambiguity };
            }
            if let Some(pending_control) = state.snapshot.pending_control.clone() {
                let terminal = match pending_control.action {
                    DurableControlAction::Interrupt => TerminalState::Interrupted,
                    DurableControlAction::Cancel => TerminalState::Cancelled,
                };
                return self.settled_terminal(state, terminal).await;
            }
            if let Some(stopped) = state.recovery_failure.take() {
                let failure = match stopped.reason {
                    ModelRetryStopReason::ModelRequestBudgetExceeded => {
                        RuntimeFailure::ModelRequestBudgetExceeded {
                            limit: state.snapshot.request.limits.max_model_requests,
                        }
                    }
                    _ => RuntimeFailure::Model {
                        code: stopped.failure.code,
                        category: stopped.failure.category,
                        message: stopped.failure.message,
                        retryable: stopped.failure.retryable,
                    },
                };
                return TerminalState::Failed { failure };
            }
            if let Some(candidate) = state.snapshot.pending_completion.clone() {
                match self.accept_completion_candidate(state, candidate).await {
                    Ok((message, decision)) => {
                        return TerminalState::Completed { message, decision };
                    }
                    Err(CompletionReviewError::Retry(_))
                        if budget.has_unreserved_model_request() => {}
                    Err(CompletionReviewError::Retry(reason))
                    | Err(CompletionReviewError::Blocked(reason)) => {
                        return TerminalState::Blocked { reason };
                    }
                }
            }
        }
        loop {
            match self.drain_controls(state, control).await {
                Ok(Some(terminal)) => {
                    return self.settled_terminal(state, terminal).await;
                }
                Ok(None) => {}
                Err(failure) => {
                    let terminal = self
                        .settled_terminal(state, TerminalState::Failed { failure })
                        .await;
                    return terminal;
                }
            }

            if deadline_expired(deadline) {
                let terminal = timeout_terminal(state, deadline);
                return self.settled_terminal(state, terminal).await;
            }
            let safe_fresh_boundary = state.recovery_model.is_none()
                && state.recovery_output.is_none()
                && state.recovery_tool.is_none()
                && state.snapshot.pending_model.is_none()
                && state.snapshot.pending_tool.is_none()
                && state.snapshot.pending_completion.is_none()
                && state.snapshot.pending_host_verification.is_none()
                && state.snapshot.pending_child_run_ids().is_empty();
            if safe_fresh_boundary
                && !state.snapshot.pending_steers.is_empty()
                && let Err(failure) = self.flush_pending_steers(state).await
            {
                return TerminalState::Failed { failure };
            }
            if safe_fresh_boundary && let Err(message) = self.reconcile_workspace(state).await {
                return TerminalState::Failed {
                    failure: RuntimeFailure::Store { message },
                };
            }
            let context_tools = self.tool_definitions(
                &state.snapshot.request.tool_policy,
                state.snapshot.request.actor.depth,
                state.snapshot.request.limits.max_depth,
                state.snapshot.request.environment.interactive,
            );
            if safe_fresh_boundary {
                let estimated =
                    match effective_context(context_input(&state.snapshot, &context_tools)) {
                        Ok(context) => context.estimated_tokens,
                        Err(error) => {
                            return TerminalState::Failed {
                                failure: RuntimeFailure::Store {
                                    message: error.to_string(),
                                },
                            };
                        }
                    };
                let hard_input_tokens =
                    u64::from(state.snapshot.request.context_policy.hard_input_tokens);
                if estimated > hard_input_tokens {
                    match self.compact_context(state, &context_tools).await {
                        Ok(
                            ContextCompactionControl::Committed
                            | ContextCompactionControl::NotNeeded,
                        ) => {}
                        Err(failure) => {
                            return TerminalState::Failed { failure };
                        }
                    }
                }
            }
            let turn = if let Some(output) = state.recovery_output.take() {
                Ok(ModelTurnControl::Output(ModelTurnOutput::new(
                    output,
                    state.snapshot.last_model_advertised_tool_names.clone(),
                )))
            } else if let Some(pending) = state.recovery_model.take() {
                self.model_turn(state, budget, control, deadline, Some(pending))
                    .await
            } else {
                self.model_turn(state, budget, control, deadline, None)
                    .await
            };
            let turn = match turn {
                Ok(ModelTurnControl::Terminal(terminal)) => {
                    return self.settled_terminal(state, terminal).await;
                }
                Ok(ModelTurnControl::Output(output)) => output,
                Err(failure) => {
                    let terminal = self
                        .settled_terminal(state, TerminalState::Failed { failure })
                        .await;
                    return terminal;
                }
            };

            let terminal_failure = match turn.finish_reason {
                ModelFinishReason::Length => Some(RuntimeFailure::OutputLimit),
                ModelFinishReason::ContentFilter => Some(RuntimeFailure::ContentFiltered),
                ModelFinishReason::InsufficientSystemResource => {
                    Some(RuntimeFailure::InsufficientSystemResource)
                }
                _ => None,
            };
            if let Some(failure) = terminal_failure {
                let terminal = self
                    .settled_terminal(state, TerminalState::Failed { failure })
                    .await;
                return terminal;
            }

            if let Err(message) = validate_tool_calls(&turn.tool_calls) {
                let terminal = self.settled_terminal(state, invalid_model(message)).await;
                return terminal;
            }
            if let Some(call) = turn.tool_calls.iter().find(|call| {
                !turn
                    .advertised_tool_names
                    .iter()
                    .any(|advertised| advertised == &call.name)
            }) {
                let terminal = self
                    .settled_terminal(
                        state,
                        invalid_model(format!(
                            "tool '{}' was not advertised by this model request",
                            call.name
                        )),
                    )
                    .await;
                return terminal;
            }
            if turn.tool_calls.is_empty() {
                if turn.finish_reason != ModelFinishReason::Stop {
                    let terminal = self
                        .settled_terminal(
                            state,
                            invalid_model("finish_reason=tool_calls without tool calls"),
                        )
                        .await;
                    return terminal;
                }
                if !state.snapshot.pending_steers.is_empty() {
                    if let Err(failure) = self.flush_pending_steers(state).await {
                        return TerminalState::Failed { failure };
                    }
                    continue;
                }
                if turn.content.trim().is_empty() {
                    return TerminalState::Failed {
                        failure: RuntimeFailure::EmptyModelOutput,
                    };
                }
                let contract = state
                    .snapshot
                    .request
                    .task_contract
                    .clone()
                    .expect("Agent run contract is validated at creation");
                let candidate = CompletionCandidate {
                    id: CompletionCandidateId::from(format!(
                        "completion-{}",
                        state
                            .snapshot
                            .last_model_response_sequence
                            .unwrap_or_default()
                    )),
                    generation_id: contract.generation_id.clone(),
                    message: turn.content,
                };
                if let Err(failure) = self
                    .publish(
                        state,
                        RuntimeEventKind::CompletionProposed {
                            candidate: candidate.clone(),
                        },
                    )
                    .await
                {
                    return TerminalState::Failed { failure };
                }
                match self.accept_completion_candidate(state, candidate).await {
                    Ok((message, decision)) => {
                        return TerminalState::Completed { message, decision };
                    }
                    Err(CompletionReviewError::Retry(_))
                        if budget.has_unreserved_model_request() =>
                    {
                        continue;
                    }
                    Err(CompletionReviewError::Retry(reason))
                    | Err(CompletionReviewError::Blocked(reason)) => {
                        return TerminalState::Blocked { reason };
                    }
                }
            }

            if turn.finish_reason != ModelFinishReason::ToolCalls {
                let terminal = self
                    .settled_terminal(
                        state,
                        invalid_model("tool calls require finish_reason=tool_calls"),
                    )
                    .await;
                return terminal;
            }

            for call in turn.tool_calls {
                if tool_call_completed(&state.snapshot.transcript, &call.id) {
                    continue;
                }
                let recovering_prepared = state
                    .recovery_tool
                    .as_ref()
                    .is_some_and(|pending| pending.invocation.call_id == call.id);
                if !recovering_prepared && !budget.reserve_tool() {
                    let limit = state.snapshot.request.limits.max_tool_calls;
                    let terminal = self
                        .settled_terminal(
                            state,
                            TerminalState::Failed {
                                failure: RuntimeFailure::ToolBudgetExceeded { limit },
                            },
                        )
                        .await;
                    return terminal;
                }
                match Box::pin(self.execute_call(state, call, budget, control, deadline)).await {
                    Ok(()) => {}
                    Err(terminal) => {
                        return self.settled_terminal(state, terminal).await;
                    }
                }
            }
            if !state.pending_children.is_empty()
                && let Err(terminal) = self.join_children(state, control, deadline).await
            {
                return self.settled_terminal(state, terminal).await;
            }
            if let Err(failure) = self.flush_pending_steers(state).await {
                let terminal = self
                    .settled_terminal(state, TerminalState::Failed { failure })
                    .await;
                return terminal;
            }
        }
    }

    async fn compact_context(
        &self,
        state: &mut RunState,
        tools: &[ToolDefinition],
    ) -> Result<ContextCompactionControl, RuntimeFailure> {
        match prepare_compaction(
            context_input(&state.snapshot, tools),
            state.snapshot.request.context_policy,
        )
        .map_err(context_projection_failure)?
        {
            ContextCompactionPreparation::NotNeeded { .. } => {
                Ok(ContextCompactionControl::NotNeeded)
            }
            ContextCompactionPreparation::LimitExceeded {
                estimated_tokens,
                hard_input_tokens,
            } => Err(RuntimeFailure::ContextLimitExceeded {
                estimated_tokens,
                hard_input_tokens,
            }),
            ContextCompactionPreparation::Local {
                projection,
                before_tokens,
                after_tokens,
            } => {
                self.publish(
                    state,
                    RuntimeEventKind::ContextCompactionCommitted {
                        projection: Box::new(projection),
                        tools: tools.to_vec(),
                        accounting: Box::new(state.snapshot.accounting.clone()),
                        before_tokens,
                        after_tokens,
                    },
                )
                .await?;
                Ok(ContextCompactionControl::Committed)
            }
        }
    }

    async fn model_turn(
        &self,
        state: &mut RunState,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        mut prepared: Option<PendingModelAction>,
    ) -> Result<ModelTurnControl, RuntimeFailure> {
        let request_number = prepared.as_ref().map_or_else(
            || state.snapshot.local_turns.saturating_add(1),
            |pending| pending.request.request_number,
        );
        let mut primary_error = prepared
            .as_ref()
            .and_then(|pending| pending.primary_failure.as_ref())
            .map(model_port_error_from_failure);

        'attempts: loop {
            let (attempt_id, request) = if let Some(pending) = prepared.take() {
                debug_assert_eq!(pending.state, DurableActionState::Prepared);
                (pending.attempt_id, pending.request)
            } else {
                let terminal_turn_due = request_number >= state.snapshot.request.limits.max_turns;
                let (permit, terminal_turn) = if terminal_turn_due {
                    (state.terminal_model_request.take(), true)
                } else if let Some(permit) = budget.reserve_model_request() {
                    (Some(permit), false)
                } else {
                    (state.terminal_model_request.take(), true)
                };
                let Some(permit) = permit else {
                    return Ok(ModelTurnControl::Terminal(TerminalState::Failed {
                        failure: RuntimeFailure::ModelRequestBudgetExceeded {
                            limit: state.snapshot.request.limits.max_model_requests,
                        },
                    }));
                };
                let tools = if terminal_turn {
                    Vec::new()
                } else {
                    self.tool_definitions(
                        &state.snapshot.request.tool_policy,
                        state.snapshot.request.actor.depth,
                        state.snapshot.request.limits.max_depth,
                        state.snapshot.request.environment.interactive,
                    )
                };
                let context = effective_context(context_input(&state.snapshot, &tools))
                    .map_err(context_projection_failure)?;
                let hard_input_tokens =
                    u64::from(state.snapshot.request.context_policy.hard_input_tokens);
                if context.estimated_tokens > hard_input_tokens {
                    return Ok(ModelTurnControl::Terminal(TerminalState::Failed {
                        failure: RuntimeFailure::ContextLimitExceeded {
                            estimated_tokens: context.estimated_tokens,
                            hard_input_tokens,
                        },
                    }));
                }
                let request = ModelRequest {
                    run_id: state.run_id().clone(),
                    parent_run_id: state.snapshot.request.parent_run_id.clone(),
                    actor: state.snapshot.request.actor,
                    model: state.snapshot.request.model.clone(),
                    system_prompt: context.system_prompt,
                    messages: context.messages,
                    tools,
                    reasoning_effort: state.snapshot.request.reasoning_effort,
                    max_output_tokens: state.snapshot.request.max_output_tokens,
                    streaming: state.snapshot.request.streaming,
                    request_number,
                    attempt: 0,
                };
                let attempt_id = AttemptId::new();
                self.publish_with_model_permit(
                    state,
                    RuntimeEventKind::ModelRequestPrepared {
                        attempt_id: attempt_id.clone(),
                        request: Box::new(request.clone()),
                    },
                    permit,
                )
                .await?;
                (attempt_id, request)
            };
            match self
                .execute_model_attempt(
                    state,
                    attempt_id.clone(),
                    request.clone(),
                    control,
                    deadline,
                )
                .await?
            {
                ModelAttemptControl::Output(output) => {
                    let advertised_tool_names =
                        request.tools.iter().map(|tool| tool.name.clone()).collect();
                    return Ok(ModelTurnControl::Output(ModelTurnOutput::new(
                        output,
                        advertised_tool_names,
                    )));
                }
                ModelAttemptControl::Failed {
                    error,
                    actionable_output,
                } => {
                    let plan = self.plan_model_failure(
                        state,
                        budget,
                        &request,
                        &mut primary_error,
                        &error,
                        actionable_output,
                    );
                    match plan {
                        ModelFailurePlan::Retry {
                            prepared: retry_prepared,
                            permit,
                        } => {
                            self.commit_model_failure(
                                state,
                                &attempt_id,
                                &error,
                                actionable_output,
                                ModelRetryDecision::Retry {
                                    prepared: retry_prepared,
                                },
                                Some(permit),
                            )
                            .await?;
                            prepared = state.snapshot.pending_model.clone();
                            if prepared.is_none() {
                                return Err(RuntimeFailure::Store {
                                    message:
                                        "committed retry did not project a pending model request"
                                            .to_owned(),
                                });
                            }
                            continue 'attempts;
                        }
                        ModelFailurePlan::Stop { reason, terminal } => {
                            self.commit_model_failure(
                                state,
                                &attempt_id,
                                &error,
                                actionable_output,
                                ModelRetryDecision::Stop { reason },
                                None,
                            )
                            .await?;
                            return Ok(ModelTurnControl::Terminal(terminal));
                        }
                    }
                }
                ModelAttemptControl::Terminal(terminal) => {
                    return Ok(ModelTurnControl::Terminal(terminal));
                }
            }
        }
    }

    async fn execute_model_attempt(
        &self,
        state: &mut RunState,
        attempt_id: AttemptId,
        request: ModelRequest,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<ModelAttemptControl, RuntimeFailure> {
        self.publish(
            state,
            RuntimeEventKind::ModelRequestInFlight {
                attempt_id: attempt_id.clone(),
            },
        )
        .await?;

        let open = self.model.stream(request);
        tokio::pin!(open);
        let mut stream = loop {
            tokio::select! {
                result = &mut open => break match result {
                    Ok(stream) => stream,
                    Err(error) => {
                        return Ok(ModelAttemptControl::Failed {
                            error,
                            actionable_output: false,
                        });
                    }
                },
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await? {
                        ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                        ControlEffect::Terminal(terminal) => {
                            return Ok(ModelAttemptControl::Terminal(terminal));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    return Ok(ModelAttemptControl::Terminal(timeout_terminal(state, deadline)));
                }
            }
        };

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut content_delta_seen = false;
        let mut reasoning_delta_seen = false;
        let mut actionable_output = false;
        let mut content_index = 0_u64;
        let mut reasoning_index = 0_u64;
        loop {
            tokio::select! {
                event = next_model_event(&mut *stream, state.snapshot.request.limits.model_event_idle_ms) => {
                    match event {
                        ModelEventPoll::Idle => {
                            let timeout_ms = state.snapshot.request.limits.model_event_idle_ms.unwrap_or_default();
                            let error = ModelPortError::new(
                                "stream_stall",
                                ModelErrorCategory::StreamStall,
                                format!("model stream produced no canonical event for {timeout_ms}ms"),
                                true,
                            );
                            return Ok(ModelAttemptControl::Failed { error, actionable_output });
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::ContentDelta { delta }))) => {
                            if !delta.is_empty() {
                                actionable_output = true;
                                content_delta_seen = true;
                                content.push_str(&delta);
                                content_index = content_index.saturating_add(1);
                                self.publish(
                                    state,
                                    RuntimeEventKind::ContentDelta {
                                        attempt_id: attempt_id.clone(),
                                        index: content_index,
                                        delta,
                                    },
                                ).await?;
                            }
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::ReasoningDelta { delta }))) => {
                            if !delta.is_empty() {
                                actionable_output = true;
                                reasoning_delta_seen = true;
                                reasoning.push_str(&delta);
                                reasoning_index = reasoning_index.saturating_add(1);
                                self.publish(
                                    state,
                                    RuntimeEventKind::ReasoningDelta {
                                        attempt_id: attempt_id.clone(),
                                        index: reasoning_index,
                                        delta,
                                    },
                                ).await?;
                            }
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::Completed { output }))) => {
                            if content_delta_seen && content != output.content {
                                return Ok(ModelAttemptControl::Terminal(invalid_model(
                                    "streamed content differs from the atomic completed content",
                                )));
                            }
                            let completed_reasoning = output.reasoning_content.clone().unwrap_or_default();
                            if reasoning_delta_seen && reasoning != completed_reasoning {
                                return Ok(ModelAttemptControl::Terminal(invalid_model(
                                    "streamed reasoning differs from the atomic completed reasoning",
                                )));
                            }
                            if !content_delta_seen && !output.content.is_empty() {
                                content_index = content_index.saturating_add(1);
                                self.publish(
                                    state,
                                    RuntimeEventKind::ContentDelta {
                                        attempt_id: attempt_id.clone(),
                                        index: content_index,
                                        delta: output.content.clone(),
                                    },
                                ).await?;
                            }
                            if !reasoning_delta_seen && !completed_reasoning.is_empty() {
                                reasoning_index = reasoning_index.saturating_add(1);
                                self.publish(
                                    state,
                                    RuntimeEventKind::ReasoningDelta {
                                        attempt_id: attempt_id.clone(),
                                        index: reasoning_index,
                                        delta: completed_reasoning,
                                    },
                                ).await?;
                            }
                            let accounting = self.cumulative_accounting(state, false).await;
                            self.publish(
                                state,
                                RuntimeEventKind::ModelResponseCommitted {
                                    attempt_id,
                                    output: Box::new(output.clone()),
                                    accounting: Box::new(accounting),
                                },
                            ).await?;
                            return Ok(ModelAttemptControl::Output(output));
                        }
                        ModelEventPoll::Event(Some(Err(error))) => {
                            return Ok(ModelAttemptControl::Failed { error, actionable_output });
                        }
                        ModelEventPoll::Event(None) => {
                            let error = ModelPortError::new(
                                "incomplete_model_stream",
                                ModelErrorCategory::Protocol,
                                "model stream ended without a completed event",
                                true,
                            );
                            return Ok(ModelAttemptControl::Failed { error, actionable_output });
                        }
                    }
                }
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await? {
                        ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                        ControlEffect::Terminal(terminal) => {
                            return Ok(ModelAttemptControl::Terminal(terminal));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    return Ok(ModelAttemptControl::Terminal(timeout_terminal(state, deadline)));
                }
            }
        }
    }

    async fn commit_model_failure(
        &self,
        state: &mut RunState,
        attempt_id: &AttemptId,
        error: &ModelPortError,
        actionable_output: bool,
        retry: ModelRetryDecision,
        permit: Option<ModelRequestPermit>,
    ) -> Result<(), RuntimeFailure> {
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish_inner(
            state,
            RuntimeEventKind::ModelRequestFailed {
                attempt_id: attempt_id.clone(),
                failure: model_attempt_failure(error, actionable_output),
                accounting: Box::new(accounting),
                retry,
            },
            permit,
        )
        .await?;
        Ok(())
    }

    async fn cumulative_accounting(&self, state: &RunState, seal: bool) -> ModelAccounting {
        let mut cumulative = state.accounting_epoch_baseline.clone();
        match self.model.accounting_snapshot(seal).await {
            Ok(current) if state.model_accounting_includes_baseline => cumulative = current,
            Ok(current) => cumulative.add_assign(&current),
            Err(_) => {
                cumulative.complete = false;
                cumulative.usage_complete = false;
                cumulative.usage_incomplete = true;
                cumulative.billing_unknown = true;
            }
        }
        cumulative
    }

    fn plan_model_failure(
        &self,
        state: &RunState,
        budget: &Arc<RuntimeBudget>,
        request: &ModelRequest,
        primary: &mut Option<ModelPortError>,
        error: &ModelPortError,
        actionable_output: bool,
    ) -> ModelFailurePlan {
        let differs_from_primary = primary.as_ref().is_some_and(|latched| {
            latched.code != error.code || latched.category != error.category
        });
        if primary.is_none() {
            *primary = Some(error.clone());
        }
        let stop_reason = if actionable_output {
            Some(ModelRetryStopReason::ActionableOutput)
        } else if !error.retryable {
            Some(ModelRetryStopReason::NotRetryable)
        } else if differs_from_primary {
            Some(ModelRetryStopReason::FailureChanged)
        } else if request.attempt >= state.snapshot.request.limits.max_model_retries {
            Some(ModelRetryStopReason::RetryLimitReached)
        } else {
            None
        };
        if let Some(reason) = stop_reason {
            let terminal = if reason == ModelRetryStopReason::ModelRequestBudgetExceeded {
                TerminalState::Failed {
                    failure: RuntimeFailure::ModelRequestBudgetExceeded {
                        limit: state.snapshot.request.limits.max_model_requests,
                    },
                }
            } else {
                TerminalState::Failed {
                    failure: latched_model_failure(primary),
                }
            };
            return ModelFailurePlan::Stop { reason, terminal };
        }
        let Some(permit) = budget.reserve_model_request() else {
            return ModelFailurePlan::Stop {
                reason: ModelRetryStopReason::ModelRequestBudgetExceeded,
                terminal: TerminalState::Failed {
                    failure: RuntimeFailure::ModelRequestBudgetExceeded {
                        limit: state.snapshot.request.limits.max_model_requests,
                    },
                },
            };
        };

        let mut next_request = request.clone();
        next_request.attempt = next_request.attempt.saturating_add(1);
        let prepared = PreparedModelRetry {
            attempt_id: AttemptId::new(),
            request: Box::new(next_request),
        };
        ModelFailurePlan::Retry { prepared, permit }
    }

    async fn execute_call(
        self: &Arc<Self>,
        state: &mut RunState,
        call: ModelToolCall,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<(), TerminalState> {
        let invocation = ToolInvocation {
            run_id: state.run_id().clone(),
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let (operation_id, operation_started) = match state.recovery_tool.take() {
            Some(pending) if pending.invocation.call_id == call.id => {
                let started = pending.state == DurableActionState::InFlight;
                debug_assert!(
                    !started
                        || (pending.invocation.name == AGENT_TOOL_NAME
                            && state.snapshot.agent_tasks.iter().any(|lifecycle| {
                                lifecycle.task.call_id == call.id
                                    && writer_tool_recovery_is_safe(lifecycle)
                            }))
                );
                (pending.operation_id, started)
            }
            Some(pending) => {
                state.recovery_tool = Some(pending);
                return Err(TerminalState::Failed {
                    failure: RuntimeFailure::InvalidModelOutput {
                        message: "persisted prepared tool does not match the next model call"
                            .to_owned(),
                    },
                });
            }
            None => {
                let operation_id = OperationId::new();
                self.publish(
                    state,
                    RuntimeEventKind::ToolPrepared {
                        operation_id: operation_id.clone(),
                        invocation: invocation.clone(),
                        workspace_access: if call.name == AGENT_TOOL_NAME {
                            agent_tool_workspace_access(&call)
                        } else if call.name == REQUEST_USER_INPUT_TOOL_NAME {
                            WorkspaceAccess::ReadOnly
                        } else {
                            self.tools.workspace_access(&invocation)
                        },
                    },
                )
                .await
                .map_err(store_terminal)?;
                (operation_id, false)
            }
        };

        let mut terminal_after_result = None;
        let outcome = if !state.snapshot.request.tool_policy.permits(&call.name) {
            ToolOutcome::rejected(
                format!("tool_not_allowed：工具 '{}' 未获准调用", call.name),
                ToolRetryDisposition::NotRetryable,
            )
        } else if call.arguments.parsed.is_none() {
            ToolOutcome::rejected(
                format!(
                    "invalid_arguments：JSON 参数格式错误：{}",
                    call.arguments.raw
                ),
                ToolRetryDisposition::AfterCorrection,
            )
        } else if call.name == AGENT_TOOL_NAME {
            if !operation_started {
                self.publish(
                    state,
                    RuntimeEventKind::ToolExecutionStarted {
                        operation_id: operation_id.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
            }
            match Box::pin(self.launch_child(state, &call, budget, control, deadline)).await {
                Ok(outcome) => outcome,
                Err(terminal) => {
                    terminal_after_result = Some(terminal);
                    if state
                        .snapshot
                        .agent_tasks
                        .iter()
                        .any(|lifecycle| lifecycle.task.call_id == call.id)
                    {
                        cancelled_tool_outcome(
                            "agent_lifecycle_stopped：子 Agent 生命周期未能安全完成",
                        )
                    } else {
                        ToolOutcome::rejected(
                            "agent_preflight_failed：子 Agent 在产生生命周期副作用前被 Host 拒绝",
                            ToolRetryDisposition::NotRetryable,
                        )
                    }
                }
            }
        } else if call.name == REQUEST_USER_INPUT_TOOL_NAME {
            if !state.snapshot.request.environment.interactive
                || state.snapshot.request.actor.kind != AgentActorKind::Root
            {
                ToolOutcome::rejected(
                    "interaction_unavailable：当前运行没有可响应 request_user_input 的交互客户端",
                    ToolRetryDisposition::AfterCorrection,
                )
            } else {
                let Some(arguments) = call.arguments.parsed.as_ref() else {
                    unreachable!("malformed arguments were rejected before interaction dispatch")
                };
                let request = match serde_json::from_value::<UserInputRequest>(arguments.clone()) {
                    Ok(request) => request,
                    Err(error) => {
                        return self
                            .commit_tool_outcome(
                                state,
                                operation_id,
                                &call,
                                ToolOutcome::rejected(
                                    format!(
                                        "invalid_arguments：request_user_input 参数无效：{error}"
                                    ),
                                    ToolRetryDisposition::AfterCorrection,
                                ),
                            )
                            .await;
                    }
                };
                if let Err(message) = request.validate() {
                    ToolOutcome::rejected(
                        format!("invalid_arguments：request_user_input 参数无效：{message}"),
                        ToolRetryDisposition::AfterCorrection,
                    )
                } else {
                    match self
                        .wait_for_interaction(
                            state,
                            &operation_id,
                            UserInteractionPrompt::UserInput { request },
                            control,
                            deadline,
                        )
                        .await
                        .map_err(store_terminal)?
                    {
                        InteractionWaitResult::Resolved(UserInteractionResponse::Answered {
                            answers,
                        }) => ToolOutcome::json(&json!({"answers": answers})).unwrap_or_else(
                            |error| {
                                ToolOutcome::error(format!("user_input_encoding_failed：{error}"))
                            },
                        ),
                        InteractionWaitResult::Resolved(UserInteractionResponse::Cancelled) => {
                            ToolOutcome::rejected(
                                "user_input_cancelled：用户取消了本次澄清请求",
                                ToolRetryDisposition::AfterCorrection,
                            )
                        }
                        InteractionWaitResult::Resolved(_) => ToolOutcome::rejected(
                            "interaction_mismatch：request_user_input 收到了错误类型的响应",
                            ToolRetryDisposition::NotRetryable,
                        ),
                        InteractionWaitResult::Terminal(terminal) => {
                            terminal_after_result = Some(terminal);
                            cancelled_tool_outcome(
                                "user_input_interrupted：等待用户输入时运行已停止",
                            )
                        }
                    }
                }
            }
        } else {
            let approval = match self.tools.approval_prompt(&invocation) {
                Ok(approval) => approval,
                Err(error) => {
                    return self
                        .commit_tool_outcome(
                            state,
                            operation_id,
                            &call,
                            ToolOutcome::rejected(
                                format!("{}：工具授权预检失败：{}", error.code, error.message),
                                ToolRetryDisposition::NotRetryable,
                            ),
                        )
                        .await;
                }
            };
            let rejected = if let Some(prompt) = approval {
                if !state.snapshot.request.environment.interactive {
                    Some(ToolOutcome::rejected(
                        "approval_required：当前非交互运行无法批准该工具调用",
                        ToolRetryDisposition::AfterCorrection,
                    ))
                } else {
                    match self
                        .wait_for_interaction(
                            state,
                            &operation_id,
                            UserInteractionPrompt::Approval {
                                prompt,
                                arguments: call
                                    .arguments
                                    .parsed
                                    .clone()
                                    .expect("validated tool arguments"),
                            },
                            control,
                            deadline,
                        )
                        .await
                        .map_err(store_terminal)?
                    {
                        InteractionWaitResult::Resolved(UserInteractionResponse::Approved) => None,
                        InteractionWaitResult::Resolved(UserInteractionResponse::Denied {
                            reason,
                        }) => Some(ToolOutcome::rejected(
                            reason.unwrap_or_else(|| "用户拒绝了工具调用".to_owned()),
                            ToolRetryDisposition::AfterCorrection,
                        )),
                        InteractionWaitResult::Resolved(UserInteractionResponse::Cancelled) => {
                            Some(ToolOutcome::rejected(
                                "用户取消了工具审批",
                                ToolRetryDisposition::AfterCorrection,
                            ))
                        }
                        InteractionWaitResult::Resolved(_) => Some(ToolOutcome::rejected(
                            "interaction_mismatch：工具审批收到了错误类型的响应",
                            ToolRetryDisposition::NotRetryable,
                        )),
                        InteractionWaitResult::Terminal(terminal) => {
                            terminal_after_result = Some(terminal);
                            Some(cancelled_tool_outcome(
                                "tool_approval_interrupted：等待工具审批时运行已停止",
                            ))
                        }
                    }
                }
            } else {
                None
            };
            if let Some(outcome) = rejected {
                outcome
            } else {
                if !operation_started {
                    self.publish(
                        state,
                        RuntimeEventKind::ToolExecutionStarted {
                            operation_id: operation_id.clone(),
                        },
                    )
                    .await
                    .map_err(store_terminal)?;
                }
                let cancellation = CancellationToken::default();
                let execution = self.tools.execute(invocation, cancellation.clone());
                tokio::pin!(execution);
                loop {
                    tokio::select! {
                        result = &mut execution => break match result {
                            Ok(outcome) => outcome,
                            Err(error) => ToolOutcome::transport_failure(format!(
                                "{}：工具执行失败：{}",
                                error.code, error.message
                            )),
                        },
                        command = control.recv() => if let Some(command) = command {
                            match self.handle_control(state, command).await.map_err(store_terminal)? {
                                ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                                ControlEffect::Terminal(terminal) => {
                                    cancellation.cancel();
                                    let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
                                    let message = if matches!(terminal, TerminalState::Interrupted) {
                                        "tool_interrupted：工具执行已中断"
                                    } else {
                                        "tool_cancelled：工具执行已取消"
                                    };
                                    terminal_after_result = Some(terminal);
                                    break cancelled_tool_outcome(message);
                                }
                            }
                        },
                        () = wait_for_deadline(deadline) => {
                            cancellation.cancel();
                            let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
                            terminal_after_result = Some(timeout_terminal(state, deadline));
                            break cancelled_tool_outcome("tool_deadline_exceeded：工具执行超过本次运行期限");
                        }
                    }
                }
            }
        };

        let unintegrated_writer_recovery = call.name == AGENT_TOOL_NAME
            && matches!(
                terminal_after_result,
                Some(TerminalState::RecoveryRequired { .. })
            )
            && state.snapshot.agent_tasks.iter().any(|lifecycle| {
                lifecycle.task.call_id == call.id
                    && super::store::is_unintegrated_writer_recovery(lifecycle)
            });
        let workspace_state = if state.snapshot.pending_tool.as_ref().is_some_and(|pending| {
            pending.workspace_access == WorkspaceAccess::MayWrite
                && !(pending.invocation.name == AGENT_TOOL_NAME
                    && (outcome.side_effect == ToolSideEffectStatus::NotApplied
                        || unintegrated_writer_recovery))
        }) {
            Some(self.observe_workspace_state(state, true).await)
        } else {
            None
        };
        self.publish(
            state,
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: call.id.clone(),
                name: call.name.clone(),
                outcome: Box::new(outcome.clone()),
                workspace_state,
            },
        )
        .await
        .map_err(store_terminal)?;
        match terminal_after_result {
            Some(terminal) => Err(terminal),
            None => Ok(()),
        }
    }

    async fn commit_tool_outcome(
        &self,
        state: &mut RunState,
        operation_id: OperationId,
        call: &ModelToolCall,
        outcome: ToolOutcome,
    ) -> Result<(), TerminalState> {
        self.publish(
            state,
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: call.id.clone(),
                name: call.name.clone(),
                outcome: Box::new(outcome),
                workspace_state: None,
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(())
    }

    async fn wait_for_interaction(
        &self,
        state: &mut RunState,
        operation_id: &OperationId,
        prompt: UserInteractionPrompt,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<InteractionWaitResult, RuntimeFailure> {
        let existing = state
            .snapshot
            .pending_tool
            .as_ref()
            .and_then(|pending| pending.interaction.as_ref())
            .cloned();
        let interaction = if let Some(existing) = existing {
            existing
        } else {
            let pending = state
                .snapshot
                .pending_tool
                .as_ref()
                .expect("an interaction is requested only for a prepared tool");
            let request = UserInteractionRequest {
                interaction_id: InteractionId::new(),
                operation_id: operation_id.clone(),
                call_id: pending.invocation.call_id.clone(),
                tool_name: pending.invocation.name.clone(),
                prompt,
            };
            self.publish(
                state,
                RuntimeEventKind::InteractionRequested {
                    request: request.clone(),
                },
            )
            .await?;
            PendingUserInteraction {
                request,
                response: None,
            }
        };
        if let Some(response) = interaction.response {
            return Ok(InteractionWaitResult::Resolved(response));
        }
        loop {
            tokio::select! {
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await? {
                        ControlEffect::Continue => {}
                        ControlEffect::InteractionResolved(response) => {
                            return Ok(InteractionWaitResult::Resolved(response));
                        }
                        ControlEffect::Terminal(terminal) => {
                            return Ok(InteractionWaitResult::Terminal(terminal));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    return Ok(InteractionWaitResult::Terminal(timeout_terminal(state, deadline)));
                }
            }
        }
    }

    async fn launch_child(
        self: &Arc<Self>,
        state: &mut RunState,
        call: &ModelToolCall,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<ToolOutcome, TerminalState> {
        if state.snapshot.request.actor.depth >= state.snapshot.request.limits.max_depth {
            return Ok(ToolOutcome::rejected(
                format!(
                    "child_depth_limit：已达到子 Agent 深度上限 {}",
                    state.snapshot.request.limits.max_depth
                ),
                ToolRetryDisposition::NotRetryable,
            ));
        }
        let Some(arguments) = call.arguments.parsed.as_ref() else {
            return Ok(ToolOutcome::rejected(
                format!(
                    "invalid_arguments：JSON 参数格式错误：{}",
                    call.arguments.raw
                ),
                ToolRetryDisposition::AfterCorrection,
            ));
        };
        let launch = match AgentLaunchRequest::parse(arguments) {
            Ok(launch) => launch,
            Err(message) => {
                return Ok(ToolOutcome::rejected(
                    format!("invalid_arguments：{message}"),
                    ToolRetryDisposition::AfterCorrection,
                ));
            }
        };
        let writer = launch.workspace_access == AgentWorkspaceAccess::IsolatedWrite;
        if writer && state.snapshot.request.actor.depth != 0 {
            return Ok(ToolOutcome::rejected(
                "writer_root_only：M6-A 隔离写入只允许由 root Agent 启动",
                ToolRetryDisposition::NotRetryable,
            ));
        }
        if writer
            && state.snapshot.agent_tasks.iter().any(|lifecycle| {
                lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                    && lifecycle.task.call_id != call.id
            })
        {
            return Ok(ToolOutcome::rejected(
                "writer_single_root_limit：M6-A 每个 root run 只允许冻结一个隔离 writer 任务",
                ToolRetryDisposition::NotRetryable,
            ));
        }
        if writer && !state.snapshot.request.environment.auto_approve {
            return Ok(ToolOutcome::rejected(
                "writer_requires_auto_approve：隔离写入子 Agent 只接受 Host 已显式启用的自动批准运行",
                ToolRetryDisposition::NotRetryable,
            ));
        }
        let writer_acceptance = if writer {
            let Some(contract) = state.snapshot.request.task_contract.as_ref() else {
                return Ok(ToolOutcome::rejected(
                    "writer_requires_exact_verifier：父任务没有冻结 TaskContract",
                    ToolRetryDisposition::NotRetryable,
                ));
            };
            let verifier = contract
                .definition
                .acceptance
                .iter()
                .filter(|acceptance| matches!(acceptance, TaskAcceptance::Verifier { .. }))
                .cloned()
                .collect::<Vec<_>>();
            if verifier.len() != 1 {
                return Ok(ToolOutcome::rejected(
                    "writer_requires_exact_verifier：隔离写入子 Agent 要求父任务恰好冻结一个 exact Verifier acceptance",
                    ToolRetryDisposition::NotRetryable,
                ));
            }
            Some(verifier.into_iter().next().expect("one verifier"))
        } else {
            None
        };
        let prompt = arguments
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty());
        let Some(prompt) = prompt else {
            return Ok(ToolOutcome::rejected(
                "invalid_arguments：agent 需要非空的 prompt",
                ToolRetryDisposition::AfterCorrection,
            ));
        };
        if writer
            && let Some(existing) = state
                .snapshot
                .agent_tasks
                .iter()
                .find(|lifecycle| lifecycle.task.call_id == call.id)
                .cloned()
        {
            return Box::pin(self.recover_writer_lifecycle(
                state, call, existing, budget, control, deadline, arguments,
            ))
            .await;
        }
        let Some(child_lease) = budget.reserve_child() else {
            return Ok(ToolOutcome::rejected(
                format!(
                    "child_concurrency_limit：已达到子 Agent 并发上限 {}",
                    state.snapshot.request.limits.max_concurrent_children
                ),
                ToolRetryDisposition::AfterCorrection,
            ));
        };
        let Some(child_terminal_model_request) = budget.reserve_terminal_model_request() else {
            return Ok(ToolOutcome::rejected(
                format!(
                    "model_request_capacity：共享逻辑模型请求预算 {} 无法为子 Agent 保留最终产物请求",
                    state.snapshot.request.limits.max_model_requests
                ),
                ToolRetryDisposition::NotRetryable,
            ));
        };
        let child_run_id = RunId::new();
        let task_id = AgentTaskId::from(format!(
            "{}-{}",
            if writer { "writer" } else { "reader" },
            child_run_id.0
        ));
        let child_depth = state.snapshot.request.actor.depth.saturating_add(1);

        let mut child_policy = state.snapshot.request.tool_policy.clone();
        if let Some(requested) = arguments.get("allowed_tools").and_then(Value::as_array) {
            let requested = requested
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            child_policy.allowed = Some(match child_policy.allowed.take() {
                Some(parent) => parent
                    .into_iter()
                    .filter(|name| requested.iter().any(|candidate| candidate == name))
                    .collect(),
                None => requested,
            });
        }
        let role = arguments
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("general");
        if writer {
            // M6-A admits one isolated writer. A role never grants authority,
            // and the writer cannot recursively create another writer.
            if !child_policy
                .denied
                .iter()
                .any(|name| name == AGENT_TOOL_NAME)
            {
                child_policy.denied.push(AGENT_TOOL_NAME.to_owned());
            }
        } else {
            for denied in [
                "apply_patch",
                "edit_file",
                "write_file",
                "delete_file",
                "exec_shell",
                "run_tests",
                "run_verifiers",
                "shell",
                "git",
                "write",
            ] {
                if !child_policy.denied.iter().any(|name| name == denied) {
                    child_policy.denied.push(denied.to_owned());
                }
            }
        }
        let child_input = if launch.expected_artifact.is_empty() {
            prompt.to_owned()
        } else {
            format!("{prompt}\n期望产物：{}", launch.expected_artifact)
        };
        let mut system_prompt = state
            .snapshot
            .transcript
            .entries
            .iter()
            .find_map(|entry| match entry {
                TranscriptEntry::System { prompt } => Some(prompt.clone()),
                _ => None,
            })
            .unwrap_or_else(|| state.snapshot.request.system_prompt.clone());
        let access_prompt = if writer {
            format!(
                "你是在同一 AgentRuntime 中运行的隔离写入子 Agent。角色：{role}。只在分配的 worktree 内修改允许路径；不要访问主工作区或 Git 元数据。必须使用本次实际提供的文件工具完成任务，不得只描述或声称已修改；已有文件先读取，再用写工具修改，写后重新读取相关文件核对最终内容。至少一次写工具成功前不得提出完成；最终结果仍由 Host 的冻结 exact verifier 验收。"
            )
        } else {
            format!(
                "你是在同一 AgentRuntime 中运行的只读后台子 Agent。角色：{role}。只使用本次实际提供的工具，不要尝试修改文件或调用不可用工具；向父 Agent 返回简洁、具体、可验证的结果。"
            )
        };
        system_prompt.blocks.push(SystemPromptBlock {
            text: access_prompt,
            cache_control: PromptCacheControl::Volatile,
        });
        let mut child_limits = state.snapshot.request.limits;
        if let Some(max_steps) = arguments.get("max_steps").and_then(Value::as_u64) {
            let max_steps = u32::try_from(max_steps.max(1)).unwrap_or(u32::MAX);
            child_limits.max_turns = child_limits.max_turns.min(max_steps);
        }
        if let Some(max_depth) = arguments.get("max_depth").and_then(Value::as_u64) {
            let requested_absolute =
                child_depth.saturating_add(max_depth.min(u64::from(u8::MAX)) as u8);
            child_limits.max_depth = child_limits.max_depth.min(requested_absolute);
        }
        if let Some(wall_time_secs) = arguments.get("wall_time_secs").and_then(Value::as_u64) {
            let requested = wall_time_secs.max(1).saturating_mul(1_000);
            child_limits.wall_time_ms = Some(
                child_limits
                    .wall_time_ms
                    .map_or(requested, |parent| parent.min(requested)),
            );
        }
        let child_deadline_unix_ms = bounded_child_deadline(
            state.snapshot.request.deadline_unix_ms,
            child_limits.wall_time_ms,
            now_unix_ms(),
        );
        let fork_context = arguments
            .get("fork_context")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let transcript = if fork_context {
            let mut transcript = state.snapshot.transcript.clone();
            if matches!(
                transcript.entries.last(),
                Some(TranscriptEntry::Assistant { .. })
            ) {
                transcript.entries.pop();
            }
            if let Some(TranscriptEntry::System { prompt }) = transcript.entries.first_mut() {
                *prompt = system_prompt.clone();
            }
            transcript
        } else {
            CanonicalTranscript::default()
        };
        let context_projection = fork_context
            .then(|| state.snapshot.context_projection.clone())
            .flatten();

        let root_run_id = state
            .snapshot
            .request
            .agent_task
            .as_ref()
            .map_or_else(|| state.run_id().clone(), |task| task.root_run_id.clone());
        let root_workspace = state.snapshot.request.agent_task.as_ref().map_or_else(
            || state.snapshot.request.environment.workspace.clone(),
            |task| task.workspace.root_workspace.clone(),
        );
        let workspace = if writer {
            let Some(orchestrator) = self.orchestrator.as_ref() else {
                return Ok(ToolOutcome::rejected(
                    "writer_orchestrator_unavailable：当前 composition 未配置隔离写入 Orchestrator",
                    ToolRetryDisposition::NotRetryable,
                ));
            };
            let plan = orchestrator
                .prepare_writer(WriterPreparation {
                    task_id: task_id.clone(),
                    root_workspace: root_workspace.clone(),
                    allowed_paths: launch.allowed_paths.clone(),
                })
                .await
                .map_err(orchestration_terminal)?;
            if plan.assignment.access != AgentWorkspaceAccess::IsolatedWrite
                || plan.assignment.root_workspace != root_workspace
                || plan.assignment.allowed_paths != launch.allowed_paths
            {
                return Err(orchestration_recovery(
                    &task_id,
                    "writer_plan_mismatch",
                    "Orchestrator 返回的 writer assignment 与 Host 冻结请求不一致",
                ));
            }
            plan.assignment
        } else {
            AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::ReadOnly,
                root_workspace: root_workspace.clone(),
                base_commit: read_only_workspace_identity(
                    self.tools
                        .observe_workspace_revision()
                        .await
                        .ok()
                        .as_deref(),
                ),
                root_branch: None,
                worktree_path: None,
                branch: None,
                allowed_paths: Vec::new(),
                owner_token: None,
            }
        };
        let task_contract = TaskContract {
            generation_id: TaskGenerationId::from(child_run_id.0.clone()),
            definition: if let Some(acceptance) = writer_acceptance {
                TaskDefinition {
                    objective: child_input,
                    constraints: vec![format!(
                        "只允许修改这些相对路径：{}",
                        launch.allowed_paths.join(", ")
                    )],
                    non_goals: vec!["不得修改主工作区、其他 worktree 或 Git 元数据".to_owned()],
                    acceptance: vec![acceptance],
                }
            } else {
                TaskDefinition::host(child_input)
            },
        };
        let task = AgentTask {
            task_id: task_id.clone(),
            root_run_id,
            parent_run_id: state.run_id().clone(),
            child_run_id: child_run_id.clone(),
            call_id: call.id.clone(),
            role: role.to_owned(),
            task_contract: task_contract.clone(),
            workspace: workspace.clone(),
            tool_policy: child_policy.clone(),
            limits: child_limits,
            deadline_unix_ms: child_deadline_unix_ms,
            expected_artifact: launch.expected_artifact.clone(),
        };
        task.validate()
            .map_err(|message| orchestration_recovery(&task_id, "invalid_agent_task", message))?;
        self.publish(
            state,
            RuntimeEventKind::AgentTaskPrepared {
                task: Box::new(task.clone()),
            },
        )
        .await
        .map_err(store_terminal)?;

        let writer_binding = if writer {
            let orchestrator = self
                .orchestrator
                .as_ref()
                .expect("writer availability checked before task preparation");
            let binding = match orchestrator.bind_writer(&task, None).await {
                Ok(binding) => binding,
                Err(error) => {
                    let finished = self
                        .finish_uncreated_writer(
                            state,
                            &task,
                            orchestration_child_outcome(&task, &error),
                        )
                        .await?;
                    return match error.kind {
                        AgentOrchestrationErrorKind::RecoveryRequired => Err(finished.terminal),
                        AgentOrchestrationErrorKind::Rejected
                        | AgentOrchestrationErrorKind::Conflict => {
                            Ok(writer_failure_tool_outcome(&task, &finished))
                        }
                    };
                }
            };
            if binding.assignment != task.workspace {
                let error = AgentOrchestrationError::new(
                    AgentOrchestrationErrorKind::RecoveryRequired,
                    "writer_binding_mismatch",
                    "创建后的 writer assignment 与已持久化 AgentTask 不一致",
                );
                let finished = self
                    .finish_uncreated_writer(
                        state,
                        &task,
                        orchestration_child_outcome(&task, &error),
                    )
                    .await?;
                return Err(finished.terminal);
            }
            self.publish(
                state,
                RuntimeEventKind::AgentWorkspaceCreated {
                    task_id: task_id.clone(),
                    assignment: binding.assignment.clone(),
                    writer_workspace_state: binding.writer_workspace_state.clone(),
                },
            )
            .await
            .map_err(store_terminal)?;
            Some(binding)
        } else {
            None
        };
        self.publish(
            state,
            RuntimeEventKind::ChildStarted {
                task_id: task_id.clone(),
                call_id: call.id.clone(),
                child_run_id: child_run_id.clone(),
                depth: child_depth,
            },
        )
        .await
        .map_err(store_terminal)?;

        let mut child_environment = state.snapshot.request.environment.clone();
        child_environment.interactive = false;
        child_environment.workspace = workspace.execution_workspace().to_owned();
        if writer {
            child_environment.trust_mode = false;
            child_environment.allow_sandbox_elevation = false;
            child_environment.sandbox = Some("isolated_writer".to_owned());
        }
        let child_request = RunRequest {
            run_id: Some(child_run_id.clone()),
            parent_run_id: Some(state.run_id().clone()),
            continued_from_run_id: None,
            model: state.snapshot.request.model.clone(),
            task_contract: Some(task_contract),
            system_prompt,
            transcript,
            reasoning_effort: state.snapshot.request.reasoning_effort,
            max_output_tokens: state.snapshot.request.max_output_tokens,
            streaming: false,
            actor: AgentActor {
                kind: AgentActorKind::Child,
                depth: child_depth,
            },
            agent_task: Some(task.clone()),
            deadline_unix_ms: child_deadline_unix_ms,
            tool_policy: child_policy,
            limits: child_limits,
            environment: child_environment,
            context_policy: state.snapshot.request.context_policy,
            context_projection,
            inherited_facts: None,
            accounting_baseline: state.accounting_epoch_baseline.clone(),
        };
        let child_runtime = writer_binding.as_ref().map_or_else(
            || self.clone(),
            |binding| self.with_tools(binding.tools.clone()),
        );
        let child = child_runtime.start_inner(
            child_request,
            budget.clone(),
            Some(child_terminal_model_request),
            state.model_accounting_includes_baseline,
        );
        if writer {
            return Box::pin(self.finish_writer_child(
                state,
                task,
                child,
                control,
                deadline,
                child_lease,
            ))
            .await;
        }
        state.pending_children.push(PendingChild {
            task_id,
            call_id: call.id.clone(),
            run_id: child_run_id.clone(),
            control: child.control.clone(),
            join: child.join,
            _lease: child_lease,
        });
        Ok(ToolOutcome::success(
            json!({
                "status": "launched",
                "agent_id": child_run_id,
                "run_id": child_run_id,
            })
            .to_string(),
        )
        .with_metadata(json!({"child_run_id": child_run_id}))
        .with_side_effect(ToolSideEffectStatus::Applied))
    }

    #[allow(clippy::too_many_arguments)]
    async fn recover_writer_lifecycle(
        self: &Arc<Self>,
        state: &mut RunState,
        call: &ModelToolCall,
        lifecycle: AgentTaskLifecycle,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        arguments: &Value,
    ) -> Result<ToolOutcome, TerminalState> {
        let task = lifecycle.task.clone();
        if task.workspace.access != AgentWorkspaceAccess::IsolatedWrite || task.call_id != call.id {
            return Err(orchestration_recovery(
                &task.task_id,
                "writer_recovery_identity",
                "持久 AgentTask 与待恢复的 agent tool call 不一致",
            ));
        }
        if lifecycle.finished.is_some() {
            return Box::pin(self.finish_writer_lifecycle(state, task, None, None, None)).await;
        }
        let orchestrator = self.orchestrator.as_ref().ok_or_else(|| {
            orchestration_recovery(
                &task.task_id,
                "writer_orchestrator_unavailable",
                "恢复 writer lifecycle 时未配置 Orchestrator",
            )
        })?;
        let seal = writer_seal_from_lifecycle(&lifecycle);
        let cleanup_settled = lifecycle
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.committed.as_ref())
            .is_some();
        let binding = if cleanup_settled || lifecycle.seal.is_some() || lifecycle.result.is_some() {
            None
        } else {
            match orchestrator.bind_writer(&task, seal.as_ref()).await {
                Ok(binding) => Some(binding),
                Err(error) => {
                    if lifecycle.workspace_created.is_some() {
                        let outcome = orchestration_child_outcome(&task, &error);
                        if error.kind == AgentOrchestrationErrorKind::RecoveryRequired {
                            self.collect_failed_writer_retained(
                                state,
                                &task,
                                outcome,
                                format!("{}：{}", error.code, error.message),
                            )
                            .await?;
                        } else {
                            self.collect_failed_writer(state, &task, outcome).await?;
                        }
                        let finished = current_agent_lifecycle(state, &task.task_id)?
                            .finished
                            .expect("failed recovered writer was finished")
                            .outcome;
                        return if matches!(
                            finished.terminal,
                            TerminalState::RecoveryRequired { .. }
                        ) {
                            Err(finished.terminal)
                        } else {
                            Ok(writer_failure_tool_outcome(&task, &finished))
                        };
                    }
                    let finished = self
                        .finish_uncreated_writer(
                            state,
                            &task,
                            orchestration_child_outcome(&task, &error),
                        )
                        .await?;
                    return match error.kind {
                        AgentOrchestrationErrorKind::RecoveryRequired => Err(finished.terminal),
                        AgentOrchestrationErrorKind::Rejected
                        | AgentOrchestrationErrorKind::Conflict => {
                            Ok(writer_failure_tool_outcome(&task, &finished))
                        }
                    };
                }
            }
        };
        if let Some(binding) = &binding {
            if binding.assignment != task.workspace {
                let error = AgentOrchestrationError::new(
                    AgentOrchestrationErrorKind::RecoveryRequired,
                    "writer_recovery_binding",
                    "恢复得到的 writer workspace 与持久 AgentTask 不一致",
                );
                let finished = self
                    .finish_uncreated_writer(
                        state,
                        &task,
                        orchestration_child_outcome(&task, &error),
                    )
                    .await?;
                return Err(finished.terminal);
            }
            if lifecycle.workspace_created.is_none() {
                self.publish(
                    state,
                    RuntimeEventKind::AgentWorkspaceCreated {
                        task_id: task.task_id.clone(),
                        assignment: binding.assignment.clone(),
                        writer_workspace_state: binding.writer_workspace_state.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
            }
        }

        let recovered_child = Box::pin(self.recover_writer_child(
            state,
            &task,
            lifecycle,
            binding,
            cleanup_settled,
            budget,
            control,
            deadline,
            arguments,
        ))
        .await?;
        Box::pin(self.finish_writer_lifecycle(
            state,
            task,
            recovered_child.child_outcome,
            recovered_child.child_replay.as_ref(),
            recovered_child.parent_terminal,
        ))
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn recover_writer_child(
        self: &Arc<Self>,
        state: &mut RunState,
        task: &AgentTask,
        lifecycle: AgentTaskLifecycle,
        binding: Option<WriterBinding>,
        cleanup_settled: bool,
        budget: &Arc<RuntimeBudget>,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        arguments: &Value,
    ) -> Result<RecoveredWriterChild, TerminalState> {
        let mut parent_terminal = None;
        let mut child_replay = self.store.load(&task.child_run_id).await.map_err(|error| {
            store_terminal(RuntimeFailure::Store {
                message: error.to_string(),
            })
        })?;
        if let Some(replay) = child_replay.as_ref()
            && !budget.absorb_recovered(
                replay.snapshot.runtime_model_requests,
                replay.snapshot.tool_calls,
            )
        {
            parent_terminal = Some(orchestration_recovery(
                &task.task_id,
                "writer_recovery_budget",
                "writer child 已消费的模型或工具额度超过 root 共享预算",
            ));
        }
        let child_outcome = if let Some(result) = lifecycle.result.clone() {
            Some(result)
        } else if let Some(terminal) = child_replay
            .as_ref()
            .and_then(|replay| replay.snapshot.terminal.clone())
        {
            Some(terminal)
        } else if cleanup_settled {
            None
        } else {
            let binding = binding
                .as_ref()
                .expect("unsettled writer recovery has an exact binding");
            if lifecycle.child_started.is_none() {
                self.publish(
                    state,
                    RuntimeEventKind::ChildStarted {
                        task_id: task.task_id.clone(),
                        call_id: task.call_id.clone(),
                        child_run_id: task.child_run_id.clone(),
                        depth: state.snapshot.request.actor.depth.saturating_add(1),
                    },
                )
                .await
                .map_err(store_terminal)?;
            }
            let child_runtime = self.with_tools(binding.tools.clone());
            let _child_lease = budget.reserve_child().ok_or_else(|| {
                orchestration_recovery(
                    &task.task_id,
                    "writer_recovery_concurrency",
                    "恢复 writer child 时没有可用并发额度",
                )
            })?;
            let mut child = if let Some(replay) = child_replay.as_ref() {
                debug_assert!(replay.snapshot.terminal.is_none());
                child_runtime.resume_inner(task.child_run_id.clone(), Some(budget.clone()))
            } else {
                let terminal_permit = budget.reserve_terminal_model_request().ok_or_else(|| {
                    orchestration_recovery(
                        &task.task_id,
                        "writer_recovery_capacity",
                        "恢复尚未创建的 writer child 时无法保留最终模型请求",
                    )
                })?;
                child_runtime.start_inner(
                    recovered_writer_request(state, task, arguments),
                    budget.clone(),
                    Some(terminal_permit),
                    state.model_accounting_includes_baseline,
                )
            };
            let outcome = loop {
                tokio::select! {
                    joined = &mut child.join => {
                        break joined.unwrap_or_else(|error| failed_child_outcome(
                            task,
                            RuntimeFailure::Join { message: error.to_string() },
                        ));
                    }
                    command = control.recv() => if let Some(command) = command {
                        match self.handle_control(state, command).await.map_err(store_terminal)? {
                            ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                            ControlEffect::Terminal(terminal) => {
                                if matches!(terminal, TerminalState::Interrupted) {
                                    child.control.interrupt().ok();
                                } else {
                                    child.control.cancel().ok();
                                }
                                parent_terminal = Some(terminal);
                            }
                        }
                    },
                    () = wait_for_deadline(deadline) => {
                        child.control.cancel().ok();
                        parent_terminal = Some(timeout_terminal(state, deadline));
                    }
                }
            };
            child_replay = self.store.load(&task.child_run_id).await.map_err(|error| {
                store_terminal(RuntimeFailure::Store {
                    message: error.to_string(),
                })
            })?;
            Some(outcome)
        };
        Ok(RecoveredWriterChild {
            child_replay,
            child_outcome,
            parent_terminal,
        })
    }

    async fn finish_writer_lifecycle(
        &self,
        state: &mut RunState,
        task: AgentTask,
        child_outcome: Option<AgentOutcome>,
        child_replay: Option<&RunReplay>,
        parent_terminal: Option<TerminalState>,
    ) -> Result<ToolOutcome, TerminalState> {
        let mut lifecycle = current_agent_lifecycle(state, &task.task_id)?;
        if let Some(finished) = lifecycle.finished.clone() {
            if matches!(
                finished.outcome.terminal,
                TerminalState::RecoveryRequired { .. }
            ) {
                return Err(finished.outcome.terminal);
            }
            if let Some(terminal) = parent_terminal {
                return Err(terminal);
            }
            return Ok(writer_tool_outcome(&task, &finished.outcome));
        }
        if lifecycle.result.is_none() {
            let child_outcome = child_outcome.ok_or_else(|| {
                orchestration_recovery(
                    &task.task_id,
                    "writer_recovery_child_result",
                    "writer lifecycle 缺少可汇聚的 child result",
                )
            })?;
            if !matches!(child_outcome.terminal, TerminalState::Completed { .. }) {
                self.collect_failed_writer(state, &task, child_outcome)
                    .await?;
                let finished = current_agent_lifecycle(state, &task.task_id)?
                    .finished
                    .expect("failed writer was finished")
                    .outcome;
                if matches!(finished.terminal, TerminalState::RecoveryRequired { .. }) {
                    return Err(finished.terminal);
                }
                if let Some(terminal) = parent_terminal {
                    return Err(terminal);
                }
                return Ok(writer_failure_tool_outcome(&task, &finished));
            }
            if let Err(message) = validate_writer_receipts(&task, &child_outcome.details.evidence) {
                return self
                    .finish_bound_writer_failure(
                        state,
                        &task,
                        child_outcome,
                        AgentOrchestrationError::new(
                            AgentOrchestrationErrorKind::Rejected,
                            "writer_receipt_mismatch",
                            message,
                        ),
                    )
                    .await;
            }
            let replay = match child_replay {
                Some(replay) => replay,
                None => {
                    return self
                        .finish_bound_writer_failure(
                            state,
                            &task,
                            child_outcome,
                            AgentOrchestrationError::new(
                                AgentOrchestrationErrorKind::RecoveryRequired,
                                "writer_child_missing",
                                "完成的 writer child 没有 canonical replay",
                            ),
                        )
                        .await;
                }
            };
            if lifecycle.seal.is_none() {
                self.publish(
                    state,
                    RuntimeEventKind::AgentSealPrepared {
                        task_id: task.task_id.clone(),
                        base_commit: task.workspace.base_commit.clone(),
                        writer_workspace_state_before: replay.snapshot.workspace_state.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
            let seal = if let Some(seal) = writer_seal_from_lifecycle(&lifecycle) {
                seal
            } else {
                let mut seal = match self
                    .orchestrator
                    .as_ref()
                    .expect("writer lifecycle requires orchestrator")
                    .seal_writer(&task)
                    .await
                {
                    Ok(seal) => seal,
                    Err(error) => {
                        return self
                            .finish_bound_writer_failure(state, &task, child_outcome, error)
                            .await;
                    }
                };
                let before = lifecycle
                    .seal
                    .as_ref()
                    .expect("seal was prepared above")
                    .writer_workspace_state_before
                    .clone();
                seal.writer_workspace_state.generation = before.generation.saturating_add(1);
                if let Err(message) = validate_writer_seal(&task, &seal) {
                    return self
                        .finish_bound_writer_failure(
                            state,
                            &task,
                            child_outcome,
                            AgentOrchestrationError::new(
                                AgentOrchestrationErrorKind::RecoveryRequired,
                                "writer_seal_mismatch",
                                message,
                            ),
                        )
                        .await;
                }
                self.publish(
                    state,
                    RuntimeEventKind::AgentSealCommitted {
                        task_id: task.task_id.clone(),
                        final_commit: seal.final_commit.clone(),
                        diff_sha256: seal.diff_sha256.clone(),
                        changed_files: seal.changed_files.clone(),
                        writer_workspace_state_after: seal.writer_workspace_state.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
                seal
            };
            let (checks, artifacts) = agent_checks_from_replay(replay);
            let mut collected = child_outcome;
            collected.details.workspace = Some(task.workspace.clone());
            collected.details.workspace_state = Some(seal.writer_workspace_state.clone());
            collected.details.base_commit = Some(seal.base_commit.clone());
            collected.details.final_commit = Some(seal.final_commit.clone());
            collected.details.diff_sha256 = Some(seal.diff_sha256.clone());
            collected.details.changed_files = seal.changed_files;
            collected.details.checks = checks;
            collected.details.artifacts = artifacts;
            collected.details.integration = WriterIntegrationStatus::AwaitingHost;
            collected.validate().map_err(|message| {
                orchestration_recovery(&task.task_id, "writer_result_invalid", message)
            })?;
            self.publish(
                state,
                RuntimeEventKind::AgentResultCollected {
                    task_id: task.task_id.clone(),
                    outcome: Box::new(collected),
                },
            )
            .await
            .map_err(store_terminal)?;
            lifecycle = current_agent_lifecycle(state, &task.task_id)?;
        }
        let seal = writer_seal_from_lifecycle(&lifecycle);
        let result = lifecycle
            .result
            .clone()
            .expect("writer result was recovered or collected");
        if !matches!(result.terminal, TerminalState::Completed { .. }) {
            if lifecycle.cleanup.is_none()
                || lifecycle
                    .cleanup
                    .as_ref()
                    .and_then(|cleanup| cleanup.committed.as_ref())
                    .is_none()
            {
                self.resume_writer_cleanup(state, &task, seal.as_ref())
                    .await?;
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
        } else {
            let seal = seal
                .or_else(|| writer_seal_from_lifecycle(&lifecycle))
                .ok_or_else(|| {
                    orchestration_recovery(
                        &task.task_id,
                        "writer_recovery_seal",
                        "成功 writer result 缺少 committed seal",
                    )
                })?;
            if lifecycle.integration.is_none() {
                let integration_id = OperationId::new();
                self.publish(
                    state,
                    RuntimeEventKind::AgentIntegrationPrepared {
                        task_id: task.task_id.clone(),
                        integration_id,
                        base_commit: seal.base_commit.clone(),
                        writer_commit: seal.final_commit.clone(),
                        diff_sha256: seal.diff_sha256.clone(),
                        expected_root_workspace_state: state.snapshot.workspace_state.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
            let integration = lifecycle
                .integration
                .clone()
                .expect("integration was prepared");
            if !integration.started {
                self.publish(
                    state,
                    RuntimeEventKind::AgentIntegrationStarted {
                        task_id: task.task_id.clone(),
                        integration_id: integration.integration_id.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
            let integration = lifecycle
                .integration
                .clone()
                .expect("started integration remains present");
            if integration.committed.is_none() && integration.failure.is_none() {
                match self
                    .orchestrator
                    .as_ref()
                    .expect("writer recovery requires orchestrator")
                    .integrate_writer(&task, &seal, &integration.expected_root_workspace_state)
                    .await
                {
                    Ok(mut integrated) => {
                        integrated.root_workspace_state.generation = integration
                            .expected_root_workspace_state
                            .generation
                            .saturating_add(1);
                        if integrated.root_head_commit == seal.final_commit {
                            self.publish(
                                state,
                                RuntimeEventKind::AgentIntegrationCommitted {
                                    task_id: task.task_id.clone(),
                                    integration_id: integration.integration_id.clone(),
                                    root_head_commit: integrated.root_head_commit,
                                    root_workspace_state_after: integrated.root_workspace_state,
                                },
                            )
                            .await
                            .map_err(store_terminal)?;
                        } else {
                            self.publish(
                                state,
                                RuntimeEventKind::AgentIntegrationFailed {
                                    task_id: task.task_id.clone(),
                                    integration_id: integration.integration_id.clone(),
                                    status: WriterIntegrationStatus::RecoveryRequired {
                                        reason: "集成后的 root HEAD 不是 Host-sealed writer commit"
                                            .to_owned(),
                                    },
                                    root_workspace_state: integration
                                        .expected_root_workspace_state
                                        .clone(),
                                },
                            )
                            .await
                            .map_err(store_terminal)?;
                        }
                    }
                    Err(error) => {
                        self.publish(
                            state,
                            RuntimeEventKind::AgentIntegrationFailed {
                                task_id: task.task_id.clone(),
                                integration_id: integration.integration_id.clone(),
                                status: writer_integration_failure_status(&error),
                                root_workspace_state: integration
                                    .expected_root_workspace_state
                                    .clone(),
                            },
                        )
                        .await
                        .map_err(store_terminal)?;
                    }
                }
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
            if let Some(failure) = lifecycle
                .integration
                .as_ref()
                .and_then(|integration| integration.failure.as_ref())
                .cloned()
                && lifecycle
                    .cleanup
                    .as_ref()
                    .and_then(|cleanup| cleanup.committed.as_ref())
                    .is_none()
            {
                if matches!(
                    failure.status,
                    WriterIntegrationStatus::RecoveryRequired { .. }
                ) {
                    let reason = writer_integration_failure_reason(&failure.status)
                        .unwrap_or("writer integration 需要人工恢复")
                        .to_owned();
                    self.retain_writer_for_recovery(state, &task, reason)
                        .await?;
                } else {
                    self.resume_writer_cleanup(state, &task, Some(&seal))
                        .await?;
                }
                lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            }
        }
        if lifecycle.finished.is_none() {
            let finished = recovered_finished_outcome(&lifecycle).map_err(|message| {
                orchestration_recovery(&task.task_id, "writer_recovery_finish", message)
            })?;
            let handoff = child_handoff(state, &finished);
            let accounting = self.cumulative_accounting(state, false).await;
            self.publish(
                state,
                RuntimeEventKind::ChildFinished {
                    call_id: task.call_id.clone(),
                    outcome: Box::new(finished),
                    accounting: Box::new(accounting),
                    handoff_content: handoff,
                },
            )
            .await
            .map_err(store_terminal)?;
            lifecycle = current_agent_lifecycle(state, &task.task_id)?;
        }
        let finished = lifecycle
            .finished
            .expect("writer lifecycle was finished above")
            .outcome;
        if let Some(terminal) = parent_terminal {
            return Err(terminal);
        }
        if matches!(finished.terminal, TerminalState::RecoveryRequired { .. }) {
            return Err(finished.terminal);
        }
        Ok(writer_tool_outcome(&task, &finished))
    }

    async fn resume_writer_cleanup(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
    ) -> Result<WriterCleanup, TerminalState> {
        let lifecycle = current_agent_lifecycle(state, &task.task_id)?;
        if lifecycle.cleanup.is_none() {
            return self.cleanup_writer(state, task, seal).await;
        }
        let cleanup = lifecycle.cleanup.expect("cleanup presence checked");
        if let Some(committed) = cleanup.committed {
            return Ok(WriterCleanup {
                worktree_removed: committed.worktree_removed,
                branch_removed: committed.branch_removed,
                retained_for_recovery: committed.retained_for_recovery,
                reason: committed.reason,
            });
        }
        let result = match self
            .orchestrator
            .as_ref()
            .expect("writer recovery requires orchestrator")
            .cleanup_writer(task, seal)
            .await
        {
            Ok(cleanup) => cleanup,
            Err(error) => WriterCleanup {
                worktree_removed: false,
                branch_removed: false,
                retained_for_recovery: true,
                reason: Some(format!("{}：{}", error.code, error.message)),
            },
        };
        self.publish(
            state,
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task.task_id.clone(),
                worktree_path: cleanup.worktree_path,
                branch: cleanup.branch,
                owner_token: cleanup.owner_token,
                worktree_removed: result.worktree_removed,
                branch_removed: result.branch_removed,
                retained_for_recovery: result.retained_for_recovery,
                reason: result.reason.clone(),
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(result)
    }

    async fn finish_writer_child(
        self: &Arc<Self>,
        state: &mut RunState,
        task: AgentTask,
        mut child: RuntimeRun,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        _child_lease: ChildLease,
    ) -> Result<ToolOutcome, TerminalState> {
        let mut parent_terminal = None;
        let outcome = loop {
            tokio::select! {
                joined = &mut child.join => {
                    break joined.unwrap_or_else(|error| failed_child_outcome(
                        &task,
                        RuntimeFailure::Join { message: error.to_string() },
                    ));
                }
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await.map_err(store_terminal)? {
                        ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                        ControlEffect::Terminal(terminal) => {
                            if matches!(terminal, TerminalState::Interrupted) {
                                child.control.interrupt().ok();
                            } else {
                                child.control.cancel().ok();
                            }
                            parent_terminal = Some(terminal);
                            break child.join.await.unwrap_or_else(|error| failed_child_outcome(
                                &task,
                                RuntimeFailure::Join { message: error.to_string() },
                            ));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    child.control.cancel().ok();
                    parent_terminal = Some(timeout_terminal(state, deadline));
                    break child.join.await.unwrap_or_else(|error| failed_child_outcome(
                        &task,
                        RuntimeFailure::Join { message: error.to_string() },
                    ));
                }
            }
        };

        let child_replay = self.store.load(&task.child_run_id).await.map_err(|error| {
            store_terminal(RuntimeFailure::Store {
                message: error.to_string(),
            })
        })?;
        Box::pin(self.finish_writer_lifecycle(
            state,
            task,
            Some(outcome),
            child_replay.as_ref(),
            parent_terminal,
        ))
        .await
    }

    async fn retain_writer_for_recovery(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        reason: String,
    ) -> Result<WriterCleanup, TerminalState> {
        let worktree_path = task
            .workspace
            .worktree_path
            .clone()
            .expect("validated writer task has a worktree path");
        let branch = task
            .workspace
            .branch
            .clone()
            .expect("validated writer task has a branch");
        let owner_token = task
            .workspace
            .owner_token
            .clone()
            .expect("validated writer task has an owner token");
        self.publish(
            state,
            RuntimeEventKind::AgentCleanupPrepared {
                task_id: task.task_id.clone(),
                worktree_path: worktree_path.clone(),
                branch: branch.clone(),
                owner_token: owner_token.clone(),
            },
        )
        .await
        .map_err(store_terminal)?;
        self.publish(
            state,
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task.task_id.clone(),
                worktree_path,
                branch,
                owner_token,
                worktree_removed: false,
                branch_removed: false,
                retained_for_recovery: true,
                reason: Some(reason.clone()),
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(WriterCleanup {
            worktree_removed: false,
            branch_removed: false,
            retained_for_recovery: true,
            reason: Some(reason),
        })
    }

    async fn finish_uncreated_writer(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        mut outcome: AgentOutcome,
    ) -> Result<AgentOutcome, TerminalState> {
        outcome.details = AgentResultDetails {
            summary: terminal_state_label(&outcome.terminal).to_owned(),
            ..AgentResultDetails::default()
        };
        self.publish(
            state,
            RuntimeEventKind::AgentResultCollected {
                task_id: task.task_id.clone(),
                outcome: Box::new(outcome.clone()),
            },
        )
        .await
        .map_err(store_terminal)?;
        let handoff = child_handoff(state, &outcome);
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish(
            state,
            RuntimeEventKind::ChildFinished {
                call_id: task.call_id.clone(),
                outcome: Box::new(outcome.clone()),
                accounting: Box::new(accounting),
                handoff_content: handoff,
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(outcome)
    }

    async fn finish_bound_writer_failure(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        mut outcome: AgentOutcome,
        error: AgentOrchestrationError,
    ) -> Result<ToolOutcome, TerminalState> {
        outcome.terminal = orchestration_terminal(error.clone());
        outcome.details = AgentResultDetails {
            summary: format!("{}：{}", error.code, error.message),
            ..AgentResultDetails::default()
        };
        if error.kind == AgentOrchestrationErrorKind::RecoveryRequired {
            self.collect_failed_writer_retained(
                state,
                task,
                outcome,
                format!("{}：{}", error.code, error.message),
            )
            .await?;
        } else {
            self.collect_failed_writer(state, task, outcome).await?;
        }
        let finished = current_agent_lifecycle(state, &task.task_id)?
            .finished
            .expect("failed bound writer was finished")
            .outcome;
        if matches!(finished.terminal, TerminalState::RecoveryRequired { .. }) {
            Err(finished.terminal)
        } else {
            Ok(writer_failure_tool_outcome(task, &finished))
        }
    }

    async fn collect_failed_writer(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        outcome: AgentOutcome,
    ) -> Result<(), TerminalState> {
        self.collect_failed_writer_with_retention(state, task, outcome, None)
            .await
    }

    async fn collect_failed_writer_retained(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        outcome: AgentOutcome,
        reason: String,
    ) -> Result<(), TerminalState> {
        self.collect_failed_writer_with_retention(state, task, outcome, Some(reason))
            .await
    }

    async fn collect_failed_writer_with_retention(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        mut outcome: AgentOutcome,
        retain_reason: Option<String>,
    ) -> Result<(), TerminalState> {
        outcome.details.workspace = None;
        outcome.details.workspace_state = None;
        outcome.details.base_commit = None;
        outcome.details.final_commit = None;
        outcome.details.diff_sha256 = None;
        outcome.details.changed_files.clear();
        outcome.details.integration = WriterIntegrationStatus::NotApplicable;
        self.publish(
            state,
            RuntimeEventKind::AgentResultCollected {
                task_id: task.task_id.clone(),
                outcome: Box::new(outcome.clone()),
            },
        )
        .await
        .map_err(store_terminal)?;
        let cleanup = match retain_reason {
            Some(reason) => self.retain_writer_for_recovery(state, task, reason).await?,
            None => self.cleanup_writer(state, task, None).await?,
        };
        let cleanup_recovery =
            cleanup.retained_for_recovery || !cleanup.worktree_removed || !cleanup.branch_removed;
        if cleanup_recovery {
            outcome.terminal = writer_cleanup_recovery_terminal(
                task,
                cleanup
                    .reason
                    .as_deref()
                    .unwrap_or("writer cleanup 未完整删除 worktree 与 branch"),
            );
        }
        let handoff = child_handoff(state, &outcome);
        let terminal_after_cleanup = outcome.terminal.clone();
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish(
            state,
            RuntimeEventKind::ChildFinished {
                call_id: task.call_id.clone(),
                outcome: Box::new(outcome),
                accounting: Box::new(accounting),
                handoff_content: handoff,
            },
        )
        .await
        .map_err(store_terminal)?;
        if cleanup_recovery {
            return Err(terminal_after_cleanup);
        }
        Ok(())
    }

    async fn cleanup_writer(
        &self,
        state: &mut RunState,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
    ) -> Result<WriterCleanup, TerminalState> {
        let worktree_path = task
            .workspace
            .worktree_path
            .clone()
            .expect("validated writer task has a worktree path");
        let branch = task
            .workspace
            .branch
            .clone()
            .expect("validated writer task has a branch");
        let owner_token = task
            .workspace
            .owner_token
            .clone()
            .expect("validated writer task has an owner token");
        self.publish(
            state,
            RuntimeEventKind::AgentCleanupPrepared {
                task_id: task.task_id.clone(),
                worktree_path: worktree_path.clone(),
                branch: branch.clone(),
                owner_token: owner_token.clone(),
            },
        )
        .await
        .map_err(store_terminal)?;
        let cleanup = match self
            .orchestrator
            .as_ref()
            .expect("writer cannot start without an orchestrator")
            .cleanup_writer(task, seal)
            .await
        {
            Ok(cleanup) => cleanup,
            Err(error) => {
                let reason = format!("{}：{}", error.code, error.message);
                let cleanup = WriterCleanup {
                    worktree_removed: false,
                    branch_removed: false,
                    retained_for_recovery: true,
                    reason: Some(reason.clone()),
                };
                self.publish(
                    state,
                    RuntimeEventKind::AgentCleanupCommitted {
                        task_id: task.task_id.clone(),
                        worktree_path,
                        branch,
                        owner_token,
                        worktree_removed: false,
                        branch_removed: false,
                        retained_for_recovery: true,
                        reason: Some(reason.clone()),
                    },
                )
                .await
                .map_err(store_terminal)?;
                return Ok(cleanup);
            }
        };
        self.publish(
            state,
            RuntimeEventKind::AgentCleanupCommitted {
                task_id: task.task_id.clone(),
                worktree_path,
                branch,
                owner_token,
                worktree_removed: cleanup.worktree_removed,
                branch_removed: cleanup.branch_removed,
                retained_for_recovery: cleanup.retained_for_recovery,
                reason: cleanup.reason.clone(),
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(cleanup)
    }

    async fn join_children(
        &self,
        state: &mut RunState,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<(), TerminalState> {
        while !state.pending_children.is_empty() {
            let mut child = state.pending_children.remove(0);
            let outcome = loop {
                tokio::select! {
                    joined = &mut child.join => break joined.unwrap_or_else(|error| AgentOutcome {
                        run_id: child.run_id.clone(),
                        parent_run_id: Some(state.run_id().clone()),
                        terminal: TerminalState::Failed {
                            failure: RuntimeFailure::Join { message: error.to_string() },
                        },
                        accounting: incomplete_accounting(),
                        runtime_model_requests: 0,
                        runtime_retries: 0,
                        tool_calls: 0,
                        details: AgentResultDetails::default(),
                    }),
                    command = control.recv() => if let Some(command) = command {
                        match self.handle_control(state, command).await.map_err(store_terminal)? {
                            ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                            ControlEffect::Terminal(terminal) => {
                                if matches!(terminal, TerminalState::Interrupted) {
                                    child.control.interrupt().ok();
                                    for pending in &state.pending_children { pending.control.interrupt().ok(); }
                                } else {
                                    child.control.cancel().ok();
                                    for pending in &state.pending_children { pending.control.cancel().ok(); }
                                }
                                let task_id = child.task_id.clone();
                                let call_id = child.call_id.clone();
                                let run_id = child.run_id.clone();
                                let outcome = child.join.await.unwrap_or_else(|error| {
                                    failed_child_outcome(
                                        &current_agent_lifecycle(state, &task_id)
                                            .expect("pending read-only child has a durable task")
                                            .task,
                                        RuntimeFailure::Join { message: error.to_string() },
                                    )
                                });
                                self.settle_readonly_child(
                                    state, task_id, call_id, run_id, outcome,
                                )
                                .await?;
                                self.join_remaining_children(state).await?;
                                return Err(terminal);
                            }
                        }
                    },
                    () = wait_for_deadline(deadline) => {
                        child.control.cancel().ok();
                        for pending in &state.pending_children { pending.control.cancel().ok(); }
                        let task_id = child.task_id.clone();
                        let call_id = child.call_id.clone();
                        let run_id = child.run_id.clone();
                        let outcome = child.join.await.unwrap_or_else(|error| {
                            failed_child_outcome(
                                &current_agent_lifecycle(state, &task_id)
                                    .expect("pending read-only child has a durable task")
                                    .task,
                                RuntimeFailure::Join { message: error.to_string() },
                            )
                        });
                        self.settle_readonly_child(state, task_id, call_id, run_id, outcome)
                            .await?;
                        self.join_remaining_children(state).await?;
                        return Err(timeout_terminal(state, deadline));
                    }
                }
            };
            self.settle_readonly_child(state, child.task_id, child.call_id, child.run_id, outcome)
                .await?;
        }
        Ok(())
    }

    async fn cancel_children(&self, state: &mut RunState) -> Result<(), TerminalState> {
        for child in &state.pending_children {
            child.control.cancel().ok();
        }
        self.join_remaining_children(state).await
    }

    async fn settled_terminal(
        &self,
        state: &mut RunState,
        terminal: TerminalState,
    ) -> TerminalState {
        self.settle_children_for(state, &terminal)
            .await
            .err()
            .unwrap_or(terminal)
    }

    async fn settle_children_for(
        &self,
        state: &mut RunState,
        terminal: &TerminalState,
    ) -> Result<(), TerminalState> {
        if matches!(terminal, TerminalState::Interrupted) {
            for child in &state.pending_children {
                child.control.interrupt().ok();
            }
            self.join_remaining_children(state).await
        } else {
            self.cancel_children(state).await
        }
    }

    async fn join_remaining_children(&self, state: &mut RunState) -> Result<(), TerminalState> {
        while !state.pending_children.is_empty() {
            let child = state.pending_children.remove(0);
            let task_id = child.task_id;
            let call_id = child.call_id;
            let run_id = child.run_id;
            let outcome = child.join.await.unwrap_or_else(|error| {
                failed_child_outcome(
                    &current_agent_lifecycle(state, &task_id)
                        .expect("pending read-only child has a durable task")
                        .task,
                    RuntimeFailure::Join {
                        message: error.to_string(),
                    },
                )
            });
            self.settle_readonly_child(state, task_id, call_id, run_id, outcome)
                .await?;
        }
        Ok(())
    }

    async fn settle_readonly_child(
        &self,
        state: &mut RunState,
        task_id: AgentTaskId,
        call_id: String,
        run_id: RunId,
        mut outcome: AgentOutcome,
    ) -> Result<(), TerminalState> {
        let lifecycle = current_agent_lifecycle(state, &task_id)?;
        if lifecycle.task.workspace.access != AgentWorkspaceAccess::ReadOnly
            || lifecycle.task.call_id != call_id
            || lifecycle.task.child_run_id != run_id
        {
            return Err(orchestration_recovery(
                &task_id,
                "readonly_child_identity",
                "待汇聚的只读子 Agent 与持久任务身份不一致",
            ));
        }
        if lifecycle.finished.is_some() {
            return Ok(());
        }
        outcome.details.workspace = Some(lifecycle.task.workspace.clone());
        outcome.details.base_commit = None;
        outcome.details.final_commit = None;
        outcome.details.diff_sha256 = None;
        outcome.details.changed_files.clear();
        outcome.details.integration = WriterIntegrationStatus::NotApplicable;
        if outcome.details.summary.trim().is_empty() {
            outcome.details.summary = terminal_state_label(&outcome.terminal).to_owned();
        }
        outcome.validate().map_err(|message| {
            orchestration_recovery(&task_id, "readonly_child_result_invalid", message)
        })?;
        if lifecycle.result.is_none() {
            self.publish(
                state,
                RuntimeEventKind::AgentResultCollected {
                    task_id: task_id.clone(),
                    outcome: Box::new(outcome.clone()),
                },
            )
            .await
            .map_err(store_terminal)?;
        }
        let handoff = readonly_child_handoff(state, &outcome);
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish(
            state,
            RuntimeEventKind::ChildFinished {
                call_id,
                outcome: Box::new(outcome),
                accounting: Box::new(accounting),
                handoff_content: handoff,
            },
        )
        .await
        .map_err(store_terminal)?;
        Ok(())
    }

    async fn drain_controls(
        &self,
        state: &mut RunState,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
    ) -> Result<Option<TerminalState>, RuntimeFailure> {
        loop {
            match control.try_recv() {
                Ok(command) => match self.handle_control(state, command).await? {
                    ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                    ControlEffect::Terminal(terminal) => return Ok(Some(terminal)),
                },
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    return Ok(None);
                }
            }
        }
    }

    async fn handle_control(
        &self,
        state: &mut RunState,
        command: ControlCommand,
    ) -> Result<ControlEffect, RuntimeFailure> {
        match command {
            ControlCommand::Steer {
                command_id,
                content,
                ack,
            } => {
                let durable = DurableCommand::Steer {
                    content: content.clone(),
                };
                if let Some(receipt) =
                    command_receipt_result(&state.snapshot, &command_id, &durable)
                {
                    acknowledge(ack, receipt);
                    return Ok(ControlEffect::Continue);
                }
                match self
                    .publish(
                        state,
                        RuntimeEventKind::SteerQueued {
                            command_id,
                            content,
                        },
                    )
                    .await
                {
                    Ok(stored) => {
                        acknowledge(ack, Ok(stored.sequence));
                        Ok(ControlEffect::Continue)
                    }
                    Err(failure) => {
                        acknowledge(
                            ack,
                            Err(ControlError::Store {
                                message: format!("{failure:?}"),
                            }),
                        );
                        Err(failure)
                    }
                }
            }
            ControlCommand::Stop {
                command_id,
                action,
                ack,
            } => {
                let durable = DurableCommand::Stop { action };
                if let Some(receipt) =
                    command_receipt_result(&state.snapshot, &command_id, &durable)
                {
                    return match receipt {
                        Ok(sequence) => {
                            acknowledge(ack, Ok(sequence));
                            Ok(ControlEffect::Terminal(terminal_for_control(action)))
                        }
                        Err(error) => {
                            acknowledge(ack, Err(error));
                            Ok(ControlEffect::Continue)
                        }
                    };
                }
                if let Some(pending) = &state.snapshot.pending_control {
                    acknowledge(ack, Err(ControlError::RunFinished));
                    return Ok(ControlEffect::Terminal(terminal_for_control(
                        pending.action,
                    )));
                }
                match self
                    .publish(
                        state,
                        RuntimeEventKind::ControlRequested { command_id, action },
                    )
                    .await
                {
                    Ok(stored) => {
                        acknowledge(ack, Ok(stored.sequence));
                        Ok(ControlEffect::Terminal(terminal_for_control(action)))
                    }
                    Err(failure) => {
                        acknowledge(
                            ack,
                            Err(ControlError::Store {
                                message: format!("{failure:?}"),
                            }),
                        );
                        Err(failure)
                    }
                }
            }
            ControlCommand::ResolveInteraction {
                command_id,
                interaction_id,
                response,
                ack,
            } => {
                let durable = DurableCommand::ResolveInteraction {
                    interaction_id: interaction_id.clone(),
                    response: response.clone(),
                };
                if let Some(receipt) =
                    command_receipt_result(&state.snapshot, &command_id, &durable)
                {
                    match receipt {
                        Ok(sequence) => {
                            let _ = ack.send(Ok(sequence));
                            return Ok(ControlEffect::InteractionResolved(response));
                        }
                        Err(error) => {
                            let _ = ack.send(Err(error));
                            return Ok(ControlEffect::Continue);
                        }
                    }
                }
                let Some(interaction) = state
                    .snapshot
                    .pending_tool
                    .as_ref()
                    .and_then(|pending| pending.interaction.as_ref())
                    .cloned()
                else {
                    let _ = ack.send(Err(ControlError::InteractionNotPending));
                    return Ok(ControlEffect::Continue);
                };
                if interaction.request.interaction_id != interaction_id {
                    let _ = ack.send(Err(ControlError::InteractionMismatch));
                    return Ok(ControlEffect::Continue);
                }
                if interaction.response.is_some() {
                    let _ = ack.send(Err(ControlError::InteractionAlreadyResolved));
                    return Ok(ControlEffect::Continue);
                }
                if let Err(message) = interaction.request.validate_response(&response) {
                    let _ = ack.send(Err(ControlError::InvalidInteractionResponse { message }));
                    return Ok(ControlEffect::Continue);
                }
                match self
                    .publish(
                        state,
                        RuntimeEventKind::InteractionResolved {
                            command_id,
                            interaction_id,
                            response: response.clone(),
                        },
                    )
                    .await
                {
                    Ok(stored) => {
                        let _ = ack.send(Ok(stored.sequence));
                        Ok(ControlEffect::InteractionResolved(response))
                    }
                    Err(failure) => {
                        let _ = ack.send(Err(ControlError::Store {
                            message: format!("{failure:?}"),
                        }));
                        Err(failure)
                    }
                }
            }
        }
    }

    async fn flush_pending_steers(&self, state: &mut RunState) -> Result<(), RuntimeFailure> {
        while let Some(pending) = state.snapshot.pending_steers.first().cloned() {
            self.publish(
                state,
                RuntimeEventKind::SteerApplied {
                    command_id: pending.command_id,
                    content: pending.content,
                },
            )
            .await?;
        }
        Ok(())
    }

    async fn publish(
        &self,
        state: &mut RunState,
        event: RuntimeEventKind,
    ) -> Result<StoredRuntimeEvent, RuntimeFailure> {
        self.publish_inner(state, event, None).await
    }

    async fn publish_with_model_permit(
        &self,
        state: &mut RunState,
        event: RuntimeEventKind,
        permit: ModelRequestPermit,
    ) -> Result<StoredRuntimeEvent, RuntimeFailure> {
        self.publish_inner(state, event, Some(permit)).await
    }

    async fn publish_inner(
        &self,
        state: &mut RunState,
        event: RuntimeEventKind,
        permit: Option<ModelRequestPermit>,
    ) -> Result<StoredRuntimeEvent, RuntimeFailure> {
        let pending = match event {
            RuntimeEventKind::Terminal { outcome } => PendingRuntimeEvent::terminal(*outcome),
            event => PendingRuntimeEvent::new(event),
        };
        let first = self.store.append(&state.lease, pending.clone()).await;
        let stored = match first {
            Ok(stored) => stored,
            Err(RunStoreError::Backend { .. }) => self
                .store
                .append(&state.lease, pending)
                .await
                .map_err(|error| RuntimeFailure::Store {
                    message: error.to_string(),
                })?,
            Err(error) => {
                return Err(RuntimeFailure::Store {
                    message: error.to_string(),
                });
            }
        };
        if let Some(permit) = permit {
            permit.consume();
        }
        apply_event(&mut state.snapshot, &stored).map_err(|error| RuntimeFailure::Store {
            message: format!(
                "persisted event could not update the live canonical projection: {error}"
            ),
        })?;
        self.sink.emit(stored.clone()).await;
        Ok(stored)
    }

    async fn finalize(
        &self,
        state: &mut RunState,
        mut terminal: TerminalState,
        _budget: &RuntimeBudget,
    ) -> AgentOutcome {
        let root = state.snapshot.request.actor.kind == AgentActorKind::Root;
        if root {
            terminal = match self.cleanup_integrated_writers(state).await {
                Ok(Some(cleanup_terminal)) | Err(cleanup_terminal) => cleanup_terminal,
                Ok(None) => terminal,
            };
        }
        let mut accounting = self.cumulative_accounting(state, root).await;
        let model_request_unsettled = state
            .snapshot
            .pending_model
            .as_ref()
            .is_some_and(|pending| pending.state == DurableActionState::InFlight);
        let recovery_may_hide_model_billing = matches!(
            &terminal,
            TerminalState::RecoveryRequired {
                ambiguity: RecoveryAmbiguity {
                    phase: RecoveryAmbiguityPhase::ModelRequest,
                    ..
                }
            }
        );
        if model_request_unsettled || recovery_may_hide_model_billing {
            accounting.complete = false;
            accounting.usage_complete = false;
            accounting.usage_incomplete = true;
            accounting.billing_unknown = true;
            let minimum_unknown_attempts = state
                .accounting_epoch_baseline
                .billing_unknown_attempts
                .saturating_add(1);
            accounting.billing_unknown_attempts = accounting
                .billing_unknown_attempts
                .max(minimum_unknown_attempts);
        }
        accounting.runtime_retries = accounting
            .runtime_retries
            .saturating_add(u64::from(state.snapshot.runtime_retries));
        let budget_can_replace = matches!(
            &terminal,
            TerminalState::Completed { .. }
                | TerminalState::Failed {
                    failure: RuntimeFailure::Model { .. }
                }
        );
        if root
            && budget_can_replace
            && (accounting.budget_exhausted || accounting.exhausted_denied > 0)
        {
            terminal = TerminalState::Failed {
                failure: RuntimeFailure::ApiRequestBudgetExceeded {
                    limit: accounting
                        .hard_request_limit
                        .unwrap_or(state.snapshot.request.limits.max_model_requests),
                },
            };
        }
        let summary = match &terminal {
            TerminalState::Completed { message, .. } => message.clone(),
            other => terminal_state_label(other).to_owned(),
        };
        let read_only_workspace = state
            .snapshot
            .request
            .agent_task
            .as_ref()
            .filter(|task| task.workspace.access == AgentWorkspaceAccess::ReadOnly)
            .map(|task| task.workspace.clone());
        let details = AgentResultDetails {
            summary,
            evidence: state.snapshot.evidence_receipts.clone(),
            workspace_state: read_only_workspace
                .as_ref()
                .map(|_| state.snapshot.workspace_state.clone()),
            workspace: read_only_workspace,
            ..AgentResultDetails::default()
        };
        let outcome = AgentOutcome {
            run_id: state.run_id().clone(),
            parent_run_id: state.snapshot.request.parent_run_id.clone(),
            terminal,
            accounting,
            runtime_model_requests: state.snapshot.runtime_model_requests,
            runtime_retries: state.snapshot.runtime_retries,
            tool_calls: state.snapshot.tool_calls,
            details,
        };
        if let Err(error) = self
            .publish(
                state,
                RuntimeEventKind::Terminal {
                    outcome: Box::new(outcome.clone()),
                },
            )
            .await
        {
            let error = match error {
                RuntimeFailure::Store { message } => message,
                other => format!("{other:?}"),
            };
            if let Ok(Some(replay)) = self.store.load(state.run_id()).await
                && let Some(existing) = replay.snapshot.terminal
            {
                return existing;
            }
            let _ = self.store.release(&state.lease).await;
            return AgentOutcome {
                terminal: TerminalState::Failed {
                    failure: RuntimeFailure::Store {
                        message: format!("terminal_persist_failed：{error}"),
                    },
                },
                ..outcome
            };
        }
        state.snapshot.terminal.clone().unwrap_or(outcome)
    }

    async fn cleanup_integrated_writers(
        &self,
        state: &mut RunState,
    ) -> Result<Option<TerminalState>, TerminalState> {
        let tasks = state
            .snapshot
            .agent_tasks
            .iter()
            .filter(|lifecycle| {
                lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                    && lifecycle
                        .integration
                        .as_ref()
                        .and_then(|integration| integration.committed.as_ref())
                        .is_some()
                    && lifecycle
                        .cleanup
                        .as_ref()
                        .and_then(|cleanup| cleanup.committed.as_ref())
                        .is_none()
            })
            .map(|lifecycle| lifecycle.task.clone())
            .collect::<Vec<_>>();
        if !tasks.is_empty() && self.orchestrator.is_none() {
            let task = &tasks[0];
            return Err(orchestration_recovery(
                &task.task_id,
                "writer_cleanup_orchestrator_unavailable",
                "root 终态前无法清理已集成 writer：Orchestrator 不可用",
            ));
        }
        for task in tasks {
            let lifecycle = current_agent_lifecycle(state, &task.task_id)?;
            let seal = writer_seal_from_lifecycle(&lifecycle).ok_or_else(|| {
                orchestration_recovery(
                    &task.task_id,
                    "writer_cleanup_missing_seal",
                    "root 终态前清理已集成 writer 时缺少 Host seal",
                )
            })?;
            let cleanup = self
                .resume_writer_cleanup(state, &task, Some(&seal))
                .await?;
            if cleanup.retained_for_recovery || !cleanup.worktree_removed || !cleanup.branch_removed
            {
                return Ok(Some(writer_cleanup_recovery_terminal(
                    &task,
                    cleanup
                        .reason
                        .as_deref()
                        .unwrap_or("root 终态前 writer cleanup 未完整完成"),
                )));
            }
        }
        Ok(None)
    }

    async fn observe_workspace_state(
        &self,
        state: &RunState,
        force_advance: bool,
    ) -> WorkspaceState {
        let revision = match self.tools.observe_workspace_revision().await {
            Ok(sha256) => WorkspaceRevision::Known { sha256 },
            Err(error) => WorkspaceRevision::Unknown {
                reason: format!("{}：{}", error.code, error.message),
            },
        };
        let unchanged = matches!(
            (&state.snapshot.workspace_state.revision, &revision),
            (
                WorkspaceRevision::Known { sha256: previous },
                WorkspaceRevision::Known { sha256: current }
            ) if previous == current
        );
        WorkspaceState {
            generation: if force_advance || !unchanged {
                state.snapshot.workspace_state.generation.saturating_add(1)
            } else {
                state.snapshot.workspace_state.generation
            },
            revision,
        }
    }

    async fn accept_completion_candidate(
        &self,
        state: &mut RunState,
        candidate: CompletionCandidate,
    ) -> Result<(String, CompletionDecision), CompletionReviewError> {
        let contract = state
            .snapshot
            .request
            .task_contract
            .clone()
            .ok_or_else(|| {
                CompletionReviewError::Blocked("Agent 运行没有冻结 TaskContract".to_owned())
            })?;
        if candidate.generation_id != contract.generation_id {
            return Err(CompletionReviewError::Blocked(
                "完成候选不属于当前任务 generation".to_owned(),
            ));
        }
        self.reconcile_workspace(state)
            .await
            .map_err(CompletionReviewError::Blocked)?;
        if let Some(rejection) = &state.snapshot.last_completion_rejection
            && rejection.candidate_id == candidate.id
        {
            return Err(CompletionReviewError::Blocked(rejection.reason.clone()));
        }
        if let Some(failure) = &state.snapshot.last_host_verification_failure
            && failure.workspace_state == state.snapshot.workspace_state
        {
            return Err(CompletionReviewError::Blocked(
                "上一次 Host verifier 失败后工作区没有发生写入；拒绝在同一 revision 重复验证"
                    .to_owned(),
            ));
        }

        let has_verifier_acceptance = contract
            .definition
            .acceptance
            .iter()
            .any(|acceptance| matches!(acceptance, TaskAcceptance::Verifier { .. }));
        let mut satisfied = Vec::with_capacity(contract.definition.acceptance.len());
        for acceptance in &contract.definition.acceptance {
            match acceptance {
                TaskAcceptance::Host { id, .. } => {
                    satisfied.push(AcceptanceSatisfaction::Host {
                        acceptance_id: id.clone(),
                    });
                }
                TaskAcceptance::Verifier { id, verifier, .. } => {
                    let receipt = if let Some(receipt) = state
                        .snapshot
                        .evidence_receipts
                        .iter()
                        .find(|receipt| {
                            receipt.generation_id == contract.generation_id
                                && receipt.acceptance_id == *id
                                && receipt.verifier == *verifier
                                && receipt.workspace_state == state.snapshot.workspace_state
                        })
                        .cloned()
                    {
                        receipt
                    } else {
                        self.run_host_verifier(state, &candidate, id, verifier)
                            .await?
                    };
                    satisfied.push(AcceptanceSatisfaction::Evidence {
                        acceptance_id: id.clone(),
                        receipt_id: receipt.id,
                    });
                }
            }
        }

        if has_verifier_acceptance {
            self.reconcile_workspace(state)
                .await
                .map_err(CompletionReviewError::Blocked)?;
        }
        let decision = CompletionDecision {
            candidate_id: candidate.id,
            generation_id: contract.generation_id,
            workspace_state: state.snapshot.workspace_state.clone(),
            satisfied,
        };
        decision
            .validate()
            .map_err(CompletionReviewError::Blocked)?;
        Ok((candidate.message, decision))
    }

    async fn reconcile_workspace(&self, state: &mut RunState) -> Result<(), String> {
        let observed = self.observe_workspace_state(state, false).await;
        if observed != state.snapshot.workspace_state {
            self.publish(
                state,
                RuntimeEventKind::WorkspaceObserved {
                    workspace_state: observed,
                },
            )
            .await
            .map_err(|failure| format!("无法提交工作区观察：{failure:?}"))?;
        }
        Ok(())
    }

    async fn run_host_verifier(
        &self,
        state: &mut RunState,
        candidate: &CompletionCandidate,
        acceptance_id: &AcceptanceId,
        verifier: &VerifierSpec,
    ) -> Result<EvidenceReceipt, CompletionReviewError> {
        let verification_id = state
            .snapshot
            .pending_host_verification
            .as_ref()
            .map(|pending| pending.verification_id.clone())
            .unwrap_or_else(|| {
                VerificationId::from(format!(
                    "host-verification:{}:{}",
                    candidate.id.0, acceptance_id.0
                ))
            });
        if state.snapshot.pending_host_verification.is_none() {
            self.publish(
                state,
                RuntimeEventKind::HostVerificationPrepared {
                    verification_id: verification_id.clone(),
                    candidate: candidate.clone(),
                    acceptance_id: acceptance_id.clone(),
                    verifier: verifier.clone(),
                    workspace_state_before: state.snapshot.workspace_state.clone(),
                },
            )
            .await
            .map_err(|failure| {
                CompletionReviewError::Blocked(format!("无法准备 Host verifier：{failure:?}"))
            })?;
        }
        let pending = state
            .snapshot
            .pending_host_verification
            .clone()
            .ok_or_else(|| {
                CompletionReviewError::Blocked(
                    "Host verifier preparation was not persisted".to_owned(),
                )
            })?;
        if pending.candidate != *candidate
            || pending.acceptance_id != *acceptance_id
            || pending.verifier != *verifier
            || pending.workspace_state_before != state.snapshot.workspace_state
        {
            return Err(CompletionReviewError::Blocked(
                "持久化 Host verifier 与当前完成候选不匹配".to_owned(),
            ));
        }
        if pending.state == DurableActionState::Prepared {
            self.publish(
                state,
                RuntimeEventKind::HostVerificationStarted {
                    verification_id: verification_id.clone(),
                },
            )
            .await
            .map_err(|failure| {
                CompletionReviewError::Blocked(format!("无法启动 Host verifier：{failure:?}"))
            })?;
        }

        let invocation = ToolInvocation {
            run_id: state.run_id().clone(),
            call_id: format!("host:{}", verification_id.0),
            name: verifier.verifier_id.clone(),
            arguments: ToolArguments::from_value(verifier.parameters.clone()),
        };
        let outcome = self
            .tools
            .execute(invocation, CancellationToken::default())
            .await
            .unwrap_or_else(|error| {
                ToolOutcome::transport_failure(format!("{}：{}", error.code, error.message))
            });
        let workspace_state_after = self.observe_workspace_state(state, true).await;
        let receipt = seal_evidence_receipt(
            state,
            &verification_id,
            acceptance_id,
            verifier,
            &outcome,
            &workspace_state_after,
        );
        self.publish(
            state,
            RuntimeEventKind::HostVerificationCommitted {
                verification_id: verification_id.clone(),
                outcome: Box::new(outcome.clone()),
                receipt: receipt.clone(),
                workspace_state_after,
            },
        )
        .await
        .map_err(|failure| {
            CompletionReviewError::Blocked(format!("无法提交 Host verifier 结果：{failure:?}"))
        })?;
        let Some(receipt) = receipt else {
            let rejection = CompletionRejection {
                candidate_id: candidate.id.clone(),
                unmet_acceptance_ids: vec![acceptance_id.clone()],
                reason: format!(
                    "Host verifier '{}' 未产生与当前任务和工作区精确匹配的通过证据",
                    verifier.verifier_id
                ),
            };
            self.publish(
                state,
                RuntimeEventKind::CompletionRejected {
                    rejection: rejection.clone(),
                },
            )
            .await
            .map_err(|failure| {
                CompletionReviewError::Blocked(format!("无法提交完成拒绝：{failure:?}"))
            })?;
            return Err(CompletionReviewError::Retry(rejection.reason));
        };
        Ok(receipt)
    }
}

#[derive(Debug)]
struct RunState {
    snapshot: RunSnapshot,
    lease: RunLease,
    accounting_epoch_baseline: ModelAccounting,
    /// Fresh runs use the same physical ledger that supplied the durable
    /// baseline, whereas a resumed run opens a new process-local ledger.
    model_accounting_includes_baseline: bool,
    started_unix_ms: u64,
    pending_children: Vec<PendingChild>,
    recovery_model: Option<PendingModelAction>,
    recovery_tool: Option<PendingToolAction>,
    recovery_output: Option<ModelOutput>,
    recovery_failure: Option<StoppedModelFailure>,
    recovered_child_ids: Vec<RunId>,
    terminal_model_request: Option<ModelRequestPermit>,
}

impl RunState {
    fn run_id(&self) -> &RunId {
        self.snapshot
            .request
            .run_id
            .as_ref()
            .expect("a replayed run snapshot always has a run id")
    }

    fn recovery_ambiguity(&self) -> Option<RecoveryAmbiguity> {
        if let Some(pending) = self
            .recovery_model
            .as_ref()
            .filter(|pending| pending.state == DurableActionState::InFlight)
        {
            return Some(RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ModelRequest,
                action_id: pending.attempt_id.0.clone(),
                message: "进程在 DeepSeek 请求进入传输层后、响应原子提交前停止；为避免重复请求和重复计费，运行未自动重发。".to_owned(),
            });
        }
        if let Some(pending) = self
            .recovery_tool
            .as_ref()
            .filter(|pending| pending.state == DurableActionState::InFlight)
            .filter(|pending| {
                pending.invocation.name != AGENT_TOOL_NAME
                    || !self.snapshot.agent_tasks.iter().any(|lifecycle| {
                        lifecycle.task.call_id == pending.invocation.call_id
                            && writer_tool_recovery_is_safe(lifecycle)
                    })
            })
        {
            return Some(RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ToolExecution,
                action_id: pending.operation_id.0.clone(),
                message: format!(
                    "进程在工具 '{}' 开始后、结果原子提交前停止；无法证明副作用是否已发生，因此未自动重跑。",
                    pending.invocation.name
                ),
            });
        }
        if let Some(pending) = self
            .snapshot
            .pending_host_verification
            .as_ref()
            .filter(|pending| pending.state == DurableActionState::InFlight)
        {
            return Some(RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::HostVerification,
                action_id: pending.verification_id.0.clone(),
                message: "进程在 Host verifier 开始后、结果与证据回执原子提交前停止；验证命令可能有副作用，因此未自动重跑。".to_owned(),
            });
        }
        self.recovered_child_ids
            .iter()
            .find(|child| {
                !self.snapshot.agent_tasks.iter().any(|lifecycle| {
                    &lifecycle.task.child_run_id == *child
                        && lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
                        && lifecycle.finished.is_none()
                })
            })
            .map(|child| RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ChildRun,
                action_id: child.0.clone(),
                message: "父进程停止时仍有未汇聚的子 Agent；本阶段不会猜测或重复启动子任务。"
                    .to_owned(),
            })
    }
}

#[derive(Debug)]
struct PendingChild {
    task_id: AgentTaskId,
    call_id: String,
    run_id: RunId,
    control: AgentControl,
    join: JoinHandle<AgentOutcome>,
    _lease: ChildLease,
}

struct RecoveredWriterChild {
    child_replay: Option<RunReplay>,
    child_outcome: Option<AgentOutcome>,
    parent_terminal: Option<TerminalState>,
}

fn writer_tool_recovery_is_safe(lifecycle: &AgentTaskLifecycle) -> bool {
    lifecycle.task.workspace.access == AgentWorkspaceAccess::IsolatedWrite
        && lifecycle.finished.as_ref().is_none_or(|finished| {
            matches!(finished.outcome.terminal, TerminalState::Completed { .. })
        })
}

#[derive(Debug)]
struct ModelTurnOutput {
    content: String,
    tool_calls: Vec<ModelToolCall>,
    finish_reason: ModelFinishReason,
    advertised_tool_names: Vec<String>,
}

impl ModelTurnOutput {
    fn new(output: ModelOutput, advertised_tool_names: Vec<String>) -> Self {
        Self {
            content: output.content,
            tool_calls: output.tool_calls,
            finish_reason: output.finish_reason,
            advertised_tool_names,
        }
    }
}

enum ModelAttemptControl {
    Output(ModelOutput),
    Failed {
        error: ModelPortError,
        actionable_output: bool,
    },
    Terminal(TerminalState),
}

enum ModelFailurePlan {
    Stop {
        reason: ModelRetryStopReason,
        terminal: TerminalState,
    },
    Retry {
        prepared: PreparedModelRetry,
        permit: ModelRequestPermit,
    },
}

enum ModelTurnControl {
    Output(ModelTurnOutput),
    Terminal(TerminalState),
}

enum CompletionReviewError {
    Retry(String),
    Blocked(String),
}

enum ContextCompactionControl {
    NotNeeded,
    Committed,
}

enum ModelEventPoll {
    Event(Option<Result<ModelStreamEvent, ModelPortError>>),
    Idle,
}

#[derive(Debug)]
struct RuntimeBudget {
    limits: RunLimits,
    model_requests: AtomicU32,
    tools: AtomicU32,
    children: AtomicU32,
}

impl RuntimeBudget {
    fn new(limits: RunLimits, model_requests: u32, tools: u32) -> Self {
        Self {
            limits,
            model_requests: AtomicU32::new(model_requests),
            tools: AtomicU32::new(tools),
            children: AtomicU32::new(0),
        }
    }

    fn reserve_model_request(self: &Arc<Self>) -> Option<ModelRequestPermit> {
        reserve(&self.model_requests, self.limits.max_model_requests).then(|| ModelRequestPermit {
            budget: self.clone(),
            consumed: false,
        })
    }

    fn reserve_terminal_model_request(self: &Arc<Self>) -> Option<ModelRequestPermit> {
        self.reserve_model_request()
    }

    fn has_unreserved_model_request(&self) -> bool {
        self.model_requests.load(Ordering::Acquire) < self.limits.max_model_requests
    }

    fn reserve_tool(&self) -> bool {
        reserve(&self.tools, self.limits.max_tool_calls)
    }

    fn absorb_recovered(&self, model_requests: u32, tools: u32) -> bool {
        if !add_with_limit(
            &self.model_requests,
            model_requests,
            self.limits.max_model_requests,
        ) {
            return false;
        }
        if add_with_limit(&self.tools, tools, self.limits.max_tool_calls) {
            true
        } else {
            let previous = self
                .model_requests
                .fetch_sub(model_requests, Ordering::AcqRel);
            debug_assert!(previous >= model_requests);
            false
        }
    }

    fn reserve_child(self: &Arc<Self>) -> Option<ChildLease> {
        reserve(&self.children, self.limits.max_concurrent_children).then(|| ChildLease {
            budget: self.clone(),
        })
    }
}

#[derive(Debug)]
struct ModelRequestPermit {
    budget: Arc<RuntimeBudget>,
    consumed: bool,
}

impl ModelRequestPermit {
    fn consume(mut self) {
        self.consumed = true;
    }
}

impl Drop for ModelRequestPermit {
    fn drop(&mut self) {
        if !self.consumed {
            let previous = self.budget.model_requests.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "model request permit accounting underflow");
        }
    }
}

#[derive(Debug)]
struct ChildLease {
    budget: Arc<RuntimeBudget>,
}

impl Drop for ChildLease {
    fn drop(&mut self) {
        let previous = self.budget.children.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "child lease accounting underflow");
    }
}

fn reserve(counter: &AtomicU32, limit: u32) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}

fn add_with_limit(counter: &AtomicU32, amount: u32, limit: u32) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(amount).filter(|next| *next <= limit)
        })
        .is_ok()
}

enum ControlEffect {
    Continue,
    Terminal(TerminalState),
    InteractionResolved(UserInteractionResponse),
}

enum InteractionWaitResult {
    Resolved(UserInteractionResponse),
    Terminal(TerminalState),
}

enum ControlCommand {
    Steer {
        command_id: CommandId,
        content: String,
        ack: Option<oneshot::Sender<Result<u64, ControlError>>>,
    },
    Stop {
        command_id: CommandId,
        action: DurableControlAction,
        ack: Option<oneshot::Sender<Result<u64, ControlError>>>,
    },
    ResolveInteraction {
        command_id: CommandId,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
        ack: oneshot::Sender<Result<u64, ControlError>>,
    },
}

fn acknowledge(
    ack: Option<oneshot::Sender<Result<u64, ControlError>>>,
    result: Result<u64, ControlError>,
) {
    if let Some(ack) = ack {
        let _ = ack.send(result);
    }
}

fn command_receipt_result(
    snapshot: &RunSnapshot,
    command_id: &CommandId,
    command: &DurableCommand,
) -> Option<Result<u64, ControlError>> {
    snapshot
        .command_receipts
        .iter()
        .find(|receipt| receipt.command_id == *command_id)
        .map(|receipt| {
            if receipt.command == *command {
                Ok(receipt.sequence)
            } else {
                Err(ControlError::CommandPayloadMismatch)
            }
        })
}

fn terminal_for_control(action: DurableControlAction) -> TerminalState {
    match action {
        DurableControlAction::Interrupt => TerminalState::Interrupted,
        DurableControlAction::Cancel => TerminalState::Cancelled,
    }
}

#[derive(Debug, Clone)]
pub struct AgentControl {
    sender: mpsc::UnboundedSender<ControlCommand>,
}

impl AgentControl {
    pub fn steer(&self, content: impl Into<String>) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Steer {
                command_id: CommandId::new(),
                content: content.into(),
                ack: None,
            })
            .map_err(|_| ControlError::RunFinished)
    }

    pub fn interrupt(&self) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Stop {
                command_id: CommandId::new(),
                action: DurableControlAction::Interrupt,
                ack: None,
            })
            .map_err(|_| ControlError::RunFinished)
    }

    pub fn cancel(&self) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Stop {
                command_id: CommandId::new(),
                action: DurableControlAction::Cancel,
                ack: None,
            })
            .map_err(|_| ControlError::RunFinished)
    }

    pub async fn steer_durable(
        &self,
        command_id: CommandId,
        content: impl Into<String>,
    ) -> Result<u64, ControlError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(ControlCommand::Steer {
                command_id,
                content: content.into(),
                ack: Some(ack),
            })
            .map_err(|_| ControlError::RunFinished)?;
        received.await.map_err(|_| ControlError::RunFinished)?
    }

    pub async fn stop_durable(
        &self,
        command_id: CommandId,
        action: DurableControlAction,
    ) -> Result<u64, ControlError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(ControlCommand::Stop {
                command_id,
                action,
                ack: Some(ack),
            })
            .map_err(|_| ControlError::RunFinished)?;
        received.await.map_err(|_| ControlError::RunFinished)?
    }

    pub async fn resolve_interaction(
        &self,
        command_id: CommandId,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    ) -> Result<u64, ControlError> {
        let (ack, received) = oneshot::channel();
        self.sender
            .send(ControlCommand::ResolveInteraction {
                command_id,
                interaction_id,
                response,
                ack,
            })
            .map_err(|_| ControlError::RunFinished)?;
        received.await.map_err(|_| ControlError::RunFinished)?
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ControlError {
    #[error("the run has already finished")]
    RunFinished,
    #[error("runtime store failed while accepting the command: {message}")]
    Store { message: String },
    #[error("command id was already committed with a different payload")]
    CommandPayloadMismatch,
    #[error("the run is not waiting for a user interaction")]
    InteractionNotPending,
    #[error("interaction id does not match the pending request")]
    InteractionMismatch,
    #[error("the pending interaction was already resolved")]
    InteractionAlreadyResolved,
    #[error("invalid interaction response: {message}")]
    InvalidInteractionResponse { message: String },
}

pub struct RuntimeRun {
    pub run_id: RunId,
    control: AgentControl,
    join: JoinHandle<AgentOutcome>,
    ready: Option<oneshot::Receiver<Result<(), RunStoreError>>>,
}

impl RuntimeRun {
    #[must_use]
    pub fn control(&self) -> AgentControl {
        self.control.clone()
    }

    /// Wait until the canonical Store has durably created or acquired this
    /// run. Transports use this handshake before acknowledging start/resume;
    /// the Agent loop itself remains the only execution path.
    pub async fn ready(mut self) -> Result<Self, RunReadyError> {
        let Some(ready) = self.ready.take() else {
            return Ok(self);
        };
        match ready.await {
            Ok(Ok(())) => Ok(self),
            Ok(Err(error)) => Err(RunReadyError::Store(error)),
            Err(_) => Err(RunReadyError::RuntimeStopped),
        }
    }

    pub async fn wait(self) -> Result<AgentOutcome, RuntimeJoinError> {
        self.join.await.map_err(|error| RuntimeJoinError {
            message: error.to_string(),
        })
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RunReadyError {
    #[error(transparent)]
    Store(#[from] RunStoreError),
    #[error("runtime task stopped before the run store was ready")]
    RuntimeStopped,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("runtime task failed: {message}")]
pub struct RuntimeJoinError {
    pub message: String,
}

fn agent_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: AGENT_TOOL_NAME.to_owned(),
        description: "启动一个使用相同 AgentRuntime 的后台子 Agent。默认只读；只有显式 isolated_write、Host exact verifier、auto-approve 与 Orchestrator clean-Git 预检同时成立时，才分配隔离 worktree 并由 Host seal/ff-only 集成。".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "交给子 Agent 的完整任务说明。"
                },
                "type": {
                    "type": "string",
                    "description": "子 Agent 的角色标签，仅影响任务侧重点，不授予写权限。"
                },
                "workspace_access": {
                    "type": "string",
                    "enum": ["read_only", "isolated_write"],
                    "description": "工作区权限。默认 read_only；isolated_write 必须同时提供 allowed_paths。"
                },
                "allowed_paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "minItems": 1,
                    "uniqueItems": true,
                    "description": "isolated_write 唯一允许修改的工作区相对路径；只读任务不得提供。"
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "是否复制父 Agent 的当前规范化对话记录。"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "子 Agent 可见工具名；只会缩小父 Agent 已有权限。"
                },
                "max_steps": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "子 Agent 最多执行的模型轮次。"
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "从该子 Agent 起允许继续派生的最大相对深度。"
                },
                "wall_time_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "子 Agent 最长运行秒数，不会超过父任务剩余期限。"
                },
                "expected_artifact": {
                    "type": "string",
                    "description": "期望子 Agent 返回的具体产物或证据。"
                }
            },
            "required": ["prompt"],
            "additionalProperties": false
        }),
    }
}

#[derive(Debug)]
struct AgentLaunchRequest {
    workspace_access: AgentWorkspaceAccess,
    allowed_paths: Vec<String>,
    expected_artifact: String,
}

impl AgentLaunchRequest {
    fn parse(arguments: &Value) -> Result<Self, String> {
        let workspace_access = match arguments
            .get("workspace_access")
            .and_then(Value::as_str)
            .unwrap_or("read_only")
        {
            "read_only" => AgentWorkspaceAccess::ReadOnly,
            "isolated_write" => AgentWorkspaceAccess::IsolatedWrite,
            other => {
                return Err(format!(
                    "workspace_access '{other}' 无效，只接受 read_only 或 isolated_write"
                ));
            }
        };
        let requested_paths = arguments
            .get("allowed_paths")
            .map(|paths| {
                paths
                    .as_array()
                    .ok_or_else(|| "allowed_paths 必须是字符串数组".to_owned())?
                    .iter()
                    .map(|path| {
                        path.as_str()
                            .map(str::trim)
                            .filter(|path| !path.is_empty())
                            .map(str::to_owned)
                            .ok_or_else(|| "allowed_paths 只能包含非空字符串".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let mut canonical_paths = BTreeSet::new();
        for requested in requested_paths {
            let mut parts = Vec::new();
            for component in std::path::Path::new(&requested).components() {
                match component {
                    Component::Normal(part) => {
                        let part = part.to_str().ok_or_else(|| {
                            "allowed_paths 必须使用有效 UTF-8 相对路径".to_owned()
                        })?;
                        parts.push(part);
                    }
                    Component::CurDir => {}
                    Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                        return Err(format!(
                            "allowed_paths 中的 '{requested}' 必须保持工作区相对且不能包含 .."
                        ));
                    }
                }
            }
            if parts.is_empty() {
                return Err("allowed_paths 不能包含空路径或工作区根目录".to_owned());
            }
            canonical_paths.insert(parts.join("/"));
        }
        let allowed_paths = canonical_paths.into_iter().collect::<Vec<_>>();
        match workspace_access {
            AgentWorkspaceAccess::ReadOnly if !allowed_paths.is_empty() => {
                return Err("read_only 子 Agent 不得提供 allowed_paths".to_owned());
            }
            AgentWorkspaceAccess::IsolatedWrite if allowed_paths.is_empty() => {
                return Err("isolated_write 子 Agent 必须提供非空 allowed_paths".to_owned());
            }
            _ => {}
        }
        let expected_artifact = arguments
            .get("expected_artifact")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(if workspace_access == AgentWorkspaceAccess::IsolatedWrite {
                "通过冻结验收器的隔离代码变更"
            } else {
                "结构化调查结果"
            })
            .to_owned();
        Ok(Self {
            workspace_access,
            allowed_paths,
            expected_artifact,
        })
    }
}

fn agent_tool_workspace_access(call: &ModelToolCall) -> WorkspaceAccess {
    call.arguments
        .parsed
        .as_ref()
        .and_then(|arguments| arguments.get("workspace_access"))
        .and_then(Value::as_str)
        .filter(|access| *access == "isolated_write")
        .map_or(WorkspaceAccess::ReadOnly, |_| WorkspaceAccess::MayWrite)
}

#[derive(Debug)]
struct MissingAgentLifecycle {
    task_id: AgentTaskId,
}

impl From<MissingAgentLifecycle> for TerminalState {
    fn from(error: MissingAgentLifecycle) -> Self {
        TerminalState::Failed {
            failure: RuntimeFailure::Store {
                message: format!(
                    "canonical AgentTask '{}' disappeared from replay",
                    error.task_id.0
                ),
            },
        }
    }
}

fn current_agent_lifecycle(
    state: &RunState,
    task_id: &AgentTaskId,
) -> Result<AgentTaskLifecycle, MissingAgentLifecycle> {
    state
        .snapshot
        .agent_tasks
        .iter()
        .find(|lifecycle| &lifecycle.task.task_id == task_id)
        .cloned()
        .ok_or_else(|| MissingAgentLifecycle {
            task_id: task_id.clone(),
        })
}

fn writer_seal_from_lifecycle(lifecycle: &AgentTaskLifecycle) -> Option<WriterSeal> {
    let seal = lifecycle.seal.as_ref()?;
    let committed = seal.committed.as_ref()?;
    Some(WriterSeal {
        base_commit: seal.base_commit.clone(),
        final_commit: committed.final_commit.clone(),
        diff_sha256: committed.diff_sha256.clone(),
        changed_files: committed.changed_files.clone(),
        writer_workspace_state: committed.writer_workspace_state_after.clone(),
    })
}

fn recovered_writer_request(state: &RunState, task: &AgentTask, arguments: &Value) -> RunRequest {
    let mut system_prompt = state
        .snapshot
        .transcript
        .entries
        .iter()
        .find_map(|entry| match entry {
            TranscriptEntry::System { prompt } => Some(prompt.clone()),
            _ => None,
        })
        .unwrap_or_else(|| state.snapshot.request.system_prompt.clone());
    system_prompt.blocks.push(SystemPromptBlock {
        text: format!(
            "你是在同一 AgentRuntime 中运行的隔离写入子 Agent。角色：{}。只在分配的 worktree 内修改允许路径；不要访问主工作区或 Git 元数据；完成前让 Host 使用冻结的 exact verifier 验收。",
            task.role
        ),
        cache_control: PromptCacheControl::Volatile,
    });
    let fork_context = arguments
        .get("fork_context")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let transcript = if fork_context {
        let mut transcript = state.snapshot.transcript.clone();
        if matches!(
            transcript.entries.last(),
            Some(TranscriptEntry::Assistant { .. })
        ) {
            transcript.entries.pop();
        }
        if let Some(TranscriptEntry::System { prompt }) = transcript.entries.first_mut() {
            *prompt = system_prompt.clone();
        }
        transcript
    } else {
        CanonicalTranscript::default()
    };
    let mut environment = state.snapshot.request.environment.clone();
    environment.workspace = task.workspace.execution_workspace().to_owned();
    environment.interactive = false;
    environment.trust_mode = false;
    environment.allow_sandbox_elevation = false;
    environment.sandbox = Some("isolated_writer".to_owned());
    RunRequest {
        run_id: Some(task.child_run_id.clone()),
        parent_run_id: Some(task.parent_run_id.clone()),
        continued_from_run_id: None,
        model: state.snapshot.request.model.clone(),
        task_contract: Some(task.task_contract.clone()),
        system_prompt,
        transcript,
        reasoning_effort: state.snapshot.request.reasoning_effort,
        max_output_tokens: state.snapshot.request.max_output_tokens,
        streaming: false,
        actor: AgentActor {
            kind: AgentActorKind::Child,
            depth: state.snapshot.request.actor.depth.saturating_add(1),
        },
        agent_task: Some(task.clone()),
        deadline_unix_ms: task.deadline_unix_ms,
        tool_policy: task.tool_policy.clone(),
        limits: task.limits,
        environment,
        context_policy: state.snapshot.request.context_policy,
        context_projection: fork_context
            .then(|| state.snapshot.context_projection.clone())
            .flatten(),
        inherited_facts: None,
        accounting_baseline: state.accounting_epoch_baseline.clone(),
    }
}

fn agent_checks_from_replay(replay: &RunReplay) -> (Vec<AgentCheck>, Vec<ToolArtifact>) {
    let mut checks = Vec::new();
    let mut artifacts = Vec::new();
    for event in &replay.events {
        if let RuntimeEventKind::HostVerificationCommitted {
            verification_id,
            outcome,
            workspace_state_after,
            ..
        } = &event.event
            && let Some(observation) = &outcome.verifier_observation
        {
            checks.push(AgentCheck {
                check_id: verification_id.0.clone(),
                verifier: observation.spec.clone(),
                workspace_state: workspace_state_after.clone(),
                outcome: (**outcome).clone(),
            });
            artifacts.extend(outcome.artifacts.clone());
        }
    }
    artifacts.sort_by(|left, right| left.id.cmp(&right.id));
    artifacts.dedup_by(|left, right| left.id == right.id);
    (checks, artifacts)
}

fn recovered_finished_outcome(lifecycle: &AgentTaskLifecycle) -> Result<AgentOutcome, String> {
    let mut outcome = lifecycle
        .result
        .clone()
        .ok_or_else(|| "writer ChildFinished 缺少 collected result".to_owned())?;
    if matches!(outcome.terminal, TerminalState::Completed { .. }) {
        let integration = lifecycle
            .integration
            .as_ref()
            .ok_or_else(|| "成功 writer 缺少 integration lifecycle".to_owned())?;
        match (&integration.committed, &integration.failure) {
            (Some(committed), None) => {
                outcome.details.integration = WriterIntegrationStatus::Integrated {
                    integration_id: integration.integration_id.clone(),
                    writer_commit: integration.writer_commit.clone(),
                    root_workspace_state: committed.root_workspace_state_after.clone(),
                };
            }
            (None, Some(failure)) => {
                outcome.details.integration = failure.status.clone();
                let reason = writer_integration_failure_reason(&failure.status)
                    .ok_or_else(|| "writer integration failure 状态无效".to_owned())?
                    .to_owned();
                outcome.terminal = match failure.status {
                    WriterIntegrationStatus::RecoveryRequired { .. } => {
                        TerminalState::RecoveryRequired {
                            ambiguity: RecoveryAmbiguity {
                                phase: RecoveryAmbiguityPhase::ChildRun,
                                action_id: format!(
                                    "agent-integration:{}",
                                    integration.integration_id.0
                                ),
                                message: reason,
                            },
                        }
                    }
                    WriterIntegrationStatus::Rejected { .. }
                    | WriterIntegrationStatus::Conflict { .. } => TerminalState::Blocked { reason },
                    WriterIntegrationStatus::NotApplicable
                    | WriterIntegrationStatus::AwaitingHost
                    | WriterIntegrationStatus::Integrated { .. } => {
                        return Err("writer integration failure 状态无效".to_owned());
                    }
                };
            }
            (Some(_), Some(_)) => {
                return Err("writer integration 同时 committed 与 failed".to_owned());
            }
            (None, None) => {
                return Err("成功 writer integration 尚未形成持久结果".to_owned());
            }
        }
    }
    if let Some(cleanup) = lifecycle
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.committed.as_ref())
        && cleanup.retained_for_recovery
    {
        outcome.terminal = writer_cleanup_recovery_terminal(
            &lifecycle.task,
            cleanup
                .reason
                .as_deref()
                .unwrap_or("writer cleanup ownership is ambiguous"),
        );
    }
    Ok(outcome)
}

fn writer_failure_tool_outcome(task: &AgentTask, outcome: &AgentOutcome) -> ToolOutcome {
    ToolOutcome::rejected(
        format!(
            "writer_child_not_completed：writer child {} 以 {} 结束，未集成",
            task.child_run_id,
            terminal_state_label(&outcome.terminal)
        ),
        ToolRetryDisposition::AfterCorrection,
    )
}

fn writer_tool_outcome(task: &AgentTask, outcome: &AgentOutcome) -> ToolOutcome {
    if !matches!(outcome.terminal, TerminalState::Completed { .. }) {
        return writer_failure_tool_outcome(task, outcome);
    }
    let (
        WriterIntegrationStatus::Integrated {
            writer_commit,
            root_workspace_state,
            ..
        },
        Some(diff_sha256),
    ) = (&outcome.details.integration, &outcome.details.diff_sha256)
    else {
        return ToolOutcome::recovery_ambiguous(
            "writer_integration_missing：writer 完成结果缺少 Host integration facts",
        );
    };
    let mut tool_outcome = ToolOutcome::success(format!(
        "writer_integrated：Host 已集成 writer commit {writer_commit}"
    ))
    .with_side_effect(ToolSideEffectStatus::Applied)
    .with_evidence(ToolEvidence {
        status: ToolEvidenceStatus::Produced,
        references: outcome
            .details
            .evidence
            .iter()
            .map(|receipt| receipt.id.0.clone())
            .collect(),
    })
    .with_metadata(json!({
        "task_id": task.task_id,
        "child_run_id": task.child_run_id,
        "writer_commit": writer_commit,
        "diff_sha256": diff_sha256,
        "changed_files": outcome.details.changed_files,
    }));
    if let WorkspaceRevision::Known { sha256 } = &root_workspace_state.revision {
        tool_outcome.workspace_revision = Some(sha256.clone());
    }
    tool_outcome
}

fn read_only_workspace_identity(observed: Option<&str>) -> String {
    observed
        .filter(|identity| {
            matches!(identity.len(), 40 | 64)
                && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .unwrap_or("0000000000000000000000000000000000000000000000000000000000000000")
        .to_owned()
}

fn orchestration_terminal(error: AgentOrchestrationError) -> TerminalState {
    match error.kind {
        AgentOrchestrationErrorKind::Rejected | AgentOrchestrationErrorKind::Conflict => {
            TerminalState::Blocked {
                reason: format!("{}：{}", error.code, error.message),
            }
        }
        AgentOrchestrationErrorKind::RecoveryRequired => TerminalState::RecoveryRequired {
            ambiguity: RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ChildRun,
                action_id: error.code,
                message: error.message,
            },
        },
    }
}

fn orchestration_child_outcome(task: &AgentTask, error: &AgentOrchestrationError) -> AgentOutcome {
    AgentOutcome {
        run_id: task.child_run_id.clone(),
        parent_run_id: Some(task.parent_run_id.clone()),
        terminal: orchestration_terminal(error.clone()),
        accounting: incomplete_accounting(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        details: AgentResultDetails {
            summary: format!("{}：{}", error.code, error.message),
            ..AgentResultDetails::default()
        },
    }
}

fn writer_integration_failure_status(error: &AgentOrchestrationError) -> WriterIntegrationStatus {
    let reason = format!("{}：{}", error.code, error.message);
    match error.kind {
        AgentOrchestrationErrorKind::Rejected => WriterIntegrationStatus::Rejected { reason },
        AgentOrchestrationErrorKind::Conflict => WriterIntegrationStatus::Conflict { reason },
        AgentOrchestrationErrorKind::RecoveryRequired => {
            WriterIntegrationStatus::RecoveryRequired { reason }
        }
    }
}

fn writer_integration_failure_reason(status: &WriterIntegrationStatus) -> Option<&str> {
    match status {
        WriterIntegrationStatus::Rejected { reason }
        | WriterIntegrationStatus::Conflict { reason }
        | WriterIntegrationStatus::RecoveryRequired { reason } => Some(reason),
        WriterIntegrationStatus::NotApplicable
        | WriterIntegrationStatus::AwaitingHost
        | WriterIntegrationStatus::Integrated { .. } => None,
    }
}

fn orchestration_recovery(
    task_id: &AgentTaskId,
    code: impl Into<String>,
    message: impl Into<String>,
) -> TerminalState {
    let code = code.into();
    TerminalState::RecoveryRequired {
        ambiguity: RecoveryAmbiguity {
            phase: RecoveryAmbiguityPhase::ChildRun,
            action_id: format!("{}:{code}", task_id.0),
            message: message.into(),
        },
    }
}

fn validate_writer_seal(task: &AgentTask, seal: &WriterSeal) -> Result<(), String> {
    if seal.base_commit != task.workspace.base_commit {
        return Err("Host seal 的 base commit 与 AgentTask 不一致".to_owned());
    }
    if seal.final_commit == seal.base_commit {
        return Err("Host seal 没有产生新的 writer commit".to_owned());
    }
    if seal.changed_files.is_empty() {
        return Err("Host seal 不接受空 diff".to_owned());
    }
    let event = RuntimeEventKind::AgentSealCommitted {
        task_id: task.task_id.clone(),
        final_commit: seal.final_commit.clone(),
        diff_sha256: seal.diff_sha256.clone(),
        changed_files: seal.changed_files.clone(),
        writer_workspace_state_after: seal.writer_workspace_state.clone(),
    };
    event.validate_agent_lifecycle_payload()
}

fn validate_writer_receipts(task: &AgentTask, receipts: &[EvidenceReceipt]) -> Result<(), String> {
    let Some((acceptance_id, verifier)) =
        task.task_contract
            .definition
            .acceptance
            .iter()
            .find_map(|acceptance| match acceptance {
                TaskAcceptance::Verifier { id, verifier, .. } => Some((id, verifier)),
                TaskAcceptance::Host { .. } => None,
            })
    else {
        return Err("writer AgentTask 没有冻结 exact verifier".to_owned());
    };
    if receipts.len() != 1
        || receipts.iter().any(|receipt| {
            receipt.generation_id != task.task_contract.generation_id
                || receipt.acceptance_id != *acceptance_id
                || receipt.verifier != *verifier
        })
    {
        return Err(
            "writer EvidenceReceipt 与 child generation、acceptance 或 exact verifier 不一致"
                .to_owned(),
        );
    }
    Ok(())
}

fn failed_child_outcome(task: &AgentTask, failure: RuntimeFailure) -> AgentOutcome {
    AgentOutcome {
        run_id: task.child_run_id.clone(),
        parent_run_id: Some(task.parent_run_id.clone()),
        terminal: TerminalState::Failed { failure },
        accounting: incomplete_accounting(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        details: AgentResultDetails::default(),
    }
}

fn child_handoff(state: &RunState, outcome: &AgentOutcome) -> String {
    let marker_kind = if state.snapshot.request.actor.depth == 0 {
        "subagent_completion"
    } else {
        "child_subagent_completion"
    };
    let receipt = serde_json::to_string(outcome).unwrap_or_else(|error| {
        json!({
            "run_id": outcome.run_id,
            "terminal": "encoding_failed",
            "error": error.to_string(),
        })
        .to_string()
    });
    format!(
        "<codewhale:runtime_event kind=\"{marker_kind}\" agent_id=\"{}\">\n{receipt}\n</codewhale:runtime_event>",
        outcome.run_id
    )
}

fn readonly_child_handoff(state: &RunState, outcome: &AgentOutcome) -> String {
    let marker_kind = if state.snapshot.request.actor.depth == 0 {
        "subagent_completion"
    } else {
        "child_subagent_completion"
    };
    let receipt = json!({
        "run_id": outcome.run_id,
        "terminal": outcome.terminal,
        "summary": outcome.details.summary,
    });
    format!(
        "<codewhale:runtime_event kind=\"{marker_kind}\" agent_id=\"{}\">\n{receipt}\n</codewhale:runtime_event>",
        outcome.run_id
    )
}

fn terminal_state_label(terminal: &TerminalState) -> &'static str {
    match terminal {
        TerminalState::Completed { .. } => "completed",
        TerminalState::Blocked { .. } => "blocked",
        TerminalState::Failed { .. } => "failed",
        TerminalState::Cancelled => "cancelled",
        TerminalState::Interrupted => "interrupted",
        TerminalState::RecoveryRequired { .. } => "recovery_required",
    }
}

fn writer_cleanup_recovery_terminal(task: &AgentTask, message: &str) -> TerminalState {
    TerminalState::RecoveryRequired {
        ambiguity: RecoveryAmbiguity {
            phase: RecoveryAmbiguityPhase::ChildRun,
            action_id: format!("agent-cleanup:{}", task.task_id.0),
            message: message.to_owned(),
        },
    }
}

fn request_user_input_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
        description: "向当前用户提出 1 到 3 个简短问题，并等待规范化答案后继续任务。".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 3,
                    "items": {
                        "type": "object",
                        "properties": {
                            "header": {"type": "string"},
                            "id": {"type": "string"},
                            "question": {"type": "string"},
                            "options": {
                                "type": "array",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string"},
                                        "description": {"type": "string"}
                                    },
                                    "required": ["label", "description"],
                                    "additionalProperties": false
                                }
                            },
                            "allow_free_text": {"type": "boolean", "default": false},
                            "multi_select": {"type": "boolean", "default": false}
                        },
                        "required": ["header", "id", "question", "options"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["questions"],
            "additionalProperties": false
        }),
    }
}

fn model_failure(error: ModelPortError) -> RuntimeFailure {
    RuntimeFailure::Model {
        code: error.code,
        category: error.category,
        message: error.message,
        retryable: error.retryable,
    }
}

fn model_attempt_failure(error: &ModelPortError, actionable_output: bool) -> ModelAttemptFailure {
    ModelAttemptFailure {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: error.retryable,
        actionable_output,
    }
}

fn model_port_error_from_failure(failure: &ModelAttemptFailure) -> ModelPortError {
    ModelPortError::new(
        failure.code.clone(),
        failure.category,
        failure.message.clone(),
        failure.retryable,
    )
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

fn context_projection_failure(
    error: codewhale_context::compaction::ContextProjectionError,
) -> RuntimeFailure {
    RuntimeFailure::Store {
        message: error.to_string(),
    }
}

fn latched_model_failure(primary: &Option<ModelPortError>) -> RuntimeFailure {
    primary
        .clone()
        .map(model_failure)
        .unwrap_or_else(|| RuntimeFailure::InvalidModelOutput {
            message: "model failed without a typed primary error".to_owned(),
        })
}

fn seal_evidence_receipt(
    state: &RunState,
    verification_id: &VerificationId,
    acceptance_id: &AcceptanceId,
    verifier: &VerifierSpec,
    outcome: &ToolOutcome,
    workspace_state: &WorkspaceState,
) -> Option<EvidenceReceipt> {
    let observation = outcome.verifier_observation.as_ref()?;
    let revision_matches = matches!(
        (&observation.workspace_revision, &workspace_state.revision),
        (
            WorkspaceRevision::Known { sha256: observed },
            WorkspaceRevision::Known { sha256: settled }
        ) if observed == settled
    );
    if !outcome.is_success()
        || observation.verdict != VerifierVerdict::Passed
        || observation.spec != *verifier
        || !crate::store::verifier_artifacts_are_available(outcome, observation)
        || !revision_matches
    {
        return None;
    }
    let generation_id = state
        .snapshot
        .request
        .task_contract
        .as_ref()?
        .generation_id
        .clone();
    let receipt = EvidenceReceipt {
        id: EvidenceReceiptId::from(format!("receipt:{}", verification_id.0)),
        generation_id,
        acceptance_id: acceptance_id.clone(),
        verification_id: verification_id.clone(),
        verifier: verifier.clone(),
        workspace_state: workspace_state.clone(),
        artifact_ids: observation.artifact_ids.clone(),
    };
    receipt.validate().ok().map(|()| receipt)
}

fn invalid_model(message: impl Into<String>) -> TerminalState {
    TerminalState::Failed {
        failure: RuntimeFailure::InvalidModelOutput {
            message: message.into(),
        },
    }
}

fn store_terminal(failure: RuntimeFailure) -> TerminalState {
    TerminalState::Failed { failure }
}

fn store_start_failure(
    error: RunStoreError,
    run_id: RunId,
    parent_run_id: Option<RunId>,
) -> AgentOutcome {
    AgentOutcome {
        run_id,
        parent_run_id,
        terminal: TerminalState::Failed {
            failure: RuntimeFailure::Store {
                message: error.to_string(),
            },
        },
        accounting: incomplete_accounting(),
        runtime_model_requests: 0,
        runtime_retries: 0,
        tool_calls: 0,
        details: AgentResultDetails::default(),
    }
}

fn signal_ready(
    ready: &mut Option<oneshot::Sender<Result<(), RunStoreError>>>,
    result: Result<(), RunStoreError>,
) {
    if let Some(sender) = ready.take() {
        let _ = sender.send(result);
    }
}

fn cancelled_tool_outcome(content: impl Into<String>) -> ToolOutcome {
    let mut outcome = ToolOutcome::recovery_ambiguous(content);
    outcome.operation = ToolOperationStatus::Cancelled;
    outcome
}

fn tool_call_completed(transcript: &CanonicalTranscript, call_id: &str) -> bool {
    transcript.entries.iter().any(|entry| {
        matches!(
            entry,
            TranscriptEntry::Tool {
                call_id: completed,
                ..
            } if completed == call_id
        )
    })
}

fn incomplete_accounting() -> ModelAccounting {
    ModelAccounting {
        complete: false,
        usage_complete: false,
        usage_missing: true,
        usage_incomplete: true,
        billing_unknown: true,
        unpriced: true,
        ..ModelAccounting::default()
    }
}

fn effective_deadline(request: &RunRequest) -> Option<u64> {
    request.deadline_unix_ms
}

fn bounded_child_deadline(
    parent_deadline_unix_ms: Option<u64>,
    child_wall_time_ms: Option<u64>,
    child_started_unix_ms: u64,
) -> Option<u64> {
    let child_deadline =
        child_wall_time_ms.map(|wall_time| child_started_unix_ms.saturating_add(wall_time));
    match (parent_deadline_unix_ms, child_deadline) {
        (Some(parent), Some(child)) => Some(parent.min(child)),
        (Some(parent), None) => Some(parent),
        (None, Some(child)) => Some(child),
        (None, None) => None,
    }
}

fn deadline_expired(deadline: Option<u64>) -> bool {
    deadline.is_some_and(|deadline| now_unix_ms() >= deadline)
}

async fn wait_for_deadline(deadline: Option<u64>) {
    match deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_sub(now_unix_ms());
            tokio::time::sleep(Duration::from_millis(remaining)).await;
        }
        None => std::future::pending().await,
    }
}

async fn next_model_event(
    stream: &mut dyn ModelStream,
    idle_timeout_ms: Option<u64>,
) -> ModelEventPoll {
    match idle_timeout_ms {
        Some(timeout_ms) => {
            match tokio::time::timeout(Duration::from_millis(timeout_ms.max(1)), stream.next())
                .await
            {
                Ok(event) => ModelEventPoll::Event(event),
                Err(_) => ModelEventPoll::Idle,
            }
        }
        None => ModelEventPoll::Event(stream.next().await),
    }
}

fn timeout_terminal(state: &RunState, deadline: Option<u64>) -> TerminalState {
    TerminalState::Failed {
        failure: RuntimeFailure::Timeout {
            phase: RuntimeTimeoutPhase::Run,
            timeout_ms: state
                .snapshot
                .request
                .limits
                .wall_time_ms
                .unwrap_or_else(|| {
                    deadline
                        .map(|deadline| deadline.saturating_sub(state.started_unix_ms))
                        .unwrap_or_default()
                }),
        },
    }
}

fn validate_tool_calls(calls: &[ModelToolCall]) -> Result<(), String> {
    let mut ids = HashSet::with_capacity(calls.len());
    for call in calls {
        if call.id.trim().is_empty() {
            return Err("tool call id must not be empty".to_owned());
        }
        if call.name.trim().is_empty() {
            return Err(format!("tool call '{}' has an empty name", call.id));
        }
        if !ids.insert(call.id.as_str()) {
            return Err(format!("duplicate tool call id '{}'", call.id));
        }
    }
    Ok(())
}

#[cfg(test)]
mod deadline_tests {
    use super::bounded_child_deadline;

    #[test]
    fn child_deadline_honors_its_wall_time_and_parent_cap() {
        let now = 1_000_000;
        assert_eq!(
            bounded_child_deadline(Some(now + 225_000), Some(180_000), now),
            Some(now + 180_000)
        );
        assert_eq!(
            bounded_child_deadline(Some(now + 30_000), Some(180_000), now),
            Some(now + 30_000)
        );
        assert_eq!(
            bounded_child_deadline(None, Some(180_000), now),
            Some(now + 180_000)
        );
        assert_eq!(bounded_child_deadline(None, None, now), None);
    }
}
