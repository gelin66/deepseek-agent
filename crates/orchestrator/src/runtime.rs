use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use dse_runtime::{
    AgentOrchestrationError, AgentOrchestrationErrorKind, AgentOrchestrator, AgentTask,
    AgentWorkspaceAccess, AgentWorkspaceAssignment, ToolExecutor, WorkspaceRevision,
    WorkspaceState, WriterArtifactState, WriterBinding, WriterCleanupMetadataState,
    WriterCleanupMode, WriterCleanupOwnership, WriterCleanupPhase, WriterCleanupPlan,
    WriterCleanupResult, WriterCleanupScope, WriterIntegration, WriterPlan, WriterPreparation,
    WriterRemovalState, WriterResourceState, WriterSeal, writer_path_set_sha256,
};
use dse_tools::{ProductionToolConfig, ProductionToolExecutor};

use crate::workspace::{
    CleanupComponentDisposition, CleanupResourceFacts, CleanupResourcePresence,
    ExactCleanupDisposition, GitWorkspaceError, GitWorkspaceOwner, OwnedWorktreeFacts, SealLimits,
    SealedWorktree, SealedWorktreeFacts, WriterWorkspaceRequest,
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

fn writer_cleanup_result(disposition: ExactCleanupDisposition) -> WriterCleanupResult {
    if disposition.worktree == CleanupComponentDisposition::AlreadyAbsent
        && disposition.branch == CleanupComponentDisposition::AlreadyAbsent
    {
        return WriterCleanupResult::AlreadyAbsent;
    }
    WriterCleanupResult::Removed {
        worktree: match disposition.worktree {
            CleanupComponentDisposition::Removed => WriterRemovalState::Removed,
            CleanupComponentDisposition::AlreadyAbsent => WriterRemovalState::AlreadyAbsent,
        },
        branch: match disposition.branch {
            CleanupComponentDisposition::Removed => WriterRemovalState::Removed,
            CleanupComponentDisposition::AlreadyAbsent => WriterRemovalState::AlreadyAbsent,
        },
    }
}

fn retained_cleanup_result(
    owner: &GitWorkspaceOwner,
    facts: &OwnedWorktreeFacts,
    before: Option<CleanupResourceFacts>,
    destructive_attempted: bool,
    expected_branch_commit: Option<&str>,
    uncertainty_code: String,
) -> WriterCleanupResult {
    let metadata = match expected_branch_commit {
        Some(expected) if !matches!(owner.cleanup_metadata_is_clear(facts, expected), Ok(true)) => {
            WriterCleanupMetadataState::Uncertain
        }
        Some(_) => WriterCleanupMetadataState::Clear,
        None => WriterCleanupMetadataState::Uncertain,
    };
    let Some(after) = owner.cleanup_resource_facts(facts).ok() else {
        return WriterCleanupResult::Retained {
            worktree: WriterResourceState::Unknown,
            branch: WriterResourceState::Unknown,
            metadata,
            uncertainty_code,
        };
    };
    if after.worktree == CleanupResourcePresence::Absent
        && after.branch == CleanupResourcePresence::Absent
    {
        if metadata == WriterCleanupMetadataState::Uncertain {
            return WriterCleanupResult::Retained {
                worktree: cleanup_resource_state(
                    before.map(|facts| facts.worktree),
                    after.worktree,
                    destructive_attempted,
                ),
                branch: cleanup_resource_state(
                    before.map(|facts| facts.branch),
                    after.branch,
                    destructive_attempted,
                ),
                metadata,
                uncertainty_code,
            };
        }
        if destructive_attempted {
            let worktree = cleanup_removal_state(before.map(|facts| facts.worktree));
            let branch = cleanup_removal_state(before.map(|facts| facts.branch));
            if worktree == WriterRemovalState::AlreadyAbsent
                && branch == WriterRemovalState::AlreadyAbsent
            {
                return WriterCleanupResult::AlreadyAbsent;
            }
            return WriterCleanupResult::Removed { worktree, branch };
        }
        return WriterCleanupResult::AlreadyAbsent;
    }
    WriterCleanupResult::Retained {
        worktree: cleanup_resource_state(
            before.map(|facts| facts.worktree),
            after.worktree,
            destructive_attempted,
        ),
        branch: cleanup_resource_state(
            before.map(|facts| facts.branch),
            after.branch,
            destructive_attempted,
        ),
        metadata,
        uncertainty_code,
    }
}

fn cleanup_resource_state(
    before: Option<CleanupResourcePresence>,
    after: CleanupResourcePresence,
    destructive_attempted: bool,
) -> WriterResourceState {
    match (before, after, destructive_attempted) {
        (_, CleanupResourcePresence::Present, _) => WriterResourceState::Retained,
        (Some(CleanupResourcePresence::Present), CleanupResourcePresence::Absent, true) => {
            WriterResourceState::Removed
        }
        (Some(CleanupResourcePresence::Absent), CleanupResourcePresence::Absent, _)
        | (_, CleanupResourcePresence::Absent, false) => WriterResourceState::AlreadyAbsent,
        (None, CleanupResourcePresence::Absent, true) => WriterResourceState::Unknown,
    }
}

fn cleanup_removal_state(before: Option<CleanupResourcePresence>) -> WriterRemovalState {
    if before == Some(CleanupResourcePresence::Present) {
        WriterRemovalState::Removed
    } else {
        WriterRemovalState::AlreadyAbsent
    }
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
        let owner = self.owner().map_err(map_git_error)?;
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
        .map_err(map_git_error)?;
        if self.assignment_for_facts(&allocation.durable_facts()) != task.workspace {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_binding_mismatch",
                "已创建或恢复的 worktree 与 AgentTask assignment 不一致",
            ));
        }
        let tools = self.writer_executor(allocation.path());
        let writer_workspace_state = match observe_workspace(tools.as_ref(), 0).await {
            Ok(state) => state,
            Err(cause) => return Err(cause),
        };
        Ok(WriterBinding {
            assignment: task.workspace.clone(),
            writer_workspace_state,
            tools,
        })
    }

    async fn seal_writer(&self, task: &AgentTask) -> Result<WriterSeal, AgentOrchestrationError> {
        let facts = self.owned_facts(task)?;
        let owner = self.owner().map_err(map_git_error)?;
        let sealed =
            tokio::task::spawn_blocking(move || match owner.recover_active(facts.clone()) {
                Ok(allocation) => match owner.seal(&allocation) {
                    Ok(sealed) => Ok(sealed),
                    Err(primary) => owner
                        .recover_prepared_seal(facts)
                        .map(|(_, sealed)| sealed)
                        .map_err(|_| primary),
                },
                Err(GitWorkspaceError::OwnershipMismatch(_) | GitWorkspaceError::Conflict(_)) => {
                    owner.recover_prepared_seal(facts).map(|(_, sealed)| sealed)
                }
                Err(error) => Err(error),
            })
            .await
            .map_err(join_error)?
            .map_err(map_git_error)?;
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

    async fn inspect_writer_cleanup(
        &self,
        task: &AgentTask,
        seal: Option<&WriterSeal>,
        phase: WriterCleanupPhase,
        reason_code: &str,
    ) -> Result<WriterCleanupPlan, AgentOrchestrationError> {
        let facts = self.owned_facts(task)?;
        let supplied_sealed_facts = seal.map(|seal| SealedWorktreeFacts {
            owner_id: facts.owner_id.clone(),
            base_commit: seal.base_commit.clone(),
            final_commit: seal.final_commit.clone(),
            diff_sha256: seal.diff_sha256.clone(),
        });
        let repository_root = self.repository_root.clone();
        let managed_root = self.managed_root.clone();
        let inspection_facts = facts.clone();
        let (first_owner, first, second_owner, second, effective_sealed_facts) =
            tokio::task::spawn_blocking(move || {
                let owner =
                    GitWorkspaceOwner::bind_existing_for_cleanup(repository_root, managed_root)?;
                let effective_sealed_facts = match supplied_sealed_facts {
                    Some(sealed) => Some(sealed),
                    None if phase == WriterCleanupPhase::Seal => owner
                        .recover_prepared_seal_for_cleanup(inspection_facts.clone())
                        .ok()
                        .map(|(_, sealed)| sealed.durable_facts()),
                    None => None,
                };
                let first_owner = owner.cleanup_owner_identity_sha256(&inspection_facts)?;
                let first = owner
                    .inspect_cleanup_paths(&inspection_facts, effective_sealed_facts.as_ref())?;
                let second = owner
                    .inspect_cleanup_paths(&inspection_facts, effective_sealed_facts.as_ref())?;
                let second_owner = owner.cleanup_owner_identity_sha256(&inspection_facts)?;
                Ok::<_, GitWorkspaceError>((
                    first_owner,
                    first,
                    second_owner,
                    second,
                    effective_sealed_facts,
                ))
            })
            .await
            .map_err(join_error)?
            .map_err(map_git_error)?;
        if first_owner != second_owner || first != second {
            return Err(orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_cleanup_scope_changed",
                "writer cleanup scope changed during Host inspection",
            ));
        }
        let paths = first
            .paths
            .iter()
            .map(|path| stable_path(path))
            .collect::<Vec<_>>();
        let in_scope_count = first
            .paths
            .iter()
            .filter(|path| {
                facts
                    .allowed_paths
                    .iter()
                    .any(|allowed| *path == allowed || path.starts_with(allowed))
            })
            .count() as u32;
        let changed_count = paths.len() as u32;
        let artifact_state = match &effective_sealed_facts {
            Some(sealed) => WriterArtifactState::KnownHostSealed {
                final_commit: sealed.final_commit.clone(),
                diff_sha256: sealed.diff_sha256.clone(),
            },
            None => WriterArtifactState::KnownUnsealed,
        };
        let expected_branch_commit = effective_sealed_facts
            .as_ref()
            .map(|seal| seal.final_commit.clone())
            .unwrap_or_else(|| facts.base_commit.clone());
        let mode = WriterCleanupMode::RemoveExact {
            expected_branch_commit,
        };
        let plan = WriterCleanupPlan {
            phase,
            reason_code: reason_code.to_owned(),
            ownership: WriterCleanupOwnership::Known {
                identity_sha256: first_owner,
            },
            artifact_state,
            scope: WriterCleanupScope::Known {
                workspace_revision: WorkspaceRevision::Known {
                    sha256: first.revision_sha256,
                },
                changed_count,
                in_scope_count,
                out_of_scope_count: changed_count.saturating_sub(in_scope_count),
                path_set_sha256: writer_path_set_sha256(&paths).map_err(|message| {
                    orchestration_error(
                        AgentOrchestrationErrorKind::RecoveryRequired,
                        "writer_cleanup_path_invalid",
                        message,
                    )
                })?,
            },
            mode,
        };
        plan.validate().map_err(|message| {
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_cleanup_plan_invalid",
                message,
            )
        })?;
        Ok(plan)
    }

    async fn execute_writer_cleanup(
        &self,
        task: &AgentTask,
        plan: &WriterCleanupPlan,
    ) -> Result<WriterCleanupResult, AgentOrchestrationError> {
        plan.validate().map_err(|message| {
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_cleanup_plan_invalid",
                message,
            )
        })?;
        let facts = self.owned_facts(task)?;
        let repository_root = self.repository_root.clone();
        let managed_root = self.managed_root.clone();
        let plan = plan.clone();
        tokio::task::spawn_blocking(move || {
            let owner = GitWorkspaceOwner::bind_existing_for_cleanup(repository_root, managed_root)
                .map_err(map_git_error)?;
            execute_writer_cleanup_sync(&owner, &facts, &plan)
        })
        .await
        .map_err(join_error)?
    }
}

