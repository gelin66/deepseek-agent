//! Config file path resolution and TOML persistence helpers.
//!
//! These helpers are used by command handlers and non-command UI code, so
//! persistence lives outside the command tree.
//!
//! Every `config.toml` mutation funnels through [`mutate_config_document`]:
//! the file is edited in place with `toml_edit` so unrelated comments,
//! ordering, and formatting survive, and the result is replaced atomically
//! (same-directory temp file + rename) with owner-only permissions.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::config::expand_path;

/// Parse the TOML document at `path` (an absent or empty file yields an empty
/// document), apply `mutate`, and atomically persist the result.
///
/// This is the single write path for TUI config mutations: `toml_edit` keeps
/// user comments and formatting intact, and the temp-file + rename write can
/// never leave a half-written config behind.
pub(crate) fn mutate_config_document<F>(path: &Path, mutate: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut toml_edit::DocumentMut) -> anyhow::Result<()>,
{
    let raw = if path.exists() {
        Some(
            fs::read_to_string(path)
                .with_context(|| format!("failed to read config at {}", path.display()))?,
        )
    } else {
        None
    };
    let mut document = match raw.as_deref() {
        Some(raw) if !raw.trim().is_empty() => raw
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("failed to parse config at {}", path.display()))?,
        _ => toml_edit::DocumentMut::new(),
    };
    mutate(&mut document)?;
    write_config_toml_atomic(path, &document.to_string())
}

/// Atomically replace `path` with `body` via a same-directory temp file and
/// rename. On Unix the file lands with 0o600 permissions: config.toml can
/// hold API keys, so this matches `ConfigStore::save` and the auth save path.
pub(crate) fn write_config_toml_atomic(path: &Path, body: &str) -> anyhow::Result<()> {
    use std::io::Write as _;

    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create config directory {}", parent.display()))?;

    let mut temporary = tempfile::NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "failed to create temporary config file in {}",
            parent.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| {
                format!(
                    "failed to secure temporary config file for {}",
                    path.display()
                )
            })?;
    }
    temporary
        .write_all(body.as_bytes())
        .with_context(|| format!("failed to write config at {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to sync config at {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace config at {}", path.display()))?;
    Ok(())
}

/// Set the value at `segments` (parent tables plus the final key), creating
/// missing intermediate tables. Replacing an existing value keeps its decor,
/// so comments above the key and trailing same-line comments survive.
///
/// Segments are separate strings rather than one dotted key, so workspace
/// paths that need quoting (`[projects."/path.with.dot"]`) resolve correctly.
pub(crate) fn set_document_value(
    doc: &mut toml_edit::DocumentMut,
    segments: &[&str],
    value: impl Into<toml_edit::Value>,
) -> anyhow::Result<()> {
    let (key, parents) = segments
        .split_last()
        .context("config value path must not be empty")?;
    let table = table_like_at_path_mut(doc.as_table_mut(), parents)?;
    match table.get_mut(key) {
        Some(item) => {
            let mut value = value.into();
            if let Some(existing) = item.as_value() {
                *value.decor_mut() = existing.decor().clone();
            }
            *item = toml_edit::Item::Value(value);
        }
        None => {
            table.insert(key, toml_edit::value(value));
        }
    }
    Ok(())
}

/// Remove one exact value without touching sibling tables or similarly named
/// keys. DeepSeek-only credential cleanup uses this instead of the retired
/// cross-provider recursive sweep.
pub(crate) fn remove_document_key(
    doc: &mut toml_edit::DocumentMut,
    segments: &[&str],
) -> anyhow::Result<()> {
    let (key, parents) = segments
        .split_last()
        .context("config value path must not be empty")?;
    let table = table_like_at_path_mut(doc.as_table_mut(), parents)?;
    remove_key_preserving_leading_decor(table, key);
    Ok(())
}

