//! Goal tools for the model-visible LLM-as-judge loop.
//!
//! The TUI already has a `/goal` command and passes its objective into the
//! engine prompt. This module keeps the runtime slice separate: a small
//! session-scoped state object plus tools the model can use to inspect and
//! close out that state.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use codewhale_protocol::agent_runtime::{
    ToolArtifact, ToolArtifactStatus, ToolEvidence, ToolEvidenceStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wait_timeout::ChildExt;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec,
    required_str,
};
use codewhale_tools::shell::{ProcessTreeOwner, configure_process_tree};

/// Maximum number of automatic goal-continuation prompt injections in one
/// engine turn. This is intra-turn granularity only — it prevents a stuck spin
/// within a single turn from making no progress. The cross-turn loop has **no
/// cap**: a goal runs until complete/blocked/paused, or an optional budget is
/// exhausted. See `goal_loop::decide_continuation`.
pub const MAX_GOAL_CONTINUATIONS_PER_TURN: u32 = 3;

const HOST_VERIFICATION_METADATA_KEY: &str = "goal_host_verification";
const HOST_VERIFICATION_REJECTION_METADATA_KEY: &str = "goal_host_verification_rejected";
// M5 deletion point: remove this metadata mirror after the TUI Goal consumer
// reads canonical ToolOutcome evidence/artifact fields from AgentRuntime.
const GOAL_EVIDENCE_ARTIFACT_METADATA_KEY: &str = "goal_evidence_artifact";
const GOAL_EVIDENCE_ARTIFACT_ID_PREFIX: &str = "goal-evidence:";
const GOAL_EVIDENCE_ARTIFACT_MEDIA_TYPE: &str = "application/json";
const TASK_CONTRACT_VERSION: u32 = 1;
const DEFAULT_GOAL_VERIFIER_ID: &str = "run_verifiers";
const MAX_CONTRACT_SCOPE_ITEMS: usize = 64;
const MAX_CONTRACT_SCOPE_ITEM_CHARS: usize = 4_000;
const MAX_CONTRACT_CUSTOM_GATES: usize = 12;
const MAX_GIT_DIFF_BYTES: usize = 256 * 1024 * 1024;
const MAX_GIT_PATH_LIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_GIT_STDERR_BYTES: usize = 64 * 1024;
const MAX_UNTRACKED_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_UNTRACKED_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared reference to the current runtime goal.
pub type SharedGoalState = Arc<Mutex<GoalState>>;

/// Create an empty shared goal state.
#[must_use]
pub fn new_shared_goal_state() -> SharedGoalState {
    Arc::new(Mutex::new(GoalState::default()))
}

/// Create shared state seeded from the host goal surface with an explicit status.
#[must_use]
pub fn new_shared_goal_state_from_host_status(
    objective: Option<String>,
    token_budget: Option<u32>,
    status: GoalStatus,
) -> SharedGoalState {
    let mut state = GoalState::default();
    state.sync_from_host_status(objective.as_deref(), token_budget, status);
    Arc::new(Mutex::new(state))
}

/// Runtime status for a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    Blocked,
}

impl GoalStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

/// Session-local goal state. `Instant` stays runtime-only; snapshots expose
/// elapsed seconds so tool output remains serializable and stable.
#[derive(Debug, Clone, Default)]
pub struct GoalState {
    /// Monotonic identity for one concrete objective. Status changes retain
    /// the generation; create/replace/clear advance it so late turn usage can
    /// never leak into a different Goal.
    generation: u64,
    objective: Option<String>,
    token_budget: Option<u32>,
    status: Option<GoalStatus>,
    tokens_used: u64,
    time_used_seconds: u64,
    continuation_count: u32,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    evidence: Option<String>,
    blocker: Option<String>,
    completion_verification: Option<GoalCompletionVerification>,
    /// Immutable acceptance contract for the current generation. This lives
    /// in the Goal state so completion does not gain a second source of truth.
    task_contract: Option<TaskContract>,
    host_verification: Option<GoalHostVerificationReceipt>,
}

impl GoalState {
    fn replace_generation(
        &mut self,
        objective: String,
        token_budget: Option<u32>,
        status: GoalStatus,
        constraints: Vec<String>,
        non_goals: Vec<String>,
        acceptance: TaskAcceptance,
    ) {
        let now = Instant::now();
        self.generation = self.generation.saturating_add(1);
        let task_contract = TaskContract::derived(
            self.generation,
            objective.clone(),
            constraints,
            non_goals,
            acceptance,
        );
        self.objective = Some(objective);
        self.token_budget = token_budget;
        self.status = Some(status);
        self.tokens_used = 0;
        self.time_used_seconds = 0;
        self.continuation_count = 0;
        self.started_at = Some(now);
        self.finished_at = (status != GoalStatus::Active).then_some(now);
        self.evidence = None;
        self.blocker = None;
        self.completion_verification = None;
        self.task_contract = Some(task_contract);
        self.host_verification = None;
    }

    #[must_use]
    pub fn objective(&self) -> Option<&str> {
        self.objective.as_deref()
    }

    #[must_use]
    pub fn token_budget(&self) -> Option<u32> {
        self.token_budget
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == Some(GoalStatus::Active)
    }

    #[must_use]
    pub fn active_generation(&self) -> Option<u64> {
        self.is_active().then_some(self.generation)
    }

    /// Return the immutable acceptance contract for the active generation.
    #[must_use]
    pub fn active_task_contract(&self) -> Option<&TaskContract> {
        self.is_active()
            .then_some(self.task_contract.as_ref())
            .flatten()
    }

    pub fn sync_from_host_status(
        &mut self,
        objective: Option<&str>,
        token_budget: Option<u32>,
        status: GoalStatus,
    ) {
        let objective = objective.map(str::trim).filter(|value| !value.is_empty());
        match objective {
            Some(objective) => {
                let changed = self.objective.as_deref() != Some(objective);
                // A host-controlled resume from a terminal state starts a new
                // execution lifetime even when the text happens to be the
                // same. Otherwise late usage and a verifier receipt from the
                // completed/blocked run could be accepted by the resumed run.
                // Pause -> Active remains the same generation by design.
                let terminal_reactivation = status == GoalStatus::Active
                    && matches!(
                        self.status,
                        Some(GoalStatus::Complete | GoalStatus::Blocked)
                    );
                let status_changed = self.status != Some(status);
                if changed || terminal_reactivation {
                    self.replace_generation(
                        objective.to_string(),
                        token_budget,
                        status,
                        Vec::new(),
                        Vec::new(),
                        TaskAcceptance::HostAcceptanceRequired,
                    );
                } else if self.token_budget != token_budget {
                    self.token_budget = token_budget;
                }

                if !changed && !terminal_reactivation && (status_changed || self.status.is_none()) {
                    self.status = Some(status);
                    self.finished_at = if status == GoalStatus::Active {
                        None
                    } else {
                        Some(Instant::now())
                    };
                    if status != GoalStatus::Active {
                        // A host pause, block, limit, or completion outranks an
                        // in-flight model completion candidate. Never let a
                        // stale verifier receipt resume or overwrite it.
                        self.host_verification = None;
                    }
                }
            }
            None => self.clear(),
        }
    }

    #[cfg(test)]
    pub fn create(&mut self, objective: String, token_budget: Option<u32>) {
        self.replace_generation(
            objective,
            token_budget,
            GoalStatus::Active,
            Vec::new(),
            Vec::new(),
            TaskAcceptance::HostAcceptanceRequired,
        );
    }

    pub(crate) fn create_with_contract(
        &mut self,
        objective: String,
        token_budget: Option<u32>,
        constraints: Vec<String>,
        non_goals: Vec<String>,
        verifier_params: Value,
    ) {
        self.replace_generation(
            objective,
            token_budget,
            GoalStatus::Active,
            constraints,
            non_goals,
            TaskAcceptance::run_verifiers(verifier_params),
        );
    }

    #[cfg(test)]
    pub fn record_usage(&mut self, token_delta: u64, time_delta_seconds: u64) {
        // A model can call `update_goal(complete)` before the enclosing turn
        // returns. The provider usage belongs to that Goal even though its
        // status already changed; only a cleared/non-existent Goal must ignore
        // the late turn accounting.
        if self.objective.is_some() {
            self.tokens_used = self.tokens_used.saturating_add(token_delta);
            self.time_used_seconds = self.time_used_seconds.saturating_add(time_delta_seconds);
        }
    }

    /// Charge provider usage only to the objective that was active when the
    /// enclosing turn began. A Goal may become complete/blocked during that
    /// same turn without changing generation, so its final usage is retained.
    pub fn record_usage_for_generation(
        &mut self,
        generation: u64,
        token_delta: u64,
        time_delta_seconds: u64,
    ) -> bool {
        if self.objective.is_none() || self.generation != generation {
            return false;
        }
        self.tokens_used = self.tokens_used.saturating_add(token_delta);
        self.time_used_seconds = self.time_used_seconds.saturating_add(time_delta_seconds);
        true
    }

    pub fn record_continuation(&mut self) {
        if self.is_active() {
            self.continuation_count = self.continuation_count.saturating_add(1);
        }
    }

