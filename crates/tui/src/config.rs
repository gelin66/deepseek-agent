//! DeepSeek-only configuration for the retained interactive client.
//!
//! Provider selection, model aliases, credentials, and endpoint resolution
//! have one production meaning: the official DeepSeek backend. Non-provider
//! settings stay here because the TUI remains a thin client of the canonical
//! application/runtime path.

use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit::log_sensitive_event;
use crate::features::{Feature, Features, FeaturesToml, is_known_feature_key};

mod models;
pub use models::*;
mod paths;
use paths::{
    canonicalize_or_keep, codewhale_home_dir, default_config_path, default_mcp_config_path,
    default_skills_dir, env_config_path, expand_pathbuf, home_config_path, workspace_config_key,
};
pub(crate) use paths::{effective_home_dir, expand_path};
mod search;
pub use search::*;
mod subagent_limits;
pub use subagent_limits::*;

const API_KEYRING_SENTINEL: &str = "__KEYRING__";
pub const DEEPSEEK_PROVIDER_ID: &str = "deepseek";
pub const DEEPSEEK_DISPLAY_NAME: &str = "DeepSeek";
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
pub const DEEPSEEK_CREDENTIAL_URL: &str = "https://platform.deepseek.com/api_keys";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeepSeekCapability {
    pub resolved_model: String,
    pub context_window: u32,
    pub max_output: u32,
    pub thinking_supported: bool,
    pub cache_telemetry_supported: bool,
    pub request_payload_mode: RequestPayloadMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_deprecation: Option<ModelAliasDeprecation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAliasDeprecation {
    pub alias: String,
    pub replacement: String,
    pub retirement_date: String,
    pub retirement_utc: String,
    pub notice: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RequestPayloadMode {
    ChatCompletions,
}

#[must_use]
pub fn deepseek_capability(resolved_model: &str) -> DeepSeekCapability {
    let normalized =
        normalize_model_name(resolved_model).unwrap_or_else(|| resolved_model.trim().to_string());
    let capability = codewhale_deepseek::official_model_capabilities(&normalized).ok();
    DeepSeekCapability {
        resolved_model: normalized,
        context_window: capability
            .map(|value| value.context_window_tokens)
            .unwrap_or(codewhale_deepseek::OFFICIAL_V4_CONTEXT_WINDOW_TOKENS),
        max_output: capability
            .map(|value| value.max_output_tokens)
            .unwrap_or(codewhale_deepseek::OFFICIAL_V4_MAX_OUTPUT_TOKENS),
        thinking_supported: capability.is_some(),
        cache_telemetry_supported: true,
        request_payload_mode: RequestPayloadMode::ChatCompletions,
        alias_deprecation: None,
    }
}

#[must_use]
pub fn canonical_model_name(model: &str) -> Option<&'static str> {
    match model.trim().to_ascii_lowercase().as_str() {
        "deepseek-v4-pro" | "deepseek-v4pro" | "pro" => Some("deepseek-v4-pro"),
        "deepseek-v4-flash" | "deepseek-v4flash" | "flash" | "deepseek-chat"
        | "deepseek-reasoner" => Some("deepseek-v4-flash"),
        _ => None,
    }
}

#[must_use]
pub fn normalize_model_name(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if !trimmed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("auto") {
        return Some("auto".to_string());
    }
    canonical_model_name(trimmed)
        .map(str::to_string)
        .or_else(|| {
            trimmed
                .to_ascii_lowercase()
                .starts_with("deepseek-")
                .then(|| trimmed.to_string())
        })
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub initial_delay: Option<f64>,
    pub max_delay: Option<f64>,
    pub exponential_base: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: u32,
    pub initial_delay: f64,
    pub max_delay: f64,
    pub exponential_base: f64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TuiConfig {
    pub alternate_screen: Option<String>,
    pub mouse_capture: Option<bool>,
    pub terminal_probe_timeout_ms: Option<u64>,
    pub stream_chunk_timeout_secs: Option<u64>,
    pub osc8_links: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ContextConfig {
    #[serde(default)]
    pub project_pack: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SubagentsConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    #[serde(default)]
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SkillsConfig {
    #[serde(default, alias = "scanCodewhaleOnly")]
    pub scan_codewhale_only: Option<bool>,
}

impl SkillsConfig {
    #[must_use]
    pub fn scan_codewhale_only(&self) -> bool {
        self.scan_codewhale_only.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_text_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub skills_dir: Option<String>,
    pub mcp_config_path: Option<String>,
    pub mcp_oauth_callback_port: Option<u16>,
    pub mcp_oauth_callback_url: Option<String>,
    pub instructions: Option<Vec<String>>,
    pub allow_shell: Option<bool>,
    #[serde(alias = "approvalPolicy")]
    pub approval_policy: Option<String>,
    #[serde(alias = "sandboxMode")]
    pub sandbox_mode: Option<String>,
    pub yolo: Option<bool>,
    pub verbosity: Option<String>,
    #[serde(alias = "sandboxBackend")]
    pub sandbox_backend: Option<String>,
    #[serde(alias = "sandboxUrl")]
    pub sandbox_url: Option<String>,
    #[serde(alias = "sandboxApiKey")]
    pub sandbox_api_key: Option<String>,
    #[serde(alias = "preferBwrap")]
    pub prefer_bwrap: Option<bool>,
    #[serde(alias = "maxSubagents")]
    pub max_subagents: Option<usize>,
    pub retry: Option<RetryConfig>,
    pub features: Option<FeaturesToml>,
    pub tui: Option<TuiConfig>,
    #[serde(default)]
    pub skills: Option<SkillsConfig>,
    #[serde(default)]
    pub search: Option<SearchConfig>,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub fleet: Option<codewhale_config::FleetConfigToml>,
    #[serde(default)]
    pub subagents: Option<SubagentsConfig>,
    #[serde(flatten)]
    pub(crate) extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ConfigFile {
    #[serde(flatten)]
    base: Config,
    profiles: Option<HashMap<String, Config>>,
}

impl Config {
    pub fn load(path: Option<PathBuf>, profile: Option<&str>) -> Result<Self> {
        let path = resolve_load_config_path(path);
        let mut config = match path.as_ref() {
            Some(path) if path.exists() => {
                let contents = fs::read_to_string(path)
                    .with_context(|| format!("读取配置失败：{}", path.display()))?;
                reject_foreign_provider_declarations(&contents)
                    .with_context(|| format!("解析配置失败：{}", path.display()))?;
                let parsed: ConfigFile = toml::from_str(&contents)
                    .with_context(|| format!("解析配置失败：{}", path.display()))?;
                apply_profile(parsed, profile)?
            }
            _ => Config::default(),
        };
        for name in ["CODEWHALE_PROVIDER", "DEEPSEEK_PROVIDER"] {
            if std::env::var(name).is_ok_and(|value| !value.trim().is_empty()) {
                anyhow::bail!(
                    "环境变量 {name} 已删除；CodeWhale 固定使用官方 DeepSeek，请移除该变量。"
                );
            }
        }
        apply_env_overrides(&mut config);
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        for retired in [
            "apiKey",
            "baseUrl",
            "defaultTextModel",
            "provider",
            "providers",
            "fallback_providers",
            "fallbackProviders",
            "model",
            "models",
            "model_catalog",
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
        ] {
            if self.extra.contains_key(retired) {
                anyhow::bail!(
                    "配置项 '{retired}' 已删除；CodeWhale 仅使用官方 DeepSeek 模型目录。"
                );
            }
        }
        if let Some(key) = self.api_key.as_deref()
            && key.trim().is_empty()
        {
            anyhow::bail!("api_key 不能为空字符串");
        }
        if let Some(model) = self.default_text_model.as_deref()
            && normalize_model_name(model).is_none()
        {
            anyhow::bail!(
                "不支持模型 '{model}'；仅支持 auto、deepseek-v4-pro 或 deepseek-v4-flash。"
            );
        }
        if let Some(base_url) = self.base_url.as_deref() {
            codewhale_config::validate_deepseek_base_url(base_url)?;
        }
        if let Some(features) = &self.features {
            for key in features.entries.keys() {
                if !is_known_feature_key(key) {
                    anyhow::bail!("未知 feature flag：{key}");
                }
            }
        }
        if let Some(policy) = self.approval_policy.as_deref()
            && !matches!(
                policy.trim().to_ascii_lowercase().as_str(),
                "on-request" | "auto"
            )
        {
            anyhow::bail!("approval_policy 无效：'{policy}'；应为 on-request 或 auto。");
        }
        if let Some(verbosity) = self.verbosity.as_deref()
            && !matches!(
                verbosity.trim().to_ascii_lowercase().as_str(),
                "normal" | "concise"
            )
        {
            anyhow::bail!("verbosity 无效：'{verbosity}'；应为 normal 或 concise。");
        }
        if let Some(mode) = self.sandbox_mode.as_deref()
            && !matches!(
                mode.trim().to_ascii_lowercase().as_str(),
                "read-only" | "workspace-write" | "danger-full-access" | "external-sandbox"
            )
        {
            anyhow::bail!("sandbox_mode 无效：'{mode}'。");
        }
        if let Some(tui) = &self.tui
            && let Some(mode) = tui.alternate_screen.as_deref()
            && !matches!(
                mode.trim().to_ascii_lowercase().as_str(),
                "auto" | "always" | "never"
            )
        {
            anyhow::bail!("tui.alternate_screen 无效：'{mode}'。");
        }
        Ok(())
    }

    #[must_use]
    pub fn insecure_skip_tls_verify(&self) -> bool {
        false
    }

    #[must_use]
    pub fn default_model(&self) -> String {
        let selected = self
            .default_text_model
            .as_deref()
            .unwrap_or(DEFAULT_TEXT_MODEL);
        normalize_model_name(selected).unwrap_or_else(|| selected.trim().to_string())
    }

    #[must_use]
    pub fn deepseek_base_url(&self) -> String {
        let configured = self.base_url.as_deref().map(str::to_string);
        let environment = std::env::var("CODEWHALE_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("DEEPSEEK_BASE_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });
        let base = configured
            .or(environment)
            .unwrap_or_else(|| DEFAULT_DEEPSEEK_BASE_URL.to_string());
        normalize_base_url(&base)
    }

    pub fn deepseek_api_key(&self) -> Result<String> {
        if std::env::var("DEEPSEEK_API_KEY_SOURCE").as_deref() == Ok("cli")
            && let Some(key) = explicit_cli_api_key_override()
        {
            return Ok(key);
        }
        if let Some(key) = self
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty() && *key != API_KEYRING_SENTINEL)
        {
            return Ok(key.to_string());
        }
        if let Some(key) = explicit_cli_api_key_override() {
            return Ok(key);
        }
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY")
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
        if base_url_uses_local_host(&self.deepseek_base_url()) {
            return Ok(String::new());
        }
        anyhow::bail!(
            "未找到 DeepSeek API Key。\n\
             1. 获取 Key：https://platform.deepseek.com/api_keys\n\
             2. 保存：codewhale auth set\n\
             也可在当前 shell 设置 DEEPSEEK_API_KEY。"
        )
    }

    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.skills_dir
            .as_deref()
            .map(expand_path)
            .or_else(default_skills_dir)
            .unwrap_or_else(|| PathBuf::from("./skills"))
    }

    #[must_use]
    pub fn mcp_config_path(&self) -> PathBuf {
        self.mcp_config_path
            .as_deref()
            .map(expand_path)
            .or_else(default_mcp_config_path)
            .unwrap_or_else(|| PathBuf::from("./mcp.json"))
    }

    #[must_use]
    pub fn instructions_paths(&self) -> Vec<PathBuf> {
        self.instructions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(expand_path)
            .collect()
    }

    #[must_use]
    pub fn project_context_pack_enabled(&self) -> bool {
        self.context.project_pack.unwrap_or(true)
    }

    #[must_use]
    pub fn allow_shell(&self) -> bool {
        self.allow_shell.unwrap_or(false)
    }

    #[must_use]
    pub fn interactive_allow_shell(&self) -> bool {
        self.allow_shell.unwrap_or(true)
    }

    #[must_use]
    pub fn max_subagents(&self) -> usize {
        self.subagents
            .as_ref()
            .and_then(|subagents| subagents.max_concurrent)
            .or(self.max_subagents)
            .unwrap_or(DEFAULT_MAX_SUBAGENTS)
            .clamp(1, MAX_SUBAGENTS)
    }

    #[must_use]
    pub fn subagents_enabled(&self) -> bool {
        self.subagents_disabled_reason().is_none()
    }

    #[must_use]
    pub fn subagents_disabled_reason(&self) -> Option<&'static str> {
        if !self.features().enabled(Feature::Subagents) {
            return Some("features.subagents=false");
        }
        let config = self.subagents.as_ref()?;
        if config.enabled == Some(false) {
            return Some("subagents.enabled=false");
        }
        if config.max_concurrent == Some(0) {
            return Some("subagents.max_concurrent=0");
        }
        if config.max_depth == Some(0) {
            return Some("subagents.max_depth=0");
        }
        None
    }

    #[must_use]
    pub fn subagent_max_spawn_depth(&self) -> u32 {
        self.subagents
            .as_ref()
            .and_then(|subagents| subagents.max_depth)
            .unwrap_or(codewhale_config::DEFAULT_SPAWN_DEPTH)
            .min(codewhale_config::MAX_SPAWN_DEPTH_CEILING)
    }

    #[must_use]
    pub fn stream_chunk_timeout_secs(&self) -> u64 {
        let raw = self
            .tui
            .as_ref()
            .and_then(|tui| tui.stream_chunk_timeout_secs)
            .or_else(|| {
                std::env::var(STREAM_CHUNK_TIMEOUT_ENV)
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap_or(DEFAULT_STREAM_CHUNK_TIMEOUT_SECS);
        if raw == 0 {
            DEFAULT_STREAM_CHUNK_TIMEOUT_SECS
        } else {
            raw.clamp(MIN_STREAM_CHUNK_TIMEOUT_SECS, MAX_STREAM_CHUNK_TIMEOUT_SECS)
        }
    }

    #[must_use]
    pub fn fleet_config(&self) -> codewhale_config::FleetConfigToml {
        self.fleet.clone().unwrap_or_default()
    }

    #[must_use]
    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    #[must_use]
    pub fn skills_config(&self) -> SkillsConfig {
        self.skills.clone().unwrap_or_default()
    }

    #[must_use]
    pub fn features(&self) -> Features {
        let mut features = Features::with_defaults();
        if let Some(table) = &self.features {
            features.apply_map(&table.entries);
        }
        features
    }

    pub fn set_feature(&mut self, key: &str, enabled: bool) -> Result<()> {
        if !is_known_feature_key(key) {
            anyhow::bail!("未知 feature flag：{key}");
        }
        self.features
            .get_or_insert_with(FeaturesToml::default)
            .entries
            .insert(key.to_string(), enabled);
        Ok(())
    }

    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        let defaults = RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_delay: 1.0,
            max_delay: 60.0,
            exponential_base: 2.0,
        };
        let Some(config) = &self.retry else {
            return defaults;
        };
        RetryPolicy {
            enabled: config.enabled.unwrap_or(defaults.enabled),
            max_retries: config.max_retries.unwrap_or(defaults.max_retries),
            initial_delay: config.initial_delay.unwrap_or(defaults.initial_delay),
            max_delay: config.max_delay.unwrap_or(defaults.max_delay),
            exponential_base: config.exponential_base.unwrap_or(defaults.exponential_base),
        }
    }

    #[must_use]
    pub fn search_provider_resolution(&self) -> SearchProviderResolution {
        if let Ok(raw) = std::env::var("CODEWHALE_SEARCH_PROVIDER")
            && let Some(provider) = SearchProvider::parse(&raw)
        {
            return SearchProviderResolution {
                provider,
                source: SearchProviderSource::EnvOverride,
            };
        }
        if let Some(provider) = self.search.as_ref().and_then(|search| search.provider) {
            return SearchProviderResolution {
                provider,
                source: SearchProviderSource::Config,
            };
        }
        SearchProviderResolution {
            provider: SearchProvider::default(),
            source: SearchProviderSource::Default,
        }
    }
}

fn reject_foreign_provider_declarations(contents: &str) -> Result<()> {
    let document: toml::Value = toml::from_str(contents)?;
    let Some(root) = document.as_table() else {
        return Ok(());
    };
    reject_foreign_provider_table(root)?;
    if let Some(profiles) = root.get("profiles").and_then(toml::Value::as_table) {
        for profile in profiles.values().filter_map(toml::Value::as_table) {
            reject_foreign_provider_table(profile)?;
            for key in [
                "api_key",
                "apiKey",
                "base_url",
                "baseUrl",
                "default_text_model",
                "defaultTextModel",
            ] {
                if profile.contains_key(key) {
                    anyhow::bail!(
                        "profile 不能设置 '{key}'；DeepSeek credential、endpoint 和 model 只允许根级配置。"
                    );
                }
            }
        }
    }
    Ok(())
}

fn reject_foreign_provider_table(table: &toml::Table) -> Result<()> {
    if table.contains_key("provider") {
        anyhow::bail!("只支持官方 DeepSeek Provider；配置项 'provider' 已删除，请移除该配置。");
    }
    if table.contains_key("providers") {
        anyhow::bail!(
            "配置表 [providers.*] 已删除；请改用根级 api_key、base_url 和 default_text_model。"
        );
    }
    Ok(())
}

fn apply_profile(config: ConfigFile, profile: Option<&str>) -> Result<Config> {
    let Some(profile_name) = profile else {
        return Ok(config.base);
    };
    let Some(override_config) = config
        .profiles
        .as_ref()
        .and_then(|profiles| profiles.get(profile_name))
    else {
        let mut available = config
            .profiles
            .as_ref()
            .map(|profiles| profiles.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        available.sort();
        anyhow::bail!(
            "找不到 profile '{profile_name}'；可用 profile：{}",
            if available.is_empty() {
                "无".to_string()
            } else {
                available.join(", ")
            }
        );
    };
    Ok(merge_config(config.base, override_config.clone()))
}

fn merge_config(base: Config, selected: Config) -> Config {
    let mut extra = base.extra;
    extra.extend(selected.extra);
    Config {
        api_key: base.api_key,
        base_url: base.base_url,
        default_text_model: base.default_text_model,
        reasoning_effort: selected.reasoning_effort.or(base.reasoning_effort),
        skills_dir: selected.skills_dir.or(base.skills_dir),
        mcp_config_path: selected.mcp_config_path.or(base.mcp_config_path),
        mcp_oauth_callback_port: selected
            .mcp_oauth_callback_port
            .or(base.mcp_oauth_callback_port),
        mcp_oauth_callback_url: selected
            .mcp_oauth_callback_url
            .or(base.mcp_oauth_callback_url),
        instructions: selected.instructions.or(base.instructions),
        allow_shell: selected.allow_shell.or(base.allow_shell),
        approval_policy: selected.approval_policy.or(base.approval_policy),
        sandbox_mode: selected.sandbox_mode.or(base.sandbox_mode),
        yolo: selected.yolo.or(base.yolo),
        verbosity: selected.verbosity.or(base.verbosity),
        sandbox_backend: selected.sandbox_backend.or(base.sandbox_backend),
        sandbox_url: selected.sandbox_url.or(base.sandbox_url),
        sandbox_api_key: selected.sandbox_api_key.or(base.sandbox_api_key),
        prefer_bwrap: selected.prefer_bwrap.or(base.prefer_bwrap),
        max_subagents: selected.max_subagents.or(base.max_subagents),
        retry: selected.retry.or(base.retry),
        features: merge_features(base.features, selected.features),
        tui: selected.tui.or(base.tui),
        skills: selected.skills.or(base.skills),
        search: selected.search.or(base.search),
        context: ContextConfig {
            project_pack: selected.context.project_pack.or(base.context.project_pack),
        },
        fleet: selected.fleet.or(base.fleet),
        subagents: selected.subagents.or(base.subagents),
        extra,
    }
}

fn merge_features(
    base: Option<FeaturesToml>,
    selected: Option<FeaturesToml>,
) -> Option<FeaturesToml> {
    match (base, selected) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => Some(value),
        (Some(mut base), Some(selected)) => {
            base.entries.extend(selected.entries);
            Some(base)
        }
    }
}

fn apply_env_overrides(config: &mut Config) {
    if let Some(base_url) = deepseek_env_override("CODEWHALE_BASE_URL", "DEEPSEEK_BASE_URL") {
        config.base_url = Some(base_url);
    }
    if let Some(model) = deepseek_env_override("CODEWHALE_MODEL", "DEEPSEEK_MODEL").or_else(|| {
        std::env::var("DEEPSEEK_DEFAULT_TEXT_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        config.default_text_model = Some(model);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SKILLS_DIR") {
        config.skills_dir = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_MCP_CONFIG") {
        config.mcp_config_path = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_ALLOW_SHELL") {
        config.allow_shell = Some(env_truthy(&value));
    }
    if let Some(value) = codewhale_env("CODEWHALE_APPROVAL_POLICY") {
        config.approval_policy = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SANDBOX_MODE") {
        config.sandbox_mode = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_YOLO") {
        config.yolo = Some(env_truthy(&value));
    }
    if let Some(value) = codewhale_env("CODEWHALE_VERBOSITY") {
        config.verbosity = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SANDBOX_BACKEND") {
        config.sandbox_backend = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SANDBOX_URL") {
        config.sandbox_url = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SANDBOX_API_KEY") {
        config.sandbox_api_key = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SEARCH_API_KEY") {
        config
            .search
            .get_or_insert_with(SearchConfig::default)
            .api_key = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_SEARCH_BASE_URL") {
        config
            .search
            .get_or_insert_with(SearchConfig::default)
            .base_url = Some(value);
    }
    if let Some(value) = codewhale_env("CODEWHALE_MAX_SUBAGENTS")
        && let Ok(parsed) = value.parse::<usize>()
    {
        config.max_subagents = Some(parsed.clamp(1, MAX_SUBAGENTS));
    }
}

fn deepseek_env_override(codewhale_name: &str, deepseek_name: &str) -> Option<String> {
    std::env::var(codewhale_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var(deepseek_name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn codewhale_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn env_truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn normalize_base_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    let official = reqwest::Url::parse(trimmed).ok().is_some_and(|url| {
        url.host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
    });
    if official {
        trimmed.trim_end_matches("/v1").to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn base_url_uses_local_host(base_url: &str) -> bool {
    reqwest::Url::parse(base_url).ok().is_some_and(|url| {
        url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
        })
    })
}

#[must_use]
pub(crate) fn workspace_trust_config_candidate_paths() -> Vec<PathBuf> {
    if let Some(path) = env_config_path() {
        return vec![path];
    }
    if let Some(home) = codewhale_home_dir() {
        return vec![home.join("config.toml")];
    }
    effective_home_dir().map_or_else(Vec::new, |home| {
        vec![home.join(".codewhale").join("config.toml")]
    })
}

#[must_use]
pub(crate) fn is_workspace_trusted(workspace: &Path) -> bool {
    is_workspace_trusted_at(None, workspace)
}

#[must_use]
pub(crate) fn is_workspace_trusted_at(config_path: Option<&Path>, workspace: &Path) -> bool {
    let Ok(path) = crate::config_persistence::config_toml_path(config_path) else {
        return false;
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(doc) = toml::from_str::<toml::Value>(&raw) else {
        return false;
    };
    workspace_trust_level_from_doc(&doc, workspace)
        .is_some_and(|level| level.trim().eq_ignore_ascii_case("trusted"))
}

pub(crate) fn save_workspace_trust_at(
    config_path: Option<&Path>,
    workspace: &Path,
) -> Result<PathBuf> {
    let path =
        crate::config_persistence::config_toml_path(config_path).context("无法解析当前配置路径")?;
    ensure_parent_dir(&path)?;
    let key = workspace_config_key(workspace);
    crate::config_persistence::mutate_config_document(&path, |doc| {
        crate::config_persistence::set_document_value(
            doc,
            &["projects", key.as_str(), "trust_level"],
            "trusted",
        )
    })
    .with_context(|| format!("写入配置失败：{}", path.display()))?;
    Ok(path)
}

fn workspace_trust_level_from_doc<'a>(doc: &'a toml::Value, workspace: &Path) -> Option<&'a str> {
    let workspace = canonicalize_or_keep(workspace);
    let projects = doc.get("projects")?.as_table()?;
    for (raw_path, project) in projects {
        if canonicalize_or_keep(&expand_path(raw_path)) == workspace {
            return project.get("trust_level").and_then(toml::Value::as_str);
        }
    }
    None
}

pub(crate) fn resolve_load_config_path(path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = path {
        return Some(expand_pathbuf(path));
    }
    if let Some(path) = env_config_path() {
        if path.exists() {
            return Some(path);
        }
        if let Some(home_path) = home_config_path()
            && home_path.exists()
        {
            return Some(home_path);
        }
        return Some(path);
    }
    home_config_path()
}

pub fn ensure_config_file_exists(path: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let path = path
        .map(expand_pathbuf)
        .or_else(default_config_path)
        .context("无法解析配置路径：未找到 home 目录")?;
    if path.exists() {
        return Ok(None);
    }
    ensure_parent_dir(&path)?;
    let content = format!(
        r#"# CodeWhale 配置
# 获取 DeepSeek API Key：https://platform.deepseek.com/api_keys
# 保存 Key：codewhale auth set

# 官方 DeepSeek API 根地址
# base_url = "https://api.deepseek.com"

default_text_model = "{DEFAULT_TEXT_MODEL}"
reasoning_effort = "auto"
"#
    );
    write_config_file_secure(&path, &content)
        .with_context(|| format!("写入配置失败：{}", path.display()))?;
    Ok(Some(path))
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败：{}", parent.display()))?;
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(parent) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                let mut permissions = meta.permissions();
                permissions.set_mode(mode & !0o077);
                if let Err(error) = fs::set_permissions(parent, permissions) {
                    tracing::warn!(
                        target: "codewhale::config",
                        path = %parent.display(),
                        %error,
                        "无法收紧配置目录权限"
                    );
                }
            }
        }
    }
    Ok(())
}

fn write_config_file_secure(path: &Path, content: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
        if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
            tracing::warn!(
                target: "codewhale::config",
                path = %path.display(),
                %error,
                "无法强制配置文件权限为 0600"
            );
        }
    }
    #[cfg(not(unix))]
    fs::write(path, content)?;
    Ok(())
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedCredential {
    KeyringAndConfigFile { backend: String, path: PathBuf },
    ConfigFile(PathBuf),
}

impl SavedCredential {
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::KeyringAndConfigFile { backend, path } => {
                format!("系统凭据库（{backend}）和 {}", path.display())
            }
            Self::ConfigFile(path) => path.display().to_string(),
        }
    }
}

