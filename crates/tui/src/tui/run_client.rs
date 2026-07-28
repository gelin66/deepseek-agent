//! Concrete interactive-TUI client for the canonical Run API.
//!
//! This module owns no model loop, event translation, or durable state. It
//! submits commands directly to [`AgentApplication`] and forwards committed
//! [`StoredRuntimeEvent`] values unchanged to the presentation layer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dse_app::AgentApplication;
use dse_localization::{MessageId, tr};
use dse_protocol::agent_runtime::{
    InteractionId, RunId, RuntimeEventKind, StoredRuntimeEvent, TerminalState,
    UserInteractionResponse,
};
use dse_protocol::run_api::{
    ContinueRunCommand, MAX_RUN_LIST_LIMIT, PendingCreationSummary, RUN_API_SCHEMA_VERSION,
    RootRunSummary, RunApiError, RunCommand, RunCommandEnvelope, RunCommandResult, RunView,
    StartRunCommand,
};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

const RUN_EVENT_BUFFER_CAPACITY: usize = 256;

/// One workspace-scoped root as seen through the canonical Run API.
///
/// `summary` owns list ordering and timestamps. `run` supplies the exact
/// TaskContract and terminal taxonomy for presentation. Neither value is
/// stored by the TUI.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiRootRun {
    pub summary: RootRunSummary,
    pub run: RunView,
}

#[derive(Debug)]
pub enum TuiRunClientError {
    ActiveRun {
        run_id: RunId,
    },
    LaunchInFlight,
    NoActiveRun,
    AmbiguousPendingCreations {
        workspace: String,
        creation_request_ids: Vec<String>,
    },
    Application(RunApiError),
    UnexpectedResult {
        operation: &'static str,
        result: Box<RunCommandResult>,
    },
}

impl std::fmt::Display for TuiRunClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ActiveRun { run_id } => {
                tr(MessageId::RunClientActive).replace("{run_id}", &run_id.to_string())
            }
            Self::LaunchInFlight => tr(MessageId::RunClientLaunchInFlight).into_owned(),
            Self::NoActiveRun => tr(MessageId::RunClientNoActive).into_owned(),
            Self::AmbiguousPendingCreations {
                workspace,
                creation_request_ids,
            } => tr(MessageId::RunClientAmbiguousPending)
                .replace("{workspace}", &format!("{workspace:?}"))
                .replace(
                    "{creation_request_ids}",
                    &format!("{creation_request_ids:?}"),
                ),
            Self::Application(error) => {
                tr(MessageId::RunClientApplicationError).replace("{error}", &format!("{error:?}"))
            }
            Self::UnexpectedResult { operation, result } => {
                tr(MessageId::RunClientUnexpectedResult)
                    .replace("{operation}", operation)
                    .replace("{result}", &format!("{result:?}"))
            }
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for TuiRunClientError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TuiRunClientSnapshot {
    pub current_active_root: Option<RunId>,
    pub latest_terminal_root: Option<RunId>,
    pub cursors: HashMap<RunId, u64>,
}

#[derive(Debug, Default)]
struct TuiRunClientState {
    current_active_root: Option<RunId>,
    latest_terminal_root: Option<RunId>,
    cursors: HashMap<RunId, u64>,
    monitored_runs: HashSet<RunId>,
    launch_in_flight: bool,
}

impl TuiRunClientState {
    fn snapshot(&self) -> TuiRunClientSnapshot {
        TuiRunClientSnapshot {
            current_active_root: self.current_active_root.clone(),
            latest_terminal_root: self.latest_terminal_root.clone(),
            cursors: self.cursors.clone(),
        }
    }

    fn plan_submit(&self, mut fresh: StartRunCommand) -> Result<RunCommand, TuiRunClientError> {
        if let Some(run_id) = &self.current_active_root {
            return Err(TuiRunClientError::ActiveRun {
                run_id: run_id.clone(),
            });
        }
        if let Some(run_id) = &self.latest_terminal_root {
            return Ok(RunCommand::Continue(ContinueRunCommand {
                run_id: run_id.clone(),
                task: fresh.task,
                expected_workspace: Some(fresh.workspace),
            }));
        }
        fresh.controls.interactive = true;
        Ok(RunCommand::Start(fresh))
    }

    fn begin_launch(&mut self) -> Result<(), TuiRunClientError> {
        if self.launch_in_flight {
            return Err(TuiRunClientError::LaunchInFlight);
        }
        self.launch_in_flight = true;
        Ok(())
    }