    pub fn mark_complete(
        &mut self,
        evidence: String,
        host_verification: GoalHostVerificationReceipt,
    ) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to complete.");
        }
        if self.status != Some(GoalStatus::Active) {
            return Err(
                "Goal is not active; host-controlled terminal states cannot be overridden.",
            );
        }
        if !self.receipt_matches_active_contract(&host_verification) {
            return Err(
                "Verifier receipt does not match the active Goal task contract and generation.",
            );
        }
        let verification = GoalCompletionVerification {
            status: "passed".to_string(),
            check: host_verification.check.clone(),
            summary: host_verification.summary.clone(),
        };
        self.status = Some(GoalStatus::Complete);
        self.finished_at = Some(Instant::now());
        self.evidence = Some(evidence);
        self.blocker = None;
        self.completion_verification = Some(verification);
        self.host_verification = Some(host_verification);
        Ok(())
    }

    /// Replace the completion receipt with evidence observed by the host after
    /// a foreground verifier finished. Model-authored text never enters this
    /// slot.
    pub fn record_host_verification(&mut self, receipt: GoalHostVerificationReceipt) -> bool {
        if !self.receipt_matches_active_contract(&receipt) {
            return false;
        }
        self.host_verification = Some(receipt);
        true
    }

    pub fn clear_host_verification(&mut self) {
        if self.is_active() {
            self.host_verification = None;
        }
    }

    /// Return a model-completed Goal to active when the terminal host gate
    /// proves that its verifier receipt no longer matches the final workspace.
    pub fn reopen_after_stale_verification(&mut self) {
        if self.status != Some(GoalStatus::Complete) || self.host_verification.is_none() {
            return;
        }
        self.status = Some(GoalStatus::Active);
        self.finished_at = None;
        self.evidence = None;
        self.blocker = None;
        self.completion_verification = None;
        self.host_verification = None;
    }

    #[must_use]
    pub fn host_verification(&self) -> Option<&GoalHostVerificationReceipt> {
        self.host_verification.as_ref()
    }

    #[must_use]
    pub fn receipt_matches_active_contract(&self, receipt: &GoalHostVerificationReceipt) -> bool {
        self.is_active()
            && self
                .task_contract
                .as_ref()
                .is_some_and(|contract| receipt.matches_contract(contract))
    }

    pub fn mark_blocked(&mut self, blocker: String) -> Result<(), &'static str> {
        if self.objective.is_none() {
            return Err("No active goal exists to block.");
        }
        if self.status != Some(GoalStatus::Active) {
            return Err(
                "Goal is not active; host-controlled terminal states cannot be overridden.",
            );
        }
        self.status = Some(GoalStatus::Blocked);
        self.finished_at = Some(Instant::now());
        self.blocker = Some(blocker);
        self.evidence = None;
        self.completion_verification = None;
        self.host_verification = None;
        Ok(())
    }

    pub fn clear(&mut self) {
        let next_generation = self.generation.saturating_add(1);
        *self = Self::default();
        self.generation = next_generation;
    }

    #[must_use]
    pub fn snapshot(&self) -> GoalSnapshot {
        // Once the goal is terminal, freeze elapsed at the finish time so the
        // sidebar timer (and any tool snapshot) stops growing after completion.
        let elapsed_seconds = match (self.started_at, self.finished_at) {
            (Some(started), Some(finished)) => {
                Some(finished.saturating_duration_since(started).as_secs())
            }
            (Some(started), None) => Some(started.elapsed().as_secs()),
            (None, _) => None,
        };
        GoalSnapshot {
            objective: self.objective.clone(),
            status: self
                .status
                .map(GoalStatus::as_str)
                .unwrap_or("none")
                .to_string(),
            token_budget: self.token_budget,
            tokens_used: self.tokens_used,
            time_used_seconds: self.time_used_seconds,
            continuation_count: self.continuation_count,
            elapsed_seconds,
            evidence: self.evidence.clone(),
            blocker: self.blocker.clone(),
            completion_verification: self.completion_verification.clone(),
            task_contract: self.task_contract.clone(),
            host_verification: self.host_verification.clone(),
        }
    }
}

/// Serializable tool output and prompt input for the current goal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GoalSnapshot {
    pub objective: Option<String>,
    pub status: String,
    pub token_budget: Option<u32>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub continuation_count: u32,
    pub elapsed_seconds: Option<u64>,
    pub evidence: Option<String>,
    pub blocker: Option<String>,
    pub completion_verification: Option<GoalCompletionVerification>,
    pub task_contract: Option<TaskContract>,
    pub host_verification: Option<GoalHostVerificationReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalCompletionVerification {
    pub status: String,
    pub check: String,
    pub summary: String,
}

/// One immutable acceptance contract for a concrete Goal generation.
///
/// Objective, constraints, non-goals, and acceptance are fixed when the
/// generation is created. An objective-only Goal deliberately requires host
/// acceptance: generic test output is useful evidence, but it is not proof of
/// an arbitrary task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskContract {
    pub version: u32,
    pub goal_generation: u64,
    pub objective: String,
    pub objective_sha256: String,
    pub constraints: Vec<String>,
    pub constraints_sha256: String,
    pub non_goals: Vec<String>,
    pub non_goals_sha256: String,
    pub acceptance: TaskAcceptance,
    pub contract_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskAcceptance {
    HostAcceptanceRequired,
    Verifier {
        verifier_id: String,
        params: Value,
        params_sha256: String,
    },
}

impl TaskAcceptance {
    fn run_verifiers(params: Value) -> Self {
        let params_sha256 = structured_value_hash("goal-verifier-params-v1", &params);
        Self::Verifier {
            verifier_id: DEFAULT_GOAL_VERIFIER_ID.to_string(),
            params,
            params_sha256,
        }
    }
}

impl TaskContract {
    fn derived(
        goal_generation: u64,
        objective: String,
        constraints: Vec<String>,
        non_goals: Vec<String>,
        acceptance: TaskAcceptance,
    ) -> Self {
        let objective_sha256 = domain_hash("goal-objective-v1", objective.as_bytes());
        let constraints_value = serde_json::to_value(&constraints).expect("constraints serialize");
        let non_goals_value = serde_json::to_value(&non_goals).expect("non-goals serialize");
        let constraints_sha256 = structured_value_hash("goal-constraints-v1", &constraints_value);
        let non_goals_sha256 = structured_value_hash("goal-non-goals-v1", &non_goals_value);
        let contract_sha256 = structured_value_hash(
            "goal-task-contract-v1",
            &json!({
                "version": TASK_CONTRACT_VERSION,
                "goal_generation": goal_generation,
                "objective": objective,
                "objective_sha256": objective_sha256,
                "constraints": constraints,
                "constraints_sha256": constraints_sha256,
                "non_goals": non_goals,
                "non_goals_sha256": non_goals_sha256,
                "acceptance": acceptance,
            }),
        );
        Self {
            version: TASK_CONTRACT_VERSION,
            goal_generation,
            objective,
            objective_sha256,
            constraints,
            constraints_sha256,
            non_goals,
            non_goals_sha256,
            acceptance,
            contract_sha256,
        }
    }

    #[must_use]
    pub(crate) fn accepts_verifier(&self, verifier_id: &str, params: &Value) -> bool {
        let TaskAcceptance::Verifier {
            verifier_id: expected_id,
            params: expected_params,
            params_sha256,
        } = &self.acceptance
        else {
            return false;
        };
        self.version == TASK_CONTRACT_VERSION
            && expected_id == verifier_id
            && *params_sha256 == structured_value_hash("goal-verifier-params-v1", params)
            && *expected_params == *params
    }

    fn verifier_binding(&self) -> Option<(&str, &str)> {
        match &self.acceptance {
            TaskAcceptance::HostAcceptanceRequired => None,
            TaskAcceptance::Verifier {
                verifier_id,
                params_sha256,
                ..
            } => Some((verifier_id, params_sha256)),
        }
    }
}

#[must_use]
#[cfg(test)]
pub(crate) fn default_run_verifiers_contract_params() -> Value {
    json!({
        "background": false,
        "commands": [],
        "level": "full",
        "max_python_files": 200,
        "profile": "auto",
    })
}

/// Host-observed verifier receipt bound to the exact Git working-tree state
/// that was checked. This is deliberately small: Goal completion needs proof,
/// not a second workflow engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalHostVerificationReceipt {
    pub tool: String,
    pub check: String,
    pub summary: String,
    pub workspace_revision: String,
    pub contract_sha256: String,
    pub goal_generation: u64,
    pub objective_sha256: String,
    pub verifier_id: String,
    pub verifier_params_sha256: String,
}

impl GoalHostVerificationReceipt {
    pub(crate) fn for_contract(
        contract: &TaskContract,
        tool: &str,
        check: String,
        summary: String,
        workspace_revision: String,
    ) -> Option<Self> {
        let (verifier_id, verifier_params_sha256) = contract.verifier_binding()?;
        Some(Self {
            tool: tool.to_string(),
            check,
            summary,
            workspace_revision,
            contract_sha256: contract.contract_sha256.clone(),
            goal_generation: contract.goal_generation,
            objective_sha256: contract.objective_sha256.clone(),
            verifier_id: verifier_id.to_string(),
            verifier_params_sha256: verifier_params_sha256.to_string(),
        })
    }

