use super::*;
use crate::core::engine::{MockApprovalEvent, mock_engine_handle};
use crate::core::events::{Event as EngineEvent, TurnOutcomeStatus};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use uuid::Uuid;

fn test_runtime_dir() -> PathBuf {
    std::env::temp_dir().join(format!("deepseek-runtime-threads-{}", Uuid::new_v4()))
}

fn test_manager_config(data_dir: PathBuf) -> RuntimeThreadManagerConfig {
    RuntimeThreadManagerConfig {
        task_data_dir: data_dir.clone(),
        data_dir,
        max_active_threads: 4,
    }
}

fn test_manager(data_dir: PathBuf) -> Result<RuntimeThreadManager> {
    RuntimeThreadManager::open(
        Config::default(),
        PathBuf::from("."),
        test_manager_config(data_dir),
    )
}

fn sample_thread(thread_id: &str) -> ThreadRecord {
    let now = Utc::now();
    ThreadRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: thread_id.to_string(),
        created_at: now,
        updated_at: now,
        model: DEFAULT_TEXT_MODEL.to_string(),
        workspace: PathBuf::from("."),
        mode: AppMode::Agent.as_setting().to_string(),
        allow_shell: false,
        trust_mode: false,
        auto_approve: false,
        latest_turn_id: None,
        latest_response_bookmark: None,
        archived: false,
        system_prompt: None,
        task_id: None,
        title: None,
        session_id: None,
    }
}

fn sample_turn(thread_id: &str, turn_id: &str, status: RuntimeTurnStatus) -> TurnRecord {
    let now = Utc::now();
    TurnRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: turn_id.to_string(),
        thread_id: thread_id.to_string(),
        status,
        input_summary: "sample".to_string(),
        created_at: now,
        started_at: Some(now),
        ended_at: None,
        duration_ms: None,
        usage: None,
        effective_provider: None,
        effective_billing_surface: None,
        effective_model: None,
        error: None,
        item_ids: Vec::new(),
        steer_count: 0,
    }
}

#[test]
fn runtime_compaction_uses_provider_route_context() {
    let limits = codewhale_config::route::RouteLimits {
        context_tokens: Some(272_000),
        input_tokens: None,
        output_tokens: None,
    };
    let config = runtime_compaction_config(
        ApiProvider::OpenaiCodex,
        "gpt-5.5",
        Some(limits),
        false,
        false,
        80.0,
    );

    assert!(config.enabled);
    assert_eq!(config.token_threshold, 217_600);
    assert_eq!(config.effective_context_window, Some(272_000));
}

#[test]
fn legacy_turn_record_has_no_invented_route_provenance() {
    let turn = sample_turn("thr_legacy", "turn_legacy", RuntimeTurnStatus::Completed);
    let mut value = serde_json::to_value(turn).expect("serialize turn");
    let object = value.as_object_mut().expect("turn object");
    object.remove("effective_provider");
    object.remove("effective_billing_surface");
    object.remove("effective_model");

    let restored: TurnRecord = serde_json::from_value(value).expect("deserialize legacy turn");
    assert_eq!(restored.effective_provider, None);
    assert_eq!(restored.effective_billing_surface, None);
    assert_eq!(restored.effective_model, None);
}

#[test]
fn turn_record_persists_billing_surface_without_raw_endpoint() {
    let mut turn = sample_turn("thr_surface", "turn_surface", RuntimeTurnStatus::Completed);
    turn.effective_provider = Some(ApiProvider::Stepfun.as_str().to_string());
    turn.effective_billing_surface = Some(crate::pricing::STEPFUN_PAYG_BILLING_SURFACE.to_string());
    turn.effective_model = Some("step-3.7-flash".to_string());

    let value = serde_json::to_value(turn).expect("serialize turn");
    assert_eq!(
        value["effective_billing_surface"],
        crate::pricing::STEPFUN_PAYG_BILLING_SURFACE
    );
    assert!(value.get("base_url").is_none());
    assert!(value.get("effective_base_url").is_none());
}

fn sample_item(turn_id: &str, item_id: &str, status: TurnItemLifecycleStatus) -> TurnItemRecord {
    TurnItemRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: item_id.to_string(),
        turn_id: turn_id.to_string(),
        kind: TurnItemKind::Status,
        status,
        summary: "sample item".to_string(),
        detail: None,
        metadata: None,
        artifact_refs: Vec::new(),
        started_at: Some(Utc::now()),
        ended_at: None,
    }
}

async fn install_mock_engine(
    manager: &RuntimeThreadManager,
    thread_id: &str,
) -> crate::core::engine::MockEngineHandle {
    let harness = mock_engine_handle();
    let mut active = manager.active.lock().await;
    active.engines.insert(
        thread_id.to_string(),
        ActiveThreadState {
            engine: harness.handle.clone(),
            active_turn: None,
            route_provider: ApiProvider::Deepseek,
            route_model: DEFAULT_TEXT_MODEL.to_string(),
        },
    );
    touch_lru(&mut active.lru, thread_id);
    harness
}

async fn wait_for_terminal_turn(
    manager: &RuntimeThreadManager,
    turn_id: &str,
    timeout: Duration,
) -> Result<TurnRecord> {
    let deadline = Instant::now() + timeout;
    loop {
        let turn = manager.store.load_turn(turn_id)?;
        if matches!(
            turn.status,
            RuntimeTurnStatus::Completed
                | RuntimeTurnStatus::Failed
                | RuntimeTurnStatus::Interrupted
                | RuntimeTurnStatus::Canceled
        ) {
            return Ok(turn);
        }
        if Instant::now() >= deadline {
            bail!("Timed out waiting for turn {turn_id}");
        }
        sleep(Duration::from_millis(20)).await;
    }
}

