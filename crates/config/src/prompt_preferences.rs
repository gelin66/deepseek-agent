//! Host-owned prompt preferences loaded from the existing product settings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::codewhale_home;

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
    codewhale_config_path: Option<PathBuf>,
    codewhale_home: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct SettingsPathCandidates {
    primary: Option<PathBuf>,
}

fn settings_path_candidates_from(inputs: SettingsPathInputs) -> SettingsPathCandidates {
    if let Some(config_path) = inputs.codewhale_config_path
        && let Some(parent) = config_path.parent()
    {
        return SettingsPathCandidates {
            primary: Some(parent.join(SETTINGS_FILE_NAME)),
        };
    }

    SettingsPathCandidates {
        primary: inputs
            .codewhale_home
            .map(|home| home.join(SETTINGS_FILE_NAME)),
    }
}

fn load_settings_source_from_candidates(
    candidates: SettingsPathCandidates,
) -> Result<SettingsSource> {
    let write_path = candidates.primary.as_ref().cloned().ok_or_else(|| {
        anyhow::anyhow!("Failed to resolve settings path: no config directory found.")
    })?;
    let read_path = write_path.clone();
    let content = if read_path.exists() {
        Some(
            std::fs::read_to_string(&read_path)
                .with_context(|| format!("Failed to read settings from {}", read_path.display()))?,
        )
    } else {
        None
    };

    Ok(SettingsSource {
        write_path,
        read_path,
        content,
    })
}

fn current_settings_path_candidates() -> SettingsPathCandidates {
    let codewhale_config_path = std::env::var("CODEWHALE_CONFIG_PATH")
        .ok()
        .and_then(|path| {
            let path = path.trim();
            (!path.is_empty()).then(|| expand_home_path(path))
        });
    settings_path_candidates_from(SettingsPathInputs {
        codewhale_config_path,
        codewhale_home: codewhale_home().ok(),
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

/// Return the canonical settings write path.
pub fn settings_path() -> Result<PathBuf> {
    let candidates = current_settings_path_candidates();
    candidates.primary.ok_or_else(|| {
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

    fn candidates(primary: Option<PathBuf>) -> SettingsPathCandidates {
        SettingsPathCandidates { primary }
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
    fn codewhale_config_path_owns_the_sibling_settings_source() {
        let inputs = SettingsPathInputs {
            codewhale_config_path: Some(PathBuf::from("/override/config.toml")),
            codewhale_home: Some(PathBuf::from("/home/user/.codewhale")),
        };

        let resolved = settings_path_candidates_from(inputs);

        assert_eq!(
            resolved.primary,
            Some(PathBuf::from("/override/settings.toml"))
        );
    }

    #[test]
    fn codewhale_home_is_the_only_default_settings_root() {
        let inputs = SettingsPathInputs {
            codewhale_config_path: None,
            codewhale_home: Some(PathBuf::from("/isolated")),
        };

        let resolved = settings_path_candidates_from(inputs);

        assert_eq!(
            resolved.primary,
            Some(PathBuf::from("/isolated/settings.toml"))
        );
    }

    #[test]
    fn settings_source_reads_only_the_canonical_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let primary = tmp.path().join("primary/settings.toml");
        let legacy = tmp.path().join(".deepseek/settings.toml");
        std::fs::create_dir_all(primary.parent().expect("primary parent"))
            .expect("create primary parent");
        std::fs::create_dir_all(legacy.parent().expect("legacy parent"))
            .expect("create legacy parent");
        std::fs::write(&legacy, "show_thinking = false\n").expect("legacy");
        std::fs::write(&primary, "locale = \"zh-TW\"\n").expect("primary");

        let source = load_settings_source_from_candidates(candidates(Some(primary.clone())))
            .expect("canonical source");
        assert_eq!(source.read_path(), primary);
        assert_eq!(source.write_path(), primary);
        assert_eq!(source.content(), Some("locale = \"zh-TW\"\n"));
        assert_eq!(
            std::fs::read_to_string(&legacy).expect("legacy unchanged"),
            "show_thinking = false\n"
        );
    }

    #[test]
    fn missing_file_defaults_but_read_failure_is_returned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("missing/settings.toml");
        let source = load_settings_source_from_candidates(candidates(Some(missing.clone())))
            .expect("missing settings defaults");
        assert_eq!(source.content(), None);
        assert_eq!(source.prompt_preferences(), PromptPreferences::default());

        std::fs::create_dir_all(&missing).expect("directory at settings path");
        let err = load_settings_source_from_candidates(candidates(Some(missing)))
            .expect_err("directory cannot be read as settings file");
        assert!(format!("{err:#}").contains("Failed to read settings"));
    }

    #[test]
    fn malformed_or_owned_field_type_errors_default_prompt_preferences() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.toml");

        std::fs::write(&path, "locale = [\n").expect("malformed settings");
        let malformed = load_settings_source_from_candidates(candidates(Some(path.clone())))
            .expect("read malformed settings");
        assert_eq!(malformed.prompt_preferences(), PromptPreferences::default());

        std::fs::write(&path, "show_thinking = []\n").expect("typed-invalid preferences");
        let typed_invalid = load_settings_source_from_candidates(candidates(Some(path.clone())))
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
        let source = load_settings_source_from_candidates(candidates(Some(path)))
            .expect("read settings source");
        assert_eq!(
            source.prompt_preferences(),
            PromptPreferences {
                show_thinking: true,
            }
        );
    }
}
