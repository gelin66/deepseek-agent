//! Host-owned prompt preferences loaded from the existing product settings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::{codewhale_home, codewhale_home_is_explicit, legacy_deepseek_home};

const SETTINGS_FILE_NAME: &str = "settings.toml";

/// The small settings subset needed before a presentation client exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptPreferences {
    /// Whether model reasoning should be projected to human-facing output.
    pub show_thinking: bool,
}

/// One settings-path decision and at most one file read, reusable by hosts.
#[derive(Debug, Clone)]
pub struct SettingsSource {
    write_path: PathBuf,
    read_path: PathBuf,
    content: Option<String>,
}

impl SettingsSource {
    #[must_use]
    pub fn write_path(&self) -> &Path {
        &self.write_path
    }

    #[must_use]
    pub fn read_path(&self) -> &Path {
        &self.read_path
    }

    #[must_use]
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    #[must_use]
    pub fn prompt_preferences(&self) -> PromptPreferences {
        let Some(content) = self.content() else {
            return PromptPreferences::default();
        };
        let Ok(settings) = toml::from_str::<PromptPreferencesWire>(content) else {
            return PromptPreferences::default();
        };

        PromptPreferences {
            show_thinking: settings.show_thinking.unwrap_or(false),
        }
    }

    /// Deserialize the exact bytes selected by the shared settings resolver.
    ///
    /// A missing file is `Ok(None)`. Callers retain ownership of their wider
    /// settings defaults and normalization, while path and read precedence stay
    /// single-owned here.
    pub fn deserialize<T>(&self) -> std::result::Result<Option<T>, toml::de::Error>
    where
        T: DeserializeOwned,
    {
        self.content().map(toml::from_str).transpose()
    }

    /// Whether a syntactically valid settings table explicitly contains `key`.
    #[must_use]
    pub fn contains_top_level_key(&self, key: &str) -> bool {
        let Some(content) = self.content() else {
            return false;
        };
        toml::from_str::<toml::Value>(content)
            .ok()
            .is_some_and(|value| {
                value
                    .as_table()
                    .is_some_and(|table| table.contains_key(key))
            })
    }
}

/// Minimal wire shape owned by [`PromptPreferences`]. Unknown settings remain
/// outside this boundary and cannot invalidate host prompt preferences.
#[derive(Debug, Deserialize)]
struct PromptPreferencesWire {
    show_thinking: Option<bool>,
}

#[derive(Debug, Clone)]
struct SettingsPathInputs {
    deepseek_config_path: Option<PathBuf>,
    codewhale_home: Option<PathBuf>,
    codewhale_home_is_explicit: bool,
    legacy_deepseek_home: Option<PathBuf>,
    platform_config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct SettingsPathCandidates {
    primary: Option<PathBuf>,
    legacy_home: Option<PathBuf>,
    legacy_config_dir: Option<PathBuf>,
}

fn settings_path_candidates_from(inputs: SettingsPathInputs) -> SettingsPathCandidates {
    if let Some(config_path) = inputs.deepseek_config_path
        && let Some(parent) = config_path.parent()
    {
        return SettingsPathCandidates {
            primary: Some(parent.join(SETTINGS_FILE_NAME)),
            legacy_home: None,
            legacy_config_dir: None,
        };
    }

    let primary = inputs
        .codewhale_home
        .map(|home| home.join(SETTINGS_FILE_NAME));
    if inputs.codewhale_home_is_explicit {
        return SettingsPathCandidates {
            primary,
            legacy_home: None,
            legacy_config_dir: None,
        };
    }

    SettingsPathCandidates {
        primary,
        legacy_home: inputs
            .legacy_deepseek_home
            .map(|home| home.join(SETTINGS_FILE_NAME)),
        legacy_config_dir: inputs
            .platform_config_dir
            .map(|dir| dir.join("deepseek").join(SETTINGS_FILE_NAME)),
    }
}

fn load_settings_source_from_candidates(
    candidates: SettingsPathCandidates,
) -> Result<SettingsSource> {
    let write_path = candidates
        .primary
        .as_ref()
        .cloned()
        .or_else(|| candidates.legacy_config_dir.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("Failed to resolve settings path: no config directory found.")
        })?;
    let read_path = resolve_read_path(&candidates).unwrap_or_else(|| write_path.clone());
    let content = if read_path.exists() {
        Some(
            std::fs::read_to_string(&read_path)
                .with_context(|| format!("Failed to read settings from {}", read_path.display()))?,
        )
    } else {
        None
    };

    migrate_settings_file_to_primary_if_needed(&write_path, &read_path);
    Ok(SettingsSource {
        write_path,
        read_path,
        content,
    })
}

