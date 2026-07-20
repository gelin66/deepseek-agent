use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt as _;
use sha2::{Digest, Sha256};
use thiserror::Error;

const WRITER_BRANCH_PREFIX: &str = "refs/heads/codewhale/writer/";
const MAX_GIT_POINTER_BYTES: u64 = 16 * 1024;
const INTEGRATION_LEASE_FILE: &str = "codewhale-integration.lease";
const INTEGRATION_HEAD_LOCK_VERSION: &str = "codewhale-integration-head-lock-v1";
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
            max_changed_files: 512,
            max_file_bytes: 8 * 1024 * 1024,
            max_total_file_bytes: 32 * 1024 * 1024,
            max_diff_bytes: 16 * 1024 * 1024,
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

    pub fn branch_ref(&self) -> &str {
        &self.branch_ref
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

    #[cfg(test)]
    fn durable_facts(&self) -> SealedWorktreeFacts {
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
pub enum CleanupDisposition {
    Removed,
    AlreadyAbsent,
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
        )?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(&allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
        )?;
        let diff = binary_diff_commits(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
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
        let root = self.validate_bound_repository(true)?;
        let allocation = self.rebuild_allocation(owned)?;
        if root.branch_ref != allocation.root_branch_ref || root.head != allocation.base_commit {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD no longer matches the prepared seal".to_string(),
            ));
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
        )?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(&allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
        )?;
        let diff = binary_diff_commits(
            &self.repository_root,
            allocation.base_commit(),
            &final_commit,
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

    /// Prove that every externally visible resource for these exact durable
    /// facts is absent. This makes cleanup retryable after a crash between the
    /// Git removal and the durable cleanup event.
    #[cfg(test)]
    pub(crate) fn exact_resources_absent(&self, facts: &OwnedWorktreeFacts) -> Result<bool> {
        self.validate_owned_facts_identity(facts)?;

        let path_absent = symlink_metadata_optional(&facts.worktree_path)?.is_none();
        let branch_absent =
            resolve_ref_optional(&self.repository_root, &facts.branch_ref)?.is_none();
        let registration_absent = !registered_worktrees(&self.repository_root)?
            .iter()
            .any(|record| same_path(&record.path, &facts.worktree_path));
        Ok(path_absent && branch_absent && registration_absent)
    }

    /// Finish the exact crash window after `git worktree remove` succeeded but
    /// before the writer ref deletion was durably observed.
    ///
    /// `None` means a worktree path or registration is still present and the
    /// caller must use normal allocation recovery. A returned disposition
    /// means only the exact owner-derived branch was absent or CAS-deleted.
    pub fn cleanup_branch_only(
        &self,
        facts: &OwnedWorktreeFacts,
        expected_branch_commit: &str,
    ) -> Result<Option<CleanupDisposition>> {
        self.validate_owned_facts_identity(facts)?;
        let expected = resolve_exact_commit(&self.repository_root, expected_branch_commit)?;
        let root = self.validate_bound_repository(true)?;
        if root.branch_ref != facts.root_branch_ref
            || (root.head != facts.base_commit && root.head != expected)
        {
            return Err(GitWorkspaceError::Conflict(
                "root branch or HEAD is incompatible with branch-only cleanup".to_string(),
            ));
        }

        let path_present = symlink_metadata_optional(&facts.worktree_path)?.is_some();
        let registered = registered_worktrees(&self.repository_root)?
            .iter()
            .any(|record| same_path(&record.path, &facts.worktree_path));
        if path_present || registered {
            return Ok(None);
        }

        let Some(actual) = resolve_ref_optional(&self.repository_root, &facts.branch_ref)? else {
            return Ok(Some(CleanupDisposition::AlreadyAbsent));
        };
        if actual != expected {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "writer branch {} points to {actual}, expected {expected}",
                facts.branch_ref
            )));
        }
        git_checked(
            &self.repository_root,
            "delete branch left by interrupted worktree cleanup",
            [
                OsString::from("update-ref"),
                OsString::from("-d"),
                facts.branch_ref.clone().into(),
                expected.into(),
            ],
        )?;
        if resolve_ref_optional(&self.repository_root, &facts.branch_ref)?.is_some() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "writer branch still exists after branch-only CAS deletion".to_string(),
            ));
        }
        Ok(Some(CleanupDisposition::Removed))
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

        let initial_paths = collect_worktree_changes(&allocation.worktree_path)?;
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

        ensure_no_unstaged_or_untracked_changes(&allocation.worktree_path)?;
        let changed_files =
            staged_changed_paths(&allocation.worktree_path, allocation.base_commit())?;
        if changed_files.is_empty() {
            return Err(GitWorkspaceError::EmptyDiff);
        }
        validate_changed_paths(allocation, &changed_files)?;
        validate_changed_modes(
            &allocation.worktree_path,
            allocation.base_commit(),
            &changed_files,
        )?;

        let diff = binary_diff_cached(&allocation.worktree_path, allocation.base_commit())?;
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
        ensure_no_unstaged_or_untracked_changes(&allocation.worktree_path)?;

        let final_commit = git_text_with_identity(
            &allocation.worktree_path,
            "commit sealed writer tree",
            [
                OsString::from("commit-tree"),
                tree.clone().into(),
                OsString::from("-p"),
                allocation.base_commit.clone().into(),
                OsString::from("-m"),
                format!("CodeWhale writer {}", allocation.owner_id).into(),
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

    /// Remove only an exactly-owned, clean worktree and its exact writer ref.
    ///
    /// There is deliberately no `remove_dir_all`, `git clean`, `git reset`,
    /// `git worktree prune`, or force removal fallback.
    pub fn cleanup(
        &self,
        allocation: &OwnedWorktree,
        expected_branch_commit: &str,
    ) -> Result<CleanupDisposition> {
        self.verify_allocation_owner(allocation)?;
        let expected = resolve_exact_commit(&self.repository_root, expected_branch_commit)?;
        let records = registered_worktrees(&self.repository_root)?;
        let record = records
            .iter()
            .find(|record| same_path(&record.path, &allocation.worktree_path));
        let branch_commit = resolve_ref_optional(&self.repository_root, &allocation.branch_ref)?;
        let path_metadata = symlink_metadata_optional(&allocation.worktree_path)?;
        let admin_metadata = symlink_metadata_optional(&allocation.worktree_git_dir)?;

        if record.is_none()
            && branch_commit.is_none()
            && path_metadata.is_none()
            && admin_metadata.is_none()
        {
            return Ok(CleanupDisposition::AlreadyAbsent);
        }
        if let Some(actual) = branch_commit.as_deref()
            && actual != expected
        {
            return Err(GitWorkspaceError::OwnershipMismatch(format!(
                "writer branch {} points to {actual}, expected {expected}",
                allocation.branch_ref
            )));
        }

        if let Some(record) = record {
            verify_record(record, allocation, &expected)?;
            verify_admin_backpointer(allocation)?;
            if path_metadata.is_some() {
                verify_worktree_backpointer(allocation)?;
                ensure_clean(&allocation.worktree_path, "owned worktree cleanup")?;
            }
            git_checked(
                &self.repository_root,
                "remove owned writer worktree",
                [
                    OsString::from("worktree"),
                    OsString::from("remove"),
                    allocation.worktree_path.as_os_str().to_owned(),
                ],
            )?;
            if registered_worktrees(&self.repository_root)?
                .iter()
                .any(|candidate| same_path(&candidate.path, &allocation.worktree_path))
                || symlink_metadata_optional(&allocation.worktree_path)?.is_some()
                || symlink_metadata_optional(&allocation.worktree_git_dir)?.is_some()
            {
                return Err(GitWorkspaceError::OwnershipMismatch(
                    "Git did not fully remove the owned worktree registration".to_string(),
                ));
            }
        } else if path_metadata.is_some() || admin_metadata.is_some() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "worktree path or admin directory exists without an exact Git registration"
                    .to_string(),
            ));
        }

        if branch_commit.is_some() {
            git_checked(
                &self.repository_root,
                "delete exact owned writer branch",
                [
                    OsString::from("update-ref"),
                    OsString::from("-d"),
                    allocation.branch_ref.clone().into(),
                    expected.into(),
                ],
            )?;
        }
        if resolve_ref_optional(&self.repository_root, &allocation.branch_ref)?.is_some() {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "owned writer branch still exists after exact deletion".to_string(),
            ));
        }
        Ok(CleanupDisposition::Removed)
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
                    "another CodeWhale root integration is active".to_string(),
                ));
            }
            return Err(GitWorkspaceError::Io {
                operation: "lock CodeWhale integration lease",
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
        validate_owner_id(&facts.owner_id)?;
        validate_limits(facts.limits)?;
        let allowed_paths = normalize_allowed_paths(facts.allowed_paths.clone())?;
        if allowed_paths != facts.allowed_paths {
            return Err(GitWorkspaceError::OwnershipMismatch(
                "persisted allowed paths are not canonical".to_string(),
            ));
        }
        resolve_exact_commit(&self.repository_root, &facts.base_commit)?;
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

fn validate_changed_modes(worktree: &Path, base: &str, changed: &[PathBuf]) -> Result<()> {
    let changed = changed.iter().cloned().collect::<BTreeSet<_>>();
    let index = git_checked(
        worktree,
        "inspect sealed index modes",
        [
            OsString::from("ls-files"),
            OsString::from("--stage"),
            OsString::from("-z"),
        ],
    )?;
    for (mode, path) in parse_mode_path_records(&index)? {
        if changed.contains(&path) && mode != "100644" && mode != "100755" {
            return Err(GitWorkspaceError::Conflict(format!(
                "writer index contains unsupported mode {mode} at {}",
                path.display()
            )));
        }
    }
    let base_tree = git_checked(
        worktree,
        "inspect base tree modes",
        [
            OsString::from("ls-tree"),
            OsString::from("-r"),
            OsString::from("-z"),
            base.to_string().into(),
            OsString::from("--"),
        ],
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

fn collect_worktree_changes(worktree: &Path) -> Result<Vec<PathBuf>> {
    let tracked = git_checked(
        worktree,
        "collect tracked writer changes",
        [
            OsString::from("-c"),
            OsString::from("diff.renames=false"),
            OsString::from("diff"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            OsString::from("HEAD"),
            OsString::from("--"),
        ],
    )?;
    let untracked = git_checked(
        worktree,
        "collect untracked writer changes",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
    )?;
    let mut paths = parse_nul_paths(&tracked)?;
    paths.extend(parse_nul_paths(&untracked)?);
    Ok(paths.into_iter().collect())
}

fn staged_changed_paths(worktree: &Path, base: &str) -> Result<Vec<PathBuf>> {
    let bytes = git_checked(
        worktree,
        "collect staged writer changes",
        [
            OsString::from("diff"),
            OsString::from("--cached"),
            OsString::from("--no-renames"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            base.to_string().into(),
            OsString::from("--"),
        ],
    )?;
    Ok(parse_nul_paths(&bytes)?.into_iter().collect())
}

fn parse_nul_paths(bytes: &[u8]) -> Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    for raw in bytes.split(|byte| *byte == 0).filter(|raw| !raw.is_empty()) {
        paths.insert(path_from_git_bytes(raw)?);
    }
    Ok(paths)
}

fn binary_diff_cached(worktree: &Path, base: &str) -> Result<Vec<u8>> {
    git_checked(
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
    )
}

fn binary_diff_commits(repo: &Path, base: &str, final_commit: &str) -> Result<Vec<u8>> {
    git_checked(
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
    let diff = binary_diff_commits(repo, allocation.base_commit(), sealed.final_commit())?;
    if sha256_hex(&diff) != sealed.diff_sha256 || diff != sealed.diff {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "sealed commit diff no longer matches Host result".to_string(),
        ));
    }
    let changed = committed_changed_paths(repo, allocation.base_commit(), sealed.final_commit())?;
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
        "CodeWhale Host\0host@codewhale.local\0CodeWhale Host\0host@codewhale.local\0CodeWhale writer {}\n",
        allocation.owner_id
    );
    if identity != expected.as_bytes() {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "prepared seal commit lacks the exact Host owner identity".to_string(),
        ));
    }
    Ok(())
}

fn committed_changed_paths(repo: &Path, base: &str, final_commit: &str) -> Result<Vec<PathBuf>> {
    let bytes = git_checked(
        repo,
        "collect sealed changed paths",
        [
            OsString::from("diff"),
            OsString::from("--no-renames"),
            OsString::from("--name-only"),
            OsString::from("-z"),
            base.to_string().into(),
            final_commit.to_string().into(),
            OsString::from("--"),
        ],
    )?;
    Ok(parse_nul_paths(&bytes)?.into_iter().collect())
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

fn ensure_no_unstaged_or_untracked_changes(worktree: &Path) -> Result<()> {
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
    let untracked = git_checked(
        worktree,
        "check untracked writer changes",
        [
            OsString::from("ls-files"),
            OsString::from("--others"),
            OsString::from("--exclude-standard"),
            OsString::from("-z"),
            OsString::from("--"),
        ],
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
    if !matches!(requested.len(), 40 | 64)
        || !requested.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(GitWorkspaceError::InvalidRequest(
            "base/expected commit must be a full hexadecimal object id".to_string(),
        ));
    }
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
    let bytes = git_checked(
        repo,
        "list registered worktrees",
        [
            OsString::from("worktree"),
            OsString::from("list"),
            OsString::from("--porcelain"),
            OsString::from("-z"),
        ],
    )?;
    let mut records = Vec::new();
    let mut fields = BTreeMap::<String, Vec<u8>>::new();
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if !fields.is_empty() {
                records.push(worktree_record_from_fields(&fields)?);
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
    Ok(WorktreeRecord {
        path,
        head,
        branch_ref,
    })
}

fn verify_record(
    record: &WorktreeRecord,
    allocation: &OwnedWorktree,
    expected: &str,
) -> Result<()> {
    if record.head != expected {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "registered worktree HEAD is {}, expected {expected}",
            record.head
        )));
    }
    if record.branch_ref.as_deref() != Some(allocation.branch_ref.as_str()) {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "registered worktree branch is {:?}, expected {}",
            record.branch_ref, allocation.branch_ref
        )));
    }
    Ok(())
}

