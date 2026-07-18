use std::path::PathBuf;

use super::*;
use crate::tui::app::QueuedMessage;

use std::sync::{Arc, atomic::AtomicBool};

fn create_test_app() -> App {
    let options = TuiOptions {
        model: "deepseek-v4-pro".to_string(),
        workspace: PathBuf::from("."),
        config_path: None,
        config_profile: None,
        allow_shell: false,
        use_alt_screen: true,
        use_mouse_capture: false,
        use_bracketed_paste: true,
        max_subagents: 1,
        skills_dir: PathBuf::from("."),
        memory_path: PathBuf::from("memory.md"),
        notes_path: PathBuf::from("notes.txt"),
        mcp_config_path: PathBuf::from("mcp.json"),
        use_memory: false,
        start_in_agent_mode: true,
        skip_onboarding: true,
        yolo: false,
        resume_session_id: None,
        initial_input: None,
    };
    let mut app = App::new(options, &Config::default());
    app.api_provider = ApiProvider::Deepseek;
    app.model = "deepseek-v4-pro".to_string();
    app.auto_model = false;
    app.last_effective_model = None;
    app.active_route_limits = None;
    app.active_context_window_override = None;
    app.status_message = None;
    app
}

#[test]
fn canonical_approval_risk_projects_one_to_one_into_tui_stakes() {
    use crate::tui::approval::ApprovalStakes;

    assert_eq!(
        project_approval_risk(ApprovalRisk::Routine),
        ApprovalStakes::Routine
    );
    assert_eq!(
        project_approval_risk(ApprovalRisk::Elevated),
        ApprovalStakes::Elevated
    );
    assert_eq!(
        project_approval_risk(ApprovalRisk::Critical),
        ApprovalStakes::Critical
    );
}

#[test]
fn canonical_slash_menu_selection_wraps_and_clamps() {
    let mut app = create_test_app();

    select_previous_slash_menu_entry(&mut app, 3);
    assert_eq!(app.slash_menu_selected, 2);

    select_next_slash_menu_entry(&mut app, 3);
    assert_eq!(app.slash_menu_selected, 0);

    app.slash_menu_selected = 99;
    select_previous_slash_menu_entry(&mut app, 3);
    assert_eq!(app.slash_menu_selected, 1);

    select_next_slash_menu_entry(&mut app, 0);
    assert_eq!(app.slash_menu_selected, 1);
}

#[test]
fn canonical_approval_can_inspect_and_copy_full_params_locally() {
    let mut app = create_test_app();
    let request = ApprovalRequest::elevated(
        "interaction-1",
        "read_file",
        "读取完整参数测试",
        &serde_json::json!({"path": "src/main.rs"}),
    );
    app.view_stack.push(ApprovalView::new(request));

    let events = app
        .view_stack
        .handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
    assert_eq!(events.len(), 1);
    assert!(
        handle_canonical_local_view_event(&mut app, events.into_iter().next().unwrap()).is_none()
    );
    assert_eq!(app.view_stack.top_kind(), Some(ModalKind::Pager));

    let copy_events = app
        .view_stack
        .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert_eq!(copy_events.len(), 1);
    assert!(
        handle_canonical_local_view_event(&mut app, copy_events.into_iter().next().unwrap(),)
            .is_none()
    );
    assert!(
        app.clipboard
            .last_written_text()
            .is_some_and(|text| text.contains("src/main.rs"))
    );
    assert!(
        app.status_message
            .as_deref()
            .is_some_and(|message| message.contains("已复制到剪贴板"))
    );

    let close_events = app
        .view_stack
        .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(close_events.is_empty());
    assert_eq!(app.view_stack.top_kind(), Some(ModalKind::Approval));
}