fn execute_writer_cleanup_sync(
    owner: &GitWorkspaceOwner,
    facts: &OwnedWorktreeFacts,
    plan: &WriterCleanupPlan,
) -> Result<WriterCleanupResult, AgentOrchestrationError> {
    execute_writer_cleanup_sync_with(owner, facts, plan, GitWorkspaceOwner::cleanup_exact)
}

fn execute_writer_cleanup_sync_with<F>(
    owner: &GitWorkspaceOwner,
    facts: &OwnedWorktreeFacts,
    plan: &WriterCleanupPlan,
    cleanup_exact: F,
) -> Result<WriterCleanupResult, AgentOrchestrationError>
where
    F: Fn(
        &GitWorkspaceOwner,
        &OwnedWorktreeFacts,
        &str,
    ) -> std::result::Result<ExactCleanupDisposition, GitWorkspaceError>,
{
    if let WriterCleanupMode::RetainForRecovery { uncertainty_code } = &plan.mode {
        return Ok(retained_cleanup_result(
            owner,
            facts,
            owner.cleanup_resource_facts(facts).ok(),
            false,
            None,
            uncertainty_code.clone(),
        ));
    }
    let WriterCleanupOwnership::Known { identity_sha256 } = &plan.ownership else {
        return Ok(WriterCleanupResult::Retained {
            worktree: WriterResourceState::Unknown,
            branch: WriterResourceState::Unknown,
            metadata: WriterCleanupMetadataState::Uncertain,
            uncertainty_code: "writer_cleanup_owner_unknown".to_owned(),
        });
    };
    let current_owner = match owner.cleanup_owner_identity_sha256(facts) {
        Ok(current) if &current == identity_sha256 => current,
        Ok(_) => {
            return Ok(WriterCleanupResult::Retained {
                worktree: WriterResourceState::Unknown,
                branch: WriterResourceState::Unknown,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code: "writer_cleanup_owner_changed".to_owned(),
            });
        }
        Err(_) => {
            return Ok(WriterCleanupResult::Retained {
                worktree: WriterResourceState::Unknown,
                branch: WriterResourceState::Unknown,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code: "writer_cleanup_owner_unprovable".to_owned(),
            });
        }
    };
    debug_assert_eq!(current_owner, *identity_sha256);
    let sealed = match &plan.artifact_state {
        WriterArtifactState::KnownUnsealed => None,
        WriterArtifactState::KnownHostSealed {
            final_commit,
            diff_sha256,
        } => Some(SealedWorktreeFacts {
            owner_id: facts.owner_id.clone(),
            base_commit: facts.base_commit.clone(),
            final_commit: final_commit.clone(),
            diff_sha256: diff_sha256.clone(),
        }),
        WriterArtifactState::Unknown => {
            return Ok(retained_cleanup_result(
                owner,
                facts,
                owner.cleanup_resource_facts(facts).ok(),
                false,
                None,
                "writer_cleanup_artifact_unknown".to_owned(),
            ));
        }
    };
    let expected = match &plan.mode {
        WriterCleanupMode::RemoveExact {
            expected_branch_commit,
        } => expected_branch_commit.as_str(),
        WriterCleanupMode::RetainForRecovery { .. } => unreachable!("handled above"),
    };
    let before = match owner.cleanup_resource_facts(facts) {
        Ok(before) => before,
        Err(_) => {
            return Ok(retained_cleanup_result(
                owner,
                facts,
                None,
                false,
                Some(expected),
                "writer_cleanup_resources_unprovable".to_owned(),
            ));
        }
    };
    let cleanup_started = match owner.cleanup_has_started(facts, expected) {
        Ok(started) => started,
        Err(error) => {
            return Ok(retained_cleanup_result(
                owner,
                facts,
                Some(before),
                false,
                Some(expected),
                git_error_code(&error).to_owned(),
            ));
        }
    };
    if cleanup_started || before.path == CleanupResourcePresence::Absent {
        return match cleanup_exact(owner, facts, expected) {
            Ok(disposition) => Ok(writer_cleanup_result(disposition)),
            Err(error) => Ok(retained_cleanup_result(
                owner,
                facts,
                Some(before),
                true,
                Some(expected),
                git_error_code(&error).to_owned(),
            )),
        };
    }
    let current = match owner.inspect_cleanup_paths(facts, sealed.as_ref()) {
        Ok(current) => current,
        Err(error) => {
            return Ok(retained_cleanup_result(
                owner,
                facts,
                Some(before),
                false,
                Some(expected),
                git_error_code(&error).to_owned(),
            ));
        }
    };
    let current_paths = current
        .paths
        .iter()
        .map(|path| stable_path(path))
        .collect::<Vec<_>>();
    let current_in_scope = current
        .paths
        .iter()
        .filter(|path| {
            facts
                .allowed_paths
                .iter()
                .any(|allowed| *path == allowed || path.starts_with(allowed))
        })
        .count() as u32;
    let current_scope = WriterCleanupScope::Known {
        workspace_revision: WorkspaceRevision::Known {
            sha256: current.revision_sha256,
        },
        changed_count: current_paths.len() as u32,
        in_scope_count: current_in_scope,
        out_of_scope_count: (current_paths.len() as u32).saturating_sub(current_in_scope),
        path_set_sha256: writer_path_set_sha256(&current_paths).map_err(|message| {
            orchestration_error(
                AgentOrchestrationErrorKind::RecoveryRequired,
                "writer_cleanup_path_invalid",
                message,
            )
        })?,
    };
    if current_scope != plan.scope {
        return Ok(retained_cleanup_result(
            owner,
            facts,
            Some(before),
            false,
            Some(expected),
            "writer_cleanup_scope_changed".to_owned(),
        ));
    }
    match cleanup_exact(owner, facts, expected) {
        Ok(disposition) => Ok(writer_cleanup_result(disposition)),
        Err(error) => Ok(retained_cleanup_result(
            owner,
            facts,
            Some(before),
            true,
            Some(expected),
            git_error_code(&error).to_owned(),
        )),
    }
}

