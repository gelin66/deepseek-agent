use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::CONFIG_FILE_NAME;

/// Canonical CodeWhale app directory name under $HOME.
pub const CODEWHALE_APP_DIR: &str = ".codewhale";

/// Resolve the CodeWhale home directory.
///
/// `$CODEWHALE_HOME` takes precedence when set. Otherwise defaults to
/// `$HOME/.codewhale`. This is the write target for new product state.
pub fn codewhale_home() -> Result<PathBuf> {
    if let Some(path) = codewhale_home_env_override() {
        return Ok(path);
    }
    let home = effective_home_dir().context("failed to resolve home directory")?;
    Ok(home.join(CODEWHALE_APP_DIR))
}

fn codewhale_home_env_override() -> Option<PathBuf> {
    let val = std::env::var("CODEWHALE_HOME").ok()?;
    let trimmed = val.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

#[doc(hidden)]
pub fn effective_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

/// Reject state subdirs that could escape the state root via path injection.
///
/// `ensure_state_dir` is a public API taking an arbitrary subdir string; every
/// in-tree caller passes a hardcoded single component
/// (e.g. `"sessions"`, `"."`). This validates defensively so a future caller
/// can never traverse out of the state root via `..` components or an absolute
/// path. Nested relative paths such as `"a/b"` are permitted.
fn ensure_safe_state_subdir(subdir: &str) -> Result<()> {
    if subdir.is_empty() {
        bail!("state subdir must not be empty");
    }
    let path = std::path::Path::new(subdir);
    if path.is_absolute() {
        bail!("state subdir must not be an absolute path: {subdir}");
    }
    if path.components().any(|c| {
        matches!(
            c,
            std::path::Component::RootDir | std::path::Component::Prefix(_)
        )
    }) {
        bail!("state subdir must not contain a root or prefix: {subdir}");
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("state subdir must not contain parent-dir (..) components: {subdir}");
    }
    Ok(())
}

/// Ensure a state subdirectory exists under the canonical CodeWhale root,
/// creating it if necessary. This is the write-path resolver.
pub fn ensure_state_dir(subdir: &str) -> Result<PathBuf> {
    ensure_safe_state_subdir(subdir)?;
    let dir = codewhale_home()?.join(subdir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}/", dir.display()))?;
    Ok(dir)
}

pub fn resolve_config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return normalize_config_file_path(path);
    }
    if let Ok(path) = std::env::var("CODEWHALE_CONFIG_PATH") {
        if let Some(path) = config_path_from_env_value(&path)? {
            return Ok(path);
        }
        return default_config_path();
    }
    default_config_path()
}

fn config_path_from_env_value(path: &str) -> Result<Option<PathBuf>> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        normalize_config_file_path(PathBuf::from(trimmed)).map(Some)
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(codewhale_home()?.join(CONFIG_FILE_NAME))
}

pub(crate) fn normalize_config_file_path(path: PathBuf) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("config path cannot be empty");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("config path cannot contain '..' components");
    }
    if path.file_name().is_none() {
        bail!("config path must include a file name");
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for config path")?
            .join(path)
    };
    let file_name = absolute
        .file_name()
        .map(OsString::from)
        .context("config path must include a file name")?;
    let parent = absolute
        .parent()
        .context("config path must include a parent directory")?;
    let parent = match parent.canonicalize() {
        Ok(parent) => parent,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => parent.to_path_buf(),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to resolve config directory {}", parent.display())
            });
        }
    };
    let normalized = parent.join(file_name);
    reject_path_symlink(&normalized)?;
    Ok(normalized)
}

pub(crate) fn normalize_project_workspace(workspace: &Path) -> Result<PathBuf> {
    if workspace.as_os_str().is_empty() {
        bail!("project workspace path cannot be empty");
    }
    if workspace
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("project workspace path cannot contain '..' components");
    }
    let absolute = if workspace.is_absolute() {
        workspace.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory for project workspace")?
            .join(workspace)
    };
    match absolute.canonicalize() {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_path_components(&absolute))
        }
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to resolve project workspace {}",
                workspace.display()
            )
        }),
    }
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

pub(crate) fn checked_path_exists(path: &Path) -> Result<bool> {
    let path = normalize_config_file_path(path.to_path_buf())?;
    path.try_exists()
        .with_context(|| format!("failed to inspect config path {}", path.display()))
}

pub(crate) fn read_checked_config_file(path: &Path) -> Result<String> {
    read_checked_toml_file(path, "config")
}

fn read_checked_toml_file(path: &Path, label: &str) -> Result<String> {
    let path = normalize_config_file_path(path.to_path_buf())?;
    read_string_no_follow(&path)
        .with_context(|| format!("failed to read {label} at {}", path.display()))
}

#[cfg(unix)]
fn read_string_no_follow(path: &Path) -> std::io::Result<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(raw)
}

#[cfg(not(unix))]
fn read_string_no_follow(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path)
}

pub(crate) fn reject_path_symlink(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        bail!("config path must not be a symlink: {}", path.display());
    }
    Ok(())
}