fn remove_key_preserving_leading_decor(table: &mut dyn toml_edit::TableLike, key: &str) -> bool {
    let mut found = false;
    let next_key = table.iter().find_map(|(candidate, _)| {
        if found {
            Some(candidate.to_owned())
        } else {
            found = candidate == key;
            None
        }
    });
    let leading_prefix = leading_prefix_for_key(table, key);
    if table.remove(key).is_none() {
        return false;
    }
    let Some(prefix) = leading_prefix else {
        return true;
    };
    let Some(next_key) = next_key else {
        return true;
    };
    if prefix.as_str() == Some("") {
        return true;
    }
    if let Some(mut next_key_decor) = table.key_mut(&next_key)
        && decor_prefix_is_empty(next_key_decor.leaf_decor())
    {
        next_key_decor.leaf_decor_mut().set_prefix(prefix);
    }
    true
}

fn decor_prefix_is_empty(decor: &toml_edit::Decor) -> bool {
    match decor.prefix() {
        Some(prefix) => prefix.as_str() == Some(""),
        None => true,
    }
}

fn leading_prefix_for_key(
    table: &dyn toml_edit::TableLike,
    key: &str,
) -> Option<toml_edit::RawString> {
    table
        .key(key)
        .and_then(|key| key.leaf_decor().prefix().cloned())
        .or_else(|| {
            table
                .get(key)
                .and_then(|item| item.as_value())
                .and_then(|value| value.decor().prefix().cloned())
        })
}

fn table_like_at_path_mut<'a>(
    root: &'a mut toml_edit::Table,
    segments: &[&str],
) -> anyhow::Result<&'a mut dyn toml_edit::TableLike> {
    let mut current: &mut dyn toml_edit::TableLike = root;
    for segment in segments {
        if current.get(segment).is_none() {
            // Implicit, so creating `projects.<workspace>.trust_level` does
            // not emit an empty `[projects]` header.
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            current.insert(segment, toml_edit::Item::Table(table));
        }
        let item = current
            .get_mut(segment)
            .expect("segment exists or was inserted above");
        match item.as_table_like_mut() {
            Some(table) => current = table,
            None => anyhow::bail!("`{segment}` in config.toml must be a table"),
        }
    }
    Ok(current)
}

pub(crate) fn persist_root_string_key(
    config_path: Option<&Path>,
    key: &str,
    value: &str,
) -> anyhow::Result<PathBuf> {
    let path = config_toml_path(config_path)?;
    mutate_config_document(&path, |doc| set_document_value(doc, &[key], value))?;
    Ok(path)
}