    fn finish_launch(&mut self) {
        self.launch_in_flight = false;
    }

    fn observe_run(&mut self, run: &RunView) -> Result<bool, TuiRunClientError> {
        let cursor = *self.cursors.entry(run.run_id.clone()).or_insert(0);
        if let Some(terminal) = run.terminal.as_ref() {
            if self.current_active_root.as_ref() == Some(&run.run_id) {
                self.current_active_root = None;
            }
            self.latest_terminal_root = terminal_can_continue(terminal).then(|| run.run_id.clone());
        } else {
            if let Some(active) = &self.current_active_root
                && active != &run.run_id
            {
                return Err(TuiRunClientError::ActiveRun {
                    run_id: active.clone(),
                });
            }
            self.current_active_root = Some(run.run_id.clone());
        }
        Ok(cursor < run.last_sequence || run.terminal.is_none())
    }

    fn begin_monitor(&mut self, run_id: &RunId) -> bool {
        self.monitored_runs.insert(run_id.clone())
    }

    fn finish_monitor(&mut self, run_id: &RunId) {
        self.monitored_runs.remove(run_id);
    }

    fn record_event(&mut self, event: &StoredRuntimeEvent) {
        self.cursors.insert(event.run_id.clone(), event.sequence);
        if let RuntimeEventKind::Terminal { outcome } = &event.event {
            if self.current_active_root.as_ref() == Some(&event.run_id) {
                self.current_active_root = None;
            }
            self.latest_terminal_root =
                terminal_can_continue(&outcome.terminal).then(|| event.run_id.clone());
        }
    }
}

#[derive(Debug)]
struct RequestIds {
    client_id: Uuid,
    next: AtomicU64,
}

impl RequestIds {
    fn new() -> Self {
        Self {
            client_id: Uuid::new_v4(),
            next: AtomicU64::new(1),
        }
    }

    fn next(&self, operation: &str) -> String {
        let sequence = self.next.fetch_add(1, Ordering::Relaxed);
        format!("tui-{operation}-{}-{sequence}", self.client_id)
    }
}

fn run_requires_resume(run: &RunView) -> bool {
    run.terminal.is_none()
}

fn terminal_can_continue(terminal: &TerminalState) -> bool {
    !matches!(terminal, TerminalState::RecoveryRequired { .. })
}

/// Thin interactive client for one in-process [`AgentApplication`].
///
/// `event_rx` receives the exact canonical Store events. A caller should keep
/// the receiver alive for as long as it expects the client to monitor runs.
pub struct TuiRunClient {
    application: Arc<AgentApplication>,
    state: Arc<Mutex<TuiRunClientState>>,
    event_tx: mpsc::Sender<StoredRuntimeEvent>,
    request_ids: RequestIds,
}

impl TuiRunClient {
    #[must_use]
    pub fn new(application: Arc<AgentApplication>) -> (Self, mpsc::Receiver<StoredRuntimeEvent>) {
        let (event_tx, event_rx) = mpsc::channel(RUN_EVENT_BUFFER_CAPACITY);
        (
            Self {
                application,
                state: Arc::new(Mutex::new(TuiRunClientState::default())),
                event_tx,
                request_ids: RequestIds::new(),
            },
            event_rx,
        )
    }

    pub async fn snapshot(&self) -> TuiRunClientSnapshot {
        self.state.lock().await.snapshot()
    }

    /// Submit a new user turn.
    ///
    /// The first turn starts an interactive root. Once that root is terminal,
    /// the next turn creates a canonical continuation. Submitting while a root
    /// is active is rejected; callers must use [`Self::steer`] for that intent.
    pub async fn submit(&self, fresh: StartRunCommand) -> Result<RunView, TuiRunClientError> {
        let command = {
            let mut state = self.state.lock().await;
            state.begin_launch()?;
            match state.plan_submit(fresh) {
                Ok(command) => command,
                Err(error) => {
                    state.finish_launch();
                    return Err(error);
                }
            }
        };
        self.execute_launch("submit", command).await
    }

    /// Return the newest canonical root for resume or continuation.
    pub async fn latest_root(
        &self,
        workspace: String,
    ) -> Result<Option<RootRunSummary>, TuiRunClientError> {
        Ok(self.list_roots(workspace, 1).await?.into_iter().next())
    }

