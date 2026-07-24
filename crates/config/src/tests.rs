use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use codewhale_secrets::{InMemoryKeyringStore, KeyringStore};
use tempfile::tempdir;

use super::*;

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

struct ScopedEnv {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl ScopedEnv {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        // SAFETY: configuration tests serialize environment mutation with
        // `env_lock`, and the guard outlives this scoped value.
        unsafe { std::env::set_var(name, value) };
        Self { name, previous }
    }

    fn remove(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        // SAFETY: see `set`.
        unsafe { std::env::remove_var(name) };
        Self { name, previous }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            if let Some(value) = self.previous.take() {
                std::env::set_var(self.name, value);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }
}

fn in_memory_secrets() -> (Arc<InMemoryKeyringStore>, Secrets) {
    let store = Arc::new(InMemoryKeyringStore::new());
    let secrets = Secrets::new(store.clone());
    (store, secrets)
}

#[test]
fn m8a_default_runtime_is_official_deepseek() {
    let _lock = env_lock();
    let _provider = ScopedEnv::remove("CODEWHALE_PROVIDER");
    let _legacy_provider = ScopedEnv::remove("DEEPSEEK_PROVIDER");
    let _model = ScopedEnv::remove("CODEWHALE_MODEL");
    let _legacy_model = ScopedEnv::remove("DEEPSEEK_MODEL");
    let _base = ScopedEnv::remove("CODEWHALE_BASE_URL");
    let _legacy_base = ScopedEnv::remove("DEEPSEEK_BASE_URL");
    let _key = ScopedEnv::remove("DEEPSEEK_API_KEY");

    let resolved = ConfigToml::default()
        .resolve_runtime_options(&CliRuntimeOverrides::default())
        .unwrap();
    assert_eq!(resolved.model, DEFAULT_DEEPSEEK_MODEL);
    assert_eq!(resolved.base_url, DEFAULT_DEEPSEEK_BASE_URL);
    assert_eq!(resolved.api_key, None);
}

#[test]
fn m8a_model_ids_preserve_future_deepseek_names_and_reject_foreign_names() {
    assert_eq!(canonical_deepseek_model("pro").unwrap(), "deepseek-v4-pro");
    assert_eq!(
        canonical_deepseek_model("deepseek-chat").unwrap(),
        "deepseek-v4-flash"
    );
    assert_eq!(
        canonical_deepseek_model("deepseek-future").unwrap(),
        "deepseek-future"
    );
    assert!(canonical_deepseek_model("gpt-5").is_err());
    assert!(canonical_deepseek_model("claude-sonnet").is_err());
}

#[test]
fn m8a_foreign_and_compat_provider_config_are_rejected() {
    for raw in [
        "provider = \"openai\"\n",
        "provider = \"deepseek\"\n",
        "[providers.deepseek]\napi_key = \"secret\"\n",
        "fallback_providers = [\"deepseek\"]\n",
        "model = \"deepseek-v4-pro\"\n",
        "http_headers = { x = \"y\" }\n",
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, raw).unwrap();
        let error = ConfigStore::load(Some(path)).expect_err("retired surface must fail");
        assert!(error.to_string().contains("配置不兼容"));
    }
}

#[test]
fn m8a_config_save_has_one_root_model_owner() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let mut store = ConfigStore::load(Some(path.clone())).unwrap();
    store.config.api_key = Some("test-secret".to_string());
    store.config.base_url = Some(DEFAULT_DEEPSEEK_BASE_URL.to_string());
    store.config.default_text_model = Some("deepseek-v4-pro".to_string());
    store.save().unwrap();

    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.contains("api_key = \"test-secret\""));
    assert!(raw.contains("default_text_model = \"deepseek-v4-pro\""));
    assert!(!raw.contains("provider"));
    assert!(!raw.contains("[providers"));
}

#[test]
fn m8a_credential_precedence_is_cli_config_keyring_env() {
    let _lock = env_lock();
    let _provider = ScopedEnv::remove("CODEWHALE_PROVIDER");
    let _legacy_provider = ScopedEnv::remove("DEEPSEEK_PROVIDER");
    let _env_key = ScopedEnv::set("DEEPSEEK_API_KEY", "env-key");
    let (keyring, secrets) = in_memory_secrets();
    keyring.set("deepseek", "keyring-key").unwrap();

    let mut config = ConfigToml {
        api_key: Some("config-key".to_string()),
        ..ConfigToml::default()
    };
    let cli = CliRuntimeOverrides {
        api_key: Some("cli-key".to_string()),
        ..CliRuntimeOverrides::default()
    };
    let resolved = config
        .resolve_runtime_options_with_secrets(&cli, &secrets)
        .unwrap();
    assert_eq!(resolved.api_key.as_deref(), Some("cli-key"));
    assert_eq!(resolved.api_key_source, Some(RuntimeApiKeySource::Cli));

    let resolved = config
        .resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets)
        .unwrap();
    assert_eq!(resolved.api_key.as_deref(), Some("config-key"));
    assert_eq!(
        resolved.api_key_source,
        Some(RuntimeApiKeySource::ConfigFile)
    );

    config.api_key = None;
    let resolved = config
        .resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets)
        .unwrap();
    assert_eq!(resolved.api_key.as_deref(), Some("keyring-key"));
    assert_eq!(resolved.api_key_source, Some(RuntimeApiKeySource::Keyring));

    keyring.delete("deepseek").unwrap();
    let resolved = config
        .resolve_runtime_options_with_secrets(&CliRuntimeOverrides::default(), &secrets)
        .unwrap();
    assert_eq!(resolved.api_key.as_deref(), Some("env-key"));
    assert_eq!(resolved.api_key_source, Some(RuntimeApiKeySource::Env));
}

