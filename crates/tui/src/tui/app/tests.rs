use super::*;
use crate::config::Config;
use crate::test_support::{EnvVarGuard, lock_test_env};
use crate::tui::history::HistoryCell;

fn test_options(yolo: bool) -> TuiOptions {
    TuiOptions {
        model: "deepseek-v4-pro".to_string(),
        language: dse_localization::ProductLanguage::SimplifiedChinese,
        workspace: PathBuf::from("."),
        config_path: None,
        allow_shell: yolo,
        use_alt_screen: true,
        use_mouse_capture: false,
        use_bracketed_paste: true,
        max_subagents: 1,
        skills_dir: PathBuf::from("."),
        mcp_config_path: PathBuf::from("mcp.json"),
        skip_onboarding: false,
        yolo,
        resume_session_id: None,
        initial_input: None,
    }
}

#[cfg(unix)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[test]
fn initial_input_prefill_waits_for_manual_submit() {
    let mut options = test_options(false);
    options.initial_input = Some(InitialInput::Prefill("review this PR".to_string()));
    let app = App::new(options, &Config::default());
    assert_eq!(app.input, "review this PR");
    assert!(!app.auto_submit_initial_input);
}

#[test]
fn m8a_app_projection_is_deepseek_only() {
    let config = Config {
        api_key: Some("fixture-key".to_string()),
        ..Config::default()
    };
    let mut options = test_options(false);
    options.model = "deepseek-v4-flash".to_string();
    let app = App::new(options, &config);
    assert_eq!(app.model, "deepseek-v4-flash");
}

#[test]
fn m17d_app_projects_both_languages_without_changing_machine_facts() {
    let config = Config {
        api_key: Some("fixture-key".to_string()),
        ..Config::default()
    };
    let mut english_options = test_options(false);
    english_options.language = dse_localization::ProductLanguage::English;
    let english = App::new(english_options, &config);

    let chinese = App::new(test_options(false), &config);
    assert_eq!(
        english.tr(MessageId::ComposerPlaceholder),
        "Write a task or use /."
    );
    assert_eq!(
        chinese.tr(MessageId::ComposerPlaceholder),
        "编写任务或使用 /。"
    );
    assert_eq!(english.model, chinese.model);
    assert_eq!(english.workspace, chinese.workspace);
    assert_eq!(english.permission_mode, chinese.permission_mode);
}

#[test]
fn configured_model_preserves_explicit_reasoning_setting() {
    let _lock = lock_test_env();
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    let _config_path = EnvVarGuard::set("DSE_CONFIG_PATH", &config_path);
    let config = Config {
        reasoning_effort: Some("off".to_owned()),
        ..Config::default()
    };
    let mut options = test_options(false);
    options.model = "deepseek-v4-pro".to_owned();

    let app = App::new(options, &config);

    assert_eq!(app.model, "deepseek-v4-pro");
    assert_eq!(app.reasoning_effort, ReasoningEffort::Off);
}

#[test]
fn app_new_uses_only_the_explicit_cost_currency_setting() {
    let _lock = lock_test_env();
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    let settings_path = tmp.path().join("settings.toml");
    let _config_path = EnvVarGuard::set("DSE_CONFIG_PATH", &config_path);

    std::fs::write(&settings_path, "cost_currency = \"usd\"\n").expect("usd settings");
    let usd = App::new(test_options(false), &Config::default());
    assert_eq!(usd.cost_currency, CostCurrency::Usd);

    std::fs::write(&settings_path, "cost_currency = \"cny\"\n").expect("cny settings");
    let cny = App::new(test_options(false), &Config::default());
    assert_eq!(cny.cost_currency, CostCurrency::Cny);
}

#[test]
fn cny_display_falls_back_to_usd_for_usd_only_costs() {
    let mut app = App::new(test_options(false), &Config::default());
    app.cost_currency = CostCurrency::Cny;
    app.session.total_cost_usd = 0.42;

    let displayed = app.total_cost_for_currency(CostCurrency::Cny);

    assert_eq!(displayed, 0.42);
    assert_eq!(app.total_cost_for_currency(CostCurrency::Cny), 0.42);
    assert_eq!(
        app.cost_display_currency(CostCurrency::Cny),
        CostCurrency::Usd
    );
}

