use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use codewhale_secrets::SecretSource;
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::fleet::{FleetConfigToml, ToolsToml};
use crate::paths::{
    checked_path_exists, normalize_config_file_path, normalize_project_workspace,
    read_checked_config_file, reject_path_symlink, resolve_config_path,
};
use crate::{CONFIG_FILE_NAME, Secrets, persistence};

pub const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-pro";
pub const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

const RETIRED_ROOT_KEYS: &[&str] = &[
    "apiKey",
    "baseUrl",
    "defaultTextModel",
    "provider",
    "providers",
    "fallback_providers",
    "fallbackProviders",
    "model",
    "auth",
    "auth_mode",
    "authMode",
    "http_headers",
    "httpHeaders",
    "insecure_skip_tls_verify",
    "insecureSkipTlsVerify",
    "path_suffix",
    "pathSuffix",
    "harness_profiles",
    "model_catalog",
    "models",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigToml {
    /// The only persisted model credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// The official DeepSeek endpoint, or an explicit loopback fixture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The logical DeepSeek model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_text_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsToml>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetConfigToml>,
    /// Non-model TUI and local-development settings keep their existing owner.
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

impl ConfigToml {
    pub fn validate(&self) -> Result<()> {
        reject_retired_extra_keys(&self.extras)?;
        if let Some(model) = self.default_text_model.as_deref() {
            canonical_deepseek_model(model)?;
        }
        if let Some(base_url) = self.base_url.as_deref() {
            validate_deepseek_base_url(base_url)?;
        }
        if self
            .api_key
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("DeepSeek API Key 不能为空字符串");
        }
        Ok(())
    }

    /// Repo-local config is untrusted. It may tighten execution posture and
    /// change non-secret local behavior, but it cannot replace the model,
    /// credential, endpoint, or telemetry owner.
    pub fn merge_project_overrides(&mut self, project: ConfigToml) {
        if project.output_mode.is_some() {
            self.output_mode = project.output_mode;
        }
        if project.verbosity.is_some() {
            self.verbosity = project.verbosity;
        }
        if project.log_level.is_some() {
            self.log_level = project.log_level;
        }
        if let Some(policy) = project.approval_policy
            && project_approval_policy_is_allowed(self.approval_policy.as_deref(), &policy)
        {
            self.approval_policy = Some(policy);
        }
        if let Some(mode) = project.sandbox_mode
            && project_sandbox_mode_is_allowed(self.sandbox_mode.as_deref(), &mode)
        {
            self.sandbox_mode = Some(mode);
        }
        if project.tools.is_some() {
            self.tools = project.tools;
        }
        if project.fleet.is_some() {
            self.fleet = project.fleet;
        }
    }

    #[must_use]
    pub fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "api_key" => self.api_key.clone(),
            "base_url" => self.base_url.clone(),
            "default_text_model" => self.default_text_model.clone(),
            "output_mode" => self.output_mode.clone(),
            "verbosity" => self.verbosity.clone(),
            "log_level" => self.log_level.clone(),
            "telemetry" => self.telemetry.map(|value| value.to_string()),
            "approval_policy" => self.approval_policy.clone(),
            "sandbox_mode" => self.sandbox_mode.clone(),
            "tools.always_load" => self.tools.as_ref().map(|tools| tools.always_load.join(",")),
            "stream_chunk_timeout_secs" | "tui.stream_chunk_timeout_secs" => {
                Some(self.stream_chunk_timeout_secs().to_string())
            }
            _ => self.extras.get(key).map(toml::Value::to_string),
        }
    }

    #[must_use]
    pub fn get_display_value(&self, key: &str) -> Option<String> {
        if let Some(value) = self.extras.get(key) {
            return Some(redact_toml_value_for_display(key, value));
        }
        self.get_value(key).map(|value| {
            if is_sensitive_config_key(key) {
                redact_secret(&value)
            } else {
                value
            }
        })
    }

    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        reject_retired_config_key(key)?;
        match key {
            "api_key" => {
                if value.trim().is_empty() {
                    bail!("DeepSeek API Key 不能为空字符串");
                }
                self.api_key = Some(value.to_string());
            }
            "base_url" => {
                validate_deepseek_base_url(value)?;
                self.base_url = Some(value.trim_end_matches('/').to_string());
            }
            "default_text_model" => {
                self.default_text_model = Some(canonical_deepseek_model(value)?);
            }
            "output_mode" => self.output_mode = Some(value.to_string()),
            "verbosity" => self.verbosity = Some(value.to_string()),
            "log_level" => self.log_level = Some(value.to_string()),
            "telemetry" => self.telemetry = Some(parse_bool(value)?),
            "approval_policy" => self.approval_policy = Some(value.to_string()),
            "sandbox_mode" => self.sandbox_mode = Some(value.to_string()),
            "tools.always_load" => {
                let values = value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
                self.tools
                    .get_or_insert_with(ToolsToml::default)
                    .always_load = values;
            }
            _ => {
                self.extras
                    .insert(key.to_string(), toml::Value::String(value.to_string()));
            }
        }
        Ok(())
    }

    pub fn unset_value(&mut self, key: &str) -> Result<()> {
        reject_retired_config_key(key)?;
        match key {
            "api_key" => self.api_key = None,
            "base_url" => self.base_url = None,
            "default_text_model" => self.default_text_model = None,
            "output_mode" => self.output_mode = None,
            "verbosity" => self.verbosity = None,
            "log_level" => self.log_level = None,
            "telemetry" => self.telemetry = None,
            "approval_policy" => self.approval_policy = None,
            "sandbox_mode" => self.sandbox_mode = None,
            "tools.always_load" => {
                if let Some(tools) = self.tools.as_mut() {
                    tools.always_load.clear();
                }
            }
            _ => {
                self.extras.remove(key);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn list_values(&self) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        for key in [
            "api_key",
            "base_url",
            "default_text_model",
            "output_mode",
            "verbosity",
            "log_level",
            "telemetry",
            "approval_policy",
            "sandbox_mode",
            "tools.always_load",
        ] {
            if let Some(value) = self.get_display_value(key) {
                values.insert(key.to_string(), value);
            }
        }
        for (key, value) in &self.extras {
            values.insert(key.clone(), redact_toml_value_for_display(key, value));
        }
        values
    }

    #[must_use]
    pub fn stream_chunk_timeout_secs(&self) -> u64 {
        const DEFAULT: u64 = crate::fleet::DEFAULT_STREAM_CHUNK_TIMEOUT_SECS;
        const MIN: u64 = crate::fleet::MIN_STREAM_CHUNK_TIMEOUT_SECS;
        const MAX: u64 = crate::fleet::MAX_STREAM_CHUNK_TIMEOUT_SECS;
        let raw = self
            .extras
            .get("tui")
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get("stream_chunk_timeout_secs"))
            .and_then(toml_value_as_u64)
            .or_else(|| {
                self.extras
                    .get("tui.stream_chunk_timeout_secs")
                    .and_then(toml_value_as_u64)
            })
            .or_else(|| {
                self.extras
                    .get("stream_chunk_timeout_secs")
                    .and_then(toml_value_as_u64)
            })
            .unwrap_or(DEFAULT);
        if raw == 0 {
            DEFAULT
        } else {
            raw.clamp(MIN, MAX)
        }
    }

    pub fn resolve_runtime_options(
        &self,
        cli: &CliRuntimeOverrides,
    ) -> Result<ResolvedRuntimeOptions> {
        let no_keyring = Secrets::new(std::sync::Arc::new(
            codewhale_secrets::InMemoryKeyringStore::new(),
        ));
        self.resolve_runtime_options_with_secrets(cli, &no_keyring)
    }

    /// Credential precedence is fixed: CLI -> config -> keyring -> environment.
    pub fn resolve_runtime_options_with_secrets(
        &self,
        cli: &CliRuntimeOverrides,
        secrets: &Secrets,
    ) -> Result<ResolvedRuntimeOptions> {
        self.validate()?;
        let env = EnvRuntimeOverrides::load()?;

        let (api_key, api_key_source) = if let Some(value) = non_empty(cli.api_key.clone()) {
            (Some(value), Some(RuntimeApiKeySource::Cli))
        } else if let Some(value) = non_empty(self.api_key.clone()) {
            (Some(value), Some(RuntimeApiKeySource::ConfigFile))
        } else {
            match secrets.resolve_with_source("deepseek") {
                Some((value, SecretSource::Keyring)) => {
                    (Some(value), Some(RuntimeApiKeySource::Keyring))
                }
                Some((value, SecretSource::Env)) => (Some(value), Some(RuntimeApiKeySource::Env)),
                None => (None, None),
            }
        };

        let model = cli
            .model
            .as_deref()
            .or(env.model.as_deref())
            .or(self.default_text_model.as_deref())
            .unwrap_or(DEFAULT_DEEPSEEK_MODEL);
        let model = canonical_deepseek_model(model)?;

        let base_url = cli
            .base_url
            .as_deref()
            .or(env.base_url.as_deref())
            .or(self.base_url.as_deref())
            .unwrap_or(DEFAULT_DEEPSEEK_BASE_URL)
            .trim_end_matches('/')
            .to_string();
        validate_deepseek_base_url(&base_url)?;

        Ok(ResolvedRuntimeOptions {
            model,
            api_key,
            api_key_source,
            base_url,
            output_mode: cli
                .output_mode
                .clone()
                .or(env.output_mode)
                .or_else(|| self.output_mode.clone()),
            log_level: cli
                .log_level
                .clone()
                .or(env.log_level)
                .or_else(|| self.log_level.clone()),
            telemetry: cli
                .telemetry
                .or(env.telemetry)
                .or(self.telemetry)
                .unwrap_or(false),
            approval_policy: cli
                .approval_policy
                .clone()
                .or(env.approval_policy)
                .or_else(|| self.approval_policy.clone()),
            sandbox_mode: cli
                .sandbox_mode
                .clone()
                .or(env.sandbox_mode)
                .or_else(|| self.sandbox_mode.clone()),
            yolo: cli.yolo.or(env.yolo),
            verbosity: cli
                .verbosity
                .clone()
                .or(env.verbosity)
                .or_else(|| self.verbosity.clone()),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct CliRuntimeOverrides {
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: Option<bool>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApiKeySource {
    Cli,
    ConfigFile,
    Keyring,
    Env,
}

impl RuntimeApiKeySource {
    #[must_use]
    pub fn as_env_value(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::ConfigFile => "config",
            Self::Keyring => "keyring",
            Self::Env => "env",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedRuntimeOptions {
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_source: Option<RuntimeApiKeySource>,
    pub base_url: String,
    pub output_mode: Option<String>,
    pub log_level: Option<String>,
    pub telemetry: bool,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
    pub config: ConfigToml,
    original_raw: Option<String>,
}

impl ConfigStore {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = resolve_config_path(path)?;
        let (config, original_raw) = if checked_path_exists(&path)? {
            let raw = read_checked_config_file(&path)?;
            reject_retired_config_surface(&raw)
                .with_context(|| format!("配置不兼容：{}", path.display()))?;
            let parsed: ConfigToml = toml::from_str(&raw)
                .with_context(|| format!("failed to parse config at {}", path.display()))?;
            parsed.validate()?;
            (parsed, Some(raw))
        } else {
            (ConfigToml::default(), None)
        };
        Ok(Self {
            path,
            config,
            original_raw,
        })
    }

    pub fn rendered_body(&self) -> Result<String> {
        self.config.validate()?;
        let serialized =
            toml::to_string_pretty(&self.config).context("failed to serialize config")?;
        if let Some(ref original_raw) = self.original_raw {
            Ok(
                merge_and_preserve_comments(&serialized, original_raw).unwrap_or_else(|error| {
                    tracing::warn!(
                        "failed to merge config comments, saving without them: {error:#}"
                    );
                    serialized
                }),
            )
        } else {
            Ok(serialized)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = normalize_config_file_path(self.path.clone())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let body = self.rendered_body()?;
        if checked_path_exists(&path)? {
            let existing = read_checked_config_file(&path)?;
            if existing == body {
                return Ok(());
            }
            write_one_time_config_backup(&path)?;
        }
        persistence::atomic_write(&path, body.as_bytes())
            .with_context(|| format!("failed to write config at {}", path.display()))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn project_approval_policy_is_allowed(current: Option<&str>, project: &str) -> bool {
    let Some(project_rank) = approval_policy_rank(project) else {
        return false;
    };
    current
        .and_then(approval_policy_rank)
        .is_none_or(|current_rank| project_rank >= current_rank)
}

pub fn project_sandbox_mode_is_allowed(current: Option<&str>, project: &str) -> bool {
    let Some(project_rank) = sandbox_mode_rank(project) else {
        return false;
    };
    current
        .and_then(sandbox_mode_rank)
        .is_none_or(|current_rank| project_rank <= current_rank)
}

pub fn load_project_config(workspace: &Path) -> Option<ConfigToml> {
    let workspace = match normalize_project_workspace(workspace) {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!("ignoring unsafe project config workspace: {error:#}");
            return None;
        }
    };
    let primary = workspace.join(".codewhale").join(CONFIG_FILE_NAME);
    let legacy = workspace.join(".deepseek").join(CONFIG_FILE_NAME);
    let path = if primary.exists() {
        primary
    } else if legacy.exists() {
        legacy
    } else {
        return None;
    };
    let raw = match read_checked_config_file(&path) {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(
                "failed to read project config {}: {error:#}",
                path.display()
            );
            return None;
        }
    };
    if let Err(error) = reject_project_model_authority(&raw) {
        tracing::warn!(
            "ignoring unsafe project config {}: {error:#}",
            path.display()
        );
        return None;
    }
    match toml::from_str::<ConfigToml>(&raw) {
        Ok(config) => Some(config),
        Err(error) => {
            tracing::warn!("failed to parse project config {}: {error}", path.display());
            None
        }
    }
}

#[must_use]
pub fn canonical_deepseek_model(model: &str) -> Result<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        bail!("DeepSeek 模型名不能为空");
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        bail!("DeepSeek 模型 ID 含有非法字符：'{trimmed}'");
    }
    let lower = trimmed.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "pro" | "deepseek-v4pro" => "deepseek-v4-pro".to_string(),
        "flash" | "deepseek-v4flash" | "deepseek-chat" | "deepseek-reasoner" => {
            "deepseek-v4-flash".to_string()
        }
        "auto" => "auto".to_string(),
        _ if lower.starts_with("deepseek-") => trimmed.to_string(),
        _ => bail!("不支持模型 '{trimmed}'；CodeWhale 仅接受官方 DeepSeek 模型 ID"),
    };
    Ok(canonical)
}

pub fn validate_deepseek_base_url(base_url: &str) -> Result<()> {
    let normalized = base_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        bail!("DeepSeek base_url 不能为空");
    }
    if is_official_deepseek_base_url(normalized) || base_url_uses_loopback(normalized) {
        return Ok(());
    }
    bail!("仅支持官方 DeepSeek endpoint；loopback 只允许用于显式离线 fixture：{normalized}")
}

#[must_use]
pub fn is_official_deepseek_base_url(base_url: &str) -> bool {
    matches!(
        base_url
            .trim()
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .as_str(),
        "https://api.deepseek.com"
            | "https://api.deepseek.com/v1"
            | "https://api.deepseek.com/beta"
    )
}

fn base_url_uses_loopback(base_url: &str) -> bool {
    matches!(
        base_url_host(base_url)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("localhost" | "127.0.0.1" | "::1")
    )
}

fn base_url_host(base_url: &str) -> Option<&str> {
    let without_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = without_scheme.split('/').next()?.rsplit('@').next()?;
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split_once(']').map(|(host, _)| host);
    }
    authority.split(':').next().filter(|host| !host.is_empty())
}

fn reject_retired_config_surface(raw: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(raw).context("failed to parse config TOML")?;
    let Some(root) = value.as_table() else {
        return Ok(());
    };
    reject_retired_table_keys(root, "root")?;
    if let Some(profiles) = root.get("profiles").and_then(toml::Value::as_table) {
        for (name, profile) in profiles {
            if let Some(profile) = profile.as_table() {
                reject_retired_table_keys(profile, &format!("profiles.{name}"))?;
                for key in ["api_key", "base_url", "default_text_model"] {
                    if profile.contains_key(key) {
                        bail!(
                            "profile '{name}' cannot set '{key}'; DeepSeek model authority is root-only"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn reject_project_model_authority(raw: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(raw).context("failed to parse project config TOML")?;
    let Some(root) = value.as_table() else {
        return Ok(());
    };
    for key in ["api_key", "base_url", "default_text_model"] {
        if root.contains_key(key) {
            bail!("project config cannot set '{key}'");
        }
    }
    reject_retired_table_keys(root, "project root")
}

fn reject_retired_table_keys(
    table: &toml::map::Map<String, toml::Value>,
    path: &str,
) -> Result<()> {
    for key in RETIRED_ROOT_KEYS {
        if table.contains_key(*key) {
            bail!(
                "配置项 '{path}.{key}' 已删除；CodeWhale 固定使用官方 DeepSeek，不再读取 Provider 兼容配置"
            );
        }
    }
    Ok(())
}

fn reject_retired_extra_keys(extras: &BTreeMap<String, toml::Value>) -> Result<()> {
    for key in RETIRED_ROOT_KEYS {
        if extras.contains_key(*key) {
            reject_retired_config_key(key)?;
        }
    }
    Ok(())
}

fn reject_retired_config_key(key: &str) -> Result<()> {
    let root = key.split('.').next().unwrap_or(key);
    if RETIRED_ROOT_KEYS.contains(&key) || RETIRED_ROOT_KEYS.contains(&root) {
        bail!("配置项 '{key}' 已删除；请使用 api_key、base_url 或 default_text_model");
    }
    Ok(())
}

fn approval_policy_rank(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "never" | "auto" => Some(0),
        "on-request" | "on-failure" => Some(1),
        "untrusted" => Some(2),
        _ => None,
    }
}

fn sandbox_mode_rank(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "read-only" => Some(0),
        "workspace-write" => Some(1),
        "danger-full-access" | "external-sandbox" => Some(2),
        _ => None,
    }
}

fn parse_bool(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Ok(true),
        "0" | "false" | "no" | "off" | "disabled" => Ok(false),
        _ => bail!("invalid boolean '{raw}'"),
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn toml_value_as_u64(value: &toml::Value) -> Option<u64> {
    match value {
        toml::Value::Integer(value) => u64::try_from(*value).ok(),
        toml::Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn redact_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 16 {
        return "********".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}***{suffix}")
}

#[must_use]
pub fn is_sensitive_config_key(key: &str) -> bool {
    let Some(segment) = key.rsplit('.').next() else {
        return false;
    };
    let normalized = segment
        .trim()
        .trim_matches('"')
        .replace('-', "_")
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "api_keys"
            | "authorization"
            | "bearer"
            | "client_secret"
            | "credential"
            | "credentials"
            | "id_token"
            | "password"
            | "passwords"
            | "passwd"
            | "proxy_authorization"
            | "refresh_token"
            | "secret"
            | "secrets"
            | "token"
            | "tokens"
    ) || normalized.ends_with("_api_key")
        || normalized.ends_with("_authorization")
        || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
}

fn redact_toml_value_for_display(key: &str, value: &toml::Value) -> String {
    redact_toml_value_for_display_inner(key, false, value).to_string()
}

fn redact_toml_value_for_display_inner(
    key: &str,
    sensitive_ancestor: bool,
    value: &toml::Value,
) -> toml::Value {
    let sensitive = sensitive_ancestor || is_sensitive_config_key(key);
    match value {
        toml::Value::String(value) if sensitive => toml::Value::String(redact_secret(value)),
        toml::Value::Array(values) => toml::Value::Array(
            values
                .iter()
                .map(|value| redact_toml_value_for_display_inner(key, sensitive, value))
                .collect(),
        ),
        toml::Value::Table(table) => {
            let mut redacted = toml::map::Map::new();
            for (child_key, child_value) in table {
                let path = if key.is_empty() {
                    child_key.clone()
                } else {
                    format!("{key}.{child_key}")
                };
                redacted.insert(
                    child_key.clone(),
                    redact_toml_value_for_display_inner(&path, sensitive, child_value),
                );
            }
            toml::Value::Table(redacted)
        }
        _ if sensitive => toml::Value::String("********".to_string()),
        _ => value.clone(),
    }
}

fn config_backup_file_name(path: &Path) -> OsString {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(CONFIG_FILE_NAME));
    file_name.push(".bak");
    file_name
}

fn checked_config_sibling_path(config_path: &Path, file_name: &OsStr) -> Result<PathBuf> {
    let config_path = normalize_config_file_path(config_path.to_path_buf())?;
    let parent = config_path
        .parent()
        .context("config path must include a parent directory")?;
    let path = parent.join(file_name);
    reject_path_symlink(&path)?;
    Ok(path)
}

fn checked_config_backup_path(path: &Path) -> Result<PathBuf> {
    checked_config_sibling_path(path, &config_backup_file_name(path))
}

fn write_one_time_config_backup(path: &Path) -> Result<()> {
    let backup = checked_config_backup_path(path)?;
    if backup.exists() {
        return Ok(());
    }
    fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to create config backup {} from {}",
            backup.display(),
            path.display()
        )
    })?;
    #[cfg(unix)]
    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "failed to set config backup permissions at {}",
            backup.display()
        )
    })?;
    Ok(())
}

pub fn merge_and_preserve_comments(serialized: &str, original_raw: &str) -> Result<String> {
    let original = original_raw
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse original config for comment merge")?;
    let mut new_doc = serialized
        .parse::<toml_edit::DocumentMut>()
        .context("failed to parse serialized config for comment merge")?;
    new_doc.set_trailing(original.trailing().clone());
    *new_doc.as_table_mut().decor_mut() = original.as_table().decor().clone();
    merge_decor_table(new_doc.as_table_mut(), original.as_table());
    Ok(new_doc.to_string())
}

fn merge_decor_table(target: &mut toml_edit::Table, source: &toml_edit::Table) {
    let keys: Vec<String> = source.iter().map(|(key, _)| key.to_owned()).collect();
    for key in &keys {
        let Some((source_key, source_item)) = source.get_key_value(key) else {
            continue;
        };
        let Some((mut target_key, target_item)) = target.get_key_value_mut(key) else {
            continue;
        };
        *target_key.leaf_decor_mut() = source_key.leaf_decor().clone();
        copy_item_decor(target_item, source_item);
        if let (Some(target), Some(source)) = (target_item.as_table_mut(), source_item.as_table()) {
            merge_decor_table(target, source);
        }
        if let (Some(target), Some(source)) = (
            target_item.as_array_of_tables_mut(),
            source_item.as_array_of_tables(),
        ) {
            for (index, source_table) in source.iter().enumerate() {
                if let Some(target_table) = target.get_mut(index) {
                    *target_table.decor_mut() = source_table.decor().clone();
                    merge_decor_table(target_table, source_table);
                }
            }
        }
    }
}

fn copy_item_decor(target: &mut toml_edit::Item, source: &toml_edit::Item) {
    match (target, source) {
        (toml_edit::Item::Table(target), toml_edit::Item::Table(source)) => {
            *target.decor_mut() = source.decor().clone();
        }
        (toml_edit::Item::Value(target), toml_edit::Item::Value(source)) => {
            *target.decor_mut() = source.decor().clone();
        }
        _ => {}
    }
}

pub fn default_secrets() -> &'static Secrets {
    static SECRETS: OnceLock<Secrets> = OnceLock::new();
    SECRETS.get_or_init(|| {
        #[cfg(test)]
        {
            Secrets::new(std::sync::Arc::new(
                codewhale_secrets::InMemoryKeyringStore::new(),
            ))
        }
        #[cfg(not(test))]
        {
            Secrets::auto_detect()
        }
    })
}

