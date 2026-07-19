//! Read-only per-workspace trust snapshot of external paths that production
//! tools may access without triggering a `PathEscape` error (#29).
//!
//! Storage: `~/.deepseek/workspace-trust.json`. The file is a JSON object
//! mapping each workspace's canonical path to a sorted list of canonical
//! paths the user has explicitly trusted from that workspace. Trust granted
//! in workspace A does not apply when running from workspace B.
//! CodeWhale no longer exposes a command that mutates this historical file;
//! existing data is still read and is never deleted during cutover.
//!
//! Threat model: this is a deliberate user opt-in to a path the workspace
//! sandbox would otherwise refuse. The only access the trust list grants is
//! through CodeWhale's own file tools (`read_file`, `write_file`, etc.) —
//! it does not loosen the OS sandbox profile (Seatbelt/Landlock) used for
//! shell commands. Sandbox-profile expansion is tracked separately so a
//! shell tool can opt into the same paths in a future release.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const TRUST_FILE_NAME: &str = "workspace-trust.json";

#[derive(Debug, Default, Clone, Deserialize)]
struct TrustFile {
    /// Map workspace canonical path → sorted unique trusted paths.
    #[serde(default)]
    workspaces: BTreeMap<String, Vec<String>>,
}

/// In-memory trust list for a single workspace, snapshotted at load time.
/// Production tool configuration consumes the loaded canonical paths.
#[derive(Debug, Default, Clone)]
pub struct WorkspaceTrust {
    paths: Vec<PathBuf>,
}

impl WorkspaceTrust {
    #[must_use]
    pub fn empty() -> Self {
        Self { paths: Vec::new() }
    }

    /// Load the trusted-paths snapshot for `workspace` from disk. Missing or
    /// malformed files yield an empty list rather than an error so a corrupt
    /// trust file never wedges the TUI; the next mutation rewrites it.
    #[must_use]
    pub fn load_for(workspace: &Path) -> Self {
        match trust_file_path() {
            Some(path) => Self::load_from_file(workspace, &path),
            None => Self::empty(),
        }
    }

    fn load_from_file(workspace: &Path, file_path: &Path) -> Self {
        let key = workspace_key(workspace);
        let file = read_trust_file_at(file_path).unwrap_or_default();
        let paths = file
            .workspaces
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect();
        Self { paths }
    }

    /// Return the trusted paths in canonical form.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

fn workspace_key(workspace: &Path) -> String {
    canonicalize_or_keep(workspace)
        .to_string_lossy()
        .into_owned()
}

fn canonicalize_or_keep(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn trust_file_path() -> Option<PathBuf> {
    codewhale_config::ensure_state_dir(".")
        .ok()
        .map(|dir| dir.join(TRUST_FILE_NAME))
}

fn read_trust_file_at(path: &Path) -> Result<TrustFile> {
    if !path.exists() {
        return Ok(TrustFile::default());
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Set up an isolated fake `~/.deepseek/workspace-trust.json` location.
    /// Returns the tmpdir (kept alive for the test) plus the explicit trust
    /// file path passed to the `*_at` helpers — avoids touching `$HOME` so
    /// tests run safely in parallel.
    fn isolated_trust_path() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let trust_path = tmp.path().join(".deepseek").join("workspace-trust.json");
        (tmp, trust_path)
    }

    #[test]
    fn empty_trust_for_unknown_workspace() {
        let (tmp, trust_path) = isolated_trust_path();
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let trust = WorkspaceTrust::load_from_file(&workspace, &trust_path);
        assert!(trust.paths().is_empty());
    }

    #[test]
    fn existing_trust_file_is_workspace_scoped() {
        let (tmp, trust_path) = isolated_trust_path();
        let ws_a = tmp.path().join("ws-a");
        let ws_b = tmp.path().join("ws-b");
        let other = tmp.path().join("data/notes");
        std::fs::create_dir_all(&ws_a).unwrap();
        std::fs::create_dir_all(&ws_b).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        let workspace_key = workspace_key(&ws_a);
        let canonical_other = canonicalize_or_keep(&other).to_string_lossy().into_owned();
        let mut workspaces = serde_json::Map::new();
        workspaces.insert(workspace_key, serde_json::json!([canonical_other]));
        let fixture = serde_json::json!({ "workspaces": workspaces });
        std::fs::create_dir_all(trust_path.parent().unwrap()).unwrap();
        std::fs::write(&trust_path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();

        let trust = WorkspaceTrust::load_from_file(&ws_a, &trust_path);
        assert_eq!(trust.paths().len(), 1);
        assert_eq!(trust.paths()[0], canonicalize_or_keep(&other));
        assert_eq!(
            WorkspaceTrust::load_from_file(&ws_b, &trust_path)
                .paths()
                .len(),
            0
        );
    }
}
