use std::fs;

use anyhow::Result;

use super::*;

#[test]
fn m8a_default_is_official_deepseek() {
    let config = Config::default();
    assert_eq!(DEEPSEEK_PROVIDER_ID, "deepseek");
    assert_eq!(config.default_model(), "deepseek-v4-pro");
    assert_eq!(config.deepseek_base_url(), "https://api.deepseek.com");
}

#[test]
fn m8h_model_names_delegate_to_the_current_official_catalog() {
    assert_eq!(
        normalize_model_name("pro").as_deref(),
        Some("deepseek-v4-pro")
    );
    assert_eq!(
        normalize_model_name("flash").as_deref(),
        Some("deepseek-v4-flash")
    );
    for unsupported in [
        "auto",
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-future",
        "gpt-5",
    ] {
        assert!(normalize_model_name(unsupported).is_none());
    }
}

#[test]
fn auto_model_and_reasoning_are_rejected_at_the_config_boundary() {
    let model = Config {
        default_text_model: Some("auto".to_owned()),
        ..Config::default()
    };
    assert!(model.validate().is_err());

    let reasoning = Config {
        reasoning_effort: Some("auto".to_owned()),
        ..Config::default()
    };
    assert!(reasoning.validate().is_err());
}

#[test]
fn retired_m10a_project_context_pack_setting_is_rejected() {
    let parsed: ConfigFile = toml::from_str("[context]\nproject_pack = false\n").unwrap();
    let config = apply_profile(parsed, None).unwrap();
    let error = config
        .validate()
        .expect_err("retired pack-off treatment must fail closed");
    assert!(error.to_string().contains("context.project_pack"));
}

#[test]
fn m10c_acceptance_progress_treatment_is_typed_and_defaults_off() {
    assert!(!Config::default().acceptance_progress_enabled());
    let parsed: ConfigFile = toml::from_str("[context]\nacceptance_progress = true\n").unwrap();
    let config = apply_profile(parsed, None).unwrap();
    config.validate().unwrap();
    assert!(config.acceptance_progress_enabled());
}

#[test]
fn m8a_retired_provider_key_is_rejected_even_when_named_deepseek() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    fs::write(&path, "provider = \"deepseek\"\n")?;
    let error = Config::load(Some(path), None).expect_err("provider key must fail");
    assert!(format!("{error:#}").contains("provider"));
    Ok(())
}

#[test]
fn m8a_foreign_provider_table_is_rejected_during_parse() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    fs::write(
        &path,
        r#"
[providers.openai]
api_key = "not-a-real-key"
"#,
    )?;
    let error = Config::load(Some(path), None).expect_err("foreign table must fail");
    assert!(error.to_string().contains("解析配置失败"));
    Ok(())
}

#[test]
fn m8a_root_values_are_the_only_model_configuration() {
    let config = Config {
        base_url: Some("https://api.deepseek.com/v1".to_string()),
        default_text_model: Some("pro".to_string()),
        ..Config::default()
    };
    assert_eq!(config.deepseek_base_url(), "https://api.deepseek.com");
    assert_eq!(config.default_model(), "deepseek-v4-pro");
}

#[test]
fn m8a_first_start_template_has_no_secret_or_provider_mode() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("config.toml");
    assert_eq!(
        ensure_config_file_exists(Some(path.clone()))?,
        Some(path.clone())
    );
    let raw = fs::read_to_string(path)?;
    assert!(raw.contains("codewhale auth set"));
    assert!(raw.contains("deepseek-v4-pro"));
    assert!(!raw.contains("api_key ="));
    assert!(!raw.contains("--provider"));
    Ok(())
}

#[test]
fn m8a_workspace_trust_round_trips_without_provider_state() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let config_path = temp.path().join("config.toml");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace)?;
    save_workspace_trust_at(Some(&config_path), &workspace)?;
    assert!(is_workspace_trusted_at(Some(&config_path), &workspace));
    let raw = fs::read_to_string(config_path)?;
    assert!(!raw.contains("providers"));
    Ok(())
}

#[test]
fn m8a_capability_comes_from_deepseek_owner() {
    let capability = deepseek_capability("deepseek-v4-flash");
    assert_eq!(capability.context_window, 1_000_000);
    assert_eq!(capability.max_output, 384_000);
    assert!(capability.thinking_supported);
    assert!(capability.cache_telemetry_supported);
}

#[test]
fn m8a_subagent_limits_have_one_provider_independent_owner() {
    let config = Config {
        subagents: Some(SubagentsConfig {
            enabled: Some(true),
            max_concurrent: Some(3),
            max_depth: Some(2),
        }),
        ..Config::default()
    };
    assert_eq!(config.max_subagents(), 3);
    assert_eq!(config.subagent_max_spawn_depth(), 2);
}

#[test]
fn m8a_shipped_example_loads_through_the_interactive_entry() {
    let parsed: ConfigFile =
        toml::from_str(include_str!("../../../../config.example.toml")).unwrap();
    let config = apply_profile(parsed, None).unwrap();
    config.validate().unwrap();
    assert_eq!(config.default_model(), "deepseek-v4-pro");
    assert!(!config.extra.contains_key("fleet"));
}

#[test]
fn m8g_interactive_config_rejects_retired_fleet_table() {
    let parsed: ConfigFile = toml::from_str("[fleet]\nmax_workers = 4\n").unwrap();
    let config = apply_profile(parsed, None).unwrap();
    let error = config.validate().expect_err("fleet table must be retired");
    assert!(error.to_string().contains("fleet"));
}
