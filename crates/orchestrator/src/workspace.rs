use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use fs2::FileExt as _;
use sha2::{Digest, Sha256};
use thiserror::Error;

const WRITER_BRANCH_PREFIX: &str = "refs/heads/dse/writer/";
const MAX_GIT_POINTER_BYTES: u64 = 16 * 1024;
const MAX_CLEANUP_PATH_BYTES_PER_ENTRY: usize = 16 * 1024;
const MAX_CLEANUP_PATH_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_REGISTERED_WORKTREE_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_REGISTERED_WORKTREES: usize = 4_096;
const HOST_MAX_CHANGED_FILES: usize = 512;
const HOST_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const HOST_MAX_TOTAL_FILE_BYTES: u64 = 32 * 1024 * 1024;
const HOST_MAX_DIFF_BYTES: usize = 16 * 1024 * 1024;
const HOST_MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const BOUNDED_GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const BOUNDED_GIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const INTEGRATION_LEASE_FILE: &str = "dse-integration.lease";
const INTEGRATION_HEAD_LOCK_VERSION: &str = "dse-integration-head-lock-v1";
const CLEANUP_HEAD_LOCK_VERSION: &str = "dse-cleanup-head-lock-v1";
const MAX_INTEGRATION_LOCK_BYTES: u64 = 4 * 1024;
static NEXT_INTEGRATION_LOCK_TEMP: AtomicU64 = AtomicU64::new(0);

/// Stable failure categories for the Host-facing Git workspace boundary.
#[derive(Debug, Error)]
pub enum GitWorkspaceError {
    #[error("invalid repository: {0}")]
    InvalidRepository(String),
    #[error("repository is dirty: {0}")]
    DirtyRepository(String),
    #[error("unsupported repository shape: {0}")]
    UnsupportedRepository(String),
    #[error("invalid writer workspace request: {0}")]
    InvalidRequest(String),
    #[error("workspace ownership mismatch: {0}")]
    OwnershipMismatch(String),
    #[error("workspace conflict: {0}")]
    Conflict(String),
    #[error("workspace limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("writer produced no diff")]
    EmptyDiff,
    #[error("Git operation `{operation}` failed ({status:?}): {detail}")]
    Git {
        operation: String,
        status: Option<i32>,
        detail: String,
    },
    #[error("filesystem operation `{operation}` failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

type Result<T> = std::result::Result<T, GitWorkspaceError>;

/// Bounds applied before the Host turns a writer worktree into a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SealLimits {
    pub max_changed_files: usize,
    pub max_file_bytes: u64,
    pub max_total_file_bytes: u64,
    pub max_diff_bytes: usize,
}

impl Default for SealLimits {
    fn default() -> Self {
        Self {
            max_changed_files: HOST_MAX_CHANGED_FILES,
            max_file_bytes: HOST_MAX_FILE_BYTES,
            max_total_file_bytes: HOST_MAX_TOTAL_FILE_BYTES,
            max_diff_bytes: HOST_MAX_DIFF_BYTES,
        }
    }
}

/// Minimal request understood by the pure Git owner.
///
/// `owner_id` is supplied by the canonical lifecycle owner so a durable event
/// can bind the allocation before any Git side effect. The branch and path are
/// derived from it; callers cannot choose either one independently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterWorkspaceRequest {
    pub owner_id: String,
    pub base_commit: String,
    pub allowed_paths: Vec<PathBuf>,
    pub limits: SealLimits,
}

impl WriterWorkspaceRequest {
    pub fn new(
        owner_id: impl Into<String>,
        base_commit: impl Into<String>,
        allowed_paths: Vec<PathBuf>,
    ) -> Self {
        Self {
            owner_id: owner_id.into(),
            base_commit: base_commit.into(),
            allowed_paths,
            limits: SealLimits::default(),
        }
    }
}

/// Durable facts persisted by the lifecycle owner before or immediately after
/// allocation. Recovery treats every field as a claim and re-proves it from
/// the repository, worktree registry, and Git backpointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedWorktreeFacts {
    pub owner_id: String,
    pub base_commit: String,
    pub worktree_path: PathBuf,
    pub branch_ref: String,
    pub root_branch_ref: String,
    pub allowed_paths: Vec<PathBuf>,
    pub limits: SealLimits,
}

/// Minimal durable seal identity. Tree, changed paths, and diff bytes are
/// deliberately reconstructed from Git instead of trusted from persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedWorktreeFacts {
    pub owner_id: String,
    pub base_commit: String,
    pub final_commit: String,
    pub diff_sha256: String,
}

/// An allocation whose identity is re-proved against Git before every
/// destructive or integrating operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedWorktree {
    owner_id: String,
    repository_root: PathBuf,
    common_git_dir: PathBuf,
    managed_root: PathBuf,
    worktree_path: PathBuf,
    worktree_git_dir: PathBuf,
    branch_ref: String,
    root_branch_ref: String,
    base_commit: String,
    allowed_paths: Vec<PathBuf>,
    limits: SealLimits,
}

impl OwnedWorktree {
    pub fn path(&self) -> &Path {
        &self.worktree_path
    }

    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    pub fn durable_facts(&self) -> OwnedWorktreeFacts {
        OwnedWorktreeFacts {
            owner_id: self.owner_id.clone(),
            base_commit: self.base_commit.clone(),
            worktree_path: self.worktree_path.clone(),
            branch_ref: self.branch_ref.clone(),
            root_branch_ref: self.root_branch_ref.clone(),
            allowed_paths: self.allowed_paths.clone(),
            limits: self.limits,
        }
    }
}

/// Host-observed writer result. The binary diff is retained for the M6-A
/// review boundary; later protocol integration may move it to an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedWorktree {
    owner_id: String,
    base_commit: String,
    final_commit: String,
    tree: String,
    changed_files: Vec<PathBuf>,
    diff_sha256: String,
    diff: Vec<u8>,
}

impl SealedWorktree {
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    pub fn final_commit(&self) -> &str {
        &self.final_commit
    }

    pub fn changed_files(&self) -> &[PathBuf] {
        &self.changed_files
    }

    pub fn diff_sha256(&self) -> &str {
        &self.diff_sha256
    }

    #[cfg(test)]
    fn diff(&self) -> &[u8] {
        &self.diff
    }

