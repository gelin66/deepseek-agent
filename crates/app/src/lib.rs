//! Transport-neutral application service for canonical Agent runs.
//!
//! This crate owns product commands and active-process control. Durable run
//! state, lifecycle, events, accounting, and completion remain owned by the
//! single [`RunStore`] and [`AgentRuntime`] contracts.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    CommandId, DurableControlAction, InteractionId, RunId, StoredRuntimeEvent, TerminalState,
    UserInteractionResponse,
};
use codewhale_protocol::run_api::{
    RUN_API_SCHEMA_VERSION, RunApiError, RunApiErrorCode, RunCommand, RunCommandEnvelope,
    RunCommandResponse, RunCommandResult, RunView, StartRunCommand,
};
use codewhale_runtime::{
    AgentControl, ControlError, DurableActionState, DurableCommand, ModelAccounting,
    ModelErrorCategory, ModelPort, ModelPortError, ModelRequest, ModelStream, RunReadyError,
    RunReplay, RunStore, RunStoreError, RuntimeEventSink, RuntimeRun,
};
use tokio::sync::{Mutex, Notify};

mod production;

pub use production::{
    DEFAULT_MAX_API_REQUESTS, ProductionApplicationConfig, ProductionApplicationError,
    ProductionPromptConfig,
};

// Production compositions may retain these credential-free settings while
// serving read-only get/events/terminal replay. A credential and per-run
// budget are bound only inside a live start/resume path.
pub use codewhale_deepseek::{
    DeepSeekConnectionConfig, DeepSeekCredential, DeepSeekEndpoint, DeepSeekModelPort,
    OfficialModelCapabilities, OfficialModelCapabilityError, TransportRetryPolicy,
    official_model_capabilities,
};
pub use codewhale_tools::sandbox::SandboxPolicy;
pub use codewhale_tools::shell::ShellPolicy;
pub use codewhale_tools::{
    ProductionExecPolicyRuleSet, ProductionExecPolicySnapshot, ProductionToolConfig,
    ProductionToolExecutionIdentity, ProductionToolExecutor,
};

/// Model port used only when a persisted run can be replayed or failed closed
/// without live model access. Constructing the application and reading
/// terminal/events therefore never requires a DeepSeek credential.
pub struct ReplayOnlyModelPort;

#[async_trait]
impl ModelPort for ReplayOnlyModelPort {
    async fn stream(&self, _request: ModelRequest) -> Result<Box<dyn ModelStream>, ModelPortError> {
        Err(ModelPortError::new(
            "resume_model_access_forbidden",
            ModelErrorCategory::Protocol,
            "纯恢复路径不得发起 DeepSeek 请求",
            false,
        ))
    }

    async fn accounting_snapshot(&self, _seal: bool) -> Result<ModelAccounting, ModelPortError> {
        Ok(ModelAccounting {
            complete: true,
            usage_complete: true,
            ..ModelAccounting::default()
        })
    }
}

/// Whether an unfinished persisted run may safely issue another live model
/// request. Terminal/failed/in-flight actions and pending children replay
/// without touching the credential or network.
#[must_use]
pub fn resume_needs_live_model(replay: &RunReplay) -> bool {
    if replay.snapshot.terminal.is_some()
        || replay.snapshot.last_model_failure.is_some()
        || !replay.snapshot.pending_children.is_empty()
    {
        return false;
    }
    if replay
        .snapshot
        .pending_model
        .as_ref()
        .is_some_and(|pending| pending.state == DurableActionState::InFlight)
        || replay
            .snapshot
            .pending_tool
            .as_ref()
            .is_some_and(|pending| pending.state == DurableActionState::InFlight)
    {
        return false;
    }
    true
}

/// Canonical product command service shared by CLI, HTTP, SSE, and stdio.
///
/// The active registry is deliberately process-local and disposable. It holds
/// only control senders for runs executing in this process; every query and
/// lifecycle decision is made from the canonical Store first.
pub struct AgentApplication {
    store: Arc<dyn RunStore>,
    composition: Arc<dyn RunComposition>,
    active: Arc<Mutex<HashMap<RunId, ActiveRun>>>,
    next_launch_token: AtomicU64,
    watch: Arc<StoreWatch>,
}

#[derive(Clone)]
struct ActiveRun {
    launch_token: u64,
    control: AgentControl,
}

struct PendingActivation {
    control: Option<AgentControl>,
}

impl PendingActivation {
    fn new(control: AgentControl) -> Self {
        Self {
            control: Some(control),
        }
    }

    fn disarm(&mut self) {
        self.control = None;
    }
}

impl Drop for PendingActivation {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.cancel();
        }
    }
}

/// Private composition boundary shared by production and deterministic tests.
/// Implementations must construct `AgentRuntime` with the Store and sink
/// supplied by this service.
#[async_trait]
trait RunComposition: Send + Sync {
    async fn start(
        &self,
        command: StartRunCommand,
        store: Arc<dyn RunStore>,
        sink: Arc<dyn RuntimeEventSink>,
    ) -> Result<RuntimeRun, RunApiError>;

    async fn resume(
        &self,
        run_id: RunId,
        replay: RunReplay,
        store: Arc<dyn RunStore>,
        sink: Arc<dyn RuntimeEventSink>,
    ) -> Result<RuntimeRun, RunApiError>;
}

#[derive(Default)]
struct StoreWatch {
    notify: Notify,
}

impl StoreWatch {
    fn wake(&self) {
        self.notify.notify_waiters();
    }
}

struct NotifyingSink {
    watch: Arc<StoreWatch>,
}

#[async_trait]
impl RuntimeEventSink for NotifyingSink {
    async fn emit(&self, _event: StoredRuntimeEvent) {
        // The runtime has already committed the event before invoking the
        // sink. Notify is only an edge-triggered wake-up, never state truth.
        self.watch.wake();
    }
}

impl AgentApplication {
    fn from_parts(store: Arc<dyn RunStore>, composition: Arc<dyn RunComposition>) -> Self {
        Self {
            store,
            composition,
            active: Arc::new(Mutex::new(HashMap::new())),
            next_launch_token: AtomicU64::new(1),
            watch: Arc::new(StoreWatch::default()),
        }
    }

    #[cfg(test)]
    fn new(store: Arc<dyn RunStore>, composition: Arc<dyn RunComposition>) -> Self {
        Self::from_parts(store, composition)
    }

