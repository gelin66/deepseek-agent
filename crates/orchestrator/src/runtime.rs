use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use codewhale_runtime::{
    AgentOrchestrationError, AgentOrchestrationErrorKind, AgentOrchestrator, AgentTask,
    AgentWorkspaceAccess, AgentWorkspaceAssignment, ToolExecutor, WorkspaceRevision,
    WorkspaceState, WriterBinding, WriterCleanup, WriterIntegration, WriterPlan, WriterPreparation,
    WriterSeal,
};
use codewhale_tools::{ProductionToolConfig, ProductionToolExecutor};

use crate::workspace::{
    CleanupDisposition, GitWorkspaceError, GitWorkspaceOwner, OwnedWorktree, OwnedWorktreeFacts,
    SealLimits, SealedWorktree, SealedWorktreeFacts, WriterWorkspaceRequest,
};

/// Concrete production implementation of the canonical writer port.
///
/// This type owns Git/worktree side effects and fresh writer executors only.
/// It never owns a model, RunStore, lifecycle event, or terminal decision.
#[derive(Clone)]
pub struct ProductionAgentOrchestrator {
    repository_root: PathBuf,
    managed_root: PathBuf,
    root_tools: ProductionToolConfig,
}

impl ProductionAgentOrchestrator {
    /// Bind the canonical root tool configuration to an already initialized
    /// external managed directory.
    ///
    /// Git shape and cleanliness are checked lazily by the read-only planning
    /// operation, so ordinary non-writer runs keep working in non-Git or dirty
    /// workspaces.
    pub fn new(
        repository_root: impl AsRef<Path>,
        managed_root: impl AsRef<Path>,
        root_tools: ProductionToolConfig,
    ) -> Result<Self, GitWorkspaceError> {
        let repository_root =
            repository_root
                .as_ref()
                .canonicalize()
                .map_err(|source| GitWorkspaceError::Io {
                    operation: "canonicalize orchestration root",
                    path: repository_root.as_ref().to_path_buf(),
                    source,
                })?;
        let managed_root =
            managed_root
                .as_ref()
                .canonicalize()
                .map_err(|source| GitWorkspaceError::Io {
                    operation: "canonicalize managed worktree root",
                    path: managed_root.as_ref().to_path_buf(),
                    source,
                })?;
        let tool_workspace = PathBuf::from(root_tools.execution_identity().workspace);
        let canonical_tool_workspace =
            tool_workspace
                .canonicalize()
                .map_err(|source| GitWorkspaceError::Io {
                    operation: "canonicalize root tool workspace",
                    path: tool_workspace.clone(),
                    source,
                })?;
        if canonical_tool_workspace != repository_root {
            return Err(GitWorkspaceError::InvalidRequest(format!(
                "root tool workspace {} does not match repository {}",
                canonical_tool_workspace.display(),
                repository_root.display()
            )));
        }
        Ok(Self {
            repository_root,
            managed_root,
            root_tools,
        })
    }

    fn owner(&self) -> Result<GitWorkspaceOwner, GitWorkspaceError> {
        GitWorkspaceOwner::bind_existing(&self.repository_root, &self.managed_root)
    }

    fn owner_for_integration(&self) -> Result<GitWorkspaceOwner, GitWorkspaceError> {
        GitWorkspaceOwner::bind_existing_for_integration(&self.repository_root, &self.managed_root)
    }