    pub(crate) fn durable_facts(&self) -> SealedWorktreeFacts {
        SealedWorktreeFacts {
            owner_id: self.owner_id.clone(),
            base_commit: self.base_commit.clone(),
            final_commit: self.final_commit.clone(),
            diff_sha256: self.diff_sha256.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationDisposition {
    Applied,
    AlreadyApplied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationResult {
    pub disposition: IntegrationDisposition,
    pub root_commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupComponentDisposition {
    Removed,
    AlreadyAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactCleanupDisposition {
    pub worktree: CleanupComponentDisposition,
    pub branch: CleanupComponentDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupResourcePresence {
    Absent,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupResourceFacts {
    pub path: CleanupResourcePresence,
    pub worktree: CleanupResourcePresence,
    pub branch: CleanupResourcePresence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanupScopeFacts {
    pub paths: Vec<PathBuf>,
    pub revision_sha256: String,
}

/// Sole owner of the M6-A Git/worktree side effects.
#[derive(Debug, Clone)]
pub struct GitWorkspaceOwner {
    repository_root: PathBuf,
    common_git_dir: PathBuf,
    managed_root: PathBuf,
}

#[derive(Debug)]
struct RepositorySnapshot {
    root: PathBuf,
    common_git_dir: PathBuf,
    head: String,
    branch_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeRecord {
    path: PathBuf,
    head: String,
    branch_ref: Option<String>,
    locked_reason: Option<String>,
}

/// Process-crash-safe ownership of the root worktree's Git `HEAD.lock`.
///
/// The persistent lease file provides cross-process liveness. `HEAD.lock`
/// blocks ordinary Git branch switching. Its exact durable-facts marker lets a
/// later process distinguish this integration's crash residue from a foreign
/// Git operation after it acquires the now-unlocked lease.
struct RootIntegrationLease {
    lease: File,
}

impl Drop for RootIntegrationLease {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lease);
    }
}

struct RootIntegrationLock {
    _lease: RootIntegrationLease,
    head_lock_path: PathBuf,
    marker: Vec<u8>,
    remove_marker_on_drop: bool,
}

struct RootCleanupLock {
    _lease: RootIntegrationLease,
    head_lock_path: PathBuf,
    marker: Vec<u8>,
    remove_marker_on_drop: bool,
}

impl Drop for RootCleanupLock {
    fn drop(&mut self) {
        if self.remove_marker_on_drop {
            remove_exact_marker_file(&self.head_lock_path, &self.marker);
        }
    }
}

impl RootCleanupLock {
    fn release(self) -> Result<()> {
        self.release_with(|path| remove_file(path, "release exact cleanup HEAD lock"))
    }

    fn release_with<F>(mut self, remove: F) -> Result<()>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        self.remove_marker_on_drop = false;
        if !exact_marker_file(&self.head_lock_path, &self.marker)? {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "cleanup HEAD lock marker changed before explicit release".to_string(),
            ));
        }
        remove(&self.head_lock_path)?;
        if symlink_metadata_optional(&self.head_lock_path)?.is_some() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "cleanup HEAD lock remained after explicit release".to_string(),
            ));
        }
        Ok(())
    }
}

impl Drop for RootIntegrationLock {
    fn drop(&mut self) {
        if self.remove_marker_on_drop {
            remove_exact_marker_file(&self.head_lock_path, &self.marker);
        }
    }
}

#[cfg(test)]
impl RootIntegrationLock {
    fn simulate_process_crash(mut self) {
        self.remove_marker_on_drop = false;
    }
}

impl GitWorkspaceOwner {
    /// Open a clean, ordinary Git repository and bind one external managed
    /// directory. The managed directory may be created, but it must not be the
    /// repository, an ancestor of it, or a descendant of it.
    #[cfg(test)]
    fn open(repository_root: impl AsRef<Path>, managed_root: impl AsRef<Path>) -> Result<Self> {
        let requested_managed = absolute_path(managed_root.as_ref())?;
        create_dir_all(&requested_managed)?;
        Self::bind_existing(repository_root, requested_managed)
    }

    /// Bind a managed directory initialized by the application composition.
    ///
    /// This never creates a filesystem entry and is therefore suitable for the
    /// pre-`AgentTaskPrepared` planning path.
    pub fn bind_existing(
        repository_root: impl AsRef<Path>,
        managed_root: impl AsRef<Path>,
    ) -> Result<Self> {
        Self::bind_existing_with_cleanliness(repository_root, managed_root, true)
    }

    /// Bind the same exact repository identity while allowing the narrow
    /// post-CAS integration recovery state, where the target root ref already
    /// points at the sealed commit but the index/worktree still represents the
    /// base commit.
    pub(crate) fn bind_existing_for_integration(
        repository_root: impl AsRef<Path>,
        managed_root: impl AsRef<Path>,
    ) -> Result<Self> {
        Self::bind_existing_with_cleanliness(repository_root, managed_root, false)
    }

    /// Bind only the repository identity needed to inspect or delete an exact
    /// owned Writer. Root cleanliness and the currently checked-out branch do
    /// not grant cleanup authority and therefore are intentionally ignored.
    pub(crate) fn bind_existing_for_cleanup(
        repository_root: impl AsRef<Path>,
        managed_root: impl AsRef<Path>,
    ) -> Result<Self> {
        let requested_root = absolute_path(repository_root.as_ref())?;
        let root = canonical_dir(&requested_root, "canonicalize cleanup repository")?;
        let top = rev_parse_path(&root, "--show-toplevel", "resolve cleanup repository root")?;
        let top = canonical_dir(&top, "canonicalize cleanup repository root")?;
        if top != root {
            return Err(GitWorkspaceError::InvalidRepository(format!(
                "requested cleanup path {} is not repository top-level {}",
                root.display(),
                top.display()
            )));
        }
        let common_git_dir = rev_parse_path(
            &root,
            "--git-common-dir",
            "resolve cleanup common Git directory",
        )?;
        let common_git_dir =
            canonical_dir(&common_git_dir, "canonicalize cleanup common Git directory")?;
        let requested_managed = absolute_path(managed_root.as_ref())?;
        let managed_root = canonical_dir(&requested_managed, "canonicalize managed root")?;
        if managed_root == root
            || managed_root.starts_with(&root)
            || root.starts_with(&managed_root)
        {
            return Err(GitWorkspaceError::InvalidRequest(format!(
                "managed root {} must be outside and unrelated to repository {}",
                managed_root.display(),
                root.display()
            )));
        }
        Ok(Self {
            repository_root: root,
            common_git_dir,
            managed_root,
        })
    }

    fn bind_existing_with_cleanliness(
        repository_root: impl AsRef<Path>,
        managed_root: impl AsRef<Path>,
        require_clean: bool,
    ) -> Result<Self> {
        let requested_root = absolute_path(repository_root.as_ref())?;
        let snapshot = inspect_repository(&requested_root, require_clean)?;
        let requested_managed = absolute_path(managed_root.as_ref())?;
        let managed_root = canonical_dir(&requested_managed, "canonicalize managed root")?;
        if managed_root == snapshot.root
            || managed_root.starts_with(&snapshot.root)
            || snapshot.root.starts_with(&managed_root)
        {
            return Err(GitWorkspaceError::InvalidRequest(format!(
                "managed root {} must be outside and unrelated to repository {}",
                managed_root.display(),
                snapshot.root.display()
            )));
        }

        Ok(Self {
            repository_root: snapshot.root,
            common_git_dir: snapshot.common_git_dir,
            managed_root,
        })
    }

    /// Return root `HEAD` without rejecting the exact post-CAS integration
    /// recovery state. Callers must subsequently prove that state through
    /// [`integrate`](Self::integrate).
    pub(crate) fn current_root_head_for_integration(&self) -> Result<String> {
        Ok(self.validate_bound_repository(false)?.head)
    }

    /// Plan from the exact clean root `HEAD` observed by the Git owner.
    pub fn plan_current(
        &self,
        owner_id: impl Into<String>,
        allowed_paths: Vec<PathBuf>,
    ) -> Result<OwnedWorktreeFacts> {
        let snapshot = self.validate_bound_repository(true)?;
        self.plan(WriterWorkspaceRequest::new(
            owner_id,
            snapshot.head,
            allowed_paths,
        ))
    }

    /// Derive and validate the exact durable assignment without creating a
    /// branch, worktree, or filesystem entry.
    ///
    /// The managed root itself is initialized when the owner is constructed;
    /// this method is therefore safe to call before `AgentTaskPrepared`.
    pub fn plan(&self, request: WriterWorkspaceRequest) -> Result<OwnedWorktreeFacts> {
        validate_owner_id(&request.owner_id)?;
        validate_limits(request.limits)?;
        let allowed_paths = normalize_allowed_paths(request.allowed_paths)?;
        let snapshot = self.validate_bound_repository(true)?;
        let base_commit = resolve_exact_commit(&self.repository_root, &request.base_commit)?;
        if snapshot.head != base_commit {
            return Err(GitWorkspaceError::Conflict(format!(
                "requested base {base_commit} is not current root HEAD {}",
                snapshot.head
            )));
        }

        let branch_ref = format!("{WRITER_BRANCH_PREFIX}{}", request.owner_id);
        validate_branch_ref(&self.repository_root, &branch_ref)?;
        let worktree_path = self.managed_root.join(&request.owner_id);
        validate_derived_path(&self.managed_root, &worktree_path)?;
        Ok(OwnedWorktreeFacts {
            owner_id: request.owner_id,
            base_commit,
            worktree_path,
            branch_ref,
            root_branch_ref: snapshot.branch_ref,
            allowed_paths,
            limits: request.limits,
        })
    }

    /// Create one unique worktree and branch from the exact current root
    /// commit. Replaying the exact owner request after Git completed the
    /// allocation is an idempotent recovery; aliases and partial ownership are
    /// rejected.
    pub fn create(&self, request: WriterWorkspaceRequest) -> Result<OwnedWorktree> {
        let facts = self.plan(request)?;
        let base_commit = facts.base_commit.clone();
        let branch_ref = facts.branch_ref.clone();
        let worktree_path = facts.worktree_path.clone();
        let path_exists = symlink_metadata_optional(&worktree_path)?.is_some();
        let branch_exists = resolve_ref_optional(&self.repository_root, &branch_ref)?.is_some();
        let registered = registered_worktrees(&self.repository_root)?
            .iter()
            .any(|record| same_path(&record.path, &worktree_path));
        if path_exists || branch_exists || registered {
            return self.recover_active(facts);
        }

        git_checked(
            &self.repository_root,
            "create owned writer worktree",
            [
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("-b"),
                branch_ref_to_short(&branch_ref)?.into(),
                worktree_path.as_os_str().to_owned(),
                base_commit.clone().into(),
            ],
        )?;
        self.recover_owned(facts)
    }

    /// Rebind an in-progress writer after restart.
    ///
    /// A writer may legitimately be dirty (or have staged files after a seal
    /// interruption), so recovery proves exact path/ref/backpointer ownership
    /// without requiring a clean writer checkout. The root must still be clean
    /// and pinned to the exact base branch and commit.
    pub fn recover_active(&self, facts: OwnedWorktreeFacts) -> Result<OwnedWorktree> {
        let root = self.validate_bound_repository(true)?;
        let allocation = self.rebuild_allocation(facts)?;
        if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD no longer matches the active assignment".to_string(),
            ));
        }
        self.verify_owned_worktree(&allocation, allocation.base_commit(), false)?;
        Ok(allocation)
    }

    /// Rebuild an unsealed allocation after restart from durable assignment
    /// facts. The writer branch and worktree must still be exactly at base.
    pub fn recover_owned(&self, facts: OwnedWorktreeFacts) -> Result<OwnedWorktree> {
        let root = self.validate_bound_repository(true)?;
        let allocation = self.rebuild_allocation(facts)?;
        if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD no longer matches the unsealed assignment".to_string(),
            ));
        }
        self.verify_owned_worktree(&allocation, allocation.base_commit(), true)?;
        Ok(allocation)
    }

    /// Rebuild a sealed allocation and result after restart. Only compact
    /// identity facts are accepted; all review material is regenerated from
    /// the exact commits and checked against the persisted digest.
    pub fn recover_sealed(
        &self,
        owned: OwnedWorktreeFacts,
        sealed: SealedWorktreeFacts,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        self.recover_sealed_with_cleanliness(owned, sealed, true)
    }

    /// Recover a sealed allocation while allowing only `integrate` to decide
    /// whether a dirty root is the exact post-CAS/base-checkout state.
    pub(crate) fn recover_sealed_for_integration(
        &self,
        owned: OwnedWorktreeFacts,
        sealed: SealedWorktreeFacts,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        self.recover_sealed_with_cleanliness(owned, sealed, false)
    }

    fn recover_sealed_with_cleanliness(
        &self,
        owned: OwnedWorktreeFacts,
        sealed: SealedWorktreeFacts,
        require_clean_root: bool,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        let root = self.validate_bound_repository(require_clean_root)?;
        let allocation = self.rebuild_allocation(owned)?;
        if root.branch_ref != allocation.root_branch_ref
            || (root.head != allocation.base_commit && root.head != sealed.final_commit)
        {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD is incompatible with the sealed assignment".to_string(),
            ));
        }
        if sealed.owner_id != allocation.owner_id || sealed.base_commit != allocation.base_commit {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "persisted seal identity belongs to another allocation".to_string(),
            ));
        }
        validate_sha256(&sealed.diff_sha256)?;
        let final_commit = resolve_exact_commit(&self.repository_root, &sealed.final_commit)?;
        self.verify_owned_worktree(&allocation, &final_commit, true)?;

        let changed_files = committed_changed_paths(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
            allocation.limits,
        )?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(&allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
            allocation.limits,
        )?;
        let diff = binary_diff_commits(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
            allocation.limits.max_diff_bytes,
        )?;
        if diff.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        if diff.len() > allocation.limits.max_diff_bytes {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "binary diff is {} bytes, limit is {}",
                diff.len(),
                allocation.limits.max_diff_bytes
            )));
        }
        let actual_hash = sha256_hex(&diff);
        if actual_hash != sealed.diff_sha256 {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "sealed diff digest is {actual_hash}, expected {}",
                sealed.diff_sha256
            )));
        }
        let recovered = SealedWorktree {
            owner_id: allocation.owner_id.clone(),
            base_commit: allocation.base_commit.clone(),
            tree: commit_tree(&self.repository_root, &final_commit)?,
            final_commit,
            changed_files,
            diff_sha256: actual_hash,
            diff,
        };
        verify_sealed_commit(&self.repository_root, &allocation, &recovered)?;
        Ok((allocation, recovered))
    }

    /// Recover the exact Host commit when the process died after publishing
    /// the writer ref but before committing `AgentSealCommitted`.
    ///
    /// No persisted digest exists in this window, so recovery additionally
    /// requires the fixed Host author/committer identity and exact owner-bound
    /// commit subject produced by [`seal`](Self::seal).
    pub fn recover_prepared_seal(
        &self,
        owned: OwnedWorktreeFacts,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        self.recover_prepared_seal_with_root_check(owned, true)
    }

    /// Recover a Host-published Writer seal solely for exact cleanup. The
    /// canonical root checkout may legitimately be dirty, advanced, switched,
    /// or detached by the time failure cleanup resumes; none of those states
    /// grants or removes authority over the isolated Writer ref/worktree.
    pub(crate) fn recover_prepared_seal_for_cleanup(
        &self,
        owned: OwnedWorktreeFacts,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        self.recover_prepared_seal_with_root_check(owned, false)
    }

    fn recover_prepared_seal_with_root_check(
        &self,
        owned: OwnedWorktreeFacts,
        require_matching_root: bool,
    ) -> Result<(OwnedWorktree, SealedWorktree)> {
        let allocation = self.rebuild_allocation(owned)?;
        if require_matching_root {
            let root = self.validate_bound_repository(true)?;
            if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit
            {
                return Err(GitWorkspaceError::Conflict(
                    "root branch or HEAD no longer matches the prepared seal".to_string(),
                ));
            }
        }
        let final_commit = resolve_ref_optional(&self.repository_root, &allocation.branch_ref)?
            .ok_or_else(|| {
                GitWorkspaceError::OwnershipMismatch(
                    "prepared seal writer branch no longer exists".to_string(),
                )
            })?;
        if final_commit == allocation.base_commit {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "prepared seal has not published a final commit".to_string(),
            ));
        }
        self.verify_owned_worktree(&allocation, &final_commit, true)?;
        verify_host_seal_identity(&self.repository_root, &allocation, &final_commit)?;

        let changed_files = committed_changed_paths(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
            allocation.limits,
        )?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(&allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
            allocation.limits,
        )?;
        let diff = binary_diff_commits(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
            allocation.limits.max_diff_bytes,
        )?;
        if diff.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        if diff.len() > allocation.limits.max_diff_bytes {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "binary diff is {} bytes, limit is {}",
                diff.len(),
                allocation.limits.max_diff_bytes
            )));
        }
        let recovered = SealedWorktree {
            owner_id: allocation.owner_id.clone(),
            base_commit: allocation.base_commit.clone(),
            tree: commit_tree(&self.repository_root, &final_commit)?,
            final_commit,
            changed_files,
            diff_sha256: sha256_hex(&diff),
            diff,
        };
        verify_sealed_commit(&self.repository_root, &allocation, &recovered)?;
        Ok((allocation, recovered))
    }

    /// Inspect the exact changed-path set of an owned Writer without depending
    /// on the root checkout's current branch, HEAD, index, or cleanliness.
    pub(crate) fn inspect_cleanup_paths(
        &self,
        facts: &OwnedWorktreeFacts,
        sealed: Option<&SealedWorktreeFacts>,
    ) -> Result<CleanupScopeFacts> {
        self.validate_owned_facts_identity(facts)?;
        let resources = self.cleanup_resource_facts(facts)?;
        if resources.worktree == CleanupResourcePresence::Absent {
            return cleanup_scope_facts(&facts.worktree_path, Vec::new(), facts.limits);
        }
        if resources.path == CleanupResourcePresence::Absent {
            return Err(GitWorkspaceError::Conflict(
                "Writer path disappeared before Host could freeze its cleanup scope".to_string(),
            ));
        }
        let allocation = self.rebuild_allocation(facts.clone())?;
        match sealed {
            Some(sealed) => {
                if sealed.owner_id != allocation.owner_id
                    || sealed.base_commit != allocation.base_commit
                {
                    return Err(GitWorkspaceError::OwnershipMismatch(
                        "cleanup seal belongs to another writer allocation".to_string(),
                    ));
                }
                validate_sha256(&sealed.diff_sha256)?;
                let final_commit =
                    resolve_exact_commit(&self.repository_root, &sealed.final_commit)?;
                self.verify_owned_worktree(&allocation, &final_commit, true)?;
                verify_host_seal_identity(&self.repository_root, &allocation, &final_commit)?;
                let diff = binary_diff_commits(
                    &self.repository_root,
                    allocation.base_commit(),
                    &final_commit,
                    allocation.limits.max_diff_bytes,
                )?;
                if sha256_hex(&diff) != sealed.diff_sha256 {
                    return Err(GitWorkspaceError::OwnershipMismatch(
                        "cleanup seal digest no longer matches the exact commits".to_string(),
                    ));
                }
                let mut paths = committed_changed_paths(
                    &self.repository_root,
                    allocation.base_commit(),
                    &final_commit,
                    allocation.limits,
                )?;
                paths.extend(collect_cleanup_changes(
                    &allocation.worktree_path,
                    allocation.limits,
                )?);
                cleanup_scope_facts(
                    &allocation.worktree_path,
                    paths
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    allocation.limits,
                )
            }
            None => {
                self.verify_owned_worktree(&allocation, allocation.base_commit(), false)?;
                let paths = collect_cleanup_changes(&allocation.worktree_path, allocation.limits)?;
                cleanup_scope_facts(&allocation.worktree_path, paths, allocation.limits)
            }
        }
    }

    /// Bind a prepared cleanup to the exact repository owner without storing
    /// filesystem paths in the protocol. The cleanup target itself is omitted
    /// so the digest remains reproducible after a successful remove/restart.
    pub(crate) fn cleanup_owner_identity_sha256(
        &self,
        facts: &OwnedWorktreeFacts,
    ) -> Result<String> {
        self.validate_owned_facts_shape(facts)?;
        let mut hasher = Sha256::new();
        for (label, value) in [
            (b"repository_root".as_slice(), &self.repository_root),
            (b"common_git_dir".as_slice(), &self.common_git_dir),
            (b"managed_root".as_slice(), &self.managed_root),
        ] {
            hash_cleanup_segment(&mut hasher, label, value.as_os_str().as_encoded_bytes());
            let metadata = fs::metadata(value).map_err(|source| GitWorkspaceError::Io {
                operation: "stat Writer cleanup owner",
                path: value.clone(),
                source,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                hash_cleanup_segment(&mut hasher, b"device", &metadata.dev().to_le_bytes());
                hash_cleanup_segment(&mut hasher, b"inode", &metadata.ino().to_le_bytes());
            }
        }
        hash_cleanup_segment(&mut hasher, b"owner_id", facts.owner_id.as_bytes());
        hash_cleanup_segment(&mut hasher, b"base_commit", facts.base_commit.as_bytes());
        hash_cleanup_segment(
            &mut hasher,
            b"worktree_path",
            facts.worktree_path.as_os_str().as_encoded_bytes(),
        );
        hash_cleanup_segment(&mut hasher, b"branch_ref", facts.branch_ref.as_bytes());
        hash_cleanup_segment(
            &mut hasher,
            b"root_branch_ref",
            facts.root_branch_ref.as_bytes(),
        );
        for path in &facts.allowed_paths {
            hash_cleanup_segment(
                &mut hasher,
                b"allowed_path",
                path.as_os_str().as_encoded_bytes(),
            );
        }
        Ok(hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    fn verify_missing_path_residue(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<()> {
        let expected = resolve_exact_commit(&self.repository_root, expected_branch_commit)?;
        let record = exact_registered_worktree(&self.repository_root, facts)?.ok_or_else(|| {
            GitWorkspaceError::OwnershipMismatch(
                "missing Writer path has no exact Git worktree registration".to_string(),
            )
        })?;
        verify_record_facts(&record, facts, &expected)?;
        require_ref(&self.repository_root, &facts.branch_ref, &expected)?;
        let admin_dir = owned_admin_entry(&self.common_git_dir, facts)?.ok_or_else(|| {
            GitWorkspaceError::OwnershipMismatch(
                "missing Writer path has no exact Git admin backpointer".to_string(),
            )
        })?;
        verify_admin_backpointer_facts(&admin_dir, &facts.worktree_path)
    }

    /// Observe whether the exact compound worktree resource (path,
    /// registration, and admin directory) and exact Writer ref still exist.
    pub(crate) fn cleanup_resource_facts(
        &self,
        facts: &OwnedWorktreeFacts,
    ) -> Result<CleanupResourceFacts> {
        self.validate_owned_facts_shape(facts)?;
        let path_present = symlink_metadata_optional(&facts.worktree_path)?.is_some();
        let registered = exact_registered_worktree(&self.repository_root, facts)?.is_some();
        let admin_present = owned_admin_entry(&self.common_git_dir, facts)?.is_some();
        let branch_present =
            resolve_ref_optional(&self.repository_root, &facts.branch_ref)?.is_some();
        Ok(CleanupResourceFacts {
            path: if path_present {
                CleanupResourcePresence::Present
            } else {
                CleanupResourcePresence::Absent
            },
            worktree: if path_present || registered || admin_present {
                CleanupResourcePresence::Present
            } else {
                CleanupResourcePresence::Absent
            },
            branch: if branch_present {
                CleanupResourcePresence::Present
            } else {
                CleanupResourcePresence::Absent
            },
        })
    }

    /// Prove that execution of a previously persisted cleanup plan has
    /// already crossed its Git-side intent boundary. A matching tombstone is
    /// sufficient even before the exact worktree lock is installed; a lock,
    /// when present, must carry the same derived cleanup identity.
    pub(crate) fn cleanup_has_started(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<bool> {
        self.validate_owned_facts_shape(facts)?;
        validate_full_object_id_claim(expected_branch_commit)?;
        let expected = expected_branch_commit.to_ascii_lowercase();
        let tombstone_ref = cleanup_tombstone_ref(facts, &expected)?;
        let Some(actual) = resolve_ref_optional(&self.repository_root, &tombstone_ref)? else {
            return Ok(false);
        };
        if actual != expected {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "Writer cleanup tombstone points to {actual}, expected {expected}"
            )));
        }
        if let Some(record) = exact_registered_worktree(&self.repository_root, facts)?
            && let Some(reason) = record.locked_reason.as_deref()
            && reason != cleanup_worktree_lock_reason(facts, &expected)
        {
            return Err(GitWorkspaceError::Conflict(
                "Writer worktree has a foreign persistent lock".to_string(),
            ));
        }
        Ok(true)
    }

    pub(crate) fn cleanup_metadata_is_clear(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<bool> {
        self.validate_owned_facts_shape(facts)?;
        validate_full_object_id_claim(expected_branch_commit)?;
        let expected = expected_branch_commit.to_ascii_lowercase();
        let tombstone_ref = cleanup_tombstone_ref(facts, &expected)?;
        if let Some(actual) = resolve_ref_optional(&self.repository_root, &tombstone_ref)? {
            if actual != expected {
                return Err(GitWorkspaceError::OwnershipMismatch(format!(
                    "Writer cleanup tombstone points to {actual}, expected {expected}"
                )));
            }
            return Ok(false);
        }
        let head_lock_path = self.root_head_lock_path()?;
        let marker = cleanup_head_lock_marker(facts, &expected);
        if exact_marker_file(&head_lock_path, &marker)? {
            return Ok(false);
        }
        if symlink_metadata_optional(&head_lock_path)?.is_some() {
            return Err(GitWorkspaceError::Conflict(
                "foreign Git HEAD lock is present while proving cleanup completion".to_string(),
            ));
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn simulate_branch_only_cleanup_guard_crash(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<()> {
        self.validate_owned_facts_identity(facts)?;
        let expected = resolve_exact_commit(&self.repository_root, expected_branch_commit)?;
        let before = self.cleanup_resource_facts(facts)?;
        if before.worktree != CleanupResourcePresence::Absent
            || before.branch != CleanupResourcePresence::Present
        {
            return Err(GitWorkspaceError::Conflict(
                "test crash fixture requires a branch-only Writer".to_string(),
            ));
        }
        ensure_writer_branch_not_checked_out_elsewhere(&self.repository_root, facts)?;
        let tombstone_ref = cleanup_tombstone_ref(facts, &expected)?;
        create_exact_ref_cas(
            &self.repository_root,
            &tombstone_ref,
            &expected,
            "test persist Writer cleanup tombstone",
        )?;
        create_cleanup_guard_worktree(
            &self.repository_root,
            facts,
            &expected,
            &cleanup_worktree_lock_reason(facts, &expected),
        )
    }

    /// Remove a previously inspected exact Writer. Dirty removal is permitted
    /// only for the explicit failed-writer discard mode and only after all
    /// owner, registry, backpointer, common-dir, branch, and commit checks pass.
    /// DSE serializes its own mutations through the repository lease;
    /// an already-running external Git plumbing command is not transactionally
    /// controlled, so any foreign Writer-branch checkout that becomes visible
    /// is retained and its branch is restored instead of being deleted.
    pub(crate) fn cleanup_exact(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<ExactCleanupDisposition> {
        self.cleanup_exact_with_hooks(
            facts,
            expected_branch_commit,
            || Ok(()),
            RootCleanupLock::release,
        )
    }

    #[cfg(test)]
    pub(crate) fn cleanup_exact_with_failed_root_lock_release(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<ExactCleanupDisposition> {
        self.cleanup_exact_with_hooks(
            facts,
            expected_branch_commit,
            || Ok(()),
            |root_lock| {
                root_lock.release_with(|path| {
                    Err(GitWorkspaceError::Io {
                        operation: "inject cleanup HEAD lock unlink failure",
                        path: path.to_path_buf(),
                        source: io::Error::other("injected unlink failure"),
                    })
                })
            },
        )
    }

    #[cfg(test)]
    fn cleanup_exact_with_hook<F>(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
        pre_cleanup_hook: F,
    ) -> Result<ExactCleanupDisposition>
    where
        F: FnOnce() -> Result<()>,
    {
        self.cleanup_exact_with_hooks(
            facts,
            expected_branch_commit,
            pre_cleanup_hook,
            RootCleanupLock::release,
        )
    }

    fn cleanup_exact_with_hooks<F, R>(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
        pre_cleanup_hook: F,
        release_root_lock: R,
    ) -> Result<ExactCleanupDisposition>
    where
        F: FnOnce() -> Result<()>,
        R: FnOnce(RootCleanupLock) -> Result<()>,
    {
        let lease = self.acquire_root_integration_lease()?;
        self.validate_owned_facts_shape(facts)?;
        validate_full_object_id_claim(expected_branch_commit)?;
        let expected = expected_branch_commit.to_ascii_lowercase();
        let root_lock = self.acquire_root_cleanup_lock(lease, facts, &expected)?;
        let cleanup = (|| {
            let tombstone_ref = cleanup_tombstone_ref(facts, &expected)?;
            let before = self.cleanup_resource_facts(facts)?;
            let tombstone = resolve_ref_optional(&self.repository_root, &tombstone_ref)?;
            if let Some(actual) = tombstone.as_deref()
                && actual != expected
            {
                return Err(GitWorkspaceError::OwnershipMismatch(format!(
                    "Writer cleanup tombstone points to {actual}, expected {expected}"
                )));
            }

            if before.branch == CleanupResourcePresence::Absent {
                retain_writer_branch_for_foreign_checkout(&self.repository_root, facts, &expected)?;
            }

            if before.worktree == CleanupResourcePresence::Absent
                && before.branch == CleanupResourcePresence::Absent
            {
                return Ok((
                    ExactCleanupDisposition {
                        worktree: CleanupComponentDisposition::AlreadyAbsent,
                        branch: CleanupComponentDisposition::AlreadyAbsent,
                    },
                    tombstone.map(|_| tombstone_ref),
                ));
            }

            let resolved = resolve_exact_commit(&self.repository_root, &expected)?;
            debug_assert_eq!(resolved, expected);

            if before.worktree == CleanupResourcePresence::Present {
                if let Some(record) = exact_registered_worktree(&self.repository_root, facts)?
                    && let Some(reason) = record.locked_reason.as_deref()
                {
                    let expected_reason = cleanup_worktree_lock_reason(facts, &expected);
                    if reason != expected_reason {
                        return Err(GitWorkspaceError::Conflict(
                            "Writer worktree has a foreign persistent lock".to_string(),
                        ));
                    }
                    if tombstone.is_none() {
                        return Err(GitWorkspaceError::OwnershipMismatch(
                            "Writer cleanup lock exists without its exact tombstone".to_string(),
                        ));
                    }
                }
                if before.branch == CleanupResourcePresence::Present {
                    if before.path == CleanupResourcePresence::Present {
                        let allocation = self.rebuild_allocation(facts.clone())?;
                        self.verify_owned_worktree(&allocation, &expected, false)?;
                    } else {
                        self.verify_missing_path_residue(facts, &expected)?;
                    }
                } else if tombstone.is_some() {
                    verify_branchless_cleanup_worktree(
                        &self.repository_root,
                        &self.common_git_dir,
                        facts,
                        &expected,
                    )?;
                } else {
                    return Err(GitWorkspaceError::OwnershipMismatch(
                        "Writer branch disappeared without a cleanup tombstone".to_string(),
                    ));
                }
            }

            ensure_writer_branch_not_checked_out_elsewhere(&self.repository_root, facts)?;
            if tombstone.is_none() {
                require_ref(&self.repository_root, &facts.branch_ref, &expected)?;
                create_exact_ref_cas(
                    &self.repository_root,
                    &tombstone_ref,
                    &expected,
                    "persist Writer cleanup tombstone",
                )?;
            }

            let mut guarded = self.cleanup_resource_facts(facts)?;
            if guarded.worktree == CleanupResourcePresence::Absent {
                if guarded.branch != CleanupResourcePresence::Present {
                    return Err(GitWorkspaceError::OwnershipMismatch(
                        "Writer cleanup tombstone exists without a branch or worktree resource"
                            .to_string(),
                    ));
                }
                create_cleanup_guard_worktree(
                    &self.repository_root,
                    facts,
                    &expected,
                    &cleanup_worktree_lock_reason(facts, &expected),
                )?;
                guarded = self.cleanup_resource_facts(facts)?;
            }

            if guarded.branch == CleanupResourcePresence::Present {
                if guarded.path == CleanupResourcePresence::Present {
                    let allocation = self.rebuild_allocation(facts.clone())?;
                    self.verify_owned_worktree(&allocation, &expected, false)?;
                } else {
                    self.verify_missing_path_residue(facts, &expected)?;
                }
            } else {
                verify_branchless_cleanup_worktree(
                    &self.repository_root,
                    &self.common_git_dir,
                    facts,
                    &expected,
                )?;
            }
            acquire_cleanup_worktree_lock(&self.repository_root, facts, &expected)?;
            ensure_cleanup_root_is_registered(&self.repository_root)?;
            pre_cleanup_hook()?;
            ensure_cleanup_root_is_registered(&self.repository_root)?;
            ensure_writer_branch_not_checked_out_elsewhere(&self.repository_root, facts)?;

            let branch = delete_exact_writer_ref_cas(&self.repository_root, facts, &expected)?;
            retain_writer_branch_for_foreign_checkout(&self.repository_root, facts, &expected)?;
            verify_branchless_cleanup_worktree(
                &self.repository_root,
                &self.common_git_dir,
                facts,
                &expected,
            )?;
            git_checked(
                &self.repository_root,
                "remove exact locked Writer worktree",
                [
                    OsString::from("worktree"),
                    OsString::from("remove"),
                    OsString::from("--force"),
                    OsString::from("--force"),
                    facts.worktree_path.as_os_str().to_owned(),
                ],
            )?;
            retain_writer_branch_for_foreign_checkout(&self.repository_root, facts, &expected)?;
            let after_worktree = self.cleanup_resource_facts(facts)?;
            if after_worktree.worktree != CleanupResourcePresence::Absent
                || after_worktree.branch != CleanupResourcePresence::Absent
            {
                return Err(GitWorkspaceError::OwnershipMismatch(
                    "exact Writer worktree or branch remained after cleanup".to_string(),
                ));
            }
            Ok((
                ExactCleanupDisposition {
                    worktree: if before.worktree == CleanupResourcePresence::Present {
                        CleanupComponentDisposition::Removed
                    } else {
                        CleanupComponentDisposition::AlreadyAbsent
                    },
                    branch,
                },
                Some(tombstone_ref),
            ))
        })();

        match cleanup {
            Ok((disposition, tombstone_ref)) => {
                release_root_lock(root_lock)?;
                if let Some(tombstone_ref) = tombstone_ref {
                    retain_writer_branch_for_foreign_checkout(
                        &self.repository_root,
                        facts,
                        &expected,
                    )?;
                    delete_exact_ref_cas(
                        &self.repository_root,
                        &tombstone_ref,
                        &expected,
                        "delete completed Writer cleanup tombstone",
                    )?;
                    if resolve_ref_optional(&self.repository_root, &tombstone_ref)?.is_some() {
                        return Err(GitWorkspaceError::OwnershipMismatch(
                            "Writer cleanup tombstone remained after cleanup".to_string(),
                        ));
                    }
                    retain_writer_branch_for_foreign_checkout(
                        &self.repository_root,
                        facts,
                        &expected,
                    )?;
                }
                Ok(disposition)
            }
            Err(error) => Err(error),
        }
    }

    /// Validate, stage, seal, and commit a writer's filesystem result using
    /// plumbing commands. `commit-tree` and `update-ref` invoke no hooks; Git
    /// signing is disabled on every command.
    pub fn seal(&self, allocation: &OwnedWorktree) -> Result<SealedWorktree> {
        self.verify_allocation_owner(allocation)?;
        let root = self.validate_bound_repository(true)?;
        if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD changed while writer was running".to_string(),
            ));
        }
        self.verify_owned_worktree(allocation, &allocation.base_commit, false)?;

        let initial_paths = collect_worktree_changes(&allocation.worktree_path, allocation.limits)?;
        if initial_paths.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(allocation, &initial_paths)?;

        git_checked(
            &allocation.worktree_path,
            "stage writer result",
            [
                OsString::from("add"),
                OsString::from("-A"),
                OsString::from("--"),
            ],
        )?;

        ensure_no_unstaged_or_untracked_changes(&allocation.worktree_path, allocation.limits)?;
        let changed_files = staged_changed_paths(
            &allocation.worktree_path,
            allocation.base_commit(),
            allocation.limits,
        )?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
            allocation.limits,
        )?;

        let diff = binary_diff_cached(
            &allocation.worktree_path,
            allocation.base_commit(),
            allocation.limits.max_diff_bytes,
        )?;
        if diff.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        if diff.len() > allocation.limits.max_diff_bytes {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "binary diff is {} bytes, limit is {}",
                diff.len(),
                allocation.limits.max_diff_bytes
            )));
        }
        let diff_sha256 = sha256_hex(&diff);
        let tree = git_text(
            &allocation.worktree_path,
            "write writer tree",
            [OsString::from("write-tree")],
        )?;

        // Recheck the root and the writer ref immediately before minting the
        // commit. This closes the ordinary race window without pretending Git
        // can provide a transaction spanning two worktrees.
        let root = self.validate_bound_repository(true)?;
        if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(
                "root changed before writer seal was committed".to_string(),
            ));
        }
        require_ref(
            &self.repository_root,
            &allocation.branch_ref,
            &allocation.base_commit,
        )?;
        ensure_no_unstaged_or_untracked_changes(&allocation.worktree_path, allocation.limits)?;

        let final_commit = git_text_with_identity(
            &allocation.worktree_path,
            "commit sealed writer tree",
            [
                OsString::from("commit-tree"),
                tree.clone().into(),
                OsString::from("-p"),
                allocation.base_commit.clone().into(),
                OsString::from("-m"),
                format!("DSE writer {}", allocation.owner_id).into(),
            ],
        )?;
        git_checked(
            &self.repository_root,
            "publish sealed writer commit",
            [
                OsString::from("update-ref"),
                allocation.branch_ref.clone().into(),
                final_commit.clone().into(),
                allocation.base_commit.clone().into(),
            ],
        )?;

        self.verify_owned_worktree(allocation, &final_commit, true)?;
        let committed_diff = binary_diff_commits(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
            allocation.limits.max_diff_bytes,
        )?;
        if committed_diff != diff {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "sealed commit diff differs from the reviewed staged diff".to_string(),
            ));
        }
        let committed_tree = commit_tree(&self.repository_root, &final_commit)?;
        if committed_tree != tree {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "sealed commit tree differs from Host write-tree result".to_string(),
            ));
        }

        Ok(SealedWorktree {
            owner_id: allocation.owner_id.clone(),
            base_commit: allocation.base_commit.clone(),
            final_commit,
            tree,
            changed_files,
            diff_sha256,
            diff,
        })
    }

    /// Fast-forward the original root branch to the Host-sealed commit.
    /// Repeated calls are idempotent only when the exact final commit is
    /// already checked out and the root remains clean.
    pub fn integrate(
        &self,
        allocation: &OwnedWorktree,
        sealed: &SealedWorktree,
    ) -> Result<IntegrationResult> {
        self.integrate_with_hooks(allocation, sealed, || Ok(()), || Ok(()))
    }

    fn integrate_with_hooks<F, G>(
        &self,
        allocation: &OwnedWorktree,
        sealed: &SealedWorktree,
        pre_cas_hook: F,
        before_materialize_hook: G,
    ) -> Result<IntegrationResult>
    where
        F: FnOnce() -> Result<()>,
        G: FnOnce() -> Result<()>,
    {
        self.verify_seal_identity(allocation, sealed)?;
        self.verify_owned_worktree(allocation, sealed.final_commit(), true)?;
        verify_sealed_commit(&self.repository_root, allocation, sealed)?;
        let lease = self.acquire_root_integration_lease()?;

        let root = self.validate_bound_repository(false)?;
        if root.branch_ref != allocation.root_branch_ref {
            return Err(GitWorkspaceError::Conflict(format!(
                "root branch changed from {} to {}",
                allocation.root_branch_ref, root.branch_ref
            )));
        }
        if root.head == sealed.final_commit {
            let _root_lock = self.acquire_root_head_lock(lease, allocation, sealed)?;
            let locked_root = self.validate_bound_repository(false)?;
            if locked_root.branch_ref != allocation.root_branch_ref
                || locked_root.head != sealed.final_commit
            {
                return Err(GitWorkspaceError::Conflict(
                    "root branch or HEAD changed before integration recovery acquired HEAD.lock"
                        .to_string(),
                ));
            }
            if root_checkout_is_clean(&self.repository_root)? {
                return Ok(IntegrationResult {
                    disposition: IntegrationDisposition::AlreadyApplied,
                    root_commit: locked_root.head,
                });
            }
            if !root_checkout_matches_commit(&self.repository_root, allocation.base_commit())? {
                return Err(GitWorkspaceError::OwnershipMismatch(
                    "root ref is integrated but index/worktree is neither clean final nor exact base recovery state"
                        .to_string(),
                ));
            }
            before_materialize_hook()?;
            materialize_root_fast_forward(
                &self.repository_root,
                allocation.base_commit(),
                sealed.final_commit(),
            )?;
            let after = self.validate_bound_repository(true)?;
            if after.branch_ref != allocation.root_branch_ref || after.head != sealed.final_commit {
                return Err(GitWorkspaceError::OwnershipMismatch(
                    "root post-recovery state does not match sealed commit".to_string(),
                ));
            }
            return Ok(IntegrationResult {
                disposition: IntegrationDisposition::AlreadyApplied,
                root_commit: after.head,
            });
        }
        if root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(format!(
                "root HEAD {} is neither base {} nor final {}",
                root.head, allocation.base_commit, sealed.final_commit
            )));
        }
        ensure_clean(&self.repository_root, "root repository before integration")?;
        self.reconcile_pre_cas_head_lock(allocation, sealed)?;

        // Test-only callers use this boundary to reproduce the historical
        // precheck -> porcelain-merge branch-switch race with real Git.
        pre_cas_hook()?;

        // The only atomic mutation is an explicit compare-and-swap of the
        // owner-bound target ref. Unlike `git merge`, this cannot advance
        // whichever branch happens to be checked out after the precheck.
        let cas = git_checked(
            &self.repository_root,
            "advance exact root branch to sealed commit",
            [
                OsString::from("update-ref"),
                allocation.root_branch_ref.clone().into(),
                sealed.final_commit.clone().into(),
                allocation.base_commit.clone().into(),
            ],
        );
        if let Err(error) = cas {
            let head_lock_path = self.root_head_lock_path()?;
            if symlink_metadata_optional(&head_lock_path)?.is_some() {
                return Err(GitWorkspaceError::Conflict(format!(
                    "root HEAD lock appeared during target-ref CAS at {}",
                    head_lock_path.display()
                )));
            }
            return Err(error);
        }

        let _root_lock = self.acquire_root_head_lock(lease, allocation, sealed)?;
        let after_cas = self.validate_bound_repository(false)?;
        if after_cas.branch_ref != allocation.root_branch_ref {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "target root ref advanced safely, but checked-out branch changed to {}; rerun integration on {}",
                after_cas.branch_ref, allocation.root_branch_ref
            )));
        }
        if after_cas.head != sealed.final_commit {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "target root ref CAS succeeded but checked-out HEAD does not resolve to sealed commit"
                    .to_string(),
            ));
        }
        if !root_checkout_matches_commit(&self.repository_root, allocation.base_commit())? {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "root index/worktree changed after integration precheck; sealed ref is retained for exact recovery"
                    .to_string(),
            ));
        }
        before_materialize_hook()?;
        materialize_root_fast_forward(
            &self.repository_root,
            allocation.base_commit(),
            sealed.final_commit(),
        )?;

        let after = self.validate_bound_repository(true)?;
        if after.branch_ref != allocation.root_branch_ref || after.head != sealed.final_commit {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "root post-integration state does not match sealed commit".to_string(),
            ));
        }
        Ok(IntegrationResult {
            disposition: IntegrationDisposition::Applied,
            root_commit: after.head,
        })
    }

    fn validate_bound_repository(&self, require_clean: bool) -> Result<RepositorySnapshot> {
        let snapshot = inspect_repository(&self.repository_root, require_clean)?;
        if snapshot.root != self.repository_root || snapshot.common_git_dir != self.common_git_dir {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "repository root or common Git directory changed".to_string(),
            ));
        }
        Ok(snapshot)
    }

    fn acquire_root_integration_lease(&self) -> Result<RootIntegrationLease> {
        let lease_path = self.common_git_dir.join(INTEGRATION_LEASE_FILE);
        let lease = open_integration_lease(&lease_path)?;
        if let Err(source) = lease.try_lock_exclusive() {
            if source.kind() == fs2::lock_contended_error().kind() {
                return Err(GitWorkspaceError::Conflict(
                    "another DSE root integration is active".to_string(),
                ));
            }
            return Err(GitWorkspaceError::Io {
                operation: "lock DSE integration lease",
                path: lease_path,
                source,
            });
        }
        Ok(RootIntegrationLease { lease })
    }

    fn acquire_root_head_lock(
        &self,
        lease: RootIntegrationLease,
        allocation: &OwnedWorktree,
        sealed: &SealedWorktree,
    ) -> Result<RootIntegrationLock> {
        self.verify_allocation_owner(allocation)?;
        self.verify_seal_identity(allocation, sealed)?;
        let head_lock_path = self.root_head_lock_path()?;
        let root_git_dir = head_lock_path
            .parent()
            .expect("root HEAD lock has Git directory parent");
        let marker = integration_head_lock_marker(allocation, sealed);
        let marker_temp_path = unique_integration_marker_temp_path(root_git_dir)?;
        install_integration_head_lock(&head_lock_path, &marker_temp_path, &marker)?;
        Ok(RootIntegrationLock {
            _lease: lease,
            head_lock_path,
            marker,
            remove_marker_on_drop: true,
        })
    }

    fn acquire_root_cleanup_lock(
        &self,
        lease: RootIntegrationLease,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<RootCleanupLock> {
        ensure_cleanup_root_is_registered(&self.repository_root)?;
        let head_lock_path = self.root_head_lock_path()?;
        let root_git_dir = head_lock_path
            .parent()
            .expect("root HEAD lock has Git directory parent");
        let marker = cleanup_head_lock_marker(facts, expected_branch_commit);
        let marker_temp_path = unique_integration_marker_temp_path(root_git_dir)?;
        install_integration_head_lock(&head_lock_path, &marker_temp_path, &marker)?;
        if let Err(error) = ensure_cleanup_root_is_registered(&self.repository_root) {
            remove_exact_marker_file(&head_lock_path, &marker);
            return Err(error);
        }
        Ok(RootCleanupLock {
            _lease: lease,
            head_lock_path,
            marker,
            remove_marker_on_drop: true,
        })
    }

    fn reconcile_pre_cas_head_lock(
        &self,
        allocation: &OwnedWorktree,
        sealed: &SealedWorktree,
    ) -> Result<()> {
        let head_lock_path = self.root_head_lock_path()?;
        let marker = integration_head_lock_marker(allocation, sealed);
        if exact_marker_file(&head_lock_path, &marker)? {
            remove_file(
                &head_lock_path,
                "remove exact crashed pre-CAS integration HEAD lock",
            )?;
        } else if symlink_metadata_optional(&head_lock_path)?.is_some() {
            return Err(GitWorkspaceError::Conflict(format!(
                "foreign Git HEAD lock is present before integration CAS at {}",
                head_lock_path.display()
            )));
        }
        Ok(())
    }

    fn root_head_lock_path(&self) -> Result<PathBuf> {
        let root_git_dir = rev_parse_path(
            &self.repository_root,
            "--absolute-git-dir",
            "resolve root worktree Git directory",
        )?;
        let root_git_dir =
            canonical_dir(&root_git_dir, "canonicalize root worktree Git directory")?;
        Ok(root_git_dir.join("HEAD.lock"))
    }

    fn validate_owned_facts_identity(&self, facts: &OwnedWorktreeFacts) -> Result<()> {
        self.validate_owned_facts_shape(facts)?;
        resolve_exact_commit(&self.repository_root, &facts.base_commit)?;
        Ok(())
    }

    fn validate_owned_facts_shape(&self, facts: &OwnedWorktreeFacts) -> Result<()> {
        validate_owner_id(&facts.owner_id)?;
        validate_limits(facts.limits)?;
        let allowed_paths = normalize_allowed_paths(facts.allowed_paths.clone())?;
        if allowed_paths != facts.allowed_paths {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "persisted allowed paths are not canonical".to_string(),
            ));
        }
        validate_full_object_id_claim(&facts.base_commit)?;
        let expected_path = self.managed_root.join(&facts.owner_id);
        validate_derived_path(&self.managed_root, &expected_path)?;
        if facts.worktree_path != expected_path {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "persisted worktree path is not owner-derived".to_string(),
            ));
        }
        let expected_branch = format!("{WRITER_BRANCH_PREFIX}{}", facts.owner_id);
        validate_branch_ref(&self.repository_root, &expected_branch)?;
        if facts.branch_ref != expected_branch {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "persisted writer branch is not owner-derived".to_string(),
            ));
        }
        validate_branch_ref(&self.repository_root, &facts.root_branch_ref)?;
        Ok(())
    }

    fn rebuild_allocation(&self, facts: OwnedWorktreeFacts) -> Result<OwnedWorktree> {
        validate_owner_id(&facts.owner_id)?;
        validate_limits(facts.limits)?;
        let allowed_paths = normalize_allowed_paths(facts.allowed_paths)?;
        let base_commit = resolve_exact_commit(&self.repository_root, &facts.base_commit)?;
        let expected_path = self.managed_root.join(&facts.owner_id);
        validate_derived_path(&self.managed_root, &expected_path)?;
        if facts.worktree_path != expected_path {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "persisted worktree path {} is not exact derived path {}",
                facts.worktree_path.display(),
                expected_path.display()
            )));
        }
        let expected_branch = format!("{WRITER_BRANCH_PREFIX}{}", facts.owner_id);
        validate_branch_ref(&self.repository_root, &expected_branch)?;
        if facts.branch_ref != expected_branch {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "persisted writer branch {} is not exact derived branch {expected_branch}",
                facts.branch_ref
            )));
        }
        validate_branch_ref(&self.repository_root, &facts.root_branch_ref)?;
        let canonical_worktree = canonical_dir(
            &facts.worktree_path,
            "canonicalize recovered writer worktree",
        )?;
        if canonical_worktree != facts.worktree_path {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "recovered worktree canonical path {} differs from persisted path {}",
                canonical_worktree.display(),
                facts.worktree_path.display()
            )));
        }
        let worktree_git_dir = rev_parse_path(
            &canonical_worktree,
            "--absolute-git-dir",
            "resolve recovered worktree git dir",
        )?;
        let worktree_git_dir =
            canonical_dir(&worktree_git_dir, "canonicalize recovered worktree git dir")?;
        Ok(OwnedWorktree {
            owner_id: facts.owner_id,
            repository_root: self.repository_root.clone(),
            common_git_dir: self.common_git_dir.clone(),
            managed_root: self.managed_root.clone(),
            worktree_path: canonical_worktree,
            worktree_git_dir,
            branch_ref: facts.branch_ref,
            root_branch_ref: facts.root_branch_ref,
            base_commit,
            allowed_paths,
            limits: facts.limits,
        })
    }

    fn verify_allocation_owner(&self, allocation: &OwnedWorktree) -> Result<()> {
        if allocation.repository_root != self.repository_root
            || allocation.common_git_dir != self.common_git_dir
            || allocation.managed_root != self.managed_root
        {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "allocation belongs to another Git workspace owner".to_string(),
            ));
        }
        validate_owner_id(&allocation.owner_id)?;
        let expected_path = self.managed_root.join(&allocation.owner_id);
        validate_derived_path(&self.managed_root, &expected_path)?;
        if !same_path(&expected_path, &allocation.worktree_path) {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "allocation path is not derived from its owner id".to_string(),
            ));
        }
        let expected_branch = format!("{WRITER_BRANCH_PREFIX}{}", allocation.owner_id);
        if allocation.branch_ref != expected_branch {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "allocation branch is not derived from its owner id".to_string(),
            ));
        }
        Ok(())
    }

    fn verify_owned_worktree(
        &self,
        allocation: &OwnedWorktree,
        expected_head: &str,
        require_clean: bool,
    ) -> Result<()> {
        self.verify_allocation_owner(allocation)?;
        let records = registered_worktrees(&self.repository_root)?;
        let record = records
            .iter()
            .find(|record| same_path(&record.path, &allocation.worktree_path))
            .ok_or_else(|| {
                GitWorkspaceError::OwnershipMismatch(format!(
                    "worktree {} is not registered",
                    allocation.worktree_path.display()
                ))
            })?;
        verify_record(record, allocation, expected_head)?;
        require_ref(&self.repository_root, &allocation.branch_ref, expected_head)?;
        verify_worktree_backpointer(allocation)?;
        verify_admin_backpointer(allocation)?;
        let common = rev_parse_path(
            &allocation.worktree_path,
            "--git-common-dir",
            "resolve writer common Git dir",
        )?;
        let common = canonical_dir(&common, "canonicalize writer common Git dir")?;
        if common != allocation.common_git_dir {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "writer worktree resolves to another common Git directory".to_string(),
            ));
        }
        let head = resolve_head(&allocation.worktree_path)?;
        if head != expected_head {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "writer worktree HEAD is {head}, expected {expected_head}"
            )));
        }
        let branch = symbolic_head(&allocation.worktree_path)?;
        if branch != allocation.branch_ref {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "writer worktree branch is {branch}, expected {}",
                allocation.branch_ref
            )));
        }
        if require_clean {
            ensure_clean(&allocation.worktree_path, "owned writer worktree")?;
        }
        Ok(())
    }

    fn verify_seal_identity(
        &self,
        allocation: &OwnedWorktree,
        sealed: &SealedWorktree,
    ) -> Result<()> {
        if sealed.owner_id != allocation.owner_id || sealed.base_commit != allocation.base_commit {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "sealed writer result belongs to another allocation".to_string(),
            ));
        }
        Ok(())
    }
}