    /// Execute one canonical Run command.
    pub async fn execute(&self, envelope: RunCommandEnvelope) -> RunCommandResponse {
        let RunCommandEnvelope {
            schema_version,
            request_id,
            command,
        } = envelope;
        let command_id = CommandId::from(request_id.clone());
        let result = if schema_version != RUN_API_SCHEMA_VERSION {
            error_result(api_error(
                RunApiErrorCode::InvalidRequest,
                format!(
                    "unsupported run API schema version {schema_version}; expected {RUN_API_SCHEMA_VERSION}"
                ),
                None,
                None,
            ))
        } else {
            match command {
                RunCommand::Start(command) => self.start(command).await,
                RunCommand::Get { run_id } => self.get(&run_id).await,
                RunCommand::Events {
                    run_id,
                    after_sequence,
                } => self.events(&run_id, after_sequence).await,
                RunCommand::Resume {
                    run_id,
                    expected_workspace,
                } => self.resume(&run_id, expected_workspace.as_deref()).await,
                RunCommand::Steer { run_id, content } => {
                    if content.trim().is_empty() {
                        error_result(api_error(
                            RunApiErrorCode::InvalidRequest,
                            "steer content must not be empty",
                            Some(run_id),
                            None,
                        ))
                    } else {
                        self.control(
                            &run_id,
                            ControlAction::Steer {
                                command_id,
                                content,
                            },
                        )
                        .await
                    }
                }
                RunCommand::Interrupt { run_id } => {
                    self.control(
                        &run_id,
                        ControlAction::Stop {
                            command_id,
                            action: DurableControlAction::Interrupt,
                        },
                    )
                    .await
                }
                RunCommand::Cancel { run_id } => {
                    self.control(
                        &run_id,
                        ControlAction::Stop {
                            command_id,
                            action: DurableControlAction::Cancel,
                        },
                    )
                    .await
                }
                RunCommand::ResolveInteraction {
                    run_id,
                    interaction_id,
                    response,
                } => {
                    self.control(
                        &run_id,
                        ControlAction::ResolveInteraction {
                            command_id,
                            interaction_id,
                            response,
                        },
                    )
                    .await
                }
            }
        };
        RunCommandResponse {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id,
            result,
        }
    }

    /// Wait until canonical events exist strictly after `after_sequence`, or
    /// return immediately when the run is terminal.
    ///
    /// The waiter is registered before querying the Store. Therefore an event
    /// committed between the query and the await cannot be missed. Wakes may
    /// be spurious and always cause another Store query.
    pub async fn wait_events(&self, run_id: &RunId, after_sequence: u64) -> RunCommandResult {
        loop {
            let notified = self.watch.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let replay = match self.load(run_id).await {
                Ok(replay) => replay,
                Err(error) => return error_result(error),
            };
            if after_sequence > replay.snapshot.last_sequence {
                return error_result(cursor_ahead(
                    run_id,
                    after_sequence,
                    replay.snapshot.last_sequence,
                ));
            }
            let events = match self.store.events_after(run_id, after_sequence).await {
                Ok(events) => strictly_after(events, after_sequence),
                Err(error) => return error_result(store_error(error)),
            };
            if !events.is_empty() || replay.snapshot.terminal.is_some() {
                return RunCommandResult::Events {
                    run_id: run_id.clone(),
                    after_sequence,
                    events,
                };
            }
            if !self.active.lock().await.contains_key(run_id) {
                return error_result(recovery_required(run_id));
            }
            notified.await;
        }
    }

    async fn start(&self, command: StartRunCommand) -> RunCommandResult {
        if let Err(error) = validate_start(&command) {
            return error_result(error);
        }
        let sink = self.event_sink();
        let run = match self
            .composition
            .start(command, self.store.clone(), sink)
            .await
        {
            Ok(run) => run,
            Err(error) => return error_result(error),
        };
        let run_id = run.run_id.clone();
        match self.activate(run).await {
            Ok(()) => self.get(&run_id).await,
            Err(error) => error_result(error),
        }
    }

    async fn get(&self, run_id: &RunId) -> RunCommandResult {
        match self.load(run_id).await {
            Ok(replay) => RunCommandResult::Run {
                run: Box::new(project_run(&replay)),
            },
            Err(error) => error_result(error),
        }
    }

    async fn events(&self, run_id: &RunId, after_sequence: u64) -> RunCommandResult {
        let replay = match self.load(run_id).await {
            Ok(replay) => replay,
            Err(error) => return error_result(error),
        };
        if after_sequence > replay.snapshot.last_sequence {
            return error_result(cursor_ahead(
                run_id,
                after_sequence,
                replay.snapshot.last_sequence,
            ));
        }
        let events = match self.store.events_after(run_id, after_sequence).await {
            Ok(events) => strictly_after(events, after_sequence),
            Err(error) => return error_result(store_error(error)),
        };
        if events.is_empty()
            && replay.snapshot.terminal.is_none()
            && !self.active.lock().await.contains_key(run_id)
        {
            return error_result(recovery_required(run_id));
        }
        RunCommandResult::Events {
            run_id: run_id.clone(),
            after_sequence,
            events,
        }
    }

    async fn resume(&self, run_id: &RunId, expected_workspace: Option<&str>) -> RunCommandResult {
        let replay = match self.load(run_id).await {
            Ok(replay) => replay,
            Err(error) => return error_result(error),
        };
        if let Some(expected_workspace) = expected_workspace
            && expected_workspace != replay.snapshot.request.environment.workspace
        {
            return error_result(api_error(
                RunApiErrorCode::RunEnvironmentMismatch,
                format!(
                    "run_resume_workspace_mismatch：expected workspace {expected_workspace:?} does not match persisted workspace {:?}",
                    replay.snapshot.request.environment.workspace
                ),
                Some(run_id.clone()),
                None,
            ));
        }
        if replay.snapshot.terminal.is_some() {
            return RunCommandResult::Run {
                run: Box::new(project_run(&replay)),
            };
        }
        if self.active.lock().await.contains_key(run_id) {
            return error_result(api_error(
                RunApiErrorCode::RunAlreadyRunning,
                format!("run {run_id} is already active in this process"),
                Some(run_id.clone()),
                None,
            ));
        }

        let sink = self.event_sink();
        let run = match self
            .composition
            .resume(run_id.clone(), replay, self.store.clone(), sink)
            .await
        {
            Ok(run) => run,
            Err(error) => return error_result(error),
        };
        match self.activate(run).await {
            Ok(()) => self.get(run_id).await,
            Err(error) => error_result(error),
        }
    }

    async fn control(&self, run_id: &RunId, action: ControlAction) -> RunCommandResult {
        let replay = match self.load(run_id).await {
            Ok(replay) => replay,
            Err(error) => return error_result(error),
        };
        if let Some(result) = committed_control_result(run_id, &replay, &action) {
            return result;
        }
        if let Some(outcome) = &replay.snapshot.terminal {
            return error_result(terminal_error(run_id, outcome.terminal.clone()));
        }

        let active = self.active.lock().await.get(run_id).cloned();
        let Some(active) = active else {
            return self.control_after_race(run_id, &action).await;
        };
        let sent = match action.clone() {
            ControlAction::Steer {
                command_id,
                content,
            } => active.control.steer_durable(command_id, content).await,
            ControlAction::Stop { command_id, action } => {
                active.control.stop_durable(command_id, action).await
            }
            ControlAction::ResolveInteraction {
                command_id,
                interaction_id,
                response,
            } => {
                active
                    .control
                    .resolve_interaction(command_id, interaction_id, response)
                    .await
            }
        };
        match sent {
            Ok(last_sequence) => RunCommandResult::Accepted {
                run_id: run_id.clone(),
                last_sequence,
            },
            Err(ControlError::RunFinished) => self.control_after_race(run_id, &action).await,
            Err(ControlError::Store { message }) => error_result(api_error(
                RunApiErrorCode::RunStoreFailed,
                message,
                Some(run_id.clone()),
                None,
            )),
            Err(ControlError::CommandPayloadMismatch) => error_result(api_error(
                RunApiErrorCode::InvalidRequest,
                "command request_id was already committed with a different payload",
                Some(run_id.clone()),
                None,
            )),
            Err(ControlError::InteractionNotPending) => error_result(api_error(
                RunApiErrorCode::InteractionNotPending,
                "run is not waiting for a user interaction",
                Some(run_id.clone()),
                None,
            )),
            Err(ControlError::InteractionMismatch) => error_result(api_error(
                RunApiErrorCode::InteractionMismatch,
                "interaction id does not match the pending request",
                Some(run_id.clone()),
                None,
            )),
            Err(ControlError::InteractionAlreadyResolved) => error_result(api_error(
                RunApiErrorCode::InteractionAlreadyResolved,
                "interaction was already resolved",
                Some(run_id.clone()),
                None,
            )),
            Err(ControlError::InvalidInteractionResponse { message }) => error_result(api_error(
                RunApiErrorCode::InvalidInteractionResponse,
                message,
                Some(run_id.clone()),
                None,
            )),
        }
    }