#[test]
fn canonical_mouse_click_on_approval_emits_decision() {
    use crossterm::event::MouseButton;

    let mut app = create_test_app();
    let request = ApprovalRequest::routine(
        "interaction-mouse",
        "read_file",
        "测试鼠标批准",
        &serde_json::json!({"path": "src/main.rs"}),
    );
    let approval = ApprovalView::new(request);
    approval.set_mouse_hitboxes(vec![Rect::new(4, 6, 24, 1)]);
    app.view_stack.push(approval);

    let events = route_canonical_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        },
    );

    assert!(matches!(
        events.as_slice(),
        [ViewEvent::ApprovalDecision {
            interaction_id,
            decision: ReviewDecision::Approved,
        }] if interaction_id == "interaction-mouse"
    ));
    assert!(app.view_stack.is_empty());
}

#[test]
fn canonical_mouse_wheel_is_consumed_by_active_modal() {
    use crate::tui::scrolling::TranscriptScroll;

    let mut app = create_test_app();
    app.viewport.transcript_scroll = TranscriptScroll::at_line(7);
    let transcript_before = app.viewport.transcript_scroll;
    let request = ApprovalRequest::routine(
        "interaction-wheel",
        "read_file",
        "测试模态框滚轮",
        &serde_json::json!({"path": "src/main.rs"}),
    );
    app.view_stack.push(ApprovalView::new(request));

    let events = route_canonical_mouse_event(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );

    assert!(events.is_empty());
    assert_eq!(app.viewport.transcript_scroll, transcript_before);
    let decision = app
        .view_stack
        .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        decision.as_slice(),
        [ViewEvent::ApprovalDecision {
            interaction_id,
            decision: ReviewDecision::Denied,
        }] if interaction_id == "interaction-wheel"
    ));
}

#[test]
fn canonical_mouse_without_modal_keeps_transcript_scroll_behavior() {
    use crate::tui::scrolling::TranscriptScroll;

    let mouse = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    let mut expected = create_test_app();
    expected.viewport.transcript_scroll = TranscriptScroll::at_line(7);
    expected.scroll_up(3);

    let mut actual = create_test_app();
    actual.viewport.transcript_scroll = TranscriptScroll::at_line(7);
    let events = route_canonical_mouse_event(&mut actual, mouse);

    assert!(events.is_empty());
    assert_eq!(
        actual.viewport.transcript_scroll,
        expected.viewport.transcript_scroll
    );
}

#[test]
fn terminal_origin_reset_recovers_viewport_without_destructive_clear() {
    assert!(TERMINAL_ORIGIN_RESET.starts_with(b"\x1b[r\x1b[?6l"));
    assert!(TERMINAL_ORIGIN_RESET.ends_with(b"\x1b[H"));
    assert!(
        !TERMINAL_ORIGIN_RESET
            .windows(b"\x1b[2J".len())
            .any(|sequence| sequence == b"\x1b[2J")
    );
    assert!(
        !TERMINAL_ORIGIN_RESET
            .windows(b"\x1b[3J".len())
            .any(|sequence| sequence == b"\x1b[3J")
    );
}

#[cfg(not(windows))]
#[test]
fn recover_terminal_modes_emits_expected_gated_sequences() {
    let mut all_on = Vec::new();
    let mut all_off = Vec::new();

    recover_terminal_modes(&mut all_on, true, true);
    recover_terminal_modes(&mut all_off, false, false);

    let on = String::from_utf8_lossy(&all_on);
    let off = String::from_utf8_lossy(&all_off);
    assert!(on.contains("\x1b[?1004h") && off.contains("\x1b[?1004h"));
    assert!(on.contains("\x1b[<1u") && on.contains("\x1b[>1u"));
    assert!(off.contains("\x1b[<1u") && off.contains("\x1b[>1u"));
    assert!(on.contains("\x1b[?1007h"));
    assert!(off.contains("\x1b[?1007l"));
    assert!(on.contains("\x1b[?1000h"));
    assert!(!off.contains("\x1b[?1000h"));
    assert!(on.contains("\x1b[?2004h"));
    assert!(!off.contains("\x1b[?2004h"));
}

#[test]
fn alternate_scroll_mode_helpers_emit_matching_sequences() {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();

    enable_alternate_scroll_mode(&mut enabled);
    disable_alternate_scroll_mode(&mut disabled);

    assert_eq!(enabled, ENABLE_ALT_SCROLL_MODE);
    assert_eq!(disabled, DISABLE_ALT_SCROLL_MODE);
}