#[test]
fn cny_display_keeps_cny_when_costs_have_cny_rates() {
    let mut app = App::new(test_options(false), &Config::default());
    app.cost_currency = CostCurrency::Cny;
    app.session.total_cost_usd = 0.42;
    app.session.total_cost_cny = 2.5;

    let displayed = app.total_cost_for_currency(CostCurrency::Cny);

    assert_eq!(displayed, 2.5);
    assert_eq!(
        app.cost_display_currency(CostCurrency::Cny),
        CostCurrency::Cny
    );
}

#[test]
fn slash_command_classifier_treats_absolute_path_as_message() {
    use crate::tui::canonical_commands::looks_like_command_input;

    assert!(looks_like_command_input("/"));
    assert!(looks_like_command_input("/help"));
    assert!(looks_like_command_input("/model deepseek-v4-pro"));
    assert!(!looks_like_command_input("/ hello"));
    assert!(!looks_like_command_input("  / hello"));
    assert!(!looks_like_command_input(
        "/usr/lib/x86_64-linux-gnu/ 是标准路径吗？"
    ));
    assert!(!looks_like_command_input("$skill-name 处理任务"));
}

#[test]
fn submit_input_accepts_absolute_slash_path_as_message() {
    let mut app = App::new(test_options(false), &Config::default());
    let input = "/usr/lib/x86_64-linux-gnu/ 是标准路径吗？";
    app.input = input.to_string();
    app.cursor_position = input.chars().count();

    let submitted = app.submit_input().expect("expected submitted input");

    assert_eq!(submitted, input);
    assert!(app.input.is_empty());
}

#[test]
fn composer_strips_raw_sgr_mouse_report_when_mouse_capture_is_enabled() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    app.insert_str("[<35;44;18M");

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
}

#[test]
fn composer_strips_corrupted_mouse_report_burst() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("draft ");
    let leaked = "43;19M[<35;44;18M[<35;45;18M5;46;18M;48;18M";

    app.insert_str(leaked);

    assert_eq!(app.input, "draft ");
    assert_eq!(app.cursor_position, "draft ".chars().count());
}

#[test]
fn composer_preserves_draft_suffix_when_stripping_mouse_report() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("commit -m");

    app.insert_str("[<65;44;18M");

    assert_eq!(app.input, "commit -m");
    assert_eq!(app.cursor_position, "commit -m".chars().count());
}

#[test]
fn composer_preserves_numeric_draft_when_stripping_mouse_report() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("123");

    app.insert_str("[<65;44;18M");

    assert_eq!(app.input, "123");
    assert_eq!(app.cursor_position, 3);
}

#[test]
fn composer_strips_raw_sgr_mouse_report_when_mouse_capture_is_disabled() {
    let mut app = App::new(test_options(false), &Config::default());

    app.insert_str("[<35;44;18M");

    assert_eq!(app.input, "");
    assert_eq!(app.cursor_position, 0);
}

#[test]
fn composer_strips_tail_only_mouse_report_burst_when_mouse_capture_is_disabled() {
    let mut app = App::new(test_options(false), &Config::default());
    app.insert_str("draft ");

    app.insert_str(";76;20M35;74;22M35;73;23M");

    assert_eq!(app.input, "draft ");
    assert_eq!(app.cursor_position, "draft ".chars().count());
}

#[test]
fn composer_keeps_coordinate_like_text_when_mouse_capture_is_disabled() {
    let mut app = App::new(test_options(false), &Config::default());

    app.insert_str("Size 12;34M");

    assert_eq!(app.input, "Size 12;34M");
    assert_eq!(app.cursor_position, "Size 12;34M".chars().count());
}

#[test]
fn composer_keeps_normal_bracket_text_with_mouse_capture_enabled() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    app.insert_str("Use [<tag>] normally");

    assert_eq!(app.input, "Use [<tag>] normally");
}

#[test]
fn composer_keeps_coordinate_like_text_with_mouse_capture_enabled() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    app.insert_str("Size 12;34M");

    assert_eq!(app.input, "Size 12;34M");
}

// === Bug #1915: broader terminal control-sequence fragments leaking
// into the composer during dense streaming output. The narrow SGR
// mouse-report filter installed in e63a4ba4a covers `[<…M` style
// bursts, but not OSC 8 hyperlink fragments (`]8;;http…`) or Kitty
// keyboard protocol responses (`[?u`, `[>1u`). These can arrive when
// crossterm's event reader is mid-sequence and the unparsed tail is
// delivered as individual Char(c) keystrokes that land in the input.