    async fn control_after_race(&self, run_id: &RunId, action: &ControlAction) -> RunCommandResult {
        match self.load(run_id).await {
            Ok(replay) => {
                if let Some(result) = committed_control_result(run_id, &replay, action) {
                    return result;
                }
                match replay.snapshot.terminal {
                    Some(outcome) => error_result(terminal_error(run_id, outcome.terminal)),
                    None => error_result(api_error(
                        RunApiErrorCode::RunNotActive,
                        format!("run {run_id} is not active in this process"),
                        Some(run_id.clone()),
                        None,
                    )),
                }
            }
            Err(error) => error_result(error),
        }
    }

    async fn activate(&self, run: RuntimeRun) -> Result<(), RunApiError> {
        // Runtime execution is detached from the caller future. Keep a
        // cancellation guard until its control handle is durably represented
        // in the process-local active registry; otherwise an aborted HTTP or
        // stdio request could leave an unregistered live run behind.
        let mut pending = PendingActivation::new(run.control());
        let run = run.ready().await.map_err(ready_error)?;
        let run_id = run.run_id.clone();
        let control = run.control();
        let launch_token = self.next_launch_token.fetch_add(1, Ordering::Relaxed);
        {
            let mut active = self.active.lock().await;
            if active.contains_key(&run_id) {
                // A concurrent launch won the registry race. Its Store lease
                // is authoritative; this launch is asked to stop and its
                // monitor is still drained below.
                let _ = control.cancel();
                spawn_monitor(run, launch_token, self.active.clone(), self.watch.clone());
                return Err(api_error(
                    RunApiErrorCode::RunAlreadyRunning,
                    format!("run {run_id} is already active in this process"),
                    Some(run_id),
                    None,
                ));
            }
            active.insert(
                run_id,
                ActiveRun {
                    launch_token,
                    control,
                },
            );
        }
        pending.disarm();
        spawn_monitor(run, launch_token, self.active.clone(), self.watch.clone());
        Ok(())
    }

    fn event_sink(&self) -> Arc<dyn RuntimeEventSink> {
        Arc::new(NotifyingSink {
            watch: self.watch.clone(),
        })
    }

    async fn load(&self, run_id: &RunId) -> Result<RunReplay, RunApiError> {
        match self.store.load(run_id).await {
            Ok(Some(replay)) => Ok(replay),
            Ok(None) => Err(api_error(
                RunApiErrorCode::RunNotFound,
                format!("run {run_id} does not exist"),
                Some(run_id.clone()),
                None,
            )),
            Err(error) => Err(store_error(error)),
        }
    }
}

#[derive(Clone)]
enum ControlAction {
    Steer {
        command_id: CommandId,
        content: String,
    },
    Stop {
        command_id: CommandId,
        action: DurableControlAction,
    },
    ResolveInteraction {
        command_id: CommandId,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    },
}

fn committed_control_result(
    run_id: &RunId,
    replay: &RunReplay,
    action: &ControlAction,
) -> Option<RunCommandResult> {
    let receipt = replay
        .snapshot
        .command_receipts
        .iter()
        .find(|receipt| receipt.command_id == *action.command_id())?;
    if receipt.command != action.durable_command() {
        return Some(error_result(api_error(
            RunApiErrorCode::InvalidRequest,
            "command request_id was already committed with a different payload",
            Some(run_id.clone()),
            None,
        )));
    }
    Some(RunCommandResult::Accepted {
        run_id: run_id.clone(),
        last_sequence: receipt.sequence,
    })
}

impl ControlAction {
    fn command_id(&self) -> &CommandId {
        match self {
            Self::Steer { command_id, .. }
            | Self::Stop { command_id, .. }
            | Self::ResolveInteraction { command_id, .. } => command_id,
        }
    }

    fn durable_command(&self) -> DurableCommand {
        match self {
            Self::Steer { content, .. } => DurableCommand::Steer {
                content: content.clone(),
            },
            Self::Stop { action, .. } => DurableCommand::Stop { action: *action },
            Self::ResolveInteraction {
                interaction_id,
                response,
                ..
            } => DurableCommand::ResolveInteraction {
                interaction_id: interaction_id.clone(),
                response: response.clone(),
            },
        }
    }
}

fn spawn_monitor(
    run: RuntimeRun,
    launch_token: u64,
    active: Arc<Mutex<HashMap<RunId, ActiveRun>>>,
    watch: Arc<StoreWatch>,
) {
    tokio::spawn(async move {
        let run_id = run.run_id.clone();
        let _ = run.wait().await;
        let mut active = active.lock().await;
        if active
            .get(&run_id)
            .is_some_and(|entry| entry.launch_token == launch_token)
        {
            active.remove(&run_id);
        }
        drop(active);
        watch.wake();
    });
}

fn validate_start(command: &StartRunCommand) -> Result<(), RunApiError> {
    if command.input.trim().is_empty() {
        return Err(api_error(
            RunApiErrorCode::InvalidRequest,
            "run input must not be empty",
            None,
            None,
        ));
    }
    if command.workspace.trim().is_empty() {
        return Err(api_error(
            RunApiErrorCode::InvalidRequest,
            "workspace must not be empty",
            None,
            None,
        ));
    }
    Ok(())
}

fn project_run(replay: &RunReplay) -> RunView {
    let snapshot = &replay.snapshot;
    let request = &snapshot.request;
    RunView {
        run_id: request.run_id.clone().unwrap_or_default(),
        parent_run_id: request.parent_run_id.clone(),
        model: request.model.clone(),
        workspace: request.environment.workspace.clone(),
        last_sequence: snapshot.last_sequence,
        terminal: snapshot
            .terminal
            .as_ref()
            .map(|outcome| outcome.terminal.clone()),
        usage: snapshot.usage,
        accounting: snapshot.accounting.clone(),
        runtime_model_requests: snapshot.runtime_model_requests,
        runtime_retries: snapshot.runtime_retries,
        tool_calls: snapshot.tool_calls,
        local_turns: snapshot.local_turns,
    }
}

fn strictly_after(events: Vec<StoredRuntimeEvent>, after_sequence: u64) -> Vec<StoredRuntimeEvent> {
    events
        .into_iter()
        .filter(|event| event.sequence > after_sequence)
        .collect()
}

fn cursor_ahead(run_id: &RunId, requested: u64, last_sequence: u64) -> RunApiError {
    api_error(
        RunApiErrorCode::EventCursorAhead,
        format!("event cursor {requested} is ahead of run {run_id} last sequence {last_sequence}"),
        Some(run_id.clone()),
        None,
    )
}

fn terminal_error(run_id: &RunId, terminal: TerminalState) -> RunApiError {
    api_error(
        RunApiErrorCode::RunTerminal,
        format!("run {run_id} is terminal"),
        Some(run_id.clone()),
        Some(terminal),
    )
}