fn inspect_repository(path: &Path, require_clean: bool) -> Result<RepositorySnapshot> {
    let top = git_probe(
        path,
        [
            OsString::from("rev-parse"),
            OsString::from("--show-toplevel"),
        ],
    )?;
    if !top.status.success() {
        return Err(GitWorkspaceError::InvalidRepository(output_detail(&top)));
    }
    let root = output_path(&top, "Git top-level")?;
    let root = canonical_dir(&root, "canonicalize Git top-level")?;
    let requested = canonical_dir(path, "canonicalize requested repository")?;
    if root != requested {
        return Err(GitWorkspaceError::InvalidRepository(format!(
            "requested path {} is not repository top-level {}",
            requested.display(),
            root.display()
        )));
    }

    let inside = git_text(
        &root,
        "check Git worktree",
        [
            OsString::from("rev-parse"),
            OsString::from("--is-inside-work-tree"),
        ],
    )?;
    if inside != "true" {
        return Err(GitWorkspaceError::InvalidRepository(
            "path is not inside a Git worktree".to_string(),
        ));
    }
    let head = resolve_head(&root).map_err(|error| match error {
        GitWorkspaceError::Git { .. } => {
            GitWorkspaceError::InvalidRepository("repository has no committed HEAD".to_string())
        }
        other => other,
    })?;
    let branch_ref = symbolic_head(&root).map_err(|_| {
        GitWorkspaceError::UnsupportedRepository(
            "detached root HEAD is not supported for guarded integration".to_string(),
        )
    })?;
    let common_git_dir = rev_parse_path(&root, "--git-common-dir", "resolve common Git directory")?;
    let common_git_dir = canonical_dir(&common_git_dir, "canonicalize common Git directory")?;

    let sparse = git_probe(
        &root,
        [
            OsString::from("config"),
            OsString::from("--bool"),
            OsString::from("core.sparseCheckout"),
        ],
    )?;
    if sparse.status.success() && trim_ascii(&sparse.stdout) == b"true" {
        return Err(GitWorkspaceError::UnsupportedRepository(
            "sparse checkout is not supported for M6-A writer worktrees".to_string(),
        ));
    }
    if !sparse.status.success() && sparse.status.code() != Some(1) {
        return Err(git_failure("inspect sparse checkout", &sparse));
    }

    let superproject = git_text(
        &root,
        "inspect superproject",
        [
            OsString::from("rev-parse"),
            OsString::from("--show-superproject-working-tree"),
        ],
    )?;
    if !superproject.is_empty() {
        return Err(GitWorkspaceError::UnsupportedRepository(format!(
            "repository is a submodule of {superproject}"
        )));
    }
    let index = git_checked(
        &root,
        "inspect index modes",
        [
            OsString::from("ls-files"),
            OsString::from("--stage"),
            OsString::from("-z"),
        ],
    )?;
    if index
        .split(|byte| *byte == 0)
        .any(|entry| entry.starts_with(b"160000 "))
    {
        return Err(GitWorkspaceError::UnsupportedRepository(
            "repositories containing submodule gitlinks are not supported".to_string(),
        ));
    }
    if require_clean {
        ensure_clean(&root, "root repository")?;
    }
    Ok(RepositorySnapshot {
        root,
        common_git_dir,
        head,
        branch_ref,
    })
}

fn validate_owner_id(owner_id: &str) -> Result<()> {
    if owner_id.is_empty()
        || owner_id.len() > 96
        || !owner_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "owner_id must be 1-96 ASCII alphanumeric, '-' or '_' characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_limits(limits: SealLimits) -> Result<()> {
    if limits.max_changed_files == 0
        || limits.max_file_bytes == 0
        || limits.max_total_file_bytes == 0
        || limits.max_diff_bytes == 0
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "all seal limits must be positive".to_string(),
        ));
    }
    if limits.max_changed_files > HOST_MAX_CHANGED_FILES
        || limits.max_file_bytes > HOST_MAX_FILE_BYTES
        || limits.max_total_file_bytes > HOST_MAX_TOTAL_FILE_BYTES
        || limits.max_diff_bytes > HOST_MAX_DIFF_BYTES
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "seal limits exceed the Host hard maximum".to_string(),
        ));
    }
    Ok(())
}

fn normalize_allowed_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(GitWorkspaceError::InvalidRequest(
            "writer must have at least one explicit allowed path".to_string(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for path in paths {
        validate_relative_path(&path, "allowed path")?;
        if path
            .components()
            .next()
            .is_some_and(|component| component.as_os_str() == OsStr::new(".git"))
        {
            return Err(GitWorkspaceError::InvalidRequest(
                "writer scope cannot include .git".to_string(),
            ));
        }
        normalized.insert(path);
    }
    Ok(normalized.into_iter().collect())
}

fn validate_relative_path(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() || path == Path::new(".") {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "{label} must name a repository-relative file or directory"
        )));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "{label} {} contains an absolute, parent, or non-normal component",
            path.display()
        )));
    }
    Ok(())
}