#[test]
fn store_load_thread_rejects_newer_schema_version() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");

    // Construct a thread record persisted with a future schema version.
    let mut thread = sample_thread("thr_future");
    thread.schema_version = CURRENT_RUNTIME_SCHEMA_VERSION + 1;

    // Bypass save_thread (which would respect our local schema_version)
    // by writing the JSON directly so we can simulate a future writer.
    let path = store.threads_dir.join(format!("{}.json", thread.id));
    std::fs::create_dir_all(path.parent().unwrap()).expect("mkdirs");
    let payload = serde_json::to_string(&thread).expect("serialize thread");
    std::fs::write(&path, payload).expect("write thread");

    let err = store
        .load_thread(&thread.id)
        .expect_err("load_thread must reject newer schema");
    let msg = format!("{err:#}");
    assert!(msg.contains("newer than supported"), "got: {msg}");

    // Cleanup so we don't leak across tests.
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn store_open_rejects_symlinked_state_file() {
    let dir = test_runtime_dir();
    std::fs::create_dir_all(&dir).expect("mkdir runtime dir");
    let target = dir.join("outside-state.json");
    let link = dir.join("state.json");
    std::fs::write(
        &target,
        serde_json::to_string(&RuntimeStoreState::default()).unwrap(),
    )
    .expect("write target");
    std::os::unix::fs::symlink(&target, &link).expect("symlink state");

    let err = RuntimeThreadStore::open(dir.clone()).expect_err("symlink state should fail");
    assert!(format!("{err:#}").contains("must not be a symlink"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_open_rejects_root_traversal() {
    let dir = test_runtime_dir();
    let bad_root = dir.join("runtime").join("..").join("outside");

    let err = RuntimeThreadStore::open(bad_root).expect_err("traversal root should fail");
    assert!(format!("{err:#}").contains("cannot contain '..'"));

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn store_open_rejects_symlinked_store_directory() {
    let dir = test_runtime_dir();
    std::fs::create_dir_all(&dir).expect("mkdir runtime dir");
    let outside = dir.join("outside-items");
    let link = dir.join("items");
    std::fs::create_dir_all(&outside).expect("mkdir outside");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink items dir");

    let err = RuntimeThreadStore::open(dir.clone()).expect_err("symlink items dir should fail");
    assert!(
        format!("{err:#}").contains("directory must not be a symlink"),
        "got: {err:#}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn store_list_items_rejects_symlinked_item_file() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");
    let item = sample_item("turn_link", "item_link", TurnItemLifecycleStatus::Completed);
    let target = dir.join("outside-item.json");
    let link = store.items_dir.join(format!("{}.json", item.id));
    std::fs::write(&target, serde_json::to_string(&item).unwrap()).expect("write target");
    std::os::unix::fs::symlink(&target, &link).expect("symlink item");

    let err = store
        .list_items_for_turn(&item.turn_id)
        .expect_err("symlink item should fail");
    assert!(format!("{err:#}").contains("must not be a symlink"));

    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn store_list_items_rejects_swapped_symlinked_store_directory() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");
    let outside = dir.join("outside-items");
    std::fs::create_dir_all(&outside).expect("mkdir outside");
    std::fs::remove_dir_all(&store.items_dir).expect("remove items dir");
    std::os::unix::fs::symlink(&outside, &store.items_dir).expect("symlink items dir");

    let err = store
        .list_items_for_turn("turn_link")
        .expect_err("swapped symlink items dir should fail");
    assert!(
        format!("{err:#}").contains("directory must not be a symlink"),
        "got: {err:#}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_load_thread_defaults_missing_session_id() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");
    let thread = sample_thread("thr_legacy_session");
    let path = store.threads_dir.join(format!("{}.json", thread.id));
    std::fs::create_dir_all(path.parent().unwrap()).expect("mkdirs");
    let mut payload = serde_json::to_value(&thread).expect("serialize thread");
    payload
        .as_object_mut()
        .expect("thread object")
        .remove("session_id");
    std::fs::write(
        &path,
        serde_json::to_string(&payload).expect("encode thread"),
    )
    .expect("write thread");

    let loaded = store
        .load_thread(&thread.id)
        .expect("legacy thread should load");
    assert_eq!(loaded.session_id, None);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn current_runtime_schema_version_is_two_on_v066() {
    // Locks the bump in (issue #124). Bump deliberately when persisted
    // shape changes.
    assert_eq!(CURRENT_RUNTIME_SCHEMA_VERSION, 2);
}

#[test]
fn store_rejects_path_like_record_ids() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");

    let err = store
        .load_thread("../outside")
        .expect_err("path traversal id should fail");
    assert!(
        format!("{err:#}").contains("unsupported characters"),
        "got: {err:#}"
    );

    let mut thread = sample_thread("thr_bad/id");
    let err = store
        .save_thread(&thread)
        .expect_err("path separator id should fail");
    assert!(
        format!("{err:#}").contains("unsupported characters"),
        "got: {err:#}"
    );

    thread.id = " thr_bad".to_string();
    let err = store
        .save_thread(&thread)
        .expect_err("whitespace id should fail");
    assert!(format!("{err:#}").contains("whitespace"), "got: {err:#}");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_load_turn_rejects_newer_schema_version() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");

    let mut turn = sample_turn("thr_t", "trn_future", RuntimeTurnStatus::InProgress);
    turn.schema_version = CURRENT_RUNTIME_SCHEMA_VERSION + 1;

    let path = store.turns_dir.join(format!("{}.json", turn.id));
    std::fs::create_dir_all(path.parent().unwrap()).expect("mkdirs");
    std::fs::write(&path, serde_json::to_string(&turn).expect("serialize turn"))
        .expect("write turn");

    let err = store
        .load_turn(&turn.id)
        .expect_err("load_turn must reject newer schema");
    assert!(
        format!("{err:#}").contains("newer than supported"),
        "got: {err:#}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_load_item_rejects_newer_schema_version() {
    let dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(dir.clone()).expect("open store");

    let mut item = sample_item("trn_t", "itm_future", TurnItemLifecycleStatus::InProgress);
    item.schema_version = CURRENT_RUNTIME_SCHEMA_VERSION + 1;

    let path = store.items_dir.join(format!("{}.json", item.id));
    std::fs::create_dir_all(path.parent().unwrap()).expect("mkdirs");
    std::fs::write(&path, serde_json::to_string(&item).expect("serialize item"))
        .expect("write item");

    let err = store
        .load_item(&item.id)
        .expect_err("load_item must reject newer schema");
    assert!(
        format!("{err:#}").contains("newer than supported"),
        "got: {err:#}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn enforce_lru_capacity_does_not_loop_when_all_threads_are_active() {
    let mut active = ActiveThreads::default();
    let harness_a = mock_engine_handle();
    let harness_b = mock_engine_handle();

    active.engines.insert(
        "thr_a".to_string(),
        ActiveThreadState {
            engine: harness_a.handle,
            active_turn: Some(ActiveTurnState {
                turn_id: "turn_a".to_string(),
                interrupt_requested: false,
                auto_approve: true,
                trust_mode: false,
            }),
            route_provider: ApiProvider::Deepseek,
            route_model: DEFAULT_TEXT_MODEL.to_string(),
        },
    );
    active.engines.insert(
        "thr_b".to_string(),
        ActiveThreadState {
            engine: harness_b.handle,
            active_turn: Some(ActiveTurnState {
                turn_id: "turn_b".to_string(),
                interrupt_requested: false,
                auto_approve: true,
                trust_mode: false,
            }),
            route_provider: ApiProvider::Deepseek,
            route_model: DEFAULT_TEXT_MODEL.to_string(),
        },
    );
    active.lru.push_back("thr_a".to_string());
    active.lru.push_back("thr_b".to_string());

    let evicted = enforce_lru_capacity(&mut active, 2);
    assert!(evicted.is_empty(), "no idle threads should be evicted");
    assert_eq!(active.engines.len(), 2);
    assert_eq!(active.lru.len(), 2);
}

#[test]
fn approval_decision_keeps_trust_mode_out_of_tool_approval() {
    assert!(matches!(
        RuntimeThreadManager::approval_decision(false, false, false),
        RuntimeApprovalDecision::DenyTool
    ));
    assert!(matches!(
        RuntimeThreadManager::approval_decision(false, true, false),
        RuntimeApprovalDecision::DenyTool
    ));
    assert!(matches!(
        RuntimeThreadManager::approval_decision(true, false, false),
        RuntimeApprovalDecision::ApproveTool
    ));
    assert!(matches!(
        RuntimeThreadManager::approval_decision(true, false, true),
        RuntimeApprovalDecision::DenyTool
    ));
    assert!(matches!(
        RuntimeThreadManager::approval_decision(true, true, true),
        RuntimeApprovalDecision::RetryWithFullAccess
    ));
}

#[test]
fn open_recovers_queued_and_in_progress_turns() -> Result<()> {
    let runtime_dir = test_runtime_dir();
    let store = RuntimeThreadStore::open(runtime_dir.clone())?;
    let thread = sample_thread("thr_recover");
    store.save_thread(&thread)?;

    let mut queued_turn = sample_turn(&thread.id, "turn_queued", RuntimeTurnStatus::Queued);
    let mut in_progress_turn =
        sample_turn(&thread.id, "turn_running", RuntimeTurnStatus::InProgress);
    let completed_turn = sample_turn(&thread.id, "turn_done", RuntimeTurnStatus::Completed);

    let queued_item = sample_item(
        &queued_turn.id,
        "item_queued",
        TurnItemLifecycleStatus::Queued,
    );
    let in_progress_item = sample_item(
        &in_progress_turn.id,
        "item_running",
        TurnItemLifecycleStatus::InProgress,
    );
    let completed_item = sample_item(
        &completed_turn.id,
        "item_done",
        TurnItemLifecycleStatus::Completed,
    );

    queued_turn.item_ids = vec![queued_item.id.clone()];
    in_progress_turn.item_ids = vec![in_progress_item.id.clone()];

    store.save_item(&queued_item)?;
    store.save_item(&in_progress_item)?;
    store.save_item(&completed_item)?;
    store.save_turn(&queued_turn)?;
    store.save_turn(&in_progress_turn)?;
    store.save_turn(&completed_turn)?;

    let manager = test_manager(runtime_dir)?;

    let queued_turn = manager.store.load_turn(&queued_turn.id)?;
    assert_eq!(queued_turn.status, RuntimeTurnStatus::Interrupted);
    assert_eq!(queued_turn.error.as_deref(), Some(RUNTIME_RESTART_REASON));
    assert!(queued_turn.ended_at.is_some());
    assert!(queued_turn.duration_ms.is_some());

    let in_progress_turn = manager.store.load_turn(&in_progress_turn.id)?;
    assert_eq!(in_progress_turn.status, RuntimeTurnStatus::Interrupted);
    assert_eq!(
        in_progress_turn.error.as_deref(),
        Some(RUNTIME_RESTART_REASON)
    );
    assert!(in_progress_turn.ended_at.is_some());
    assert!(in_progress_turn.duration_ms.is_some());

    let completed_turn = manager.store.load_turn(&completed_turn.id)?;
    assert_eq!(completed_turn.status, RuntimeTurnStatus::Completed);
    assert!(completed_turn.error.is_none());

    let queued_item = manager.store.load_item("item_queued")?;
    assert_eq!(queued_item.status, TurnItemLifecycleStatus::Interrupted);
    assert!(queued_item.ended_at.is_some());

    let in_progress_item = manager.store.load_item("item_running")?;
    assert_eq!(
        in_progress_item.status,
        TurnItemLifecycleStatus::Interrupted
    );
    assert!(in_progress_item.ended_at.is_some());

    let completed_item = manager.store.load_item("item_done")?;
    assert_eq!(completed_item.status, TurnItemLifecycleStatus::Completed);

    Ok(())
}

#[tokio::test]
async fn thread_lifecycle_persists_across_restart() -> Result<()> {
    let runtime_dir = test_runtime_dir();
    let manager = test_manager(runtime_dir.clone())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;
    let tx_event = harness.tx_event;
    tokio::spawn(async move {
        if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "engine_turn_1".to_string(),
                    created_at: chrono::Utc::now(),
                    route: None,
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageDelta {
                    index: 0,
                    content: "mock response".to_string(),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageComplete { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 10,
                        output_tokens: 12,
                        ..Usage::default()
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
        }
    });

    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "first prompt".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                ..Default::default()
            },
        )
        .await?;
    let completed = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
    assert_eq!(completed.status, RuntimeTurnStatus::Completed);

    drop(manager);

    let reopened = test_manager(runtime_dir)?;
    let reopened_thread = reopened.get_thread(&thread.id).await?;
    assert_eq!(reopened_thread.id, thread.id);
    let turns = reopened.store.list_turns_for_thread(&thread.id)?;
    assert_eq!(turns.len(), 1);
    assert!(!reopened.store.list_items_for_turn(&turns[0].id)?.is_empty());
    let events = reopened.events_since(&thread.id, None)?;
    assert!(
        events.iter().any(|ev| ev.event == "turn.completed"),
        "expected turn.completed event after restart"
    );
    Ok(())
}

#[tokio::test]
async fn completed_turn_without_engine_output_fails() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;
    let tx_event = harness.tx_event;
    tokio::spawn(async move {
        if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "engine_empty_turn".to_string(),
                    created_at: chrono::Utc::now(),
                    route: None,
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 10,
                        output_tokens: 0,
                        ..Usage::default()
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
        }
    });

    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "empty turn".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                ..Default::default()
            },
        )
        .await?;

    let failed = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
    assert_eq!(failed.status, RuntimeTurnStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some(EMPTY_TURN_REASON));

    let events = manager.events_since(&thread.id, None)?;
    assert!(events.iter().any(|ev| {
        ev.event == "item.failed"
            && ev
                .payload
                .get("item")
                .and_then(|item| item.get("kind"))
                .and_then(Value::as_str)
                == Some("error")
    }));
    assert!(events.iter().any(|ev| {
        ev.event == "turn.completed"
            && ev
                .payload
                .get("turn")
                .and_then(|turn| turn.get("status"))
                .and_then(Value::as_str)
                == Some("failed")
    }));
    Ok(())
}

#[tokio::test]
async fn create_thread_defaults_auto_approve_to_false() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    assert!(!thread.auto_approve);
    Ok(())
}