#[test]
fn composer_strips_osc8_hyperlink_fragment() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("draft ");

    // OSC 8 prefix with URL body but no terminator delivered yet —
    // exactly what crossterm hands us if its event reader is
    // interrupted mid-sequence and the leading ESC is consumed by the
    // parser before the rest gets reclassified as Char(c).
    app.insert_str("]8;;https://example.com");

    assert_eq!(app.input, "draft ");
    assert_eq!(app.cursor_position, "draft ".chars().count());
}

#[test]
fn composer_strips_closing_osc8_fragment() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("hello ");

    // The closing wrapper `]8;;` (with a stray ST `\\` from a
    // chopped escape) can arrive on its own when the parser ate
    // the start of the sequence in a previous read but caught the
    // tail as keystrokes.
    app.insert_str("]8;;\\");

    assert_eq!(app.input, "hello ");
    assert_eq!(app.cursor_position, "hello ".chars().count());
}

#[test]
fn composer_strips_kitty_keyboard_protocol_fragment() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("ready ");

    // Kitty keyboard protocol responses look like `\x1b[?1u`,
    // `\x1b[>1u`, `\x1b[<1u`, or `\x1b[?u`. With the ESC consumed,
    // the tail shape is `[?…u`, `[>…u`, or `[<…u`.
    app.insert_str("[?1u[>1u[<1u[?u");

    assert_eq!(app.input, "ready ");
    assert_eq!(app.cursor_position, "ready ".chars().count());
}

#[test]
fn composer_strips_dec_private_mode_set_reset_fragments() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("ok ");

    // Regression for #2592: DEC private mode set/reset chatter ends in
    // `h`/`l`, not `u`, so the `u`-only terminator used to leak the
    // leading `[`. Bracketed paste, mouse capture, focus reporting, and
    // synchronized output all leak during dense streaming.
    app.insert_str("[?2004h[?2004l[?1000h[?1004h[?2026h[?25l");

    assert_eq!(app.input, "ok ");
    assert_eq!(app.cursor_position, "ok ".chars().count());
}

#[test]
fn composer_keeps_bracket_question_word_text() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    // The `h`/`l` terminator only counts after a numeric parameter, so
    // ordinary prose where a letter follows `[?` directly is preserved.
    app.insert_str("[?help] and [?later]");

    assert_eq!(app.input, "[?help] and [?later]");
}

#[test]
fn composer_strips_mixed_control_sequence_burst() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;
    app.insert_str("hi");

    // Mixed dense burst combining all three fragment families
    // described in #1915.
    app.insert_str("[<35;44;18M]8;;https://example.com[?1u");

    assert_eq!(app.input, "hi");
    assert_eq!(app.cursor_position, 2);
}

#[test]
fn composer_keeps_legitimate_url_text_with_mouse_capture_enabled() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    // URLs typed by the user must survive the filter — only
    // recognized control-sequence shapes are stripped.
    app.insert_str("see https://example.com/path?a=1&b=2 for info");

    assert_eq!(app.input, "see https://example.com/path?a=1&b=2 for info");
}

#[test]
fn composer_keeps_legitimate_bracket_question_text() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    // Text that uses brackets, question marks, and lowercase `u` —
    // shapes that overlap Kitty fragments — must not be eaten.
    app.insert_str("[is this ok?] sure");

    assert_eq!(app.input, "[is this ok?] sure");
}

#[test]
fn composer_keeps_legitimate_closing_bracket_digit_text() {
    let mut app = App::new(test_options(false), &Config::default());
    app.use_mouse_capture = true;

    // Plain `]8` followed by spaces and words must survive — only
    // the OSC 8 shape `]8;` (with the mandatory `;` separator)
    // should be treated as a fragment.
    app.insert_str("array[]8 elements");

    assert_eq!(app.input, "array[]8 elements");
}

// initial_onboarding_state tests
// These pin the logic that decides whether the TUI shows the
// onboarding flow (Welcome → Language → ApiKey → …) or goes
// straight to the chat view.  Getting this wrong either locks
// first-run users out of the API-key prompt or nags returning
// users whose key is already configured.

#[test]
fn skip_onboarding_suppresses_all_onboarding_states() {
    assert_eq!(
        initial_onboarding_state(true, false, true, true),
        OnboardingState::None
    );
    assert_eq!(
        initial_onboarding_state(true, true, true, true),
        OnboardingState::None
    );
}

#[test]
fn fully_configured_returning_user_skips_onboarding() {
    assert_eq!(
        initial_onboarding_state(false, true, false, false),
        OnboardingState::None
    );
}