async fn writer_seal(
    tools: Arc<ProductionToolExecutor>,
    sealed: SealedWorktree,
) -> Result<WriterSeal, AgentOrchestrationError> {
    let writer_workspace_state = match observe_workspace(tools.as_ref(), 0).await {
        Ok(state) => state,
        Err(_) => observe_workspace(tools.as_ref(), 0).await?,
    };
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
            "无法确认 Writer 工作区 revision",
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

fn join_error(_error: tokio::task::JoinError) -> AgentOrchestrationError {
    orchestration_error(
        AgentOrchestrationErrorKind::RecoveryRequired,
        "writer_git_task_join_failed",
        "Writer Git 后台任务异常终止",
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
    let code = git_error_code(&error);
    orchestration_error(kind, code, git_error_public_message(code))
}

fn git_error_code(error: &GitWorkspaceError) -> &'static str {
    match error {
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
    }
}

fn git_error_public_message(code: &str) -> &'static str {
    match code {
        "writer_repository_invalid" => "当前目录不是可用的 Git 仓库",
        "writer_repository_dirty" => "当前操作要求根工作区保持干净",
        "writer_repository_unsupported" => "当前 Git 仓库结构不受 Writer 支持",
        "writer_request_invalid" => "Writer 请求不满足 Host 约束",
        "writer_ownership_mismatch" => "Writer 资源身份无法被 Host 精确证明",
        "writer_workspace_conflict" => "Writer 或根工作区状态已经变化",
        "writer_limit_exceeded" => "Writer 变更超过 Host 冻结限制",
        "writer_empty_diff" => "Writer 没有形成可集成的代码变更",
        "writer_git_failed" => "Writer Git 操作失败",
        "writer_io_failed" => "Writer 文件系统操作失败",
        _ => "Writer 工作区操作失败",
    }
}