#[tokio::test]
async fn start_turn_passes_effective_auto_approve_to_engine() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: Some(false),
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;

    let _turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "override approval".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: Some(true),
                ..Default::default()
            },
        )
        .await?;

    match rx_op.recv().await {
        Some(Op::SendMessage { auto_approve, .. }) => assert!(auto_approve),
        other => panic!("expected SendMessage op, got {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn start_turn_can_override_thread_auto_approve_to_false() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: Some(true),
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;

    let _turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "disable approval".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: Some(false),
                ..Default::default()
            },
        )
        .await?;

    match rx_op.recv().await {
        Some(Op::SendMessage { auto_approve, .. }) => assert!(!auto_approve),
        other => panic!("expected SendMessage op, got {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn multi_turn_continuity_same_thread() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;
    let tx_event = harness.tx_event;
    tokio::spawn(async move {
        let mut turn_index = 0u8;
        while let Some(op) = rx_op.recv().await {
            if !matches!(op, Op::SendMessage { .. }) {
                continue;
            }
            turn_index = turn_index.saturating_add(1);
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: format!("engine_turn_{turn_index}"),
                    created_at: chrono::Utc::now(),
                    route: None,
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageDelta {
                    index: 0,
                    content: format!("reply {turn_index}"),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageComplete { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 5,
                        output_tokens: 5,
                        ..Usage::default()
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                    tool_catalog: None,
                    base_url: None,
                })
                .await;
            if turn_index >= 2 {
                break;
            }
        }
    });

    let turn_1 = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "first".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                ..Default::default()
            },
        )
        .await?;
    let turn_1 = wait_for_terminal_turn(&manager, &turn_1.id, Duration::from_secs(2)).await?;
    assert_eq!(turn_1.status, RuntimeTurnStatus::Completed);

    let turn_2 = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "second".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                ..Default::default()
            },
        )
        .await?;
    let turn_2 = wait_for_terminal_turn(&manager, &turn_2.id, Duration::from_secs(2)).await?;
    assert_eq!(turn_2.status, RuntimeTurnStatus::Completed);

    let persisted_thread = manager.get_thread(&thread.id).await?;
    assert_eq!(
        persisted_thread.latest_turn_id.as_deref(),
        Some(turn_2.id.as_str())
    );
    let turns = manager.store.list_turns_for_thread(&thread.id)?;
    assert_eq!(turns.len(), 2);
    let mut items = Vec::new();
    for turn in &turns {
        items.extend(manager.store.list_items_for_turn(&turn.id)?);
    }
    assert!(items.iter().any(|item| {
        item.kind == TurnItemKind::UserMessage && item.detail.as_deref() == Some("first")
    }));
    assert!(items.iter().any(|item| {
        item.kind == TurnItemKind::UserMessage && item.detail.as_deref() == Some("second")
    }));

    let events = manager.events_since(&thread.id, None)?;
    let started = events
        .iter()
        .filter(|ev| ev.event == "turn.started")
        .count();
    let completed = events
        .iter()
        .filter(|ev| ev.event == "turn.completed")
        .count();
    assert_eq!(started, 2);
    assert_eq!(completed, 2);
    Ok(())
}