#[test]
fn returning_user_missing_api_key_goes_to_api_key_screen() {
    assert_eq!(
        initial_onboarding_state(false, true, true, false),
        OnboardingState::ApiKey
    );
    // workspace trust doesn't affect the api-key gate
    assert_eq!(
        initial_onboarding_state(false, true, true, true),
        OnboardingState::ApiKey
    );
}

#[test]
fn first_run_user_always_starts_at_welcome() {
    assert_eq!(
        initial_onboarding_state(false, false, false, false),
        OnboardingState::Welcome
    );
    assert_eq!(
        initial_onboarding_state(false, false, true, false),
        OnboardingState::Welcome
    );
    assert_eq!(
        initial_onboarding_state(false, false, false, true),
        OnboardingState::Welcome
    );
}

#[test]
fn onboarding_workspace_trust_gate_only_fires_for_onboarded_user() {
    assert!(onboarding_is_workspace_trust_gate(false, true, false, true));
    assert!(!onboarding_is_workspace_trust_gate(true, true, false, true));
    assert!(!onboarding_is_workspace_trust_gate(false, true, true, true));
    assert!(!onboarding_is_workspace_trust_gate(
        false, false, false, true
    ));
}

#[test]
fn onboarded_user_still_gets_workspace_trust_prompt_when_needed() {
    assert_eq!(
        initial_onboarding_state(false, true, false, true),
        OnboardingState::TrustDirectory
    );
}

// App::new tests: missing key is detected

#[test]
fn app_new_detects_missing_api_key_with_default_config() {
    let _lock = lock_test_env();
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    let _config_path = EnvVarGuard::set("DSE_CONFIG_PATH", &config_path);
    let _provider_env = EnvVarGuard::remove("DSE_PROVIDER");
    let _legacy_provider_env = EnvVarGuard::remove("DEEPSEEK_PROVIDER");
    let _api_key_envs: Vec<_> = [
        "DEEPSEEK_API_KEY",
        "NVIDIA_API_KEY",
        "NVIDIA_NIM_API_KEY",
        "OPENAI_API_KEY",
        "ATLASCLOUD_API_KEY",
        "WANJIE_ARK_API_KEY",
        "WANJIE_API_KEY",
        "WANJIE_MAAS_API_KEY",
        "OPENROUTER_API_KEY",
        "NOVITA_API_KEY",
        "FIREWORKS_API_KEY",
        "SILICONFLOW_API_KEY",
        "MOONSHOT_API_KEY",
        "KIMI_API_KEY",
        "SGLANG_API_KEY",
        "VLLM_API_KEY",
        "OLLAMA_API_KEY",
    ]
    .into_iter()
    .map(EnvVarGuard::remove)
    .collect();

    // Config::default() carries no api_key, and this test isolates process
    // env/settings so previous tests or developer shells cannot satisfy it.
    let app = App::new(test_options(false), &Config::default());
    assert!(
        app.onboarding_needs_api_key,
        "default config (no key) must set onboarding_needs_api_key"
    );
}

#[test]
fn app_new_with_explicit_api_key_does_not_trigger_onboarding() {
    let _lock = lock_test_env();
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    let _config_path = EnvVarGuard::set("DSE_CONFIG_PATH", &config_path);
    let _provider_env = EnvVarGuard::remove("DSE_PROVIDER");
    let _legacy_provider_env = EnvVarGuard::remove("DEEPSEEK_PROVIDER");

    let config = Config {
        api_key: Some("sk-test-onboarding-key".to_string()),
        ..Config::default()
    };
    let app = App::new(test_options(false), &config);
    assert!(
        !app.onboarding_needs_api_key,
        "explicit config.api_key must satisfy the onboarding check"
    );
}

#[test]
fn new_caches_workspace_skills_for_slash_menu() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let skill_dir = workspace.join(".agents").join("skills").join("local-skill");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: local-skill\ndescription: Local workspace skill\n---\nUse the local skill.\n",
    )
    .expect("skill file");

    let mut options = test_options(false);
    options.workspace = workspace.clone();
    let global_skills_dir = tmp.path().join("global-skills");
    options.skills_dir = global_skills_dir.clone();
    let app = App::new(options, &Config::default());

    assert_eq!(
        resolve_skills_dir(&workspace, &global_skills_dir, &Config::default()),
        workspace.join(".agents").join("skills")
    );
    assert!(app.cached_skills.iter().any(|(name, description)| {
        name == "local-skill" && description == "Local workspace skill"
    }));
}