    fn assignment_for_facts(&self, facts: &OwnedWorktreeFacts) -> AgentWorkspaceAssignment {
        AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::IsolatedWrite,
            root_workspace: stable_path(&self.repository_root),
            base_commit: facts.base_commit.clone(),
            worktree_path: Some(stable_path(&facts.worktree_path)),
            root_branch: Some(facts.root_branch_ref.clone()),
            branch: Some(facts.branch_ref.clone()),
            allowed_paths: facts
                .allowed_paths
                .iter()
                .map(|path| stable_path(path))
                .collect(),
            owner_token: Some(facts.owner_id.clone()),
        }
    }

    fn owned_facts(&self, task: &AgentTask) -> Result<OwnedWorktreeFacts, AgentOrchestrationError> {
        task.validate().map_err(|message| {
            orchestration_error(
                AgentOrchestrationErrorKind::Rejected,
                "writer_task_invalid",
                message,
            )
        })?;
        if task.workspace.access != AgentWorkspaceAccess::IsolatedWrite {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::Rejected,
                "writer_access_required",
                "AgentTask 未获得 isolated_write workspace authority",
            ));
        }
        if task.workspace.root_workspace != stable_path(&self.repository_root) {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_root_mismatch",
                "持久化 writer root workspace 与当前 Git owner 不一致",
            ));
        }
        let facts = OwnedWorktreeFacts {
            owner_id: task
                .workspace
                .owner_token
                .clone()
                .expect("validated writer assignment has owner token"),
            base_commit: task.workspace.base_commit.clone(),
            worktree_path: PathBuf::from(
                task.workspace
                    .worktree_path
                    .as_deref()
                    .expect("validated writer assignment has worktree path"),
            ),
            branch_ref: task
                .workspace
                .branch
                .clone()
                .expect("validated writer assignment has branch"),
            root_branch_ref: task
                .workspace
                .root_branch
                .clone()
                .expect("validated writer assignment has root branch"),
            allowed_paths: task
                .workspace
                .allowed_paths
                .iter()
                .map(PathBuf::from)
                .collect(),
            limits: SealLimits::default(),
        };
        if self.assignment_for_facts(&facts) != task.workspace {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_assignment_mismatch",
                "持久化 writer assignment 不是当前 owner 的精确派生值",
            ));
        }
        Ok(facts)
    }

    fn writer_executor(&self, workspace: &Path) -> Arc<ProductionToolExecutor> {
        Arc::new(ProductionToolExecutor::new(
            self.root_tools
                .rebind_isolated_writer_workspace(workspace.to_path_buf()),
        ))
    }

    fn root_executor(&self) -> Arc<ProductionToolExecutor> {
        Arc::new(ProductionToolExecutor::new(self.root_tools.clone()))
    }
}