#[tokio::test]
async fn interrupt_turn_marks_interrupted_after_cleanup() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let harness = install_mock_engine(&manager, &thread.id).await;
    let mut rx_op = harness.rx_op;
    let tx_event = harness.tx_event;
    let cancel_token = harness.cancel_token;
    let cleanup_delay = Duration::from_millis(140);
    tokio::spawn(async move {
        if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "engine_turn_interrupt".to_string(),
                    created_at: chrono::Utc::now(),
                    route: None,
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageDelta {
                    index: 0,
                    content: "partial".to_string(),
                })
                .await;
            cancel_token.cancelled().await;
            sleep(cleanup_delay).await;
        }
    });

    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "interrupt me".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                ..Default::default()
            },
        )
        .await?;

    sleep(Duration::from_millis(20)).await;
    let interrupted_at = Instant::now();
    let interrupt_result = manager.interrupt_turn(&thread.id, &turn.id).await?;
    assert_eq!(interrupt_result.status, RuntimeTurnStatus::InProgress);

    let final_turn = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(3)).await?;
    assert_eq!(final_turn.status, RuntimeTurnStatus::Interrupted);
    assert!(
        interrupted_at.elapsed() >= cleanup_delay,
        "turn transitioned before cleanup finished"
    );

    let events = manager.events_since(&thread.id, None)?;
    let interrupt_seq = events
        .iter()
        .find(|ev| ev.event == "turn.interrupt_requested")
        .map(|ev| ev.seq)
        .context("missing turn.interrupt_requested event")?;
    let completed = events
        .iter()
        .find(|ev| ev.event == "turn.completed")
        .context("missing turn.completed event")?;
    assert!(completed.seq > interrupt_seq);
    assert_eq!(
        completed
            .payload
            .get("turn")
            .and_then(|turn| turn.get("status"))
            .and_then(Value::as_str),
        Some("interrupted")
    );
    Ok(())
}