    #[must_use]
    pub(crate) fn matches_contract(&self, contract: &TaskContract) -> bool {
        let Some((verifier_id, verifier_params_sha256)) = contract.verifier_binding() else {
            return false;
        };
        self.contract_sha256 == contract.contract_sha256
            && self.goal_generation == contract.goal_generation
            && self.objective_sha256 == contract.objective_sha256
            && self.verifier_id == verifier_id
            && self.verifier_params_sha256 == verifier_params_sha256
            && self.tool == verifier_id
    }
}

/// Revision-bound output from a successful checker. Artifacts are useful
/// evidence, but only a contract-matching host receipt can close a Goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalEvidenceArtifact {
    pub tool: String,
    pub check: String,
    pub summary: String,
    pub workspace_revision: String,
    pub verifier_id: String,
    pub verifier_params_sha256: String,
}

impl GoalSnapshot {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.objective.is_some() && self.status == GoalStatus::Active.as_str()
    }

    #[must_use]
    pub fn from_thread_goal(goal: &codewhale_protocol::ThreadGoal) -> Self {
        Self {
            objective: Some(goal.objective.clone()),
            status: thread_goal_status_as_goal_status(goal.status.clone())
                .as_str()
                .to_string(),
            token_budget: goal
                .token_budget
                .and_then(|value| u32::try_from(value.max(0)).ok()),
            tokens_used: u64::try_from(goal.tokens_used.max(0)).unwrap_or(u64::MAX),
            time_used_seconds: u64::try_from(goal.time_used_seconds.max(0)).unwrap_or(u64::MAX),
            continuation_count: u32::try_from(goal.continuation_count.max(0)).unwrap_or(u32::MAX),
            elapsed_seconds: None,
            evidence: None,
            blocker: None,
            completion_verification: None,
            task_contract: None,
            host_verification: None,
        }
    }
}

#[must_use]
pub fn thread_goal_status_as_goal_status(
    status: codewhale_protocol::ThreadGoalStatus,
) -> GoalStatus {
    match status {
        codewhale_protocol::ThreadGoalStatus::Active => GoalStatus::Active,
        codewhale_protocol::ThreadGoalStatus::Paused => GoalStatus::Paused,
        codewhale_protocol::ThreadGoalStatus::Complete => GoalStatus::Complete,
        codewhale_protocol::ThreadGoalStatus::Blocked
        | codewhale_protocol::ThreadGoalStatus::UsageLimited
        | codewhale_protocol::ThreadGoalStatus::BudgetLimited => GoalStatus::Blocked,
    }
}

fn lock_goal_state(
    state: &SharedGoalState,
) -> Result<std::sync::MutexGuard<'_, GoalState>, ToolError> {
    state
        .lock()
        .map_err(|_| ToolError::execution_failed("goal state lock poisoned"))
}

/// Capture a bounded digest of the Git revision plus every tracked change and
/// non-ignored untracked file in the workspace. A verifier receipt is only
/// reusable while this digest remains identical.
pub(crate) async fn capture_workspace_revision(workspace: &Path) -> Result<String, String> {
    let workspace = workspace.to_path_buf();
    tokio::task::spawn_blocking(move || capture_workspace_revision_sync(&workspace))
        .await
        .map_err(|err| format!("workspace revision task failed: {err}"))?
}

fn capture_workspace_revision_sync(workspace: &Path) -> Result<String, String> {
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|err| format!("cannot resolve workspace {}: {err}", workspace.display()))?;
    let root_raw = run_git_capped(
        &canonical_workspace,
        &["rev-parse", "--show-toplevel"],
        MAX_GIT_PATH_LIST_BYTES,
    )?;
    let root_text = std::str::from_utf8(&root_raw)
        .map_err(|_| "git repository root is not valid UTF-8".to_string())?
        .trim();
    if root_text.is_empty() {
        return Err("git did not report a repository root".to_string());
    }
    let repository_root = PathBuf::from(root_text)
        .canonicalize()
        .map_err(|err| format!("cannot resolve Git repository root {root_text}: {err}"))?;

    let head = run_git_capped(
        &canonical_workspace,
        &["rev-parse", "--verify", "HEAD"],
        4 * 1024,
    )
    .unwrap_or_else(|_| b"unborn".to_vec());
    let staged = run_git_capped(
        &canonical_workspace,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--",
            ".",
        ],
        MAX_GIT_DIFF_BYTES,
    )?;
    let unstaged = run_git_capped(
        &canonical_workspace,
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            ".",
        ],
        MAX_GIT_DIFF_BYTES,
    )?;
    let untracked = run_git_capped(
        &canonical_workspace,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--full-name",
            "-z",
            "--",
            ".",
        ],
        MAX_GIT_PATH_LIST_BYTES,
    )?;

    let mut hasher = Sha256::new();
    hash_segment(
        &mut hasher,
        b"workspace",
        canonical_workspace.as_os_str().as_encoded_bytes(),
    );
    hash_segment(
        &mut hasher,
        b"repository",
        repository_root.as_os_str().as_encoded_bytes(),
    );
    hash_segment(&mut hasher, b"head", &head);
    hash_segment(&mut hasher, b"staged", &staged);
    hash_segment(&mut hasher, b"unstaged", &unstaged);

    let mut total_untracked_bytes = 0_u64;
    for raw_path in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = git_path_from_bytes(raw_path)?;
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!(
                "git reported an unsafe untracked path: {}",
                relative.display()
            ));
        }
        let path = repository_root.join(&relative);
        let metadata = std::fs::symlink_metadata(&path).map_err(|err| {
            format!(
                "cannot inspect untracked path {} while sealing verifier evidence: {err}",
                path.display()
            )
        })?;
        hash_segment(&mut hasher, b"untracked-path", raw_path);
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path)
                .map_err(|err| format!("cannot read symlink {}: {err}", path.display()))?;
            hash_segment(
                &mut hasher,
                b"untracked-symlink",
                target.as_os_str().as_encoded_bytes(),
            );
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "unsupported untracked filesystem entry {}",
                path.display()
            ));
        }
        if metadata.len() > MAX_UNTRACKED_FILE_BYTES {
            return Err(format!(
                "untracked file {} is too large to bind verifier evidence ({} bytes; limit {})",
                path.display(),
                metadata.len(),
                MAX_UNTRACKED_FILE_BYTES
            ));
        }
        total_untracked_bytes = total_untracked_bytes.saturating_add(metadata.len());
        if total_untracked_bytes > MAX_UNTRACKED_TOTAL_BYTES {
            return Err(format!(
                "untracked files are too large to bind verifier evidence (limit {MAX_UNTRACKED_TOTAL_BYTES} bytes)"
            ));
        }
        hash_file(&mut hasher, &path)?;
    }

    let digest = hasher.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{hex}"))
}

fn hash_segment(hasher: &mut Sha256, label: &[u8], value: &[u8]) {
    hasher.update((label.len() as u64).to_le_bytes());
    hasher.update(label);
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn domain_hash(domain: &str, value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hash_segment(&mut hasher, domain.as_bytes(), value);
    format_sha256(hasher.finalize().as_slice())
}

fn structured_value_hash(domain: &str, value: &Value) -> String {
    let canonical = canonical_json(value);
    let encoded = serde_json::to_vec(&canonical).expect("JSON value is serializable");
    domain_hash(domain, &encoded)
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&object[key]));
            }
            Value::Object(canonical)
        }
        primitive => primitive.clone(),
    }
}

fn format_sha256(digest: &[u8]) -> String {
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn hash_file(hasher: &mut Sha256, path: &Path) -> Result<(), String> {
    let mut file = File::open(path)
        .map_err(|err| format!("cannot open untracked file {}: {err}", path.display()))?;
    hasher.update(b"untracked-file\0");
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| format!("cannot read untracked file {}: {err}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

#[cfg(unix)]
fn git_path_from_bytes(path: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
}

#[cfg(not(unix))]
fn git_path_from_bytes(path: &[u8]) -> Result<PathBuf, String> {
    String::from_utf8(path.to_vec())
        .map(PathBuf::from)
        .map_err(|_| "git reported a non-UTF-8 untracked path".to_string())
}

fn run_git_capped(workspace: &Path, args: &[&str], cap: usize) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(workspace).args(args);
    run_command_capped(
        command,
        &format!("git {}", args.join(" ")),
        cap,
        GIT_COMMAND_TIMEOUT,
    )
}

fn run_command_capped(
    mut command: Command,
    label: &str,
    stdout_cap: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    configure_process_tree(&mut command);

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run {label}: {err}"))?;
    let mut process_tree = ProcessTreeOwner::attach_std(&child, label).map_err(|err| {
        let _ = child.kill();
        let _ = child.wait();
        format!("cannot own {label} process tree: {err}")
    })?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} stdout pipe was not created"));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} stderr pipe was not created"));
        }
    };
    let stdout_reader = spawn_capped_reader(stdout, stdout_cap);
    let stderr_reader = spawn_capped_reader(stderr, MAX_GIT_STDERR_BYTES);

    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            let _ = receive_capped_reader(stdout_reader, label, "stdout");
            let _ = receive_capped_reader(stderr_reader, label, "stderr");
            return Err(format!(
                "{label} timed out after {:.1}s while sealing verifier evidence",
                timeout.as_secs_f64()
            ));
        }
        Err(err) => {
            let _ = process_tree.kill();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot wait for {label}: {err}"));
        }
    };

    let stdout = receive_capped_reader(stdout_reader, label, "stdout")?;
    let stderr = receive_capped_reader(stderr_reader, label, "stderr")?;
    // The direct process has been reaped and both inherited pipes are closed.
    // Sweep any detached descendant before releasing the shared tree owner.
    let _ = process_tree.kill();
    process_tree.disarm();
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        return Err(format!("{label} failed with {}: {}", status, detail.trim()));
    }
    Ok(stdout)
}