#[cfg(not(windows))]
#[test]
fn bracketed_paste_helpers_tolerate_unsupported_terminals() {
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("terminal mode unsupported"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("terminal mode unsupported"))
        }
    }

    let mut writer = FailingWriter;
    assert!(!try_enable_bracketed_paste_mode(&mut writer));
    disable_bracketed_paste_mode(&mut writer);
}

#[cfg(windows)]
#[test]
fn recover_terminal_modes_runs_without_panic_on_windows() {
    let mut buf = Vec::new();
    recover_terminal_modes(&mut buf, true, true);
    recover_terminal_modes(&mut buf, false, false);
}

#[cfg(windows)]
#[test]
fn kitty_keyboard_helpers_emit_direct_windows_sequences() {
    let mut pushed = Vec::new();
    let mut popped = Vec::new();

    push_keyboard_enhancement_flags(&mut pushed);
    pop_keyboard_enhancement_flags(&mut popped);

    assert!(String::from_utf8_lossy(&pushed).contains("\x1b[>0u"));
    assert!(String::from_utf8_lossy(&popped).contains("\x1b[<1u"));
}

#[test]
fn terminal_probe_timeout_defaults_and_clamps() {
    let mut config = Config::default();
    assert_eq!(terminal_probe_timeout(&config), Duration::from_millis(500));

    config.tui = Some(crate::config::TuiConfig {
        terminal_probe_timeout_ms: Some(750),
        ..crate::config::TuiConfig::default()
    });
    assert_eq!(terminal_probe_timeout(&config), Duration::from_millis(750));

    config
        .tui
        .as_mut()
        .expect("tui config")
        .terminal_probe_timeout_ms = Some(0);
    assert_eq!(terminal_probe_timeout(&config), Duration::from_millis(100));

    config
        .tui
        .as_mut()
        .expect("tui config")
        .terminal_probe_timeout_ms = Some(60_000);
    assert_eq!(
        terminal_probe_timeout(&config),
        Duration::from_millis(5_000)
    );
}

#[test]
fn raw_mode_probe_handshake_assigns_cleanup_sequentially() {
    let enabled = AtomicBool::new(false);
    let abandoned = AtomicBool::new(false);
    assert!(!raw_mode_probe_handshake(&enabled, &abandoned));
    assert!(raw_mode_probe_handshake(&abandoned, &enabled));

    let enabled = AtomicBool::new(false);
    let abandoned = AtomicBool::new(false);
    assert!(!raw_mode_probe_handshake(&abandoned, &enabled));
    assert!(raw_mode_probe_handshake(&enabled, &abandoned));
}

#[test]
fn raw_mode_probe_handshake_never_leaks_under_concurrent_race() {
    for _ in 0..200 {
        let enabled = Arc::new(AtomicBool::new(false));
        let abandoned = Arc::new(AtomicBool::new(false));
        let task_enabled = Arc::clone(&enabled);
        let task_abandoned = Arc::clone(&abandoned);
        let task =
            std::thread::spawn(move || raw_mode_probe_handshake(&task_enabled, &task_abandoned));

        let caller_disables = raw_mode_probe_handshake(&abandoned, &enabled);
        let task_disables = task.join().expect("probe task");
        assert!(task_disables || caller_disables);
    }
}

#[test]
fn sanitize_stream_chunk_preserves_unicode_and_visible_whitespace() {
    let chunk = "你好，DeepSeek\t🚀\n café";
    assert_eq!(sanitize_stream_chunk(chunk), chunk);
}

#[test]
fn sanitize_stream_chunk_drops_terminal_control_characters() {
    assert_eq!(
        sanitize_stream_chunk("text\u{1b}[2Jmore\u{7}\u{8}\r\n"),
        "text[2Jmore\n"
    );
    assert_eq!(sanitize_stream_chunk("\u{1b}\u{7}\u{8}"), "");
}