    /// List canonical root summaries for one exact workspace.
    pub async fn list_roots(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<RootRunSummary>, TuiRunClientError> {
        let operation = "list-roots";
        let response = self
            .application
            .execute(self.envelope(operation, RunCommand::ListRoots { workspace, limit }))
            .await;
        match response.result {
            RunCommandResult::Runs { runs, .. } => Ok(runs),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult {
                operation,
                result: Box::new(result),
            }),
        }
    }

    /// Build the bounded Run Hub projection using only canonical list/get
    /// commands. The TUI does not read SQLite or derive a second lifecycle.
    pub async fn list_root_views(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<TuiRootRun>, TuiRunClientError> {
        let summaries = self.list_roots(workspace, limit).await?;
        let mut roots = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let run = self
                .execute_run_command(
                    "run-hub-get",
                    RunCommand::Get {
                        run_id: summary.run_id.clone(),
                    },
                )
                .await?;
            roots.push(TuiRootRun { summary, run });
        }
        Ok(roots)
    }

    /// Select an independent root for the next composer submission.
    ///
    /// This clears only the process-local continuation choice. The canonical
    /// source Run and every Store event remain unchanged.
    pub async fn prepare_new_root(&self) -> Result<(), TuiRunClientError> {
        let mut state = self.state.lock().await;
        if let Some(run_id) = state.current_active_root.clone() {
            return Err(TuiRunClientError::ActiveRun { run_id });
        }
        state.latest_terminal_root = None;
        Ok(())
    }

    /// List canonical creation receipts that reserved a Run identity but did
    /// not commit `RunCreated`.
    ///
    /// The payload remains private to the application and Store. The TUI does
    /// not reconstruct the original prompt or maintain a second recovery
    /// journal.
    pub async fn list_pending_creations(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<PendingCreationSummary>, TuiRunClientError> {
        let operation = "list-pending-creations";
        let response = self
            .application
            .execute(self.envelope(
                operation,
                RunCommand::ListPendingCreations { workspace, limit },
            ))
            .await;
        match response.result {
            RunCommandResult::PendingCreations { creations, .. } => Ok(creations),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult {
                operation,
                result: Box::new(result),
            }),
        }
    }

    /// Recover one exact canonical creation receipt and adopt its reserved Run.
    ///
    /// `AgentApplication` owns exact payload replay. This client only supplies
    /// the durable creation identity and then uses the
    /// same adoption/monitoring path as Start, Continue, and Resume.
    pub async fn recover_creation(
        &self,
        creation_request_id: String,
    ) -> Result<RunView, TuiRunClientError> {
        self.begin_launch().await?;
        self.execute_launch(
            "recover-creation",
            RunCommand::RecoverCreation {
                creation_request_id,
            },
        )
        .await
    }

    /// Recover the sole unambiguous pending creation for one workspace.
    ///
    /// Unknown-billing receipts are deliberately sent to canonical recovery:
    /// the application returns its original typed `RunApiError` without
    /// rerouting or reconstructing the prompt. Multiple receipts require an
    /// explicit picker and are never silently reduced to "latest".
    pub async fn recover_pending_for_workspace(
        &self,
        workspace: String,
    ) -> Result<Option<RunView>, TuiRunClientError> {
        let creations = self
            .list_pending_creations(workspace.clone(), MAX_RUN_LIST_LIMIT)
            .await?;
        match creations.as_slice() {
            [] => Ok(None),
            [creation] => self
                .recover_creation(creation.creation_request_id.clone())
                .await
                .map(Some),
            _ => Err(TuiRunClientError::AmbiguousPendingCreations {
                workspace,
                creation_request_ids: creations
                    .into_iter()
                    .map(|creation| creation.creation_request_id)
                    .collect(),
            }),
        }
    }

    /// Attach to a terminal run for replay, or recover a non-terminal run.
    ///
    /// `Get` is read-only and cannot make an inactive durable run active.
    /// Therefore every non-terminal result is followed by canonical `Resume`
    /// before the run becomes this client's active root.
    pub async fn attach_or_resume(
        &self,
        run_id: RunId,
        expected_workspace: Option<String>,
    ) -> Result<RunView, TuiRunClientError> {
        self.begin_launch().await?;
        let loaded = match self
            .execute_run_command("attach-get", RunCommand::Get { run_id })
            .await
        {
            Ok(run) => run,
            Err(error) => {
                self.state.lock().await.finish_launch();
                return Err(error);
            }
        };
        if run_requires_resume(&loaded) {
            let resumed = match self
                .execute_run_command(
                    "attach-resume",
                    RunCommand::Resume {
                        run_id: loaded.run_id,
                        expected_workspace,
                    },
                )
                .await
            {
                Ok(run) => run,
                Err(error) => {
                    self.state.lock().await.finish_launch();
                    return Err(error);
                }
            };
            return self.adopt_launched_run(resumed).await;
        }
        self.adopt_launched_run(loaded).await
    }