#[async_trait]
impl AgentOrchestrator for ProductionAgentOrchestrator {
    async fn prepare_writer(
        &self,
        request: WriterPreparation,
    ) -> Result<WriterPlan, AgentOrchestrationError> {
        if request.root_workspace != stable_path(&self.repository_root) {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::Rejected,
                "writer_root_mismatch",
                "writer 请求的 root workspace 与 production composition 不一致",
            ));
        }
        let owner = self.owner().map_err(map_git_error)?;
        let owner_id = request.task_id.0;
        let allowed_paths = request
            .allowed_paths
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let facts =
            tokio::task::spawn_blocking(move || owner.plan_current(owner_id, allowed_paths))
                .await
                .map_err(join_error)?
                .map_err(map_git_error)?;
        let assignment = self.assignment_for_facts(&facts);
        assignment.validate().map_err(|message| {
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_plan_invalid",
                message,
            )
        })?;
        Ok(WriterPlan { assignment })
    }

    async fn bind_writer(
        &self,
        task: &AgentTask,
        sealed: Option<&WriterSeal>,
    ) -> Result<WriterBinding, AgentOrchestrationError> {
        let facts = self.owned_facts(task)?;
        let owner = self.owner().map_err(map_active_git_error)?;
        let binding_recovery_owner = owner.clone();
        let sealed_facts = sealed.map(|seal| SealedWorktreeFacts {
            owner_id: facts.owner_id.clone(),
            base_commit: seal.base_commit.clone(),
            final_commit: seal.final_commit.clone(),
            diff_sha256: seal.diff_sha256.clone(),
        });
        let allocation = tokio::task::spawn_blocking(move || match sealed_facts {
            Some(sealed) => owner
                .recover_sealed(facts, sealed)
                .map(|(allocation, _)| allocation),
            None => {
                let request = WriterWorkspaceRequest {
                    owner_id: facts.owner_id.clone(),
                    base_commit: facts.base_commit.clone(),
                    allowed_paths: facts.allowed_paths.clone(),
                    limits: facts.limits,
                };
                match owner.create(request) {
                    Ok(allocation) => Ok(allocation),
                    Err(
                        GitWorkspaceError::OwnershipMismatch(_) | GitWorkspaceError::Conflict(_),
                    ) => owner
                        .recover_prepared_seal(facts)
                        .map(|(allocation, _)| allocation),
                    Err(error) => Err(error),
                }
            }
        })
        .await
        .map_err(join_error)?
        .map_err(map_active_git_error)?;
        if self.assignment_for_facts(&allocation.durable_facts()) != task.workspace {
            let cause = orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_binding_mismatch",
                "已创建或恢复的 worktree 与 AgentTask assignment 不一致",
            );
            return Err(rollback_or_retain_failed_binding(
                binding_recovery_owner,
                allocation,
                cause,
            )
            .await);
        }
        let tools = self.writer_executor(allocation.path());
        let writer_workspace_state = match observe_workspace(tools.as_ref(), 0).await {
            Ok(state) => state,
            Err(cause) => {
                return Err(rollback_or_retain_failed_binding(
                    binding_recovery_owner,
                    allocation,
                    cause,
                )
                .await);
            }
        };
        Ok(WriterBinding {
            assignment: task.workspace.clone(),
            writer_workspace_state,
            tools,
        })
    }

    async fn seal_writer(&self, task: &AgentTask) -> Result<WriterSeal, AgentOrchestrationError> {
        let facts = self.owned_facts(task)?;
        let owner = self.owner().map_err(map_active_git_error)?;
        let sealed =
            tokio::task::spawn_blocking(move || match owner.recover_active(facts.clone()) {
                Ok(allocation) => owner.seal(&allocation),
                Err(GitWorkspaceError::OwnershipMismatch(_) | GitWorkspaceError::Conflict(_)) => {
                    owner.recover_prepared_seal(facts).map(|(_, sealed)| sealed)
                }
                Err(error) => Err(error),
            })
            .await
            .map_err(join_error)?
            .map_err(map_active_git_error)?;
        writer_seal(
            self.writer_executor(Path::new(task.workspace.execution_workspace())),
            sealed,
        )
        .await
    }

    async fn integrate_writer(
        &self,
        task: &AgentTask,
        seal: &WriterSeal,
        expected_root: &WorkspaceState,
    ) -> Result<WriterIntegration, AgentOrchestrationError> {
        let actual_before =
            observe_workspace(self.root_executor().as_ref(), expected_root.generation).await?;
        let facts = self.owned_facts(task)?;
        let sealed_facts = SealedWorktreeFacts {
            owner_id: facts.owner_id.clone(),
            base_commit: seal.base_commit.clone(),
            final_commit: seal.final_commit.clone(),
            diff_sha256: seal.diff_sha256.clone(),
        };
        let owner = self
            .owner_for_integration()
            .map_err(map_integration_git_error)?;
        let inspection_owner = owner.clone();
        let current_head = tokio::task::spawn_blocking(move || {
            inspection_owner.current_root_head_for_integration()
        })
        .await
        .map_err(join_error)?
        .map_err(map_integration_git_error)?;
        if current_head == seal.base_commit {
            if &actual_before != expected_root {
                return Err(orchestration_error(
                    AgentOrchestrationErrorKind::Conflict,
                    "writer_root_revision_changed",
                    "writer 集成前 root workspace revision 已改变",
                ));
            }
        } else if current_head != seal.final_commit {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::Conflict,
                "writer_root_head_changed",
                "root HEAD 既不是 writer base，也不是 exact Host-sealed commit",
            ));
        }
        let result = tokio::task::spawn_blocking(move || {
            let (allocation, sealed) = owner.recover_sealed_for_integration(facts, sealed_facts)?;
            owner.integrate(&allocation, &sealed)
        })
        .await
        .map_err(join_error)?
        .map_err(map_integration_git_error)?;
        if result.root_commit != seal.final_commit {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_integrated_commit_mismatch",
                "CAS 集成后的 root commit 与 Host seal 不一致",
            ));
        }
        let root_workspace_state = observe_workspace(
            self.root_executor().as_ref(),
            expected_root.generation.saturating_add(1),
        )
        .await?;
        Ok(WriterIntegration {
            root_head_commit: result.root_commit,
            root_workspace_state,
        })
    }

    async fn cleanup_writer(
        &self,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
    ) -> Result<WriterCleanup, AgentOrchestrationError> {
        let facts = self.owned_facts(task)?;
        let expected_commit = seal
            .map(|seal| seal.final_commit.clone())
            .unwrap_or_else(|| facts.base_commit.clone());
        let owner = self.owner().map_err(map_active_git_error)?;
        let sealed_facts = seal.map(|seal| SealedWorktreeFacts {
            owner_id: facts.owner_id.clone(),
            base_commit: seal.base_commit.clone(),
            final_commit: seal.final_commit.clone(),
            diff_sha256: seal.diff_sha256.clone(),
        });
        let cleanup = tokio::task::spawn_blocking(move || {
            if let Some(disposition) = owner.cleanup_branch_only(&facts, &expected_commit)? {
                return Ok(disposition);
            }
            let allocation = match sealed_facts {
                Some(sealed) => owner.recover_sealed(facts, sealed)?.0,
                None => owner.recover_active(facts)?,
            };
            owner.cleanup(&allocation, &expected_commit)
        })
        .await
        .map_err(join_error)?;
        match cleanup {
            Ok(CleanupDisposition::Removed | CleanupDisposition::AlreadyAbsent) => {
                Ok(WriterCleanup {
                    worktree_removed: true,
                    branch_removed: true,
                    retained_for_recovery: false,
                    reason: None,
                })
            }
            Err(GitWorkspaceError::DirtyRepository(message)) => Ok(WriterCleanup {
                worktree_removed: false,
                branch_removed: false,
                retained_for_recovery: true,
                reason: Some(message),
            }),
            Err(error) => Err(map_git_error(error)),
        }
    }
}