#[tokio::test]
async fn approval_required_without_consumer_is_denied_and_persisted() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            auto_approve: Some(false),
            ..Default::default()
        })
        .await?;
    let mut harness = install_mock_engine(&manager, &thread.id).await;
    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "needs approval".to_string(),
                auto_approve: Some(false),
                ..Default::default()
            },
        )
        .await?;
    assert!(matches!(
        harness.rx_op.recv().await,
        Some(Op::SendMessage { .. })
    ));

    harness
        .tx_event
        .send(EngineEvent::ApprovalRequired {
            approval_key: "test_key".to_string(),
            approval_grouping_key: "test_key".to_string(),
            id: "tool_without_consumer".to_string(),
            tool_name: "exec_command".to_string(),
            description: "host must decide".to_string(),
            input: serde_json::json!({}),
            intent_summary: None,
            approval_force_prompt: false,
        })
        .await?;
    assert_eq!(
        harness.recv_approval_event().await,
        Some(MockApprovalEvent::Denied {
            id: "tool_without_consumer".to_string(),
        })
    );
    harness
        .tx_event
        .send(EngineEvent::TurnComplete {
            usage: Usage::default(),
            status: TurnOutcomeStatus::Completed,
            error: None,
            tool_catalog: None,
            base_url: None,
        })
        .await?;
    wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;

    let events = manager.events_since(&thread.id, None)?;
    assert!(
        events
            .iter()
            .any(|event| event.event == "approval.required")
    );
    assert!(events.iter().any(|event| {
        event.event == "approval.decided"
            && event.payload.get("decision").and_then(Value::as_str) == Some("deny")
            && event.payload.get("reason").and_then(Value::as_str)
                == Some("no interactive approval consumer")
    }));
    Ok(())
}