fn recovery_required(run_id: &RunId) -> RunApiError {
    api_error(
        RunApiErrorCode::RunRecoveryRequired,
        format!("run {run_id} is durable but inactive; resume it before requesting new events"),
        Some(run_id.clone()),
        None,
    )
}

fn ready_error(error: RunReadyError) -> RunApiError {
    match error {
        RunReadyError::Store(error) => store_error(error),
        RunReadyError::RuntimeStopped => api_error(
            RunApiErrorCode::RunStoreFailed,
            "runtime stopped before the Store ready handshake",
            None,
            None,
        ),
    }
}

fn store_error(error: RunStoreError) -> RunApiError {
    match error {
        RunStoreError::AlreadyExists { run_id } => api_error(
            RunApiErrorCode::RunAlreadyExists,
            format!("run {run_id} already exists"),
            Some(run_id),
            None,
        ),
        RunStoreError::NotFound { run_id } => api_error(
            RunApiErrorCode::RunNotFound,
            format!("run {run_id} does not exist"),
            Some(run_id),
            None,
        ),
        RunStoreError::AlreadyRunning { run_id } => api_error(
            RunApiErrorCode::RunAlreadyRunning,
            format!("run {run_id} is already running"),
            Some(run_id),
            None,
        ),
        RunStoreError::AlreadyTerminal { run_id } => api_error(
            RunApiErrorCode::RunTerminal,
            format!("run {run_id} is terminal"),
            Some(run_id),
            None,
        ),
        other => api_error(
            RunApiErrorCode::RunStoreFailed,
            other.to_string(),
            store_error_run_id(&other),
            None,
        ),
    }
}

fn store_error_run_id(error: &RunStoreError) -> Option<RunId> {
    match error {
        RunStoreError::AlreadyExists { run_id }
        | RunStoreError::NotFound { run_id }
        | RunStoreError::AlreadyRunning { run_id }
        | RunStoreError::AlreadyTerminal { run_id }
        | RunStoreError::StaleLease { run_id, .. }
        | RunStoreError::EventConflict { run_id, .. }
        | RunStoreError::Corrupt { run_id, .. } => Some(run_id.clone()),
        RunStoreError::UnsupportedSchema { .. } | RunStoreError::Backend { .. } => None,
    }
}

fn api_error(
    code: RunApiErrorCode,
    message: impl Into<String>,
    run_id: Option<RunId>,
    terminal: Option<TerminalState>,
) -> RunApiError {
    RunApiError {
        code,
        message: message.into(),
        run_id,
        terminal,
    }
}