async fn rollback_or_retain_failed_binding(
    owner: GitWorkspaceOwner,
    allocation: OwnedWorktree,
    cause: AgentOrchestrationError,
) -> AgentOrchestrationError {
    let worktree_path = stable_path(allocation.path());
    let branch_ref = allocation.branch_ref().to_owned();
    let base_commit = allocation.base_commit().to_owned();
    let rollback =
        tokio::task::spawn_blocking(move || owner.cleanup(&allocation, &base_commit)).await;
    match rollback {
        Ok(Ok(CleanupDisposition::Removed | CleanupDisposition::AlreadyAbsent)) => {
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_binding_postcondition_rolled_back",
                format!(
                    "writer bind 后置条件失败，未启动的精确 worktree/branch 已回滚；原始错误 {}: {}",
                    cause.code, cause.message
                ),
            )
        }
        Ok(Err(rollback_error)) => orchestration_error(
            AgentOrchestrationErrorKind::RecoveryRequired,
            "writer_binding_postcondition_retained",
            format!(
                "writer bind 后置条件失败，无法安全回滚，因此保留精确恢复资源 path={worktree_path}, branch={branch_ref}；原始错误 {}: {}；回滚拒绝: {rollback_error}",
                cause.code, cause.message
            ),
        ),
        Err(join) => orchestration_error(
            AgentOrchestrationErrorKind::RecoveryRequired,
            "writer_binding_postcondition_retained",
            format!(
                "writer bind 后置条件失败，回滚任务未完成，因此保留精确恢复资源 path={worktree_path}, branch={branch_ref}；原始错误 {}: {}；回滚任务错误: {join}",
                cause.code, cause.message
            ),
        ),
    }
}