fn spawn_capped_reader(
    reader: impl Read + Send + 'static,
    cap: usize,
) -> std::sync::mpsc::Receiver<Result<Vec<u8>, String>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(read_capped(reader, cap));
    });
    receiver
}

fn receive_capped_reader(
    receiver: std::sync::mpsc::Receiver<Result<Vec<u8>, String>>,
    label: &str,
    stream: &str,
) -> Result<Vec<u8>, String> {
    match receiver.recv_timeout(GIT_READER_DRAIN_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "{label} {stream} pipe did not close within {:.1}s",
            GIT_READER_DRAIN_TIMEOUT.as_secs_f64()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{label} {stream} reader stopped unexpectedly"))
        }
    }
}

fn read_capped(mut reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("cannot read git output: {err}"))?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > cap {
            return Err(format!("git output exceeded the {cap}-byte evidence limit"));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

/// Evidence observed by the host across one foreground verifier execution.
pub(crate) struct HostVerifierObservation {
    pub(crate) check: String,
    pub(crate) summary: String,
    pub(crate) revision_before: Result<String, String>,
    pub(crate) revision_after: Result<String, String>,
}

/// Attach a host-only verifier receipt when the workspace stayed unchanged
/// for the entire foreground check. Existing tool metadata is preserved.
pub(crate) fn attach_host_verification(
    result: &mut ToolOutcome,
    tool: &str,
    contract: &TaskContract,
    verifier_params: &Value,
    observation: HostVerifierObservation,
) {
    if !contract.accepts_verifier(tool, verifier_params) {
        reject_host_verification(
            result,
            "verifier result does not exactly match the active Goal task contract",
        );
        return;
    }
    let HostVerifierObservation {
        check,
        summary,
        revision_before,
        revision_after,
    } = observation;
    let observation = match (revision_before, revision_after) {
        (Ok(before), Ok(after)) if before == after && result.is_success() => {
            let Some(receipt) =
                GoalHostVerificationReceipt::for_contract(contract, tool, check, summary, after)
            else {
                reject_host_verification(
                    result,
                    "active Goal task contract requires host acceptance",
                );
                return;
            };
            Ok(receipt)
        }
        (Ok(_), Ok(_)) if result.is_success() => Err(
            "workspace changed while the verifier was running; rerun a foreground verifier"
                .to_string(),
        ),
        (Err(err), _) | (_, Err(err)) if result.is_success() => Err(format!(
            "could not bind verifier evidence to the workspace revision: {err}"
        )),
        _ => return,
    };

    let metadata = result.metadata.get_or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({"tool_metadata": metadata.take()});
    }
    let object = metadata
        .as_object_mut()
        .expect("metadata was normalized to an object");
    match observation {
        Ok(receipt) => {
            object.insert(
                HOST_VERIFICATION_METADATA_KEY.to_string(),
                serde_json::to_value(receipt).expect("receipt is serializable"),
            );
        }
        Err(reason) => {
            object
                .entry(HOST_VERIFICATION_REJECTION_METADATA_KEY.to_string())
                .or_insert_with(|| Value::String(reason));
        }
    }
}

/// Attach a revision-bound checker artifact without granting Goal completion
/// authority. This is the only evidence produced by `run_tests`.
///
/// The typed fields are the canonical M4 projection. The metadata mirror is
/// retained only for the not-yet-migrated TUI Goal consumer and is deleted in
/// M5 when TaskContract/evidence completion moves into AgentRuntime.
pub(crate) fn attach_goal_evidence_artifact(
    result: &mut ToolOutcome,
    tool: &str,
    verifier_params: &Value,
    check: String,
    summary: String,
    revision_before: Result<String, String>,
    revision_after: Result<String, String>,
) {
    let observation = match (revision_before, revision_after) {
        (Ok(before), Ok(after)) if before == after && result.is_success() => {
            Ok(GoalEvidenceArtifact {
                tool: tool.to_string(),
                check,
                summary,
                workspace_revision: after,
                verifier_id: tool.to_string(),
                verifier_params_sha256: structured_value_hash(
                    "goal-verifier-params-v1",
                    verifier_params,
                ),
            })
        }
        (Ok(_), Ok(_)) if result.is_success() => Err((
            ToolEvidenceStatus::Stale,
            "workspace changed while the evidence command was running; rerun it in the final workspace"
                .to_string(),
        )),
        (Err(err), _) | (_, Err(err)) if result.is_success() => Err((
            ToolEvidenceStatus::Missing,
            format!("could not bind checker evidence to the workspace revision: {err}"),
        )),
        _ => {
            reject_goal_evidence_artifact(result);
            return;
        }
    };

    let metadata_entry = match observation {
        Ok(artifact) => {
            let artifact_value =
                serde_json::to_value(&artifact).expect("Goal evidence artifact is serializable");
            let artifact_bytes = serde_json::to_vec(&canonical_json(&artifact_value))
                .expect("Goal evidence artifact JSON is serializable");
            let artifact_sha256 = format_sha256(Sha256::digest(&artifact_bytes).as_slice());
            let artifact_id = format!("{GOAL_EVIDENCE_ARTIFACT_ID_PREFIX}{artifact_sha256}");

            result.evidence = ToolEvidence {
                status: ToolEvidenceStatus::Produced,
                references: vec![artifact_id.clone()],
            };
            result.artifacts = vec![ToolArtifact {
                id: artifact_id,
                status: ToolArtifactStatus::Available,
                sha256: Some(artifact_sha256),
                media_type: Some(GOAL_EVIDENCE_ARTIFACT_MEDIA_TYPE.to_string()),
                byte_len: Some(
                    u64::try_from(artifact_bytes.len())
                        .expect("Goal evidence artifact length fits in u64"),
                ),
            }];
            result.workspace_revision = Some(artifact.workspace_revision.clone());
            (
                GOAL_EVIDENCE_ARTIFACT_METADATA_KEY.to_string(),
                artifact_value,
            )
        }
        Err((status, reason)) => {
            set_goal_evidence_status(result, status);
            (
                HOST_VERIFICATION_REJECTION_METADATA_KEY.to_string(),
                Value::String(reason),
            )
        }
    };

    let metadata = result.metadata.get_or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({"tool_metadata": metadata.take()});
    }
    metadata
        .as_object_mut()
        .expect("metadata was normalized to an object")
        .insert(metadata_entry.0, metadata_entry.1);
}

/// Record that a checker ran but did not produce acceptable evidence (for
/// example, it failed or executed zero tests). Rejection never creates an
/// artifact reference or a workspace binding.
pub(crate) fn reject_goal_evidence_artifact(result: &mut ToolOutcome) {
    set_goal_evidence_status(result, ToolEvidenceStatus::Rejected);
}

fn set_goal_evidence_status(result: &mut ToolOutcome, status: ToolEvidenceStatus) {
    result.evidence = ToolEvidence {
        status,
        references: Vec::new(),
    };
    result.artifacts.clear();
    result.workspace_revision = None;
}

/// Explain why a successful command is not strong enough to complete a Goal
/// (for example, a zero-test filter or a model-supplied custom command).
pub(crate) fn reject_host_verification(result: &mut ToolOutcome, reason: impl Into<String>) {
    let metadata = result.metadata.get_or_insert_with(|| json!({}));
    if !metadata.is_object() {
        *metadata = json!({"tool_metadata": metadata.take()});
    }
    metadata
        .as_object_mut()
        .expect("metadata was normalized to an object")
        .insert(
            HOST_VERIFICATION_REJECTION_METADATA_KEY.to_string(),
            Value::String(reason.into()),
        );
}

#[must_use]
pub(crate) fn is_host_verifier_tool(tool: &str) -> bool {
    matches!(tool, "run_tests" | "run_verifiers")
}

#[must_use]
pub(crate) fn host_verification_from_result(
    tool: &str,
    result: &ToolOutcome,
) -> Option<GoalHostVerificationReceipt> {
    if !result.is_success() || !is_host_verifier_tool(tool) {
        return None;
    }
    let receipt = result
        .metadata
        .as_ref()?
        .get(HOST_VERIFICATION_METADATA_KEY)?;
    let receipt: GoalHostVerificationReceipt = serde_json::from_value(receipt.clone()).ok()?;
    (receipt.tool == tool && receipt.verifier_id == tool).then_some(receipt)
}

fn parse_token_budget(input: &Value) -> Result<Option<u32>, ToolError> {
    let Some(raw) = input.get("token_budget") else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let Some(value) = raw.as_u64() else {
        return Err(ToolError::invalid_input(
            "token_budget must be a non-negative integer",
        ));
    };
    u32::try_from(value)
        .map(Some)
        .map_err(|_| ToolError::invalid_input("token_budget is too large"))
}