fn map_integration_git_error(error: GitWorkspaceError) -> AgentOrchestrationError {
    let root_conflict = matches!(
        &error,
        GitWorkspaceError::DirtyRepository(_)
            | GitWorkspaceError::Conflict(_)
            | GitWorkspaceError::InvalidRepository(_)
            | GitWorkspaceError::UnsupportedRepository(_)
    );
    let mut mapped = map_git_error(error);
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

    use dse_runtime::{
        AgentTaskId, ContextPolicy, ModelRouteAudit, ModelRouteProfile, ReasoningEffort, RunId,
        RunLimits, TaskContract, TaskDefinition, TaskGenerationId, ToolPolicy,
    };
    use dse_tools::shell::ShellPolicy;
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
    async fn binding_cleanup_discards_exact_pristine_and_dirty_resources() {
        let pristine_fixture = RepositoryFixture::new();
        let pristine = ProductionAgentOrchestrator::new(
            &pristine_fixture.root,
            &pristine_fixture.managed,
            ProductionToolConfig::new(&pristine_fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("pristine orchestrator");
        let pristine_plan = pristine
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_bind_pristine"),
                root_workspace: stable_path(&pristine_fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("pristine plan");
        let pristine_task = writer_task(pristine_plan.assignment);
        pristine
            .bind_writer(&pristine_task, None)
            .await
            .expect("pristine allocation");
        let cleanup_plan = pristine
            .inspect_writer_cleanup(
                &pristine_task,
                None,
                WriterCleanupPhase::Binding,
                "synthetic_observation_failure",
            )
            .await
            .expect("freeze pristine cleanup");
        assert!(matches!(
            pristine
                .execute_writer_cleanup(&pristine_task, &cleanup_plan)
                .await
                .expect("discard pristine Writer"),
            WriterCleanupResult::Removed { .. }
        ));
        assert!(
            !pristine_fixture
                .managed
                .join("writer_bind_pristine")
                .exists()
        );

        let dirty_fixture = RepositoryFixture::new();
        let dirty = ProductionAgentOrchestrator::new(
            &dirty_fixture.root,
            &dirty_fixture.managed,
            ProductionToolConfig::new(&dirty_fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("dirty orchestrator");
        let dirty_plan = dirty
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_bind_dirty"),
                root_workspace: stable_path(&dirty_fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("dirty plan");
        let dirty_task = writer_task(dirty_plan.assignment);
        dirty
            .bind_writer(&dirty_task, None)
            .await
            .expect("dirty allocation");
        fs::write(
            dirty_fixture.managed.join("writer_bind_dirty/src/lib.rs"),
            "pub fn value() -> u8 { 99 }\n",
        )
        .expect("dirty writer");
        let cleanup_plan = dirty
            .inspect_writer_cleanup(
                &dirty_task,
                None,
                WriterCleanupPhase::Binding,
                "synthetic_observation_failure",
            )
            .await
            .expect("freeze dirty cleanup");
        assert!(matches!(
            dirty
                .execute_writer_cleanup(&dirty_task, &cleanup_plan)
                .await
                .expect("discard dirty Writer"),
            WriterCleanupResult::Removed { .. }
        ));
        assert!(!dirty_fixture.managed.join("writer_bind_dirty").exists());
    }

    #[tokio::test]
    async fn frozen_cleanup_plan_removes_exact_git_residue_after_path_disappears() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_missing_path_plan"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(plan.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        let cleanup_plan = orchestrator
            .inspect_writer_cleanup(
                &task,
                None,
                WriterCleanupPhase::Binding,
                "writer_binding_failed",
            )
            .await
            .expect("freeze cleanup plan while the Writer path is inspectable");
        assert!(matches!(
            cleanup_plan.scope,
            WriterCleanupScope::Known {
                changed_count: 0,
                in_scope_count: 0,
                out_of_scope_count: 0,
                ..
            }
        ));
        assert_eq!(
            cleanup_plan.mode,
            WriterCleanupMode::RemoveExact {
                expected_branch_commit: task.workspace.base_commit.clone(),
            }
        );
        fs::remove_dir_all(Path::new(task.workspace.execution_workspace()))
            .expect("simulate path disappearance after the cleanup plan was frozen");
        assert!(!Path::new(task.workspace.execution_workspace()).exists());
        assert_eq!(
            orchestrator
                .execute_writer_cleanup(&task, &cleanup_plan)
                .await
                .expect("remove exact missing-path residue"),
            WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            }
        );
        assert_eq!(
            orchestrator
                .execute_writer_cleanup(&task, &cleanup_plan)
                .await
                .expect("repeat exact missing-path cleanup"),
            WriterCleanupResult::AlreadyAbsent
        );
        assert!(
            resolve_ref_optional_for_test(
                &fixture.root,
                task.workspace.branch.as_deref().expect("writer branch"),
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn frozen_cleanup_plan_resumes_a_branch_only_guard_without_scope_reinspection() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_branch_guard_resume"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(plan.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        let cleanup_plan = orchestrator
            .inspect_writer_cleanup(
                &task,
                None,
                WriterCleanupPhase::Binding,
                "writer_binding_failed",
            )
            .await
            .expect("freeze cleanup plan before the crash window");
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
        let facts = orchestrator.owned_facts(&task).expect("owned Writer facts");
        let owner = orchestrator.owner().expect("Git owner");
        owner
            .simulate_branch_only_cleanup_guard_crash(&facts, &task.workspace.base_commit)
            .expect("simulate crash after the synthetic guard was persisted");
        fs::write(
            Path::new(task.workspace.execution_workspace()).join("outside-frozen-scope.txt"),
            "must be discarded without re-inspecting the frozen scope\n",
        )
        .expect("drift the synthetic guard after cleanup intent");
        assert!(
            Path::new(task.workspace.execution_workspace()).exists(),
            "the synthetic no-checkout guard must exercise the path-present recovery branch"
        );
        assert!(
            owner
                .cleanup_has_started(&facts, &task.workspace.base_commit)
                .expect("prove cleanup intent"),
            "the exact tombstone must bypass ordinary scope reinspection"
        );

        assert_eq!(
            orchestrator
                .execute_writer_cleanup(&task, &cleanup_plan)
                .await
                .expect("resume the frozen cleanup action"),
            WriterCleanupResult::Removed {
                worktree: WriterRemovalState::Removed,
                branch: WriterRemovalState::Removed,
            }
        );
        assert!(!Path::new(task.workspace.execution_workspace()).exists());
        assert!(
            resolve_ref_optional_for_test(
                &fixture.root,
                task.workspace.branch.as_deref().expect("Writer branch"),
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn cleanup_metadata_failure_is_retained_until_production_recovery_finishes() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let writer = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_cleanup_metadata_recovery"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(writer.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        let plan = orchestrator
            .inspect_writer_cleanup(
                &task,
                None,
                WriterCleanupPhase::Binding,
                "writer_binding_failed",
            )
            .await
            .expect("freeze cleanup plan");
        let WriterCleanupMode::RemoveExact {
            expected_branch_commit,
        } = &plan.mode
        else {
            panic!("exact cleanup plan")
        };
        let facts = orchestrator.owned_facts(&task).expect("owned Writer facts");
        let owner = orchestrator.owner().expect("Git owner");

        let retained = execute_writer_cleanup_sync_with(
            &owner,
            &facts,
            &plan,
            GitWorkspaceOwner::cleanup_exact_with_failed_root_lock_release,
        )
        .expect("cleanup execution failure is a typed result");
        assert_eq!(
            retained,
            WriterCleanupResult::Retained {
                worktree: WriterResourceState::Removed,
                branch: WriterResourceState::Removed,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code: "writer_io_failed".to_owned(),
            }
        );
        assert!(
            !owner
                .cleanup_metadata_is_clear(&facts, expected_branch_commit)
                .expect("inspect retained cleanup metadata"),
            "business resources being absent must not hide retained cleanup metadata"
        );

        assert_eq!(
            execute_writer_cleanup_sync(&owner, &facts, &plan)
                .expect("resume the exact production cleanup"),
            WriterCleanupResult::AlreadyAbsent
        );
        assert!(
            owner
                .cleanup_metadata_is_clear(&facts, expected_branch_commit)
                .expect("prove final cleanup metadata removal")
        );

        let uncertainty_code = "writer_cleanup_identity_unprovable".to_owned();
        let retain_plan = WriterCleanupPlan {
            phase: WriterCleanupPhase::Child,
            reason_code: "writer_child_recovery_required".to_owned(),
            ownership: WriterCleanupOwnership::Unknown {
                uncertainty_code: uncertainty_code.clone(),
            },
            artifact_state: WriterArtifactState::Unknown,
            scope: WriterCleanupScope::Unknown {
                uncertainty_code: uncertainty_code.clone(),
            },
            mode: WriterCleanupMode::RetainForRecovery {
                uncertainty_code: uncertainty_code.clone(),
            },
        };
        assert_eq!(
            execute_writer_cleanup_sync(&owner, &facts, &retain_plan)
                .expect("retain-only production cleanup result"),
            WriterCleanupResult::Retained {
                worktree: WriterResourceState::AlreadyAbsent,
                branch: WriterResourceState::AlreadyAbsent,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code,
            },
            "missing exact cleanup authority can never prove metadata clear"
        );
    }

    #[tokio::test]
    async fn frozen_cleanup_scope_drift_retains_every_writer_resource() {
        let fixture = RepositoryFixture::new();
        fs::write(fixture.root.join(".gitignore"), "src/ignored.bin\n").expect("write ignore rule");
        git_ok(
            &fixture.root,
            &[OsString::from("add"), OsString::from(".gitignore")],
        );
        git_ok(
            &fixture.root,
            &[
                OsString::from("commit"),
                OsString::from("-m"),
                OsString::from("ignore writer fixture"),
            ],
        );
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_scope_drift"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(plan.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        let writer = Path::new(task.workspace.execution_workspace());
        fs::write(writer.join("src/lib.rs"), "pub fn value() -> u8 { 2 }\n")
            .expect("tracked Writer edit");
        fs::write(writer.join("src/ignored.bin"), b"first ignored bytes")
            .expect("ignored Writer artifact");
        let cleanup_plan = orchestrator
            .inspect_writer_cleanup(
                &task,
                None,
                WriterCleanupPhase::Child,
                "writer_child_failed",
            )
            .await
            .expect("freeze dirty Writer cleanup scope");

        let changed_tracked = b"pub fn value() -> u8 { 200 }\n";
        let changed_ignored = b"changed ignored bytes";
        fs::write(writer.join("src/lib.rs"), changed_tracked).expect("drift tracked bytes");
        fs::write(writer.join("src/ignored.bin"), changed_ignored).expect("drift ignored bytes");
        let branch_before = resolve_ref_optional_for_test(
            &fixture.root,
            task.workspace.branch.as_deref().expect("writer branch"),
        );

        let result = orchestrator
            .execute_writer_cleanup(&task, &cleanup_plan)
            .await
            .expect("scope drift is a typed retained result");
        assert!(matches!(
            result,
            WriterCleanupResult::Retained {
                ref uncertainty_code,
                ..
            } if uncertainty_code == "writer_cleanup_scope_changed"
        ));
        assert!(writer.exists(), "scope drift must retain the worktree");
        assert_eq!(
            fs::read(writer.join("src/lib.rs")).unwrap(),
            changed_tracked
        );
        assert_eq!(
            fs::read(writer.join("src/ignored.bin")).unwrap(),
            changed_ignored
        );
        assert_eq!(
            resolve_ref_optional_for_test(
                &fixture.root,
                task.workspace.branch.as_deref().expect("writer branch"),
            ),
            branch_before,
            "scope drift must not delete or move the Writer branch"
        );
    }

    #[tokio::test]
    async fn seal_phase_cleanup_recovers_an_uncommitted_host_seal_identity() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_seal_cleanup"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(plan.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        fs::write(
            Path::new(task.workspace.execution_workspace()).join("src/lib.rs"),
            "pub fn value() -> u8 { 7 }\n",
        )
        .expect("writer edit");
        let facts = orchestrator.owned_facts(&task).expect("owned facts");
        let owner = orchestrator.owner().expect("Git owner");
        let allocation = owner.recover_active(facts).expect("active allocation");
        let sealed = owner
            .seal(&allocation)
            .expect("publish Host seal side effect");

        git_ok(
            &fixture.root,
            &[
                OsString::from("switch"),
                OsString::from("-c"),
                OsString::from("seal-cleanup-other"),
            ],
        );
        fs::write(fixture.root.join("root-advanced.txt"), "advanced root\n")
            .expect("advance root bytes");
        git_ok(
            &fixture.root,
            &[OsString::from("add"), OsString::from("root-advanced.txt")],
        );
        git_ok(
            &fixture.root,
            &[
                OsString::from("commit"),
                OsString::from("-m"),
                OsString::from("advance switched root"),
            ],
        );
        fs::write(fixture.root.join("root-dirty.txt"), "dirty root\n").expect("dirty root bytes");
        let root_head_before = git_text(&fixture.root, &["rev-parse", "HEAD"]);
        let root_branch_before = git_text(&fixture.root, &["symbolic-ref", "HEAD"]);
        let root_dirty_before = fs::read(fixture.root.join("root-dirty.txt"))
            .expect("read dirty root bytes before cleanup");

        let cleanup_plan = orchestrator
            .inspect_writer_cleanup(&task, None, WriterCleanupPhase::Seal, "writer_seal_failed")
            .await
            .expect("recover Host seal into cleanup plan");
        assert_eq!(
            cleanup_plan.artifact_state,
            WriterArtifactState::KnownHostSealed {
                final_commit: sealed.final_commit().to_owned(),
                diff_sha256: sealed.diff_sha256().to_owned(),
            }
        );
        fs::remove_dir_all(Path::new(task.workspace.execution_workspace()))
            .expect("simulate path disappearance after plan commit");
        assert_eq!(
            cleanup_plan.mode,
            WriterCleanupMode::RemoveExact {
                expected_branch_commit: sealed.final_commit().to_owned(),
            }
        );
        assert!(matches!(
            orchestrator
                .execute_writer_cleanup(&task, &cleanup_plan)
                .await
                .expect("remove recovered sealed Writer"),
            WriterCleanupResult::Removed { .. }
        ));
        assert_eq!(
            git_text(&fixture.root, &["rev-parse", "HEAD"]),
            root_head_before,
            "cleanup must not move the advanced root HEAD"
        );
        assert_eq!(
            git_text(&fixture.root, &["symbolic-ref", "HEAD"]),
            root_branch_before,
            "cleanup must not switch the root branch"
        );
        assert_eq!(
            fs::read(fixture.root.join("root-dirty.txt"))
                .expect("read dirty root bytes after cleanup"),
            root_dirty_before,
            "cleanup must not alter dirty root bytes"
        );
    }

    #[tokio::test]
    async fn missing_writer_path_before_cleanup_plan_is_not_destructively_guessed() {
        let fixture = RepositoryFixture::new();
        let orchestrator = ProductionAgentOrchestrator::new(
            &fixture.root,
            &fixture.managed,
            ProductionToolConfig::new(&fixture.root).with_shell_policy(ShellPolicy::Full),
        )
        .expect("production orchestrator");
        let plan = orchestrator
            .prepare_writer(WriterPreparation {
                task_id: AgentTaskId::from("writer_missing_before_plan"),
                root_workspace: stable_path(&fixture.root),
                allowed_paths: vec!["src".to_owned()],
            })
            .await
            .expect("writer plan");
        let task = writer_task(plan.assignment);
        orchestrator
            .bind_writer(&task, None)
            .await
            .expect("writer allocation");
        fs::remove_dir_all(Path::new(task.workspace.execution_workspace()))
            .expect("simulate path disappearance before inspection");

        let error = orchestrator
            .inspect_writer_cleanup(
                &task,
                None,
                WriterCleanupPhase::Binding,
                "writer_binding_failed",
            )
            .await
            .expect_err("Host cannot invent an empty scope after the path vanished");
        assert_eq!(error.kind, AgentOrchestrationErrorKind::Conflict);
        assert_eq!(error.code, "writer_workspace_conflict");
        assert!(
            resolve_ref_optional_for_test(
                &fixture.root,
                task.workspace.branch.as_deref().expect("writer branch"),
            )
            .is_some(),
            "inspection failure must retain the exact Writer ref"
        );
    }

    #[tokio::test]
    async fn join_failure_is_mapped_without_exposing_the_panic_payload() {
        let join = tokio::task::spawn_blocking(|| panic!("secret-path-and-git-stderr"))
            .await
            .expect_err("test task must panic");
        let error = join_error(join);
        assert_eq!(error.code, "writer_git_task_join_failed");
        assert_eq!(error.message, "Writer Git 后台任务异常终止");
        assert!(!error.message.contains("secret-path-and-git-stderr"));
    }

    #[test]
    fn git_failure_is_mapped_without_exposing_paths_stderr_or_file_bytes() {
        let error = map_git_error(GitWorkspaceError::Git {
            operation: "seal /secret/worktree/TOP_SECRET_BYTES".to_owned(),
            status: Some(128),
            detail: "fatal: private git stderr TOP_SECRET_BYTES".to_owned(),
        });
        assert_eq!(error.code, "writer_git_failed");
        assert_eq!(error.message, "Writer Git 操作失败");
        for secret in ["/secret/worktree", "private git stderr", "TOP_SECRET_BYTES"] {
            assert!(!error.message.contains(secret));
        }
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

        let cleanup_plan = orchestrator
            .inspect_writer_cleanup(
                &task,
                Some(&seal),
                WriterCleanupPhase::PostIntegration,
                "writer_integrated",
            )
            .await
            .expect("freeze exact cleanup plan");

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
            .execute_writer_cleanup(&task, &cleanup_plan)
            .await
            .expect("resume branch-only cleanup");
        assert!(matches!(
            cleanup,
            WriterCleanupResult::Removed {
                worktree: WriterRemovalState::AlreadyAbsent,
                branch: WriterRemovalState::Removed,
            }
        ));
        let repeated_cleanup = orchestrator
            .execute_writer_cleanup(&task, &cleanup_plan)
            .await
            .expect("idempotent cleanup");
        assert_eq!(repeated_cleanup, WriterCleanupResult::AlreadyAbsent);

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
            model: "deepseek-v4-pro".to_owned(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(262_144),
            context_policy: ContextPolicy {
                hard_input_tokens: 90_000,
            },
            route: ModelRouteAudit {
                profile: ModelRouteProfile::FixedActor,
                policy_version: "deepseek_fixed_actor_v1".to_owned(),
                reason_code: "fixed_isolated_writer".to_owned(),
            },
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
                    OsString::from("DSE Test"),
                ],
            );
            git_ok(
                &root,
                &[
                    OsString::from("config"),
                    OsString::from("user.email"),
                    OsString::from("test@dse.local"),
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
