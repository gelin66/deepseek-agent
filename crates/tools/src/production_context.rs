use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tokio_util::sync::CancellationToken;

use crate::ToolError;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReadSnapshot {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
struct FileReadTracker {
    reads: HashMap<PathBuf, FileReadSnapshot>,
}

type SharedFileReadTracker = Arc<Mutex<FileReadTracker>>;

/// Workspace-bound state shared by the production tool implementations.
///
/// This context deliberately contains only state that is independent from
/// the TUI runtime. Clones share read-before-edit freshness state while all
/// policy values remain ordinary snapshot values for the active tool run.
#[derive(Debug, Clone)]
pub struct ProductionToolContext {
    /// Workspace root used to resolve relative tool paths.
    workspace: PathBuf,
    /// Allow paths outside the workspace without validation.
    trust_mode: bool,
    /// Explicit external roots that tools may access outside the workspace.
    trusted_external_paths: Vec<PathBuf>,
    /// Allow workspace symlinks to resolve to targets outside the workspace.
    follow_symlinks: bool,
    /// Cancellation signal for the active tool execution, when present.
    cancel_token: Option<CancellationToken>,
    /// Whether approval checks may be skipped for eligible tool operations.
    auto_approve: bool,
    file_read_tracker: SharedFileReadTracker,
}

impl ProductionToolContext {
    /// Create a context with restrictive path and approval defaults.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            trust_mode: false,
            trusted_external_paths: Vec::new(),
            follow_symlinks: false,
            cancel_token: None,
            auto_approve: false,
            file_read_tracker: Arc::new(Mutex::new(FileReadTracker::default())),
        }
    }

    /// Workspace root for relative tool paths.
    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Whether unrestricted paths are enabled.
    #[must_use]
    pub fn trust_mode(&self) -> bool {
        self.trust_mode
    }

    /// Explicit external roots trusted by the user.
    #[must_use]
    pub fn trusted_external_paths(&self) -> &[PathBuf] {
        &self.trusted_external_paths
    }

    /// Whether workspace symlinks may resolve outside the workspace.
    #[must_use]
    pub fn follow_symlinks(&self) -> bool {
        self.follow_symlinks
    }

    /// Cancellation token for the active invocation.
    #[must_use]
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.cancel_token.as_ref()
    }

    /// Whether eligible operations may bypass approval prompts.
    #[must_use]
    pub fn auto_approve(&self) -> bool {
        self.auto_approve
    }

    /// Set unrestricted path access for this context snapshot.
    #[must_use]
    pub fn with_trust_mode(mut self, trust_mode: bool) -> Self {
        self.trust_mode = trust_mode;
        self
    }

    /// Set the user-approved external roots for this context snapshot.
    #[must_use]
    pub fn with_trusted_external_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.trusted_external_paths = paths;
        self
    }

    /// Set workspace symlink traversal for this context snapshot.
    #[must_use]
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    /// Set the approval posture for this context snapshot.
    #[must_use]
    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.auto_approve = auto_approve;
        self
    }

    /// Create an invocation snapshot with a new cancellation signal.
    ///
    /// The new snapshot deliberately shares read-before-edit freshness with
    /// the session context because it addresses the same workspace revision.
    #[must_use]
    pub fn for_invocation(&self, cancellation: CancellationToken) -> Self {
        let mut context = self.clone();
        context.cancel_token = Some(cancellation);
        context
    }

    /// Rebind this context to another workspace and discard old read evidence.
    ///
    /// A child-agent worktree is a different filesystem view. Carrying the
    /// parent's freshness snapshots into it could authorize an edit against a
    /// file that was never read in the child workspace.
    pub fn rebind_workspace(&mut self, workspace: impl Into<PathBuf>) {
        self.workspace = workspace.into();
        self.file_read_tracker = Arc::new(Mutex::new(FileReadTracker::default()));
    }

    /// Remember the current on-disk state after a successful file read.
    ///
    /// Tracking is best-effort: a metadata race or poisoned tracker must not
    /// turn an otherwise successful read into a tool failure.
    pub fn note_file_read(&self, path: &Path) {
        let Ok(snapshot) = file_read_snapshot(path) else {
            return;
        };
        let Ok(mut tracker) = self.file_read_tracker.lock() else {
            return;
        };
        tracker.reads.insert(path.to_path_buf(), snapshot);
    }

    /// Require a successful and still-current read before a narrow edit.
    pub fn require_fresh_file_read(
        &self,
        path: &Path,
        requested_path: &str,
    ) -> Result<(), ToolError> {
        let prior = {
            let tracker = self.file_read_tracker.lock().map_err(|_| {
                ToolError::execution_failed(
                    "Failed to check read-before-edit state: tracker lock poisoned".to_string(),
                )
            })?;
            tracker.reads.get(path).cloned()
        };

        let Some(prior) = prior else {
            return Err(ToolError::execution_failed(format!(
                "Refusing edit_file for {} because it has not been read in this session. \
                 Recovery: call read_file with path=\"{requested_path}\" to inspect the current contents, \
                 then retry edit_file with a unique search string.",
                path.display()
            )));
        };

        let current = file_read_snapshot(path).map_err(|error| {
            ToolError::execution_failed(format!(
                "Refusing edit_file for {} because the file could not be checked for staleness ({error}). \
                 Recovery: call read_file with path=\"{requested_path}\" again, then retry edit_file.",
                path.display()
            ))
        })?;

        if current != prior {
            return Err(ToolError::execution_failed(format!(
                "Refusing edit_file for {} because it changed since the last read_file call. \
                 Recovery: call read_file with path=\"{requested_path}\" again and retry with the current contents.",
                path.display()
            )));
        }

        Ok(())
    }

    /// Resolve a path relative to the workspace and reject path escapes.
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace.join(raw)
        };

        if self.trust_mode {
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }

        let workspace_canonical = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());

        if self.follow_symlinks {
            let candidate_normalized = normalize_path(&candidate);
            let workspace_normalized = normalize_path(&self.workspace);
            let workspace_canonical_normalized = normalize_path(&workspace_canonical);

            if candidate_normalized.starts_with(&workspace_normalized)
                || candidate_normalized.starts_with(&workspace_canonical_normalized)
            {
                if candidate.exists() {
                    return Ok(candidate.canonicalize().unwrap_or(candidate));
                }
                return self.resolve_nonexistent_path(candidate, &workspace_canonical);
            }
        }

        let candidate_canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| normalize_path(&candidate));
        let workspace_normalized = normalize_path(&workspace_canonical);

        if !candidate_canonical.starts_with(&workspace_normalized) {
            let workspace_plain = normalize_path(&self.workspace);
            let candidate_normalized = normalize_path(&candidate);
            if !candidate_normalized.starts_with(&workspace_plain)
                && !self.is_trusted_external_path(&candidate_canonical)
                && !self.is_trusted_external_path(&candidate_normalized)
            {
                return Err(ToolError::PathEscape {
                    path: candidate_canonical,
                });
            }
        }

        if candidate.exists() {
            let canonical = candidate.canonicalize().map_err(|error| {
                ToolError::execution_failed(format!(
                    "Failed to canonicalize {}: {}",
                    candidate.display(),
                    error
                ))
            })?;

            if !canonical.starts_with(&workspace_canonical)
                && !self.is_trusted_external_path(&canonical)
            {
                return Err(ToolError::PathEscape { path: canonical });
            }

            return Ok(canonical);
        }

        self.resolve_nonexistent_path(candidate, &workspace_canonical)
    }

    fn resolve_nonexistent_path(
        &self,
        candidate: PathBuf,
        workspace_canonical: &Path,
    ) -> Result<PathBuf, ToolError> {
        let workspace_normalized = normalize_path(workspace_canonical);
        let workspace_plain = normalize_path(&self.workspace);
        let mut existing_ancestor = candidate.clone();
        let mut suffix_parts: Vec<std::ffi::OsString> = Vec::new();

        while !existing_ancestor.exists() {
            if let Some(file_name) = existing_ancestor.file_name() {
                suffix_parts.push(file_name.to_owned());
            }
            match existing_ancestor.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    existing_ancestor = parent.to_path_buf();
                }
                _ => break,
            }
        }
        let ancestor_normalized = normalize_path(&existing_ancestor);

        let canonical_ancestor = if existing_ancestor.exists() {
            existing_ancestor
                .canonicalize()
                .unwrap_or(existing_ancestor)
        } else {
            existing_ancestor
        };

        let mut canonical = canonical_ancestor;
        for part in suffix_parts.into_iter().rev() {
            canonical.push(part);
        }
        let canonical = normalize_path(&canonical);

        if self.follow_symlinks
            && (ancestor_normalized.starts_with(&workspace_plain)
                || ancestor_normalized.starts_with(&workspace_normalized))
        {
            return Ok(canonical);
        }

        if !canonical.starts_with(workspace_canonical)
            && !canonical.starts_with(&workspace_normalized)
            && !self.is_trusted_external_path(&canonical)
        {
            return Err(ToolError::PathEscape { path: canonical });
        }

        Ok(canonical)
    }

    fn is_trusted_external_path(&self, path: &Path) -> bool {
        self.trusted_external_paths
            .iter()
            .any(|trusted| path.starts_with(trusted))
    }
}