#[test]
fn cached_skills_merges_across_candidate_directories() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");

    // Higher-precedence directory contains a stale empty dir for `foo`
    // (no SKILL.md). This used to shadow the real definition further
    // down the candidate list when the cache only scanned a single dir.
    std::fs::create_dir_all(workspace.join(".agents").join("skills").join("foo"))
        .expect("stale empty dir");

    // Lower-precedence directory has the real skill.
    let real_dir = workspace.join(".claude").join("skills").join("foo");
    std::fs::create_dir_all(&real_dir).expect("real skill dir");
    std::fs::write(
        real_dir.join("SKILL.md"),
        "---\nname: foo\ndescription: Real foo skill\n---\nbody\n",
    )
    .expect("skill file");

    let mut options = test_options(false);
    options.workspace = workspace.clone();
    options.skills_dir = tmp.path().join("global-skills");
    let app = App::new(options, &Config::default());

    assert!(
        app.cached_skills
            .iter()
            .any(|(name, description)| name == "foo" && description == "Real foo skill"),
        "cached_skills should fall through to lower-precedence dir when higher-precedence one has an empty stub: {:?}",
        app.cached_skills,
    );
}

#[test]
fn cached_skills_respect_dse_only_scan_config() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");

    let claude_dir = workspace
        .join(".claude")
        .join("skills")
        .join("claude-skill");
    std::fs::create_dir_all(&claude_dir).expect("claude skill dir");
    std::fs::write(
        claude_dir.join("SKILL.md"),
        "---\nname: claude-skill\ndescription: Claude skill\n---\nbody\n",
    )
    .expect("write claude skill");

    let dse_dir = workspace.join(".dse").join("skills").join("dse-skill");
    std::fs::create_dir_all(&dse_dir).expect("dse skill dir");
    std::fs::write(
        dse_dir.join("SKILL.md"),
        "---\nname: dse-skill\ndescription: DSE skill\n---\nbody\n",
    )
    .expect("write dse skill");

    let mut options = test_options(false);
    options.workspace = workspace.clone();
    let global_skills_dir = tmp.path().join("global-skills");
    options.skills_dir = global_skills_dir.clone();
    let config = Config {
        skills: Some(crate::config::SkillsConfig {
            scan_dse_only: Some(true),
        }),
        ..Default::default()
    };
    let app = App::new(options, &config);

    assert_eq!(
        resolve_skills_dir(&workspace, &global_skills_dir, &config),
        workspace.join(".dse").join("skills")
    );
    assert!(
        app.cached_skills
            .iter()
            .any(|(name, _)| name == "dse-skill"),
        "DSE skill should be cached: {:?}",
        app.cached_skills
    );
    assert!(
        !app.cached_skills
            .iter()
            .any(|(name, _)| name == "claude-skill"),
        "strict scan should not cache Claude skills: {:?}",
        app.cached_skills
    );
}

#[test]
fn resolve_skills_dir_requires_dse_skills_to_be_directory() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(workspace.join(".dse")).expect("dse dir");
    std::fs::write(workspace.join(".dse").join("skills"), "not a directory").expect("skills file");

    let global_skills_dir = tmp.path().join("global-skills");
    let config = Config {
        skills: Some(crate::config::SkillsConfig {
            scan_dse_only: Some(true),
        }),
        ..Default::default()
    };

    let resolved = resolve_skills_dir(&workspace, &global_skills_dir, &config);

    assert_eq!(resolved, global_skills_dir);
}