pub fn save_api_key(api_key: &str) -> Result<SavedCredential> {
    let key = api_key.trim();
    if key.is_empty() {
        anyhow::bail!("拒绝保存空 API Key");
    }
    let path = save_api_key_to_config_file(key)?;
    #[cfg(not(test))]
    {
        let secrets = codewhale_secrets::Secrets::auto_detect();
        match secrets.set("deepseek", key) {
            Ok(()) => {
                let backend = secrets.backend_name().to_string();
                log_sensitive_event(
                    "credential.save",
                    json!({
                        "backend": backend.clone(),
                        "config_path": path.display().to_string(),
                        "dual_write": true,
                    }),
                );
                return Ok(SavedCredential::KeyringAndConfigFile { backend, path });
            }
            Err(error) => tracing::warn!("系统凭据库写入失败，仅保存到 config.toml：{error}"),
        }
    }
    Ok(SavedCredential::ConfigFile(path))
}

fn save_api_key_to_config_file(api_key: &str) -> Result<PathBuf> {
    let path = default_config_path().context("无法解析配置路径：未找到 home 目录")?;
    ensure_parent_dir(&path)?;
    if path.exists() {
        crate::config_persistence::mutate_config_document(&path, |doc| {
            crate::config_persistence::set_document_value(doc, &["api_key"], api_key)
        })
        .with_context(|| format!("写入配置失败：{}", path.display()))?;
    } else {
        let content = format!(
            r#"# CodeWhale 配置
api_key = "{api_key}"
default_text_model = "{DEFAULT_TEXT_MODEL}"
reasoning_effort = "max"
"#
        );
        crate::config_persistence::write_config_toml_atomic(&path, &content)
            .with_context(|| format!("写入配置失败：{}", path.display()))?;
    }
    log_sensitive_event(
        "credential.save",
        json!({
            "backend": "config_file",
            "config_path": path.display().to_string(),
        }),
    );
    Ok(path)
}