fn validate_branch_ref(repo: &Path, branch_ref: &str) -> Result<()> {
    let output = git_probe(
        repo,
        [
            OsString::from("check-ref-format"),
            branch_ref.to_string().into(),
        ],
    )?;
    if !output.status.success() {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "derived writer branch is invalid: {branch_ref}"
        )));
    }
    Ok(())
}

fn validate_derived_path(managed_root: &Path, path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        GitWorkspaceError::InvalidRequest("derived worktree has no parent".to_string())
    })?;
    if parent != managed_root {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "derived worktree {} is not a direct child of managed root {}",
            path.display(),
            managed_root.display()
        )));
    }
    Ok(())
}

fn validate_changed_paths(allocation: &OwnedWorktree, paths: &[PathBuf]) -> Result<()> {
    if paths.len() > allocation.limits.max_changed_files {
        return Err(GitWorkspaceError::LimitExceeded(format!(
            "{} changed files exceed limit {}",
            paths.len(),
            allocation.limits.max_changed_files
        )));
    }
    let mut total = 0u64;
    for path in paths {
        validate_relative_path(path, "changed path")?;
        if !allocation
            .allowed_paths
            .iter()
            .any(|allowed| path == allowed || path.starts_with(allowed))
        {
            return Err(GitWorkspaceError::Conflict(format!(
                "writer changed path outside allowed scope: {}",
                path.display()
            )));
        }
        let absolute = allocation.worktree_path.join(path);
        if let Some(metadata) = symlink_metadata_optional(&absolute)? {
            let file_type = metadata.file_type();
            if !file_type.is_file() {
                return Err(GitWorkspaceError::Conflict(format!(
                    "writer changed special filesystem entry: {}",
                    path.display()
                )));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    return Err(GitWorkspaceError::Conflict(format!(
                        "writer changed hard-linked file: {}",
                        path.display()
                    )));
                }
            }
            if metadata.len() > allocation.limits.max_file_bytes {
                return Err(GitWorkspaceError::LimitExceeded(format!(
                    "{} is {} bytes, per-file limit is {}",
                    path.display(),
                    metadata.len(),
                    allocation.limits.max_file_bytes
                )));
            }
            total = total.saturating_add(metadata.len());
            if total > allocation.limits.max_total_file_bytes {
                return Err(GitWorkspaceError::LimitExceeded(format!(
                    "changed files total {total} bytes, limit is {}",
                    allocation.limits.max_total_file_bytes
                )));
            }
        }
    }
    Ok(())
}

fn validate_changed_modes(
    worktree: &Path,
    base: &str,
    changed: &[PathBuf],
    limits: SealLimits,
) -> Result<()> {
    let changed = changed.iter().cloned().collect::<BTreeSet<_>>();
    let mut index_args = vec![
        OsString::from("ls-files"),
        OsString::from("--stage"),
        OsString::from("-z"),
        OsString::from("--"),
    ];
    index_args.extend(changed.iter().map(|path| path.as_os_str().to_owned()));
    let index = git_checked_bounded(
        worktree,
        "inspect sealed index modes",
        index_args,
        cleanup_path_output_limit(limits),
    )?;
    for (mode, path) in parse_mode_path_records(&index)? {
        if changed.contains(&path) && mode != "100644" && mode != "100755" {
            return Err(GitWorkspaceError::Conflict(format!(
                "writer index contains unsupported mode {mode} at {}",
                path.display()
            )));
        }
    }
    let mut base_args = vec![
        OsString::from("ls-tree"),
        OsString::from("-r"),
        OsString::from("-z"),
        base.to_string().into(),
        OsString::from("--"),
    ];
    base_args.extend(changed.iter().map(|path| path.as_os_str().to_owned()));
    let base_tree = git_checked_bounded(
        worktree,
        "inspect base tree modes",
        base_args,
        cleanup_path_output_limit(limits),
    )?;
    for (mode, path) in parse_mode_path_records(&base_tree)? {
        if changed.contains(&path) && mode != "100644" && mode != "100755" {
            return Err(GitWorkspaceError::Conflict(format!(
                "writer changed unsupported base mode {mode} at {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn parse_mode_path_records(bytes: &[u8]) -> Result<Vec<(String, PathBuf)>> {
    let mut records = Vec::new();
    for entry in bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "Git mode record lacked path separator".to_string(),
            ));
        };
        let header = &entry[..tab];
        let mode_end = header
            .iter()
            .position(|byte| *byte == b' ')
            .unwrap_or(header.len());
        let mode = std::str::from_utf8(&header[..mode_end])
            .map_err(|_| {
                GitWorkspaceError::OwnershipMismatch("Git mode record was not ASCII".to_string())
            })?
            .to_string();
        records.push((mode, path_from_git_bytes(&entry[tab + 1..])?));
    }
    Ok(records)
}

fn collect_worktree_changes(worktree: &Path, limits: SealLimits) -> Result<Vec<PathBuf>> {
    let output_limit = cleanup_path_output_limit(limits);
    let tracked = git_checked_bounded_nul_records(
        worktree,
        "collect tracked writer changes",
        [
            OsString::from("-c"),
            OsString::from("diff.renames=false"),
            OsString::from("diff"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            OsString::from("HEAD"),
            OsString::from("--"),
        ],
        output_limit,
        limits.max_changed_files,
    )?;
    let untracked = git_checked_bounded_nul_records(
        worktree,
        "collect untracked writer changes",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
        output_limit,
        limits.max_changed_files,
    )?;
    let mut paths = parse_nul_paths(&tracked, limits.max_changed_files)?;
    paths.extend(parse_nul_paths(&untracked, limits.max_changed_files)?);
    ensure_path_count(&paths, limits.max_changed_files)?;
    Ok(paths.into_iter().collect())
}

fn collect_cleanup_changes(worktree: &Path, limits: SealLimits) -> Result<Vec<PathBuf>> {
    let mut paths = collect_worktree_changes(worktree, limits)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let ignored = git_checked_bounded_nul_records(
        worktree,
        "collect ignored writer cleanup paths",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--ignored"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
        cleanup_path_output_limit(limits),
        limits.max_changed_files,
    )?;
    paths.extend(parse_nul_paths(&ignored, limits.max_changed_files)?);
    ensure_path_count(&paths, limits.max_changed_files)?;
    Ok(paths.into_iter().collect())
}

fn cleanup_path_output_limit(limits: SealLimits) -> usize {
    limits
        .max_changed_files
        .saturating_add(1)
        .saturating_mul(MAX_CLEANUP_PATH_BYTES_PER_ENTRY)
        .min(MAX_CLEANUP_PATH_OUTPUT_BYTES)
}

fn cleanup_scope_facts(
    worktree: &Path,
    paths: Vec<PathBuf>,
    limits: SealLimits,
) -> Result<CleanupScopeFacts> {
    if paths.len() > limits.max_changed_files {
        return Err(GitWorkspaceError::LimitExceeded(format!(
            "{} cleanup paths exceed limit {}",
            paths.len(),
            limits.max_changed_files
        )));
    }
    let mut hasher = Sha256::new();
    let mut total_file_bytes = 0_u64;
    for path in &paths {
        validate_relative_path(path, "cleanup path")?;
        hash_cleanup_segment(&mut hasher, b"path", path.as_os_str().as_encoded_bytes());
        let absolute = worktree.join(path);
        let Some(metadata) = symlink_metadata_optional(&absolute)? else {
            hash_cleanup_segment(&mut hasher, b"kind", b"absent");
            continue;
        };
        let file_type = metadata.file_type();
        if file_type.is_file() {
            if metadata.len() > limits.max_file_bytes {
                return Err(GitWorkspaceError::LimitExceeded(format!(
                    "cleanup file {} is {} bytes, per-file limit is {}",
                    path.display(),
                    metadata.len(),
                    limits.max_file_bytes
                )));
            }
            total_file_bytes = total_file_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| {
                    GitWorkspaceError::LimitExceeded(
                        "cleanup file byte count overflowed".to_string(),
                    )
                })?;
            if total_file_bytes > limits.max_total_file_bytes {
                return Err(GitWorkspaceError::LimitExceeded(format!(
                    "cleanup files total {total_file_bytes} bytes, limit is {}",
                    limits.max_total_file_bytes
                )));
            }
            hash_cleanup_segment(&mut hasher, b"kind", b"file");
            hash_cleanup_segment(&mut hasher, b"length", &metadata.len().to_le_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                hash_cleanup_segment(&mut hasher, b"mode", &metadata.mode().to_le_bytes());
            }
            let mut file = File::open(&absolute).map_err(|source| GitWorkspaceError::Io {
                operation: "open writer cleanup path",
                path: absolute.clone(),
                source,
            })?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|source| GitWorkspaceError::Io {
                        operation: "read writer cleanup path",
                        path: absolute.clone(),
                        source,
                    })?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        } else if file_type.is_symlink() {
            hash_cleanup_segment(&mut hasher, b"kind", b"symlink");
            let target = fs::read_link(&absolute).map_err(|source| GitWorkspaceError::Io {
                operation: "read writer cleanup symlink",
                path: absolute.clone(),
                source,
            })?;
            hash_cleanup_segment(
                &mut hasher,
                b"target",
                target.as_os_str().as_encoded_bytes(),
            );
        } else {
            return Err(GitWorkspaceError::Conflict(format!(
                "writer cleanup path is a special filesystem entry: {}",
                path.display()
            )));
        }
    }
    Ok(CleanupScopeFacts {
        paths,
        revision_sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

fn hash_cleanup_segment(hasher: &mut Sha256, label: &[u8], bytes: &[u8]) {
    hasher.update((label.len() as u64).to_le_bytes());
    hasher.update(label);
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn staged_changed_paths(worktree: &Path, base: &str, limits: SealLimits) -> Result<Vec<PathBuf>> {
    let bytes = git_checked_bounded_nul_records(
        worktree,
        "collect staged writer changes",
        [
            OsString::from("diff"),
            OsString::from("--cached"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--no-renames"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            base.to_string().into(),
            OsString::from("--"),
        ],
        cleanup_path_output_limit(limits),
        limits.max_changed_files,
    )?;
    Ok(parse_nul_paths(&bytes, limits.max_changed_files)?
        .into_iter()
        .collect())
}

fn parse_nul_paths(bytes: &[u8], max_entries: usize) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    for raw in bytes.split(|byte| *byte == 0).filter(|raw| !raw.is_empty()) {
        paths.insert(path_from_git_bytes(raw)?);
        ensure_path_count(&paths, max_entries)?;
    }
    Ok(paths)
}

fn ensure_path_count(paths: &BTreeSet<PathBuf>, max_entries: usize) -> Result<()> {
    if paths.len() > max_entries {
        return Err(GitWorkspaceError::LimitExceeded(format!(
            "{} changed paths exceed limit {max_entries}",
            paths.len()
        )));
    }
    Ok(())
}

fn binary_diff_cached(worktree: &Path, base: &str, max_bytes: usize) -> Result<Vec<u8>> {
    git_checked_bounded(
        worktree,
        "render staged binary diff",
        [
            OsString::from("diff"),
            OsString::from("--cached"),
            OsString::from("--binary"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--no-renames"),
            base.to_string().into(),
            OsString::from("--"),
        ],
        max_bytes,
    )
}

fn binary_diff_commits(
    repo: &Path,
    base: &str,
    final_commit: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    git_checked_bounded(
        repo,
        "render sealed binary diff",
        [
            OsString::from("diff"),
            OsString::from("--binary"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--no-renames"),
            base.to_string().into(),
            final_commit.to_string().into(),
            OsString::from("--"),
        ],
        max_bytes,
    )
}

fn verify_sealed_commit(
    repo: &Path,
    allocation: &OwnedWorktree,
    sealed: &SealedWorktree,
) -> Result<()> {
    verify_host_seal_identity(repo, allocation, sealed.final_commit())?;
    let ancestry = git_text(
        repo,
        "inspect sealed commit ancestry",
        [
            OsString::from("rev-list"),
            OsString::from("--parents"),
            OsString::from("-n"),
            OsString::from("1"),
            sealed.final_commit.clone().into(),
        ],
    )?;
    let ancestry = ancestry.split_ascii_whitespace().collect::<Vec<_>>();
    if ancestry.as_slice()
        != [
            sealed.final_commit.as_str(),
            allocation.base_commit.as_str(),
        ]
    {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "sealed commit ancestry is {ancestry:?}, expected one parent {}",
            allocation.base_commit,
        )));
    }
    let tree = commit_tree(repo, sealed.final_commit())?;
    if tree != sealed.tree {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "sealed commit tree no longer matches Host result".to_string(),
        ));
    }
    let diff = binary_diff_commits(
        repo,
        allocation.base_commit(),
        sealed.final_commit(),
        allocation.limits.max_diff_bytes,
    )?;
    if sha256_hex(&diff) != sealed.diff_sha256 || diff != sealed.diff {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "sealed commit diff no longer matches Host result".to_string(),
        ));
    }
    let changed = committed_changed_paths(
        repo,
        allocation.base_commit(),
        sealed.final_commit(),
        allocation.limits,
    )?;
    if changed != sealed.changed_files {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "sealed commit changed-file set no longer matches Host result".to_string(),
        ));
    }
    Ok(())
}

fn verify_host_seal_identity(
    repo: &Path,
    allocation: &OwnedWorktree,
    final_commit: &str,
) -> Result<()> {
    verify_host_seal_identity_for_owner(repo, &allocation.owner_id, final_commit)
}

fn verify_host_seal_identity_for_owner(
    repo: &Path,
    owner_id: &str,
    final_commit: &str,
) -> Result<()> {
    let identity = git_checked(
        repo,
        "inspect prepared seal identity",
        [
            OsString::from("show"),
            OsString::from("-s"),
            OsString::from("--format=%an%x00%ae%x00%cn%x00%ce%x00%s"),
            final_commit.to_string().into(),
        ],
    )?;
    let expected = format!(
        "DSE Host\0host@dse.local\0DSE Host\0host@dse.local\0DSE writer {}\n",
        owner_id
    );
    if identity != expected.as_bytes() {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "prepared seal commit lacks the exact Host owner identity".to_string(),
        ));
    }
    Ok(())
}

fn committed_changed_paths(
    repo: &Path,
    base: &str,
    final_commit: &str,
    limits: SealLimits,
) -> Result<Vec<PathBuf>> {
    let bytes = git_checked_bounded_nul_records(
        repo,
        "collect sealed changed paths",
        [
            OsString::from("diff"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--no-renames"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            base.to_string().into(),
            final_commit.to_string().into(),
            OsString::from("--"),
        ],
        cleanup_path_output_limit(limits),
        limits.max_changed_files,
    )?;
    Ok(parse_nul_paths(&bytes, limits.max_changed_files)?
        .into_iter()
        .collect())
}

fn commit_tree(repo: &Path, commit: &str) -> Result<String> {
    git_text(
        repo,
        "inspect commit tree",
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            format!("{commit}^{{tree}}").into(),
        ],
    )
}

fn ensure_no_unstaged_or_untracked_changes(worktree: &Path, limits: SealLimits) -> Result<()> {
    let unstaged = git_probe(
        worktree,
        [
            OsString::from("diff"),
            OsString::from("--quiet"),
            OsString::from("--"),
        ],
    )?;
    if unstaged.status.code() == Some(1) {
        return Err(GitWorkspaceError::Conflict(
            "writer filesystem changed while Host was staging it".to_string(),
        ));
    }
    if !unstaged.status.success() {
        return Err(git_failure("check unstaged writer changes", &unstaged));
    }
    let untracked = git_checked_bounded_nul_records(
        worktree,
        "check untracked writer changes",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
        cleanup_path_output_limit(limits),
        limits.max_changed_files,
    )?;
    if !untracked.is_empty() {
        return Err(GitWorkspaceError::Conflict(
            "writer filesystem gained untracked files while Host was staging it".to_string(),
        ));
    }
    Ok(())
}

fn ensure_clean(repo: &Path, label: &str) -> Result<()> {
    let status = repository_status(repo)?;
    if !status.is_empty() {
        let entries = status
            .split(|byte| *byte == 0)
            .filter(|e| !e.is_empty())
            .count();
        return Err(GitWorkspaceError::DirtyRepository(format!(
            "{label} has {entries} changed or untracked entries"
        )));
    }
    Ok(())
}

fn repository_status(repo: &Path) -> Result<Vec<u8>> {
    git_checked(
        repo,
        "inspect repository status",
        [
            OsString::from("status"),
            OsString::from("--porcelain=v2"),
            OsString::from("-z"),
            OsString::from("--untracked-files=all"),
        ],
    )
}

fn root_checkout_is_clean(repo: &Path) -> Result<bool> {
    Ok(repository_status(repo)?.is_empty())
}

/// Prove that the root index and filesystem still represent `commit`,
/// independent of what symbolic `HEAD` currently resolves to.
fn root_checkout_matches_commit(repo: &Path, commit: &str) -> Result<bool> {
    let index = git_probe(
        repo,
        [
            OsString::from("diff"),
            OsString::from("--cached"),
            OsString::from("--quiet"),
            commit.to_string().into(),
            OsString::from("--"),
        ],
    )?;
    if index.status.code() == Some(1) {
        return Ok(false);
    }
    if !index.status.success() {
        return Err(git_failure(
            "compare root index with recovery commit",
            &index,
        ));
    }

    let worktree = git_probe(
        repo,
        [
            OsString::from("diff"),
            OsString::from("--quiet"),
            OsString::from("--"),
        ],
    )?;
    if worktree.status.code() == Some(1) {
        return Ok(false);
    }
    if !worktree.status.success() {
        return Err(git_failure(
            "compare root worktree with recovery index",
            &worktree,
        ));
    }

    let untracked = git_checked(
        repo,
        "inspect untracked root files during integration recovery",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
    )?;
    Ok(untracked.is_empty())
}

fn materialize_root_fast_forward(repo: &Path, base: &str, final_commit: &str) -> Result<()> {
    git_checked(
        repo,
        "materialize sealed root tree",
        [
            OsString::from("read-tree"),
            OsString::from("-m"),
            OsString::from("-u"),
            base.to_string().into(),
            final_commit.to_string().into(),
        ],
    )?;
    ensure_clean(repo, "root repository after integration materialization")
}

fn resolve_exact_commit(repo: &Path, requested: &str) -> Result<String> {
    validate_full_object_id_claim(requested)?;
    let resolved = git_text(
        repo,
        "resolve exact commit",
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            format!("{requested}^{{commit}}").into(),
        ],
    )?;
    if !resolved.eq_ignore_ascii_case(requested) {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "requested commit {requested} resolved to different object {resolved}"
        )));
    }
    Ok(resolved)
}

fn validate_full_object_id_claim(requested: &str) -> Result<()> {
    if !matches!(requested.len(), 40 | 64)
        || !requested.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "base/expected commit must be a full hexadecimal object id".to_string(),
        ));
    }
    Ok(())
}

fn resolve_head(repo: &Path) -> Result<String> {
    git_text(
        repo,
        "resolve HEAD commit",
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("HEAD^{commit}"),
        ],
    )
}

fn symbolic_head(repo: &Path) -> Result<String> {
    git_text(
        repo,
        "resolve symbolic HEAD",
        [
            OsString::from("symbolic-ref"),
            OsString::from("-q"),
            OsString::from("HEAD"),
        ],
    )
}

fn resolve_ref_optional(repo: &Path, reference: &str) -> Result<Option<String>> {
    let output = git_probe(
        repo,
        [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            format!("{reference}^{{commit}}").into(),
        ],
    )?;
    if output.status.success() {
        return Ok(Some(output_text(&output, "resolved ref")?));
    }
    if output.status.code() == Some(128) {
        return Ok(None);
    }
    Err(git_failure("resolve optional ref", &output))
}

fn require_ref(repo: &Path, reference: &str, expected: &str) -> Result<()> {
    let actual = resolve_ref_optional(repo, reference)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(format!("required writer ref is missing: {reference}"))
    })?;
    if actual != expected {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "writer ref {reference} points to {actual}, expected {expected}"
        )));
    }
    Ok(())
}

fn ensure_writer_branch_not_checked_out_elsewhere(
    repo: &Path,
    facts: &OwnedWorktreeFacts,
) -> Result<()> {
    if registered_worktrees(repo)?.into_iter().any(|record| {
        record.branch_ref.as_deref() == Some(facts.branch_ref.as_str())
            && !same_path(&record.path, &facts.worktree_path)
    }) {
        return Err(GitWorkspaceError::Conflict(
            "Writer branch is checked out by another worktree".to_string(),
        ));
    }
    Ok(())
}

fn retain_writer_branch_for_foreign_checkout(
    repo: &Path,
    facts: &OwnedWorktreeFacts,
    expected: &str,
) -> Result<()> {
    let foreign_checkout = registered_worktrees(repo)?.into_iter().any(|record| {
        record.branch_ref.as_deref() == Some(facts.branch_ref.as_str())
            && !same_path(&record.path, &facts.worktree_path)
    });
    if !foreign_checkout {
        return Ok(());
    }
    if resolve_ref_optional(repo, &facts.branch_ref)?.is_none() {
        create_exact_ref_cas(
            repo,
            &facts.branch_ref,
            expected,
            "restore Writer branch for a concurrent foreign checkout",
        )?;
    }
    Err(GitWorkspaceError::Conflict(
        "a concurrent worktree checkout claimed the Writer branch; the branch was retained"
            .to_string(),
    ))
}

fn ensure_cleanup_root_is_registered(repo: &Path) -> Result<()> {
    if !registered_worktrees(repo)?
        .iter()
        .any(|record| same_path(&record.path, repo))
    {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "Git worktree registry no longer contains the canonical root".to_string(),
        ));
    }
    Ok(())
}

