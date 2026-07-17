//! Concrete interactive-TUI client for the canonical Run API.
//!
//! This module owns no model loop, event translation, or durable state. It
//! submits commands directly to [`AgentApplication`] and forwards committed
//! [`StoredRuntimeEvent`] values unchanged to the presentation layer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use codewhale_app::AgentApplication;
use codewhale_protocol::agent_runtime::{
    InteractionId, RunId, RunPurpose, StoredRuntimeEvent, UserInteractionResponse,
};
use codewhale_protocol::run_api::{
    CompactRunCommand, ContinueRunCommand, MAX_RUN_LIST_LIMIT, RUN_API_SCHEMA_VERSION,
    RootRunSummary, RunApiError, RunCommand, RunCommandEnvelope, RunCommandResult, RunView,
    StartRunCommand,
};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

const RUN_EVENT_BUFFER_CAPACITY: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum TuiRunClientError {
    #[error("已有活动中的根运行：{run_id}")]
    ActiveRun { run_id: RunId },
    #[error("另一个运行启动、恢复或附加操作仍在进行")]
    LaunchInFlight,
    #[error("当前没有可控制的活动根运行")]
    NoActiveRun,
    #[error("Run API 返回错误：{0:?}")]
    Application(RunApiError),
    #[error("Run API 对 {operation} 返回了意外结果：{result:?}")]
    UnexpectedResult {
        operation: &'static str,
        result: RunCommandResult,
    },
}

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
                input: fresh.input,
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
        if run.terminal.is_some() {
            if self.current_active_root.as_ref() == Some(&run.run_id) {
                self.current_active_root = None;
            }
            self.latest_terminal_root = Some(run.run_id.clone());
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
        if event.event.is_terminal() {
            if self.current_active_root.as_ref() == Some(&event.run_id) {
                self.current_active_root = None;
            }
            self.latest_terminal_root = Some(event.run_id.clone());
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

fn compact_command(run_id: RunId, expected_workspace: Option<String>) -> RunCommand {
    RunCommand::Compact(CompactRunCommand {
        run_id,
        expected_workspace,
    })
}

fn agent_roots(runs: Vec<RootRunSummary>, limit: u32) -> Vec<RootRunSummary> {
    runs.into_iter()
        .filter(|run| run.purpose == RunPurpose::Agent)
        .take(limit as usize)
        .collect()
}

fn run_requires_resume(run: &RunView) -> bool {
    run.terminal.is_none()
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

    /// Resume execution of one durable non-terminal run.
    pub async fn resume(
        &self,
        run_id: RunId,
        expected_workspace: Option<String>,
    ) -> Result<RunView, TuiRunClientError> {
        self.begin_launch().await?;
        self.execute_launch(
            "resume",
            RunCommand::Resume {
                run_id,
                expected_workspace,
            },
        )
        .await
    }

    /// Compact one terminal root into a new canonical compaction root.
    pub async fn compact(
        &self,
        run_id: RunId,
        expected_workspace: Option<String>,
    ) -> Result<RunView, TuiRunClientError> {
        self.begin_launch().await?;
        self.execute_launch("compact", compact_command(run_id, expected_workspace))
            .await
    }

    /// Return only user-facing Agent roots for the workspace picker.
    ///
    /// The canonical Store also lists internal context-compaction roots. Those
    /// remain valid continuation sources but are not independent sessions.
    pub async fn list_roots(
        &self,
        workspace: String,
        limit: u32,
    ) -> Result<Vec<RootRunSummary>, TuiRunClientError> {
        let operation = "list-roots";
        // Fetch the widest valid canonical window before filtering. Otherwise
        // a recent internal compaction root can hide the latest Agent root
        // when a picker asks for `limit = 1`.
        let canonical_limit = if (1..=MAX_RUN_LIST_LIMIT).contains(&limit) {
            MAX_RUN_LIST_LIMIT
        } else {
            limit
        };
        let response = self
            .application
            .execute(self.envelope(
                operation,
                RunCommand::ListRoots {
                    workspace,
                    limit: canonical_limit,
                },
            ))
            .await;
        match response.result {
            RunCommandResult::Runs { runs, .. } => Ok(agent_roots(runs, limit)),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult { operation, result }),
        }
    }

    /// Return the newest canonical root, including an internal compaction
    /// head. A compaction root is hidden from a user-facing picker, but it is
    /// still the correct source for resume/continue because it owns the newest
    /// canonical transcript.
    pub async fn latest_root(
        &self,
        workspace: String,
    ) -> Result<Option<RootRunSummary>, TuiRunClientError> {
        let operation = "latest-root";
        let response = self
            .application
            .execute(self.envelope(
                operation,
                RunCommand::ListRoots {
                    workspace,
                    limit: 1,
                },
            ))
            .await;
        match response.result {
            RunCommandResult::Runs { runs, .. } => Ok(runs.into_iter().next()),
            RunCommandResult::Error { error } => Err(TuiRunClientError::Application(error)),
            result => Err(TuiRunClientError::UnexpectedResult { operation, result }),
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
            result => Err(TuiRunClientError::UnexpectedResult { operation, result }),
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
            result => Err(TuiRunClientError::UnexpectedResult { operation, result }),
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

    use codewhale_protocol::agent_runtime::{
        AgentOutcome, ModelAccounting, ReasoningEffort, RunLimits, RuntimeEventId,
        RuntimeEventKind, TerminalState, ToolPolicy,
    };
    use codewhale_protocol::run_api::RunProductControls;

    use super::*;

    fn start_command(input: &str) -> StartRunCommand {
        StartRunCommand {
            input: input.to_owned(),
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
            purpose: Default::default(),
            parent_run_id: None,
            continued_from_run_id: None,
            model: "deepseek-v4-flash".to_owned(),
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

    fn root_summary(run_id: &str, purpose: RunPurpose, updated_at_unix_ms: u64) -> RootRunSummary {
        RootRunSummary {
            run_id: RunId::from(run_id),
            purpose,
            continued_from_run_id: None,
            workspace: "/workspace/project".to_owned(),
            last_sequence: 1,
            terminal: true,
            created_at_unix_ms: 1,
            updated_at_unix_ms,
        }
    }

    fn terminal_event(run_id: &RunId, sequence: u64) -> StoredRuntimeEvent {
        StoredRuntimeEvent {
            schema_version: codewhale_protocol::agent_runtime::AGENT_RUNTIME_EVENT_SCHEMA_VERSION,
            run_id: run_id.clone(),
            parent_run_id: None,
            event_id: RuntimeEventId::terminal(),
            sequence,
            occurred_at_unix_ms: 1,
            event: RuntimeEventKind::Terminal {
                outcome: Box::new(AgentOutcome {
                    run_id: run_id.clone(),
                    parent_run_id: None,
                    terminal: TerminalState::Completed {
                        message: "完成".to_owned(),
                    },
                    accounting: ModelAccounting::default(),
                    runtime_model_requests: 0,
                    runtime_retries: 0,
                    tool_calls: 0,
                }),
            },
        }
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
        assert_eq!(command.input, "第一轮");
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
        assert_eq!(command.input, "继续任务");
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
    fn compact_planning_preserves_source_and_workspace_guard() {
        let RunCommand::Compact(command) = compact_command(
            RunId::from("terminal-root"),
            Some("/workspace/project".to_owned()),
        ) else {
            panic!("compact helper must plan canonical Compact")
        };
        assert_eq!(command.run_id, RunId::from("terminal-root"));
        assert_eq!(
            command.expected_workspace.as_deref(),
            Some("/workspace/project")
        );
    }

    #[test]
    fn root_picker_filters_internal_compaction_roots() {
        let roots = agent_roots(
            vec![
                root_summary("compact", RunPurpose::ContextCompaction, 4),
                root_summary("agent-new", RunPurpose::Agent, 3),
                root_summary("compact-old", RunPurpose::ContextCompaction, 2),
                root_summary("agent-old", RunPurpose::Agent, 1),
            ],
            1,
        );
        assert_eq!(
            roots
                .iter()
                .map(|run| run.run_id.clone())
                .collect::<Vec<_>>(),
            vec![RunId::from("agent-new")]
        );
    }

    #[test]
    fn attach_decision_resumes_only_non_terminal_runs() {
        let active = run_view("active", None, 2);
        let terminal = run_view(
            "terminal",
            Some(TerminalState::Completed {
                message: "完成".to_owned(),
            }),
            7,
        );
        assert!(run_requires_resume(&active));
        assert!(!run_requires_resume(&terminal));
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
}
