use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use codewhale_context::compaction::{
    ContextCompactionPreparation, effective_context, estimate_projection_tokens,
    prepare_compaction, projection_from_summary,
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::*;

enum RunLaunch {
    Create(Box<RunRequest>),
    Resume(RunId),
}

#[derive(Clone)]
pub struct AgentRuntime {
    model: Arc<dyn ModelPort>,
    tools: Arc<dyn ToolExecutor>,
    sink: Arc<dyn RuntimeEventSink>,
    store: Arc<dyn RunStore>,
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
        }
    }

    #[must_use]
    pub fn store(&self) -> Arc<dyn RunStore> {
        self.store.clone()
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
        self.start_inner(request, budget)
    }

    /// Reopen one canonical run. The persisted request, transcript, counters,
    /// and terminal state win over all caller-local state.
    #[must_use]
    pub fn resume(self: &Arc<Self>, run_id: RunId) -> RuntimeRun {
        let (sender, receiver) = mpsc::unbounded_channel();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let control = AgentControl { sender };
        let runtime = self.clone();
        let task_run_id = run_id.clone();
        let join = tokio::spawn(async move {
            runtime
                .run_launch(RunLaunch::Resume(task_run_id), None, receiver, ready_sender)
                .await
        });
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
        let join = tokio::spawn(async move {
            runtime
                .run_launch(
                    RunLaunch::Create(Box::new(request)),
                    Some(budget),
                    receiver,
                    ready_sender,
                )
                .await
        });
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
        budget: Option<Arc<RuntimeBudget>>,
        mut control: mpsc::UnboundedReceiver<ControlCommand>,
        ready: oneshot::Sender<Result<(), RunStoreError>>,
    ) -> AgentOutcome {
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
        let accounting_epoch_baseline = snapshot.accounting.clone();
        let recovery_model = snapshot.pending_model.clone();
        let recovery_context_compaction = snapshot.pending_context_compaction.clone();
        let recovery_tool = snapshot.pending_tool.clone();
        let recovery_output = response_is_current
            .then_some(snapshot.last_model_output.clone())
            .flatten();
        let recovery_failure = resumed
            .then_some(snapshot.last_model_failure.clone())
            .flatten();
        let recovery_context_compaction_failure = resumed
            .then_some(snapshot.last_context_compaction_failure.clone())
            .flatten();
        let recovered_child_ids = snapshot.pending_children.clone();
        let mut state = RunState {
            snapshot,
            lease,
            accounting_epoch_baseline,
            model_accounting_includes_baseline: !resumed,
            started_unix_ms: replay
                .events
                .first()
                .map_or_else(now_unix_ms, |event| event.occurred_at_unix_ms),
            pending_children: Vec::new(),
            recovery_model,
            recovery_context_compaction,
            recovery_tool,
            recovery_output,
            recovery_failure,
            recovery_context_compaction_failure,
            recovered_child_ids,
        };

        if resumed {
            if let Some(ambiguity) = state.recovery_ambiguity() {
                return self
                    .finalize(
                        &mut state,
                        TerminalState::RecoveryRequired { ambiguity },
                        &budget,
                    )
                    .await;
            }
            if let Some(control) = state.snapshot.pending_control.clone() {
                let terminal = match control.action {
                    DurableControlAction::Interrupt => TerminalState::Interrupted,
                    DurableControlAction::Cancel => TerminalState::Cancelled,
                };
                self.settle_children_for(&mut state, &terminal).await;
                return self.finalize(&mut state, terminal, &budget).await;
            }
            if let Some(stopped) = state.recovery_failure.take() {
                let failure = match stopped.reason {
                    ModelRetryStopReason::RequestBudgetExceeded => {
                        RuntimeFailure::RequestBudgetExceeded {
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
                return self
                    .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                    .await;
            }
            if state.snapshot.request.purpose == RunPurpose::ContextCompaction
                && let Some(stopped) = state.recovery_context_compaction_failure.take()
            {
                return self
                    .finalize(
                        &mut state,
                        TerminalState::Failed {
                            failure: RuntimeFailure::ContextCompactionFailed {
                                message: stopped.failure.message,
                            },
                        },
                        &budget,
                    )
                    .await;
            }
        }

        loop {
            match self.drain_controls(&mut state, &mut control).await {
                Ok(Some(terminal)) => {
                    self.settle_children_for(&mut state, &terminal).await;
                    return self.finalize(&mut state, terminal, &budget).await;
                }
                Ok(None) => {}
                Err(failure) => {
                    self.cancel_children(&mut state).await;
                    return self
                        .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                        .await;
                }
            }

            if deadline_expired(deadline) {
                self.cancel_children(&mut state).await;
                let terminal = timeout_terminal(&state, deadline);
                return self.finalize(&mut state, terminal, &budget).await;
            }
            let safe_fresh_boundary = state.recovery_model.is_none()
                && state.recovery_output.is_none()
                && state.recovery_tool.is_none()
                && state.snapshot.pending_model.is_none()
                && state.snapshot.pending_tool.is_none()
                && state.snapshot.pending_children.is_empty();
            if safe_fresh_boundary
                && !state.snapshot.pending_steers.is_empty()
                && let Err(failure) = self.flush_pending_steers(&mut state).await
            {
                return self
                    .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                    .await;
            }
            if state.snapshot.request.purpose == RunPurpose::ContextCompaction {
                if state.snapshot.last_context_compaction.is_some() {
                    return self
                        .finalize(
                            &mut state,
                            TerminalState::Completed {
                                message: "上下文压缩已完成。".to_owned(),
                            },
                            &budget,
                        )
                        .await;
                }
                match self
                    .compact_context(
                        &mut state,
                        &budget,
                        &mut control,
                        deadline,
                        ContextCompactionTrigger::Manual,
                        true,
                    )
                    .await
                {
                    Ok(ContextCompactionControl::Terminal(terminal)) => {
                        return self.finalize(&mut state, terminal, &budget).await;
                    }
                    Ok(
                        ContextCompactionControl::Committed | ContextCompactionControl::NotNeeded,
                    ) => {
                        return self
                            .finalize(
                                &mut state,
                                TerminalState::Completed {
                                    message: "上下文压缩已完成。".to_owned(),
                                },
                                &budget,
                            )
                            .await;
                    }
                    Err(failure) => {
                        return self
                            .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                            .await;
                    }
                }
            }
            if safe_fresh_boundary && state.snapshot.request.context_policy.auto_compact {
                let estimated = match effective_context(
                    &state.snapshot.transcript,
                    state.snapshot.context_projection.as_ref(),
                ) {
                    Ok(context) => context.estimated_tokens,
                    Err(error) => {
                        return self
                            .finalize(
                                &mut state,
                                TerminalState::Failed {
                                    failure: RuntimeFailure::Store {
                                        message: error.to_string(),
                                    },
                                },
                                &budget,
                            )
                            .await;
                    }
                };
                let trigger = if estimated
                    > u64::from(state.snapshot.request.context_policy.hard_input_tokens)
                {
                    ContextCompactionTrigger::PreflightLimit
                } else {
                    ContextCompactionTrigger::Threshold
                };
                match self
                    .compact_context(
                        &mut state,
                        &budget,
                        &mut control,
                        deadline,
                        trigger,
                        trigger == ContextCompactionTrigger::PreflightLimit,
                    )
                    .await
                {
                    Ok(ContextCompactionControl::Terminal(terminal)) => {
                        return self.finalize(&mut state, terminal, &budget).await;
                    }
                    Ok(
                        ContextCompactionControl::Committed | ContextCompactionControl::NotNeeded,
                    ) => {}
                    Err(failure) => {
                        return self
                            .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                            .await;
                    }
                }
            }
            if state.recovery_output.is_none()
                && state.recovery_model.is_none()
                && state.snapshot.local_turns >= state.snapshot.request.limits.max_turns
            {
                self.cancel_children(&mut state).await;
                let limit = state.snapshot.request.limits.max_turns;
                return self
                    .finalize(
                        &mut state,
                        TerminalState::Failed {
                            failure: RuntimeFailure::TurnBudgetExceeded { limit },
                        },
                        &budget,
                    )
                    .await;
            }
            let turn = if let Some(output) = state.recovery_output.take() {
                Ok(ModelTurnControl::Output(ModelTurnOutput::from(output)))
            } else if let Some(pending) = state.recovery_model.take() {
                self.model_turn(&mut state, &budget, &mut control, deadline, Some(pending))
                    .await
            } else {
                self.model_turn(&mut state, &budget, &mut control, deadline, None)
                    .await
            };
            let turn = match turn {
                Ok(ModelTurnControl::Terminal(terminal)) => {
                    self.settle_children_for(&mut state, &terminal).await;
                    return self.finalize(&mut state, terminal, &budget).await;
                }
                Ok(ModelTurnControl::Output(output)) => output,
                Err(failure) => {
                    self.cancel_children(&mut state).await;
                    return self
                        .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                        .await;
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
                self.cancel_children(&mut state).await;
                return self
                    .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                    .await;
            }

            if let Err(message) = validate_tool_calls(&turn.tool_calls) {
                self.cancel_children(&mut state).await;
                return self
                    .finalize(&mut state, invalid_model(message), &budget)
                    .await;
            }
            if turn.tool_calls.is_empty() {
                if turn.finish_reason != ModelFinishReason::Stop {
                    self.cancel_children(&mut state).await;
                    return self
                        .finalize(
                            &mut state,
                            invalid_model("finish_reason=tool_calls without tool calls"),
                            &budget,
                        )
                        .await;
                }
                if !state.pending_children.is_empty() {
                    match self.join_children(&mut state, &mut control, deadline).await {
                        Ok(()) => {
                            if let Err(failure) = self.flush_pending_steers(&mut state).await {
                                self.cancel_children(&mut state).await;
                                return self
                                    .finalize(
                                        &mut state,
                                        TerminalState::Failed { failure },
                                        &budget,
                                    )
                                    .await;
                            }
                            continue;
                        }
                        Err(terminal) => {
                            self.cancel_children(&mut state).await;
                            return self.finalize(&mut state, terminal, &budget).await;
                        }
                    }
                }
                if !state.snapshot.pending_steers.is_empty() {
                    if let Err(failure) = self.flush_pending_steers(&mut state).await {
                        return self
                            .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                            .await;
                    }
                    continue;
                }
                if turn.content.trim().is_empty() {
                    return self
                        .finalize(
                            &mut state,
                            TerminalState::Failed {
                                failure: RuntimeFailure::EmptyModelOutput,
                            },
                            &budget,
                        )
                        .await;
                }
                return self
                    .finalize(
                        &mut state,
                        TerminalState::Completed {
                            message: turn.content,
                        },
                        &budget,
                    )
                    .await;
            }

            if turn.finish_reason != ModelFinishReason::ToolCalls {
                self.cancel_children(&mut state).await;
                return self
                    .finalize(
                        &mut state,
                        invalid_model("tool calls require finish_reason=tool_calls"),
                        &budget,
                    )
                    .await;
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
                    self.cancel_children(&mut state).await;
                    let limit = state.snapshot.request.limits.max_tool_calls;
                    return self
                        .finalize(
                            &mut state,
                            TerminalState::Failed {
                                failure: RuntimeFailure::ToolBudgetExceeded { limit },
                            },
                            &budget,
                        )
                        .await;
                }
                match self
                    .execute_call(&mut state, call, &budget, &mut control, deadline)
                    .await
                {
                    Ok(()) => {}
                    Err(terminal) => {
                        self.settle_children_for(&mut state, &terminal).await;
                        return self.finalize(&mut state, terminal, &budget).await;
                    }
                }
            }
            if let Err(failure) = self.flush_pending_steers(&mut state).await {
                self.cancel_children(&mut state).await;
                return self
                    .finalize(&mut state, TerminalState::Failed { failure }, &budget)
                    .await;
            }
        }
    }

    async fn compact_context(
        &self,
        state: &mut RunState,
        budget: &RuntimeBudget,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
        trigger: ContextCompactionTrigger,
        force: bool,
    ) -> Result<ContextCompactionControl, RuntimeFailure> {
        let current = effective_context(
            &state.snapshot.transcript,
            state.snapshot.context_projection.as_ref(),
        )
        .map_err(context_projection_failure)?;
        if state
            .snapshot
            .last_context_compaction_failure
            .as_ref()
            .is_some_and(|failure| failure.source_projection_sha256 == current.sha256)
        {
            if force {
                let message = state
                    .snapshot
                    .last_context_compaction_failure
                    .as_ref()
                    .map(|failure| failure.failure.message.clone())
                    .unwrap_or_else(|| "context compaction failed".to_owned());
                return Err(RuntimeFailure::ContextCompactionFailed { message });
            }
            if current.estimated_tokens
                > u64::from(state.snapshot.request.context_policy.hard_input_tokens)
            {
                return Err(RuntimeFailure::ContextLimitExceeded {
                    estimated_tokens: current.estimated_tokens,
                    hard_input_tokens: u64::from(
                        state.snapshot.request.context_policy.hard_input_tokens,
                    ),
                });
            }
            return Ok(ContextCompactionControl::NotNeeded);
        }

        let mut prepared = state.recovery_context_compaction.take();
        if prepared.is_none() {
            let template = self.compaction_request_template(state);
            match prepare_compaction(
                &state.snapshot.transcript,
                state.snapshot.context_projection.as_ref(),
                state.snapshot.request.context_policy,
                &template,
                force,
            )
            .map_err(context_projection_failure)?
            {
                ContextCompactionPreparation::NotNeeded { .. } => {
                    return Ok(ContextCompactionControl::NotNeeded);
                }
                ContextCompactionPreparation::LimitExceeded {
                    estimated_tokens,
                    hard_input_tokens,
                } => {
                    return Err(RuntimeFailure::ContextLimitExceeded {
                        estimated_tokens,
                        hard_input_tokens,
                    });
                }
                ContextCompactionPreparation::Local {
                    projection,
                    before_tokens,
                    after_tokens,
                } => {
                    self.publish(
                        state,
                        RuntimeEventKind::ContextCompactionCommitted {
                            compaction_id: ContextCompactionId::new(),
                            trigger,
                            projection: Box::new(projection),
                            output: None,
                            accounting: Box::new(state.snapshot.accounting.clone()),
                            before_tokens,
                            after_tokens,
                        },
                    )
                    .await?;
                    return Ok(ContextCompactionControl::Committed);
                }
                ContextCompactionPreparation::Model { plan, request } => {
                    if !budget.reserve_model_request() {
                        return Err(RuntimeFailure::RequestBudgetExceeded {
                            limit: state.snapshot.request.limits.max_model_requests,
                        });
                    }
                    let compaction_id = ContextCompactionId::new();
                    let attempt_id = AttemptId::new();
                    self.publish(
                        state,
                        RuntimeEventKind::ContextCompactionPrepared {
                            compaction_id: compaction_id.clone(),
                            attempt_id: attempt_id.clone(),
                            trigger,
                            plan: Box::new(plan.clone()),
                            request: Box::new(request.clone()),
                        },
                    )
                    .await?;
                    prepared = Some(PendingContextCompaction {
                        compaction_id,
                        trigger,
                        plan,
                        attempt_id,
                        request,
                        state: DurableActionState::Prepared,
                    });
                }
            }
        }

        loop {
            let Some(pending) = prepared.take() else {
                return Err(RuntimeFailure::Store {
                    message: "context compaction lost its durable prepared request".to_owned(),
                });
            };
            debug_assert_eq!(pending.state, DurableActionState::Prepared);
            match self
                .execute_context_compaction_attempt(state, &pending, control, deadline)
                .await?
            {
                ContextCompactionAttemptControl::Output(output) => {
                    let projection = projection_from_summary(&pending.plan, &output.content);
                    let after_tokens =
                        estimate_projection_tokens(&state.snapshot.transcript, &projection)
                            .map_err(context_projection_failure)?;
                    if after_tokens >= pending.plan.before_tokens
                        || after_tokens
                            > u64::from(state.snapshot.request.context_policy.hard_input_tokens)
                    {
                        let error = ModelPortError::new(
                            "context_compaction_ineffective",
                            ModelErrorCategory::Protocol,
                            format!(
                                "context summary did not reduce the request below the hard limit: before={}, after={}, hard={}",
                                pending.plan.before_tokens,
                                after_tokens,
                                state.snapshot.request.context_policy.hard_input_tokens
                            ),
                            false,
                        );
                        self.commit_context_compaction_failure(
                            state,
                            &pending,
                            &error,
                            Some(output),
                            ModelRetryDecision::Stop {
                                reason: ModelRetryStopReason::ActionableOutput,
                            },
                        )
                        .await?;
                        return self.context_compaction_stopped(state, force);
                    }
                    let accounting = self.cumulative_accounting(state, false).await;
                    self.publish(
                        state,
                        RuntimeEventKind::ContextCompactionCommitted {
                            compaction_id: pending.compaction_id,
                            trigger: pending.trigger,
                            projection: Box::new(projection),
                            output: Some(Box::new(output)),
                            accounting: Box::new(accounting),
                            before_tokens: pending.plan.before_tokens,
                            after_tokens,
                        },
                    )
                    .await?;
                    return Ok(ContextCompactionControl::Committed);
                }
                ContextCompactionAttemptControl::Failed {
                    error,
                    actionable_output,
                    output,
                } => {
                    let retry = self.plan_context_compaction_failure(
                        state,
                        budget,
                        &pending,
                        &error,
                        actionable_output,
                    );
                    self.commit_context_compaction_failure(
                        state,
                        &pending,
                        &error,
                        output,
                        retry.clone(),
                    )
                    .await?;
                    match retry {
                        ModelRetryDecision::Retry { .. } => {
                            prepared = state.snapshot.pending_context_compaction.clone();
                        }
                        ModelRetryDecision::Stop { .. } => {
                            return self.context_compaction_stopped(state, force);
                        }
                    }
                }
                ContextCompactionAttemptControl::Terminal(terminal) => {
                    return Ok(ContextCompactionControl::Terminal(terminal));
                }
            }
        }
    }

    fn compaction_request_template(&self, state: &RunState) -> ModelRequest {
        ModelRequest {
            run_id: state.run_id().clone(),
            parent_run_id: state.snapshot.request.parent_run_id.clone(),
            actor: state.snapshot.request.actor,
            model: state.snapshot.request.model.clone(),
            system_prompt: state.snapshot.request.system_prompt.clone(),
            messages: Vec::new(),
            tools: Vec::new(),
            reasoning_effort: ReasoningEffort::Low,
            max_output_tokens: Some(
                state
                    .snapshot
                    .request
                    .context_policy
                    .summary_max_output_tokens
                    .max(1),
            ),
            streaming: false,
            request_number: state.snapshot.local_turns.saturating_add(1),
            attempt: 0,
        }
    }

    async fn execute_context_compaction_attempt(
        &self,
        state: &mut RunState,
        pending: &PendingContextCompaction,
        control: &mut mpsc::UnboundedReceiver<ControlCommand>,
        deadline: Option<u64>,
    ) -> Result<ContextCompactionAttemptControl, RuntimeFailure> {
        self.publish(
            state,
            RuntimeEventKind::ContextCompactionInFlight {
                compaction_id: pending.compaction_id.clone(),
                attempt_id: pending.attempt_id.clone(),
            },
        )
        .await?;

        let open = self.model.stream(pending.request.clone());
        tokio::pin!(open);
        let mut stream = loop {
            tokio::select! {
                result = &mut open => break match result {
                    Ok(stream) => stream,
                    Err(error) => {
                        return Ok(ContextCompactionAttemptControl::Failed {
                            error,
                            actionable_output: false,
                            output: None,
                        });
                    }
                },
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await? {
                        ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                        ControlEffect::Terminal(terminal) => {
                            return Ok(ContextCompactionAttemptControl::Terminal(terminal));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    return Ok(ContextCompactionAttemptControl::Terminal(
                        timeout_terminal(state, deadline),
                    ));
                }
            }
        };

        let mut streamed_content = String::new();
        let mut streamed_reasoning = String::new();
        loop {
            tokio::select! {
                event = next_model_event(
                    &mut *stream,
                    state.snapshot.request.limits.model_event_idle_ms,
                ) => {
                    match event {
                        ModelEventPoll::Idle => {
                            let timeout_ms = state
                                .snapshot
                                .request
                                .limits
                                .model_event_idle_ms
                                .unwrap_or_default();
                            return Ok(ContextCompactionAttemptControl::Failed {
                                error: ModelPortError::new(
                                    "context_compaction_stream_stall",
                                    ModelErrorCategory::StreamStall,
                                    format!(
                                        "context compaction stream produced no canonical event for {timeout_ms}ms"
                                    ),
                                    true,
                                ),
                                actionable_output: !streamed_content.is_empty()
                                    || !streamed_reasoning.is_empty(),
                                output: None,
                            });
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::ContentDelta { delta }))) => {
                            streamed_content.push_str(&delta);
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::ReasoningDelta { delta }))) => {
                            streamed_reasoning.push_str(&delta);
                        }
                        ModelEventPoll::Event(Some(Ok(ModelStreamEvent::Completed { output }))) => {
                            let completed_reasoning =
                                output.reasoning_content.clone().unwrap_or_default();
                            let stream_matches = (streamed_content.is_empty()
                                || streamed_content == output.content)
                                && (streamed_reasoning.is_empty()
                                    || streamed_reasoning == completed_reasoning);
                            if !stream_matches
                                || output.finish_reason != ModelFinishReason::Stop
                                || !output.tool_calls.is_empty()
                                || output.content.trim().is_empty()
                            {
                                return Ok(ContextCompactionAttemptControl::Failed {
                                    error: ModelPortError::new(
                                        "context_compaction_output_invalid",
                                        ModelErrorCategory::Protocol,
                                        "context compaction returned an invalid completed output",
                                        false,
                                    ),
                                    actionable_output: true,
                                    output: Some(output),
                                });
                            }
                            return Ok(ContextCompactionAttemptControl::Output(output));
                        }
                        ModelEventPoll::Event(Some(Err(error))) => {
                            return Ok(ContextCompactionAttemptControl::Failed {
                                error,
                                actionable_output: !streamed_content.is_empty()
                                    || !streamed_reasoning.is_empty(),
                                output: None,
                            });
                        }
                        ModelEventPoll::Event(None) => {
                            return Ok(ContextCompactionAttemptControl::Failed {
                                error: ModelPortError::new(
                                    "context_compaction_stream_incomplete",
                                    ModelErrorCategory::Protocol,
                                    "context compaction stream ended without a completed event",
                                    true,
                                ),
                                actionable_output: !streamed_content.is_empty()
                                    || !streamed_reasoning.is_empty(),
                                output: None,
                            });
                        }
                    }
                }
                command = control.recv() => if let Some(command) = command {
                    match self.handle_control(state, command).await? {
                        ControlEffect::Continue | ControlEffect::InteractionResolved(_) => {}
                        ControlEffect::Terminal(terminal) => {
                            return Ok(ContextCompactionAttemptControl::Terminal(terminal));
                        }
                    }
                },
                () = wait_for_deadline(deadline) => {
                    return Ok(ContextCompactionAttemptControl::Terminal(
                        timeout_terminal(state, deadline),
                    ));
                }
            }
        }
    }

    fn plan_context_compaction_failure(
        &self,
        state: &RunState,
        budget: &RuntimeBudget,
        pending: &PendingContextCompaction,
        error: &ModelPortError,
        actionable_output: bool,
    ) -> ModelRetryDecision {
        let reason = if actionable_output {
            Some(ModelRetryStopReason::ActionableOutput)
        } else if !error.retryable {
            Some(ModelRetryStopReason::NotRetryable)
        } else if pending.request.attempt >= state.snapshot.request.context_policy.max_retries {
            Some(ModelRetryStopReason::RetryLimitReached)
        } else if !budget.reserve_model_request() {
            Some(ModelRetryStopReason::RequestBudgetExceeded)
        } else {
            None
        };
        if let Some(reason) = reason {
            return ModelRetryDecision::Stop { reason };
        }
        let mut request = pending.request.clone();
        request.attempt = request.attempt.saturating_add(1);
        ModelRetryDecision::Retry {
            prepared: PreparedModelRetry {
                attempt_id: AttemptId::new(),
                request: Box::new(request),
            },
        }
    }

    async fn commit_context_compaction_failure(
        &self,
        state: &mut RunState,
        pending: &PendingContextCompaction,
        error: &ModelPortError,
        output: Option<ModelOutput>,
        retry: ModelRetryDecision,
    ) -> Result<(), RuntimeFailure> {
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish(
            state,
            RuntimeEventKind::ContextCompactionAttemptFailed {
                compaction_id: pending.compaction_id.clone(),
                attempt_id: pending.attempt_id.clone(),
                failure: model_attempt_failure(error, output.is_some()),
                output: output.map(Box::new),
                accounting: Box::new(accounting),
                retry,
            },
        )
        .await?;
        Ok(())
    }

    fn context_compaction_stopped(
        &self,
        state: &RunState,
        force: bool,
    ) -> Result<ContextCompactionControl, RuntimeFailure> {
        let current = effective_context(
            &state.snapshot.transcript,
            state.snapshot.context_projection.as_ref(),
        )
        .map_err(context_projection_failure)?;
        let stopped = state
            .snapshot
            .last_context_compaction_failure
            .as_ref()
            .ok_or_else(|| RuntimeFailure::Store {
                message: "stopped context compaction has no durable failure".to_owned(),
            })?;
        if stopped.failure.code == "deepseek_request_budget_exhausted" {
            return Err(RuntimeFailure::RequestBudgetExceeded {
                limit: state.snapshot.request.limits.max_model_requests,
            });
        }
        if force {
            return Err(RuntimeFailure::ContextCompactionFailed {
                message: stopped.failure.message.clone(),
            });
        }
        if current.estimated_tokens
            > u64::from(state.snapshot.request.context_policy.hard_input_tokens)
        {
            return Err(RuntimeFailure::ContextLimitExceeded {
                estimated_tokens: current.estimated_tokens,
                hard_input_tokens: u64::from(
                    state.snapshot.request.context_policy.hard_input_tokens,
                ),
            });
        }
        Ok(ContextCompactionControl::NotNeeded)
    }

    async fn model_turn(
        &self,
        state: &mut RunState,
        budget: &RuntimeBudget,
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
                if !budget.reserve_model_request() {
                    return Ok(ModelTurnControl::Terminal(TerminalState::Failed {
                        failure: RuntimeFailure::RequestBudgetExceeded {
                            limit: state.snapshot.request.limits.max_model_requests,
                        },
                    }));
                }
                let context = effective_context(
                    &state.snapshot.transcript,
                    state.snapshot.context_projection.as_ref(),
                )
                .map_err(context_projection_failure)?;
                let request = ModelRequest {
                    run_id: state.run_id().clone(),
                    parent_run_id: state.snapshot.request.parent_run_id.clone(),
                    actor: state.snapshot.request.actor,
                    model: state.snapshot.request.model.clone(),
                    system_prompt: context.system_prompt,
                    messages: context.messages,
                    tools: self.tool_definitions(
                        &state.snapshot.request.tool_policy,
                        state.snapshot.request.actor.depth,
                        state.snapshot.request.limits.max_depth,
                        state.snapshot.request.environment.interactive,
                    ),
                    reasoning_effort: state.snapshot.request.reasoning_effort,
                    max_output_tokens: state.snapshot.request.max_output_tokens,
                    streaming: state.snapshot.request.streaming,
                    request_number,
                    attempt: 0,
                };
                let attempt_id = AttemptId::new();
                self.publish(
                    state,
                    RuntimeEventKind::ModelRequestPrepared {
                        attempt_id: attempt_id.clone(),
                        request: Box::new(request.clone()),
                    },
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
                    return Ok(ModelTurnControl::Output(ModelTurnOutput::from(output)));
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
                    self.commit_model_failure(
                        state,
                        &attempt_id,
                        &error,
                        actionable_output,
                        plan.decision(),
                    )
                    .await?;
                    match plan {
                        ModelFailurePlan::Retry { .. } => {
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
                        ModelFailurePlan::Stop { terminal, .. } => {
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
    ) -> Result<(), RuntimeFailure> {
        let accounting = self.cumulative_accounting(state, false).await;
        self.publish(
            state,
            RuntimeEventKind::ModelRequestFailed {
                attempt_id: attempt_id.clone(),
                failure: model_attempt_failure(error, actionable_output),
                accounting: Box::new(accounting),
                retry,
            },
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
        budget: &RuntimeBudget,
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
        } else if !budget.reserve_model_request() {
            Some(ModelRetryStopReason::RequestBudgetExceeded)
        } else {
            None
        };
        if let Some(reason) = stop_reason {
            let terminal = if reason == ModelRetryStopReason::RequestBudgetExceeded {
                TerminalState::Failed {
                    failure: RuntimeFailure::RequestBudgetExceeded {
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

        let mut next_request = request.clone();
        next_request.attempt = next_request.attempt.saturating_add(1);
        let prepared = PreparedModelRetry {
            attempt_id: AttemptId::new(),
            request: Box::new(next_request),
        };
        ModelFailurePlan::Retry { prepared }
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
        let operation_id = match state.recovery_tool.take() {
            Some(pending) if pending.invocation.call_id == call.id => {
                debug_assert_eq!(pending.state, DurableActionState::Prepared);
                pending.operation_id
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
                    },
                )
                .await
                .map_err(store_terminal)?;
                operation_id
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
            self.publish(
                state,
                RuntimeEventKind::ToolExecutionStarted {
                    operation_id: operation_id.clone(),
                },
            )
            .await
            .map_err(store_terminal)?;
            self.launch_child(state, &call, budget)
                .await
                .map_err(store_terminal)?
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
                self.publish(
                    state,
                    RuntimeEventKind::ToolExecutionStarted {
                        operation_id: operation_id.clone(),
                    },
                )
                .await
                .map_err(store_terminal)?;
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

        self.publish(
            state,
            RuntimeEventKind::ToolOutcomeCommitted {
                operation_id,
                call_id: call.id.clone(),
                name: call.name.clone(),
                outcome: outcome.clone(),
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
                outcome,
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
    ) -> Result<ToolOutcome, RuntimeFailure> {
        if state.snapshot.request.actor.depth >= state.snapshot.request.limits.max_depth {
            return Ok(ToolOutcome::error(format!(
                "child_depth_limit：已达到子 Agent 深度上限 {}",
                state.snapshot.request.limits.max_depth
            )));
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
        let Some(child_lease) = budget.reserve_child() else {
            return Ok(ToolOutcome::error(format!(
                "child_concurrency_limit：已达到子 Agent 并发上限 {}",
                state.snapshot.request.limits.max_concurrent_children
            )));
        };
        let child_run_id = RunId::new();
        let child_depth = state.snapshot.request.actor.depth.saturating_add(1);
        self.publish(
            state,
            RuntimeEventKind::ChildStarted {
                call_id: call.id.clone(),
                child_run_id: child_run_id.clone(),
                depth: child_depth,
            },
        )
        .await?;

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
        // Until WorkspaceLane is the owner of child worktrees, every child is
        // read-only. Profiles change instructions, never the safety boundary.
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
        let expected_artifact = arguments
            .get("expected_artifact")
            .and_then(Value::as_str)
            .map(|value| format!("\n期望产物：{value}"))
            .unwrap_or_default();
        let child_input = format!("{prompt}{expected_artifact}");
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
                "你是在同一 AgentRuntime 中运行的只读后台子 Agent。角色：{role}。只使用本次实际提供的工具，不要尝试修改文件或调用不可用工具；向父 Agent 返回简洁、具体、可验证的结果。"
            ),
            cache_control: PromptCacheControl::Volatile,
        });
        let mut child_limits = state.snapshot.request.limits;
        if let Some(max_steps) = arguments.get("max_steps").and_then(Value::as_u64) {
            child_limits.max_turns = child_limits.max_turns.min(max_steps.max(1) as u32);
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
        let mut child_environment = state.snapshot.request.environment.clone();
        child_environment.interactive = false;
        let child_request = RunRequest {
            run_id: Some(child_run_id.clone()),
            parent_run_id: Some(state.run_id().clone()),
            continued_from_run_id: None,
            purpose: RunPurpose::Agent,
            model: state.snapshot.request.model.clone(),
            input: child_input,
            system_prompt,
            transcript,
            reasoning_effort: state.snapshot.request.reasoning_effort,
            max_output_tokens: state.snapshot.request.max_output_tokens,
            streaming: false,
            actor: AgentActor {
                kind: AgentActorKind::Child,
                depth: child_depth,
            },
            deadline_unix_ms: state.snapshot.request.deadline_unix_ms,
            tool_policy: child_policy,
            limits: child_limits,
            environment: child_environment,
            context_policy: state.snapshot.request.context_policy,
            context_projection,
            accounting_baseline: ModelAccounting::default(),
        };
        let child = self.start_inner(child_request, budget.clone());
        state.pending_children.push(PendingChild {
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
                                let _ = child.join.await;
                                self.join_remaining_children(state).await;
                                return Err(terminal);
                            }
                        }
                    },
                    () = wait_for_deadline(deadline) => {
                        child.control.cancel().ok();
                        for pending in &state.pending_children { pending.control.cancel().ok(); }
                        let _ = child.join.await;
                        self.join_remaining_children(state).await;
                        return Err(timeout_terminal(state, deadline));
                    }
                }
            };
            let marker_kind = if state.snapshot.request.actor.depth == 0 {
                "subagent_completion"
            } else {
                "child_subagent_completion"
            };
            let receipt = json!({
                "run_id": outcome.run_id,
                "terminal": outcome.terminal,
            })
            .to_string();
            let handoff = format!(
                "<codewhale:runtime_event kind=\"{marker_kind}\" agent_id=\"{}\">\n{receipt}\n</codewhale:runtime_event>",
                outcome.run_id
            );
            let accounting = self.cumulative_accounting(state, false).await;
            self.publish(
                state,
                RuntimeEventKind::ChildFinished {
                    call_id: child.call_id.clone(),
                    outcome: Box::new(outcome.clone()),
                    accounting: Box::new(accounting),
                    handoff_content: handoff.clone(),
                },
            )
            .await
            .map_err(store_terminal)?;
        }
        Ok(())
    }

    async fn cancel_children(&self, state: &mut RunState) {
        for child in &state.pending_children {
            child.control.cancel().ok();
        }
        self.join_remaining_children(state).await;
    }

    async fn settle_children_for(&self, state: &mut RunState, terminal: &TerminalState) {
        if matches!(terminal, TerminalState::Interrupted) {
            for child in &state.pending_children {
                child.control.interrupt().ok();
            }
            self.join_remaining_children(state).await;
        } else {
            self.cancel_children(state).await;
        }
    }

    async fn join_remaining_children(&self, state: &mut RunState) {
        for child in state.pending_children.drain(..) {
            let _ = child.join.await;
        }
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
        let mut accounting = self.cumulative_accounting(state, root).await;
        let model_request_unsettled = state
            .snapshot
            .pending_model
            .as_ref()
            .is_some_and(|pending| pending.state == DurableActionState::InFlight)
            || state
                .snapshot
                .pending_context_compaction
                .as_ref()
                .is_some_and(|pending| pending.state == DurableActionState::InFlight);
        let recovery_may_hide_model_billing = matches!(
            &terminal,
            TerminalState::RecoveryRequired {
                ambiguity: RecoveryAmbiguity {
                    phase: RecoveryAmbiguityPhase::ModelRequest
                        | RecoveryAmbiguityPhase::ContextCompactionModelRequest
                        | RecoveryAmbiguityPhase::ChildRun,
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
                failure: RuntimeFailure::RequestBudgetExceeded {
                    limit: accounting
                        .hard_request_limit
                        .unwrap_or(state.snapshot.request.limits.max_model_requests),
                },
            };
        }
        let outcome = AgentOutcome {
            run_id: state.run_id().clone(),
            parent_run_id: state.snapshot.request.parent_run_id.clone(),
            terminal,
            accounting,
            runtime_model_requests: state.snapshot.runtime_model_requests,
            runtime_retries: state.snapshot.runtime_retries,
            tool_calls: state.snapshot.tool_calls,
        };
        if self
            .publish(
                state,
                RuntimeEventKind::Terminal {
                    outcome: Box::new(outcome.clone()),
                },
            )
            .await
            .is_err()
        {
            if let Ok(Some(replay)) = self.store.load(state.run_id()).await
                && let Some(existing) = replay.snapshot.terminal
            {
                return existing;
            }
            let _ = self.store.release(&state.lease).await;
            return AgentOutcome {
                terminal: TerminalState::Failed {
                    failure: RuntimeFailure::Store {
                        message: "failed to persist the terminal event".to_owned(),
                    },
                },
                ..outcome
            };
        }
        state.snapshot.terminal.clone().unwrap_or(outcome)
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
    recovery_context_compaction: Option<PendingContextCompaction>,
    recovery_tool: Option<PendingToolAction>,
    recovery_output: Option<ModelOutput>,
    recovery_failure: Option<StoppedModelFailure>,
    recovery_context_compaction_failure: Option<StoppedContextCompactionFailure>,
    recovered_child_ids: Vec<RunId>,
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
            .recovery_context_compaction
            .as_ref()
            .filter(|pending| pending.state == DurableActionState::InFlight)
        {
            return Some(RecoveryAmbiguity {
                phase: RecoveryAmbiguityPhase::ContextCompactionModelRequest,
                action_id: pending.attempt_id.0.clone(),
                message: "进程在 DeepSeek 上下文压缩请求进入传输层后、压缩投影原子提交前停止；为避免重复请求和重复计费，运行未自动重发。".to_owned(),
            });
        }
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
        self.recovered_child_ids
            .first()
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
    call_id: String,
    run_id: RunId,
    control: AgentControl,
    join: JoinHandle<AgentOutcome>,
    _lease: ChildLease,
}

#[derive(Debug)]
struct ModelTurnOutput {
    content: String,
    tool_calls: Vec<ModelToolCall>,
    finish_reason: ModelFinishReason,
}

impl From<ModelOutput> for ModelTurnOutput {
    fn from(output: ModelOutput) -> Self {
        Self {
            content: output.content,
            tool_calls: output.tool_calls,
            finish_reason: output.finish_reason,
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
    },
}

impl ModelFailurePlan {
    fn decision(&self) -> ModelRetryDecision {
        match self {
            Self::Stop { reason, .. } => ModelRetryDecision::Stop { reason: *reason },
            Self::Retry { prepared, .. } => ModelRetryDecision::Retry {
                prepared: prepared.clone(),
            },
        }
    }
}

enum ModelTurnControl {
    Output(ModelTurnOutput),
    Terminal(TerminalState),
}

enum ContextCompactionControl {
    NotNeeded,
    Committed,
    Terminal(TerminalState),
}

enum ContextCompactionAttemptControl {
    Output(ModelOutput),
    Failed {
        error: ModelPortError,
        actionable_output: bool,
        output: Option<ModelOutput>,
    },
    Terminal(TerminalState),
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

    fn reserve_model_request(&self) -> bool {
        reserve(&self.model_requests, self.limits.max_model_requests)
    }

    fn reserve_tool(&self) -> bool {
        reserve(&self.tools, self.limits.max_tool_calls)
    }

    fn reserve_child(self: &Arc<Self>) -> Option<ChildLease> {
        reserve(&self.children, self.limits.max_concurrent_children).then(|| ChildLease {
            budget: self.clone(),
        })
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
        description:
            "启动一个使用相同 AgentRuntime 的只读后台子 Agent；结果会由运行时自动回注父 Agent。"
                .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "交给子 Agent 的完整任务说明。"
                },
                "type": {
                    "type": "string",
                    "description": "子 Agent 的角色标签，仅影响任务侧重点。"
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