    /// Reopen one root selected in the Run Hub and replay it from sequence 1.
    ///
    /// The cursor reset is process-local presentation state. Canonical events
    /// remain append-only and are read from the same RunStore; no model or
    /// tool work is repeated for a terminal Run.
    pub async fn reopen_from_hub(
        &self,
        run_id: RunId,
        expected_workspace: Option<String>,
    ) -> Result<RunView, TuiRunClientError> {
        {
            let mut state = self.state.lock().await;
            if let Some(active) = state.current_active_root.clone() {
                return Err(TuiRunClientError::ActiveRun { run_id: active });
            }
            state.cursors.insert(run_id.clone(), 0);
        }
        self.attach_or_resume(run_id, expected_workspace).await
    }

    pub async fn steer(&self, content: String) -> Result<u64, TuiRunClientError> {
        let run_id = self.active_run_id().await?;
        self.execute_control("steer", RunCommand::Steer { run_id, content })
            .await
    }

    pub async fn interrupt(&self) -> Result<u64, TuiRunClientError> {
        let run_id = self.active_run_id().await?;
        self.execute_control("interrupt", RunCommand::Interrupt { run_id })
            .await
    }

    pub async fn cancel(&self) -> Result<u64, TuiRunClientError> {
        let run_id = self.active_run_id().await?;
        self.execute_control("cancel", RunCommand::Cancel { run_id })
            .await
    }

    pub async fn resolve_interaction(
        &self,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    ) -> Result<u64, TuiRunClientError> {
        let run_id = self.active_run_id().await?;
        self.execute_control(
            "resolve-interaction",
            RunCommand::ResolveInteraction {
                run_id,
                interaction_id,
                response,
            },
        )
        .await
    }

    async fn begin_launch(&self) -> Result<(), TuiRunClientError> {
        self.state.lock().await.begin_launch()
    }

    async fn active_run_id(&self) -> Result<RunId, TuiRunClientError> {
        self.state
            .lock()
            .await
            .current_active_root
            .clone()
            .ok_or(TuiRunClientError::NoActiveRun)
    }

    async fn execute_launch(
        &self,
        operation: &'static str,
        command: RunCommand,
    ) -> Result<RunView, TuiRunClientError> {
        let run = match self.execute_run_command(operation, command).await {
            Ok(run) => run,
            Err(error) => {
                self.state.lock().await.finish_launch();
                return Err(error);
            }
        };
        self.adopt_launched_run(run).await
    }

    async fn execute_run_command(
        &self,
        operation: &'static str,
        command: RunCommand,
    ) -> Result<RunView, TuiRunClientError> {
        // One request id is allocated for this canonical command. The envelope
        // is built once so any same-execution retry can reuse it unchanged.
        let envelope = self.envelope(operation, command);
        let response = self.application.execute(envelope).await;
        match response.result {
            RunCommandResult::Run { run } => Ok(*run),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult {
                operation,
                result: Box::new(result),
            }),
        }
    }

    async fn adopt_launched_run(&self, run: RunView) -> Result<RunView, TuiRunClientError> {
        let monitor = {
            let mut state = self.state.lock().await;
            state.finish_launch();
            state.observe_run(&run)?
        };
        if monitor {
            self.ensure_monitor(run.run_id.clone()).await;
        }
        Ok(run)
    }

    async fn execute_control(
        &self,
        operation: &'static str,
        command: RunCommand,
    ) -> Result<u64, TuiRunClientError> {
        let envelope = self.envelope(operation, command);
        let response = self.application.execute(envelope).await;
        match response.result {
            RunCommandResult::Accepted { last_sequence, .. } => Ok(last_sequence),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult {
                operation,
                result: Box::new(result),
            }),
        }
    }

    fn envelope(&self, operation: &str, command: RunCommand) -> RunCommandEnvelope {
        RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: self.request_ids.next(operation),
            command,
        }
    }

    async fn ensure_monitor(&self, run_id: RunId) {
        if !self.state.lock().await.begin_monitor(&run_id) {
            return;
        }
        let application = self.application.clone();
        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            monitor_run(application, state, event_tx, run_id).await;
        });
    }
}