async fn writer_seal(
    tools: Arc<ProductionToolExecutor>,
    sealed: SealedWorktree,
) -> Result<WriterSeal, AgentOrchestrationError> {
    let writer_workspace_state = observe_workspace(tools.as_ref(), 0).await?;
    let mut changed_files = sealed
        .changed_files()
        .iter()
        .map(|path| stable_path(path))
        .collect::<Vec<_>>();
    changed_files.sort();
    Ok(WriterSeal {
        base_commit: sealed.base_commit().to_owned(),
        final_commit: sealed.final_commit().to_owned(),
        diff_sha256: sealed.diff_sha256().to_owned(),
        changed_files,
        writer_workspace_state,
    })
}

async fn observe_workspace(
    tools: &dyn ToolExecutor,
    generation: u64,
) -> Result<WorkspaceState, AgentOrchestrationError> {
    let revision = tools.observe_workspace_revision().await.map_err(|error| {
        orchestration_error(
            AgentOrchestrationErrorKind::RecoveryRequired,
            error.code,
            error.message,
        )
    })?;
    Ok(WorkspaceState {
        generation,
        revision: WorkspaceRevision::Known { sha256: revision },
    })
}

fn stable_path(path: &Path) -> String {
    path.display().to_string()
}

fn join_error(error: tokio::task::JoinError) -> AgentOrchestrationError {
    orchestration_error(
        AgentOrchestrationErrorKind::RecoveryRequired,
        "writer_git_task_join_failed",
        format!("Git workspace task failed to join: {error}"),
    )
}

fn map_git_error(error: GitWorkspaceError) -> AgentOrchestrationError {
    let kind = match &error {
        GitWorkspaceError::Conflict(_) => AgentOrchestrationErrorKind::Conflict,
        GitWorkspaceError::OwnershipMismatch(_)
        | GitWorkspaceError::Git { .. }
        | GitWorkspaceError::Io { .. } => AgentOrchestrationErrorKind::RecoveryRequired,
        GitWorkspaceError::InvalidRepository(_)
        | GitWorkspaceError::DirtyRepository(_)
        | GitWorkspaceError::UnsupportedRepository(_)
        | GitWorkspaceError::InvalidRequest(_)
        | GitWorkspaceError::LimitExceeded(_)
        | GitWorkspaceError::EmptyDiff => AgentOrchestrationErrorKind::Rejected,
    };
    let code = match &error {
        GitWorkspaceError::InvalidRepository(_) => "writer_repository_invalid",
        GitWorkspaceError::DirtyRepository(_) => "writer_repository_dirty",
        GitWorkspaceError::UnsupportedRepository(_) => "writer_repository_unsupported",
        GitWorkspaceError::InvalidRequest(_) => "writer_request_invalid",
        GitWorkspaceError::OwnershipMismatch(_) => "writer_ownership_mismatch",
        GitWorkspaceError::Conflict(_) => "writer_workspace_conflict",
        GitWorkspaceError::LimitExceeded(_) => "writer_limit_exceeded",
        GitWorkspaceError::EmptyDiff => "writer_empty_diff",
        GitWorkspaceError::Git { .. } => "writer_git_failed",
        GitWorkspaceError::Io { .. } => "writer_io_failed",
    };
    orchestration_error(kind, code, error.to_string())
}

fn map_active_git_error(error: GitWorkspaceError) -> AgentOrchestrationError {
    let mut mapped = map_git_error(error);
    if mapped.kind == AgentOrchestrationErrorKind::Rejected {
        mapped.kind = AgentOrchestrationErrorKind::RecoveryRequired;
    }
    mapped
}