fn cleanup_tombstone_ref(facts: &OwnedWorktreeFacts, expected: &str) -> Result<String> {
    validate_owner_id(&facts.owner_id)?;
    if !matches!(expected.len(), 40 | 64) || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "Writer cleanup tombstone requires a full hexadecimal object id".to_string(),
        ));
    }
    let reference = format!("refs/dse/cleanup/{}/{}", facts.owner_id, expected);
    Ok(reference)
}

fn cleanup_worktree_lock_reason(facts: &OwnedWorktreeFacts, expected: &str) -> String {
    format!("dse-cleanup-v1:{}:{expected}", facts.owner_id)
}

fn create_exact_ref_cas(
    repo: &Path,
    reference: &str,
    expected: &str,
    operation: &'static str,
) -> Result<()> {
    git_checked(
        repo,
        operation,
        [
            OsString::from("update-ref"),
            reference.into(),
            expected.into(),
            "0".repeat(expected.len()).into(),
        ],
    )?;
    require_ref(repo, reference, expected)
}

fn delete_exact_ref_cas(
    repo: &Path,
    reference: &str,
    expected: &str,
    operation: &'static str,
) -> Result<()> {
    let Some(actual) = resolve_ref_optional(repo, reference)? else {
        return Ok(());
    };
    if actual != expected {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "ref {reference} points to {actual}, expected {expected}"
        )));
    }
    git_checked(
        repo,
        operation,
        [
            OsString::from("update-ref"),
            OsString::from("-d"),
            reference.into(),
            expected.into(),
        ],
    )?;
    if resolve_ref_optional(repo, reference)?.is_some() {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "ref {reference} remained after compare-and-swap deletion"
        )));
    }
    Ok(())
}

fn create_cleanup_guard_worktree(
    repo: &Path,
    facts: &OwnedWorktreeFacts,
    expected: &str,
    lock_reason: &str,
) -> Result<()> {
    if symlink_metadata_optional(&facts.worktree_path)?.is_some()
        || exact_registered_worktree(repo, facts)?.is_some()
    {
        return Err(GitWorkspaceError::Conflict(
            "cannot create a cleanup guard over an existing Writer resource".to_string(),
        ));
    }
    require_ref(repo, &facts.branch_ref, expected)?;
    git_checked(
        repo,
        "create locked Writer cleanup guard worktree",
        [
            OsString::from("worktree"),
            OsString::from("add"),
            OsString::from("--no-checkout"),
            OsString::from("--lock"),
            OsString::from("--reason"),
            lock_reason.into(),
            facts.worktree_path.as_os_str().to_owned(),
            branch_ref_to_short(&facts.branch_ref)?.into(),
        ],
    )?;
    let record = exact_registered_worktree(repo, facts)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(
            "Git did not register the exact Writer cleanup guard".to_string(),
        )
    })?;
    verify_record_facts(&record, facts, expected)?;
    if record.locked_reason.as_deref() != Some(lock_reason) {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "Writer cleanup guard was not created with the exact lock reason".to_string(),
        ));
    }
    Ok(())
}

fn acquire_cleanup_worktree_lock(
    repo: &Path,
    facts: &OwnedWorktreeFacts,
    expected: &str,
) -> Result<()> {
    let reason = cleanup_worktree_lock_reason(facts, expected);
    let record = exact_registered_worktree(repo, facts)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(
            "exact Writer worktree disappeared before cleanup lock".to_string(),
        )
    })?;
    match record.locked_reason.as_deref() {
        Some(actual) if actual == reason => return Ok(()),
        Some(_) => {
            return Err(GitWorkspaceError::Conflict(
                "Writer worktree has a foreign persistent lock".to_string(),
            ));
        }
        None => {}
    }
    git_checked(
        repo,
        "lock exact Writer worktree for cleanup",
        [
            OsString::from("worktree"),
            OsString::from("lock"),
            OsString::from("--reason"),
            reason.clone().into(),
            facts.worktree_path.as_os_str().to_owned(),
        ],
    )?;
    let locked = exact_registered_worktree(repo, facts)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(
            "exact Writer worktree disappeared after cleanup lock".to_string(),
        )
    })?;
    if locked.locked_reason.as_deref() != Some(reason.as_str()) {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "Writer worktree cleanup lock reason changed unexpectedly".to_string(),
        ));
    }
    Ok(())
}

fn verify_branchless_cleanup_worktree(
    repo: &Path,
    common_git_dir: &Path,
    facts: &OwnedWorktreeFacts,
    expected: &str,
) -> Result<()> {
    if resolve_ref_optional(repo, &facts.branch_ref)?.is_some() {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "branchless cleanup verification observed a live Writer branch".to_string(),
        ));
    }
    let record = exact_registered_worktree(repo, facts)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(
            "branchless Writer cleanup lacks its exact worktree registration".to_string(),
        )
    })?;
    if record.branch_ref.as_deref() != Some(facts.branch_ref.as_str())
        || record.head.len() != expected.len()
        || !record.head.bytes().all(|byte| byte == b'0')
    {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "branchless Writer registration does not have the expected dangling HEAD identity"
                .to_string(),
        ));
    }
    let admin_dir = owned_admin_entry(common_git_dir, facts)?.ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(
            "branchless Writer cleanup lacks its exact Git admin entry".to_string(),
        )
    })?;
    verify_admin_backpointer_facts(&admin_dir, &facts.worktree_path)?;
    if symlink_metadata_optional(&facts.worktree_path)?.is_some() {
        let canonical_worktree = canonical_dir(
            &facts.worktree_path,
            "canonicalize branchless Writer cleanup worktree",
        )?;
        if canonical_worktree != facts.worktree_path {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "branchless Writer cleanup path changed identity".to_string(),
            ));
        }
        verify_worktree_pointer_to_admin(&facts.worktree_path, &admin_dir)?;
    }
    Ok(())
}

fn delete_exact_writer_ref_cas(
    repo: &Path,
    facts: &OwnedWorktreeFacts,
    expected: &str,
) -> Result<CleanupComponentDisposition> {
    let Some(actual) = resolve_ref_optional(repo, &facts.branch_ref)? else {
        return Ok(CleanupComponentDisposition::AlreadyAbsent);
    };
    if actual != expected {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "writer branch {} points to {actual}, expected {expected}",
            facts.branch_ref
        )));
    }
    ensure_writer_branch_not_checked_out_elsewhere(repo, facts)?;
    git_checked(
        repo,
        "delete exact owned writer branch",
        [
            OsString::from("update-ref"),
            OsString::from("-d"),
            facts.branch_ref.clone().into(),
            expected.to_owned().into(),
        ],
    )?;
    if resolve_ref_optional(repo, &facts.branch_ref)?.is_some() {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "exact Writer branch remained after compare-and-swap deletion".to_string(),
        ));
    }
    Ok(CleanupComponentDisposition::Removed)
}

fn branch_ref_to_short(branch_ref: &str) -> Result<String> {
    branch_ref
        .strip_prefix("refs/heads/")
        .map(str::to_string)
        .ok_or_else(|| {
            GitWorkspaceError::InvalidRequest(format!(
                "writer ref is not a local branch: {branch_ref}"
            ))
        })
}

fn registered_worktrees(repo: &Path) -> Result<Vec<WorktreeRecord>> {
    let bytes = git_checked_bounded_double_nul_records(
        repo,
        "list registered worktrees",
        [
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
            OsString::from("-z"),
        ],
        MAX_REGISTERED_WORKTREE_OUTPUT_BYTES,
        MAX_REGISTERED_WORKTREES,
    )?;
    let mut records = Vec::new();
    let mut fields = BTreeMap::<String, Vec<u8>>::new();
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if !fields.is_empty() {
                records.push(worktree_record_from_fields(&fields)?);
                if records.len() > MAX_REGISTERED_WORKTREES {
                    return Err(GitWorkspaceError::LimitExceeded(format!(
                        "Git worktree registry exceeds {MAX_REGISTERED_WORKTREES} records"
                    )));
                }
                fields.clear();
            }
            continue;
        }
        let (key, value) = if let Some(index) = field.iter().position(|byte| *byte == b' ') {
            (&field[..index], &field[index + 1..])
        } else {
            (field, &b""[..])
        };
        let key = std::str::from_utf8(key)
            .map_err(|_| {
                GitWorkspaceError::OwnershipMismatch(
                    "worktree registry key was not ASCII".to_string(),
                )
            })?
            .to_string();
        fields.insert(key, value.to_vec());
    }
    if !fields.is_empty() {
        records.push(worktree_record_from_fields(&fields)?);
        if records.len() > MAX_REGISTERED_WORKTREES {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "Git worktree registry exceeds {MAX_REGISTERED_WORKTREES} records"
            )));
        }
    }
    Ok(records)
}

fn worktree_record_from_fields(fields: &BTreeMap<String, Vec<u8>>) -> Result<WorktreeRecord> {
    let path = fields
        .get("worktree")
        .ok_or_else(|| {
            GitWorkspaceError::OwnershipMismatch("worktree registry record lacked path".to_string())
        })
        .and_then(|raw| path_from_git_bytes(raw))?;
    let head = fields
        .get("HEAD")
        .ok_or_else(|| {
            GitWorkspaceError::OwnershipMismatch("worktree registry record lacked HEAD".to_string())
        })
        .and_then(|raw| ascii_string(raw, "worktree HEAD"))?;
    let branch_ref = fields
        .get("branch")
        .map(|raw| ascii_string(raw, "worktree branch"))
        .transpose()?;
    let locked_reason = fields
        .get("locked")
        .map(|raw| ascii_string(raw, "worktree lock reason"))
        .transpose()?;
    Ok(WorktreeRecord {
        path,
        head,
        branch_ref,
        locked_reason,
    })
}

fn verify_record(
    record: &WorktreeRecord,
    allocation: &OwnedWorktree,
    expected: &str,
) -> Result<()> {
    verify_record_identity(record, &allocation.branch_ref, expected)
}

fn verify_record_facts(
    record: &WorktreeRecord,
    facts: &OwnedWorktreeFacts,
    expected: &str,
) -> Result<()> {
    verify_record_identity(record, &facts.branch_ref, expected)
}

fn verify_record_identity(
    record: &WorktreeRecord,
    expected_branch_ref: &str,
    expected: &str,
) -> Result<()> {
    if record.head != expected {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "registered worktree HEAD is {}, expected {expected}",
            record.head
        )));
    }
    if record.branch_ref.as_deref() != Some(expected_branch_ref) {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "registered worktree branch is {:?}, expected {}",
            record.branch_ref, expected_branch_ref
        )));
    }
    Ok(())
}

fn verify_worktree_backpointer(allocation: &OwnedWorktree) -> Result<()> {
    verify_worktree_pointer_to_admin(&allocation.worktree_path, &allocation.worktree_git_dir)?;
    let admin_parent = allocation.common_git_dir.join("worktrees");
    let admin_parent = canonical_dir(&admin_parent, "canonicalize worktree admin root")?;
    if !allocation.worktree_git_dir.starts_with(admin_parent) {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "worktree admin directory is outside common Git worktrees directory".to_string(),
        ));
    }
    Ok(())
}

fn verify_worktree_pointer_to_admin(worktree_path: &Path, admin_dir: &Path) -> Result<()> {
    let pointer_path = worktree_path.join(".git");
    let metadata = fs::symlink_metadata(&pointer_path).map_err(|source| GitWorkspaceError::Io {
        operation: "stat worktree .git pointer",
        path: pointer_path.clone(),
        source,
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_GIT_POINTER_BYTES {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "{} is not a bounded Git pointer file",
            pointer_path.display()
        )));
    }
    let pointer = read_bounded_pointer(&pointer_path)?;
    let raw = pointer.strip_prefix("gitdir: ").ok_or_else(|| {
        GitWorkspaceError::OwnershipMismatch(format!(
            "{} lacks gitdir pointer",
            pointer_path.display()
        ))
    })?;
    let target = resolve_pointer_path(worktree_path, Path::new(raw));
    let target = canonical_dir(&target, "canonicalize .git pointer target")?;
    let expected_admin = canonical_dir(admin_dir, "canonicalize expected worktree admin dir")?;
    if target != expected_admin {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "worktree .git points to {}, expected {}",
            target.display(),
            expected_admin.display()
        )));
    }
    Ok(())
}

fn verify_admin_backpointer(allocation: &OwnedWorktree) -> Result<()> {
    verify_admin_backpointer_facts(&allocation.worktree_git_dir, &allocation.worktree_path)
}

fn verify_admin_backpointer_facts(admin_dir: &Path, worktree_path: &Path) -> Result<()> {
    let backpointer = admin_dir.join("gitdir");
    let raw = read_bounded_pointer(&backpointer)?;
    let target = resolve_pointer_path(admin_dir, Path::new(&raw));
    let expected = worktree_path.join(".git");
    if !same_path(&target, &expected) {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "worktree admin backpointer targets {}, expected {}",
            target.display(),
            expected.display()
        )));
    }
    Ok(())
}

fn exact_registered_worktree(
    repository_root: &Path,
    facts: &OwnedWorktreeFacts,
) -> Result<Option<WorktreeRecord>> {
    let mut matching = registered_worktrees(repository_root)?
        .into_iter()
        .filter(|record| same_path(&record.path, &facts.worktree_path));
    let record = matching.next();
    if matching.next().is_some() {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "multiple Git worktree records target the exact Writer path".to_string(),
        ));
    }
    Ok(record)
}

fn owned_admin_entry(common_git_dir: &Path, facts: &OwnedWorktreeFacts) -> Result<Option<PathBuf>> {
    let admin_root = common_git_dir.join("worktrees");
    if symlink_metadata_optional(&admin_root)?.is_none() {
        return Ok(None);
    }
    let expected = facts.worktree_path.join(".git");
    let entries = fs::read_dir(&admin_root).map_err(|source| GitWorkspaceError::Io {
        operation: "list Git worktree admin directory",
        path: admin_root.clone(),
        source,
    })?;
    let mut matching = None;
    for entry in entries {
        let entry = entry.map_err(|source| GitWorkspaceError::Io {
            operation: "read Git worktree admin entry",
            path: admin_root.clone(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&facts.owner_id) {
            continue;
        }
        let admin_dir = entry.path();
        let metadata =
            fs::symlink_metadata(&admin_dir).map_err(|source| GitWorkspaceError::Io {
                operation: "stat Git worktree admin entry",
                path: admin_dir.clone(),
                source,
            })?;
        if !metadata.file_type().is_dir() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "Writer-prefixed Git admin entry is not a directory".to_string(),
            ));
        }
        let head_path = admin_dir.join("HEAD");
        if symlink_metadata_optional(&head_path)?.is_none() {
            continue;
        }
        let head = read_bounded_pointer(&head_path)?;
        if head.trim_end() != format!("ref: {}", facts.branch_ref) {
            continue;
        }
        let backpointer = admin_dir.join("gitdir");
        if symlink_metadata_optional(&backpointer)?.is_none() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "Writer-prefixed Git admin entry lacks a backpointer".to_string(),
            ));
        }
        let raw = read_bounded_pointer(&backpointer)?;
        let target = resolve_pointer_path(&admin_dir, Path::new(&raw));
        if !same_path(&target, &expected) {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "exact Writer Git admin entry targets another worktree path".to_string(),
            ));
        }
        if matching.is_some() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "multiple Git admin entries target the exact Writer path".to_string(),
            ));
        }
        matching = Some(admin_dir);
    }
    Ok(matching)
}

fn read_bounded_pointer(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path).map_err(|source| GitWorkspaceError::Io {
        operation: "stat Git pointer",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_GIT_POINTER_BYTES {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "Git pointer {} exceeds size bound",
            path.display()
        )));
    }
    let bytes = fs::read(path).map_err(|source| GitWorkspaceError::Io {
        operation: "read Git pointer",
        path: path.to_path_buf(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        GitWorkspaceError::OwnershipMismatch(format!("Git pointer {} is not UTF-8", path.display()))
    })?;
    Ok(text.trim().to_string())
}

fn resolve_pointer_path(base: &Path, pointer: &Path) -> PathBuf {
    if pointer.is_absolute() {
        pointer.to_path_buf()
    } else {
        base.join(pointer)
    }
}

fn rev_parse_path(repo: &Path, arg: &str, operation: &str) -> Result<PathBuf> {
    let text = git_text(
        repo,
        operation,
        [OsString::from("rev-parse"), OsString::from(arg)],
    )?;
    let path = PathBuf::from(text);
    Ok(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

fn git_text<I>(repo: &Path, operation: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    let bytes = git_checked(repo, operation, args)?;
    ascii_string(trim_ascii(&bytes), operation)
}

fn git_text_with_identity<I>(repo: &Path, operation: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = OsString>,
{
    let output = git_output(repo, args, true)?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    ascii_string(trim_ascii(&output.stdout), operation)
}

fn git_checked<I>(repo: &Path, operation: &str, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = OsString>,
{
    let output = git_output(repo, args, false)?;
    if !output.status.success() {
        return Err(git_failure(operation, &output));
    }
    Ok(output.stdout)
}

fn git_checked_bounded<I>(
    repo: &Path,
    operation: &str,
    args: I,
    stdout_limit: usize,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = OsString>,
{
    git_checked_bounded_inner(repo, operation, args, stdout_limit, None)
}

fn git_checked_bounded_nul_records<I>(
    repo: &Path,
    operation: &str,
    args: I,
    stdout_limit: usize,
    max_records: usize,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = OsString>,
{
    git_checked_bounded_inner(
        repo,
        operation,
        args,
        stdout_limit,
        Some(BoundedRecordCounter::nul(max_records)),
    )
}

fn git_checked_bounded_double_nul_records<I>(
    repo: &Path,
    operation: &str,
    args: I,
    stdout_limit: usize,
    max_records: usize,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = OsString>,
{
    git_checked_bounded_inner(
        repo,
        operation,
        args,
        stdout_limit,
        Some(BoundedRecordCounter::double_nul(max_records)),
    )
}

fn git_checked_bounded_inner<I>(
    repo: &Path,
    operation: &str,
    args: I,
    stdout_limit: usize,
    mut record_counter: Option<BoundedRecordCounter>,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = OsString>,
{
    if stdout_limit > HOST_MAX_GIT_OUTPUT_BYTES {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "{operation} output limit exceeds the Host hard maximum"
        )));
    }
    let stdout_file = tempfile::NamedTempFile::new().map_err(|source| GitWorkspaceError::Io {
        operation: "create bounded Git output file",
        path: repo.to_path_buf(),
        source,
    })?;
    let child_stdout =
        stdout_file
            .as_file()
            .try_clone()
            .map_err(|source| GitWorkspaceError::Io {
                operation: "clone bounded Git output file",
                path: repo.to_path_buf(),
                source,
            })?;
    let mut monitor = File::open(stdout_file.path()).map_err(|source| GitWorkspaceError::Io {
        operation: "open bounded Git output monitor",
        path: repo.to_path_buf(),
        source,
    })?;
    let mut command = isolated_git_command(repo, false);
    command
        .args(args)
        .stdout(Stdio::from(child_stdout))
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn().map_err(|source| GitWorkspaceError::Io {
        operation: "launch bounded Git operation",
        path: repo.to_path_buf(),
        source,
    })?;
    let mut child = BoundedGitChild::new(child);
    let deadline = Instant::now() + BOUNDED_GIT_COMMAND_TIMEOUT;
    let stdout_limit_u64 = u64::try_from(stdout_limit).unwrap_or(u64::MAX);
    let status = loop {
        if stdout_file
            .as_file()
            .metadata()
            .map_err(|source| GitWorkspaceError::Io {
                operation: "inspect bounded Git output",
                path: repo.to_path_buf(),
                source,
            })?
            .len()
            > stdout_limit_u64
        {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host output byte limit"
            )));
        }
        if let Some(counter) = record_counter.as_mut() {
            scan_bounded_records(&mut monitor, counter, deadline, repo, operation)?;
        }
        if let Some(status) = child.try_wait().map_err(|source| GitWorkspaceError::Io {
            operation: "poll bounded Git operation",
            path: repo.to_path_buf(),
            source,
        })? {
            child.terminate_process_group_and_reap();
            break status;
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host time limit"
            )));
        }
        thread::sleep(BOUNDED_GIT_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    };
    if let Some(counter) = record_counter.as_mut() {
        scan_bounded_records(&mut monitor, counter, deadline, repo, operation)?;
    }
    if !status.success() {
        return Err(GitWorkspaceError::Git {
            operation: operation.to_owned(),
            status: status.code(),
            detail: "bounded Git operation failed".to_string(),
        });
    }
    let output_len = stdout_file
        .as_file()
        .metadata()
        .map_err(|source| GitWorkspaceError::Io {
            operation: "inspect completed bounded Git output",
            path: repo.to_path_buf(),
            source,
        })?
        .len();
    if output_len > stdout_limit_u64 {
        return Err(GitWorkspaceError::LimitExceeded(format!(
            "{operation} exceeded the Host output byte limit"
        )));
    }
    let mut output = File::open(stdout_file.path()).map_err(|source| GitWorkspaceError::Io {
        operation: "open completed bounded Git output",
        path: repo.to_path_buf(),
        source,
    })?;
    output
        .seek(SeekFrom::Start(0))
        .map_err(|source| GitWorkspaceError::Io {
            operation: "rewind bounded Git output",
            path: repo.to_path_buf(),
            source,
        })?;
    let mut bytes = Vec::with_capacity(usize::try_from(output_len).unwrap_or(stdout_limit));
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host time limit"
            )));
        }
        let read = output
            .read(&mut buffer)
            .map_err(|source| GitWorkspaceError::Io {
                operation: "read bounded Git output",
                path: repo.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > stdout_limit {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host output byte limit"
            )));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(bytes)
}

struct BoundedGitChild {
    child: Child,
    armed: bool,
}

