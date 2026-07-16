use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
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
        let control = AgentControl { sender };
        let runtime = self.clone();
        let task_run_id = run_id.clone();
        let join = tokio::spawn(async move {
            runtime
                .run_launch(RunLaunch::Resume(task_run_id), None, receiver)
                .await
        });
        RuntimeRun {
            run_id,
            control,
            join,
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
        let control = AgentControl { sender };
        let runtime = self.clone();
        let join = tokio::spawn(async move {
            runtime
                .run_launch(RunLaunch::Create(Box::new(request)), Some(budget), receiver)
                .await
        });
        RuntimeRun {
            run_id,
            control,
            join,
        }
    }

    async fn run_launch(
        self: Arc<Self>,
        launch: RunLaunch,
        budget: Option<Arc<RuntimeBudget>>,
        mut control: mpsc::UnboundedReceiver<ControlCommand>,
    ) -> AgentOutcome {
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
                    Err(error) => return store_start_failure(error, run_id, parent_run_id),
                }
            }
            RunLaunch::Resume(run_id) => match self.store.acquire(&run_id).await {
                Ok(acquired) => {
                    for event in &acquired.replay.events {
                        self.sink.emit(event.clone()).await;
                    }
                    if let Some(outcome) = acquired.replay.snapshot.terminal.clone() {
                        return outcome;
                    }
                    let Some(lease) = acquired.lease else {
                        return store_start_failure(
                            RunStoreError::Corrupt {
                                run_id: run_id.clone(),
                                message: "non-terminal resume returned no writer lease".to_owned(),
                            },
                            run_id,
                            None,
                        );
                    };
                    (lease, acquired.replay)
                }
                Err(error) => return store_start_failure(error, run_id, None),
            },
        };
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
        let recovery_tool = snapshot.pending_tool.clone();
        let recovery_output = response_is_current
            .then_some(snapshot.last_model_output.clone())
            .flatten();
        let recovery_failure = resumed
            .then_some(snapshot.last_model_failure.clone())
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
            pending_steers: Vec::new(),
            pending_children: Vec::new(),
            recovery_model,
            recovery_tool,
            recovery_output,
            recovery_failure,
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
                        Ok(()) => continue,
                        Err(terminal) => {
                            self.cancel_children(&mut state).await;
                            return self.finalize(&mut state, terminal, &budget).await;
                        }
                    }
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
                let transcript = &state.snapshot.transcript;
                let system_prompt = transcript
                    .entries
                    .iter()
                    .find_map(|entry| match entry {
                        TranscriptEntry::System { prompt } => Some(prompt.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| state.snapshot.request.system_prompt.clone());
                let request = ModelRequest {
                    run_id: state.run_id().clone(),
                    parent_run_id: state.snapshot.request.parent_run_id.clone(),
                    actor: state.snapshot.request.actor,
                    model: state.snapshot.request.model.clone(),
                    system_prompt,
                    messages: transcript.project_messages(),
                    tools: self.tool_definitions(
                        &state.snapshot.request.tool_policy,
                        state.snapshot.request.actor.depth,
                        state.snapshot.request.limits.max_depth,
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
                command = control.recv() => match command {
                    Some(ControlCommand::Steer(content)) => {
                        state.pending_steers.push(content);
                        return Ok(ModelAttemptControl::Terminal(recovery_terminal(
                            RecoveryAmbiguityPhase::ModelRequest,
                            &attempt_id.0,
                            "模型请求已进入传输层，无法证明取消前是否已发送或计费",
                        )));
                    }
                    Some(ControlCommand::Interrupt) => {
                        return Ok(ModelAttemptControl::Terminal(TerminalState::Interrupted));
                    }
                    Some(ControlCommand::Cancel) => {
                        return Ok(ModelAttemptControl::Terminal(TerminalState::Cancelled));
                    }
                    None => {}
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
                command = control.recv() => match command {
                    Some(ControlCommand::Steer(content)) => {
                        state.pending_steers.push(content);
                        return Ok(ModelAttemptControl::Terminal(recovery_terminal(
                            RecoveryAmbiguityPhase::ModelRequest,
                            &attempt_id.0,
                            "模型流尚未原子提交，无法证明已生成内容和计费状态",
                        )));
                    }
                    Some(ControlCommand::Interrupt) => {
                        return Ok(ModelAttemptControl::Terminal(TerminalState::Interrupted));
                    }
                    Some(ControlCommand::Cancel) => {
                        return Ok(ModelAttemptControl::Terminal(TerminalState::Cancelled));
                    }
                    None => {}
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
                    command = control.recv() => match command {
                        Some(ControlCommand::Steer(content)) => {
                            state.pending_steers.push(content);
                        }
                        Some(ControlCommand::Interrupt) => {
                            cancellation.cancel();
                            let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
                            terminal_after_result = Some(TerminalState::Interrupted);
                            break cancelled_tool_outcome("tool_interrupted：工具执行已中断");
                        }
                        Some(ControlCommand::Cancel) => {
                            cancellation.cancel();
                            let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
                            terminal_after_result = Some(TerminalState::Cancelled);
                            break cancelled_tool_outcome("tool_cancelled：工具执行已取消");
                        }
                        None => {}
                    },
                    () = wait_for_deadline(deadline) => {
                        cancellation.cancel();
                        let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
                        terminal_after_result = Some(timeout_terminal(state, deadline));
                        break cancelled_tool_outcome("tool_deadline_exceeded：工具执行超过本次运行期限");
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
                "你是在同一 AgentRuntime 中运行的后台子 Agent。角色：{role}。请向父 Agent 返回简洁、具体、可验证的结果。"
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
        let child_request = RunRequest {
            run_id: Some(child_run_id.clone()),
            parent_run_id: Some(state.run_id().clone()),
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
            environment: state.snapshot.request.environment.clone(),
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
                    command = control.recv() => match command {
                        Some(ControlCommand::Steer(content)) => {
                            self.apply_steer(state, content).await.map_err(store_terminal)?;
                        }
                        Some(ControlCommand::Interrupt) => {
                            child.control.interrupt().ok();
                            for pending in &state.pending_children { pending.control.interrupt().ok(); }
                            let _ = child.join.await;
                            self.join_remaining_children(state).await;
                            return Err(TerminalState::Interrupted);
                        }
                        Some(ControlCommand::Cancel) => {
                            child.control.cancel().ok();
                            for pending in &state.pending_children { pending.control.cancel().ok(); }
                            let _ = child.join.await;
                            self.join_remaining_children(state).await;
                            return Err(TerminalState::Cancelled);
                        }
                        None => {}
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
                Ok(ControlCommand::Steer(content)) => self.apply_steer(state, content).await?,
                Ok(ControlCommand::Interrupt) => return Ok(Some(TerminalState::Interrupted)),
                Ok(ControlCommand::Cancel) => return Ok(Some(TerminalState::Cancelled)),
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    return Ok(None);
                }
            }
        }
    }

    async fn apply_steer(
        &self,
        state: &mut RunState,
        content: String,
    ) -> Result<(), RuntimeFailure> {
        self.publish(
            state,
            RuntimeEventKind::Steered {
                content: content.clone(),
            },
        )
        .await?;
        Ok(())
    }

    async fn flush_pending_steers(&self, state: &mut RunState) -> Result<(), RuntimeFailure> {
        for content in std::mem::take(&mut state.pending_steers) {
            self.apply_steer(state, content).await?;
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
        if matches!(terminal, TerminalState::RecoveryRequired { .. }) {
            accounting.complete = false;
            accounting.usage_complete = false;
            accounting.usage_incomplete = true;
            accounting.billing_unknown = true;
            accounting.billing_unknown_attempts =
                accounting.billing_unknown_attempts.saturating_add(1);
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
    pending_steers: Vec<String>,
    pending_children: Vec<PendingChild>,
    recovery_model: Option<PendingModelAction>,
    recovery_tool: Option<PendingToolAction>,
    recovery_output: Option<ModelOutput>,
    recovery_failure: Option<StoppedModelFailure>,
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

#[derive(Debug, Clone)]
enum ControlCommand {
    Steer(String),
    Interrupt,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct AgentControl {
    sender: mpsc::UnboundedSender<ControlCommand>,
}

impl AgentControl {
    pub fn steer(&self, content: impl Into<String>) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Steer(content.into()))
            .map_err(|_| ControlError::RunFinished)
    }

    pub fn interrupt(&self) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Interrupt)
            .map_err(|_| ControlError::RunFinished)
    }

    pub fn cancel(&self) -> Result<(), ControlError> {
        self.sender
            .send(ControlCommand::Cancel)
            .map_err(|_| ControlError::RunFinished)
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ControlError {
    #[error("the run has already finished")]
    RunFinished,
}

pub struct RuntimeRun {
    pub run_id: RunId,
    control: AgentControl,
    join: JoinHandle<AgentOutcome>,
}

impl RuntimeRun {
    #[must_use]
    pub fn control(&self) -> AgentControl {
        self.control.clone()
    }

    pub async fn wait(self) -> Result<AgentOutcome, RuntimeJoinError> {
        self.join.await.map_err(|error| RuntimeJoinError {
            message: error.to_string(),
        })
    }
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

fn recovery_terminal(
    phase: RecoveryAmbiguityPhase,
    action_id: &str,
    message: &str,
) -> TerminalState {
    TerminalState::RecoveryRequired {
        ambiguity: RecoveryAmbiguity {
            phase,
            action_id: action_id.to_owned(),
            message: message.to_owned(),
        },
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