fn verify_worktree_backpointer(allocation: &OwnedWorktree) -> Result<()> {
    let pointer_path = allocation.worktree_path.join(".git");
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
    let target = resolve_pointer_path(&allocation.worktree_path, Path::new(raw));
    let target = canonical_dir(&target, "canonicalize .git pointer target")?;
    if target != allocation.worktree_git_dir {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "worktree .git points to {}, expected {}",
            target.display(),
            allocation.worktree_git_dir.display()
        )));
    }
    let admin_parent = allocation.common_git_dir.join("worktrees");
    let admin_parent = canonical_dir(&admin_parent, "canonicalize worktree admin root")?;
    if !allocation.worktree_git_dir.starts_with(admin_parent) {
        return Err(GitWorkspaceError::OwnershipMismatch(
            "worktree admin directory is outside common Git worktrees directory".to_string(),
        ));
    }
    Ok(())
}

fn verify_admin_backpointer(allocation: &OwnedWorktree) -> Result<()> {
    let backpointer = allocation.worktree_git_dir.join("gitdir");
    let raw = read_bounded_pointer(&backpointer)?;
    let target = resolve_pointer_path(&allocation.worktree_git_dir, Path::new(&raw));
    let expected = allocation.worktree_path.join(".git");
    if !same_path(&target, &expected) {
        return Err(GitWorkspaceError::OwnershipMismatch(format!(
            "worktree admin backpointer targets {}, expected {}",
            target.display(),
            expected.display()
        )));
    }
    Ok(())
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
        .arg("commit.gpgSign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    if with_identity {
        command
            .env("GIT_AUTHOR_NAME", "CodeWhale Host")
            .env("GIT_AUTHOR_EMAIL", "host@codewhale.local")
            .env("GIT_COMMITTER_NAME", "CodeWhale Host")
            .env("GIT_COMMITTER_EMAIL", "host@codewhale.local");
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
                operation: "inspect existing CodeWhale integration lease",
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
                    operation: "open existing CodeWhale integration lease",
                    path: path.to_path_buf(),
                    source,
                })
        }
        Err(source) => Err(GitWorkspaceError::Io {
            operation: "create CodeWhale integration lease",
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

fn unique_integration_marker_temp_path(root_git_dir: &Path) -> Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_INTEGRATION_LOCK_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = root_git_dir.join(format!(
        ".codewhale-head-lock-{}-{timestamp}-{sequence}.tmp",
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
    use std::cell::Cell;
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
                    OsString::from("CodeWhale Test"),
                ],
            );
            git_ok(
                &root,
                [
                    OsString::from("config"),
                    OsString::from("user.email"),
                    OsString::from("test@codewhale.local"),
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
        assert_eq!(facts.branch_ref, "refs/heads/codewhale/writer/writer_plan");
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

        assert_eq!(
            owner
                .cleanup(&allocation, sealed.final_commit())
                .expect("cleanup"),
            CleanupDisposition::Removed
        );
        assert!(!allocation.path().exists());
        assert_eq!(
            owner
                .cleanup(&allocation, sealed.final_commit())
                .expect("idempotent cleanup"),
            CleanupDisposition::AlreadyAbsent
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
                            "Git switched branches while CodeWhale held HEAD.lock".to_string(),
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
            "CodeWhale must never delete a foreign Git lock"
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
            resolve_ref_optional(&repo.root, allocation.branch_ref())
                .expect("writer ref")
                .as_deref(),
            Some(sealed.final_commit())
        );

        assert_eq!(
            owner
                .cleanup_branch_only(&facts, sealed.final_commit())
                .expect("resume branch deletion"),
            Some(CleanupDisposition::Removed)
        );
        assert_eq!(
            owner
                .cleanup_branch_only(&facts, sealed.final_commit())
                .expect("repeat branch deletion"),
            Some(CleanupDisposition::AlreadyAbsent)
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
    fn cleanup_never_removes_a_dirty_or_ref_mismatched_worktree() {
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
            owner.cleanup(&allocation, allocation.base_commit()),
            Err(GitWorkspaceError::DirtyRepository(_))
        ));
        assert!(allocation.path().exists());

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
            owner.cleanup(&allocation, allocation.base_commit()),
            Err(GitWorkspaceError::OwnershipMismatch(_))
        ));
        assert!(allocation.path().exists());
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
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(any(unix, windows)))]
fn null_device() -> &'static str {
    "/dev/null"
}