impl BoundedGitChild {
    fn new(child: Child) -> Self {
        Self { child, armed: true }
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn terminate_process_group_and_reap(&mut self) {
        if !self.armed {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.armed = false;
    }
}

impl Drop for BoundedGitChild {
    fn drop(&mut self) {
        self.terminate_process_group_and_reap();
    }
}

enum BoundedRecordSeparator {
    Nul,
    DoubleNul,
}

struct BoundedRecordCounter {
    separator: BoundedRecordSeparator,
    max_records: usize,
    records: usize,
    previous_was_nul: bool,
}

impl BoundedRecordCounter {
    fn nul(max_records: usize) -> Self {
        Self {
            separator: BoundedRecordSeparator::Nul,
            max_records,
            records: 0,
            previous_was_nul: false,
        }
    }

    fn double_nul(max_records: usize) -> Self {
        Self {
            separator: BoundedRecordSeparator::DoubleNul,
            max_records,
            records: 0,
            previous_was_nul: false,
        }
    }

    fn observe(&mut self, bytes: &[u8]) -> bool {
        for byte in bytes {
            match self.separator {
                BoundedRecordSeparator::Nul => {
                    if *byte == 0 {
                        self.records = self.records.saturating_add(1);
                    }
                }
                BoundedRecordSeparator::DoubleNul => {
                    if *byte == 0 {
                        if self.previous_was_nul {
                            self.records = self.records.saturating_add(1);
                            self.previous_was_nul = false;
                        } else {
                            self.previous_was_nul = true;
                        }
                    } else {
                        self.previous_was_nul = false;
                    }
                }
            }
            if self.records > self.max_records {
                return false;
            }
        }
        true
    }
}

fn scan_bounded_records(
    monitor: &mut File,
    counter: &mut BoundedRecordCounter,
    deadline: Instant,
    repo: &Path,
    operation: &str,
) -> Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host time limit"
            )));
        }
        let read = monitor
            .read(&mut buffer)
            .map_err(|source| GitWorkspaceError::Io {
                operation: "scan bounded Git records",
                path: repo.to_path_buf(),
                source,
            })?;
        if read == 0 {
            return Ok(());
        }
        if !counter.observe(&buffer[..read]) {
            return Err(GitWorkspaceError::LimitExceeded(format!(
                "{operation} exceeded the Host record limit {}",
                counter.max_records
            )));
        }
    }
}

fn git_probe<I>(repo: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = OsString>,
{
    git_output(repo, args, false)
}

fn git_output<I>(repo: &Path, args: I, with_identity: bool) -> Result<Output>
where
    I: IntoIterator<Item = OsString>,
{
    let mut command = isolated_git_command(repo, with_identity);
    command.args(args);
    command.output().map_err(|source| GitWorkspaceError::Io {
        operation: "launch git",
        path: repo.to_path_buf(),
        source,
    })
}

fn isolated_git_command(repo: &Path, with_identity: bool) -> Command {
    let mut command = Command::new("git");
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_QUARANTINE_PATH",
        "GIT_REPLACE_REF_BASE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_PREFIX",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
    ] {
        command.env_remove(variable);
    }
    for (variable, _) in std::env::vars_os() {
        let variable_text = variable.to_string_lossy();
        if variable_text.starts_with("GIT_CONFIG_KEY_")
            || variable_text.starts_with("GIT_CONFIG_VALUE_")
        {
            command.env_remove(variable);
        }
    }
    command
        .current_dir(repo)
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("commit.gpgSign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    if with_identity {
        command
            .env("GIT_AUTHOR_NAME", "DSE Host")
            .env("GIT_AUTHOR_EMAIL", "host@dse.local")
            .env("GIT_COMMITTER_NAME", "DSE Host")
            .env("GIT_COMMITTER_EMAIL", "host@dse.local");
    }
    command
}

fn git_failure(operation: &str, output: &Output) -> GitWorkspaceError {
    GitWorkspaceError::Git {
        operation: operation.to_string(),
        status: output.status.code(),
        detail: output_detail(output),
    }
}

fn output_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(trim_ascii(&output.stderr));
    if stderr.is_empty() {
        let stdout = String::from_utf8_lossy(trim_ascii(&output.stdout));
        if stdout.is_empty() {
            "no Git diagnostic".to_string()
        } else {
            stdout.into_owned()
        }
    } else {
        stderr.into_owned()
    }
}

fn output_path(output: &Output, label: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(output_text(output, label)?))
}

fn output_text(output: &Output, label: &str) -> Result<String> {
    ascii_string(trim_ascii(&output.stdout), label)
}

fn ascii_string(bytes: &[u8], label: &str) -> Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|_| GitWorkspaceError::OwnershipMismatch(format!("{label} was not valid UTF-8")))
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    bytes
}

fn path_from_git_bytes(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes).map(PathBuf::from).map_err(|_| {
            GitWorkspaceError::OwnershipMismatch(
                "Git emitted a non-Unicode path on this platform".to_string(),
            )
        })
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|source| GitWorkspaceError::Io {
        operation: "read current directory",
        path: PathBuf::from("."),
        source,
    })?;
    Ok(cwd.join(path))
}

fn canonical_dir(path: &Path, operation: &'static str) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|source| GitWorkspaceError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|source| GitWorkspaceError::Io {
        operation: "stat canonical directory",
        path: canonical.clone(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(GitWorkspaceError::InvalidRequest(format!(
            "{} is not a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
}

#[cfg(test)]
fn create_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| GitWorkspaceError::Io {
        operation: "create managed root",
        path: path.to_path_buf(),
        source,
    })
}

fn open_integration_lease(path: &Path) -> Result<File> {
    let opened = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path);
    match opened {
        Ok(file) => Ok(file),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|source| GitWorkspaceError::Io {
                operation: "inspect existing DSE integration lease",
                path: path.to_path_buf(),
                source,
            })?;
            if !metadata.file_type().is_file() {
                return Err(GitWorkspaceError::OwnershipMismatch(format!(
                    "integration lease {} is not a regular file",
                    path.display()
                )));
            }
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|source| GitWorkspaceError::Io {
                    operation: "open existing DSE integration lease",
                    path: path.to_path_buf(),
                    source,
                })
        }
        Err(source) => Err(GitWorkspaceError::Io {
            operation: "create DSE integration lease",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn integration_head_lock_marker(allocation: &OwnedWorktree, sealed: &SealedWorktree) -> Vec<u8> {
    format!(
        "{INTEGRATION_HEAD_LOCK_VERSION}\nowner={}\nroot_ref={}\nwriter_ref={}\nbase={}\nfinal={}\ndiff_sha256={}\n",
        allocation.owner_id,
        allocation.root_branch_ref,
        allocation.branch_ref,
        allocation.base_commit,
        sealed.final_commit,
        sealed.diff_sha256,
    )
    .into_bytes()
}

fn cleanup_head_lock_marker(facts: &OwnedWorktreeFacts, expected_branch_commit: &str) -> Vec<u8> {
    format!(
        "{CLEANUP_HEAD_LOCK_VERSION}\nowner={}\nroot_ref={}\nwriter_ref={}\nbase={}\nexpected={}\n",
        facts.owner_id,
        facts.root_branch_ref,
        facts.branch_ref,
        facts.base_commit,
        expected_branch_commit,
    )
    .into_bytes()
}

fn unique_integration_marker_temp_path(root_git_dir: &Path) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_INTEGRATION_LOCK_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = root_git_dir.join(format!(
        ".dse-head-lock-{}-{timestamp}-{sequence}.tmp",
        std::process::id()
    ));
    if symlink_metadata_optional(&path)?.is_some() {
        return Err(GitWorkspaceError::Conflict(
            "unique integration lock staging path already exists".to_string(),
        ));
    }
    Ok(path)
}

fn install_integration_head_lock(
    head_lock_path: &Path,
    marker_temp_path: &Path,
    marker: &[u8],
) -> Result<()> {
    if marker.len() as u64 > MAX_INTEGRATION_LOCK_BYTES {
        return Err(GitWorkspaceError::LimitExceeded(
            "integration lock marker exceeds fixed size limit".to_string(),
        ));
    }
    if exact_marker_file(head_lock_path, marker)? {
        remove_file(head_lock_path, "remove exact crashed integration HEAD lock")?;
    } else if symlink_metadata_optional(head_lock_path)?.is_some() {
        return Err(GitWorkspaceError::Conflict(format!(
            "foreign Git HEAD lock is present at {}",
            head_lock_path.display()
        )));
    }

    let mut marker_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(marker_temp_path)
        .map_err(|source| GitWorkspaceError::Io {
            operation: "create integration HEAD lock marker",
            path: marker_temp_path.to_path_buf(),
            source,
        })?;
    marker_file
        .write_all(marker)
        .map_err(|source| GitWorkspaceError::Io {
            operation: "write integration HEAD lock marker",
            path: marker_temp_path.to_path_buf(),
            source,
        })?;
    marker_file
        .sync_all()
        .map_err(|source| GitWorkspaceError::Io {
            operation: "sync integration HEAD lock marker",
            path: marker_temp_path.to_path_buf(),
            source,
        })?;

    let link_result = fs::hard_link(marker_temp_path, head_lock_path);
    let result = match link_result {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            if exact_marker_file(head_lock_path, marker)? {
                Ok(())
            } else {
                Err(GitWorkspaceError::Conflict(format!(
                    "foreign Git HEAD lock won the integration lock race at {}",
                    head_lock_path.display()
                )))
            }
        }
        Err(source) => Err(GitWorkspaceError::Io {
            operation: "publish integration HEAD lock marker",
            path: head_lock_path.to_path_buf(),
            source,
        }),
    };
    drop(marker_file);
    if let Err(source) = fs::remove_file(marker_temp_path)
        && source.kind() != io::ErrorKind::NotFound
    {
        return Err(GitWorkspaceError::Io {
            operation: "remove integration HEAD lock staging file",
            path: marker_temp_path.to_path_buf(),
            source,
        });
    }
    result?;
    if !exact_marker_file(head_lock_path, marker)? {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "published integration HEAD lock marker changed unexpectedly".to_string(),
        ));
    }
    Ok(())
}