fn map_integration_git_error(error: GitWorkspaceError) -> AgentOrchestrationError {
    let root_conflict = matches!(
        &error,
        GitWorkspaceError::DirtyRepository(_)
            | GitWorkspaceError::Conflict(_)
            | GitWorkspaceError::InvalidRepository(_)
            | GitWorkspaceError::UnsupportedRepository(_)
    );
    let mut mapped = map_active_git_error(error);
    if root_conflict {
        mapped.kind = AgentOrchestrationErrorKind::Conflict;
    }
    mapped
}

fn orchestration_error(
    kind: AgentOrchestrationErrorKind,
    code: impl Into<String>,
    message: impl Into<String>,
) -> AgentOrchestrationError {
    AgentOrchestrationError::new(kind, code, message)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::process::Command;

    use codewhale_runtime::{
        AgentTaskId, RunId, RunLimits, TaskContract, TaskDefinition, TaskGenerationId, ToolPolicy,
    };
    use codewhale_tools::shell::ShellPolicy;
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn prepare_rejects_dirty_root_before_any_writer_side_effect() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        fs::write(fixture.root.join("dirty.txt"), "external change\n").expect("dirty root");

        let error = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_dirty_prepare"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect_err("dirty root must be rejected at writer admission");
        assert_eq!(error.kind, AgentOrchestrationErrorKind::Rejected);
        assert_eq!(error.code, "writer_repository_dirty");
        assert!(!fixture.managed.join("writer_dirty_prepare").exists());
    }

    #[tokio::test]
    async fn failed_bind_postconditions_roll_back_pristine_or_retain_dirty_resources() {
        let pristine_fixture = RepositoryFixture::new();
        let pristine_owner =
            GitWorkspaceOwner::bind_existing(&pristine_fixture.root, &pristine_fixture.managed)
                .expect("pristine owner");
        let pristine = pristine_owner
            .create(WriterWorkspaceRequest::new(
                "writer_bind_rollback",
                pristine_fixture.head.clone(),
                vec![PathBuf::from("src")],
            ))
            .expect("pristine allocation");
        let pristine_facts = pristine.durable_facts();
        let error = rollback_or_retain_failed_binding(
            pristine_owner.clone(),
            pristine,
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "synthetic_observation_failure",
                "synthetic observation failure",
            ),
        )
        .await;
        assert_eq!(error.code, "writer_binding_postcondition_rolled_back");
        assert!(
            pristine_owner
                .exact_resources_absent(&pristine_facts)
                .expect("pristine resources absent")
        );

        let dirty_fixture = RepositoryFixture::new();
        let dirty_owner =
            GitWorkspaceOwner::bind_existing(&dirty_fixture.root, &dirty_fixture.managed)
                .expect("dirty owner");
        let dirty = dirty_owner
            .create(WriterWorkspaceRequest::new(
                "writer_bind_retained",
                dirty_fixture.head.clone(),
                vec![PathBuf::from("src")],
            ))
            .expect("dirty allocation");
        let dirty_facts = dirty.durable_facts();
        fs::write(
            dirty.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 99 }\n",
        )
        .expect("dirty writer");
        let error = rollback_or_retain_failed_binding(
            dirty_owner.clone(),
            dirty,
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "synthetic_observation_failure",
                "synthetic observation failure",
            ),
        )
        .await;
        assert_eq!(error.code, "writer_binding_postcondition_retained");
        assert!(dirty_facts.worktree_path.exists());
        assert!(
            resolve_ref_optional_for_test(&dirty_fixture.root, &dirty_facts.branch_ref).is_some()
        );
    }

    #[tokio::test]
    async fn production_writer_is_isolated_recoverable_and_integrates_once() {
        let fixture = RepositoryFixture::new();
        let root_tools = ProductionToolConfig::new(&fixture.root)
            .with_shell_policy(ShellPolicy::Full)
            .with_auto_approve(true);
        let orchestrator =
            ProductionAgentOrchestrator::new(&fixture.root, &fixture.managed, root_tools.clone())
                .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_runtime"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("pure writer plan");
        assert!(!fixture.managed.join("writer_runtime").exists());

        let task = writer_task(plan.assignment);
        let first = orchestrator
            .bind_writer(&task, None)
            .await
            .expect("create writer");
        let root_before = observe_workspace(&ProductionToolExecutor::new(root_tools), 0)
            .await
            .expect("root state before writer");
        fs::write(
            Path::new(first.assignment.execution_workspace()).join("src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .expect("edit isolated writer");

        let recovered = orchestrator
            .bind_writer(&task, None)
            .await
            .expect("recover dirty exact writer");
        assert_eq!(recovered.assignment, first.assignment);
        assert_eq!(
            fs::read_to_string(fixture.root.join("src/lib.rs"))
                .expect("root source before integration"),
            "pub fn value() -> u8 { 1 }\n",
            "writer mutation must not touch the canonical root"
        );

        let seal = orchestrator.seal_writer(&task).await.expect("seal writer");
        assert_eq!(seal.base_commit, fixture.head);
        assert_eq!(seal.diff_sha256.len(), 64);
        assert_eq!(seal.changed_files, vec!["src/lib.rs"]);
        assert_eq!(
            git_text(&fixture.root, &["rev-parse", "HEAD"]),
            fixture.head,
            "Host seal must not advance root"
        );
        let rebound_after_seal = orchestrator
            .bind_writer(&task, None)
            .await
            .expect("recover seal side effect before durable commit event");
        assert_eq!(rebound_after_seal.assignment, task.workspace);
        let repeated_seal = orchestrator
            .seal_writer(&task)
            .await
            .expect("reconstruct prepared Host seal");
        assert_eq!(repeated_seal.final_commit, seal.final_commit);
        assert_eq!(repeated_seal.diff_sha256, seal.diff_sha256);

        fs::write(fixture.root.join("root-dirty.txt"), "external change\n")
            .expect("dirty canonical root");
        let dirty_error = orchestrator
            .integrate_writer(&task, &seal, &root_before)
            .await
            .expect_err("dirty root must block integration");
        assert_eq!(
            dirty_error.kind,
            AgentOrchestrationErrorKind::Conflict,
            "integration-time root mutation is a conflict, not writer rejection"
        );
        assert_eq!(dirty_error.code, "writer_root_revision_changed");
        fs::remove_file(fixture.root.join("root-dirty.txt")).expect("restore clean root");

        git_ok(
            &fixture.root,
            &[
                OsString::from("update-ref"),
                task.workspace
                    .root_branch
                    .as_deref()
                    .expect("root branch")
                    .into(),
                seal.final_commit.clone().into(),
                seal.base_commit.clone().into(),
            ],
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("src/lib.rs"))
                .expect("post-CAS pre-recovery root source"),
            "pub fn value() -> u8 { 1 }\n",
            "simulated crash advances only the ref before tree materialization"
        );
        let integrated = orchestrator
            .integrate_writer(&task, &seal, &root_before)
            .await
            .expect("recover post-CAS integration");
        assert_eq!(integrated.root_head_commit, seal.final_commit);
        assert_eq!(
            fs::read_to_string(fixture.root.join("src/lib.rs")).expect("integrated root source"),
            "pub fn value() -> u8 { 2 }\n"
        );
        let repeated = orchestrator
            .integrate_writer(&task, &seal, &root_before)
            .await
            .expect("recover integration committed before durable event");
        assert_eq!(repeated.root_head_commit, seal.final_commit);

        git_ok(
            &fixture.root,
            &[
                OsString::from("worktree"),
                OsString::from("remove"),
                Path::new(task.workspace.execution_workspace())
                    .as_os_str()
                    .to_owned(),
            ],
        );
        assert!(
            resolve_ref_optional_for_test(
                &fixture.root,
                task.workspace.branch.as_deref().expect("writer branch"),
            )
            .is_some(),
            "simulated crash leaves only the writer branch"
        );
        let cleanup = orchestrator
            .cleanup_writer(&task, Some(&seal))
            .await
            .expect("resume branch-only cleanup");
        assert!(cleanup.worktree_removed);
        assert!(cleanup.branch_removed);
        let repeated_cleanup = orchestrator
            .cleanup_writer(&task, Some(&seal))
            .await
            .expect("idempotent cleanup");
        assert!(repeated_cleanup.worktree_removed);
        assert!(repeated_cleanup.branch_removed);

        fs::write(fixture.root.join("root-advanced.txt"), "external commit\n")
            .expect("advance root file");
        git_ok(
            &fixture.root,
            &[OsString::from("add"), OsString::from("root-advanced.txt")],
        );
        git_ok(
            &fixture.root,
            &[
                OsString::from("commit"),
                OsString::from("-m"),
                OsString::from("external advance"),
            ],
        );
        let advanced_error = orchestrator
            .integrate_writer(&task, &seal, &root_before)
            .await
            .expect_err("advanced root must block stale integration");
        assert_eq!(advanced_error.kind, AgentOrchestrationErrorKind::Conflict);
        assert_eq!(advanced_error.code, "writer_root_head_changed");
    }

    fn writer_task(workspace: AgentWorkspaceAssignment) -> AgentTask {
        let child_run_id = RunId::from("writer-child");
        AgentTask {
            task_id: AgentTaskId::from("writer_runtime"),
            root_run_id: RunId::from("root"),
            parent_run_id: RunId::from("root"),
            child_run_id: child_run_id.clone(),
            call_id: "call-writer".to_owned(),
            role: "implementer".to_owned(),
            task_contract: TaskContract {
                generation_id: TaskGenerationId::from(child_run_id.0.clone()),
                definition: TaskDefinition::host("修改 src/lib.rs"),
            },
            workspace,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            deadline_unix_ms: None,
            expected_artifact: "通过 Host seal 的代码 diff".to_owned(),
        }
    }

    struct RepositoryFixture {
        _temp: TempDir,
        root: PathBuf,
        managed: PathBuf,
        head: String,
    }

    impl RepositoryFixture {
        fn new() -> Self {
            let temp = TempDir::new().expect("temporary directory");
            let root = temp.path().join("repo");
            let managed = temp.path().join("worktrees");
            fs::create_dir_all(root.join("src")).expect("source directory");
            fs::create_dir(&managed).expect("managed root");
            git_ok(
                temp.path(),
                &[
                    OsString::from("init"),
                    OsString::from("-b"),
                    OsString::from("main"),
                    root.as_os_str().to_owned(),
                ],
            );
            git_ok(
                &root,
                &[
                    OsString::from("config"),
                    OsString::from("user.name"),
                    OsString::from("CodeWhale Test"),
                ],
            );
            git_ok(
                &root,
                &[
                    OsString::from("config"),
                    OsString::from("user.email"),
                    OsString::from("test@codewhale.local"),
                ],
            );
            fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")
                .expect("source file");
            git_ok(
                &root,
                &[OsString::from("add"), OsString::from("src/lib.rs")],
            );
            git_ok(
                &root,
                &[
                    OsString::from("commit"),
                    OsString::from("-m"),
                    OsString::from("base"),
                ],
            );
            let root = root.canonicalize().expect("canonical repository root");
            let managed = managed.canonicalize().expect("canonical managed root");
            let head = git_text(&root, &["rev-parse", "HEAD"]);
            Self {
                _temp: temp,
                root,
                managed,
                head,
            }
        }
    }

    fn git_ok(cwd: &Path, args: &[OsString]) {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("launch git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_text(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("launch git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf8 git output")
            .trim()
            .to_owned()
    }

    fn resolve_ref_optional_for_test(cwd: &Path, reference: &str) -> Option<String> {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
            .output()
            .expect("launch git");
        if !output.status.success() {
            return None;
        }
        Some(
            String::from_utf8(output.stdout)
                .expect("utf8 git output")
                .trim()
                .to_owned(),
        )
    }
}