#[test]
fn cached_skills_include_configured_directory() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");

    let configured_dir = tmp.path().join("configured-skills");
    let configured_skill_dir = configured_dir.join("configured-skill");
    std::fs::create_dir_all(&configured_skill_dir).expect("configured skill dir");
    std::fs::write(
        configured_skill_dir.join("SKILL.md"),
        "---\nname: configured-skill\ndescription: Configured skill\n---\nbody\n",
    )
    .expect("write configured skill");

    let mut options = test_options(false);
    options.workspace = workspace.clone();
    options.skills_dir = configured_dir.clone();
    let config = Config {
        skills_dir: Some(configured_dir.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let app = App::new(options, &config);

    assert!(
        app.cached_skills
            .iter()
            .any(|(name, description)| name == "configured-skill"
                && description == "Configured skill"),
        "configured skill dir should be merged: {:?}",
        app.cached_skills
    );
}

#[test]
fn cached_skills_preserve_configured_directory_in_dse_only_scan() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");

    let dse_skill_dir = workspace.join(".dse").join("skills").join("workspace-dse");
    std::fs::create_dir_all(&dse_skill_dir).expect("workspace dse skill dir");
    std::fs::write(
        dse_skill_dir.join("SKILL.md"),
        "---\nname: workspace-dse\ndescription: Workspace DSE skill\n---\nbody\n",
    )
    .expect("write workspace dse skill");

    let configured_dir = tmp.path().join("configured-skills");
    let configured_skill_dir = configured_dir.join("configured-skill");
    std::fs::create_dir_all(&configured_skill_dir).expect("configured skill dir");
    std::fs::write(
        configured_skill_dir.join("SKILL.md"),
        "---\nname: configured-skill\ndescription: Configured skill\n---\nbody\n",
    )
    .expect("write configured skill");

    let mut options = test_options(false);
    options.workspace = workspace.clone();
    options.skills_dir = configured_dir.clone();
    let config = Config {
        skills_dir: Some(configured_dir.to_string_lossy().into_owned()),
        skills: Some(crate::config::SkillsConfig {
            scan_dse_only: Some(true),
        }),
        ..Default::default()
    };
    let app = App::new(options, &config);

    assert_eq!(
        resolve_skills_dir(&workspace, &configured_dir, &config),
        configured_dir
    );
    assert!(
        app.cached_skills
            .iter()
            .any(|(name, _)| name == "workspace-dse"),
        "workspace DSE skill should still be cached: {:?}",
        app.cached_skills
    );
    assert!(
        app.cached_skills
            .iter()
            .any(|(name, _)| name == "configured-skill"),
        "explicit configured skills_dir should still be cached: {:?}",
        app.cached_skills
    );
}

#[test]
fn cached_skills_reject_dse_only_workspace_symlink_escape() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let escape_target = tmp.path().join("escape-target");
    let escaped_skill_dir = escape_target.join("escaped-skill");
    std::fs::create_dir_all(workspace.join(".dse")).expect("dse dir");
    std::fs::create_dir_all(&escaped_skill_dir).expect("escaped skill dir");
    std::fs::write(
        escaped_skill_dir.join("SKILL.md"),
        "---\nname: escaped-skill\ndescription: Escaped skill\n---\nbody\n",
    )
    .expect("write escaped skill");

    let link_path = workspace.join(".dse").join("skills");
    if create_dir_symlink(&escape_target, &link_path).is_err() {
        return;
    }

    let global_skills_dir = tmp.path().join("global-skills");
    let mut options = test_options(false);
    options.workspace = workspace.clone();
    options.skills_dir = global_skills_dir.clone();
    let config = Config {
        skills: Some(crate::config::SkillsConfig {
            scan_dse_only: Some(true),
        }),
        ..Default::default()
    };
    let app = App::new(options, &config);

    assert_eq!(
        resolve_skills_dir(&workspace, &global_skills_dir, &config),
        global_skills_dir
    );
    assert!(
        !app.cached_skills
            .iter()
            .any(|(name, _)| name == "escaped-skill"),
        "strict app cache must not follow escaped workspace DSE symlinks: {:?}",
        app.cached_skills
    );
}

#[test]
fn paste_defers_oversized_text_consolidation_until_submit() {
    // (#3263): a large paste stays inline so the user can still edit it.
    // At submit time, the full text is sent to the model with the @mention
    // appended so the model can also read the paste file backup.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut opts = test_options(false);
    opts.workspace = tmp.path().to_path_buf();
    let mut app = App::new(opts, &Config::default());
    let full_content = "y".repeat(MAX_SUBMITTED_INPUT_CHARS + 256);

    app.insert_paste_text(&full_content);

    assert_eq!(app.input, full_content);
    assert_eq!(app.cursor_position, app.input.chars().count());
    let pastes_dir = tmp.path().join(".dse/pastes");
    assert!(
        !pastes_dir.exists() || std::fs::read_dir(&pastes_dir).unwrap().next().is_none(),
        "paste file should not be written before submit"
    );
    assert!(
        app.status_toasts
            .iter()
            .all(|toast| !toast.text.contains("backed up")),
        "backup toast should not appear before submit"
    );

    let submitted = app.submit_input().expect("expected submitted input");
    // The submitted text should contain the original content with the
    // @mention appended at the end (#3263).
    assert!(
        submitted.starts_with(&full_content),
        "submitted should contain full content, got: {}",
        &submitted[..submitted.len().min(80)]
    );
    let mention_start = full_content.len();
    assert!(
        submitted[mention_start..].starts_with("\n@.dse/pastes/paste-"),
        "expected @mention suffix, got: {}",
        &submitted[mention_start..]
    );
    assert!(submitted.ends_with(".md"), "expected .md extension");
    let mention = &submitted[mention_start + 2..]; // strip '\n@'
    let abs = tmp.path().join(mention);
    assert!(abs.is_file(), "paste file must exist at {abs:?}");
    let written = std::fs::read_to_string(&abs).expect("read");
    assert_eq!(written, full_content);
    assert!(
        app.status_toasts
            .iter()
            .any(|toast| toast.text.contains("backed up")),
        "expected backup toast after submit"
    );
}