#[tokio::test]
async fn approval_required_with_stale_active_turn_is_denied() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: Some(true),
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let mut harness = install_mock_engine(&manager, &thread.id).await;
    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "needs approval".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: Some(true),
                ..Default::default()
            },
        )
        .await?;

    assert!(matches!(
        harness.rx_op.recv().await,
        Some(Op::SendMessage { .. })
    ));
    {
        let mut active = manager.active.lock().await;
        let state = active
            .engines
            .get_mut(&thread.id)
            .context("missing active thread state")?;
        state.active_turn = None;
    }

    harness
        .tx_event
        .send(EngineEvent::ApprovalRequired {
            approval_key: "test_key".to_string(),
            approval_grouping_key: "test_key".to_string(),
            id: "tool_stale".to_string(),
            tool_name: "exec_command".to_string(),
            description: "stale approval".to_string(),
            input: serde_json::json!({}),
            intent_summary: None,
            approval_force_prompt: false,
        })
        .await?;

    assert_eq!(
        harness.recv_approval_event().await,
        Some(MockApprovalEvent::Denied {
            id: "tool_stale".to_string(),
        })
    );

    harness
        .tx_event
        .send(EngineEvent::TurnComplete {
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                ..Usage::default()
            },
            status: TurnOutcomeStatus::Completed,
            error: None,
            tool_catalog: None,
            base_url: None,
        })
        .await?;

    let terminal = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
    assert_eq!(terminal.status, RuntimeTurnStatus::Completed);
    Ok(())
}

#[tokio::test]
async fn thinking_delta_emits_agent_reasoning_item() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: None,
            auto_approve: Some(true),
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;
    let mut harness = install_mock_engine(&manager, &thread.id).await;
    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "show your thinking".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: Some(true),
                ..Default::default()
            },
        )
        .await?;
    assert!(matches!(
        harness.rx_op.recv().await,
        Some(Op::SendMessage { .. })
    ));

    harness
        .tx_event
        .send(EngineEvent::ThinkingStarted { index: 0 })
        .await?;
    harness
        .tx_event
        .send(EngineEvent::ThinkingDelta {
            index: 0,
            content: "Let me reason about this.".to_string(),
        })
        .await?;
    harness
        .tx_event
        .send(EngineEvent::ThinkingComplete { index: 0 })
        .await?;
    harness
        .tx_event
        .send(EngineEvent::TurnComplete {
            usage: Usage::default(),
            status: TurnOutcomeStatus::Completed,
            error: None,
            tool_catalog: None,
            base_url: None,
        })
        .await?;

    let terminal = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(5)).await?;
    assert_eq!(terminal.status, RuntimeTurnStatus::Completed);
    let events = manager.events_since(&thread.id, None)?;
    let delta = events
        .iter()
        .find(|record| {
            record.event == "item.delta"
                && record.payload.get("kind").and_then(Value::as_str) == Some("agent_reasoning")
        })
        .expect("expected persisted reasoning delta");
    assert_eq!(
        delta.payload.get("delta").and_then(Value::as_str),
        Some("Let me reason about this.")
    );
    assert!(events.iter().any(|record| {
        record.event == "item.completed"
            && record
                .payload
                .get("item")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                == Some("agent_reasoning")
    }));
    Ok(())
}

#[tokio::test]
async fn elevation_required_with_stale_active_turn_is_denied() -> Result<()> {
    let manager = test_manager(test_runtime_dir())?;
    let thread = manager
        .create_thread(CreateThreadRequest {
            model: None,
            workspace: None,
            mode: None,
            allow_shell: None,
            trust_mode: Some(true),
            auto_approve: Some(true),
            archived: false,
            system_prompt: None,
            task_id: None,
            ..Default::default()
        })
        .await?;

    let mut harness = install_mock_engine(&manager, &thread.id).await;
    let turn = manager
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt: "needs elevation".to_string(),
                input_summary: None,
                model: None,
                mode: None,
                allow_shell: None,
                trust_mode: Some(true),
                auto_approve: Some(true),
                ..Default::default()
            },
        )
        .await?;

    assert!(matches!(
        harness.rx_op.recv().await,
        Some(Op::SendMessage { .. })
    ));
    {
        let mut active = manager.active.lock().await;
        let state = active
            .engines
            .get_mut(&thread.id)
            .context("missing active thread state")?;
        state.active_turn = None;
    }

    harness
        .tx_event
        .send(EngineEvent::ElevationRequired {
            tool_id: "tool_stale_elevated".to_string(),
            tool_name: "exec_command".to_string(),
            command: None,
            denial_reason: "sandbox denied".to_string(),
            blocked_network: false,
            blocked_write: false,
        })
        .await?;

    assert_eq!(
        harness.recv_approval_event().await,
        Some(MockApprovalEvent::Denied {
            id: "tool_stale_elevated".to_string(),
        })
    );

    harness
        .tx_event
        .send(EngineEvent::TurnComplete {
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                ..Usage::default()
            },
            status: TurnOutcomeStatus::Completed,
            error: None,
            tool_catalog: None,
            base_url: None,
        })
        .await?;

    let terminal = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
    assert_eq!(terminal.status, RuntimeTurnStatus::Completed);
    Ok(())
}

