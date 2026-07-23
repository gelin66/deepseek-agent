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
fn m8a_model_aliases_are_bounded_to_official_ids() {
    assert_eq!(
        normalize_model_name("pro").as_deref(),
        Some("deepseek-v4-pro")
    );
    assert_eq!(
        normalize_model_name("deepseek-chat").as_deref(),
        Some("deepseek-v4-flash")
    );
    assert_eq!(normalize_model_name("auto").as_deref(), Some("auto"));
    assert!(normalize_model_name("gpt-5").is_none());
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