#[test]
fn paste_under_threshold_does_not_consolidate() {
    // Negative path: a small paste must NOT spawn a paste file. The
    // input stays inline so the user can edit it freely.
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut opts = test_options(false);
    opts.workspace = tmp.path().to_path_buf();
    let mut app = App::new(opts, &Config::default());
    let small = "hello world\nthis is fine".to_string();

    app.insert_paste_text(&small);

    assert_eq!(app.input, small);
    assert!(!app.input.starts_with("@.dse/pastes/"));
    // No paste file gets written for under-cap pastes.
    let pastes_dir = tmp.path().join(".dse/pastes");
    assert!(
        !pastes_dir.exists() || std::fs::read_dir(&pastes_dir).unwrap().next().is_none(),
        "no paste file should be written for under-cap content"
    );
}

#[test]
fn submit_input_consolidates_oversized_input_into_paste_file() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut opts = test_options(false);
    opts.workspace = tmp.path().to_path_buf();
    let mut app = App::new(opts, &Config::default());
    let full_content = "x".repeat(MAX_SUBMITTED_INPUT_CHARS + 128);
    app.input = full_content.clone();
    app.cursor_position = app.input.chars().count();

    let submitted = app.submit_input().expect("expected submitted input");

    // The submitted text should still contain the original content, with
    // the @mention appended at the end so the model can read the file
    // while the composer stays editable for the user (#3263).
    assert!(
        submitted.starts_with(&full_content),
        "submitted text should contain original content, got: {}",
        &submitted[..submitted.len().min(80)]
    );
    let mention_start = full_content.len();
    assert!(
        submitted[mention_start..].starts_with("\n@.dse/pastes/paste-"),
        "submitted text should end with @mention, got suffix: {}",
        &submitted[mention_start..]
    );
    assert!(
        submitted.ends_with(".md"),
        "expected .md extension, got: {submitted}"
    );

    // The paste file must exist on disk with the full original content.
    let mention = &submitted[mention_start + 2..]; // strip leading '\n@'
    let abs_path = tmp.path().join(mention);
    assert!(abs_path.is_file(), "paste file must exist at {abs_path:?}");
    let written = std::fs::read_to_string(&abs_path).expect("read paste file");
    assert_eq!(written, full_content);

    // A status toast should have been pushed.
    assert!(
        app.status_toasts
            .iter()
            .any(|toast| toast.text.contains("backed up")),
        "expected backup toast, got: {:?}",
        app.status_toasts
            .iter()
            .map(|t| &t.text)
            .collect::<Vec<_>>()
    );

    // The composer must be clear after submit.
    assert!(app.input.is_empty());
}

#[test]
fn app_starts_without_seeded_transcript_messages() {
    let app = App::new(test_options(false), &Config::default());
    assert!(app.history.is_empty());
}

#[test]
fn test_clear_input() {
    let mut app = App::new(test_options(false), &Config::default());
    app.input = "test input".to_string();
    app.cursor_position = app.input.len();
    app.pending_paste_reference = Some("@.dse/pastes/input.md".to_string());
    app.oversized_paste_full_text = Some("full input".to_string());
    app.clear_input();
    assert!(app.input.is_empty());
    assert_eq!(app.cursor_position, 0);
    assert!(app.pending_paste_reference.is_none());
    assert!(app.oversized_paste_full_text.is_none());
}

#[test]
fn app_new_respects_allow_shell_option_when_not_yolo() {
    let mut options = test_options(false);
    options.allow_shell = false;
    let app = App::new(options, &Config::default());
    assert!(!app.allow_shell);
}