fn parse_contract_scope(input: &Value, field: &str) -> Result<Vec<String>, ToolError> {
    let Some(raw) = input.get(field) else {
        return Ok(Vec::new());
    };
    let Some(items) = raw.as_array() else {
        return Err(ToolError::invalid_input(format!(
            "{field} must be an array of strings"
        )));
    };
    if items.len() > MAX_CONTRACT_SCOPE_ITEMS {
        return Err(ToolError::invalid_input(format!(
            "{field} may contain at most {MAX_CONTRACT_SCOPE_ITEMS} items"
        )));
    }
    let mut normalized = Vec::with_capacity(items.len());
    for item in items {
        let Some(item) = item.as_str() else {
            return Err(ToolError::invalid_input(format!(
                "every {field} item must be a string"
            )));
        };
        let item = item.trim();
        if item.is_empty() {
            return Err(ToolError::invalid_input(format!(
                "{field} items cannot be empty"
            )));
        }
        if item.chars().count() > MAX_CONTRACT_SCOPE_ITEM_CHARS {
            return Err(ToolError::invalid_input(format!(
                "each {field} item may contain at most {MAX_CONTRACT_SCOPE_ITEM_CHARS} characters"
            )));
        }
        if !normalized.iter().any(|existing| existing == item) {
            normalized.push(item.to_string());
        }
    }
    Ok(normalized)
}

fn parse_task_acceptance(input: &Value) -> Result<Option<Value>, ToolError> {
    let Some(raw) = input.get("acceptance") else {
        return Ok(None);
    };
    let Some(acceptance) = raw.as_object() else {
        return Err(ToolError::invalid_input("acceptance must be an object"));
    };
    if acceptance
        .keys()
        .any(|key| !matches!(key.as_str(), "verifier_id" | "params"))
    {
        return Err(ToolError::invalid_input(
            "acceptance supports only verifier_id and params",
        ));
    }
    let verifier_id = acceptance
        .get("verifier_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid_input("acceptance.verifier_id is required"))?;
    if verifier_id != DEFAULT_GOAL_VERIFIER_ID {
        return Err(ToolError::invalid_input(
            "acceptance.verifier_id must be run_verifiers",
        ));
    }
    let params = acceptance
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| ToolError::invalid_input("acceptance.params must be an object"))?;
    if params.keys().any(|key| {
        !matches!(
            key.as_str(),
            "profile" | "level" | "max_python_files" | "commands" | "background"
        )
    }) {
        return Err(ToolError::invalid_input(
            "acceptance.params contains an unsupported run_verifiers field",
        ));
    }

    let profile = params
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    if !matches!(profile, "auto" | "rust" | "node" | "python" | "go") {
        return Err(ToolError::invalid_input(
            "acceptance.params.profile is not supported",
        ));
    }
    let level = params
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("full");
    if level != "full" {
        return Err(ToolError::invalid_input(
            "Goal acceptance requires run_verifiers level=full",
        ));
    }
    let max_python_files = params
        .get("max_python_files")
        .and_then(Value::as_u64)
        .unwrap_or(200);
    if !(1..=1000).contains(&max_python_files) {
        return Err(ToolError::invalid_input(
            "acceptance.params.max_python_files must be between 1 and 1000",
        ));
    }
    let background = params
        .get("background")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if background {
        return Err(ToolError::invalid_input(
            "Goal acceptance cannot use background verification",
        ));
    }
    let commands = params
        .get("commands")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if commands.len() > MAX_CONTRACT_CUSTOM_GATES {
        return Err(ToolError::invalid_input(format!(
            "acceptance.params.commands may contain at most {MAX_CONTRACT_CUSTOM_GATES} gates"
        )));
    }
    let commands = commands
        .iter()
        .map(normalize_contract_command)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(json!({
        "background": false,
        "commands": commands,
        "level": "full",
        "max_python_files": max_python_files,
        "profile": profile,
    })))
}

fn normalize_contract_command(raw: &Value) -> Result<Value, ToolError> {
    let Some(command) = raw.as_object() else {
        return Err(ToolError::invalid_input(
            "every acceptance command must be an object",
        ));
    };
    if command
        .keys()
        .any(|key| !matches!(key.as_str(), "name" | "program" | "args" | "cwd"))
    {
        return Err(ToolError::invalid_input(
            "acceptance command contains an unsupported field",
        ));
    }
    let required_exact = |field: &str| -> Result<String, ToolError> {
        let value = command.get(field).and_then(Value::as_str).ok_or_else(|| {
            ToolError::invalid_input(format!("acceptance command.{field} is required"))
        })?;
        if value.is_empty() || value.trim() != value {
            return Err(ToolError::invalid_input(format!(
                "acceptance command.{field} must be non-empty and have no surrounding whitespace"
            )));
        }
        Ok(value.to_string())
    };
    let name = required_exact("name")?;
    let program = required_exact("program")?;
    if Path::new(&program)
        .file_name()
        .and_then(|name| name.to_str())
        == Some("true")
    {
        return Err(ToolError::invalid_input(
            "acceptance commands cannot use the no-op true program",
        ));
    }
    let args = command
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if args.iter().any(|arg| !arg.is_string()) {
        return Err(ToolError::invalid_input(
            "acceptance command.args must contain only strings",
        ));
    }
    let cwd = match command.get("cwd") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(value)) if !value.trim().is_empty() && value.trim() == value => {
            Value::String(value.clone())
        }
        _ => {
            return Err(ToolError::invalid_input(
                "acceptance command.cwd must be a non-empty string without surrounding whitespace",
            ));
        }
    };
    Ok(json!({
        "args": args,
        "cwd": cwd,
        "name": name,
        "program": program,
    }))
}