fn resolve_read_path(candidates: &SettingsPathCandidates) -> Option<PathBuf> {
    candidates
        .primary
        .as_ref()
        .filter(|path| path.exists())
        .cloned()
        .or_else(|| {
            candidates
                .legacy_home
                .as_ref()
                .filter(|path| path.exists())
                .cloned()
        })
        .or_else(|| {
            candidates
                .legacy_config_dir
                .as_ref()
                .filter(|path| path.exists())
                .cloned()
        })
}

fn current_settings_path_candidates() -> SettingsPathCandidates {
    let deepseek_config_path = std::env::var("DEEPSEEK_CONFIG_PATH").ok().and_then(|path| {
        let path = path.trim();
        (!path.is_empty()).then(|| expand_home_path(path))
    });
    settings_path_candidates_from(SettingsPathInputs {
        deepseek_config_path,
        codewhale_home: codewhale_home().ok(),
        codewhale_home_is_explicit: codewhale_home_is_explicit(),
        legacy_deepseek_home: legacy_deepseek_home().ok(),
        platform_config_dir: dirs::config_dir(),
    })
}

fn expand_home_path(path: &str) -> PathBuf {
    let Some(stripped) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    if !(stripped.is_empty() || stripped.starts_with('/') || stripped.starts_with('\\')) {
        return PathBuf::from(path);
    }
    let Some(mut home) = crate::effective_home_dir() else {
        return PathBuf::from(path);
    };
    let suffix = stripped.trim_start_matches(['/', '\\']);
    if !suffix.is_empty() {
        home.push(suffix);
    }
    home
}

fn migrate_settings_file_to_primary_if_needed(primary: &Path, active_read_path: &Path) {
    if primary == active_read_path || primary.exists() || !active_read_path.exists() {
        return;
    }

    let Some(parent) = primary.parent() else {
        return;
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        tracing::warn!(
            "failed to create settings migration directory {}: {err}",
            parent.display()
        );
        return;
    }
    if let Err(err) = std::fs::copy(active_read_path, primary) {
        tracing::warn!(
            "failed to migrate settings from {} to {}: {err}",
            active_read_path.display(),
            primary.display()
        );
    }
}

/// Return the canonical settings write path.
pub fn settings_path() -> Result<PathBuf> {
    let candidates = current_settings_path_candidates();
    candidates
        .primary
        .or(candidates.legacy_config_dir)
        .ok_or_else(|| {
            anyhow::anyhow!("Failed to resolve settings path: no config directory found.")
        })
}

/// Resolve and read the settings source exactly once.
pub fn load_settings_source() -> Result<SettingsSource> {
    load_settings_source_from_candidates(current_settings_path_candidates())
}