#[test]
fn summarize_text_truncates() {
    let out = summarize_text("abcdefghijklmnopqrstuvwxyz", 10);
    assert_eq!(out, "abcdefg...");
}

#[test]
fn approval_decision_requires_auto_approve_and_trust_for_full_access() {
    assert_eq!(
        RuntimeThreadManager::approval_decision(false, false, false),
        RuntimeApprovalDecision::DenyTool
    );
    assert_eq!(
        RuntimeThreadManager::approval_decision(false, true, false),
        RuntimeApprovalDecision::DenyTool
    );
    assert_eq!(
        RuntimeThreadManager::approval_decision(true, false, false),
        RuntimeApprovalDecision::ApproveTool
    );
    assert_eq!(
        RuntimeThreadManager::approval_decision(true, false, true),
        RuntimeApprovalDecision::DenyTool
    );
    assert_eq!(
        RuntimeThreadManager::approval_decision(true, true, true),
        RuntimeApprovalDecision::RetryWithFullAccess
    );
}

#[test]
fn opening_manager_recovers_stale_queued_and_in_progress_work() -> Result<()> {
    let data_dir = test_runtime_dir();
    let manager = test_manager(data_dir.clone())?;
    let started_at = Utc::now() - chrono::Duration::seconds(5);
    let created_at = started_at - chrono::Duration::seconds(1);

    let thread = ThreadRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "thr_restart".to_string(),
        created_at,
        updated_at: created_at,
        model: DEFAULT_TEXT_MODEL.to_string(),
        workspace: PathBuf::from("."),
        mode: "agent".to_string(),
        allow_shell: false,
        trust_mode: false,
        auto_approve: false,
        latest_turn_id: Some("turn_in_progress".to_string()),
        latest_response_bookmark: None,
        archived: false,
        system_prompt: None,
        task_id: None,
        title: None,
        session_id: None,
    };
    manager.store.save_thread(&thread)?;

    let completed_item = TurnItemRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "item_completed".to_string(),
        turn_id: "turn_in_progress".to_string(),
        kind: TurnItemKind::Status,
        status: TurnItemLifecycleStatus::Completed,
        summary: "done".to_string(),
        detail: None,
        metadata: None,
        artifact_refs: Vec::new(),
        started_at: Some(started_at),
        ended_at: Some(started_at + chrono::Duration::seconds(1)),
    };
    let in_progress_item = TurnItemRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "item_in_progress".to_string(),
        turn_id: "turn_in_progress".to_string(),
        kind: TurnItemKind::ToolCall,
        status: TurnItemLifecycleStatus::InProgress,
        summary: "running".to_string(),
        detail: None,
        metadata: None,
        artifact_refs: Vec::new(),
        started_at: Some(started_at),
        ended_at: None,
    };
    let queued_item = TurnItemRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "item_queued".to_string(),
        turn_id: "turn_queued".to_string(),
        kind: TurnItemKind::ToolCall,
        status: TurnItemLifecycleStatus::Queued,
        summary: "queued".to_string(),
        detail: None,
        metadata: None,
        artifact_refs: Vec::new(),
        started_at: None,
        ended_at: None,
    };
    manager.store.save_item(&completed_item)?;
    manager.store.save_item(&in_progress_item)?;
    manager.store.save_item(&queued_item)?;

    manager.store.save_turn(&TurnRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "turn_in_progress".to_string(),
        thread_id: thread.id.clone(),
        status: RuntimeTurnStatus::InProgress,
        input_summary: "hello".to_string(),
        created_at,
        started_at: Some(started_at),
        ended_at: None,
        duration_ms: None,
        usage: None,
        effective_provider: None,
        effective_billing_surface: None,
        effective_model: None,
        error: None,
        item_ids: vec![completed_item.id.clone(), in_progress_item.id.clone()],
        steer_count: 0,
    })?;
    manager.store.save_turn(&TurnRecord {
        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
        id: "turn_queued".to_string(),
        thread_id: thread.id.clone(),
        status: RuntimeTurnStatus::Queued,
        input_summary: "later".to_string(),
        created_at,
        started_at: None,
        ended_at: None,
        duration_ms: None,
        usage: None,
        effective_provider: None,
        effective_billing_surface: None,
        effective_model: None,
        error: None,
        item_ids: vec![queued_item.id.clone()],
        steer_count: 0,
    })?;
    drop(manager);

    let recovered = test_manager(data_dir)?;

    let recovered_thread = recovered.store.load_thread(&thread.id)?;
    assert!(recovered_thread.updated_at >= thread.updated_at);

    let recovered_in_progress_turn = recovered.store.load_turn("turn_in_progress")?;
    assert_eq!(
        recovered_in_progress_turn.status,
        RuntimeTurnStatus::Interrupted
    );
    assert_eq!(
        recovered_in_progress_turn.error.as_deref(),
        Some(RUNTIME_RESTART_REASON)
    );
    assert!(recovered_in_progress_turn.ended_at.is_some());
    assert!(
        recovered_in_progress_turn
            .duration_ms
            .is_some_and(|duration| duration >= 5_000)
    );

    let recovered_queued_turn = recovered.store.load_turn("turn_queued")?;
    assert_eq!(recovered_queued_turn.status, RuntimeTurnStatus::Interrupted);
    assert_eq!(
        recovered_queued_turn.error.as_deref(),
        Some(RUNTIME_RESTART_REASON)
    );
    assert!(recovered_queued_turn.ended_at.is_some());
    assert_eq!(recovered_queued_turn.duration_ms, None);

    assert_eq!(
        recovered.store.load_item(&completed_item.id)?.status,
        TurnItemLifecycleStatus::Completed
    );
    let recovered_in_progress_item = recovered.store.load_item(&in_progress_item.id)?;
    assert_eq!(
        recovered_in_progress_item.status,
        TurnItemLifecycleStatus::Interrupted
    );
    assert!(recovered_in_progress_item.ended_at.is_some());

    let recovered_queued_item = recovered.store.load_item(&queued_item.id)?;
    assert_eq!(
        recovered_queued_item.status,
        TurnItemLifecycleStatus::Interrupted
    );
    assert!(recovered_queued_item.ended_at.is_some());

    Ok(())
}