#[test]
fn canonical_start_command_honors_disabled_subagents() {
    let mut app = create_test_app();
    app.max_subagents = 12;
    let config = Config {
        subagents: Some(crate::config::SubagentsConfig {
            enabled: Some(false),
            ..crate::config::SubagentsConfig::default()
        }),
        ..Config::default()
    };

    let command = canonical_start_command(&app, &config, "检查项目".to_owned());
    assert_eq!(command.limits.max_depth, 0);
    assert_eq!(command.limits.max_concurrent_children, 0);
}

#[test]
fn canonical_start_command_uses_configured_subagent_limits() {
    let mut app = create_test_app();
    app.max_subagents = 12;
    let config = Config {
        subagents: Some(crate::config::SubagentsConfig {
            max_concurrent: Some(4),
            max_depth: Some(2),
            ..crate::config::SubagentsConfig::default()
        }),
        ..Config::default()
    };

    let command = canonical_start_command(&app, &config, "并行审计".to_owned());
    assert_eq!(command.limits.max_depth, 2);
    assert_eq!(command.limits.max_concurrent_children, 4);
}

#[test]
fn pending_input_preview_projects_all_live_buckets() {
    let mut app = create_test_app();
    app.push_pending_steer(QueuedMessage::new("steer-msg".to_string(), None));
    app.rejected_steers.push_back("rejected-msg".to_string());
    app.queue_message(QueuedMessage::new("queued-msg".to_string(), None));

    let preview = build_pending_input_preview(&app);
    assert_eq!(preview.pending_steers, ["steer-msg"]);
    assert_eq!(preview.rejected_steers, ["rejected-msg"]);
    assert_eq!(preview.queued_messages, ["queued-msg"]);
}

#[test]
fn context_usage_uses_latest_canonical_prompt_usage() {
    let mut app = create_test_app();
    app.session.last_prompt_tokens = Some(320_000);

    let (used, max, percent) = context_usage_snapshot(&app).expect("context usage");
    assert_eq!(max, 1_000_000);
    assert_eq!(used, 320_000);
    assert_eq!(percent, 32.0);
}

fn complete_release_json(tag: &str) -> serde_json::Value {
    let assets = REQUIRED_RELEASE_ASSETS
        .iter()
        .map(|name| serde_json::json!({ "name": name, "state": "uploaded" }))
        .collect::<Vec<_>>();
    serde_json::json!({
        "tag_name": tag,
        "draft": false,
        "prerelease": false,
        "assets": assets,
    })
}

#[test]
fn semver_parser_accepts_core_versions_and_rejects_suffixes() {
    assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
    assert_eq!(parse_semver("1.2"), Some((1, 2, 0)));
    assert_eq!(parse_semver("1.2.3-pre"), None);
    assert_eq!(parse_semver("not-a-version"), None);
}

#[test]
fn version_hint_requires_a_publishable_complete_release() {
    let complete = complete_release_json("v0.8.47");
    let hint = version_hint_from_release_json(&complete, "0.8.46").expect("new release");
    assert!(hint.contains("v0.8.47 available"));

    let mut draft = complete_release_json("v0.8.47");
    draft["draft"] = serde_json::Value::Bool(true);
    assert!(version_hint_from_release_json(&draft, "0.8.46").is_none());

    let mut missing_asset = complete_release_json("v0.8.47");
    missing_asset["assets"]
        .as_array_mut()
        .expect("assets")
        .pop();
    assert!(version_hint_from_release_json(&missing_asset, "0.8.46").is_none());

    assert!(version_hint_from_release_json(&complete, "0.8.47").is_none());
}

#[test]
fn startup_version_source_respects_update_configuration() {
    assert_eq!(
        startup_version_check_source(&UpdateConfig {
            check_for_updates: false,
            update_uri: Some("https://mirror.example/releases/latest".to_string()),
        }),
        StartupVersionCheckSource::Disabled
    );
    assert_eq!(
        startup_version_check_source(&UpdateConfig {
            check_for_updates: true,
            update_uri: Some(" https://mirror.example/releases/latest ".to_string()),
        }),
        StartupVersionCheckSource::ConfiguredUrl(
            "https://mirror.example/releases/latest".to_string()
        )
    );
    assert_eq!(
        startup_version_check_source(&UpdateConfig::default()),
        StartupVersionCheckSource::ReleaseResolver
    );
}