pub(crate) fn config_toml_path(config_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = config_path {
        return Ok(expand_path(path.to_string_lossy().as_ref()));
    }
    crate::config::resolve_load_config_path(None)
        .context("failed to resolve the active config.toml path")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        home: Option<OsString>,
        userprofile: Option<OsString>,
        codewhale_home: Option<OsString>,
        codewhale_config_path: Option<OsString>,
        retired_config_path: Option<OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new(home: &Path) -> Self {
            let lock = crate::test_support::lock_test_env();
            let home_str = OsString::from(home.as_os_str());
            let config_path = home.join(".codewhale").join("config.toml");
            let config_str = OsString::from(config_path.as_os_str());
            let home_prev = env::var_os("HOME");
            let userprofile_prev = env::var_os("USERPROFILE");
            let codewhale_home_prev = env::var_os("CODEWHALE_HOME");
            let codewhale_config_prev = env::var_os("CODEWHALE_CONFIG_PATH");
            let deepseek_config_prev = env::var_os("DEEPSEEK_CONFIG_PATH");

            // Safety: test-only environment mutation guarded by process-wide mutex.
            unsafe {
                env::set_var("HOME", &home_str);
                env::set_var("USERPROFILE", &home_str);
                env::remove_var("CODEWHALE_HOME");
                env::set_var("CODEWHALE_CONFIG_PATH", &config_str);
                env::remove_var("DEEPSEEK_CONFIG_PATH");
            }

            Self {
                home: home_prev,
                userprofile: userprofile_prev,
                codewhale_home: codewhale_home_prev,
                codewhale_config_path: codewhale_config_prev,
                retired_config_path: deepseek_config_prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.home.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("HOME", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("HOME");
                }
            }

            if let Some(value) = self.userprofile.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("USERPROFILE", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("USERPROFILE");
                }
            }

            if let Some(value) = self.codewhale_home.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("CODEWHALE_HOME", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("CODEWHALE_HOME");
                }
            }

            if let Some(value) = self.codewhale_config_path.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("CODEWHALE_CONFIG_PATH", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("CODEWHALE_CONFIG_PATH");
                }
            }

            if let Some(value) = self.retired_config_path.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("DEEPSEEK_CONFIG_PATH", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("DEEPSEEK_CONFIG_PATH");
                }
            }
        }
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn config_toml_path_uses_codewhale_home_for_fresh_installs() {
        let temp_root = temp_root("codewhale-config-path-fresh");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);

        unsafe {
            env::remove_var("CODEWHALE_CONFIG_PATH");
        }

        assert_eq!(
            config_toml_path(None).unwrap(),
            temp_root.join(".codewhale").join("config.toml")
        );
    }

    #[test]
    fn config_toml_path_ignores_legacy_config_when_it_exists() {
        let temp_root = temp_root("codewhale-config-path-legacy");
        let legacy_config = temp_root.join(".deepseek").join("config.toml");
        fs::create_dir_all(legacy_config.parent().unwrap()).unwrap();
        fs::write(&legacy_config, "").unwrap();
        let _guard = EnvGuard::new(&temp_root);

        unsafe { env::remove_var("CODEWHALE_CONFIG_PATH") };

        assert_eq!(
            config_toml_path(None).unwrap(),
            temp_root.join(".codewhale").join("config.toml")
        );
        assert!(
            legacy_config.exists(),
            "retired config must remain untouched"
        );
    }

    #[test]
    fn config_toml_path_ignores_legacy_config_when_codewhale_home_is_set() {
        let temp_root = temp_root("codewhale-config-path-explicit-home");
        let explicit_home = temp_root.join("isolated-codewhale");
        let legacy_config = temp_root.join(".deepseek").join("config.toml");
        fs::create_dir_all(legacy_config.parent().unwrap()).unwrap();
        fs::write(&legacy_config, "").unwrap();
        let _guard = EnvGuard::new(&temp_root);

        unsafe {
            env::remove_var("CODEWHALE_CONFIG_PATH");
            env::set_var("CODEWHALE_HOME", &explicit_home);
        }

        assert_eq!(
            config_toml_path(None).unwrap(),
            explicit_home.join("config.toml")
        );
    }

    #[test]
    fn config_toml_path_ignores_retired_env() {
        let temp_root = temp_root("codewhale-config-path-env");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let preferred = temp_root.join("preferred.toml");
        let legacy = temp_root.join("legacy.toml");

        unsafe {
            env::set_var("CODEWHALE_CONFIG_PATH", &preferred);
            env::set_var("DEEPSEEK_CONFIG_PATH", &legacy);
        }

        assert_eq!(config_toml_path(None).unwrap(), preferred);
    }

    #[test]
    fn config_toml_path_uses_codewhale_env_even_when_target_is_missing() {
        let temp_root = temp_root("codewhale-config-path-missing-env-fallback");
        let home_config = temp_root.join(".codewhale").join("config.toml");
        fs::create_dir_all(home_config.parent().unwrap()).unwrap();
        fs::write(&home_config, "# existing fallback\n").unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let missing_env = temp_root.join("override").join("missing.toml");

        unsafe {
            env::set_var("DEEPSEEK_CONFIG_PATH", &missing_env);
            env::remove_var("CODEWHALE_CONFIG_PATH");
        }

        assert_eq!(config_toml_path(None).unwrap(), home_config);
        assert!(!missing_env.exists());
    }

    // ------------------------------------------------------------------
    // Golden-file coverage for the shared toml_edit mutation path
    // (findings #18/#19/#20): unrelated comments, ordering, and quoted
    // workspace tables must survive every supported mutation.
    // ------------------------------------------------------------------

    const GOLDEN_CONFIG: &str = r#"# CodeWhale golden config fixture, top note.
# api_key = "sk-placeholder" (uncomment to set the key by hand)
default_text_model = "deepseek-v4-pro" # pinned for release QA

# workspace trust note
[projects."/Users/example/work"]
trust_level = "trusted" # granted manually