#[derive(Debug, Clone, Default)]
struct EnvRuntimeOverrides {
    model: Option<String>,
    base_url: Option<String>,
    output_mode: Option<String>,
    log_level: Option<String>,
    telemetry: Option<bool>,
    approval_policy: Option<String>,
    sandbox_mode: Option<String>,
    yolo: Option<bool>,
    verbosity: Option<String>,
}

impl EnvRuntimeOverrides {
    fn load() -> Result<Self> {
        for name in ["CODEWHALE_PROVIDER", "DEEPSEEK_PROVIDER"] {
            if let Ok(value) = std::env::var(name)
                && !value.trim().is_empty()
            {
                bail!("环境变量 {name} 已删除；CodeWhale 固定使用官方 DeepSeek，请移除该变量");
            }
        }
        let model = first_env(&[
            "CODEWHALE_MODEL",
            "DEEPSEEK_MODEL",
            "DEEPSEEK_DEFAULT_TEXT_MODEL",
        ]);
        if let Some(model) = model.as_deref() {
            canonical_deepseek_model(model)?;
        }
        let base_url = first_env(&["CODEWHALE_BASE_URL", "DEEPSEEK_BASE_URL"]);
        if let Some(base_url) = base_url.as_deref() {
            validate_deepseek_base_url(base_url)?;
        }
        Ok(Self {
            model,
            base_url,
            output_mode: first_env(&["DEEPSEEK_OUTPUT_MODE"]),
            log_level: first_env(&["DEEPSEEK_LOG_LEVEL"]),
            telemetry: parse_optional_bool_env("DEEPSEEK_TELEMETRY")?,
            approval_policy: first_env(&["DEEPSEEK_APPROVAL_POLICY"]),
            sandbox_mode: first_env(&["DEEPSEEK_SANDBOX_MODE"]),
            yolo: parse_optional_bool_env("DEEPSEEK_YOLO")?,
            verbosity: first_env(&["CODEWHALE_VERBOSITY", "DEEPSEEK_VERBOSITY"]),
        })
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .and_then(|value| non_empty(Some(value)))
    })
}

fn parse_optional_bool_env(name: &str) -> Result<Option<bool>> {
    std::env::var(name)
        .ok()
        .map(|value| parse_bool(&value).with_context(|| format!("invalid {name}")))
        .transpose()
}