fn exact_marker_file(path: &Path, marker: &[u8]) -> Result<bool> {
    let Some(metadata) = symlink_metadata_optional(path)? else {
        return Ok(false);
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_INTEGRATION_LOCK_BYTES {
        return Ok(false);
    }
    let bytes = fs::read(path).map_err(|source| GitWorkspaceError::Io {
        operation: "read integration HEAD lock marker",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(bytes == marker)
}

fn remove_exact_marker_file(path: &Path, marker: &[u8]) {
    if exact_marker_file(path, marker).unwrap_or(false) {
        let _ = fs::remove_file(path);
    }
}

fn remove_file(path: &Path, operation: &'static str) -> Result<()> {
    fs::remove_file(path).map_err(|source| GitWorkspaceError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn symlink_metadata_optional(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(GitWorkspaceError::Io {
            operation: "stat path without following symlinks",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing into String cannot fail");
    }
    encoded
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GitWorkspaceError::InvalidRequest(
            "diff SHA-256 must be exactly 64 hexadecimal characters".to_string(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::process::Command;
    use tempfile::TempDir;

    struct TestRepository {
        _temp: TempDir,
        root: PathBuf,
        managed: PathBuf,
        head: String,
    }

    impl TestRepository {
        fn new() -> Self {
            let temp = TempDir::new().expect("tempdir");
            let root = temp.path().join("repo");
            let managed = temp.path().join("managed");
            fs::create_dir_all(root.join("src")).expect("create source");
            git_ok(
                temp.path(),
                [
                    OsString::from("init"),
                    OsString::from("-b"),
                    OsString::from("main"),
                    root.as_os_str().to_owned(),
                ],
            );
            git_ok(
                &root,
                [
                    OsString::from("config"),
                    OsString::from("user.name"),
                    OsString::from("DSE Test"),
                ],
            );
            git_ok(
                &root,
                [
                    OsString::from("config"),
                    OsString::from("user.email"),
                    OsString::from("test@dse.local"),
                ],
            );
            fs::write(root.join("README.md"), "base\n").expect("write readme");
            fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")
                .expect("write source");
            git_ok(
                &root,
                [
                    OsString::from("add"),
                    OsString::from("README.md"),
                    OsString::from("src/lib.rs"),
                ],
            );
            git_ok(
                &root,
                [
                    OsString::from("commit"),
                    OsString::from("-m"),
                    OsString::from("base"),
                ],
            );
            let head = git_test_text(
                &root,
                [OsString::from("rev-parse"), OsString::from("HEAD^{commit}")],
            );
            Self {
                _temp: temp,
                root,
                managed,
                head,
            }
        }

        fn owner(&self) -> GitWorkspaceOwner {
            GitWorkspaceOwner::open(&self.root, &self.managed).expect("open owner")
        }

        fn request(&self, owner_id: &str, allowed: &[&str]) -> WriterWorkspaceRequest {
            WriterWorkspaceRequest::new(
                owner_id,
                self.head.clone(),
                allowed.iter().map(PathBuf::from).collect(),
            )
        }
    }

    #[test]
    fn plan_is_read_only_and_derives_exact_assignment() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let before_worktrees = registered_worktrees(&repo.root).expect("worktrees before plan");
        let facts = owner
            .plan_current("writer_plan", vec![PathBuf::from("src")])
            .expect("plan writer");

        assert_eq!(facts.base_commit, repo.head);
        assert_eq!(facts.root_branch_ref, "refs/heads/main");
        assert_eq!(facts.branch_ref, "refs/heads/dse/writer/writer_plan");
        assert_eq!(
            facts.worktree_path,
            repo.managed
                .canonicalize()
                .expect("canonical managed root")
                .join("writer_plan")
        );
        assert!(!facts.worktree_path.exists());
        assert!(
            resolve_ref_optional(&repo.root, &facts.branch_ref)
                .expect("inspect planned branch")
                .is_none()
        );
        assert_eq!(
            registered_worktrees(&repo.root).expect("worktrees after plan"),
            before_worktrees
        );
    }

    #[test]
    fn create_seal_integrate_and_cleanup_is_guarded_and_idempotent() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_happy", &["src"]))
            .expect("create writer");
        let replayed = owner
            .create(repo.request("writer_happy", &["src"]))
            .expect("recover completed create");
        assert_eq!(replayed, allocation);
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .expect("edit writer");

        let sealed = owner.seal(&allocation).expect("seal writer");
        let reopened = repo.owner();
        let (recovered_allocation, recovered_seal) = reopened
            .recover_sealed(allocation.durable_facts(), sealed.durable_facts())
            .expect("recover sealed writer");
        assert_eq!(recovered_allocation, allocation);
        assert_eq!(recovered_seal, sealed);
        assert_eq!(sealed.base_commit(), repo.head);
        assert_ne!(sealed.final_commit(), repo.head);
        assert_eq!(sealed.changed_files(), &[PathBuf::from("src/lib.rs")]);
        assert_eq!(sealed.diff_sha256().len(), 64);
        assert!(!sealed.diff().is_empty());
        assert_eq!(
            git_test_text(
                &repo.root,
                [OsString::from("rev-parse"), OsString::from("HEAD^{commit}"),],
            ),
            repo.head,
            "sealing must not advance root"
        );

        let integrated = owner
            .integrate(&allocation, &sealed)
            .expect("integrate writer");
        assert_eq!(integrated.disposition, IntegrationDisposition::Applied);
        assert_eq!(integrated.root_commit, sealed.final_commit());
        let repeated = owner
            .integrate(&allocation, &sealed)
            .expect("idempotent integration");
        assert_eq!(repeated.disposition, IntegrationDisposition::AlreadyApplied);
        assert_eq!(
            fs::read_to_string(repo.root.join("src/lib.rs")).expect("root source"),
            "pub fn value() -> u8 { 2 }\n"
        );

        let cleanup_facts = allocation.durable_facts();
        assert_eq!(
            owner
                .cleanup_exact(&cleanup_facts, sealed.final_commit())
                .expect("cleanup"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::Removed,
                branch: CleanupComponentDisposition::Removed,
            }
        );
        assert!(!allocation.path().exists());
        assert_eq!(
            owner
                .cleanup_exact(&cleanup_facts, sealed.final_commit())
                .expect("idempotent cleanup"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::AlreadyAbsent,
            }
        );
    }

    #[test]
    fn integration_recovers_exact_post_cas_base_checkout() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_cas_recovery", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 21 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");

        git_ok(
            &repo.root,
            [
                OsString::from("update-ref"),
                allocation.root_branch_ref.clone().into(),
                sealed.final_commit().into(),
                allocation.base_commit().into(),
            ],
        );
        let crashed_lease = owner
            .acquire_root_integration_lease()
            .expect("acquire integration lease after simulated CAS");
        let crashed_lock = owner
            .acquire_root_head_lock(crashed_lease, &allocation, &sealed)
            .expect("acquire integration HEAD lock after simulated CAS");
        let head_lock_path = crashed_lock.head_lock_path.clone();
        crashed_lock.simulate_process_crash();
        assert!(
            head_lock_path.exists(),
            "simulated process crash must leave exact durable marker"
        );
        assert!(matches!(
            GitWorkspaceOwner::bind_existing(&repo.root, &repo.managed),
            Err(GitWorkspaceError::DirtyRepository(_))
        ));
        assert_eq!(
            fs::read_to_string(repo.root.join("src/lib.rs")).expect("pre-recovery source"),
            "pub fn value() -> u8 { 1 }\n"
        );

        let recovered_owner =
            GitWorkspaceOwner::bind_existing_for_integration(&repo.root, &repo.managed)
                .expect("bind post-CAS recovery");
        let (recovered_allocation, recovered_seal) = recovered_owner
            .recover_sealed_for_integration(allocation.durable_facts(), sealed.durable_facts())
            .expect("recover sealed allocation");
        let result = recovered_owner
            .integrate(&recovered_allocation, &recovered_seal)
            .expect("materialize post-CAS recovery");
        assert_eq!(result.disposition, IntegrationDisposition::AlreadyApplied);
        assert_eq!(result.root_commit, sealed.final_commit());
        assert_eq!(
            fs::read_to_string(repo.root.join("src/lib.rs")).expect("recovered source"),
            "pub fn value() -> u8 { 21 }\n"
        );
        ensure_clean(&repo.root, "recovered root").expect("clean recovered root");
        assert!(
            !head_lock_path.exists(),
            "recovery must remove its exact crashed HEAD lock"
        );
    }

    #[test]
    fn integration_rejects_preexisting_branch_switch_without_changing_bytes() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_preexisting_switch", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 22 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        git_ok(
            &repo.root,
            [
                OsString::from("branch"),
                OsString::from("other"),
                allocation.base_commit().into(),
            ],
        );
        git_ok(
            &repo.root,
            [OsString::from("switch"), OsString::from("other")],
        );
        let root_git_dir = rev_parse_path(
            &repo.root,
            "--absolute-git-dir",
            "resolve test root Git directory",
        )
        .expect("root Git directory");
        let index_path = root_git_dir.join("index");
        let index_before = fs::read(&index_path).expect("index bytes before rejection");
        let source_before =
            fs::read(repo.root.join("src/lib.rs")).expect("source before rejection");
        let main_before = resolve_ref_optional(&repo.root, "refs/heads/main")
            .expect("main ref")
            .expect("main branch");
        let other_before = resolve_ref_optional(&repo.root, "refs/heads/other")
            .expect("other ref")
            .expect("other branch");

        let error = owner
            .integrate(&allocation, &sealed)
            .expect_err("preexisting branch switch must fail closed");
        assert!(matches!(error, GitWorkspaceError::Conflict(_)));
        assert_eq!(
            symbolic_head(&repo.root).expect("current branch"),
            "refs/heads/other"
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, "refs/heads/other")
                .expect("other ref")
                .expect("other branch"),
            other_before
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, "refs/heads/main")
                .expect("main ref")
                .expect("main branch"),
            main_before
        );
        assert_eq!(
            fs::read(&index_path).expect("index bytes after rejection"),
            index_before,
            "non-target index bytes must remain unchanged"
        );
        assert_eq!(
            fs::read(repo.root.join("src/lib.rs")).expect("source after rejection"),
            source_before,
            "non-target worktree bytes must remain unchanged"
        );
        assert!(
            !root_git_dir.join("HEAD.lock").exists(),
            "failed integration must release its exact HEAD lock"
        );
    }

    #[test]
    fn integration_head_lock_blocks_the_after_cas_branch_switch_counterexample() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_after_cas_lock", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 24 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        git_ok(
            &repo.root,
            [
                OsString::from("branch"),
                OsString::from("other"),
                allocation.base_commit().into(),
            ],
        );
        let switch_was_blocked = Cell::new(false);

        let integrated = owner
            .integrate_with_hooks(
                &allocation,
                &sealed,
                || Ok(()),
                || {
                    let switch = git_probe(
                        &repo.root,
                        [OsString::from("switch"), OsString::from("other")],
                    )?;
                    switch_was_blocked.set(!switch.status.success());
                    if switch.status.success() {
                        return Err(GitWorkspaceError::OwnershipMismatch(
                            "Git switched branches while DSE held HEAD.lock".to_string(),
                        ));
                    }
                    assert!(
                        output_detail(&switch).contains("HEAD.lock"),
                        "Git must report the held HEAD lock: {}",
                        output_detail(&switch)
                    );
                    assert_eq!(
                        symbolic_head(&repo.root)?,
                        allocation.root_branch_ref,
                        "failed external switch must leave the target branch checked out"
                    );
                    assert!(root_checkout_matches_commit(
                        &repo.root,
                        allocation.base_commit()
                    )?);
                    Ok(())
                },
            )
            .expect("integrate while external switch is blocked");
        assert!(switch_was_blocked.get());
        assert_eq!(integrated.root_commit, sealed.final_commit());
        assert_eq!(
            resolve_ref_optional(&repo.root, "refs/heads/other")
                .expect("other ref")
                .expect("other branch"),
            repo.head,
            "non-target ref must remain at base"
        );
        assert_eq!(
            symbolic_head(&repo.root).expect("current branch"),
            allocation.root_branch_ref
        );
        assert_eq!(
            fs::read_to_string(repo.root.join("src/lib.rs")).expect("integrated source"),
            "pub fn value() -> u8 { 24 }\n"
        );
    }

    #[test]
    fn integration_does_not_delete_an_active_or_foreign_head_lock() {
        let active_repo = TestRepository::new();
        let active_owner = active_repo.owner();
        let active_allocation = active_owner
            .create(active_repo.request("writer_active_lock", &["src"]))
            .expect("create active-lock writer");
        fs::write(
            active_allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 25 }\n",
        )
        .expect("edit active-lock writer");
        let active_seal = active_owner
            .seal(&active_allocation)
            .expect("seal active-lock writer");
        let active_lease = active_owner
            .acquire_root_integration_lease()
            .expect("hold active integration lease");
        let active_lock = active_owner
            .acquire_root_head_lock(active_lease, &active_allocation, &active_seal)
            .expect("hold active integration lock");
        let active_marker =
            fs::read(&active_lock.head_lock_path).expect("read active HEAD lock marker");
        let active_error = active_owner
            .integrate(&active_allocation, &active_seal)
            .expect_err("second live integration must fail closed");
        assert!(matches!(active_error, GitWorkspaceError::Conflict(_)));
        assert_eq!(
            fs::read(&active_lock.head_lock_path).expect("active marker remains"),
            active_marker,
            "contending integration must not delete a live lock"
        );
        let active_head_lock_path = active_lock.head_lock_path.clone();
        drop(active_lock);
        assert!(!active_head_lock_path.exists());

        let foreign_repo = TestRepository::new();
        let foreign_owner = foreign_repo.owner();
        let foreign_allocation = foreign_owner
            .create(foreign_repo.request("writer_foreign_lock", &["src"]))
            .expect("create foreign-lock writer");
        fs::write(
            foreign_allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 26 }\n",
        )
        .expect("edit foreign-lock writer");
        let foreign_seal = foreign_owner
            .seal(&foreign_allocation)
            .expect("seal foreign-lock writer");
        let foreign_git_dir = rev_parse_path(
            &foreign_repo.root,
            "--absolute-git-dir",
            "resolve foreign-lock test Git directory",
        )
        .expect("foreign-lock Git directory");
        let foreign_head_lock = foreign_git_dir.join("HEAD.lock");
        let foreign_bytes = b"ref: refs/heads/foreign-operation\n";
        fs::write(&foreign_head_lock, foreign_bytes).expect("write foreign HEAD lock");

        let foreign_error = foreign_owner
            .integrate(&foreign_allocation, &foreign_seal)
            .expect_err("foreign Git lock must fail closed");
        assert!(
            matches!(foreign_error, GitWorkspaceError::Conflict(_)),
            "{foreign_error:?}"
        );
        assert_eq!(
            fs::read(&foreign_head_lock).expect("foreign lock remains"),
            foreign_bytes,
            "DSE must never delete a foreign Git lock"
        );
        assert_eq!(
            resolve_ref_optional(&foreign_repo.root, "refs/heads/main")
                .expect("foreign test main ref")
                .expect("foreign test main branch"),
            foreign_repo.head
        );
        assert_eq!(
            fs::read_to_string(foreign_repo.root.join("src/lib.rs"))
                .expect("foreign test root source"),
            "pub fn value() -> u8 { 1 }\n"
        );
    }

    #[test]
    fn integration_preserves_a_tracked_edit_that_appears_after_checkout_proof() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_late_root_edit", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 27 }\n",
        )
        .expect("edit writer source");
        fs::write(
            allocation.path().join("src/generated.rs"),
            "pub const GENERATED: bool = true;\n",
        )
        .expect("add writer source");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let root_git_dir = rev_parse_path(
            &repo.root,
            "--absolute-git-dir",
            "resolve late-edit test Git directory",
        )
        .expect("late-edit Git directory");
        let index_path = root_git_dir.join("index");
        let index_before = fs::read(&index_path).expect("index before late edit");

        let error = owner
            .integrate_with_hooks(
                &allocation,
                &sealed,
                || Ok(()),
                || {
                    fs::write(
                        repo.root.join("src/lib.rs"),
                        "pub fn value() -> u8 { 200 }\n",
                    )
                    .map_err(|source| GitWorkspaceError::Io {
                        operation: "write concurrent tracked root edit",
                        path: repo.root.join("src/lib.rs"),
                        source,
                    })
                },
            )
            .expect_err("late tracked root edit must stop materialization");
        assert!(matches!(error, GitWorkspaceError::Git { .. }));
        assert_eq!(
            fs::read_to_string(repo.root.join("src/lib.rs")).expect("preserved root edit"),
            "pub fn value() -> u8 { 200 }\n",
            "read-tree must not overwrite the concurrent tracked edit"
        );
        assert!(
            !repo.root.join("src/generated.rs").exists(),
            "read-tree failure must not partially materialize another writer file"
        );
        assert_eq!(
            fs::read(&index_path).expect("index after refused materialization"),
            index_before,
            "failed read-tree must leave root index bytes unchanged"
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, "refs/heads/main")
                .expect("late-edit main ref")
                .expect("late-edit main branch"),
            sealed.final_commit(),
            "the exact target ref remains recoverable after the CAS"
        );
        assert!(
            !root_git_dir.join("HEAD.lock").exists(),
            "failed materialization must release its exact HEAD lock"
        );
    }

    #[test]
    fn cleanup_resumes_the_exact_branch_only_crash_window() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_branch_cleanup", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 23 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let facts = allocation.durable_facts();

        git_ok(
            &repo.root,
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                allocation.path().as_os_str().to_owned(),
            ],
        );
        assert!(!allocation.path().exists());
        assert_eq!(
            resolve_ref_optional(&repo.root, &allocation.branch_ref)
                .expect("writer ref")
                .as_deref(),
            Some(sealed.final_commit())
        );

        assert_eq!(
            owner
                .cleanup_exact_with_hook(&facts, sealed.final_commit(), || {
                    let writer_branch =
                        branch_ref_to_short(&facts.branch_ref).expect("short writer branch");
                    let output = Command::new("git")
                        .current_dir(&repo.root)
                        .args(["switch", &writer_branch])
                        .output()
                        .expect("attempt concurrent root switch");
                    assert!(
                        !output.status.success(),
                        "root HEAD.lock must close the checkout-before-CAS window"
                    );
                    Ok(())
                })
                .expect("resume branch deletion"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::Removed,
            }
        );
        assert_eq!(
            owner
                .cleanup_exact(&facts, sealed.final_commit())
                .expect("repeat branch deletion"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::AlreadyAbsent,
            }
        );
    }

    #[test]
    fn cleanup_tombstone_recovers_each_destructive_git_crash_window() {
        for crash_after in ["intent", "branch", "worktree"] {
            let repo = TestRepository::new();
            let owner = repo.owner();
            let allocation = owner
                .create(repo.request(&format!("writer_cleanup_{crash_after}"), &["src"]))
                .expect("create writer");
            fs::write(
                allocation.path().join("src/lib.rs"),
                format!("pub fn phase() -> &'static str {{ \"{crash_after}\" }}\n"),
            )
            .expect("edit writer");
            let sealed = owner.seal(&allocation).expect("seal writer");
            let facts = allocation.durable_facts();
            let expected = sealed.final_commit();
            let tombstone = cleanup_tombstone_ref(&facts, expected).expect("cleanup tombstone");

            create_exact_ref_cas(
                &repo.root,
                &tombstone,
                expected,
                "test persist cleanup tombstone",
            )
            .expect("persist tombstone");
            if crash_after != "intent" {
                acquire_cleanup_worktree_lock(&repo.root, &facts, expected)
                    .expect("lock writer for simulated cleanup");
                assert_eq!(
                    delete_exact_writer_ref_cas(&repo.root, &facts, expected)
                        .expect("delete writer branch"),
                    CleanupComponentDisposition::Removed
                );
                assert_eq!(
                    resolve_ref_optional(&repo.root, &tombstone)
                        .expect("resolve retained tombstone")
                        .as_deref(),
                    Some(expected),
                    "the tombstone must keep an unintegrated seal reachable"
                );
                if crash_after == "branch" {
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("gc"),
                            OsString::from("--prune=now"),
                            OsString::from("--quiet"),
                        ],
                    );
                    assert_eq!(
                        resolve_exact_commit(&repo.root, expected)
                            .expect("tombstone-protected commit after GC"),
                        expected
                    );
                }
            }
            if crash_after == "worktree" {
                git_ok(
                    &repo.root,
                    [
                        OsString::from("worktree"),
                        OsString::from("remove"),
                        OsString::from("--force"),
                        OsString::from("--force"),
                        facts.worktree_path.as_os_str().to_owned(),
                    ],
                );
            }

            let recovered = owner
                .cleanup_exact(&facts, expected)
                .expect("recover interrupted cleanup");
            assert_eq!(
                recovered,
                ExactCleanupDisposition {
                    worktree: if crash_after == "worktree" {
                        CleanupComponentDisposition::AlreadyAbsent
                    } else {
                        CleanupComponentDisposition::Removed
                    },
                    branch: if crash_after == "intent" {
                        CleanupComponentDisposition::Removed
                    } else {
                        CleanupComponentDisposition::AlreadyAbsent
                    },
                },
                "unexpected recovery disposition after {crash_after}"
            );
            assert_eq!(
                resolve_ref_optional(&repo.root, &tombstone).expect("final tombstone state"),
                None
            );
            assert_eq!(
                owner
                    .cleanup_exact(&facts, expected)
                    .expect("repeat recovered cleanup"),
                ExactCleanupDisposition {
                    worktree: CleanupComponentDisposition::AlreadyAbsent,
                    branch: CleanupComponentDisposition::AlreadyAbsent,
                }
            );
        }
    }

    #[test]
    fn cleanup_tombstone_oid_mismatch_has_zero_destructive_side_effects() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_foreign_cleanup_tombstone", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 99 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let facts = allocation.durable_facts();
        let tombstone = cleanup_tombstone_ref(&facts, sealed.final_commit())
            .expect("derived cleanup tombstone");
        git_ok(
            &repo.root,
            [
                OsString::from("update-ref"),
                tombstone.clone().into(),
                allocation.base_commit().into(),
            ],
        );
        let writer_bytes = fs::read(allocation.path().join("src/lib.rs"))
            .expect("Writer bytes before rejected cleanup");

        assert!(matches!(
            owner.cleanup_exact(&facts, sealed.final_commit()),
            Err(GitWorkspaceError::OwnershipMismatch(_))
        ));
        assert_eq!(
            fs::read(allocation.path().join("src/lib.rs"))
                .expect("Writer bytes after rejected cleanup"),
            writer_bytes
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, &facts.branch_ref)
                .expect("retained Writer branch")
                .as_deref(),
            Some(sealed.final_commit())
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, &tombstone)
                .expect("retained foreign tombstone")
                .as_deref(),
            Some(allocation.base_commit())
        );
    }

    #[test]
    fn branch_only_cleanup_blocks_a_concurrent_foreign_worktree_checkout() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_branch_add_race", &["src"]))
            .expect("create writer");
        let facts = allocation.durable_facts();
        git_ok(
            &repo.root,
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                allocation.path().as_os_str().to_owned(),
            ],
        );
        let foreign_path = repo.managed.join("foreign-writer-checkout");

        let cleaned = owner
            .cleanup_exact_with_hook(&facts, allocation.base_commit(), || {
                let output = Command::new("git")
                    .current_dir(&repo.root)
                    .args([
                        "worktree",
                        "add",
                        foreign_path.to_str().expect("UTF-8 test path"),
                        &branch_ref_to_short(&facts.branch_ref).expect("short Writer branch"),
                    ])
                    .output()
                    .expect("attempt foreign Writer checkout");
                assert!(
                    !output.status.success(),
                    "the exact locked cleanup guard must keep the Writer branch occupied"
                );
                assert!(!foreign_path.exists());
                Ok(())
            })
            .expect("cleanup while foreign checkout loses the race");
        assert_eq!(
            cleaned,
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::Removed,
            }
        );
        assert_eq!(
            registered_worktrees(&repo.root)
                .expect("final worktrees")
                .len(),
            1
        );
    }

    #[test]
    fn observed_late_foreign_checkout_restores_the_writer_branch() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_late_foreign_checkout", &["src"]))
            .expect("create Writer");
        let facts = allocation.durable_facts();
        let expected = allocation.base_commit();
        let tombstone = cleanup_tombstone_ref(&facts, expected).expect("cleanup tombstone");
        let foreign_path = repo.managed.join("late-foreign-checkout");
        git_ok(
            &repo.root,
            [
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("--detach"),
                OsString::from("--no-checkout"),
                foreign_path.as_os_str().to_owned(),
                expected.into(),
            ],
        );
        let foreign_admin = rev_parse_path(
            &foreign_path,
            "--absolute-git-dir",
            "resolve foreign test admin dir",
        )
        .expect("foreign admin dir");
        create_exact_ref_cas(
            &repo.root,
            &tombstone,
            expected,
            "test persist cleanup tombstone",
        )
        .expect("persist tombstone");
        acquire_cleanup_worktree_lock(&repo.root, &facts, expected).expect("lock exact Writer");
        delete_exact_writer_ref_cas(&repo.root, &facts, expected).expect("delete Writer branch");
        fs::write(
            foreign_admin.join("HEAD"),
            format!("ref: {}\n", facts.branch_ref),
        )
        .expect("simulate a delayed foreign registration publish");

        assert!(matches!(
            owner.cleanup_exact(&facts, expected),
            Err(GitWorkspaceError::Conflict(_))
        ));
        assert_eq!(
            resolve_ref_optional(&repo.root, &facts.branch_ref)
                .expect("restored Writer branch")
                .as_deref(),
            Some(expected)
        );
        assert!(allocation.path().exists(), "owned Writer must be retained");
        assert_eq!(
            resolve_ref_optional(&repo.root, &tombstone)
                .expect("retained cleanup tombstone")
                .as_deref(),
            Some(expected)
        );
    }

    #[test]
    fn cleanup_preserves_unrelated_user_worktrees() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_with_user_worktree", &["src"]))
            .expect("create writer");
        let facts = allocation.durable_facts();
        let user_path = repo.managed.join("user-worktree");
        git_ok(
            &repo.root,
            [
                OsString::from("worktree"),
                OsString::from("add"),
                OsString::from("-b"),
                OsString::from("user-worktree"),
                user_path.as_os_str().to_owned(),
                repo.head.clone().into(),
            ],
        );
        let user_head = resolve_head(&user_path).expect("user worktree HEAD before cleanup");

        assert_eq!(
            owner
                .cleanup_exact(&facts, allocation.base_commit())
                .expect("cleanup with unrelated user worktree"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::Removed,
                branch: CleanupComponentDisposition::Removed,
            }
        );
        assert!(user_path.exists());
        assert_eq!(
            resolve_head(&user_path).expect("user worktree HEAD after cleanup"),
            user_head
        );
        assert_eq!(
            symbolic_head(&user_path).expect("user worktree branch after cleanup"),
            "refs/heads/user-worktree"
        );
        assert!(
            registered_worktrees(&repo.root)
                .expect("final worktree registry")
                .iter()
                .any(|record| same_path(&record.path, &user_path))
        );
    }

    #[test]
    fn cleanup_head_lock_release_failure_is_not_reported_as_success() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_cleanup_lock_release", &["src"]))
            .expect("create writer");
        let facts = allocation.durable_facts();
        let lease = owner
            .acquire_root_integration_lease()
            .expect("acquire cleanup lease");
        let lock = owner
            .acquire_root_cleanup_lock(lease, &facts, allocation.base_commit())
            .expect("acquire cleanup HEAD lock");
        let lock_path = lock.head_lock_path.clone();
        let marker = lock.marker.clone();
        let error = lock
            .release_with(|path| {
                Err(GitWorkspaceError::Io {
                    operation: "inject cleanup HEAD lock unlink failure",
                    path: path.to_path_buf(),
                    source: io::Error::other("injected unlink failure"),
                })
            })
            .expect_err("failed unlink must not report cleanup lock release");
        assert!(matches!(error, GitWorkspaceError::Io { .. }));
        assert!(
            exact_marker_file(&lock_path, &marker).expect("retained exact cleanup marker"),
            "an unreported release must remain exactly recoverable"
        );

        let retry_lease = owner
            .acquire_root_integration_lease()
            .expect("reacquire cleanup lease");
        owner
            .acquire_root_cleanup_lock(retry_lease, &facts, allocation.base_commit())
            .expect("reconcile exact failed release marker")
            .release()
            .expect("release recovered cleanup HEAD lock");
        assert!(!lock_path.exists());
    }

    #[test]
    fn cleanup_release_failure_keeps_the_tombstone_until_exact_recovery() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_cleanup_release_recovery", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 71 }\n",
        )
        .expect("edit Writer");
        let sealed = owner.seal(&allocation).expect("seal Writer");
        let facts = allocation.durable_facts();
        let expected = sealed.final_commit();
        let tombstone = cleanup_tombstone_ref(&facts, expected).expect("cleanup tombstone");
        let retained_lock = RefCell::new(None::<(PathBuf, Vec<u8>)>);
        owner
            .cleanup_exact_with_hooks(
                &facts,
                expected,
                || Ok(()),
                |root_lock| {
                    retained_lock.replace(Some((
                        root_lock.head_lock_path.clone(),
                        root_lock.marker.clone(),
                    )));
                    root_lock.release_with(|path| {
                        Err(GitWorkspaceError::Io {
                            operation: "inject post-cleanup HEAD lock unlink failure",
                            path: path.to_path_buf(),
                            source: io::Error::other("injected unlink failure"),
                        })
                    })
                },
            )
            .expect_err("production cleanup must report a failed root lock release");
        let (root_lock_path, root_lock_marker) = retained_lock
            .into_inner()
            .expect("production cleanup reached explicit root lock release");
        assert!(
            exact_marker_file(&root_lock_path, &root_lock_marker)
                .expect("retained cleanup HEAD lock")
        );
        assert!(!facts.worktree_path.exists());
        assert_eq!(
            resolve_ref_optional(&repo.root, &facts.branch_ref).expect("deleted Writer branch"),
            None
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, &tombstone)
                .expect("retained cleanup tombstone")
                .as_deref(),
            Some(expected),
            "the tombstone must outlive a failed root lock release"
        );
        assert_eq!(
            resolve_exact_commit(&repo.root, expected).expect("tombstone-protected seal"),
            expected
        );

        assert_eq!(
            owner
                .cleanup_exact(&facts, expected)
                .expect("recover cleanup after the failed root lock release"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::AlreadyAbsent,
            }
        );
        assert!(!root_lock_path.exists());
        assert_eq!(
            resolve_ref_optional(&repo.root, &tombstone).expect("final tombstone state"),
            None
        );
        git_ok(
            &repo.root,
            [
                OsString::from("reflog"),
                OsString::from("expire"),
                OsString::from("--expire=now"),
                OsString::from("--all"),
            ],
        );
        git_ok(
            &repo.root,
            [
                OsString::from("gc"),
                OsString::from("--prune=now"),
                OsString::from("--quiet"),
            ],
        );
        assert!(
            resolve_exact_commit(&repo.root, expected).is_err(),
            "the fixture must prove idempotence after the unreachable seal is collected"
        );
        assert_eq!(
            owner
                .cleanup_exact(&facts, expected)
                .expect("repeat cleanup without the collected seal object"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::AlreadyAbsent,
            }
        );
    }

    #[test]
    fn branch_only_cleanup_retains_a_writer_ref_checked_out_by_root() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_branch_checked_out", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 31 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let facts = allocation.durable_facts();
        git_ok(
            &repo.root,
            [
                OsString::from("worktree"),
                OsString::from("remove"),
                allocation.path().as_os_str().to_owned(),
            ],
        );
        let writer_branch = branch_ref_to_short(&allocation.branch_ref).expect("short branch");
        git_ok(
            &repo.root,
            [OsString::from("switch"), writer_branch.clone().into()],
        );

        assert!(matches!(
            owner.cleanup_exact(&facts, sealed.final_commit()),
            Err(GitWorkspaceError::Conflict(_))
        ));
        assert_eq!(
            symbolic_head(&repo.root).expect("root branch"),
            allocation.branch_ref
        );
        assert_eq!(
            resolve_head(&repo.root).expect("root HEAD"),
            sealed.final_commit()
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, &allocation.branch_ref)
                .expect("writer ref")
                .as_deref(),
            Some(sealed.final_commit())
        );
    }

    #[test]
    fn live_cleanup_has_zero_side_effect_when_writer_branch_is_checked_out_elsewhere() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_live_foreign_checkout", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 37 }\n",
        )
        .expect("edit writer");
        let facts = allocation.durable_facts();
        let writer_branch = branch_ref_to_short(&allocation.branch_ref).expect("short branch");
        git_ok(
            &repo.root,
            [
                OsString::from("switch"),
                OsString::from("--ignore-other-worktrees"),
                writer_branch.into(),
            ],
        );
        let writer_bytes_before =
            fs::read(allocation.path().join("src/lib.rs")).expect("writer bytes before cleanup");

        assert!(matches!(
            owner.cleanup_exact(&facts, allocation.base_commit()),
            Err(GitWorkspaceError::Conflict(_))
        ));
        assert!(allocation.path().exists(), "Writer path must be retained");
        assert_eq!(
            fs::read(allocation.path().join("src/lib.rs")).expect("writer bytes after cleanup"),
            writer_bytes_before
        );
        assert_eq!(
            symbolic_head(&repo.root).expect("root branch"),
            allocation.branch_ref
        );
        assert_eq!(
            resolve_ref_optional(&repo.root, &allocation.branch_ref)
                .expect("writer ref")
                .as_deref(),
            Some(allocation.base_commit())
        );
    }

    #[test]
    fn cleanup_resumes_an_exact_missing_path_registry_window() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_missing_path_cleanup", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 29 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let facts = allocation.durable_facts();
        let frozen_scope = owner
            .inspect_cleanup_paths(&facts, Some(&sealed.durable_facts()))
            .expect("freeze sealed cleanup scope before path loss");
        assert_eq!(frozen_scope.paths, vec![PathBuf::from("src/lib.rs")]);

        fs::remove_dir_all(allocation.path()).expect("simulate checkout deletion crash");
        assert!(!allocation.path().exists());
        assert!(
            exact_registered_worktree(&repo.root, &facts)
                .expect("registered residue")
                .is_some()
        );
        assert!(
            owned_admin_entry(&owner.common_git_dir, &facts)
                .expect("admin residue")
                .is_some()
        );
        assert!(matches!(
            owner.inspect_cleanup_paths(&facts, Some(&sealed.durable_facts())),
            Err(GitWorkspaceError::Conflict(_))
        ));

        assert_eq!(
            owner
                .cleanup_exact(&facts, sealed.final_commit())
                .expect("finish exact missing-path cleanup"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::Removed,
                branch: CleanupComponentDisposition::Removed,
            }
        );
        assert_eq!(
            owner
                .cleanup_exact(&facts, sealed.final_commit())
                .expect("repeat exact missing-path cleanup"),
            ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::AlreadyAbsent,
                branch: CleanupComponentDisposition::AlreadyAbsent,
            }
        );
    }

    #[test]
    fn cleanup_retains_missing_path_residue_with_a_changed_admin_backpointer() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_missing_path_guard", &["src"]))
            .expect("create writer");
        let facts = allocation.durable_facts();
        let admin_dir = owned_admin_entry(&owner.common_git_dir, &facts)
            .expect("inspect admin entry")
            .expect("exact admin entry");

        fs::remove_dir_all(allocation.path()).expect("simulate checkout deletion crash");
        fs::write(
            admin_dir.join("gitdir"),
            repo.root.join(".git").display().to_string(),
        )
        .expect("replace admin backpointer");

        let cleanup = owner.cleanup_exact(&facts, allocation.base_commit());
        assert!(
            matches!(
                cleanup,
                Err(GitWorkspaceError::Conflict(_) | GitWorkspaceError::OwnershipMismatch(_))
            ),
            "unexpected cleanup result: {cleanup:?}"
        );
        assert!(admin_dir.exists(), "uncertain admin entry must be retained");
        assert_eq!(
            resolve_ref_optional(&repo.root, &facts.branch_ref)
                .expect("inspect retained branch")
                .as_deref(),
            Some(allocation.base_commit())
        );
        let tombstone = cleanup_tombstone_ref(&facts, allocation.base_commit())
            .expect("derived cleanup tombstone");
        assert_eq!(
            resolve_ref_optional(&repo.root, &tombstone).expect("inspect cleanup tombstone"),
            None,
            "identity rejection must not persist cleanup intent"
        );
    }

    #[test]
    fn git_commands_clear_redirects_and_disable_optional_locks() {
        let command = isolated_git_command(Path::new("."), false);
        let environment = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            environment.get("GIT_OPTIONAL_LOCKS"),
            Some(&Some("0".to_string()))
        );
        assert_eq!(
            environment.get("GIT_NO_REPLACE_OBJECTS"),
            Some(&Some("1".to_string()))
        );
        for redirect in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_NAMESPACE",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            assert_eq!(
                environment.get(redirect),
                Some(&None),
                "{redirect} must be removed from child Git"
            );
        }
    }

    #[test]
    fn open_and_create_reject_dirty_unborn_sparse_and_symbolic_base() {
        let temp = TempDir::new().expect("tempdir");
        let unborn = temp.path().join("unborn");
        fs::create_dir(&unborn).expect("unborn dir");
        git_ok(
            &unborn,
            [
                OsString::from("init"),
                OsString::from("-b"),
                OsString::from("main"),
            ],
        );
        assert!(matches!(
            GitWorkspaceOwner::open(&unborn, temp.path().join("unborn-managed")),
            Err(GitWorkspaceError::InvalidRepository(_))
        ));

        let repo = TestRepository::new();
        fs::write(repo.root.join("dirty.txt"), "dirty").expect("dirty file");
        assert!(matches!(
            GitWorkspaceOwner::open(&repo.root, &repo.managed),
            Err(GitWorkspaceError::DirtyRepository(_))
        ));
        fs::remove_file(repo.root.join("dirty.txt")).expect("remove dirty file");

        let inside_managed = repo.root.join(".managed-worktrees");
        fs::create_dir(&inside_managed).expect("inside managed directory");
        assert!(matches!(
            GitWorkspaceOwner::bind_existing(&repo.root, &inside_managed),
            Err(GitWorkspaceError::InvalidRequest(message))
                if message.contains("must be outside")
        ));
        fs::remove_dir(&inside_managed).expect("remove inside managed directory");

        let owner = repo.owner();
        let symbolic =
            WriterWorkspaceRequest::new("writer_symbolic", "HEAD", vec![PathBuf::from("src")]);
        assert!(matches!(
            owner.create(symbolic),
            Err(GitWorkspaceError::InvalidRequest(_))
        ));

        git_ok(
            &repo.root,
            [
                OsString::from("config"),
                OsString::from("core.sparseCheckout"),
                OsString::from("true"),
            ],
        );
        assert!(matches!(
            GitWorkspaceOwner::open(&repo.root, &repo.managed),
            Err(GitWorkspaceError::UnsupportedRepository(_))
        ));
    }

    #[test]
    fn seal_rejects_empty_out_of_scope_and_special_changes() {
        let empty_repo = TestRepository::new();
        let empty_owner = empty_repo.owner();
        let empty = empty_owner
            .create(empty_repo.request("writer_empty", &["src"]))
            .expect("create empty writer");
        assert!(matches!(
            empty_owner.seal(&empty),
            Err(GitWorkspaceError::EmptyDiff)
        ));

        let scope_repo = TestRepository::new();
        let scope_owner = scope_repo.owner();
        let outside = scope_owner
            .create(scope_repo.request("writer_scope", &["src"]))
            .expect("create scoped writer");
        fs::write(outside.path().join("README.md"), "outside scope\n").expect("outside edit");
        assert!(matches!(
            scope_owner.seal(&outside),
            Err(GitWorkspaceError::Conflict(message))
                if message.contains("outside allowed scope")
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let special_repo = TestRepository::new();
            let special_owner = special_repo.owner();
            let special = special_owner
                .create(special_repo.request("writer_special", &["src"]))
                .expect("create special writer");
            symlink("../README.md", special.path().join("src/link")).expect("create symlink");
            assert!(matches!(
                special_owner.seal(&special),
                Err(GitWorkspaceError::Conflict(message))
                    if message.contains("special filesystem entry")
            ));
        }
    }

    #[test]
    fn integration_fails_closed_when_root_advances() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_conflict", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 3 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");

        fs::write(repo.root.join("README.md"), "root advanced\n").expect("advance root");
        git_ok(
            &repo.root,
            [OsString::from("add"), OsString::from("README.md")],
        );
        git_ok(
            &repo.root,
            [
                OsString::from("commit"),
                OsString::from("-m"),
                OsString::from("advance root"),
            ],
        );
        assert!(matches!(
            owner.integrate(&allocation, &sealed),
            Err(GitWorkspaceError::Conflict(_))
        ));
    }

    #[test]
    fn cleanup_discards_exact_dirty_writer_but_rejects_ref_mismatch() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_cleanup_guard", &["src"]))
            .expect("create writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 4 }\n",
        )
        .expect("dirty writer");
        assert!(matches!(
            owner.cleanup_exact(&allocation.durable_facts(), allocation.base_commit()),
            Ok(ExactCleanupDisposition {
                worktree: CleanupComponentDisposition::Removed,
                branch: CleanupComponentDisposition::Removed,
            })
        ));
        assert!(!allocation.path().exists());

        let allocation = owner
            .create(repo.request("writer_cleanup_ref_guard", &["src"]))
            .expect("create second writer");
        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 5 }\n",
        )
        .expect("edit second writer");
        git_ok(
            allocation.path(),
            [OsString::from("add"), OsString::from("src/lib.rs")],
        );
        git_ok(
            allocation.path(),
            [
                OsString::from("commit"),
                OsString::from("-m"),
                OsString::from("untrusted writer commit"),
            ],
        );
        assert!(matches!(
            owner.cleanup_exact(&allocation.durable_facts(), allocation.base_commit()),
            Err(GitWorkspaceError::OwnershipMismatch(_))
        ));
        assert!(allocation.path().exists());
    }

    #[test]
    fn cleanup_is_independent_of_root_dirty_advanced_and_detached_states() {
        for root_state in ["dirty", "advanced", "detached", "switched"] {
            let repo = TestRepository::new();
            let allocation = repo
                .owner()
                .create(repo.request(&format!("writer_root_{root_state}"), &["src"]))
                .expect("create writer");
            fs::write(
                allocation.path().join("src/lib.rs"),
                format!("pub fn value() -> &'static str {{ \"{root_state}\" }}\n"),
            )
            .expect("edit writer");

            match root_state {
                "dirty" => {
                    fs::write(repo.root.join("README.md"), "dirty root bytes\n")
                        .expect("dirty root");
                }
                "advanced" => {
                    fs::write(repo.root.join("README.md"), "advanced root bytes\n")
                        .expect("advance root bytes");
                    git_ok(
                        &repo.root,
                        [OsString::from("add"), OsString::from("README.md")],
                    );
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("commit"),
                            OsString::from("-m"),
                            OsString::from("advance root before cleanup"),
                        ],
                    );
                }
                "detached" => {
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("checkout"),
                            OsString::from("--detach"),
                            repo.head.clone().into(),
                        ],
                    );
                }
                "switched" => {
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("switch"),
                            OsString::from("-c"),
                            OsString::from("cleanup-other"),
                        ],
                    );
                }
                _ => unreachable!(),
            }
            let head_before = git_test_text(
                &repo.root,
                [OsString::from("rev-parse"), OsString::from("HEAD^{commit}")],
            );
            let status_before = repository_status(&repo.root).expect("root status before cleanup");
            let bytes_before = fs::read(repo.root.join("README.md")).expect("root bytes before");
            let symbolic_head_before = symbolic_head(&repo.root).ok();

            let cleanup_owner =
                GitWorkspaceOwner::bind_existing_for_cleanup(&repo.root, &repo.managed)
                    .expect("bind cleanup owner independent of root checkout state");
            assert_eq!(
                cleanup_owner
                    .cleanup_exact(&allocation.durable_facts(), allocation.base_commit())
                    .expect("cleanup exact Writer"),
                ExactCleanupDisposition {
                    worktree: CleanupComponentDisposition::Removed,
                    branch: CleanupComponentDisposition::Removed,
                }
            );
            assert_eq!(
                git_test_text(
                    &repo.root,
                    [OsString::from("rev-parse"), OsString::from("HEAD^{commit}")],
                ),
                head_before
            );
            assert_eq!(
                repository_status(&repo.root).expect("root status after cleanup"),
                status_before
            );
            assert_eq!(
                fs::read(repo.root.join("README.md")).expect("root bytes after"),
                bytes_before
            );
            assert_eq!(
                symbolic_head(&repo.root).ok(),
                symbolic_head_before,
                "cleanup must not change the root symbolic HEAD"
            );
        }
    }

    #[test]
    fn prepared_seal_cleanup_recovery_is_independent_of_all_root_checkout_states() {
        for root_state in ["dirty", "advanced", "detached", "switched"] {
            let repo = TestRepository::new();
            let allocation = repo
                .owner()
                .create(repo.request(&format!("sealed_cleanup_{root_state}"), &["src"]))
                .expect("create writer");
            fs::write(
                allocation.path().join("src/lib.rs"),
                format!("pub fn value() -> &'static str {{ \"{root_state}\" }}\n"),
            )
            .expect("edit writer");
            let sealed = repo.owner().seal(&allocation).expect("seal writer");
            let facts = allocation.durable_facts();

            match root_state {
                "dirty" => {
                    fs::write(repo.root.join("README.md"), "dirty sealed root bytes\n")
                        .expect("dirty root");
                }
                "advanced" => {
                    fs::write(repo.root.join("README.md"), "advanced sealed root bytes\n")
                        .expect("advance root bytes");
                    git_ok(
                        &repo.root,
                        [OsString::from("add"), OsString::from("README.md")],
                    );
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("commit"),
                            OsString::from("-m"),
                            OsString::from("advance root after seal"),
                        ],
                    );
                }
                "detached" => {
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("checkout"),
                            OsString::from("--detach"),
                            repo.head.clone().into(),
                        ],
                    );
                }
                "switched" => {
                    git_ok(
                        &repo.root,
                        [
                            OsString::from("switch"),
                            OsString::from("-c"),
                            OsString::from("sealed-cleanup-other"),
                        ],
                    );
                }
                _ => unreachable!(),
            }
            let head_before = git_test_text(
                &repo.root,
                [OsString::from("rev-parse"), OsString::from("HEAD^{commit}")],
            );
            let status_before = repository_status(&repo.root).expect("root status before cleanup");
            let bytes_before = fs::read(repo.root.join("README.md")).expect("root bytes before");
            let symbolic_head_before = symbolic_head(&repo.root).ok();

            let cleanup_owner =
                GitWorkspaceOwner::bind_existing_for_cleanup(&repo.root, &repo.managed)
                    .expect("bind cleanup owner");
            let (_, recovered) = cleanup_owner
                .recover_prepared_seal_for_cleanup(facts.clone())
                .expect("recover exact Host seal independent of root checkout");
            assert_eq!(recovered.final_commit(), sealed.final_commit());
            assert_eq!(recovered.diff_sha256(), sealed.diff_sha256());
            assert_eq!(
                cleanup_owner
                    .cleanup_exact(&facts, sealed.final_commit())
                    .expect("cleanup recovered sealed Writer"),
                ExactCleanupDisposition {
                    worktree: CleanupComponentDisposition::Removed,
                    branch: CleanupComponentDisposition::Removed,
                }
            );
            assert_eq!(
                git_test_text(
                    &repo.root,
                    [OsString::from("rev-parse"), OsString::from("HEAD^{commit}")],
                ),
                head_before
            );
            assert_eq!(
                repository_status(&repo.root).expect("root status after cleanup"),
                status_before
            );
            assert_eq!(
                fs::read(repo.root.join("README.md")).expect("root bytes after"),
                bytes_before
            );
            assert_eq!(symbolic_head(&repo.root).ok(), symbolic_head_before);
        }
    }

    #[test]
    fn cleanup_scope_scan_stops_before_reading_an_oversized_ignored_file() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let mut request = repo.request("writer_cleanup_limit", &["src"]);
        request.limits.max_file_bytes = 16;
        request.limits.max_total_file_bytes = 32;
        let allocation = owner.create(request).expect("create bounded writer");
        fs::write(allocation.path().join(".gitignore"), "ignored.bin\n")
            .expect("write ignore rule");
        fs::write(allocation.path().join("ignored.bin"), vec![b'x'; 17])
            .expect("write oversized ignored file");

        assert!(matches!(
            owner.inspect_cleanup_paths(&allocation.durable_facts(), None),
            Err(GitWorkspaceError::LimitExceeded(message))
                if message.contains("per-file limit")
        ));
        assert!(allocation.path().exists(), "inspection must be read-only");
        assert_eq!(
            resolve_ref_optional(&repo.root, &allocation.branch_ref)
                .expect("inspect writer branch")
                .as_deref(),
            Some(allocation.base_commit())
        );
    }

    #[test]
    fn cleanup_path_enumeration_stops_at_the_first_excess_entry() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let mut request = repo.request("writer_cleanup_path_limit", &["src"]);
        request.limits.max_changed_files = 1;
        let allocation = owner.create(request).expect("create bounded writer");
        fs::write(allocation.path().join("src/a.tmp"), b"a").expect("first path");
        fs::write(allocation.path().join("src/b.tmp"), b"b").expect("second path");

        assert!(matches!(
            owner.inspect_cleanup_paths(&allocation.durable_facts(), None),
            Err(GitWorkspaceError::LimitExceeded(message))
                if message.contains("Host record limit 1")
        ));
        assert!(allocation.path().exists(), "bounded scan must be read-only");
        assert_eq!(
            resolve_ref_optional(&repo.root, &allocation.branch_ref)
                .expect("inspect retained branch")
                .as_deref(),
            Some(allocation.base_commit())
        );
    }

    #[test]
    fn sealed_path_enumeration_enforces_the_same_entry_limit() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_sealed_path_limit", &["src"]))
            .expect("create writer");
        fs::write(allocation.path().join("src/a.rs"), b"pub fn a() {}\n")
            .expect("first committed path");
        fs::write(allocation.path().join("src/b.rs"), b"pub fn b() {}\n")
            .expect("second committed path");
        let sealed = owner.seal(&allocation).expect("seal two paths");
        let limits = SealLimits {
            max_changed_files: 1,
            ..SealLimits::default()
        };

        assert!(matches!(
            committed_changed_paths(
                &repo.root,
                allocation.base_commit(),
                sealed.final_commit(),
                limits,
            ),
            Err(GitWorkspaceError::LimitExceeded(message))
                if message.contains("Host record limit 1")
        ));
    }

    #[test]
    fn seal_limits_above_host_maxima_are_rejected_before_git_execution() {
        let repo = TestRepository::new();
        let mut request = repo.request("writer_unbounded_limit", &["src"]);
        request.limits.max_diff_bytes = HOST_MAX_DIFF_BYTES + 1;

        assert!(matches!(
            repo.owner().create(request),
            Err(GitWorkspaceError::InvalidRequest(message))
                if message.contains("Host hard maximum")
        ));
        assert!(!repo.managed.join("writer_unbounded_limit").exists());
    }

    #[cfg(unix)]
    #[test]
    fn bounded_git_record_limit_terminates_the_producer_process_group() {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = TestRepository::new();
        let script = repo.root.join("record-producer.sh");
        let pid_file = repo.root.join("record-producer.pid");
        fs::write(
            &script,
            "#!/bin/sh\necho $$ > \"$1\"\nwhile :; do printf 'x\\000'; sleep 0.02; done\n",
        )
        .expect("write record producer");
        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("make script executable");
        let alias = format!(
            "alias.record-producer=!{} {}",
            script.display(),
            pid_file.display()
        );

        let error = git_checked_bounded_nul_records(
            &repo.root,
            "exercise record limit",
            [
                OsString::from("-c"),
                alias.into(),
                OsString::from("record-producer"),
            ],
            1024,
            1,
        )
        .expect_err("the second NUL record must stop the producer");
        assert!(matches!(
            error,
            GitWorkspaceError::LimitExceeded(message)
                if message.contains("Host record limit 1")
        ));
        assert_process_is_reaped(&pid_file);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_git_success_terminates_a_descendant_that_keeps_stdout_open() {
        use std::os::unix::fs::PermissionsExt as _;

        let repo = TestRepository::new();
        let script = repo.root.join("descendant-producer.sh");
        let pid_file = repo.root.join("descendant-producer.pid");
        fs::write(
            &script,
            "#!/bin/sh\n(sleep 30; printf 'late') &\necho $! > \"$1\"\nexit 0\n",
        )
        .expect("write descendant producer");
        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("make script executable");
        let alias = format!(
            "alias.descendant=!{} {}",
            script.display(),
            pid_file.display()
        );
        let started = Instant::now();

        let output = git_checked_bounded(
            &repo.root,
            "exercise descendant cleanup",
            [
                OsString::from("-c"),
                alias.into(),
                OsString::from("descendant"),
            ],
            1024,
        )
        .expect("Git leader succeeds while Host owns the whole process group");
        assert!(output.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "Host must not wait for the detached descendant"
        );
        assert_process_is_reaped(&pid_file);
    }

    #[test]
    fn recovery_rejects_forged_durable_facts() {
        let repo = TestRepository::new();
        let owner = repo.owner();
        let allocation = owner
            .create(repo.request("writer_recovery_guard", &["src"]))
            .expect("create writer");

        let mut forged_owned = allocation.durable_facts();
        forged_owned.worktree_path = repo.managed.join("another-owner");
        assert!(matches!(
            owner.recover_owned(forged_owned),
            Err(GitWorkspaceError::OwnershipMismatch(_))
        ));

        fs::write(
            allocation.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 5 }\n",
        )
        .expect("edit writer");
        let sealed = owner.seal(&allocation).expect("seal writer");
        let mut forged_seal = sealed.durable_facts();
        forged_seal.diff_sha256 = "0".repeat(64);
        assert!(matches!(
            owner.recover_sealed(allocation.durable_facts(), forged_seal),
            Err(GitWorkspaceError::OwnershipMismatch(_))
        ));
    }

    fn git_ok<I>(cwd: &Path, args: I)
    where
        I: IntoIterator<Item = OsString>,
    {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("launch test git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_test_text<I>(cwd: &Path, args: I) -> String
    where
        I: IntoIterator<Item = OsString>,
    {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("launch test git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf8 git output")
            .trim()
            .to_string()
    }

    #[cfg(unix)]
    fn assert_process_is_reaped(pid_file: &Path) {
        let pid = fs::read_to_string(pid_file)
            .expect("producer pid file")
            .trim()
            .parse::<u32>()
            .expect("producer pid");
        for _ in 0..100 {
            if !test_process_is_alive(pid) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !test_process_is_alive(pid),
            "producer process {pid} survived"
        );
    }

    #[cfg(unix)]
    fn test_process_is_alive(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(any(unix, windows)))]
fn null_device() -> &'static str {
    "/dev/null"
}