async fn monitor_run(
    application: Arc<AgentApplication>,
    state: Arc<Mutex<TuiRunClientState>>,
    event_tx: mpsc::Sender<StoredRuntimeEvent>,
    run_id: RunId,
) {
    loop {
        let cursor = {
            let state = state.lock().await;
            state.cursors.get(&run_id).copied().unwrap_or(0)
        };
        let result = application.wait_events(&run_id, cursor).await;
        let events = match result {
            RunCommandResult::Events {
                run_id: response_run_id,
                events,
                ..
            } if response_run_id == run_id => events,
            RunCommandResult::Error { error } => {
                tracing::warn!(
                    run_id = %run_id,
                    code = ?error.code,
                    message = %error.message,
                    "canonical TUI run monitor stopped"
                );
                break;
            }
            other => {
                tracing::warn!(
                    run_id = %run_id,
                    result = ?other,
                    "canonical TUI run monitor received an unexpected result"
                );
                break;
            }
        };
        if events.is_empty() {
            break;
        }
        let mut terminal = false;
        for event in events {
            terminal |= event.event.is_terminal();
            if record_and_send(&state, &event_tx, event).await.is_err() {
                terminal = true;
                break;
            }
        }
        if terminal {
            break;
        }
    }
    state.lock().await.finish_monitor(&run_id);
}