fn json_result(snapshot: &GoalSnapshot) -> Result<ToolOutcome, ToolError> {
    ToolOutcome::json(snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
}

pub struct CreateGoalTool {
    goal_state: SharedGoalState,
}

impl CreateGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for CreateGoalTool {
    fn name(&self) -> &'static str {
        "create_goal"
    }

    fn description(&self) -> &'static str {
        "Create the current runtime goal. Use this only when the user explicitly asks to pursue a persistent objective."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "The full objective to pursue. Keep the complete user goal, not a shortened one-turn version."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Optional soft token budget for the goal."
                },
                "constraints": {
                    "type": "array",
                    "maxItems": MAX_CONTRACT_SCOPE_ITEMS,
                    "items": { "type": "string", "minLength": 1 },
                    "description": "Optional explicit constraints. Objective-only goals remain simple and require host acceptance."
                },
                "non_goals": {
                    "type": "array",
                    "maxItems": MAX_CONTRACT_SCOPE_ITEMS,
                    "items": { "type": "string", "minLength": 1 },
                    "description": "Optional explicit non-goals that must remain out of scope."
                },
                "acceptance": {
                    "type": "object",
                    "description": "Optional trusted task acceptance. Without it, checks are artifacts and only the host can complete the Goal.",
                    "properties": {
                        "verifier_id": {
                            "type": "string",
                            "const": "run_verifiers"
                        },
                        "params": {
                            "type": "object",
                            "properties": {
                                "profile": {
                                    "type": "string",
                                    "enum": ["auto", "rust", "node", "python", "go"],
                                    "default": "auto"
                                },
                                "level": {
                                    "type": "string",
                                    "const": "full",
                                    "default": "full"
                                },
                                "max_python_files": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "maximum": 1000,
                                    "default": 200
                                },
                                "commands": {
                                    "type": "array",
                                    "maxItems": MAX_CONTRACT_CUSTOM_GATES,
                                    "default": [],
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "name": { "type": "string", "minLength": 1 },
                                            "program": { "type": "string", "minLength": 1 },
                                            "args": {
                                                "type": "array",
                                                "items": { "type": "string" },
                                                "default": []
                                            },
                                            "cwd": { "type": "string", "minLength": 1 }
                                        },
                                        "required": ["name", "program"],
                                        "additionalProperties": false
                                    }
                                },
                                "background": {
                                    "type": "boolean",
                                    "const": false,
                                    "default": false
                                }
                            },
                            "additionalProperties": false
                        }
                    },
                    "required": ["verifier_id", "params"],
                    "additionalProperties": false
                }
            },
            "required": ["objective"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let objective = required_str(&input, "objective")?.trim().to_string();
        if objective.is_empty() {
            return Err(ToolError::invalid_input("objective cannot be empty"));
        }
        let token_budget = parse_token_budget(&input)?;
        let constraints = parse_contract_scope(&input, "constraints")?;
        let non_goals = parse_contract_scope(&input, "non_goals")?;
        let acceptance = parse_task_acceptance(&input)?;
        let snapshot = {
            let mut state = lock_goal_state(&self.goal_state)?;
            if let Some(verifier_params) = acceptance {
                state.create_with_contract(
                    objective,
                    token_budget,
                    constraints,
                    non_goals,
                    verifier_params,
                );
            } else {
                state.replace_generation(
                    objective,
                    token_budget,
                    GoalStatus::Active,
                    constraints,
                    non_goals,
                    TaskAcceptance::HostAcceptanceRequired,
                );
            }
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

pub struct GetGoalTool {
    goal_state: SharedGoalState,
}

impl GetGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for GetGoalTool {
    fn name(&self) -> &'static str {
        "get_goal"
    }

    fn description(&self) -> &'static str {
        "Inspect the current runtime goal state, including objective, status, token budget, elapsed time, evidence, and blocker."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutcome, ToolError> {
        let snapshot = {
            let state = lock_goal_state(&self.goal_state)?;
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

pub struct UpdateGoalTool {
    goal_state: SharedGoalState,
}

impl UpdateGoalTool {
    #[must_use]
    pub fn new(goal_state: SharedGoalState) -> Self {
        Self { goal_state }
    }
}

#[async_trait]
impl ToolSpec for UpdateGoalTool {
    fn name(&self) -> &'static str {
        "update_goal"
    }

    fn description(&self) -> &'static str {
        "Update the runtime goal completion gate. Only mark complete when the objective has verified evidence; mark blocked only after a real blocker prevents progress."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["complete", "blocked"],
                    "description": "Use complete only when the goal is fully satisfied; blocked when meaningful progress cannot continue. Pause, resume, and budget-limit states are controlled by the user or system."
                },
                "evidence": {
                    "type": "string",
                    "description": "Required when status is complete. Briefly cite the proof that the goal is done."
                },
                "blocker": {
                    "type": "string",
                    "description": "Required when status is blocked. Explain the condition preventing progress."
                },
                "objective": {
                    "type": "string",
                    "description": "Reserved for future host-controlled goal edits; ignored by update_goal."
                }
            },
            "required": ["status"],
            "additionalProperties": false
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let status = required_str(&input, "status")?.trim().to_ascii_lowercase();
        if status == "complete" {
            let evidence = input
                .get("evidence")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if evidence.is_empty() {
                return Err(ToolError::invalid_input(
                    "evidence is required when status is complete",
                ));
            }
            let receipt = {
                let state = lock_goal_state(&self.goal_state)?;
                if matches!(
                    state
                        .active_task_contract()
                        .map(|contract| &contract.acceptance),
                    Some(TaskAcceptance::HostAcceptanceRequired)
                ) {
                    return Err(ToolError::invalid_input(
                        "当前任务契约要求宿主验收；模型不能用 update_goal 将 objective-only Goal 标记为完成",
                    ));
                }
                state.host_verification().cloned().ok_or_else(|| {
                    ToolError::invalid_input(
                        "Goal 还没有匹配任务契约的宿主验证凭据；请在最终代码状态上用契约中的精确参数运行前台 run_verifiers",
                    )
                })?
            };
            let current_revision = capture_workspace_revision(context.workspace())
                .await
                .map_err(|err| {
                    ToolError::execution_failed(format!(
                        "无法确认 Goal 验证凭据对应的工作区版本：{err}"
                    ))
                })?;
            if current_revision != receipt.workspace_revision {
                if let Ok(mut state) = self.goal_state.lock()
                    && state.host_verification() == Some(&receipt)
                {
                    state.clear_host_verification();
                }
                return Err(ToolError::invalid_input(
                    "工作区在验证通过后发生了变化；请用任务契约中的精确参数重新运行前台 run_verifiers",
                ));
            }
            let snapshot = {
                let mut state = lock_goal_state(&self.goal_state)?;
                if state.host_verification() != Some(&receipt) {
                    return Err(ToolError::invalid_input(
                        "Goal 验证凭据已被新的验证结果替换；请重新检查后再完成",
                    ));
                }
                state
                    .mark_complete(evidence, receipt)
                    .map_err(ToolError::invalid_input)?;
                state.snapshot()
            };
            return json_result(&snapshot);
        }
        let snapshot = {
            let mut state = lock_goal_state(&self.goal_state)?;
            match status.as_str() {
                "blocked" => {
                    let blocker = input
                        .get("blocker")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_string();
                    if blocker.is_empty() {
                        return Err(ToolError::invalid_input(
                            "blocker is required when status is blocked",
                        ));
                    }
                    state
                        .mark_blocked(blocker)
                        .map_err(ToolError::invalid_input)?;
                }
                other => {
                    return Err(ToolError::invalid_input(format!(
                        "unsupported goal status '{other}'; update_goal can only mark complete or blocked"
                    )));
                }
            }
            state.snapshot()
        };
        json_result(&snapshot)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use serde_json::{Value, json};
    use tempfile::{TempDir, tempdir};

    use super::*;

    fn git_workspace() -> TempDir {
        let directory = tempdir().expect("temp workspace");
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(directory.path())
            .status()
            .expect("git init should run");
        assert!(status.success(), "git init should succeed");
        directory
    }

    async fn record_test_receipt(state: &SharedGoalState, workspace: &Path) {
        let revision = capture_workspace_revision(workspace)
            .await
            .expect("workspace revision");
        let mut goal = state.lock().expect("goal lock");
        let receipt = test_receipt(&mut goal, &revision);
        assert!(goal.record_host_verification(receipt));
    }

    fn test_receipt(goal: &mut GoalState, revision: &str) -> GoalHostVerificationReceipt {
        if matches!(
            goal.active_task_contract()
                .map(|contract| &contract.acceptance),
            Some(TaskAcceptance::HostAcceptanceRequired)
        ) {
            let objective = goal.objective().expect("objective").to_string();
            let token_budget = goal.token_budget();
            goal.create_with_contract(
                objective,
                token_budget,
                Vec::new(),
                Vec::new(),
                default_run_verifiers_contract_params(),
            );
        }
        GoalHostVerificationReceipt::for_contract(
            goal.active_task_contract().expect("active task contract"),
            DEFAULT_GOAL_VERIFIER_ID,
            "run_verifiers profile=auto level=full".to_string(),
            "verifiers passed".to_string(),
            revision.to_string(),
        )
        .expect("explicit verifier contract")
    }

    #[cfg(unix)]
    #[test]
    fn evidence_command_timeout_kills_the_process_group_and_returns_bounded() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30 & wait"]);
        let started = Instant::now();

        let error = run_command_capped(
            command,
            "hanging evidence command",
            1024,
            Duration::from_millis(100),
        )
        .expect_err("the evidence command must time out");

        assert!(error.contains("timed out"), "unexpected error: {error}");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "timeout path exceeded its bounded settlement window"
        );
    }

    #[tokio::test]
    async fn create_get_and_complete_goal() {
        let workspace = git_workspace();
        let state = new_shared_goal_state();
        let ctx = ToolContext::new(workspace.path());

        let create = CreateGoalTool::new(state.clone());
        let created = create
            .execute(
                json!({
                    "objective": "ship the runtime slice",
                    "token_budget": 1200,
                    "constraints": ["  keep one runtime  ", "keep one runtime"],
                    "non_goals": ["provider expansion"],
                    "acceptance": {
                        "verifier_id": "run_verifiers",
                        "params": {}
                    }
                }),
                &ctx,
            )
            .await
            .expect("create goal");
        assert!(created.is_success());
        let created_json: Value = serde_json::from_str(&created.content).expect("created json");
        assert_eq!(
            created_json.get("status").and_then(Value::as_str),
            Some("active")
        );

        let get = GetGoalTool::new(state.clone());
        let current = get.execute(json!({}), &ctx).await.expect("get goal");
        assert!(current.content.contains("ship the runtime slice"));
        let current_json: Value = serde_json::from_str(&current.content).expect("current json");
        assert_eq!(
            current_json.get("token_budget").and_then(Value::as_u64),
            Some(1200)
        );
        assert_eq!(
            current_json["task_contract"]["constraints"],
            json!(["keep one runtime"])
        );
        assert_eq!(
            current_json["task_contract"]["non_goals"],
            json!(["provider expansion"])
        );
        assert_eq!(
            current_json["task_contract"]["acceptance"]["kind"],
            json!("verifier")
        );
        record_test_receipt(&state, workspace.path()).await;

        let update = UpdateGoalTool::new(state.clone());
        let completed = update
            .execute(
                json!({
                    "status": "complete",
                    "evidence": "focused tests passed"
                }),
                &ctx,
            )
            .await
            .expect("complete goal");
        let completed_json: Value =
            serde_json::from_str(&completed.content).expect("completed json");
        assert_eq!(
            completed_json.get("status").and_then(Value::as_str),
            Some("complete")
        );
        assert!(completed.content.contains("focused tests passed"));
        assert_eq!(
            completed_json
                .get("completion_verification")
                .and_then(|verification| verification.get("check"))
                .and_then(Value::as_str),
            Some("run_verifiers profile=auto level=full")
        );
        assert!(!state.lock().expect("goal lock").is_active());
    }

    #[tokio::test]
    async fn objective_only_goal_requires_host_acceptance() {
        let workspace = git_workspace();
        let state = new_shared_goal_state();
        let context = ToolContext::new(workspace.path());
        CreateGoalTool::new(state.clone())
            .execute(
                json!({"objective": "write an unrelated document"}),
                &context,
            )
            .await
            .expect("create objective-only goal");
        let contract = state
            .lock()
            .expect("goal lock")
            .active_task_contract()
            .expect("task contract")
            .clone();
        assert_eq!(contract.acceptance, TaskAcceptance::HostAcceptanceRequired);

        let mut verifier_result = ToolOutcome::success("all generic gates passed");
        let params = default_run_verifiers_contract_params();
        attach_host_verification(
            &mut verifier_result,
            DEFAULT_GOAL_VERIFIER_ID,
            &contract,
            &params,
            HostVerifierObservation {
                check: "generic full gates".to_string(),
                summary: "passed".to_string(),
                revision_before: Ok("sha256:same".to_string()),
                revision_after: Ok("sha256:same".to_string()),
            },
        );
        assert!(
            host_verification_from_result(DEFAULT_GOAL_VERIFIER_ID, &verifier_result).is_none()
        );
        assert!(
            verifier_result.metadata.as_ref().unwrap()[HOST_VERIFICATION_REJECTION_METADATA_KEY]
                .as_str()
                .is_some_and(|reason| reason.contains("does not exactly match"))
        );
        let error = UpdateGoalTool::new(state.clone())
            .execute(
                json!({"status": "complete", "evidence": "generic gates"}),
                &context,
            )
            .await
            .expect_err("objective-only Goal must not auto-complete");
        assert!(error.to_string().contains("任务契约"));
        assert!(state.lock().expect("goal lock").is_active());
    }

    #[tokio::test]
    async fn update_goal_requires_completion_evidence() {
        let state = new_shared_goal_state_from_host_status(
            Some("prove completion".to_string()),
            None,
            GoalStatus::Active,
        );
        let update = UpdateGoalTool::new(state);
        let err = update
            .execute(json!({"status": "complete"}), &ToolContext::new("."))
            .await
            .expect_err("missing evidence should fail");

        assert!(err.to_string().contains("evidence is required"));
    }

    #[tokio::test]
    async fn update_goal_rejects_model_authored_not_applicable_bypass() {
        let state = new_shared_goal_state_from_host_status(
            Some("write the release notes".to_string()),
            None,
            GoalStatus::Active,
        );
        let update = UpdateGoalTool::new(state.clone());
        let error = update
            .execute(
                json!({
                    "status": "complete",
                    "evidence": "release notes drafted and reviewed in thread",
                    "verification": {
                        "status": "not_applicable",
                        "check": "no automated verifier applies",
                        "summary": "writing task completed with evidence in thread"
                    }
                }),
                &ToolContext::new("."),
            )
            .await
            .expect_err("model text must not bypass host evidence");

        assert!(error.to_string().contains("要求宿主验收"));
        assert!(state.lock().expect("goal lock").is_active());
    }

    #[tokio::test]
    async fn update_goal_requires_host_verification_to_complete() {
        let state = new_shared_goal_state();
        state.lock().expect("goal lock").create_with_contract(
            "prove completion".to_string(),
            None,
            Vec::new(),
            Vec::new(),
            default_run_verifiers_contract_params(),
        );
        let update = UpdateGoalTool::new(state.clone());
        let err = update
            .execute(
                json!({
                    "status": "complete",
                    "evidence": "all checks look good"
                }),
                &ToolContext::new("."),
            )
            .await
            .expect_err("missing verifier gate should fail");

        assert!(err.to_string().contains("宿主验证凭据"));
        assert!(state.lock().expect("goal lock").is_active());
    }

    #[tokio::test]
    async fn update_goal_rejects_workspace_changes_after_verification() {
        let workspace = git_workspace();
        let state = new_shared_goal_state_from_host_status(
            Some("prove the final revision".to_string()),
            None,
            GoalStatus::Active,
        );
        record_test_receipt(&state, workspace.path()).await;
        fs::write(workspace.path().join("changed-after-test.txt"), "new state")
            .expect("mutate workspace");

        let error = UpdateGoalTool::new(state.clone())
            .execute(
                json!({
                    "status": "complete",
                    "evidence": "stale test result"
                }),
                &ToolContext::new(workspace.path()),
            )
            .await
            .expect_err("stale receipt must fail");

        assert!(error.to_string().contains("工作区在验证通过后发生了变化"));
        let goal = state.lock().expect("goal lock");
        assert!(goal.is_active());
        assert!(goal.host_verification().is_none());
    }

    #[tokio::test]
    async fn workspace_revision_tracks_untracked_content() {
        let workspace = git_workspace();
        let before = capture_workspace_revision(workspace.path())
            .await
            .expect("initial revision");
        fs::write(workspace.path().join("artifact.txt"), "one").expect("write artifact");
        let first = capture_workspace_revision(workspace.path())
            .await
            .expect("revision with artifact");
        fs::write(workspace.path().join("artifact.txt"), "two").expect("change artifact");
        let second = capture_workspace_revision(workspace.path())
            .await
            .expect("changed artifact revision");

        assert_ne!(before, first);
        assert_ne!(first, second);
    }

    #[test]
    fn host_verification_receipt_requires_stable_successful_result() {
        let mut goal = GoalState::default();
        goal.create_with_contract(
            "bind the receipt".to_string(),
            None,
            Vec::new(),
            Vec::new(),
            default_run_verifiers_contract_params(),
        );
        let contract = goal
            .active_task_contract()
            .expect("active contract")
            .clone();
        let params = default_run_verifiers_contract_params();
        let mut accepted = ToolOutcome::success("ok").with_metadata(json!({"existing": true}));
        attach_host_verification(
            &mut accepted,
            "run_verifiers",
            &contract,
            &params,
            HostVerifierObservation {
                check: "full gates".to_string(),
                summary: "passed".to_string(),
                revision_before: Ok("sha256:same".to_string()),
                revision_after: Ok("sha256:same".to_string()),
            },
        );
        let receipt = host_verification_from_result("run_verifiers", &accepted)
            .expect("stable successful verifier should carry a receipt");
        assert_eq!(receipt.workspace_revision, "sha256:same");
        assert_eq!(accepted.metadata.as_ref().unwrap()["existing"], true);

        let mut rejected = ToolOutcome::success("ok");
        attach_host_verification(
            &mut rejected,
            "run_verifiers",
            &contract,
            &params,
            HostVerifierObservation {
                check: "quick".to_string(),
                summary: "passed".to_string(),
                revision_before: Ok("sha256:before".to_string()),
                revision_after: Ok("sha256:after".to_string()),
            },
        );
        assert!(host_verification_from_result("run_verifiers", &rejected).is_none());
        assert!(
            rejected.metadata.as_ref().unwrap()[HOST_VERIFICATION_REJECTION_METADATA_KEY]
                .as_str()
                .unwrap()
                .contains("workspace changed")
        );
    }

    #[test]
    fn checker_artifact_projection_is_stable_and_failures_do_not_mint_artifacts() {
        let params = json!({"profile": "auto", "level": "full"});
        let attach_stable = |result: &mut ToolOutcome| {
            attach_goal_evidence_artifact(
                result,
                "run_verifiers",
                &params,
                "full gates".to_string(),
                "all gates passed".to_string(),
                Ok("sha256:stable".to_string()),
                Ok("sha256:stable".to_string()),
            );
        };

        let mut first = ToolOutcome::success("ok");
        attach_stable(&mut first);
        let mut second = ToolOutcome::success("ok");
        attach_stable(&mut second);
        assert_eq!(first.evidence.status, ToolEvidenceStatus::Produced);
        assert_eq!(first.evidence.references, second.evidence.references);
        assert_eq!(first.artifacts, second.artifacts);
        assert_eq!(first.workspace_revision.as_deref(), Some("sha256:stable"));
        assert_eq!(first.artifacts[0].status, ToolArtifactStatus::Available);
        assert_eq!(first.evidence.references[0], first.artifacts[0].id);

        let mut stale = ToolOutcome::success("ok");
        attach_goal_evidence_artifact(
            &mut stale,
            "run_verifiers",
            &params,
            "full gates".to_string(),
            "all gates passed".to_string(),
            Ok("sha256:before".to_string()),
            Ok("sha256:after".to_string()),
        );
        assert_eq!(stale.evidence.status, ToolEvidenceStatus::Stale);
        assert!(stale.evidence.references.is_empty());
        assert!(stale.artifacts.is_empty());
        assert!(stale.workspace_revision.is_none());

        let mut missing = ToolOutcome::success("ok");
        attach_goal_evidence_artifact(
            &mut missing,
            "run_tests",
            &json!({}),
            "cargo test".to_string(),
            "tests passed".to_string(),
            Err("git unavailable".to_string()),
            Ok("sha256:after".to_string()),
        );
        assert_eq!(missing.evidence.status, ToolEvidenceStatus::Missing);
        assert!(missing.evidence.references.is_empty());
        assert!(missing.artifacts.is_empty());
        assert!(missing.workspace_revision.is_none());

        let mut rejected = ToolOutcome::error("tests failed");
        reject_goal_evidence_artifact(&mut rejected);
        assert_eq!(rejected.evidence.status, ToolEvidenceStatus::Rejected);
        assert!(rejected.evidence.references.is_empty());
        assert!(rejected.artifacts.is_empty());
        assert!(rejected.workspace_revision.is_none());
    }

    #[test]
    fn goal_accepts_only_the_active_contract_receipt() {
        let mut goal = GoalState::default();
        goal.create("implement the exact objective".to_string(), None);
        let valid = test_receipt(&mut goal, "sha256:final");

        let mut wrong_generation = valid.clone();
        wrong_generation.goal_generation = wrong_generation.goal_generation.saturating_add(1);
        assert!(!goal.record_host_verification(wrong_generation));

        let mut wrong_objective = valid.clone();
        wrong_objective.objective_sha256 = "sha256:other-objective".to_string();
        assert!(!goal.record_host_verification(wrong_objective));

        let mut wrong_verifier = valid.clone();
        wrong_verifier.verifier_id = "run_tests".to_string();
        wrong_verifier.tool = "run_tests".to_string();
        assert!(!goal.record_host_verification(wrong_verifier));

        let mut wrong_params = valid.clone();
        wrong_params.verifier_params_sha256 = "sha256:other-params".to_string();
        assert!(!goal.record_host_verification(wrong_params));

        let mut wrong_contract = valid.clone();
        wrong_contract.contract_sha256 = "sha256:other-contract".to_string();
        assert!(!goal.record_host_verification(wrong_contract));

        assert!(goal.record_host_verification(valid.clone()));
        goal.mark_complete("contract matched".to_string(), valid)
            .expect("matching receipt completes the Goal");
        assert_eq!(goal.snapshot().status, "complete");
    }

    #[test]
    fn prior_generation_receipt_cannot_complete_replaced_goal() {
        let mut goal = GoalState::default();
        goal.create("first objective".to_string(), None);
        let stale = test_receipt(&mut goal, "sha256:first");
        goal.create("second objective".to_string(), None);

        assert!(!goal.record_host_verification(stale.clone()));
        assert!(
            goal.mark_complete("stale".to_string(), stale).is_err(),
            "a receipt from the prior generation must fail closed"
        );
        assert!(goal.is_active());
    }

    #[tokio::test]
    async fn update_goal_rejects_model_resume() {
        let state = new_shared_goal_state_from_host_status(
            Some("pause remains host controlled".to_string()),
            None,
            GoalStatus::Paused,
        );
        let update = UpdateGoalTool::new(state);
        let err = update
            .execute(json!({"status": "active"}), &ToolContext::new("."))
            .await
            .expect_err("model resume should fail");

        assert!(err.to_string().contains("complete or blocked"));
    }

    #[tokio::test]
    async fn host_pause_revokes_receipt_and_cannot_be_overridden_by_update_goal() {
        let workspace = git_workspace();
        let state = new_shared_goal_state_from_host_status(
            Some("host pause wins".to_string()),
            None,
            GoalStatus::Active,
        );
        record_test_receipt(&state, workspace.path()).await;
        {
            let mut goal = state.lock().expect("goal lock");
            goal.sync_from_host_status(Some("host pause wins"), None, GoalStatus::Paused);
            assert!(goal.host_verification().is_none());
        }

        let error = UpdateGoalTool::new(state.clone())
            .execute(
                json!({
                    "status": "complete",
                    "evidence": "stale in-flight completion"
                }),
                &ToolContext::new(workspace.path()),
            )
            .await
            .expect_err("model must not override host pause");

        assert!(error.to_string().contains("宿主验证凭据"));
        assert_eq!(state.lock().expect("goal lock").snapshot().status, "paused");

        let blocked_error = UpdateGoalTool::new(state.clone())
            .execute(
                json!({
                    "status": "blocked",
                    "blocker": "model tries to overwrite pause"
                }),
                &ToolContext::new(workspace.path()),
            )
            .await
            .expect_err("model must not replace host pause with blocked");
        assert!(blocked_error.to_string().contains("host-controlled"));
        assert_eq!(state.lock().expect("goal lock").snapshot().status, "paused");
    }

    #[test]
    fn paused_host_goal_is_not_active() {
        let state = new_shared_goal_state_from_host_status(
            Some("wait for user".to_string()),
            Some(42),
            GoalStatus::Paused,
        );
        let snapshot = state.lock().expect("goal lock").snapshot();

        assert_eq!(snapshot.status, "paused");
        assert_eq!(snapshot.token_budget, Some(42));
        assert!(!snapshot.is_active());
    }

    #[test]
    fn goal_state_projects_usage_and_continuations() {
        let state = new_shared_goal_state_from_host_status(
            Some("persist accounting".to_string()),
            Some(1_000),
            GoalStatus::Active,
        );
        {
            let mut goal = state.lock().expect("goal lock");
            goal.record_usage(300, 12);
            goal.record_continuation();
        }

        let snapshot = state.lock().expect("goal lock").snapshot();
        assert_eq!(snapshot.tokens_used, 300);
        assert_eq!(snapshot.time_used_seconds, 12);
        assert_eq!(snapshot.continuation_count, 1);
    }

    #[test]
    fn completion_candidate_still_records_its_enclosing_turn_usage() {
        let state = new_shared_goal_state_from_host_status(
            Some("account the final turn".to_string()),
            Some(1_000),
            GoalStatus::Active,
        );
        {
            let mut goal = state.lock().expect("goal lock");
            let receipt = test_receipt(&mut goal, "sha256:test");
            goal.mark_complete("verified".to_string(), receipt)
                .expect("completion candidate");
            goal.record_usage(321, 7);
        }

        let snapshot = state.lock().expect("goal lock").snapshot();
        assert_eq!(snapshot.status, "complete");
        assert_eq!(snapshot.tokens_used, 321);
        assert_eq!(snapshot.time_used_seconds, 7);
    }

    #[test]
    fn terminal_same_objective_reactivation_starts_a_clean_generation() {
        let mut goal = GoalState::default();
        goal.create("resume this objective".to_string(), Some(1_000));
        let completed_generation = goal.active_generation().expect("active generation");
        goal.record_usage_for_generation(completed_generation, 321, 7);
        let receipt = test_receipt(&mut goal, "sha256:old");
        goal.mark_complete("old evidence".to_string(), receipt)
            .expect("complete first generation");

        goal.sync_from_host_status(
            Some("resume this objective"),
            Some(1_000),
            GoalStatus::Active,
        );

        let resumed_generation = goal.active_generation().expect("resumed generation");
        assert!(resumed_generation > completed_generation);
        assert!(!goal.record_usage_for_generation(completed_generation, 99, 1));
        let snapshot = goal.snapshot();
        assert_eq!(snapshot.status, "active");
        assert_eq!(snapshot.tokens_used, 0);
        assert_eq!(snapshot.time_used_seconds, 0);
        assert!(snapshot.evidence.is_none());
        assert!(snapshot.completion_verification.is_none());
        assert!(snapshot.host_verification.is_none());
    }

    #[test]
    fn paused_same_objective_resume_retains_generation_and_usage() {
        let mut goal = GoalState::default();
        goal.create("pause this objective".to_string(), Some(1_000));
        let generation = goal.active_generation().expect("active generation");
        assert!(goal.record_usage_for_generation(generation, 123, 4));
        goal.sync_from_host_status(
            Some("pause this objective"),
            Some(1_000),
            GoalStatus::Paused,
        );
        goal.sync_from_host_status(
            Some("pause this objective"),
            Some(1_000),
            GoalStatus::Active,
        );

        assert_eq!(goal.active_generation(), Some(generation));
        let snapshot = goal.snapshot();
        assert_eq!(snapshot.tokens_used, 123);
        assert_eq!(snapshot.time_used_seconds, 4);
    }

    #[test]
    fn completed_goal_snapshot_freezes_elapsed() {
        // Regression: a completed goal's snapshot elapsed_seconds must not keep
        // growing. Before the fix, snapshot() always used started_at.elapsed(),
        // so a finished goal's elapsed kept ticking in the sidebar/tool output.
        let state = new_shared_goal_state_from_host_status(
            Some("freeze on completion".to_string()),
            None,
            GoalStatus::Active,
        );
        let first = {
            let mut goal = state.lock().expect("goal lock");
            let receipt = test_receipt(&mut goal, "sha256:test");
            goal.mark_complete("evidence".to_string(), receipt)
                .expect("mark complete");
            goal.snapshot()
        };
        let elapsed_at_completion = first.elapsed_seconds.expect("elapsed present");

        // Sleep past a whole-second boundary. Under the old (buggy) code,
        // snapshot() returned started_at.elapsed().as_secs(), so this would
        // tick up by at least one second and the assertion below would fail.
        // With the freeze, the completed snapshot stays at the captured value.
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        let second = state.lock().expect("goal lock").snapshot();
        assert_eq!(second.status, "complete");
        assert_eq!(
            second.elapsed_seconds,
            Some(elapsed_at_completion),
            "completed goal elapsed must be frozen, not keep ticking"
        );
    }

    #[test]
    fn protocol_thread_goal_converts_to_runtime_snapshot() {
        let snapshot = GoalSnapshot::from_thread_goal(&codewhale_protocol::ThreadGoal {
            thread_id: "thread-1".to_string(),
            goal_id: "goal-1".to_string(),
            objective: "Bridge the goal models".to_string(),
            status: codewhale_protocol::ThreadGoalStatus::Active,
            token_budget: Some(2_000),
            tokens_used: 750,
            time_used_seconds: 44,
            continuation_count: 3,
            created_at: 1,
            updated_at: 2,
        });

        assert_eq!(
            snapshot.objective.as_deref(),
            Some("Bridge the goal models")
        );
        assert_eq!(snapshot.status, "active");
        assert_eq!(snapshot.token_budget, Some(2_000));
        assert_eq!(snapshot.tokens_used, 750);
        assert_eq!(snapshot.time_used_seconds, 44);
        assert_eq!(snapshot.continuation_count, 3);
    }
}