#[must_use]
pub fn has_api_key(config: &Config) -> bool {
    has_config_api_key(config)
        || has_env_api_key(config)
        || base_url_uses_local_host(&config.deepseek_base_url())
}

#[must_use]
pub fn has_config_api_key(config: &Config) -> bool {
    config
        .api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty() && key != API_KEYRING_SENTINEL)
}

#[must_use]
pub fn has_env_api_key(_config: &Config) -> bool {
    explicit_cli_api_key_override().is_some()
        || std::env::var("DEEPSEEK_API_KEY").is_ok_and(|key| !key.trim().is_empty())
}

#[must_use]
pub fn uses_env_only_api_key(config: &Config) -> bool {
    has_env_api_key(config) && !has_config_api_key(config)
}

pub(crate) fn explicit_cli_api_key_override() -> Option<String> {
    if std::env::var("DEEPSEEK_API_KEY_SOURCE").as_deref() != Ok("cli") {
        return None;
    }
    std::env::var("CODEWHALE_CLI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty())
        })
}

pub fn clear_api_key() -> Result<()> {
    let path = default_config_path().context("无法解析配置路径：未找到 home 目录")?;
    if !path.exists() {
        return Ok(());
    }
    crate::config_persistence::mutate_config_document(&path, |doc| {
        crate::config_persistence::remove_document_key(doc, &["api_key"])
    })
    .with_context(|| format!("写入配置失败：{}", path.display()))?;
    log_sensitive_event(
        "credential.clear",
        json!({
            "backend": "config_file",
            "config_path": path.display().to_string(),
            "scope": "deepseek",
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