fn error_result(error: RunApiError) -> RunCommandResult {
    RunCommandResult::Error { error }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use codewhale_protocol::agent_runtime::{
        ModelAccounting, ModelFinishReason, ModelOutput, ModelRequest, ModelStreamEvent,
        ReasoningEffort, RunEnvironment, RunLimits, RunRequest, RuntimeEventKind, ToolDefinition,
        ToolInvocation, ToolOutcome, ToolPolicy, Usage,
    };
    use codewhale_protocol::run_api::RunProductControls;
    use codewhale_runtime::{
        AgentRuntime, CancellationToken, InMemoryRunStore, ModelPort, ModelPortError, ModelStream,
        ToolExecutionError, ToolExecutor,
    };

    use super::*;

    #[derive(Clone, Copy)]
    enum ModelMode {
        Complete,
        Pending,
    }

    struct FixtureModel {
        mode: ModelMode,
    }

    #[async_trait]
    impl ModelPort for FixtureModel {
        async fn stream(
            &self,
            _request: ModelRequest,
        ) -> Result<Box<dyn ModelStream>, ModelPortError> {
            Ok(Box::new(FixtureStream {
                mode: self.mode,
                emitted: false,
            }))
        }

        async fn accounting_snapshot(&self, seal: bool) -> Result<ModelAccounting, ModelPortError> {
            Ok(ModelAccounting {
                sealed: seal,
                complete: true,
                usage_complete: true,
                ..ModelAccounting::default()
            })
        }
    }

    struct FixtureStream {
        mode: ModelMode,
        emitted: bool,
    }

    #[async_trait]
    impl ModelStream for FixtureStream {
        async fn next(&mut self) -> Option<Result<ModelStreamEvent, ModelPortError>> {
            match self.mode {
                ModelMode::Pending => pending().await,
                ModelMode::Complete if self.emitted => None,
                ModelMode::Complete => {
                    self.emitted = true;
                    Some(Ok(ModelStreamEvent::Completed {
                        output: ModelOutput {
                            content: "完成".to_owned(),
                            reasoning_content: None,
                            tool_calls: Vec::new(),
                            finish_reason: ModelFinishReason::Stop,
                            usage: Usage {
                                input_tokens: 4,
                                output_tokens: 1,
                                ..Usage::default()
                            },
                        },
                    }))
                }
            }
        }
    }

    #[derive(Default)]
    struct FixtureTools;

    #[async_trait]
    impl ToolExecutor for FixtureTools {
        fn definitions(&self) -> Vec<ToolDefinition> {
            Vec::new()
        }

        async fn execute(
            &self,
            _invocation: ToolInvocation,
            _cancellation: CancellationToken,
        ) -> Result<ToolOutcome, ToolExecutionError> {
            unreachable!("fixture has no tools")
        }
    }

    struct FixtureComposition {
        mode: ModelMode,
        starts: AtomicUsize,
        resumes: AtomicUsize,
        last_resume_sequence: AtomicU64,
        ready_gate: Option<Arc<ReadyGate>>,
    }

    impl FixtureComposition {
        fn new(mode: ModelMode) -> Self {
            Self {
                mode,
                starts: AtomicUsize::new(0),
                resumes: AtomicUsize::new(0),
                last_resume_sequence: AtomicU64::new(0),
                ready_gate: None,
            }
        }

        fn gated(mode: ModelMode, ready_gate: Arc<ReadyGate>) -> Self {
            Self {
                mode,
                starts: AtomicUsize::new(0),
                resumes: AtomicUsize::new(0),
                last_resume_sequence: AtomicU64::new(0),
                ready_gate: Some(ready_gate),
            }
        }

        fn runtime(
            &self,
            store: Arc<dyn RunStore>,
            sink: Arc<dyn RuntimeEventSink>,
        ) -> Arc<AgentRuntime> {
            let sink: Arc<dyn RuntimeEventSink> = match &self.ready_gate {
                Some(gate) => Arc::new(GatedSink {
                    inner: sink,
                    gate: gate.clone(),
                    blocked: AtomicBool::new(false),
                }),
                None => sink,
            };
            Arc::new(AgentRuntime::new(
                Arc::new(FixtureModel { mode: self.mode }),
                Arc::new(FixtureTools),
                sink,
                store,
            ))
        }
    }

    struct ReadyGate {
        entered: tokio::sync::Semaphore,
        release: tokio::sync::Semaphore,
    }

    impl Default for ReadyGate {
        fn default() -> Self {
            Self {
                entered: tokio::sync::Semaphore::new(0),
                release: tokio::sync::Semaphore::new(0),
            }
        }
    }

    struct GatedSink {
        inner: Arc<dyn RuntimeEventSink>,
        gate: Arc<ReadyGate>,
        blocked: AtomicBool,
    }

    #[async_trait]
    impl RuntimeEventSink for GatedSink {
        async fn emit(&self, event: StoredRuntimeEvent) {
            if !self.blocked.swap(true, Ordering::AcqRel) {
                self.gate.entered.add_permits(1);
                let permit = self.gate.release.acquire().await.expect("ready gate open");
                permit.forget();
            }
            self.inner.emit(event).await;
        }
    }

    #[async_trait]
    impl RunComposition for FixtureComposition {
        async fn start(
            &self,
            command: StartRunCommand,
            store: Arc<dyn RunStore>,
            sink: Arc<dyn RuntimeEventSink>,
        ) -> Result<RuntimeRun, RunApiError> {
            self.starts.fetch_add(1, Ordering::AcqRel);
            let mut request = request_from(command);
            request.environment.provider = "deepseek".to_owned();
            request.environment.tool_catalog_sha256 = Some("fixture-catalog".to_owned());
            request.environment.execution_fingerprint_sha256 = Some("fixture-execution".to_owned());
            Ok(self.runtime(store, sink).start(request))
        }

        async fn resume(
            &self,
            run_id: RunId,
            replay: RunReplay,
            store: Arc<dyn RunStore>,
            sink: Arc<dyn RuntimeEventSink>,
        ) -> Result<RuntimeRun, RunApiError> {
            self.resumes.fetch_add(1, Ordering::AcqRel);
            self.last_resume_sequence
                .store(replay.snapshot.last_sequence, Ordering::Release);
            Ok(self.runtime(store, sink).resume(run_id))
        }
    }

    fn request_from(command: StartRunCommand) -> RunRequest {
        let mut request = RunRequest::new(command.input, "你是 CodeWhale 编码 Agent");
        request.model = command
            .model
            .unwrap_or_else(|| "deepseek-v4-flash".to_owned());
        request.reasoning_effort = command.reasoning_effort;
        request.max_output_tokens = command.max_output_tokens;
        request.streaming = command.streaming;
        request.tool_policy = command.tool_policy;
        request.limits = command.limits;
        request.environment = RunEnvironment {
            workspace: command.workspace,
            auto_approve: command.controls.auto_approve,
            trust_mode: command.controls.trust_mode,
            allow_sandbox_elevation: command.controls.allow_sandbox_elevation,
            sandbox: command.controls.sandbox,
            ..RunEnvironment::default()
        };
        request
    }

    fn start_command(input: &str) -> StartRunCommand {
        StartRunCommand {
            input: input.to_owned(),
            workspace: "/workspace/project".to_owned(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(4_096),
            max_api_requests: std::num::NonZeroU32::new(6),
            streaming: true,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits {
                model_event_idle_ms: None,
                ..RunLimits::default()
            },
            controls: RunProductControls::default(),
        }
    }

    fn envelope(request_id: &str, command: RunCommand) -> RunCommandEnvelope {
        RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: request_id.to_owned(),
            command,
        }
    }

    fn run_result(response: RunCommandResponse) -> RunView {
        match response.result {
            RunCommandResult::Run { run } => *run,
            other => panic!("expected run result, got {other:?}"),
        }
    }

    fn error_code(response: RunCommandResponse) -> RunApiErrorCode {
        error(response).code
    }

    fn error(response: RunCommandResponse) -> RunApiError {
        match response.result {
            RunCommandResult::Error { error } => error,
            other => panic!("expected error result, got {other:?}"),
        }
    }

    async fn new_fixture(
        mode: ModelMode,
    ) -> (
        Arc<AgentApplication>,
        Arc<InMemoryRunStore>,
        Arc<FixtureComposition>,
    ) {
        let store = Arc::new(InMemoryRunStore::default());
        let composition = Arc::new(FixtureComposition::new(mode));
        let app = Arc::new(AgentApplication::new(store.clone(), composition.clone()));
        (app, store, composition)
    }

    async fn wait_terminal(store: &dyn RunStore, run_id: &RunId) -> RunReplay {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let replay = store
                    .load(run_id)
                    .await
                    .expect("load run")
                    .expect("run exists");
                if replay.snapshot.terminal.is_some() {
                    return replay;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("run reaches terminal")
    }

    async fn wait_for_event(
        store: &InMemoryRunStore,
        run_id: &RunId,
        predicate: impl Fn(&RuntimeEventKind) -> bool,
    ) -> RunReplay {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let replay = store
                    .load(run_id)
                    .await
                    .expect("load run")
                    .expect("run exists");
                if replay.events.iter().any(|event| predicate(&event.event)) {
                    return replay;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("event is committed")
    }

    async fn seed_resumable(store: &InMemoryRunStore, run_id: &str) -> RunId {
        let run_id = RunId::from(run_id);
        let mut request = request_from(start_command("恢复测试"));
        request.run_id = Some(run_id.clone());
        request.environment.provider = "deepseek".to_owned();
        request.environment.tool_catalog_sha256 = Some("fixture-catalog".to_owned());
        request.environment.execution_fingerprint_sha256 = Some("fixture-execution".to_owned());
        let created = store.create(request).await.expect("seed run");
        store
            .release(&created.lease)
            .await
            .expect("release seed lease");
        run_id
    }

    #[tokio::test]
    async fn replay_only_composition_needs_no_credential_or_network() {
        let port = ReplayOnlyModelPort;
        let accounting = port
            .accounting_snapshot(false)
            .await
            .expect("read-only replay accounting is local");
        assert!(accounting.complete);
        assert!(accounting.usage_complete);

        let store = InMemoryRunStore::default();
        let run_id = seed_resumable(&store, "lazy-live-model").await;
        let mut replay = store
            .load(&run_id)
            .await
            .expect("load local run")
            .expect("run exists");
        assert!(resume_needs_live_model(&replay));

        replay
            .snapshot
            .pending_children
            .push(RunId::from("pending-child"));
        assert!(!resume_needs_live_model(&replay));
    }

    #[tokio::test]
    async fn start_acknowledges_only_after_durable_create_and_projects_store_truth() {
        let (app, store, _) = new_fixture(ModelMode::Pending).await;
        let response = app
            .execute(envelope(
                "start-1",
                RunCommand::Start(start_command("修复边界错误")),
            ))
            .await;
        let run = run_result(response);
        assert!(run.last_sequence >= 1);
        assert_eq!(run.workspace, "/workspace/project");
        assert_eq!(run.model, "deepseek-v4-flash");
        let replay = store
            .load(&run.run_id)
            .await
            .expect("load")
            .expect("durable run");
        assert!(matches!(
            replay.events.first().map(|event| &event.event),
            Some(RuntimeEventKind::RunCreated { .. })
        ));

        let fetched = run_result(
            app.execute(envelope(
                "get-1",
                RunCommand::Get {
                    run_id: run.run_id.clone(),
                },
            ))
            .await,
        );
        assert_eq!(fetched.run_id, run.run_id);
        assert!(fetched.last_sequence >= run.last_sequence);

        let _ = app
            .execute(envelope(
                "cancel-1",
                RunCommand::Cancel { run_id: run.run_id },
            ))
            .await;
    }

    #[tokio::test]
    async fn terminal_resume_is_store_replay_without_composition() {
        let (app, store, composition) = new_fixture(ModelMode::Complete).await;
        let run = run_result(
            app.execute(envelope(
                "start",
                RunCommand::Start(start_command("立即完成")),
            ))
            .await,
        );
        let terminal = wait_terminal(store.as_ref(), &run.run_id).await;
        let before_events = terminal.events.clone();
        assert_eq!(composition.resumes.load(Ordering::Acquire), 0);

        let resumed = run_result(
            app.execute(envelope(
                "resume",
                RunCommand::Resume {
                    run_id: run.run_id.clone(),
                    expected_workspace: Some("/workspace/project".to_owned()),
                },
            ))
            .await,
        );
        assert!(resumed.terminal.is_some());
        assert_eq!(composition.resumes.load(Ordering::Acquire), 0);
        let mismatch = error(
            app.execute(envelope(
                "resume-wrong-workspace",
                RunCommand::Resume {
                    run_id: run.run_id.clone(),
                    expected_workspace: Some("/workspace/other".to_owned()),
                },
            ))
            .await,
        );
        assert_eq!(mismatch.code, RunApiErrorCode::RunEnvironmentMismatch);
        assert!(
            mismatch
                .message
                .starts_with("run_resume_workspace_mismatch：")
        );
        assert_eq!(composition.resumes.load(Ordering::Acquire), 0);
        assert_eq!(
            store
                .load(&run.run_id)
                .await
                .expect("load")
                .expect("run")
                .events,
            before_events
        );
    }

    #[tokio::test]
    async fn sqlite_terminal_replay_survives_application_rebuild_without_composition() {
        let directory = tempfile::tempdir().expect("temporary state directory");
        let database = directory.path().join("state.db");
        let first_store = Arc::new(
            codewhale_state::StateStore::open(Some(database.clone())).expect("open first store"),
        );
        let first_composition = Arc::new(FixtureComposition::new(ModelMode::Complete));
        let first_app = AgentApplication::new(first_store.clone(), first_composition.clone());

        let run = run_result(
            first_app
                .execute(envelope(
                    "start-persistent",
                    RunCommand::Start(start_command("持久终态重放")),
                ))
                .await,
        );
        let terminal = wait_terminal(first_store.as_ref(), &run.run_id).await;
        let frozen_events = terminal.events.clone();
        assert_eq!(first_composition.starts.load(Ordering::Acquire), 1);
        drop(first_app);
        drop(first_store);

        let reopened_store = Arc::new(
            codewhale_state::StateStore::open(Some(database)).expect("reopen canonical store"),
        );
        let replay_only_composition = Arc::new(FixtureComposition::new(ModelMode::Pending));
        let rebuilt_app =
            AgentApplication::new(reopened_store.clone(), replay_only_composition.clone());

        let fetched = run_result(
            rebuilt_app
                .execute(envelope(
                    "get-persistent",
                    RunCommand::Get {
                        run_id: run.run_id.clone(),
                    },
                ))
                .await,
        );
        assert_eq!(fetched.terminal, project_run(&terminal).terminal);

        let events = rebuilt_app
            .execute(envelope(
                "events-persistent",
                RunCommand::Events {
                    run_id: run.run_id.clone(),
                    after_sequence: 0,
                },
            ))
            .await;
        assert!(matches!(
            events.result,
            RunCommandResult::Events { ref events, .. } if events == &frozen_events
        ));

        let resumed = run_result(
            rebuilt_app
                .execute(envelope(
                    "resume-persistent",
                    RunCommand::Resume {
                        run_id: run.run_id.clone(),
                        expected_workspace: None,
                    },
                ))
                .await,
        );
        assert_eq!(resumed.terminal, fetched.terminal);
        assert_eq!(replay_only_composition.starts.load(Ordering::Acquire), 0);
        assert_eq!(replay_only_composition.resumes.load(Ordering::Acquire), 0);
        assert_eq!(
            reopened_store
                .load(&run.run_id)
                .await
                .expect("load replayed run")
                .expect("persistent run")
                .events,
            frozen_events
        );
    }

    #[tokio::test]
    async fn canonical_commands_use_one_store_and_durable_typed_controls() {
        let (app, store, composition) = new_fixture(ModelMode::Pending).await;
        let seeded = seed_resumable(&store, "resumable-run").await;
        let resumed = run_result(
            app.execute(envelope(
                "resume",
                RunCommand::Resume {
                    run_id: seeded.clone(),
                    expected_workspace: Some("/workspace/project".to_owned()),
                },
            ))
            .await,
        );
        assert_eq!(resumed.run_id, seeded);
        assert_eq!(composition.resumes.load(Ordering::Acquire), 1);
        assert_eq!(
            composition.last_resume_sequence.load(Ordering::Acquire),
            1,
            "composition must receive the exact replay already loaded by the application"
        );

        let steer = app
            .execute(envelope(
                "steer",
                RunCommand::Steer {
                    run_id: seeded.clone(),
                    content: "先检查测试".to_owned(),
                },
            ))
            .await;
        let accepted_sequence = match steer.result {
            RunCommandResult::Accepted { last_sequence, .. } => last_sequence,
            other => panic!("expected durable steer acceptance, got {other:?}"),
        };
        let steered = wait_for_event(&store, &seeded, |event| {
            matches!(event, RuntimeEventKind::SteerQueued { .. })
        })
        .await;
        assert_eq!(steered.snapshot.last_sequence, accepted_sequence);
        let retried = app
            .execute(envelope(
                "steer",
                RunCommand::Steer {
                    run_id: seeded.clone(),
                    content: "先检查测试".to_owned(),
                },
            ))
            .await;
        assert!(matches!(
            retried.result,
            RunCommandResult::Accepted { last_sequence, .. }
                if last_sequence == accepted_sequence
        ));
        let conflict = error(
            app.execute(envelope(
                "steer",
                RunCommand::Steer {
                    run_id: seeded.clone(),
                    content: "复用同一 request_id 的不同内容".to_owned(),
                },
            ))
            .await,
        );
        assert_eq!(conflict.code, RunApiErrorCode::InvalidRequest);
        let stop_steered = app
            .execute(envelope(
                "cancel-steered",
                RunCommand::Cancel {
                    run_id: seeded.clone(),
                },
            ))
            .await;
        assert!(matches!(
            stop_steered.result,
            RunCommandResult::Accepted { .. }
        ));
        wait_terminal(store.as_ref(), &seeded).await;

        let interrupt_run = run_result(
            app.execute(envelope(
                "start-interrupt",
                RunCommand::Start(start_command("等待中断")),
            ))
            .await,
        );
        let interrupted = app
            .execute(envelope(
                "interrupt",
                RunCommand::Interrupt {
                    run_id: interrupt_run.run_id.clone(),
                },
            ))
            .await;
        assert!(matches!(
            interrupted.result,
            RunCommandResult::Accepted { .. }
        ));
        wait_terminal(store.as_ref(), &interrupt_run.run_id).await;

        let cancel_run = run_result(
            app.execute(envelope(
                "start-cancel",
                RunCommand::Start(start_command("等待取消")),
            ))
            .await,
        );
        let cancelled = app
            .execute(envelope(
                "cancel",
                RunCommand::Cancel {
                    run_id: cancel_run.run_id.clone(),
                },
            ))
            .await;
        assert!(matches!(
            cancelled.result,
            RunCommandResult::Accepted { .. }
        ));
        wait_terminal(store.as_ref(), &cancel_run.run_id).await;

        let events = app
            .execute(envelope(
                "events",
                RunCommand::Events {
                    run_id: cancel_run.run_id.clone(),
                    after_sequence: 0,
                },
            ))
            .await;
        assert!(matches!(
            events.result,
            RunCommandResult::Events { ref events, .. } if !events.is_empty()
        ));
        let fetched = run_result(
            app.execute(envelope(
                "get",
                RunCommand::Get {
                    run_id: cancel_run.run_id,
                },
            ))
            .await,
        );
        assert!(fetched.terminal.is_some());
    }

    #[tokio::test]
    async fn concurrent_same_request_id_with_different_payload_is_never_double_accepted() {
        let (app, store, _) = new_fixture(ModelMode::Pending).await;
        let run = run_result(
            app.execute(envelope(
                "start-concurrent-command",
                RunCommand::Start(start_command("并发控制")),
            ))
            .await,
        );
        wait_for_event(&store, &run.run_id, |event| {
            matches!(event, RuntimeEventKind::ModelRequestInFlight { .. })
        })
        .await;

        let first = app.execute(envelope(
            "same-request-id",
            RunCommand::Steer {
                run_id: run.run_id.clone(),
                content: "内容 A".to_owned(),
            },
        ));
        let second = app.execute(envelope(
            "same-request-id",
            RunCommand::Steer {
                run_id: run.run_id.clone(),
                content: "内容 B".to_owned(),
            },
        ));
        let (first, second) = tokio::join!(first, second);
        let results = [first.result, second.result];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, RunCommandResult::Accepted { .. }))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    RunCommandResult::Error {
                        error: RunApiError {
                            code: RunApiErrorCode::InvalidRequest,
                            ..
                        }
                    }
                ))
                .count(),
            1
        );

        let replay = store.load(&run.run_id).await.unwrap().unwrap();
        assert_eq!(
            replay
                .events
                .iter()
                .filter(|event| matches!(
                    &event.event,
                    RuntimeEventKind::SteerQueued { command_id, .. }
                        if command_id == &CommandId::from("same-request-id")
                ))
                .count(),
            1
        );
        let stopped = app
            .execute(envelope(
                "cancel-concurrent-command",
                RunCommand::Cancel {
                    run_id: run.run_id.clone(),
                },
            ))
            .await;
        assert!(matches!(stopped.result, RunCommandResult::Accepted { .. }));
        wait_terminal(store.as_ref(), &run.run_id).await;
    }

    #[tokio::test]
    async fn terminal_control_races_replay_the_exact_durable_receipt() {
        let (app, store, _) = new_fixture(ModelMode::Pending).await;
        let same_payload_run = run_result(
            app.execute(envelope(
                "start-same-cancel",
                RunCommand::Start(start_command("并发相同取消")),
            ))
            .await,
        );
        wait_for_event(&store, &same_payload_run.run_id, |event| {
            matches!(event, RuntimeEventKind::ModelRequestInFlight { .. })
        })
        .await;
        let first = app.execute(envelope(
            "same-cancel-id",
            RunCommand::Cancel {
                run_id: same_payload_run.run_id.clone(),
            },
        ));
        let second = app.execute(envelope(
            "same-cancel-id",
            RunCommand::Cancel {
                run_id: same_payload_run.run_id.clone(),
            },
        ));
        let (first, second) = tokio::join!(first, second);
        let sequences = [first.result, second.result]
            .into_iter()
            .map(|result| match result {
                RunCommandResult::Accepted { last_sequence, .. } => last_sequence,
                other => panic!("same cancel must replay accepted receipt, got {other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(sequences[0], sequences[1]);
        let same_payload_replay = wait_terminal(store.as_ref(), &same_payload_run.run_id).await;
        assert_eq!(
            same_payload_replay
                .events
                .iter()
                .filter(|event| matches!(event.event, RuntimeEventKind::ControlRequested { .. }))
                .count(),
            1
        );
        assert_eq!(
            same_payload_replay
                .events
                .iter()
                .filter(|event| event.event.is_terminal())
                .count(),
            1
        );

        let conflicting_run = run_result(
            app.execute(envelope(
                "start-conflicting-stop",
                RunCommand::Start(start_command("并发冲突停止")),
            ))
            .await,
        );
        wait_for_event(&store, &conflicting_run.run_id, |event| {
            matches!(event, RuntimeEventKind::ModelRequestInFlight { .. })
        })
        .await;
        let cancel = app.execute(envelope(
            "same-stop-id",
            RunCommand::Cancel {
                run_id: conflicting_run.run_id.clone(),
            },
        ));
        let interrupt = app.execute(envelope(
            "same-stop-id",
            RunCommand::Interrupt {
                run_id: conflicting_run.run_id.clone(),
            },
        ));
        let (cancel, interrupt) = tokio::join!(cancel, interrupt);
        let results = [cancel.result, interrupt.result];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, RunCommandResult::Accepted { .. }))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    RunCommandResult::Error {
                        error: RunApiError {
                            code: RunApiErrorCode::InvalidRequest,
                            ..
                        }
                    }
                ))
                .count(),
            1
        );
        let conflicting_replay = wait_terminal(store.as_ref(), &conflicting_run.run_id).await;
        assert_eq!(
            conflicting_replay
                .events
                .iter()
                .filter(|event| matches!(event.event, RuntimeEventKind::ControlRequested { .. }))
                .count(),
            1
        );
        assert_eq!(
            conflicting_replay
                .events
                .iter()
                .filter(|event| event.event.is_terminal())
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn resume_workspace_mismatch_precedes_composition() {
        let (app, store, composition) = new_fixture(ModelMode::Pending).await;
        let run_id = seed_resumable(&store, "workspace-mismatch").await;

        let mismatch = error(
            app.execute(envelope(
                "resume-workspace-mismatch",
                RunCommand::Resume {
                    run_id,
                    expected_workspace: Some("/workspace/other".to_owned()),
                },
            ))
            .await,
        );
        assert_eq!(mismatch.code, RunApiErrorCode::RunEnvironmentMismatch);
        assert!(
            mismatch
                .message
                .starts_with("run_resume_workspace_mismatch：")
        );
        assert_eq!(composition.resumes.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn commands_return_typed_not_found_not_active_terminal_and_input_errors() {
        let (app, store, composition) = new_fixture(ModelMode::Pending).await;
        let bad_schema = RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION + 1,
            request_id: "bad-schema".to_owned(),
            command: RunCommand::Get {
                run_id: RunId::from("missing"),
            },
        };
        assert_eq!(
            error_code(app.execute(bad_schema).await),
            RunApiErrorCode::InvalidRequest
        );
        assert_eq!(
            error_code(
                app.execute(envelope("empty", RunCommand::Start(start_command("   ")),))
                    .await
            ),
            RunApiErrorCode::InvalidRequest
        );
        assert_eq!(
            error_code(
                app.execute(envelope(
                    "missing",
                    RunCommand::Cancel {
                        run_id: RunId::from("missing"),
                    },
                ))
                .await
            ),
            RunApiErrorCode::RunNotFound
        );

        let inactive = seed_resumable(&store, "inactive").await;
        assert_eq!(
            error_code(
                app.execute(envelope(
                    "inactive",
                    RunCommand::Interrupt { run_id: inactive },
                ))
                .await
            ),
            RunApiErrorCode::RunNotActive
        );

        let active = run_result(
            app.execute(envelope(
                "active",
                RunCommand::Start(start_command("保持活跃")),
            ))
            .await,
        );
        let active_mismatch = error(
            app.execute(envelope(
                "active-workspace-mismatch",
                RunCommand::Resume {
                    run_id: active.run_id.clone(),
                    expected_workspace: Some("/workspace/other".to_owned()),
                },
            ))
            .await,
        );
        assert_eq!(
            active_mismatch.code,
            RunApiErrorCode::RunEnvironmentMismatch
        );
        assert!(
            active_mismatch
                .message
                .starts_with("run_resume_workspace_mismatch：")
        );
        assert_eq!(composition.resumes.load(Ordering::Acquire), 0);
        assert_eq!(
            error_code(
                app.execute(envelope(
                    "double-resume",
                    RunCommand::Resume {
                        run_id: active.run_id.clone(),
                        expected_workspace: None,
                    },
                ))
                .await
            ),
            RunApiErrorCode::RunAlreadyRunning
        );
        let _ = app
            .execute(envelope(
                "cancel-active",
                RunCommand::Cancel {
                    run_id: active.run_id.clone(),
                },
            ))
            .await;
        wait_terminal(store.as_ref(), &active.run_id).await;
        assert_eq!(
            error_code(
                app.execute(envelope(
                    "terminal-control",
                    RunCommand::Steer {
                        run_id: active.run_id,
                        content: "继续".to_owned(),
                    },
                ))
                .await
            ),
            RunApiErrorCode::RunTerminal
        );
    }

    #[tokio::test]
    async fn event_cursor_is_strictly_exclusive_and_rejects_cursor_ahead() {
        let (app, store, _) = new_fixture(ModelMode::Complete).await;
        let run = run_result(
            app.execute(envelope(
                "start",
                RunCommand::Start(start_command("游标测试")),
            ))
            .await,
        );
        let replay = wait_terminal(store.as_ref(), &run.run_id).await;
        let after = replay.events[1].sequence;
        let response = app
            .execute(envelope(
                "events",
                RunCommand::Events {
                    run_id: run.run_id.clone(),
                    after_sequence: after,
                },
            ))
            .await;
        match response.result {
            RunCommandResult::Events { events, .. } => {
                assert!(events.iter().all(|event| event.sequence > after));
                assert_eq!(events.len(), replay.events.len() - 2);
            }
            other => panic!("expected events, got {other:?}"),
        }
        assert_eq!(
            error_code(
                app.execute(envelope(
                    "ahead",
                    RunCommand::Events {
                        run_id: run.run_id,
                        after_sequence: replay.snapshot.last_sequence + 1,
                    },
                ))
                .await
            ),
            RunApiErrorCode::EventCursorAhead
        );
    }

    #[tokio::test]
    async fn wait_events_subscribes_before_store_query_and_notify_only_wakes() {
        let (app, store, _) = new_fixture(ModelMode::Pending).await;
        let run = run_result(
            app.execute(envelope(
                "start",
                RunCommand::Start(start_command("等待事件")),
            ))
            .await,
        );
        let replay = wait_for_event(&store, &run.run_id, |event| {
            matches!(event, RuntimeEventKind::ModelRequestInFlight { .. })
        })
        .await;
        let cursor = replay.snapshot.last_sequence;
        let waiter_app = app.clone();
        let waiter_run_id = run.run_id.clone();
        let waiter =
            tokio::spawn(async move { waiter_app.wait_events(&waiter_run_id, cursor).await });
        tokio::task::yield_now().await;
        let accepted = app
            .execute(envelope(
                "cancel",
                RunCommand::Cancel { run_id: run.run_id },
            ))
            .await;
        assert!(matches!(accepted.result, RunCommandResult::Accepted { .. }));
        let result = tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("waiter wakes")
            .expect("waiter task");
        assert!(matches!(
            result,
            RunCommandResult::Events { events, .. }
                if !events.is_empty() && events.iter().all(|event| event.sequence > cursor)
        ));
    }

    #[tokio::test]
    async fn wait_events_requires_explicit_recovery_for_an_inactive_durable_run() {
        let (app, store, _) = new_fixture(ModelMode::Pending).await;
        let run_id = seed_resumable(&store, "recovery-required").await;
        let replay = store
            .load(&run_id)
            .await
            .expect("load seeded run")
            .expect("seeded run exists");

        let result = tokio::time::timeout(
            Duration::from_millis(100),
            app.wait_events(&run_id, replay.snapshot.last_sequence),
        )
        .await
        .expect("inactive durable run must not leave an event transport hanging");

        assert!(matches!(
            result,
            RunCommandResult::Error { error }
                if error.code == RunApiErrorCode::RunRecoveryRequired
                    && error.run_id.as_ref() == Some(&run_id)
        ));

        let polled = app
            .execute(envelope(
                "poll-recovery-required",
                RunCommand::Events {
                    run_id: run_id.clone(),
                    after_sequence: replay.snapshot.last_sequence,
                },
            ))
            .await;
        assert_eq!(error_code(polled), RunApiErrorCode::RunRecoveryRequired);
    }

    #[tokio::test]
    async fn concurrent_terminal_monitors_remove_only_their_active_launches() {
        let (app, store, _) = new_fixture(ModelMode::Complete).await;
        let mut starts = Vec::new();
        for index in 0..24 {
            let app = app.clone();
            starts.push(tokio::spawn(async move {
                run_result(
                    app.execute(envelope(
                        &format!("start-{index}"),
                        RunCommand::Start(start_command(&format!("任务 {index}"))),
                    ))
                    .await,
                )
            }));
        }
        let mut run_ids = Vec::new();
        for start in starts {
            run_ids.push(start.await.expect("start task").run_id);
        }
        for run_id in run_ids {
            wait_terminal(store.as_ref(), &run_id).await;
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if app.active.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("active registry cleans up");
    }

    #[tokio::test]
    async fn aborted_activation_cancels_run_before_control_registration() {
        let store = Arc::new(InMemoryRunStore::default());
        let ready_gate = Arc::new(ReadyGate::default());
        let composition = Arc::new(FixtureComposition::gated(
            ModelMode::Pending,
            ready_gate.clone(),
        ));
        let app = Arc::new(AgentApplication::new(store.clone(), composition));
        let run_id = seed_resumable(&store, "aborted-activation").await;

        let task_app = app.clone();
        let task_run_id = run_id.clone();
        let activation = tokio::spawn(async move {
            task_app
                .execute(envelope(
                    "resume-abort",
                    RunCommand::Resume {
                        run_id: task_run_id,
                        expected_workspace: None,
                    },
                ))
                .await
        });
        let entered = ready_gate
            .entered
            .acquire()
            .await
            .expect("ready gate entered");
        entered.forget();

        activation.abort();
        assert!(activation.await.is_err());
        ready_gate.release.add_permits(1);

        let replay = wait_terminal(store.as_ref(), &run_id).await;
        assert!(matches!(
            replay.snapshot.terminal.map(|outcome| outcome.terminal),
            Some(TerminalState::Cancelled)
        ));
        assert!(app.active.lock().await.is_empty());
    }

    #[test]
    fn store_failures_map_to_stable_application_codes() {
        assert_eq!(
            store_error(RunStoreError::AlreadyExists {
                run_id: RunId::from("duplicate")
            })
            .code,
            RunApiErrorCode::RunAlreadyExists
        );
        assert_eq!(
            store_error(RunStoreError::AlreadyRunning {
                run_id: RunId::from("active")
            })
            .code,
            RunApiErrorCode::RunAlreadyRunning
        );
        assert_eq!(
            store_error(RunStoreError::Backend {
                message: "disk unavailable".to_owned()
            })
            .code,
            RunApiErrorCode::RunStoreFailed
        );
    }
}