#[test]
fn parse_mode_defaults_to_agent() {
    assert_eq!(parse_mode("unknown"), AppMode::Agent);
    assert_eq!(parse_mode("plan"), AppMode::Plan);
}

#[test]
fn parse_mode_opt_resolves_explicit_tokens_and_aliases() {
    assert_eq!(parse_mode_opt("agent"), Some(AppMode::Agent));
    assert_eq!(parse_mode_opt("1"), Some(AppMode::Agent));
    assert_eq!(parse_mode_opt("plan"), Some(AppMode::Plan));
    assert_eq!(parse_mode_opt("2"), Some(AppMode::Plan));
    assert_eq!(parse_mode_opt("auto"), Some(AppMode::Agent));
    assert_eq!(parse_mode_opt("3"), None);
    assert_eq!(parse_mode_opt("yolo"), Some(AppMode::Yolo));
    assert_eq!(parse_mode_opt("4"), Some(AppMode::Yolo));
    assert_eq!(parse_mode_opt(" PLAN "), Some(AppMode::Plan));
}

#[test]
fn parse_mode_opt_rejects_prompt_fragments() {
    for input in [
        "plan a trip to Tokyo",
        "switch the agent on",
        "enter yolo mode",
        "agent of chaos",
        "mode",
    ] {
        assert_eq!(parse_mode_opt(input), None);
    }
}

#[test]
fn parse_mode_wrapper_defaults_and_resolves_numeric_aliases() {
    assert_eq!(parse_mode("plan a trip to Tokyo"), AppMode::Agent);
    assert_eq!(parse_mode("auto"), AppMode::Agent);
    assert_eq!(parse_mode("1"), AppMode::Agent);
    assert_eq!(parse_mode("2"), AppMode::Plan);
    assert_eq!(parse_mode("3"), AppMode::Agent);
    assert_eq!(parse_mode("4"), AppMode::Yolo);
}

#[test]
fn summary_merge_appends_section_to_base_prompt() {
    let merged = merge_summary_into_prompt(
        Some("You are a helpful agent."),
        "## 📋 Conversation Summary (Auto-Generated)\n\nUser prefers lists.",
    );
    assert!(merged.starts_with("You are a helpful agent."));
    assert!(merged.contains(COMPACTION_SUMMARY_BEGIN));
    assert!(merged.contains("User prefers lists."));
    assert!(merged.ends_with(COMPACTION_SUMMARY_END));
    // Reload restore keys on the marker: SyncSession maps the record to
    // SystemPrompt::Text and extract_compaction_summary_prompt checks
    // `contains("Conversation Summary (Auto-Generated)")`.
    assert!(merged.contains("Conversation Summary (Auto-Generated)"));
}

#[test]
fn summary_merge_replaces_existing_section_idempotently() {
    let first = merge_summary_into_prompt(Some("Base prompt."), "summary v1");
    let second = merge_summary_into_prompt(Some(&first), "summary v2");
    assert!(second.contains("summary v2"));
    assert!(!second.contains("summary v1"));
    assert_eq!(
        second.matches(COMPACTION_SUMMARY_BEGIN).count(),
        1,
        "repeated compactions must swap the section, not stack duplicates"
    );
    assert!(second.starts_with("Base prompt."));
}

#[test]
fn summary_merge_handles_missing_base() {
    let merged = merge_summary_into_prompt(None, "only summary");
    assert!(merged.starts_with(COMPACTION_SUMMARY_BEGIN));
    assert!(merged.contains("only summary"));
    let empty_base = merge_summary_into_prompt(Some(""), "only summary");
    assert!(empty_base.starts_with(COMPACTION_SUMMARY_BEGIN));
}

#[test]
fn summary_strip_preserves_text_after_section() {
    let with_tail = format!(
        "Base.\n\n{COMPACTION_SUMMARY_BEGIN}\nold summary\n{COMPACTION_SUMMARY_END}\n\nTrailing rules."
    );
    let stripped = strip_summary_section(&with_tail);
    assert!(stripped.contains("Base."));
    assert!(stripped.contains("Trailing rules."));
    assert!(!stripped.contains("old summary"));
    // Re-merge keeps the tail intact.
    let merged = merge_summary_into_prompt(Some(&with_tail), "new summary");
    assert!(merged.contains("Trailing rules."));
    assert!(merged.contains("new summary"));
}

#[test]
fn summary_strip_handles_missing_end_sentinel() {
    let broken = format!("Base.\n\n{COMPACTION_SUMMARY_BEGIN}\ntruncated…");
    let stripped = strip_summary_section(&broken);
    assert_eq!(stripped, "Base.");
}