# second workspace note
[projects."/Users/example/quoted.project"]
trust_level = "untrusted" # keep in sync with docs
"#;

    fn write_golden_config(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, GOLDEN_CONFIG).unwrap();
    }

    #[test]
    fn golden_replacing_existing_root_value_only_touches_that_value() {
        let temp_root = temp_root("codewhale-golden-root-value");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let path = temp_root.join(".codewhale").join("config.toml");
        write_golden_config(&path);

        persist_root_string_key(Some(&path), "default_text_model", "deepseek-v4-flash")
            .expect("persist should succeed");

        let body = fs::read_to_string(&path).unwrap();
        let expected = GOLDEN_CONFIG.replace(
            "default_text_model = \"deepseek-v4-pro\" # pinned for release QA",
            "default_text_model = \"deepseek-v4-flash\" # pinned for release QA",
        );
        assert_eq!(body, expected, "only the model value may change");
    }

    #[test]
    fn golden_production_mutation_preserves_unrelated_comments_order_and_quoted_tables() {
        let temp_root = temp_root("codewhale-golden-mutations");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let path = temp_root.join(".codewhale").join("config.toml");
        write_golden_config(&path);

        mutate_config_document(&path, |doc| {
            set_document_value(doc, &["api_key"], "test-only-key")?;
            set_document_value(
                doc,
                &["projects", "/Users/example/work", "trust_level"],
                "trusted",
            )
        })
        .unwrap();
        let body = fs::read_to_string(&path).unwrap();
        for comment in [
            "# CodeWhale golden config fixture, top note.",
            "# api_key = \"sk-placeholder\" (uncomment to set the key by hand)",
            "# pinned for release QA",
            "# workspace trust note",
            "# granted manually",
            "# second workspace note",
            "# keep in sync with docs",
        ] {
            assert!(body.contains(comment), "comment lost: {comment}\n{body}");
        }
        assert!(body.contains("api_key = \"test-only-key\""), "{body}");
        // Updated in place, keeping the workspace trust trailing comment.
        assert!(
            body.contains("trust_level = \"trusted\" # granted manually"),
            "{body}"
        );
        assert!(
            body.contains("[projects.\"/Users/example/quoted.project\"]"),
            "{body}"
        );

        // Original section order is intact.
        let root_model_at = body.find("default_text_model = ").unwrap();
        let projects_at = body.find("[projects.").unwrap();
        assert!(root_model_at < projects_at, "{body}");

        let parsed: toml::Value = toml::from_str(&body).unwrap();
        assert_eq!(
            parsed.get("api_key").and_then(toml::Value::as_str),
            Some("test-only-key")
        );
        assert_eq!(
            parsed
                .get("projects")
                .and_then(|projects| projects.get("/Users/example/work"))
                .and_then(|project| project.get("trust_level"))
                .and_then(toml::Value::as_str),
            Some("trusted")
        );
    }

    #[test]
    fn set_document_value_inserts_api_key_even_when_a_comment_mentions_it() {
        // Finding #20 at the primitive level: the old string scan treated a
        // comment mentioning api_key as an existing assignment and skipped
        // the insert entirely.
        let temp_root = temp_root("codewhale-golden-api-key-comment");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let path = temp_root.join(".codewhale").join("config.toml");
        write_golden_config(&path);

        mutate_config_document(&path, |doc| {
            set_document_value(doc, &["api_key"], "sk-fresh")
        })
        .expect("mutation should succeed");

        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("# api_key = \"sk-placeholder\""),
            "comment lost: {body}"
        );
        let parsed: toml::Value = toml::from_str(&body).unwrap();
        assert_eq!(
            parsed.get("api_key").and_then(toml::Value::as_str),
            Some("sk-fresh"),
            "real key must be inserted despite the comment: {body}"
        );
    }

    #[test]
    fn set_document_value_rejects_non_table_parents() {
        let mut doc = "model = \"deepseek-v4-pro\"\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let err = set_document_value(&mut doc, &["model", "nested"], "x")
            .expect_err("scalar parent must be rejected");
        assert!(err.to_string().contains("must be a table"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn config_writes_land_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_root = temp_root("codewhale-persist-perms");
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);
        let path = temp_root.join(".codewhale").join("config.toml");
        write_golden_config(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        persist_root_string_key(Some(&path), "api_key", "test-only-key")
            .expect("persist should succeed");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.toml can hold api keys");
    }
}
