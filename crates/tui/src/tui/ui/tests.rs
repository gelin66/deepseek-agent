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
fn focus_gained_forces_terminal_viewport_recapture() {
    assert!(terminal_event_needs_viewport_recapture(&Event::FocusGained));
    assert!(!terminal_event_needs_viewport_recapture(&Event::FocusLost));
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
fn event_poll_timeout_has_a_nonzero_floor() {
    assert_eq!(
        clamp_event_poll_timeout(Duration::ZERO),
        Duration::from_millis(1)
    );
    assert_eq!(
        clamp_event_poll_timeout(Duration::from_micros(250)),
        Duration::from_millis(1)
    );
    assert_eq!(
        clamp_event_poll_timeout(Duration::from_millis(24)),
        Duration::from_millis(24)
    );
}

#[test]
fn animation_and_poll_intervals_follow_motion_preferences() {
    let mut app = create_test_app();
    app.low_motion = false;
    assert_eq!(
        animation_interval_ms(&app, true, false),
        UI_STATUS_ANIMATION_MS
    );
    assert_eq!(
        animation_interval_ms(&app, false, true),
        UI_UNDERWATER_ANIMATION_MS
    );
    assert_eq!(
        animation_interval_ms(&app, true, true),
        UI_STATUS_ANIMATION_MS.min(UI_UNDERWATER_ANIMATION_MS)
    );
    assert_eq!(active_poll_ms(&app), UI_ACTIVE_POLL_MS);
    assert_eq!(idle_poll_ms(&app), UI_IDLE_POLL_MS);

    app.low_motion = true;
    assert_eq!(animation_interval_ms(&app, true, false), 2_400);
    assert_eq!(
        animation_interval_ms(&app, true, true),
        UI_UNDERWATER_ANIMATION_MS
    );
    assert_eq!(active_poll_ms(&app), 96);
    assert_eq!(idle_poll_ms(&app), 120);
}

#[test]
fn status_animation_ticks_only_for_live_state() {
    let mut app = create_test_app();
    assert!(!should_tick_status_animation(&app, false, false, false));
    assert!(should_tick_status_animation(&app, true, false, false));

    app.is_loading = true;
    assert!(should_tick_status_animation(&app, false, false, false));
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

#[test]
fn context_pressure_warning_reflects_auto_compact_state() {
    let mut app = create_test_app();
    app.active_route_limits = Some(codewhale_config::route::RouteLimits {
        context_tokens: Some(20_000),
        ..codewhale_config::route::RouteLimits::default()
    });
    app.session.last_prompt_tokens = Some(18_000);
    app.auto_compact = true;
    app.auto_compact_threshold_percent = 70.0;

    maybe_warn_context_pressure(&mut app);

    let status = app.status_message.expect("context warning");
    assert!(status.contains("Auto-compaction will run before the next send."));
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