fn file_read_snapshot(path: &Path) -> Result<FileReadSnapshot, ToolError> {
    let metadata = fs::metadata(path).map_err(|error| {
        ToolError::execution_failed(format!("Failed to inspect {}: {error}", path.display()))
    })?;
    Ok(FileReadSnapshot {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut prefix: Option<std::ffi::OsString> = None;
    let mut is_root = false;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix = Some(prefix_component.as_os_str().to_owned());
            }
            Component::RootDir => {
                is_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let parent = Component::ParentDir.as_os_str();
                if let Some(last) = stack.pop() {
                    if last == parent {
                        stack.push(last);
                        stack.push(parent.to_owned());
                    }
                } else if !is_root {
                    stack.push(parent.to_owned());
                }
            }
            Component::Normal(part) => {
                stack.push(part.to_owned());
            }
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if is_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in stack {
        normalized.push(part);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    #[test]
    fn resolves_relative_and_parent_component_workspace_paths() {
        let workspace = tempdir().expect("workspace");
        let file = workspace.path().join("source.rs");
        fs::write(&file, "source").expect("write source");
        let context = ProductionToolContext::new(workspace.path());

        assert_eq!(
            context.resolve_path("source.rs").expect("relative path"),
            file.canonicalize().expect("canonical source")
        );
        assert!(
            context
                .resolve_path("new/../safe.txt")
                .expect("parent component path")
                .starts_with(
                    workspace
                        .path()
                        .canonicalize()
                        .expect("canonical workspace")
                )
        );
    }

    #[test]
    fn rejects_paths_outside_workspace_by_default() {
        let workspace = tempdir().expect("workspace");
        let outside = tempdir().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").expect("write outside file");
        let context = ProductionToolContext::new(workspace.path());

        assert!(matches!(
            context.resolve_path(outside_file.to_str().expect("utf-8 path")),
            Err(ToolError::PathEscape { .. })
        ));
        assert!(matches!(
            context.resolve_path("../escape.txt"),
            Err(ToolError::PathEscape { .. })
        ));
    }

    #[test]
    fn trust_modes_allow_explicit_external_paths() {
        let workspace = tempdir().expect("workspace");
        let outside = tempdir().expect("outside");
        let outside_file = outside.path().join("notes.md");
        fs::write(&outside_file, "notes").expect("write outside file");

        let explicit = ProductionToolContext::new(workspace.path())
            .with_trusted_external_paths(vec![outside.path().canonicalize().expect("canonical")]);
        assert_eq!(
            explicit
                .resolve_path(outside_file.to_str().expect("utf-8 path"))
                .expect("trusted path"),
            outside_file.canonicalize().expect("canonical file")
        );

        let unrestricted = ProductionToolContext::new(workspace.path()).with_trust_mode(true);
        assert!(
            unrestricted
                .resolve_path(outside.path().to_str().unwrap())
                .is_ok()
        );
    }

    #[test]
    #[cfg(unix)]
    fn following_workspace_symlink_is_explicit() {
        let root = tempdir().expect("root");
        let workspace = root.path().join("workspace");
        let target = root.path().join("outside/target");
        fs::create_dir_all(&workspace).expect("workspace directory");
        fs::create_dir_all(&target).expect("target directory");
        symlink(&target, workspace.join("linked")).expect("symlink");

        let restrictive = ProductionToolContext::new(&workspace);
        assert!(matches!(
            restrictive.resolve_path("linked/new.txt"),
            Err(ToolError::PathEscape { .. })
        ));

        let following = restrictive.with_follow_symlinks(true);
        assert_eq!(
            following
                .resolve_path("linked/new.txt")
                .expect("follow workspace symlink"),
            target
                .canonicalize()
                .expect("canonical target")
                .join("new.txt")
        );
    }

    #[test]
    fn clones_share_read_freshness_and_detect_stale_files() {
        let workspace = tempdir().expect("workspace");
        let file = workspace.path().join("source.rs");
        fs::write(&file, "one").expect("initial write");
        let context = ProductionToolContext::new(workspace.path());

        assert!(
            context
                .require_fresh_file_read(&file, "source.rs")
                .expect_err("unread file")
                .to_string()
                .contains("not been read")
        );

        context.clone().note_file_read(&file);
        context
            .require_fresh_file_read(&file, "source.rs")
            .expect("shared fresh read");

        fs::write(&file, "a longer replacement").expect("external write");
        assert!(
            context
                .require_fresh_file_read(&file, "source.rs")
                .expect_err("stale file")
                .to_string()
                .contains("changed since")
        );
    }

    #[test]
    fn carries_active_cancellation_state() {
        let token = CancellationToken::new();
        let context = ProductionToolContext::new(".").for_invocation(token.clone());

        assert!(!context.cancellation_token().unwrap().is_cancelled());
        token.cancel();
        assert!(context.cancellation_token().unwrap().is_cancelled());
    }

    #[test]
    fn invocation_snapshot_shares_freshness_but_replaces_cancellation() {
        let workspace = tempdir().expect("workspace");
        let file = workspace.path().join("source.rs");
        fs::write(&file, "one").expect("write");
        let context = ProductionToolContext::new(workspace.path());
        context.note_file_read(&file);

        let cancellation = CancellationToken::new();
        let invocation = context.for_invocation(cancellation.clone());
        invocation
            .require_fresh_file_read(&file, "source.rs")
            .expect("invocation shares freshness");
        assert!(context.cancellation_token().is_none());
        assert!(!invocation.cancellation_token().unwrap().is_cancelled());
        cancellation.cancel();
        assert!(invocation.cancellation_token().unwrap().is_cancelled());
    }

    #[test]
    fn workspace_rebind_clears_read_freshness() {
        let parent = tempdir().expect("parent workspace");
        let child = tempdir().expect("child workspace");
        let parent_file = parent.path().join("source.rs");
        let child_file = child.path().join("source.rs");
        fs::write(&parent_file, "parent").expect("parent write");
        fs::write(&child_file, "child").expect("child write");

        let context = ProductionToolContext::new(parent.path());
        context.note_file_read(&parent_file);
        let mut rebound = context.clone();
        rebound.rebind_workspace(child.path());

        context
            .require_fresh_file_read(&parent_file, "source.rs")
            .expect("parent keeps its freshness");
        assert!(
            rebound
                .require_fresh_file_read(&child_file, "source.rs")
                .expect_err("rebound context must read child file")
                .to_string()
                .contains("not been read")
        );
    }
}