#[test]
fn m8a_any_retired_provider_environment_selector_fails_closed() {
    let _lock = env_lock();
    let _provider = ScopedEnv::set("CODEWHALE_PROVIDER", "deepseek");
    let error = ConfigToml::default()
        .resolve_runtime_options(&CliRuntimeOverrides::default())
        .expect_err("provider selector must not survive cutover");
    assert!(error.to_string().contains("CODEWHALE_PROVIDER"));
}

#[test]
fn m8a_endpoint_accepts_official_or_loopback_only() {
    for accepted in [
        "https://api.deepseek.com",
        "https://api.deepseek.com/v1/",
        "http://127.0.0.1:4567/v1",
        "http://localhost:4567",
        "http://[::1]:4567",
    ] {
        validate_deepseek_base_url(accepted).unwrap();
    }
    assert!(validate_deepseek_base_url("https://api.openai.com/v1").is_err());
    assert!(validate_deepseek_base_url("http://192.0.2.1:4567").is_err());
}

#[test]
fn m8a_project_config_cannot_change_model_authority() {
    let directory = tempdir().unwrap();
    let project_dir = directory.path().join(".codewhale");
    std::fs::create_dir_all(&project_dir).unwrap();
    for (key, value) in [
        ("api_key", "\"secret\""),
        ("base_url", "\"http://127.0.0.1:1\""),
        ("default_text_model", "\"deepseek-v4-flash\""),
        ("provider", "\"deepseek\""),
    ] {
        std::fs::write(
            project_dir.join("config.toml"),
            format!("{key} = {value}\n"),
        )
        .unwrap();
        assert!(load_project_config(directory.path()).is_none());
    }
}

#[test]
fn m8a_project_merge_only_applies_non_model_policy() {
    let mut global = ConfigToml {
        api_key: Some("global-key".to_string()),
        base_url: Some(DEFAULT_DEEPSEEK_BASE_URL.to_string()),
        default_text_model: Some("deepseek-v4-pro".to_string()),
        approval_policy: Some("auto".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        ..ConfigToml::default()
    };
    let project = ConfigToml {
        api_key: Some("project-key".to_string()),
        base_url: Some("http://127.0.0.1:1".to_string()),
        default_text_model: Some("deepseek-v4-flash".to_string()),
        approval_policy: Some("on-request".to_string()),
        sandbox_mode: Some("read-only".to_string()),
        ..ConfigToml::default()
    };
    global.merge_project_overrides(project);
    assert_eq!(global.api_key.as_deref(), Some("global-key"));
    assert_eq!(
        global.default_text_model.as_deref(),
        Some("deepseek-v4-pro")
    );
    assert_eq!(global.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(global.sandbox_mode.as_deref(), Some("read-only"));
}

#[test]
fn m8a_config_commands_reject_retired_keys_and_redact_secrets() {
    let mut config = ConfigToml::default();
    assert!(config.set_value("provider", "deepseek").is_err());
    assert!(
        config
            .set_value("providers.deepseek.api_key", "secret")
            .is_err()
    );
    config
        .set_value("api_key", "sk-1234567890-abcdefgh")
        .unwrap();
    assert_eq!(
        config.get_display_value("api_key").as_deref(),
        Some("sk-1***efgh")
    );
    assert!(!config.list_values()["api_key"].contains("1234567890"));
}

#[test]
fn m8a_shipped_example_matches_the_deepseek_only_schema() {
    let config: ConfigToml = toml::from_str(include_str!("../../../config.example.toml")).unwrap();
    config.validate().unwrap();
    assert_eq!(
        config.default_text_model.as_deref(),
        Some("deepseek-v4-pro")
    );
    assert!(!config.extras.contains_key("fleet"));
    assert!(!config.extras.contains_key("provider"));
    assert!(!config.extras.contains_key("providers"));
}

#[test]
fn m8g_rejects_retired_fleet_configuration() {
    let config: ConfigToml = toml::from_str("[fleet]\nmax_workers = 4\n").unwrap();
    let error = config.validate().expect_err("fleet config must be retired");
    assert!(error.to_string().contains("fleet"));
}

#[test]
fn comments_survive_a_deepseek_only_config_update() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        "# personal note\napi_key = \"old-secret\" # keep this\n",
    )
    .unwrap();
    let mut store = ConfigStore::load(Some(path.clone())).unwrap();
    store.config.api_key = Some("new-secret".to_string());
    store.save().unwrap();
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.contains("# personal note"));
    assert!(raw.contains("# keep this"));
    assert!(raw.contains("new-secret"));
}

#[test]
fn config_and_state_paths_reject_parent_escape() {
    assert!(resolve_config_path(Some("../config.toml".into())).is_err());
    assert!(ensure_state_dir("../escape").is_err());
}