/// Load host-owned prompt preferences from the existing settings source.
pub fn load_prompt_preferences() -> Result<PromptPreferences> {
    Ok(load_settings_source()?.prompt_preferences())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(
        primary: Option<PathBuf>,
        legacy_home: Option<PathBuf>,
        legacy_config_dir: Option<PathBuf>,
    ) -> SettingsPathCandidates {
        SettingsPathCandidates {
            primary,
            legacy_home,
            legacy_config_dir,
        }
    }

    #[test]
    fn prompt_preferences_default_to_hidden_thinking() {
        assert_eq!(
            PromptPreferences::default(),
            PromptPreferences {
                show_thinking: false,
            }
        );
    }

    #[test]
    fn deepseek_config_path_owns_the_sibling_settings_source() {
        let inputs = SettingsPathInputs {
            deepseek_config_path: Some(PathBuf::from("/override/config.toml")),
            codewhale_home: Some(PathBuf::from("/home/user/.codewhale")),
            codewhale_home_is_explicit: false,
            legacy_deepseek_home: Some(PathBuf::from("/home/user/.deepseek")),
            platform_config_dir: Some(PathBuf::from("/platform")),
        };

        let resolved = settings_path_candidates_from(inputs);

        assert_eq!(
            resolved.primary,
            Some(PathBuf::from("/override/settings.toml"))
        );
        assert_eq!(resolved.legacy_home, None);
        assert_eq!(resolved.legacy_config_dir, None);
    }

    #[test]
    fn explicit_codewhale_home_is_an_isolation_boundary() {
        let inputs = SettingsPathInputs {
            deepseek_config_path: None,
            codewhale_home: Some(PathBuf::from("/isolated")),
            codewhale_home_is_explicit: true,
            legacy_deepseek_home: Some(PathBuf::from("/ambient/.deepseek")),
            platform_config_dir: Some(PathBuf::from("/platform")),
        };

        let resolved = settings_path_candidates_from(inputs);

        assert_eq!(
            resolved.primary,
            Some(PathBuf::from("/isolated/settings.toml"))
        );
        assert_eq!(resolved.legacy_home, None);
        assert_eq!(resolved.legacy_config_dir, None);
    }

    #[test]
    fn settings_source_prefers_primary_then_home_legacy_then_platform_legacy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join("primary/settings.toml");
        let legacy_home = tmp.path().join("home-legacy/settings.toml");
        let platform = tmp.path().join("platform/deepseek/settings.toml");
        for path in [&primary, &legacy_home, &platform] {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        }
        std::fs::write(&platform, "locale = \"es-MX\"\n").expect("platform");
        std::fs::write(&legacy_home, "locale = \"ja\"\n").expect("legacy home");
        std::fs::write(&primary, "locale = \"zh-TW\"\n").expect("primary");

        let all = load_settings_source_from_candidates(candidates(
            Some(primary.clone()),
            Some(legacy_home.clone()),
            Some(platform.clone()),
        ))
        .expect("all candidates");
        assert_eq!(all.read_path(), primary);

        std::fs::remove_file(&primary).expect("remove primary");
        let without_primary = load_settings_source_from_candidates(candidates(
            Some(primary.clone()),
            Some(legacy_home.clone()),
            Some(platform.clone()),
        ))
        .expect("legacy home candidate");
        assert_eq!(without_primary.read_path(), legacy_home);

        std::fs::remove_file(&legacy_home).expect("remove legacy home");
        std::fs::remove_file(&primary).expect("remove migrated primary");
        let platform_only = load_settings_source_from_candidates(candidates(
            Some(primary),
            Some(legacy_home),
            Some(platform.clone()),
        ))
        .expect("platform candidate");
        assert_eq!(platform_only.read_path(), platform);
    }

    #[test]
    fn fallback_load_reuses_selected_bytes_and_migrates_them_to_primary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join("primary/settings.toml");
        let legacy = tmp.path().join("legacy/settings.toml");
        std::fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy directory");
        let body = "locale = \"ja\"\nshow_thinking = true\n";
        std::fs::write(&legacy, body).expect("legacy settings");

        let source = load_settings_source_from_candidates(candidates(
            Some(primary.clone()),
            Some(legacy.clone()),
            None,
        ))
        .expect("legacy source");

        assert_eq!(source.read_path(), legacy);
        assert_eq!(source.write_path(), primary);
        assert_eq!(source.content(), Some(body));
        assert_eq!(
            std::fs::read_to_string(&primary).expect("migrated primary"),
            body
        );
        assert_eq!(
            source.prompt_preferences(),
            PromptPreferences {
                show_thinking: true,
            }
        );
    }

    #[test]
    fn missing_file_defaults_but_read_failure_is_returned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("missing/settings.toml");
        let source =
            load_settings_source_from_candidates(candidates(Some(missing.clone()), None, None))
                .expect("missing settings defaults");
        assert_eq!(source.content(), None);
        assert_eq!(source.prompt_preferences(), PromptPreferences::default());

        std::fs::create_dir_all(&missing).expect("directory at settings path");
        let err = load_settings_source_from_candidates(candidates(Some(missing), None, None))
            .expect_err("directory cannot be read as settings file");
        assert!(format!("{err:#}").contains("Failed to read settings"));
    }

    #[test]
    fn malformed_or_owned_field_type_errors_default_prompt_preferences() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.toml");

        std::fs::write(&path, "locale = [\n").expect("malformed settings");
        let malformed =
            load_settings_source_from_candidates(candidates(Some(path.clone()), None, None))
                .expect("read malformed settings");
        assert_eq!(malformed.prompt_preferences(), PromptPreferences::default());

        std::fs::write(&path, "show_thinking = []\n").expect("typed-invalid preferences");
        let typed_invalid =
            load_settings_source_from_candidates(candidates(Some(path.clone()), None, None))
                .expect("read typed-invalid preferences");
        assert_eq!(
            typed_invalid.prompt_preferences(),
            PromptPreferences::default(),
            "an invalid owned field defaults the small preference document"
        );
    }

    #[test]
    fn unrelated_field_type_errors_do_not_pollute_prompt_preferences() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.toml");
        std::fs::write(
            &path,
            "future_setting = []\nshow_thinking = true\nsidebar_width_percent = \"wide\"\n",
        )
        .expect("unrelated typed-invalid setting");
        let source = load_settings_source_from_candidates(candidates(Some(path), None, None))
            .expect("read settings source");
        assert_eq!(
            source.prompt_preferences(),
            PromptPreferences {
                show_thinking: true,
            }
        );
    }
}