#[test]
fn obsolete_default_mode_yolo_cannot_grant_authority() {
    let _env_lock = lock_test_env();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.toml");
    std::fs::write(
        tmp.path().join("settings.toml"),
        "default_mode = \"yolo\"\n",
    )
    .expect("obsolete settings fixture");
    let _config_env = EnvVarGuard::set("DSE_CONFIG_PATH", &config_path);
    let mut options = test_options(false);
    options.config_path = Some(config_path);

    let app = App::new(options, &Config::default());

    assert!(!app.allow_shell);
    assert!(!app.workspace_trust_accepted);
    assert_eq!(app.permission_mode, RunPermissionMode::Ask);
}

#[test]
fn default_permission_mode_requests_approval() {
    let app = App::new(test_options(false), &Config::default());
    assert_eq!(app.permission_mode, RunPermissionMode::Ask);
}

#[test]
fn yolo_is_the_only_startup_full_access_override() {
    let app = App::new(test_options(true), &Config::default());
    assert_eq!(app.permission_mode, RunPermissionMode::FullAccess);
}

#[test]
fn test_scroll_operations() {
    let mut app = App::new(test_options(false), &Config::default());
    // Just verify scroll methods can be called without panic
    app.scroll_up(5);
    app.scroll_down(3);
}

#[test]
fn test_add_message() {
    let mut app = App::new(test_options(false), &Config::default());
    let initial_len = app.history.len();
    app.add_message(HistoryCell::User {
        content: "test".to_string(),
    });
    assert_eq!(app.history.len(), initial_len + 1);
}

#[test]
fn composer_paste_normalizes_crlf_and_bare_carriage_returns() {
    let mut app = App::new(test_options(false), &Config::default());
    app.insert_paste_text("a\r\nb\rc");

    assert_eq!(app.input, "a\nb\nc");
    assert_eq!(app.cursor_position, "a\nb\nc".chars().count());
}

#[test]
fn bracketed_paste_preserves_bare_carriage_return_line_breaks() {
    let mut app = App::new(test_options(false), &Config::default());

    app.insert_paste_text("alpha\r  indented\r# literal heading\r- literal list");

    assert_eq!(
        app.input,
        "alpha\n  indented\n# literal heading\n- literal list"
    );
    assert_eq!(app.cursor_position, app.input.chars().count());
}

#[test]
fn composer_enter_submits_normally() {
    let mut app = App::new(test_options(false), &Config::default());
    app.input = "hello world".to_string();
    app.cursor_position = "hello world".chars().count();

    let result = app.handle_composer_enter();

    assert_eq!(
        result.as_deref(),
        Some("hello world"),
        "ordinary Enter must submit the composer"
    );
    assert!(
        app.input.is_empty(),
        "submit_input should clear the composer"
    );
}

#[test]
fn delete_word_backward_removes_previous_word_only() {
    let mut app = App::new(test_options(false), &Config::default());
    app.input = "hello world".to_string();
    app.cursor_position = char_count(&app.input);

    app.delete_word_backward();

    assert_eq!(app.input, "hello ");
    assert_eq!(app.cursor_position, char_count("hello "));
}

#[test]
fn delete_word_backward_handles_trailing_space_and_utf8() {
    let mut app = App::new(test_options(false), &Config::default());
    app.input = "cafe 你好   ".to_string();
    app.cursor_position = char_count(&app.input);

    app.delete_word_backward();

    assert_eq!(app.input, "cafe ");
    assert_eq!(app.cursor_position, char_count("cafe "));
}

#[test]
fn status_classifier_does_not_paint_negated_success_green() {
    use super::StatusToastLevel;
    // Failures that happen to contain a success keyword ("saved", "found")
    // must not toast green (#3757 UX review).
    let (level, _, _) = App::classify_status_text("Custom provider was not saved.");
    assert_ne!(level, StatusToastLevel::Success);
    let (level, _, _) = App::classify_status_text("Queued message not found");
    assert_ne!(level, StatusToastLevel::Success);
    let (level, _, _) = App::classify_status_text("Could not enable subagents");
    assert_ne!(level, StatusToastLevel::Success);
    let (level, _, _) = App::classify_status_text("No sessions found");
    assert_ne!(level, StatusToastLevel::Success);

    // Genuine successes still classify green.
    let (level, _, _) = App::classify_status_text("Agent task saved: reviewer.toml");
    assert_eq!(level, StatusToastLevel::Success);

    // Both cancel spellings classify as Warning.
    let (level, _, _) = App::classify_status_text("Turn canceled");
    assert_eq!(level, StatusToastLevel::Warning);
    let (level, _, _) = App::classify_status_text("Turn cancelled");
    assert_eq!(level, StatusToastLevel::Warning);
}