async fn record_and_send(
    state: &Mutex<TuiRunClientState>,
    event_tx: &mpsc::Sender<StoredRuntimeEvent>,
    event: StoredRuntimeEvent,
) -> Result<(), mpsc::error::SendError<StoredRuntimeEvent>> {
    state.lock().await.record_event(&event);
    event_tx.send(event).await
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::path::Path;
    use std::time::Duration;

    use dse_app::{
        DeepSeekConnectionConfig, DeepSeekEndpoint, ProductionApplicationConfig,
        ProductionPromptConfig, ProductionToolConfig, ShellPolicy,
    };
    use dse_protocol::agent_runtime::{
        AgentOutcome, CommandId, ModelAccounting, ReasoningEffort, RunLimits, RuntimeEventId,
        RuntimeEventKind, TerminalState, ToolPolicy,
    };
    use dse_protocol::run_api::{PendingCreationKind, RunProductControls};
    use dse_protocol::task::TaskDefinition;
    use dse_runtime::{CreationIntent, RunStore};
    use dse_state::StateStore;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use wiremock::MockServer;

    use super::*;

    fn start_command(input: &str) -> StartRunCommand {
        StartRunCommand {
            task: TaskDefinition::host(input),
            workspace: "/workspace/project".to_owned(),
            model: Some("deepseek-v4-flash".to_owned()),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(4_096),
            max_api_requests: NonZeroU32::new(8),
            streaming: true,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            controls: RunProductControls::default(),
        }
    }

    fn run_view(run_id: &str, terminal: Option<TerminalState>, last_sequence: u64) -> RunView {
        RunView {
            run_id: RunId::from(run_id),
            parent_run_id: None,
            continued_from_run_id: None,
            model: "deepseek-v4-flash".to_owned(),
            task_contract: None,
            workspace: "/workspace/project".to_owned(),
            last_sequence,
            terminal,
            usage: Default::default(),
            accounting: Default::default(),
            runtime_model_requests: 0,
            runtime_retries: 0,
            tool_calls: 0,
            local_turns: 0,
        }
    }

    fn terminal_event(run_id: &RunId, sequence: u64) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: dse_protocol::agent_runtime::AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: run_id.clone(),
            parent_run_id: None,
            event_id: RuntimeEventId::terminal(),
            sequence,
            occurred_at_unix_ms: 1,
            event: RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: run_id.clone(),
                    parent_run_id: None,
                    terminal: TerminalState::Blocked {
                        reason: "fixture terminal".to_owned(),
                    },
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 0,
                    runtime_retries: 0,
                    tool_calls: 0,
                    details: Default::default(),
                }),
            },
        }
    }

    struct RecoveryFixture {
        _root: TempDir,
        workspace: String,
        store: StateStore,
        application: Arc<AgentApplication>,
        model: MockServer,
    }

    impl RecoveryFixture {
        async fn new() -> Self {
            let model = MockServer::start().await;
            let root = TempDir::new().expect("isolated recovery fixture");
            let workspace_path = root.path().join("workspace");
            let skills_dir = root.path().join("skills");
            std::fs::create_dir_all(&workspace_path).expect("create fixture workspace");
            std::fs::create_dir_all(&skills_dir).expect("create fixture skills directory");
            let workspace = std::fs::canonicalize(&workspace_path)
                .expect("canonical fixture workspace")
                .display()
                .to_string();
            let state_path = root.path().join("state.db");
            let store = StateStore::open(Some(state_path.clone())).expect("open fixture RunStore");
            let application =
                fixture_application(&state_path, &model.uri(), &workspace_path, &skills_dir);
            Self {
                _root: root,
                workspace,
                store,
                application,
                model,
            }
        }

        async fn reserve(
            &self,
            creation_request_id: &str,
            reserved_run_id: &str,
            command: RunCommand,
        ) {
            let (kind, workspace, source_run_id) = match &command {
                RunCommand::Start(command) => {
                    (PendingCreationKind::Start, command.workspace.clone(), None)
                }
                RunCommand::Continue(command) => (
                    PendingCreationKind::Continue,
                    command
                        .expected_workspace
                        .clone()
                        .expect("fixture continuation workspace"),
                    Some(command.run_id.clone()),
                ),
                other => panic!("fixture only reserves creation commands: {other:?}"),
            };
            self.store
                .reserve_creation(
                    &CommandId::from(creation_request_id),
                    &creation_digest(&command),
                    RunId::from(reserved_run_id),
                    CreationIntent {
                        kind,
                        workspace,
                        source_run_id,
                        command,
                    },
                )
                .await
                .expect("reserve fixture creation");
        }

        async fn received_model_requests(&self) -> usize {
            self.model
                .received_requests()
                .await
                .expect("loopback request journal")
                .len()
        }
    }

    fn fixture_application(
        state_path: &Path,
        base_url: &str,
        workspace: &Path,
        skills_dir: &Path,
    ) -> Arc<AgentApplication> {
        let connection = DeepSeekConnectionConfig {
            endpoint: DeepSeekEndpoint::loopback_fixture(format!("{base_url}/v1"))
                .expect("loopback DeepSeek endpoint"),
            strict_tools: false,
            response_header_timeout: Duration::from_secs(2),
            stream_idle_timeout: Duration::from_secs(2),
        };
        let config = ProductionApplicationConfig::official()
            .with_state_db_path(state_path)
            .with_deepseek_connection(connection)
            .with_tool_config(
                ProductionToolConfig::new(workspace).with_shell_policy(ShellPolicy::None),
            )
            .with_prompt(ProductionPromptConfig {
                skills_dir: Some(skills_dir.to_path_buf()),
                ..ProductionPromptConfig::default()
            })
            .with_default_max_api_requests(NonZeroU32::new(2).expect("non-zero request limit"))
            .with_api_key("offline-creation-recovery-key")
            .expect("fixture credential");
        Arc::new(AgentApplication::production(config).expect("fixture AgentApplication"))
    }

    fn creation_digest(command: &RunCommand) -> String {
        let bytes = serde_json::to_vec(command).expect("serialize canonical creation command");
        let digest = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{digest}")
    }

    #[test]
    fn initial_submit_plans_an_interactive_start() {
        let state = TuiRunClientState::default();
        let RunCommand::Start(command) = state
            .plan_submit(start_command("第一轮"))
            .expect("initial submit")
        else {
            panic!("initial submit must start a root")
        };
        assert!(command.controls.interactive);
        assert_eq!(command.task.objective, "第一轮");
    }

    #[test]
    fn terminal_follow_up_plans_a_continuation() {
        let state = TuiRunClientState {
            latest_terminal_root: Some(RunId::from("terminal-root")),
            ..TuiRunClientState::default()
        };
        let RunCommand::Continue(command) = state
            .plan_submit(start_command("继续任务"))
            .expect("follow-up submit")
        else {
            panic!("terminal follow-up must continue the terminal root")
        };
        assert_eq!(command.run_id, RunId::from("terminal-root"));
        assert_eq!(command.task.objective, "继续任务");
        assert_eq!(
            command.expected_workspace.as_deref(),
            Some("/workspace/project")
        );
    }

    #[test]
    fn active_root_rejects_a_second_submit() {
        let state = TuiRunClientState {
            current_active_root: Some(RunId::from("active-root")),
            ..TuiRunClientState::default()
        };
        assert!(matches!(
            state.plan_submit(start_command("并发提交")),
            Err(TuiRunClientError::ActiveRun { run_id })
                if run_id == RunId::from("active-root")
        ));
    }

    #[test]
    fn attach_decision_resumes_only_non_terminal_runs() {
        let active = run_view("active", None, 2);
        let terminal = run_view(
            "terminal",
            Some(TerminalState::Blocked {
                reason: "fixture terminal".to_owned(),
            }),
            7,
        );
        assert!(run_requires_resume(&active));
        assert!(!run_requires_resume(&terminal));
    }

    #[test]
    fn recovery_required_is_never_selected_as_a_continuation_source() {
        let mut state = TuiRunClientState {
            latest_terminal_root: Some(RunId::from("older-terminal")),
            ..TuiRunClientState::default()
        };
        let recovery = run_view(
            "recovery-root",
            Some(TerminalState::RecoveryRequired {
                ambiguity: dse_protocol::agent_runtime::RecoveryAmbiguity {
                    phase: dse_protocol::agent_runtime::RecoveryAmbiguityPhase::ModelRequest,
                    action_id: "request-1".to_owned(),
                    message: "fixture ambiguity".to_owned(),
                },
            }),
            7,
        );

        assert!(state.observe_run(&recovery).expect("observe recovery root"));
        assert_eq!(state.latest_terminal_root, None);
        assert!(matches!(
            state.plan_submit(start_command("start independently")),
            Ok(RunCommand::Start(_))
        ));
    }

    #[test]
    fn run_observation_and_terminal_event_advance_only_canonical_state() {
        let mut state = TuiRunClientState::default();
        let active = run_view("run-1", None, 1);
        assert!(state.observe_run(&active).expect("observe active run"));
        assert_eq!(state.current_active_root, Some(RunId::from("run-1")));
        assert!(state.begin_monitor(&active.run_id));
        assert!(!state.begin_monitor(&active.run_id));

        let event = terminal_event(&active.run_id, 7);
        state.record_event(&event);
        assert_eq!(state.cursors.get(&active.run_id), Some(&7));
        assert_eq!(state.current_active_root, None);
        assert_eq!(state.latest_terminal_root, Some(active.run_id));
    }

    #[tokio::test]
    async fn terminal_state_is_recorded_before_the_event_reaches_the_ui() {
        let run_id = RunId::from("run-terminal-order");
        let state = Mutex::new(TuiRunClientState {
            current_active_root: Some(run_id.clone()),
            ..TuiRunClientState::default()
        });
        let (event_tx, mut event_rx) = mpsc::channel(1);

        record_and_send(&state, &event_tx, terminal_event(&run_id, 9))
            .await
            .expect("UI receiver remains connected");
        let received = event_rx.recv().await.expect("terminal event");
        let snapshot = state.lock().await.snapshot();

        assert_eq!(received.run_id, run_id);
        assert_eq!(snapshot.current_active_root, None);
        assert_eq!(snapshot.latest_terminal_root, Some(run_id.clone()));
        assert_eq!(snapshot.cursors.get(&run_id), Some(&9));
    }

    #[tokio::test]
    async fn prepare_new_root_clears_only_the_ephemeral_continuation_choice() {
        let fixture = RecoveryFixture::new().await;
        let (client, _events) = TuiRunClient::new(fixture.application);
        client.state.lock().await.latest_terminal_root = Some(RunId::from("terminal-root"));

        client
            .prepare_new_root()
            .await
            .expect("idle client may choose a fresh root");

        let snapshot = client.snapshot().await;
        assert_eq!(snapshot.latest_terminal_root, None);
        assert!(matches!(
            client
                .state
                .lock()
                .await
                .plan_submit(start_command("new root")),
            Ok(RunCommand::Start(_))
        ));
    }

    #[test]
    fn request_ids_are_unique_per_intent_and_reused_by_envelope_clone() {
        let ids = RequestIds::new();
        let first = RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: ids.next("start"),
            command: RunCommand::Start(start_command("第一轮")),
        };
        let retry = first.clone();
        let second = RunCommandEnvelope {
            schema_version: RUN_API_SCHEMA_VERSION,
            request_id: ids.next("start"),
            command: RunCommand::Start(start_command("第二轮")),
        };
        assert_eq!(first.request_id, retry.request_id);
        assert_ne!(first.request_id, second.request_id);
    }

    #[tokio::test]
    async fn no_pending_creation_returns_none_without_starting_a_run() {
        let fixture = RecoveryFixture::new().await;
        let (client, _events) = TuiRunClient::new(fixture.application.clone());

        assert!(
            client
                .recover_pending_for_workspace(fixture.workspace.clone())
                .await
                .expect("empty canonical pending list")
                .is_none()
        );
        assert_eq!(fixture.received_model_requests().await, 0);
    }

    #[tokio::test]
    async fn sole_explicit_pending_creation_recovers_and_adopts_reserved_run() {
        let fixture = RecoveryFixture::new().await;
        let creation_request_id = "tui-recover-explicit";
        let reserved_run_id = "tui-reserved-explicit";
        let mut command = start_command("恢复未发布的显式模型任务");
        command.workspace = fixture.workspace.clone();
        fixture
            .reserve(
                creation_request_id,
                reserved_run_id,
                RunCommand::Start(command),
            )
            .await;
        let (client, _events) = TuiRunClient::new(fixture.application.clone());

        let recovered = client
            .recover_pending_for_workspace(fixture.workspace.clone())
            .await
            .expect("recover sole explicit creation")
            .expect("one pending creation");

        assert_eq!(recovered.run_id, RunId::from(reserved_run_id));
        assert!(
            fixture
                .store
                .load(&recovered.run_id)
                .await
                .expect("load recovered run")
                .is_some(),
            "recovery must create the exact reserved canonical Run"
        );
        assert!(
            client
                .list_pending_creations(fixture.workspace, 10)
                .await
                .expect("list consumed creations")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn fixed_actor_pending_creation_recovers_once_through_the_host_policy() {
        let fixture = RecoveryFixture::new().await;
        let creation_request_id = "tui-recover-host-policy";
        let reserved_run_id = "tui-reserved-host-policy";
        let mut command = start_command("不得重复固定 actor 路由");
        command.workspace = fixture.workspace.clone();
        command.model = None;
        fixture
            .reserve(
                creation_request_id,
                reserved_run_id,
                RunCommand::Start(command),
            )
            .await;
        let (client, _events) = TuiRunClient::new(fixture.application.clone());

        let recovered = client
            .recover_pending_for_workspace(fixture.workspace.clone())
            .await
            .expect("deterministic Host routing is safe to recover")
            .expect("one pending creation");
        assert_eq!(recovered.run_id, RunId::from(reserved_run_id));
        let replay = fixture
            .store
            .load(&recovered.run_id)
            .await
            .expect("load recovered fixed route run")
            .expect("recovered fixed route run exists");
        assert_eq!(replay.snapshot.request.model, "deepseek-v4-pro");
        assert_eq!(
            replay.snapshot.request.route.profile,
            dse_runtime::ModelRouteProfile::FixedActor
        );
        assert_eq!(
            replay.snapshot.request.route.reason_code,
            "fixed_root_responsible"
        );
        assert!(
            client
                .list_pending_creations(fixture.workspace, 10)
                .await
                .expect("list consumed fixed route creation")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn multiple_pending_creations_fail_closed_without_starting_any_run() {
        let fixture = RecoveryFixture::new().await;
        for (request_id, run_id, input) in [
            ("tui-pending-a", "tui-reserved-a", "待恢复任务甲"),
            ("tui-pending-b", "tui-reserved-b", "待恢复任务乙"),
        ] {
            let mut command = start_command(input);
            command.workspace = fixture.workspace.clone();
            fixture
                .reserve(request_id, run_id, RunCommand::Start(command))
                .await;
        }
        let (client, _events) = TuiRunClient::new(fixture.application.clone());

        let error = client
            .recover_pending_for_workspace(fixture.workspace.clone())
            .await
            .expect_err("multiple creations require an explicit choice");
        let TuiRunClientError::AmbiguousPendingCreations {
            workspace,
            creation_request_ids,
        } = error
        else {
            panic!("expected explicit ambiguity, got {error:?}")
        };
        assert_eq!(workspace, fixture.workspace);
        assert_eq!(creation_request_ids.len(), 2);
        assert!(
            creation_request_ids
                .iter()
                .any(|request_id| request_id == "tui-pending-a")
        );
        assert!(
            creation_request_ids
                .iter()
                .any(|request_id| request_id == "tui-pending-b")
        );
        assert!(
            fixture
                .store
                .load(&RunId::from("tui-reserved-a"))
                .await
                .expect("load first reserved run")
                .is_none()
        );
        assert!(
            fixture
                .store
                .load(&RunId::from("tui-reserved-b"))
                .await
                .expect("load second reserved run")
                .is_none()
        );
        assert_eq!(fixture.received_model_requests().await, 0);
    }
}
