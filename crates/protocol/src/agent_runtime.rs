//! Canonical domain protocol for the single Agent runtime.
//!
//! These types deliberately describe model turns and runtime facts without
//! depending on a UI, an HTTP transport, or a database representation.

use std::collections::HashSet;
use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::task::{
    AcceptanceId, CompletionCandidate, CompletionDecision, CompletionRejection, EvidenceReceipt,
    FailedVerifierEvidence, TaskContract, VerificationId, VerifierObservation, VerifierSpec,
    VerifierVerdict, WorkspaceMutationEvidence, WorkspaceRevision, WorkspaceState, canonical_json,
};

pub const MIN_SUPPORTED_AGENT_RUNTIME_EVENT_SCHEMA_VERSION: u32 = 19;
pub const AGENT_RUNTIME_EVENT_SCHEMA_VERSION: u32 = 19;
pub const AGENT_TOOL_NAME: &str = "agent";
pub const REQUEST_USER_INPUT_TOOL_NAME: &str = "request_user_input";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheControl {
    Stable,
    Volatile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPromptBlock {
    pub text: String,
    pub cache_control: PromptCacheControl,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemPrompt {
    pub blocks: Vec<SystemPromptBlock>,
}

impl SystemPrompt {
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            blocks: vec![SystemPromptBlock {
                text: text.into(),
                cache_control: PromptCacheControl::Stable,
            }],
        }
    }
}

impl From<String> for SystemPrompt {
    fn from(value: String) -> Self {
        Self::from_text(value)
    }
}

impl From<&str> for SystemPrompt {
    fn from(value: &str) -> Self {
        Self::from_text(value)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    #[default]
    High,
    Max,
}

/// Stable origin of one immutable model selection.
///
/// `FixedActor` is a Host-owned actor profile, not a user-selectable model
/// mode. The selected model and reasoning effort remain on `RunRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteProfile {
    Explicit,
    FixedActor,
}

/// Minimal durable audit fact for one immutable per-run model selection.
///
/// The selected model and reasoning remain the canonical `RunRequest`
/// fields. This record keeps only the neutral profile and Host decision
/// identity needed to audit or reopen that selection without routing again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ModelRouteAudit {
    pub profile: ModelRouteProfile,
    pub policy_version: String,
    pub reason_code: String,
}

impl ModelRouteAudit {
    pub fn validate(&self) -> Result<(), String> {
        validate_reason_code("model route policy version", &self.policy_version)?;
        validate_reason_code("model route reason code", &self.reason_code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActorKind {
    Root,
    Child,
}

/// Host-frozen authority for where model-authored workspace writes may run.
///
/// This is independent from child depth: read-only child Agents remain
/// available in both modes, while an isolated Writer requires an explicit
/// `IsolatedWriter` root run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteExecutionMode {
    #[default]
    Root,
    IsolatedWriter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiSurface {
    StandardChat,
    StrictChat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActor {
    pub kind: AgentActorKind,
    pub depth: u8,
}

impl Default for AgentActor {
    fn default() -> Self {
        Self {
            kind: AgentActorKind::Root,
            depth: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub String);

impl RunId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<&str> for RunId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for RunId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentTaskId(pub String);

impl fmt::Display for AgentTaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<&str> for AgentTaskId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for AgentTaskId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPolicy {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Vec<String>>,
    #[serde(default)]
    pub denied: Vec<String>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed: None,
            denied: Vec::new(),
        }
    }
}

impl ToolPolicy {
    #[must_use]
    pub fn permits(&self, name: &str) -> bool {
        self.enabled
            && !self.denied.iter().any(|denied| denied == name)
            && self
                .allowed
                .as_ref()
                .is_none_or(|allowed| allowed.iter().any(|candidate| candidate == name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLimits {
    pub max_turns: u32,
    /// Canonical model requests admitted by the AgentRuntime. Physical HTTP
    /// admission, including route selection and transport retries, is an
    /// application/model-accounting concern rather than a runtime limit.
    pub max_model_requests: u32,
    pub max_model_retries: u32,
    pub max_tool_calls: u32,
    pub max_depth: u8,
    /// Maximum child runs admitted across the whole root runtime tree before
    /// their parent has joined and consumed their outcome.
    #[serde(default = "default_max_concurrent_children")]
    pub max_concurrent_children: u32,
    /// Maximum silence between canonical model stream events. Raw transport
    /// bytes that do not decode to a model event do not reset this deadline.
    /// `None` explicitly disables the runtime-level event idle deadline.
    #[serde(default = "default_model_event_idle_ms")]
    pub model_event_idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wall_time_ms: Option<u64>,
}

fn default_max_concurrent_children() -> u32 {
    64
}

fn default_model_event_idle_ms() -> Option<u64> {
    Some(900_000)
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_turns: 64,
            max_model_requests: 64,
            max_model_retries: 2,
            max_tool_calls: 256,
            max_depth: 4,
            max_concurrent_children: default_max_concurrent_children(),
            model_event_idle_ms: default_model_event_idle_ms(),
            wall_time_ms: None,
        }
    }
}

/// Host-assigned filesystem authority for one child Agent.
///
/// A role profile never implies this authority. Only an
/// [`AgentWorkspaceAssignment`] created and validated by the Host can grant an
/// isolated writer worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceAccess {
    ReadOnly,
    IsolatedWrite,
}

/// Exact workspace identity owned by one canonical [`AgentTask`].
///
/// `base_commit` is the immutable Git object from which the task view was
/// created. Writer-only fields are all-or-nothing so replay can distinguish an
/// owned worktree from an unrelated directory or branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentWorkspaceAssignment {
    pub access: AgentWorkspaceAccess,
    pub root_workspace: String,
    pub base_commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    /// Exact root branch that the Host may fast-forward after verification.
    ///
    /// This must survive restart independently from the writer branch:
    /// another root branch may point at the same base commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_token: Option<String>,
}

impl AgentWorkspaceAssignment {
    #[must_use]
    pub fn execution_workspace(&self) -> &str {
        self.worktree_path
            .as_deref()
            .unwrap_or(self.root_workspace.as_str())
    }

    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("root workspace", &self.root_workspace)?;
        validate_git_object_id("base commit", &self.base_commit)?;
        validate_relative_path_list("allowed path", &self.allowed_paths)?;
        match self.access {
            AgentWorkspaceAccess::ReadOnly => {
                if self.worktree_path.is_some()
                    || self.root_branch.is_some()
                    || self.branch.is_some()
                    || !self.allowed_paths.is_empty()
                    || self.owner_token.is_some()
                {
                    return Err(
                        "read-only Agent workspace cannot own a worktree, root branch, writer branch, allowed write paths, or owner token".to_owned(),
                    );
                }
            }
            AgentWorkspaceAccess::IsolatedWrite => {
                let worktree = self.worktree_path.as_deref().ok_or_else(|| {
                    "isolated writer workspace requires a worktree path".to_owned()
                })?;
                require_agent_text("worktree path", worktree)?;
                if worktree == self.root_workspace {
                    return Err(
                        "isolated writer worktree must differ from the root workspace".to_owned(),
                    );
                }
                require_agent_text(
                    "root worktree branch",
                    self.root_branch.as_deref().ok_or_else(|| {
                        "isolated writer workspace requires the exact root branch".to_owned()
                    })?,
                )?;
                require_agent_text(
                    "worktree branch",
                    self.branch
                        .as_deref()
                        .ok_or_else(|| "isolated writer workspace requires a branch".to_owned())?,
                )?;
                require_agent_text(
                    "worktree owner token",
                    self.owner_token.as_deref().ok_or_else(|| {
                        "isolated writer workspace requires an owner token".to_owned()
                    })?,
                )?;
                if self.allowed_paths.is_empty() {
                    return Err(
                        "isolated writer workspace requires at least one allowed path".to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}

/// Frozen Host contract for one canonical child Agent.
///
/// Model-supplied intent is resolved into this type before any child run or
/// worktree side effect begins. Every field is persisted so recovery never
/// needs to reconstruct authority from a prompt or role name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentTask {
    pub task_id: AgentTaskId,
    pub root_run_id: RunId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub call_id: String,
    pub role: String,
    pub task_contract: TaskContract,
    pub workspace: AgentWorkspaceAssignment,
    /// Host-frozen official model selection for this child run.
    pub model: String,
    pub reasoning_effort: ReasoningEffort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    pub context_policy: ContextPolicy,
    pub route: ModelRouteAudit,
    pub tool_policy: ToolPolicy,
    pub limits: RunLimits,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    pub expected_artifact: String,
}

impl AgentTask {
    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("Agent task id", &self.task_id.0)?;
        require_agent_text("root run id", &self.root_run_id.0)?;
        require_agent_text("parent run id", &self.parent_run_id.0)?;
        require_agent_text("child run id", &self.child_run_id.0)?;
        require_agent_text("Agent call id", &self.call_id)?;
        require_agent_text("Agent role", &self.role)?;
        require_agent_text("expected artifact", &self.expected_artifact)?;
        require_agent_text("Agent task model", &self.model)?;
        self.route.validate()?;
        if self.child_run_id == self.root_run_id || self.child_run_id == self.parent_run_id {
            return Err("child run id must differ from root and parent run ids".to_owned());
        }
        self.task_contract.validate()?;
        if self.task_contract.generation_id.0 != self.child_run_id.0 {
            return Err("Agent task generation id must equal the child run id".to_owned());
        }
        self.workspace.validate()?;
        if self.limits.max_turns == 0
            || self.limits.max_model_requests == 0
            || self.limits.max_tool_calls == 0
        {
            return Err("Agent task limits must admit turns, model requests, and tools".to_owned());
        }
        if self.deadline_unix_ms == Some(0) {
            return Err("Agent task deadline must be a positive Unix timestamp".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
    /// Terminal root run whose canonical conversation this new root run
    /// continues. This is independent from `parent_run_id`, which is reserved
    /// for the root/child Agent hierarchy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued_from_run_id: Option<RunId>,
    pub model: String,
    /// Host-owned audit identity for the immutable `model` and
    /// `reasoning_effort` selected for this run.
    pub route: ModelRouteAudit,
    /// Frozen Host task boundary for this Agent run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_contract: Option<TaskContract>,
    pub system_prompt: SystemPrompt,
    #[serde(default)]
    pub transcript: CanonicalTranscript,
    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    pub streaming: bool,
    #[serde(default)]
    pub actor: AgentActor,
    /// Frozen orchestration contract for a child Agent. Root runs must not
    /// carry this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_task: Option<AgentTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    #[serde(default)]
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub limits: RunLimits,
    #[serde(default)]
    pub environment: RunEnvironment,
    #[serde(default)]
    pub context_policy: ContextPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_projection: Option<ContextProjection>,
    /// Typed facts inherited only across a canonical root continuation.
    /// They remain RunStore facts; clients cannot use them to rewrite the
    /// transcript or satisfy the new task generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_facts: Option<InheritedRunFacts>,
    /// Physical model accounting already owned by the fresh run's
    /// `ModelPort`. Persisting it in `run_created` closes the crash window for
    /// pre-runtime calls such as DeepSeek auto-routing. A reopened process
    /// adds only its new local accounting to this durable baseline.
    #[serde(default)]
    pub accounting_baseline: ModelAccounting,
}

impl RunRequest {
    #[must_use]
    pub fn new(task_contract: TaskContract, system_prompt: impl Into<SystemPrompt>) -> Self {
        let run_id = RunId::from(task_contract.generation_id.0.clone());
        Self {
            run_id: Some(run_id),
            parent_run_id: None,
            continued_from_run_id: None,
            model: "deepseek-v4-flash".to_owned(),
            route: ModelRouteAudit {
                profile: ModelRouteProfile::Explicit,
                policy_version: "runtime_explicit_v1".to_owned(),
                reason_code: "explicit_model".to_owned(),
            },
            task_contract: Some(task_contract),
            system_prompt: system_prompt.into(),
            transcript: CanonicalTranscript::default(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: None,
            streaming: true,
            actor: AgentActor::default(),
            agent_task: None,
            deadline_unix_ms: None,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            environment: RunEnvironment::default(),
            context_policy: ContextPolicy::default(),
            context_projection: None,
            inherited_facts: None,
            accounting_baseline: ModelAccounting::default(),
        }
    }

    /// Validate the exact relationship between a persisted run and its
    /// orchestration task. Reducers call this when accepting `RunCreated`.
    pub fn validate_agent_task_binding(&self) -> Result<(), String> {
        require_agent_text("RunRequest model", &self.model)?;
        self.route.validate()?;
        match self.actor.kind {
            AgentActorKind::Root => {
                if self.parent_run_id.is_some() || self.agent_task.is_some() {
                    return Err("root Agent run cannot carry a parent run or AgentTask".to_owned());
                }
            }
            AgentActorKind::Child => {
                let task = self
                    .agent_task
                    .as_ref()
                    .ok_or_else(|| "child Agent run requires an AgentTask".to_owned())?;
                task.validate()?;
                if self.run_id.as_ref() != Some(&task.child_run_id)
                    || self.parent_run_id.as_ref() != Some(&task.parent_run_id)
                    || self.task_contract.as_ref() != Some(&task.task_contract)
                    || self.model != task.model
                    || self.reasoning_effort != task.reasoning_effort
                    || self.max_output_tokens != task.max_output_tokens
                    || self.context_policy != task.context_policy
                    || self.route != task.route
                    || self.tool_policy != task.tool_policy
                    || self.limits != task.limits
                    || self.deadline_unix_ms != task.deadline_unix_ms
                    || self.environment.workspace != task.workspace.execution_workspace()
                {
                    return Err(
                        "child RunRequest does not exactly match its frozen AgentTask".to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}

/// Host-owned context limits for the official DeepSeek model selected for a
/// run. Transport clients cannot supply these values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPolicy {
    pub hard_input_tokens: u32,
}

/// Persisted per-request projection produced by context compaction.
///
/// `CanonicalTranscript` remains complete and append-only. This projection
/// supplies only the compacted model-visible prefix; transcript entries after
/// `source_entry_count` are appended when preparing the next request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextProjection {
    pub source_entry_count: u64,
    pub source_projection_sha256: String,
    /// Canonical transcript entries selected into `messages`, in exact
    /// order. System entries are never selected.
    pub selected_entry_indices: Vec<u64>,
    pub messages: Vec<ModelMessage>,
}

/// Immutable host facts required to reopen a run without silently changing
/// its workspace or model-visible tool catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEnvironment {
    /// Canonical absolute workspace path. Empty is reserved for unit tests and
    /// embedders that do not expose a filesystem.
    pub workspace: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_catalog_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_fingerprint_sha256: Option<String>,
    pub write_execution_mode: WriteExecutionMode,
    pub auto_approve: bool,
    pub trust_mode: bool,
    pub allow_sandbox_elevation: bool,
    /// Whether this client can answer durable runtime interaction requests.
    /// Headless callers keep this disabled so approval-gated tools fail closed
    /// instead of leaving a run waiting for a response that can never arrive.
    #[serde(default)]
    pub interactive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ModelMessage {
    User {
        content: String,
    },
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ModelToolCall>,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
    pub actor: AgentActor,
    pub model: String,
    pub system_prompt: SystemPrompt,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolDefinition>,
    pub reasoning_effort: ReasoningEffort,
    pub max_output_tokens: Option<u32>,
    pub streaming: bool,
    pub request_number: u32,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolArguments {
    /// Exact argument bytes received from DeepSeek. This is replayed without
    /// normalizing whitespace or object key order.
    pub raw: String,
    /// Parsed JSON when valid. Malformed arguments remain representable and
    /// are returned to the model as an ordinary failed tool outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed: Option<Value>,
}

impl ToolArguments {
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        Self {
            raw: value.to_string(),
            parsed: Some(value),
        }
    }

    #[must_use]
    pub fn parse(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let parsed = serde_json::from_str(&raw).ok();
        Self { raw, parsed }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: ToolArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    InsufficientSystemResource,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub reasoning_replay_tokens: u64,
}

impl Usage {
    #[must_use]
    pub fn total_tokens(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn add_assign(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_hit_tokens = self.cache_hit_tokens.saturating_add(other.cache_hit_tokens);
        self.cache_miss_tokens = self
            .cache_miss_tokens
            .saturating_add(other.cache_miss_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.reasoning_replay_tokens = self
            .reasoning_replay_tokens
            .saturating_add(other.reasoning_replay_tokens);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelOutput {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
    pub finish_reason: ModelFinishReason,
    #[serde(default)]
    pub usage: Usage,
}

/// Redacted facts observed while consuming one model response.
///
/// These booleans deliberately omit response text, reasoning text, tool
/// arguments, credentials, headers, and local paths. The DeepSeek transport
/// is the sole production writer; Runtime uses the facts only to decide
/// whether replaying the request is safe and to persist a diagnosable failure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelResponseEvidence {
    pub response_headers_received: bool,
    pub content_observed: bool,
    pub reasoning_observed: bool,
    pub tool_call_observed: bool,
    pub tool_call_id_observed: bool,
    pub tool_call_name_observed: bool,
    pub tool_call_arguments_observed: bool,
    pub finish_reason_observed: bool,
    pub finish_reason_trusted: bool,
    pub usage_received: bool,
    pub stream_done_received: bool,
}

impl ModelResponseEvidence {
    pub fn merge(&mut self, other: Self) {
        self.response_headers_received |= other.response_headers_received;
        self.content_observed |= other.content_observed;
        self.reasoning_observed |= other.reasoning_observed;
        self.tool_call_observed |= other.tool_call_observed;
        self.tool_call_id_observed |= other.tool_call_id_observed;
        self.tool_call_name_observed |= other.tool_call_name_observed;
        self.tool_call_arguments_observed |= other.tool_call_arguments_observed;
        self.finish_reason_observed |= other.finish_reason_observed;
        self.finish_reason_trusted |= other.finish_reason_trusted;
        self.usage_received |= other.usage_received;
        self.stream_done_received |= other.stream_done_received;
    }

    #[must_use]
    pub fn actionable_output(self) -> bool {
        self.content_observed || self.reasoning_observed || self.tool_call_observed
    }

    #[must_use]
    pub fn replay_safe(self) -> bool {
        !self.actionable_output() && !self.finish_reason_observed && !self.stream_done_received
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelStreamEvent {
    ResponseProgress { evidence: ModelResponseEvidence },
    ContentDelta { delta: String },
    ReasoningDelta { delta: String },
    Completed { output: ModelOutput },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub run_id: RunId,
    pub call_id: String,
    pub name: String,
    pub arguments: ToolArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInvocationStatus {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTransportStatus {
    NotStarted,
    Succeeded,
    Failed,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOperationStatus {
    NotStarted,
    Succeeded,
    Failed,
    Cancelled,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSideEffectStatus {
    NotApplicable,
    NotApplied,
    Applied,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRetryDisposition {
    NotNeeded,
    AfterCorrection,
    /// Repeating the invocation cannot duplicate a task-workspace side
    /// effect. This is a safety statement, not a promise that retrying will
    /// succeed or that the Host will retry automatically.
    Safe,
    Unsafe,
    NotRetryable,
}

/// Stable, model- and evaluator-visible reason for an unsuccessful tool call.
///
/// Lifecycle, side effects, and retry safety remain owned by their existing
/// typed fields; this enum only supplies the missing causal identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureCode {
    MalformedArguments,
    SchemaValidation,
    InvocationRejected,
    UnknownTool,
    MissingField,
    InvalidField,
    WorkspacePrecondition,
    StaleRead,
    AmbiguousEdit,
    PatchParse,
    OperationFailed,
    TransportFailed,
    SideEffectAmbiguous,
    VerifierFailed,
}

impl ToolFailureCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedArguments => "malformed_arguments",
            Self::SchemaValidation => "schema_validation",
            Self::InvocationRejected => "invocation_rejected",
            Self::UnknownTool => "unknown_tool",
            Self::MissingField => "missing_field",
            Self::InvalidField => "invalid_field",
            Self::WorkspacePrecondition => "workspace_precondition",
            Self::StaleRead => "stale_read",
            Self::AmbiguousEdit => "ambiguous_edit",
            Self::PatchParse => "patch_parse",
            Self::OperationFailed => "operation_failed",
            Self::TransportFailed => "transport_failed",
            Self::SideEffectAmbiguous => "side_effect_ambiguous",
            Self::VerifierFailed => "verifier_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEvidenceStatus {
    NotApplicable,
    Missing,
    Produced,
    Rejected,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEvidence {
    pub status: ToolEvidenceStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

impl Default for ToolEvidence {
    fn default() -> Self {
        Self {
            status: ToolEvidenceStatus::NotApplicable,
            references: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolArtifactStatus {
    NotApplicable,
    Available,
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArtifact {
    pub id: String,
    pub status: ToolArtifactStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_len: Option<u64>,
    /// Small canonical evidence payloads remain replay-verifiable after a
    /// crash instead of degrading into untrusted digest metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VerificationArtifactPayload {
    pub summary: String,
    pub verifier: VerifierSpec,
    pub verdict: VerifierVerdict,
    pub workspace_revision: WorkspaceRevision,
}

impl VerificationArtifactPayload {
    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("verification artifact summary", &self.summary)?;
        self.verifier.validate()?;
        self.workspace_revision.validate()
    }
}

fn canonical_verification_artifact_bytes(content: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&canonical_json(content))
}

impl ToolArtifact {
    const VERIFICATION_ID_PREFIX: &'static str = "verification-evidence:";
    const VERIFICATION_MEDIA_TYPE: &'static str = "application/vnd.dse.verification+json";

    #[must_use]
    pub fn inline_verification(payload: VerificationArtifactPayload) -> Self {
        payload
            .validate()
            .expect("verification artifact payload is valid");
        let content = canonical_json(
            &serde_json::to_value(payload)
                .expect("verification artifact payload JSON is serializable"),
        );
        let bytes = canonical_verification_artifact_bytes(&content)
            .expect("canonical verification evidence JSON is serializable");
        let sha256 = format_prefixed_sha256(&bytes);
        Self {
            id: format!("{}{sha256}", Self::VERIFICATION_ID_PREFIX),
            status: ToolArtifactStatus::Available,
            sha256: Some(sha256),
            media_type: Some(Self::VERIFICATION_MEDIA_TYPE.to_owned()),
            byte_len: Some(
                u64::try_from(bytes.len()).expect("verification evidence length fits in u64"),
            ),
            inline_content: Some(content),
        }
    }

    pub fn validate_inline_verification(&self) -> Result<VerificationArtifactPayload, String> {
        let content = self
            .inline_content
            .as_ref()
            .ok_or_else(|| "verification artifact is missing its inline payload".to_owned())?;
        let bytes = canonical_verification_artifact_bytes(content)
            .map_err(|error| format!("verification artifact cannot be encoded: {error}"))?;
        let sha256 = format_prefixed_sha256(&bytes);
        if self.status != ToolArtifactStatus::Available
            || self.sha256.as_deref() != Some(sha256.as_str())
            || self.id != format!("{}{sha256}", Self::VERIFICATION_ID_PREFIX)
            || self.media_type.as_deref() != Some(Self::VERIFICATION_MEDIA_TYPE)
            || self.byte_len != u64::try_from(bytes.len()).ok()
        {
            return Err(
                "verification artifact metadata does not match its inline payload".to_owned(),
            );
        }
        let payload: VerificationArtifactPayload = serde_json::from_value(content.clone())
            .map_err(|error| format!("verification artifact payload is invalid: {error}"))?;
        payload.validate()?;
        Ok(payload)
    }
}

/// The one tool outcome shared by tool implementations, AgentRuntime,
/// transcript projection, persistence, and every client surface.
///
/// `metadata` is tool-specific structured payload only. Host decisions must
/// use the typed lifecycle, retry, evidence, artifact, and revision fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<ToolFailureCode>,
    pub invocation: ToolInvocationStatus,
    pub transport: ToolTransportStatus,
    pub operation: ToolOperationStatus,
    pub side_effect: ToolSideEffectStatus,
    pub retry: ToolRetryDisposition,
    pub evidence: ToolEvidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_revision: Option<String>,
    /// Verifier implementations may report a typed observation. Only the
    /// Host runtime can match it to a frozen contract and seal a receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier_observation: Option<VerifierObservation>,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostVerificationFailure {
    pub outcome: ToolOutcome,
    pub workspace_state: WorkspaceState,
    /// Deterministic rejection that must be committed after the verifier
    /// result. Persisting it in the replay projection closes the crash window
    /// between those two canonical events without rerunning the verifier.
    pub rejection: CompletionRejection,
}

/// Durable local progress toward a `failed_write_pass` acceptance.
///
/// Child Writer provenance travels in its sealed receipt; this state only
/// tracks failure and effective mutations observed in the current workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TemporalEvidenceProgress {
    pub acceptance_id: AcceptanceId,
    pub verifier: VerifierSpec,
    pub failure: FailedVerifierEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutation: Option<WorkspaceMutationEvidence>,
}

impl TemporalEvidenceProgress {
    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("temporal evidence acceptance id", &self.acceptance_id.0)?;
        self.verifier.validate()?;
        self.failure.validate()?;
        if let Some(mutation) = &self.mutation {
            mutation.validate()?;
            if self.failure.workspace_state.generation > mutation.workspace_state_before.generation
            {
                return Err("temporal verifier failure must precede its mutation".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InheritedRunFacts {
    pub workspace_state: WorkspaceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_completion_rejection: Option<CompletionRejection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_host_verification_failure: Option<HostVerificationFailure>,
}

impl ToolOutcome {
    #[must_use]
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            failure_code: None,
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Succeeded,
            side_effect: ToolSideEffectStatus::NotApplicable,
            retry: ToolRetryDisposition::NotNeeded,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            verifier_observation: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            failure_code: Some(ToolFailureCode::OperationFailed),
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Failed,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::NotRetryable,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            verifier_observation: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn rejected(content: impl Into<String>, retry: ToolRetryDisposition) -> Self {
        Self {
            failure_code: Some(ToolFailureCode::InvocationRejected),
            invocation: ToolInvocationStatus::Rejected,
            transport: ToolTransportStatus::NotStarted,
            operation: ToolOperationStatus::NotStarted,
            side_effect: ToolSideEffectStatus::NotApplied,
            retry,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            verifier_observation: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn transport_failure(content: impl Into<String>) -> Self {
        Self {
            failure_code: Some(ToolFailureCode::TransportFailed),
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Failed,
            operation: ToolOperationStatus::Indeterminate,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::Unsafe,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            verifier_observation: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn recovery_ambiguous(content: impl Into<String>) -> Self {
        Self {
            failure_code: Some(ToolFailureCode::SideEffectAmbiguous),
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Indeterminate,
            operation: ToolOperationStatus::Indeterminate,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::Unsafe,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            verifier_observation: None,
            content: content.into(),
            metadata: None,
        }
    }

    /// Reconcile an executor outcome with the workspace authority frozen in
    /// `ToolPrepared`. A read-only invocation cannot apply a task-workspace
    /// mutation, so an incomplete operation may remain transport/operation
    /// indeterminate without inventing write recovery or an unsafe replay.
    /// Outcomes that explicitly claim an applied side effect are left intact
    /// for the RunStore reducer to reject as an authority violation.
    #[must_use]
    pub fn with_workspace_access_guarantee(mut self, workspace_access: WorkspaceAccess) -> Self {
        if workspace_access == WorkspaceAccess::ReadOnly && !self.is_success() {
            if self.side_effect == ToolSideEffectStatus::Indeterminate {
                self.side_effect = ToolSideEffectStatus::NotApplicable;
            }
            if self.retry == ToolRetryDisposition::Unsafe {
                self.retry = ToolRetryDisposition::Safe;
            }
            if self.failure_code == Some(ToolFailureCode::SideEffectAmbiguous) {
                self.failure_code = Some(ToolFailureCode::OperationFailed);
            }
        }
        self
    }

    #[must_use]
    pub fn is_success(&self) -> bool {
        self.invocation == ToolInvocationStatus::Accepted
            && self.transport == ToolTransportStatus::Succeeded
            && self.operation == ToolOperationStatus::Succeeded
    }

    /// Render the sole model-visible failure envelope used by root and child
    /// requests. Successful tool output remains byte-for-byte unchanged.
    #[must_use]
    pub fn model_content(&self) -> String {
        let Some(code) = self.failure_code else {
            return self.content.clone();
        };
        let recovery = match code {
            ToolFailureCode::MalformedArguments
            | ToolFailureCode::SchemaValidation
            | ToolFailureCode::MissingField
            | ToolFailureCode::InvalidField
            | ToolFailureCode::PatchParse => "修正参数后再调用；不要原样重复。",
            ToolFailureCode::InvocationRejected => match self.retry {
                ToolRetryDisposition::AfterCorrection => "根据 Host 拒绝原因修正调用后再试。",
                _ => "当前 Host 不接受该调用；不要原样重复。",
            },
            ToolFailureCode::WorkspacePrecondition | ToolFailureCode::StaleRead => {
                "先重新读取相关文件或工作区状态，再根据最新内容修正调用。"
            }
            ToolFailureCode::AmbiguousEdit => "先读取更多上下文，再使用唯一且更精确的编辑范围。",
            ToolFailureCode::UnknownTool => "只使用当前请求实际提供的工具名。",
            ToolFailureCode::TransportFailed => match self.retry {
                ToolRetryDisposition::Safe => {
                    "传输失败，但已确认没有工作区副作用，可以安全重新调用。"
                }
                ToolRetryDisposition::AfterCorrection => "修正传输参数或环境后再调用。",
                _ => "不要自动重放；先确认副作用状态或等待 Host recovery。",
            },
            ToolFailureCode::SideEffectAmbiguous => {
                "副作用状态无法安全确认；不要自动重放，等待 Host recovery 或人工处理。"
            }
            ToolFailureCode::VerifierFailed => "根据确定性 verifier 结果修改工作区后重新验证。",
            ToolFailureCode::OperationFailed => match self.retry {
                ToolRetryDisposition::AfterCorrection => "根据错误详情修正操作后再调用。",
                ToolRetryDisposition::Safe => "已确认没有副作用，可以安全重试。",
                _ => "不要盲目重试；先根据错误详情选择修正或停止。",
            },
        };
        format!(
            "工具失败：code={}; operation={}; side_effect={}; retry={}。\n恢复建议：{}\n{}",
            code.as_str(),
            tool_operation_status_name(self.operation),
            tool_side_effect_status_name(self.side_effect),
            tool_retry_disposition_name(self.retry),
            recovery,
            self.content
        )
    }

    /// A deterministic verifier may report external side effects as
    /// indeterminate, but it cannot mutate the task workspace. Both the live
    /// completion gate and replay use this exact revision relation.
    #[must_use]
    pub fn has_stable_verifier_revision(
        &self,
        before: &WorkspaceRevision,
        after: &WorkspaceRevision,
    ) -> bool {
        let Some(observation) = &self.verifier_observation else {
            return false;
        };
        self.side_effect != ToolSideEffectStatus::Applied
            && matches!(
                (
                    before,
                    after,
                    &observation.workspace_revision,
                    self.workspace_revision.as_deref(),
                ),
                (
                    WorkspaceRevision::Known { sha256: before },
                    WorkspaceRevision::Known { sha256: after },
                    WorkspaceRevision::Known { sha256: observed },
                    Some(outcome),
                ) if before == after && after == observed && observed == outcome
            )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.is_success() && self.failure_code.is_some() {
            return Err("a successful tool outcome cannot carry a failure code".to_owned());
        }
        if !self.is_success() && self.failure_code.is_none() {
            return Err("an unsuccessful tool outcome requires a failure code".to_owned());
        }
        if matches!(
            self.failure_code,
            Some(
                ToolFailureCode::MalformedArguments
                    | ToolFailureCode::SchemaValidation
                    | ToolFailureCode::InvocationRejected
                    | ToolFailureCode::UnknownTool
                    | ToolFailureCode::MissingField
                    | ToolFailureCode::PatchParse
            )
        ) && self.invocation != ToolInvocationStatus::Rejected
        {
            return Err("a preflight failure code requires a rejected invocation".to_owned());
        }
        if matches!(
            self.failure_code,
            Some(
                ToolFailureCode::WorkspacePrecondition
                    | ToolFailureCode::StaleRead
                    | ToolFailureCode::AmbiguousEdit
                    | ToolFailureCode::VerifierFailed
            )
        ) && (self.invocation != ToolInvocationStatus::Accepted
            || self.transport != ToolTransportStatus::Succeeded
            || self.operation != ToolOperationStatus::Failed)
        {
            return Err(
                "an observed tool-operation failure code requires accepted/failed lifecycle facts"
                    .to_owned(),
            );
        }
        if self.failure_code == Some(ToolFailureCode::TransportFailed)
            && (self.invocation != ToolInvocationStatus::Accepted
                || self.transport != ToolTransportStatus::Failed
                || self.operation != ToolOperationStatus::Indeterminate)
        {
            return Err(
                "a transport failure code requires accepted/failed/indeterminate lifecycle facts"
                    .to_owned(),
            );
        }
        if self.failure_code == Some(ToolFailureCode::SideEffectAmbiguous)
            && (self.side_effect != ToolSideEffectStatus::Indeterminate
                || self.retry != ToolRetryDisposition::Unsafe)
        {
            return Err(
                "a side-effect ambiguity requires indeterminate effects and unsafe retry"
                    .to_owned(),
            );
        }
        if self.invocation == ToolInvocationStatus::Rejected
            && (self.transport != ToolTransportStatus::NotStarted
                || self.operation != ToolOperationStatus::NotStarted
                || self.side_effect != ToolSideEffectStatus::NotApplied)
        {
            return Err(
                "a rejected invocation must not start transport, operation, or side effects"
                    .to_owned(),
            );
        }
        if self.transport != ToolTransportStatus::Succeeded
            && self.operation == ToolOperationStatus::Succeeded
        {
            return Err(
                "an operation cannot succeed when tool transport did not succeed".to_owned(),
            );
        }
        if self.operation == ToolOperationStatus::Succeeded
            && self.retry != ToolRetryDisposition::NotNeeded
        {
            return Err("a successful operation cannot request a retry".to_owned());
        }
        if self.retry == ToolRetryDisposition::Safe
            && !matches!(
                self.side_effect,
                ToolSideEffectStatus::NotApplicable | ToolSideEffectStatus::NotApplied
            )
        {
            return Err("safe retry requires proof that no side effect was applied".to_owned());
        }
        for artifact in &self.artifacts {
            if artifact.status == ToolArtifactStatus::Available && artifact.sha256.is_none() {
                return Err(format!(
                    "available artifact '{}' must carry a sha256 digest",
                    artifact.id
                ));
            }
            if artifact.inline_content.is_some() {
                artifact.validate_inline_verification().map(|_| ())?;
            }
        }
        if let Some(observation) = &self.verifier_observation {
            observation.validate()?;
        }
        Ok(())
    }

    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Self::success(serde_json::to_string(value)?))
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    #[must_use]
    pub fn with_failure_code(mut self, failure_code: ToolFailureCode) -> Self {
        self.failure_code = Some(failure_code);
        self
    }

    #[must_use]
    pub fn with_side_effect(mut self, side_effect: ToolSideEffectStatus) -> Self {
        self.side_effect = side_effect;
        self
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence: ToolEvidence) -> Self {
        self.evidence = evidence;
        self
    }
}

fn tool_operation_status_name(status: ToolOperationStatus) -> &'static str {
    match status {
        ToolOperationStatus::NotStarted => "not_started",
        ToolOperationStatus::Succeeded => "succeeded",
        ToolOperationStatus::Failed => "failed",
        ToolOperationStatus::Cancelled => "cancelled",
        ToolOperationStatus::Indeterminate => "indeterminate",
    }
}

fn tool_side_effect_status_name(status: ToolSideEffectStatus) -> &'static str {
    match status {
        ToolSideEffectStatus::NotApplicable => "not_applicable",
        ToolSideEffectStatus::NotApplied => "not_applied",
        ToolSideEffectStatus::Applied => "applied",
        ToolSideEffectStatus::Indeterminate => "indeterminate",
    }
}

fn tool_retry_disposition_name(retry: ToolRetryDisposition) -> &'static str {
    match retry {
        ToolRetryDisposition::NotNeeded => "not_needed",
        ToolRetryDisposition::AfterCorrection => "after_correction",
        ToolRetryDisposition::Safe => "safe",
        ToolRetryDisposition::Unsafe => "unsafe",
        ToolRetryDisposition::NotRetryable => "not_retryable",
    }
}

/// One Host-observed deterministic check attached to an Agent result.
///
/// The outcome remains an observation. Only the canonical Runtime may convert
/// an exactly matching successful observation into an [`EvidenceReceipt`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentCheck {
    pub check_id: String,
    pub verifier: VerifierSpec,
    pub workspace_state: WorkspaceState,
    pub outcome: ToolOutcome,
}

impl AgentCheck {
    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("Agent check id", &self.check_id)?;
        self.verifier.validate()?;
        self.workspace_state.validate()?;
        self.outcome.validate()?;
        if let Some(observation) = &self.outcome.verifier_observation {
            if observation.spec != self.verifier {
                return Err("Agent check verifier does not match its observation".to_owned());
            }
            if observation.workspace_revision != self.workspace_state.revision {
                return Err(
                    "Agent check workspace does not match its verifier observation".to_owned(),
                );
            }
        }
        Ok(())
    }
}

/// Parent-side integration disposition for a writer result.
///
/// A child terminal can only report `awaiting_host`; `integrated` is a Host
/// fact produced after the guarded root-workspace operation commits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterIntegrationStatus {
    #[default]
    NotApplicable,
    AwaitingHost,
    Rejected {
        reason: String,
    },
    Conflict {
        reason: String,
    },
    RecoveryRequired {
        reason: String,
    },
    Integrated {
        integration_id: OperationId,
        writer_commit: String,
        root_workspace_state: WorkspaceState,
    },
}

impl WriterIntegrationStatus {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::NotApplicable | Self::AwaitingHost => Ok(()),
            Self::Rejected { reason }
            | Self::Conflict { reason }
            | Self::RecoveryRequired { reason } => {
                require_agent_text("writer integration reason", reason)
            }
            Self::Integrated {
                integration_id,
                writer_commit,
                root_workspace_state,
            } => {
                require_agent_text("writer integration id", &integration_id.0)?;
                validate_git_object_id("writer commit", writer_commit)?;
                require_known_workspace_state("integrated root workspace", root_workspace_state)
            }
        }
    }
}

/// Durable Host phase that led to cleanup of one isolated Writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterCleanupPhase {
    Binding,
    Child,
    Seal,
    Integration,
    PostIntegration,
}

/// Whether cleanup is operating on an unsealed workspace or an immutable
/// Host-sealed artifact. `Unknown` is fail-closed and can only be retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterArtifactState {
    KnownUnsealed,
    KnownHostSealed {
        final_commit: String,
        diff_sha256: String,
    },
    Unknown,
}

/// Host-observed file scope at the cleanup boundary. Paths themselves are not
/// persisted; the canonical ordered set is bound by its digest and counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterCleanupScope {
    Known {
        workspace_revision: WorkspaceRevision,
        changed_count: u32,
        in_scope_count: u32,
        out_of_scope_count: u32,
        path_set_sha256: String,
    },
    Unknown {
        uncertainty_code: String,
    },
}

/// Stable identity of the repository owner that inspected this cleanup.
/// Destructive execution requires a known digest; failed inspection records
/// only a stable uncertainty code and cannot authorize deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterCleanupOwnership {
    Known { identity_sha256: String },
    Unknown { uncertainty_code: String },
}

/// Destructive authority frozen before any cleanup side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterCleanupMode {
    RemoveExact { expected_branch_commit: String },
    RetainForRecovery { uncertainty_code: String },
}

/// Complete cleanup authority and evidence frozen by the Host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WriterCleanupPlan {
    pub phase: WriterCleanupPhase,
    pub reason_code: String,
    pub ownership: WriterCleanupOwnership,
    pub artifact_state: WriterArtifactState,
    pub scope: WriterCleanupScope,
    pub mode: WriterCleanupMode,
}

impl WriterCleanupPlan {
    pub fn validate(&self) -> Result<(), String> {
        validate_reason_code("writer cleanup reason code", &self.reason_code)?;
        match &self.ownership {
            WriterCleanupOwnership::Known { identity_sha256 } => {
                validate_sha256("writer cleanup owner identity", identity_sha256)?;
            }
            WriterCleanupOwnership::Unknown { uncertainty_code } => {
                validate_reason_code("writer cleanup owner uncertainty", uncertainty_code)?;
            }
        }
        match &self.artifact_state {
            WriterArtifactState::KnownUnsealed | WriterArtifactState::Unknown => {}
            WriterArtifactState::KnownHostSealed {
                final_commit,
                diff_sha256,
            } => {
                validate_git_object_id("cleanup sealed commit", final_commit)?;
                validate_sha256("cleanup sealed diff sha256", diff_sha256)?;
            }
        }
        match &self.scope {
            WriterCleanupScope::Known {
                workspace_revision,
                changed_count,
                in_scope_count,
                out_of_scope_count,
                path_set_sha256,
            } => {
                let WorkspaceRevision::Known { sha256 } = workspace_revision else {
                    return Err("known writer cleanup scope requires a known revision".to_owned());
                };
                validate_sha256("writer cleanup workspace revision", sha256)?;
                if in_scope_count.checked_add(*out_of_scope_count) != Some(*changed_count) {
                    return Err(
                        "writer cleanup scope counts must cover every changed path".to_owned()
                    );
                }
                validate_sha256("writer cleanup path set sha256", path_set_sha256)?;
            }
            WriterCleanupScope::Unknown { uncertainty_code } => {
                validate_reason_code("writer cleanup scope uncertainty", uncertainty_code)?;
            }
        }
        match &self.mode {
            WriterCleanupMode::RemoveExact {
                expected_branch_commit,
            } => {
                validate_git_object_id("cleanup expected branch commit", expected_branch_commit)?;
                if matches!(self.ownership, WriterCleanupOwnership::Unknown { .. })
                    || matches!(self.artifact_state, WriterArtifactState::Unknown)
                    || matches!(self.scope, WriterCleanupScope::Unknown { .. })
                {
                    return Err(
                        "destructive cleanup requires known artifact and scope facts".to_owned(),
                    );
                }
            }
            WriterCleanupMode::RetainForRecovery { uncertainty_code } => {
                validate_reason_code("writer cleanup retention uncertainty", uncertainty_code)?;
                if let WriterCleanupOwnership::Unknown {
                    uncertainty_code: owner_code,
                } = &self.ownership
                    && owner_code != uncertainty_code
                {
                    return Err("writer cleanup owner and retention codes disagree".to_owned());
                }
            }
        }
        Ok(())
    }
}

/// Digest one canonical repository-relative path set without persisting its
/// contents in the cleanup lifecycle.
pub fn writer_path_set_sha256(paths: &[String]) -> Result<String, String> {
    let mut canonical = paths.to_vec();
    canonical.sort();
    canonical.dedup();
    validate_relative_path_list("writer cleanup path", &canonical)?;
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical writer cleanup path set is serializable");
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterRemovalState {
    Removed,
    AlreadyAbsent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterResourceState {
    Removed,
    AlreadyAbsent,
    Retained,
    Unknown,
}

/// Whether Git-side cleanup metadata is conclusively gone. This is separate
/// from the Writer worktree/branch because a tombstone or exact root lock can
/// remain after both user-visible resources were removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterCleanupMetadataState {
    Clear,
    Uncertain,
}

/// Exact result of executing a persisted cleanup plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriterCleanupResult {
    Removed {
        worktree: WriterRemovalState,
        branch: WriterRemovalState,
    },
    AlreadyAbsent,
    Retained {
        worktree: WriterResourceState,
        branch: WriterResourceState,
        metadata: WriterCleanupMetadataState,
        uncertainty_code: String,
    },
}

impl WriterCleanupResult {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Removed { worktree, branch }
                if *worktree == WriterRemovalState::AlreadyAbsent
                    && *branch == WriterRemovalState::AlreadyAbsent =>
            {
                Err("writer cleanup with no removed resource must use already_absent".to_owned())
            }
            Self::Removed { .. } | Self::AlreadyAbsent => Ok(()),
            Self::Retained {
                worktree,
                branch,
                metadata,
                uncertainty_code,
            } => {
                validate_reason_code("writer cleanup result uncertainty", uncertainty_code)?;
                if !matches!(
                    worktree,
                    WriterResourceState::Retained | WriterResourceState::Unknown
                ) && !matches!(
                    branch,
                    WriterResourceState::Retained | WriterResourceState::Unknown
                ) && *metadata == WriterCleanupMetadataState::Clear
                {
                    return Err(
                        "retained writer cleanup requires a retained resource or uncertain cleanup metadata"
                            .to_owned()
                    );
                }
                Ok(())
            }
        }
    }

    #[must_use]
    pub fn retained_for_recovery(&self) -> bool {
        matches!(self, Self::Retained { .. })
    }
}

/// Structured result shared by root and child Agent outcomes.
///
/// Human summary and unresolved notes are advisory. Files, checks, evidence,
/// artifacts, workspace identity, and integration status are Host-observed
/// facts and must survive event replay unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentResultDetails {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<AgentCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<AgentWorkspaceAssignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_state: Option<WorkspaceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_sha256: Option<String>,
    #[serde(default)]
    pub integration: WriterIntegrationStatus,
}

impl AgentResultDetails {
    pub fn validate(&self) -> Result<(), String> {
        validate_relative_path_list("changed file", &self.changed_files)?;
        validate_unique_non_empty_text("unresolved item", &self.unresolved)?;
        validate_unique_non_empty_text(
            "evidence receipt id",
            &self
                .evidence
                .iter()
                .map(|receipt| receipt.id.0.clone())
                .collect::<Vec<_>>(),
        )?;
        for receipt in &self.evidence {
            receipt.validate()?;
        }
        validate_unique_non_empty_text(
            "Agent check id",
            &self
                .checks
                .iter()
                .map(|check| check.check_id.clone())
                .collect::<Vec<_>>(),
        )?;
        for check in &self.checks {
            check.validate()?;
        }
        validate_unique_non_empty_text(
            "Agent artifact id",
            &self
                .artifacts
                .iter()
                .map(|artifact| artifact.id.clone())
                .collect::<Vec<_>>(),
        )?;
        for artifact in &self.artifacts {
            require_agent_text("Agent artifact id", &artifact.id)?;
            if artifact.status == ToolArtifactStatus::Available && artifact.sha256.is_none() {
                return Err(format!(
                    "available Agent artifact '{}' requires a sha256 digest",
                    artifact.id
                ));
            }
            if artifact.inline_content.is_some() {
                artifact.validate_inline_verification().map(|_| ())?;
            }
        }
        if let Some(workspace) = &self.workspace {
            workspace.validate()?;
        }
        if let Some(workspace_state) = &self.workspace_state {
            workspace_state.validate()?;
        }
        self.integration.validate()?;
        let workspace_access = self.workspace.as_ref().map(|workspace| workspace.access);
        match (workspace_access, &self.integration) {
            (
                Some(AgentWorkspaceAccess::ReadOnly) | None,
                WriterIntegrationStatus::AwaitingHost
                | WriterIntegrationStatus::Rejected { .. }
                | WriterIntegrationStatus::Conflict { .. }
                | WriterIntegrationStatus::RecoveryRequired { .. }
                | WriterIntegrationStatus::Integrated { .. },
            ) => {
                return Err(
                    "only an isolated writer workspace may carry writer integration state"
                        .to_owned(),
                );
            }
            (Some(AgentWorkspaceAccess::IsolatedWrite), WriterIntegrationStatus::NotApplicable) => {
                return Err(
                    "isolated writer result must report an explicit integration state".to_owned(),
                );
            }
            _ => {}
        }
        match workspace_access {
            Some(AgentWorkspaceAccess::IsolatedWrite) => {
                let workspace = self
                    .workspace
                    .as_ref()
                    .expect("isolated writer access came from a workspace");
                let workspace_state = self.workspace_state.as_ref().ok_or_else(|| {
                    "isolated writer result requires its sealed workspace state".to_owned()
                })?;
                require_known_workspace_state("sealed writer workspace", workspace_state)?;
                let base_commit = self
                    .base_commit
                    .as_deref()
                    .ok_or_else(|| "isolated writer result requires its base commit".to_owned())?;
                let final_commit = self.final_commit.as_deref().ok_or_else(|| {
                    "isolated writer result requires its sealed final commit".to_owned()
                })?;
                let diff_sha256 = self.diff_sha256.as_deref().ok_or_else(|| {
                    "isolated writer result requires its sealed diff digest".to_owned()
                })?;
                validate_git_object_id("writer result base commit", base_commit)?;
                validate_git_object_id("writer result final commit", final_commit)?;
                validate_sha256("writer result diff sha256", diff_sha256)?;
                if workspace.base_commit != base_commit {
                    return Err(
                        "writer result base commit must match its workspace assignment".to_owned(),
                    );
                }
                if base_commit == final_commit {
                    return Err(
                        "writer result final commit must differ from its base commit".to_owned(),
                    );
                }
            }
            Some(AgentWorkspaceAccess::ReadOnly) | None => {
                if self.base_commit.is_some()
                    || self.final_commit.is_some()
                    || self.diff_sha256.is_some()
                {
                    return Err(
                        "non-writer Agent result cannot carry writer commit or diff fields"
                            .to_owned(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptEntry {
    System {
        prompt: SystemPrompt,
    },
    User {
        content: String,
    },
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ModelToolCall>,
    },
    Tool {
        call_id: String,
        name: String,
        outcome: Box<ToolOutcome>,
    },
    ChildOutcome {
        call_id: String,
        child_run_id: RunId,
        outcome: Box<AgentOutcome>,
        handoff_content: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CanonicalTranscript {
    pub entries: Vec<TranscriptEntry>,
}

impl CanonicalTranscript {
    /// Validate a settled transcript before it becomes the starting history
    /// of another run. Tool results bind to the exact latest unresolved
    /// assistant call; orphan, duplicate, or renamed results are corruption.
    pub fn validate_complete_tool_history(&self) -> Result<(), String> {
        let mut pending = Vec::<ModelToolCall>::new();
        for (entry_index, entry) in self.entries.iter().enumerate() {
            match entry {
                TranscriptEntry::Assistant { tool_calls, .. } => {
                    if !pending.is_empty() {
                        return Err(format!(
                            "assistant transcript entry {entry_index} begins before {} tool result(s) are settled",
                            pending.len()
                        ));
                    }
                    validate_model_tool_calls(tool_calls)?;
                    pending = tool_calls.clone();
                }
                TranscriptEntry::Tool {
                    call_id,
                    name,
                    outcome,
                } => {
                    outcome.validate()?;
                    let Some(position) = pending.iter().position(|call| call.id == *call_id) else {
                        return Err(format!(
                            "tool transcript entry {entry_index} has orphan or duplicate call id '{call_id}'"
                        ));
                    };
                    if pending[position].name != *name {
                        return Err(format!(
                            "tool transcript entry {entry_index} names '{name}' instead of '{}' for call id '{call_id}'",
                            pending[position].name
                        ));
                    }
                    pending.remove(position);
                }
                TranscriptEntry::System { .. }
                | TranscriptEntry::User { .. }
                | TranscriptEntry::ChildOutcome { .. } => {}
            }
        }
        if !pending.is_empty() {
            return Err(format!(
                "settled transcript ends with {} unresolved tool call(s)",
                pending.len()
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn project_messages(&self) -> Vec<ModelMessage> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                TranscriptEntry::System { .. } => None,
                TranscriptEntry::User { content } => Some(ModelMessage::User {
                    content: content.clone(),
                }),
                TranscriptEntry::Assistant {
                    content,
                    reasoning_content,
                    tool_calls,
                } => Some(ModelMessage::Assistant {
                    content: content.clone(),
                    reasoning_content: reasoning_content.clone(),
                    tool_calls: tool_calls.clone(),
                }),
                TranscriptEntry::Tool {
                    call_id,
                    name,
                    outcome,
                } => Some(ModelMessage::Tool {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    content: outcome.model_content(),
                }),
                TranscriptEntry::ChildOutcome {
                    handoff_content, ..
                } => Some(ModelMessage::User {
                    content: handoff_content.clone(),
                }),
            })
            .collect()
    }
}

pub fn validate_model_tool_calls(calls: &[ModelToolCall]) -> Result<(), String> {
    let mut ids = HashSet::with_capacity(calls.len());
    for call in calls {
        if call.id.trim().is_empty() {
            return Err("tool call id must not be empty".to_owned());
        }
        if call.name.trim().is_empty() {
            return Err(format!("tool call '{}' has an empty name", call.id));
        }
        if !ids.insert(call.id.as_str()) {
            return Err(format!("duplicate tool call id '{}'", call.id));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRequestAccounting {
    pub started: u64,
    pub completed: u64,
    pub in_flight: u64,
    pub retries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceUsage {
    pub surface: ApiSurface,
    pub model: String,
    pub response_count: u64,
    pub usage_response_count: u64,
    pub usage: Usage,
    pub cost_nanousd: u64,
    pub cost_nanocny: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAccounting {
    pub hard_request_limit: Option<u32>,
    pub root: ActorRequestAccounting,
    pub child: ActorRequestAccounting,
    pub transport_retries: u64,
    pub runtime_retries: u64,
    pub sealed_denied: u64,
    /// Number of physical API admission attempts rejected because the shared
    /// hard request budget had no remaining slot.
    pub exhausted_denied: u64,
    /// True only when physical API admission rejected at least one request as
    /// exhausted. Reaching `hard_request_limit` without attempting another
    /// physical request does not set this flag.
    pub budget_exhausted: bool,
    pub sealed: bool,
    pub complete: bool,
    pub usage_complete: bool,
    pub usage_missing: bool,
    pub usage_incomplete: bool,
    pub billing_unknown: bool,
    pub unpriced: bool,
    pub usage_responses: u64,
    pub usage_missing_responses: u64,
    pub incomplete_responses: u64,
    pub billing_unknown_attempts: u64,
    pub unpriced_usage_responses: u64,
    pub records_after_seal: u64,
    pub usage: Usage,
    #[serde(default)]
    pub surface_usage: Vec<SurfaceUsage>,
    pub cost_nanousd: u64,
    pub cost_nanocny: u64,
}

impl ModelAccounting {
    #[must_use]
    pub fn total_started(&self) -> u64 {
        self.root.started.saturating_add(self.child.started)
    }

    #[must_use]
    pub fn total_completed(&self) -> u64 {
        self.root.completed.saturating_add(self.child.completed)
    }

    #[must_use]
    pub fn total_in_flight(&self) -> u64 {
        self.root.in_flight.saturating_add(self.child.in_flight)
    }

    /// Merge accounting from a newly opened backend process into the durable
    /// baseline recovered from the run log.
    pub fn add_assign(&mut self, other: &Self) {
        if self.total_started() == 0
            && self.usage == Usage::default()
            && self.surface_usage.is_empty()
            && self.cost_nanousd == 0
            && self.cost_nanocny == 0
        {
            *self = other.clone();
            return;
        }
        self.hard_request_limit = self.hard_request_limit.or(other.hard_request_limit);
        add_actor_accounting(&mut self.root, other.root);
        add_actor_accounting(&mut self.child, other.child);
        self.transport_retries = self
            .transport_retries
            .saturating_add(other.transport_retries);
        self.runtime_retries = self.runtime_retries.saturating_add(other.runtime_retries);
        self.sealed_denied = self.sealed_denied.saturating_add(other.sealed_denied);
        self.exhausted_denied = self.exhausted_denied.saturating_add(other.exhausted_denied);
        self.budget_exhausted |= other.budget_exhausted;
        self.sealed |= other.sealed;
        self.complete &= other.complete;
        self.usage_complete &= other.usage_complete;
        self.usage_missing |= other.usage_missing;
        self.usage_incomplete |= other.usage_incomplete;
        self.billing_unknown |= other.billing_unknown;
        self.unpriced |= other.unpriced;
        self.usage_responses = self.usage_responses.saturating_add(other.usage_responses);
        self.usage_missing_responses = self
            .usage_missing_responses
            .saturating_add(other.usage_missing_responses);
        self.incomplete_responses = self
            .incomplete_responses
            .saturating_add(other.incomplete_responses);
        self.billing_unknown_attempts = self
            .billing_unknown_attempts
            .saturating_add(other.billing_unknown_attempts);
        self.unpriced_usage_responses = self
            .unpriced_usage_responses
            .saturating_add(other.unpriced_usage_responses);
        self.records_after_seal = self
            .records_after_seal
            .saturating_add(other.records_after_seal);
        self.usage.add_assign(other.usage);
        self.cost_nanousd = self.cost_nanousd.saturating_add(other.cost_nanousd);
        self.cost_nanocny = self.cost_nanocny.saturating_add(other.cost_nanocny);
        for surface in &other.surface_usage {
            if let Some(existing) = self.surface_usage.iter_mut().find(|existing| {
                existing.surface == surface.surface && existing.model == surface.model
            }) {
                existing.response_count = existing
                    .response_count
                    .saturating_add(surface.response_count);
                existing.usage_response_count = existing
                    .usage_response_count
                    .saturating_add(surface.usage_response_count);
                existing.usage.add_assign(surface.usage);
                existing.cost_nanousd = existing.cost_nanousd.saturating_add(surface.cost_nanousd);
                existing.cost_nanocny = existing.cost_nanocny.saturating_add(surface.cost_nanocny);
            } else {
                self.surface_usage.push(surface.clone());
            }
        }
    }
}

fn add_actor_accounting(target: &mut ActorRequestAccounting, other: ActorRequestAccounting) {
    target.started = target.started.saturating_add(other.started);
    target.completed = target.completed.saturating_add(other.completed);
    target.in_flight = target.in_flight.saturating_add(other.in_flight);
    target.retries = target.retries.saturating_add(other.retries);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorCategory {
    Transport,
    Timeout,
    StreamStall,
    RateLimit,
    Authentication,
    Protocol,
    Service,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTimeoutPhase {
    Model,
    Tool,
    Run,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAmbiguityPhase {
    ModelRequest,
    ToolExecution,
    HostVerification,
    ChildRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAmbiguity {
    pub phase: RecoveryAmbiguityPhase,
    pub action_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeFailure {
    EmptyModelOutput,
    IncompleteModelStream,
    OutputLimit,
    ContentFiltered,
    InsufficientSystemResource,
    ModelRequestBudgetExceeded {
        limit: u32,
    },
    ApiRequestBudgetExceeded {
        limit: u32,
    },
    TurnBudgetExceeded {
        limit: u32,
    },
    ToolBudgetExceeded {
        limit: u32,
    },
    DepthLimit {
        limit: u8,
    },
    Timeout {
        phase: RuntimeTimeoutPhase,
        timeout_ms: u64,
    },
    InvalidModelOutput {
        message: String,
    },
    AccountingIncomplete {
        message: String,
    },
    ContextLimitExceeded {
        estimated_tokens: u64,
        hard_input_tokens: u64,
    },
    Model {
        code: String,
        category: ModelErrorCategory,
        message: String,
        retryable: bool,
    },
    Store {
        message: String,
    },
    Join {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TerminalState {
    Completed {
        message: String,
        decision: CompletionDecision,
    },
    Blocked {
        reason: String,
    },
    Failed {
        failure: RuntimeFailure,
    },
    Cancelled,
    Interrupted,
    RecoveryRequired {
        ambiguity: RecoveryAmbiguity,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentOutcome {
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
    pub terminal: TerminalState,
    pub accounting: ModelAccounting,
    pub runtime_model_requests: u32,
    pub runtime_retries: u32,
    pub tool_calls: u32,
    pub details: AgentResultDetails,
}

impl AgentOutcome {
    pub fn validate(&self) -> Result<(), String> {
        require_agent_text("Agent outcome run id", &self.run_id.0)?;
        if self
            .parent_run_id
            .as_ref()
            .is_some_and(|parent| parent == &self.run_id)
        {
            return Err("Agent outcome cannot be its own parent".to_owned());
        }
        self.details.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeEventId(pub String);

impl fmt::Display for RuntimeEventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl RuntimeEventId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    #[must_use]
    pub fn run_created() -> Self {
        Self("run_created".to_owned())
    }

    #[must_use]
    pub fn terminal() -> Self {
        Self("terminal".to_owned())
    }
}

impl Default for RuntimeEventId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttemptId(pub String);

impl AttemptId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for AttemptId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub String);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for OperationId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for OperationId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InteractionId(pub String);

impl InteractionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for InteractionId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for InteractionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for InteractionId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandId(pub String);

impl CommandId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for CommandId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for CommandId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for CommandId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRisk {
    Routine,
    Elevated,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalPrompt {
    pub title: String,
    pub description: String,
    pub risk: ApprovalRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInputOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInputQuestion {
    pub header: String,
    pub id: String,
    pub question: String,
    pub options: Vec<UserInputOption>,
    #[serde(default)]
    pub allow_free_text: bool,
    #[serde(default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInputRequest {
    pub questions: Vec<UserInputQuestion>,
}

impl UserInputRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.questions.is_empty() || self.questions.len() > 3 {
            return Err("questions must contain 1 to 3 items".to_owned());
        }
        let mut question_ids = HashSet::new();
        for question in &self.questions {
            if question.header.trim().is_empty()
                || question.id.trim().is_empty()
                || question.question.trim().is_empty()
            {
                return Err("question header, id, and text must not be empty".to_owned());
            }
            if !question_ids.insert(question.id.as_str()) {
                return Err(format!("duplicate question id '{}'", question.id));
            }
            if !(2..=4).contains(&question.options.len()) {
                return Err("each question must contain 2 to 4 options".to_owned());
            }
            if question.options.iter().any(|option| {
                option.label.trim().is_empty() || option.description.trim().is_empty()
            }) {
                return Err("option label and description must not be empty".to_owned());
            }
            let mut option_labels = HashSet::new();
            for option in &question.options {
                if !option_labels.insert(option.label.as_str()) {
                    return Err(format!(
                        "question '{}' contains duplicate option label '{}'",
                        question.id, option.label
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserInputAnswer {
    pub id: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserInteractionPrompt {
    Approval {
        prompt: ToolApprovalPrompt,
        arguments: Value,
    },
    UserInput {
        request: UserInputRequest,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInteractionRequest {
    pub interaction_id: InteractionId,
    pub operation_id: OperationId,
    pub call_id: String,
    pub tool_name: String,
    pub prompt: UserInteractionPrompt,
}

impl UserInteractionRequest {
    pub fn validate_response(&self, response: &UserInteractionResponse) -> Result<(), String> {
        match (&self.prompt, response) {
            (UserInteractionPrompt::Approval { .. }, UserInteractionResponse::Approved)
            | (UserInteractionPrompt::Approval { .. }, UserInteractionResponse::Denied { .. })
            | (UserInteractionPrompt::Approval { .. }, UserInteractionResponse::Cancelled)
            | (UserInteractionPrompt::UserInput { .. }, UserInteractionResponse::Cancelled) => {
                Ok(())
            }
            (
                UserInteractionPrompt::UserInput { request },
                UserInteractionResponse::Answered { answers },
            ) => validate_user_input_answers(request, answers),
            (UserInteractionPrompt::Approval { .. }, UserInteractionResponse::Answered { .. }) => {
                Err("approval interaction cannot be resolved with user-input answers".to_owned())
            }
            (
                UserInteractionPrompt::UserInput { .. },
                UserInteractionResponse::Approved | UserInteractionResponse::Denied { .. },
            ) => Err("user-input interaction requires answers or cancellation".to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserInteractionResponse {
    Approved,
    Denied {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Answered {
        answers: Vec<UserInputAnswer>,
    },
    Cancelled,
}

fn validate_user_input_answers(
    request: &UserInputRequest,
    answers: &[UserInputAnswer],
) -> Result<(), String> {
    request.validate()?;
    let mut seen_answers = HashSet::new();
    for answer in answers {
        let Some(question) = request
            .questions
            .iter()
            .find(|question| question.id == answer.id)
        else {
            return Err(format!(
                "answer references unknown question id '{}'",
                answer.id
            ));
        };
        if answer.value.trim().is_empty() || answer.label.trim().is_empty() {
            return Err(format!(
                "answer for question '{}' must contain a label and value",
                answer.id
            ));
        }
        if !seen_answers.insert((answer.id.as_str(), answer.label.as_str())) {
            return Err(format!(
                "question '{}' contains duplicate answer '{}'",
                answer.id, answer.label
            ));
        }
        let known_option = question
            .options
            .iter()
            .find(|option| option.label == answer.label);
        match known_option {
            Some(option) if answer.value != option.label => {
                return Err(format!(
                    "answer '{}' for question '{}' changed the selected option value",
                    answer.label, answer.id
                ));
            }
            None if !question.allow_free_text => {
                return Err(format!(
                    "question '{}' does not allow a free-text answer",
                    answer.id
                ));
            }
            _ => {}
        }
    }
    for question in &request.questions {
        let count = answers
            .iter()
            .filter(|answer| answer.id == question.id)
            .count();
        if count == 0 {
            return Err(format!("question '{}' has no answer", question.id));
        }
        if !question.multi_select && count > 1 {
            return Err(format!(
                "question '{}' does not allow multiple answers",
                question.id
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableControlAction {
    Interrupt,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    ReadOnly,
    MayWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAttemptFailure {
    pub code: String,
    pub category: ModelErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub retry_safe: bool,
    pub actionable_output: bool,
    pub response: ModelResponseEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRetryStopReason {
    ActionableOutput,
    UnsafeReplay,
    NotRetryable,
    FailureChanged,
    RetryLimitReached,
    ModelRequestBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedModelRetry {
    pub attempt_id: AttemptId,
    pub request: Box<ModelRequest>,
}

/// The durable host decision made after one model attempt fails.
///
/// A retry is prepared inside the same canonical event that commits the
/// failed attempt. There is deliberately no standalone retry-prepared event,
/// so replay can neither lose an admitted retry nor invent one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ModelRetryDecision {
    Stop { reason: ModelRetryStopReason },
    Retry { prepared: PreparedModelRetry },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingRuntimeEvent {
    pub event_id: RuntimeEventId,
    pub event: RuntimeEventKind,
}

impl PendingRuntimeEvent {
    #[must_use]
    pub fn new(event: RuntimeEventKind) -> Self {
        Self {
            event_id: RuntimeEventId::new(),
            event,
        }
    }

    #[must_use]
    pub fn terminal(outcome: AgentOutcome) -> Self {
        Self {
            event_id: RuntimeEventId::terminal(),
            event: RuntimeEventKind::Terminal {
                outcome: Box::new(outcome),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEventKind {
    RunCreated {
        request: Box<RunRequest>,
    },
    ContextCompactionCommitted {
        projection: Box<ContextProjection>,
        tools: Vec<ToolDefinition>,
        accounting: Box<ModelAccounting>,
        before_tokens: u64,
        after_tokens: u64,
    },
    ModelRequestPrepared {
        attempt_id: AttemptId,
        request: Box<ModelRequest>,
    },
    ModelRequestInFlight {
        attempt_id: AttemptId,
    },
    ModelRequestFailed {
        attempt_id: AttemptId,
        failure: ModelAttemptFailure,
        accounting: Box<ModelAccounting>,
        retry: ModelRetryDecision,
    },
    ModelResponseCommitted {
        attempt_id: AttemptId,
        output: Box<ModelOutput>,
        accounting: Box<ModelAccounting>,
    },
    ContentDelta {
        attempt_id: AttemptId,
        index: u64,
        delta: String,
    },
    ReasoningDelta {
        attempt_id: AttemptId,
        index: u64,
        delta: String,
    },
    ToolPrepared {
        operation_id: OperationId,
        invocation: ToolInvocation,
        workspace_access: WorkspaceAccess,
    },
    ToolExecutionStarted {
        operation_id: OperationId,
    },
    InteractionRequested {
        request: UserInteractionRequest,
    },
    InteractionResolved {
        command_id: CommandId,
        interaction_id: InteractionId,
        response: UserInteractionResponse,
    },
    ToolOutcomeCommitted {
        operation_id: OperationId,
        call_id: String,
        name: String,
        outcome: Box<ToolOutcome>,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace_state: Option<WorkspaceState>,
    },
    WorkspaceObserved {
        workspace_state: WorkspaceState,
    },
    CompletionProposed {
        candidate: CompletionCandidate,
    },
    HostVerificationPrepared {
        verification_id: VerificationId,
        candidate: CompletionCandidate,
        acceptance_id: AcceptanceId,
        verifier: VerifierSpec,
        workspace_state_before: WorkspaceState,
    },
    HostVerificationStarted {
        verification_id: VerificationId,
    },
    HostVerificationCommitted {
        verification_id: VerificationId,
        outcome: Box<ToolOutcome>,
        #[serde(skip_serializing_if = "Option::is_none")]
        receipt: Option<Box<EvidenceReceipt>>,
        workspace_state_after: WorkspaceState,
    },
    CompletionRejected {
        rejection: CompletionRejection,
    },
    AgentTaskPrepared {
        task: Box<AgentTask>,
    },
    AgentWorkspaceCreated {
        task_id: AgentTaskId,
        assignment: AgentWorkspaceAssignment,
        writer_workspace_state: WorkspaceState,
    },
    AgentSealPrepared {
        task_id: AgentTaskId,
        base_commit: String,
        writer_workspace_state_before: WorkspaceState,
    },
    AgentSealCommitted {
        task_id: AgentTaskId,
        final_commit: String,
        diff_sha256: String,
        changed_files: Vec<String>,
        writer_workspace_state_after: WorkspaceState,
    },
    ChildStarted {
        task_id: AgentTaskId,
        call_id: String,
        child_run_id: RunId,
        depth: u8,
    },
    AgentResultCollected {
        task_id: AgentTaskId,
        outcome: Box<AgentOutcome>,
    },
    AgentIntegrationPrepared {
        task_id: AgentTaskId,
        integration_id: OperationId,
        base_commit: String,
        writer_commit: String,
        diff_sha256: String,
        expected_root_workspace_state: WorkspaceState,
    },
    AgentIntegrationStarted {
        task_id: AgentTaskId,
        integration_id: OperationId,
    },
    AgentIntegrationFailed {
        task_id: AgentTaskId,
        integration_id: OperationId,
        status: WriterIntegrationStatus,
        root_workspace_state: WorkspaceState,
    },
    AgentIntegrationCommitted {
        task_id: AgentTaskId,
        integration_id: OperationId,
        root_head_commit: String,
        root_workspace_state_after: WorkspaceState,
    },
    AgentCleanupPrepared {
        task_id: AgentTaskId,
        plan: Box<WriterCleanupPlan>,
    },
    AgentCleanupCommitted {
        task_id: AgentTaskId,
        result: WriterCleanupResult,
    },
    ChildFinished {
        call_id: String,
        outcome: Box<AgentOutcome>,
        accounting: Box<ModelAccounting>,
        handoff_content: String,
    },
    SteerQueued {
        command_id: CommandId,
        content: String,
    },
    SteerApplied {
        command_id: CommandId,
        content: String,
    },
    ControlRequested {
        command_id: CommandId,
        action: DurableControlAction,
    },
    Terminal {
        outcome: Box<AgentOutcome>,
    },
}

impl RuntimeEventKind {
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal { .. })
    }

    /// Validate fields whose exact identity is required by the M6-A
    /// orchestration reducer. Cross-event ordering and equality are reducer
    /// responsibilities; this method rejects malformed individual facts.
    pub fn validate_agent_lifecycle_payload(&self) -> Result<(), String> {
        match self {
            Self::AgentTaskPrepared { task } => task.validate(),
            Self::AgentWorkspaceCreated {
                task_id,
                assignment,
                writer_workspace_state,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                assignment.validate()?;
                if assignment.access != AgentWorkspaceAccess::IsolatedWrite {
                    return Err(
                        "Agent workspace creation requires an isolated writer assignment"
                            .to_owned(),
                    );
                }
                require_known_workspace_state("created writer workspace", writer_workspace_state)
            }
            Self::AgentSealPrepared {
                task_id,
                base_commit,
                writer_workspace_state_before,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                validate_git_object_id("writer base commit", base_commit)?;
                require_known_workspace_state(
                    "writer workspace before seal",
                    writer_workspace_state_before,
                )
            }
            Self::AgentSealCommitted {
                task_id,
                final_commit,
                diff_sha256,
                changed_files,
                writer_workspace_state_after,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                validate_git_object_id("writer final commit", final_commit)?;
                validate_sha256("writer diff sha256", diff_sha256)?;
                validate_relative_path_list("changed file", changed_files)?;
                if changed_files.is_empty() {
                    return Err("writer seal requires at least one changed file".to_owned());
                }
                require_known_workspace_state(
                    "writer workspace after seal",
                    writer_workspace_state_after,
                )
            }
            Self::ChildStarted {
                task_id,
                call_id,
                child_run_id,
                depth,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                require_agent_text("Agent call id", call_id)?;
                require_agent_text("child run id", &child_run_id.0)?;
                if *depth == 0 {
                    return Err("child Agent depth must be greater than zero".to_owned());
                }
                Ok(())
            }
            Self::AgentResultCollected { task_id, outcome } => {
                require_agent_text("Agent task id", &task_id.0)?;
                outcome.validate()
            }
            Self::AgentIntegrationPrepared {
                task_id,
                integration_id,
                base_commit,
                writer_commit,
                diff_sha256,
                expected_root_workspace_state,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                require_agent_text("writer integration id", &integration_id.0)?;
                validate_git_object_id("integration base commit", base_commit)?;
                validate_git_object_id("integration writer commit", writer_commit)?;
                if base_commit == writer_commit {
                    return Err(
                        "writer integration commit must differ from its base commit".to_owned()
                    );
                }
                validate_sha256("integration diff sha256", diff_sha256)?;
                require_known_workspace_state(
                    "expected root workspace",
                    expected_root_workspace_state,
                )
            }
            Self::AgentIntegrationStarted {
                task_id,
                integration_id,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                require_agent_text("writer integration id", &integration_id.0)
            }
            Self::AgentIntegrationFailed {
                task_id,
                integration_id,
                status,
                root_workspace_state,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                require_agent_text("writer integration id", &integration_id.0)?;
                match status {
                    WriterIntegrationStatus::Rejected { .. }
                    | WriterIntegrationStatus::Conflict { .. }
                    | WriterIntegrationStatus::RecoveryRequired { .. } => status.validate()?,
                    WriterIntegrationStatus::NotApplicable
                    | WriterIntegrationStatus::AwaitingHost
                    | WriterIntegrationStatus::Integrated { .. } => {
                        return Err(
                            "failed writer integration requires rejected, conflict, or recovery-required status"
                                .to_owned(),
                        );
                    }
                }
                require_known_workspace_state(
                    "root workspace after failed integration",
                    root_workspace_state,
                )
            }
            Self::AgentIntegrationCommitted {
                task_id,
                integration_id,
                root_head_commit,
                root_workspace_state_after,
            } => {
                require_agent_text("Agent task id", &task_id.0)?;
                require_agent_text("writer integration id", &integration_id.0)?;
                validate_git_object_id("integrated root commit", root_head_commit)?;
                require_known_workspace_state(
                    "root workspace after integration",
                    root_workspace_state_after,
                )
            }
            Self::AgentCleanupPrepared { task_id, plan } => {
                require_agent_text("Agent task id", &task_id.0)?;
                plan.validate()
            }
            Self::AgentCleanupCommitted { task_id, result } => {
                require_agent_text("Agent task id", &task_id.0)?;
                result.validate()
            }
            Self::ChildFinished { outcome, .. } | Self::Terminal { outcome } => outcome.validate(),
            _ => Ok(()),
        }
    }
}

fn require_agent_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    if value.contains('\0') {
        return Err(format!("{label} must not contain NUL"));
    }
    Ok(())
}

fn validate_reason_code(label: &str, value: &str) -> Result<(), String> {
    require_agent_text(label, value)?;
    if value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(format!(
            "{label} must be at most 96 lowercase ASCII letters, digits, or underscores"
        ));
    }
    Ok(())
}

fn validate_git_object_id(label: &str, value: &str) -> Result<(), String> {
    require_agent_text(label, value)?;
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{label} must be a 40- or 64-character Git object id"
        ));
    }
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<(), String> {
    require_agent_text(label, value)?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{label} must be a 64-character hexadecimal digest"));
    }
    Ok(())
}

fn format_prefixed_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn validate_unique_non_empty_text(label: &str, values: &[String]) -> Result<(), String> {
    let mut unique = HashSet::new();
    for value in values {
        require_agent_text(label, value)?;
        if !unique.insert(value.as_str()) {
            return Err(format!("{label} '{value}' appears more than once"));
        }
    }
    Ok(())
}

fn validate_relative_path_list(label: &str, values: &[String]) -> Result<(), String> {
    validate_unique_non_empty_text(label, values)?;
    for value in values {
        let path = Path::new(value);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!(
                "{label} '{value}' must stay relative to its workspace"
            ));
        }
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("{label} values must be sorted in canonical order"));
    }
    Ok(())
}

fn require_known_workspace_state(label: &str, state: &WorkspaceState) -> Result<(), String> {
    state.validate()?;
    if matches!(&state.revision, WorkspaceRevision::Unknown { .. }) {
        return Err(format!("{label} requires a known workspace revision"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRuntimeEvent {
    pub schema_version: u32,
    pub run_id: RunId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
    pub event_id: RuntimeEventId,
    pub sequence: u64,
    pub occurred_at_unix_ms: u64,
    pub event: RuntimeEventKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{
        CompletionCandidateId, CompletionRequiredTransition, EvidenceSealRejection, TaskDefinition,
        TaskGenerationId, VerifierPlan, VerifierStep,
    };

    #[test]
    fn m14_observer_corpus_uses_protocol_valid_typed_facts() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../eval/fixtures/m14-observer-conformance-v1.json"
        ))
        .expect("M14 observer corpus JSON");
        assert_eq!(
            corpus["stable_tool_outcome_fields"],
            serde_json::json!([
                "failure_code",
                "invocation",
                "transport",
                "operation",
                "side_effect",
                "retry"
            ])
        );
        let cases = corpus["cases"].as_array().expect("M14 cases");
        assert_eq!(cases.len(), 12);
        for case in cases {
            if let Some(events) = case.get("events").and_then(Value::as_array) {
                for event in events {
                    let Some(outcome) = event.get("outcome") else {
                        continue;
                    };
                    let outcome: ToolOutcome = serde_json::from_value(outcome.clone())
                        .expect("corpus ToolOutcome must deserialize through protocol owner");
                    outcome
                        .validate()
                        .expect("corpus ToolOutcome must satisfy protocol invariants");
                }
            }
            if case["kind"] == "writer_assignment" {
                let assignment: AgentWorkspaceAssignment =
                    serde_json::from_value(case["observed"].clone())
                        .expect("corpus Writer assignment must deserialize");
                assert_eq!(
                    assignment.validate().is_ok(),
                    case["expected_valid"].as_bool().expect("expected_valid"),
                    "{}",
                    case["case_id"].as_str().expect("case id")
                );
            }
        }
    }

    fn known_workspace(generation: u64, digest: char) -> WorkspaceState {
        WorkspaceState {
            generation,
            revision: WorkspaceRevision::Known {
                sha256: digest.to_string().repeat(64),
            },
        }
    }

    fn child_contract(child_run_id: &RunId) -> TaskContract {
        TaskContract {
            generation_id: TaskGenerationId::from(child_run_id.0.clone()),
            definition: TaskDefinition::host("审计协议边界"),
        }
    }

    fn writer_assignment() -> AgentWorkspaceAssignment {
        AgentWorkspaceAssignment {
            access: AgentWorkspaceAccess::IsolatedWrite,
            root_workspace: "/workspace/root".to_owned(),
            base_commit: "a".repeat(40),
            worktree_path: Some("/workspace/worktrees/task-1".to_owned()),
            root_branch: Some("refs/heads/main".to_owned()),
            branch: Some("codex/task-1".to_owned()),
            allowed_paths: vec![
                "crates/protocol".to_owned(),
                "docs/product/ROADMAP.md".to_owned(),
            ],
            owner_token: Some("owner-task-1".to_owned()),
        }
    }

    fn child_task() -> AgentTask {
        let root_run_id = RunId::from("root-run");
        let child_run_id = RunId::from("child-run");
        AgentTask {
            task_id: AgentTaskId::from("task-1"),
            root_run_id: root_run_id.clone(),
            parent_run_id: root_run_id,
            child_run_id: child_run_id.clone(),
            call_id: "call-1".to_owned(),
            role: "协议审计".to_owned(),
            task_contract: child_contract(&child_run_id),
            workspace: AgentWorkspaceAssignment {
                access: AgentWorkspaceAccess::ReadOnly,
                root_workspace: "/workspace/root".to_owned(),
                base_commit: "a".repeat(40),
                worktree_path: None,
                root_branch: None,
                branch: None,
                allowed_paths: Vec::new(),
                owner_token: None,
            },
            model: "deepseek-v4-flash".to_owned(),
            reasoning_effort: ReasoningEffort::High,
            max_output_tokens: Some(262_144),
            context_policy: ContextPolicy {
                hard_input_tokens: 90_000,
            },
            route: ModelRouteAudit {
                profile: ModelRouteProfile::FixedActor,
                policy_version: "fixture_fixed_actor_v1".to_owned(),
                reason_code: "fixed_read_only_investigation".to_owned(),
            },
            tool_policy: ToolPolicy {
                enabled: true,
                allowed: Some(vec!["read".to_owned()]),
                denied: Vec::new(),
            },
            limits: RunLimits {
                max_turns: 8,
                max_model_requests: 8,
                max_tool_calls: 16,
                max_depth: 2,
                ..RunLimits::default()
            },
            deadline_unix_ms: Some(1_800_000_000_000),
            expected_artifact: "结构化协议审计结果".to_owned(),
        }
    }

    #[test]
    fn current_agent_protocol_schema_versions_are_explicit_cutovers() {
        assert_eq!(MIN_SUPPORTED_AGENT_RUNTIME_EVENT_SCHEMA_VERSION, 19);
        assert_eq!(AGENT_RUNTIME_EVENT_SCHEMA_VERSION, 19);
    }

    #[test]
    fn runtime_event_v14_completion_rejection_round_trips_all_typed_fields() {
        let event = RuntimeEventKind::CompletionRejected {
            rejection: CompletionRejection {
                candidate_id: CompletionCandidateId::from("candidate-1"),
                generation_id: TaskGenerationId::from("generation-1"),
                unmet_acceptance_ids: vec![AcceptanceId::from("tests")],
                cause: EvidenceSealRejection::VerifierFailed,
                required_transition: CompletionRequiredTransition::EffectiveWorkspaceMutation,
                reason: "确定性 verifier 失败".to_owned(),
            },
        };
        let encoded = serde_json::to_value(&event).expect("serialize RuntimeEvent v14");
        assert_eq!(encoded["kind"], "completion_rejected");
        assert_eq!(encoded["rejection"]["generation_id"], "generation-1");
        assert_eq!(encoded["rejection"]["cause"], "verifier_failed");
        assert_eq!(
            encoded["rejection"]["required_transition"],
            "effective_workspace_mutation"
        );
        assert_eq!(
            serde_json::from_value::<RuntimeEventKind>(encoded).expect("round-trip event"),
            event
        );
    }

    #[test]
    fn workspace_assignment_separates_read_only_and_isolated_writer_authority() {
        let writer = writer_assignment();
        assert!(writer.validate().is_ok());
        assert_eq!(writer.execution_workspace(), "/workspace/worktrees/task-1");

        let mut unsafe_scope = writer.clone();
        unsafe_scope.allowed_paths = vec!["../outside".to_owned()];
        assert!(unsafe_scope.validate().is_err());

        let mut missing_owner = writer.clone();
        missing_owner.owner_token = None;
        assert!(missing_owner.validate().is_err());

        let mut read_only = writer;
        read_only.access = AgentWorkspaceAccess::ReadOnly;
        assert!(read_only.validate().is_err());

        read_only.worktree_path = None;
        read_only.root_branch = None;
        read_only.branch = None;
        read_only.owner_token = None;
        assert!(read_only.validate().is_err());
        read_only.allowed_paths.clear();
        assert!(read_only.validate().is_ok());
        assert_eq!(read_only.execution_workspace(), "/workspace/root");
    }

    #[test]
    fn child_run_request_must_exactly_match_its_frozen_agent_task() {
        let task = child_task();
        assert!(task.validate().is_ok());
        let mut request = RunRequest::new(task.task_contract.clone(), "system");
        request.run_id = Some(task.child_run_id.clone());
        request.parent_run_id = Some(task.parent_run_id.clone());
        request.actor = AgentActor {
            kind: AgentActorKind::Child,
            depth: 1,
        };
        request.model = task.model.clone();
        request.reasoning_effort = task.reasoning_effort;
        request.max_output_tokens = task.max_output_tokens;
        request.context_policy = task.context_policy;
        request.route = task.route.clone();
        request.tool_policy = task.tool_policy.clone();
        request.limits = task.limits;
        request.deadline_unix_ms = task.deadline_unix_ms;
        request.environment.workspace = task.workspace.execution_workspace().to_owned();
        request.agent_task = Some(task.clone());
        assert!(request.validate_agent_task_binding().is_ok());

        let mut mismatched = request.clone();
        mismatched.environment.workspace = "/workspace/other".to_owned();
        assert!(mismatched.validate_agent_task_binding().is_err());
        let mut mismatched_route = request.clone();
        mismatched_route.route.reason_code = "fixed_read_only_recheck".to_owned();
        assert!(mismatched_route.validate_agent_task_binding().is_err());

        let mut root = request;
        root.actor = AgentActor::default();
        root.parent_run_id = None;
        assert!(root.validate_agent_task_binding().is_err());
    }

    #[test]
    fn writer_result_requires_host_observed_seal_and_integration_state() {
        let details = AgentResultDetails {
            summary: "协议变更完成".to_owned(),
            changed_files: vec!["crates/protocol/src/agent_runtime.rs".to_owned()],
            workspace: Some(writer_assignment()),
            workspace_state: Some(known_workspace(2, 'c')),
            base_commit: Some("a".repeat(40)),
            final_commit: Some("b".repeat(40)),
            diff_sha256: Some("d".repeat(64)),
            integration: WriterIntegrationStatus::AwaitingHost,
            ..AgentResultDetails::default()
        };
        assert!(details.validate().is_ok());

        let encoded = serde_json::to_value(&details).unwrap();
        assert_eq!(encoded["integration"]["state"], "awaiting_host");
        assert_eq!(
            serde_json::from_value::<AgentResultDetails>(encoded).unwrap(),
            details
        );

        let mut missing_seal = details.clone();
        missing_seal.diff_sha256 = None;
        assert!(missing_seal.validate().is_err());

        let mut read_only_with_writer_state = details;
        read_only_with_writer_state.workspace = Some(child_task().workspace);
        assert!(read_only_with_writer_state.validate().is_err());
    }

    #[test]
    fn m6_agent_lifecycle_events_round_trip_and_reject_ambiguous_cleanup() {
        let task = child_task();
        let prepared = RuntimeEventKind::AgentTaskPrepared {
            task: Box::new(task.clone()),
        };
        assert!(prepared.validate_agent_lifecycle_payload().is_ok());
        let encoded = serde_json::to_value(&prepared).unwrap();
        assert_eq!(encoded["kind"], "agent_task_prepared");
        assert_eq!(
            serde_json::from_value::<RuntimeEventKind>(encoded).unwrap(),
            prepared
        );

        let started = RuntimeEventKind::ChildStarted {
            task_id: task.task_id.clone(),
            call_id: task.call_id.clone(),
            child_run_id: task.child_run_id.clone(),
            depth: 1,
        };
        assert!(started.validate_agent_lifecycle_payload().is_ok());
        assert_eq!(
            serde_json::to_value(&started).unwrap()["task_id"],
            task.task_id.0
        );

        let seal = RuntimeEventKind::AgentSealCommitted {
            task_id: task.task_id.clone(),
            final_commit: "b".repeat(40),
            diff_sha256: "d".repeat(64),
            changed_files: vec!["crates/protocol/src/agent_runtime.rs".to_owned()],
            writer_workspace_state_after: known_workspace(2, 'e'),
        };
        assert!(seal.validate_agent_lifecycle_payload().is_ok());

        let failed = RuntimeEventKind::AgentIntegrationFailed {
            task_id: task.task_id.clone(),
            integration_id: OperationId::from("integration-1"),
            status: WriterIntegrationStatus::RecoveryRequired {
                reason: "集成操作结果不确定".to_owned(),
            },
            root_workspace_state: known_workspace(3, 'f'),
        };
        assert!(failed.validate_agent_lifecycle_payload().is_ok());
        let encoded = serde_json::to_value(&failed).unwrap();
        assert_eq!(encoded["kind"], "agent_integration_failed");
        assert_eq!(encoded["status"]["state"], "recovery_required");
        assert_eq!(
            serde_json::from_value::<RuntimeEventKind>(encoded).unwrap(),
            failed
        );

        let invalid_failed = RuntimeEventKind::AgentIntegrationFailed {
            task_id: task.task_id.clone(),
            integration_id: OperationId::from("integration-1"),
            status: WriterIntegrationStatus::AwaitingHost,
            root_workspace_state: known_workspace(3, 'f'),
        };
        assert!(invalid_failed.validate_agent_lifecycle_payload().is_err());

        let plan = WriterCleanupPlan {
            phase: WriterCleanupPhase::Integration,
            reason_code: "writer_integration_conflict".to_owned(),
            ownership: WriterCleanupOwnership::Known {
                identity_sha256: "c".repeat(64),
            },
            artifact_state: WriterArtifactState::KnownHostSealed {
                final_commit: "b".repeat(40),
                diff_sha256: "d".repeat(64),
            },
            scope: WriterCleanupScope::Known {
                workspace_revision: WorkspaceRevision::Known {
                    sha256: "e".repeat(64),
                },
                changed_count: 1,
                in_scope_count: 1,
                out_of_scope_count: 0,
                path_set_sha256: "f".repeat(64),
            },
            mode: WriterCleanupMode::RemoveExact {
                expected_branch_commit: "b".repeat(40),
            },
        };
        let prepared = RuntimeEventKind::AgentCleanupPrepared {
            task_id: task.task_id.clone(),
            plan: Box::new(plan.clone()),
        };
        assert!(prepared.validate_agent_lifecycle_payload().is_ok());
        assert_eq!(
            serde_json::from_value::<RuntimeEventKind>(serde_json::to_value(&prepared).unwrap())
                .unwrap(),
            prepared
        );

        let partial = RuntimeEventKind::AgentCleanupCommitted {
            task_id: task.task_id.clone(),
            result: WriterCleanupResult::Retained {
                worktree: WriterResourceState::AlreadyAbsent,
                branch: WriterResourceState::Retained,
                metadata: WriterCleanupMetadataState::Clear,
                uncertainty_code: "writer_branch_tip_changed".to_owned(),
            },
        };
        assert!(partial.validate_agent_lifecycle_payload().is_ok());

        let mut invalid_plan = plan.clone();
        invalid_plan.scope = WriterCleanupScope::Unknown {
            uncertainty_code: "writer_scope_unknown".to_owned(),
        };
        assert!(invalid_plan.validate().is_err());

        let mut invalid_revision = plan.clone();
        invalid_revision.scope = WriterCleanupScope::Known {
            workspace_revision: WorkspaceRevision::Known {
                sha256: "b".repeat(40),
            },
            changed_count: 0,
            in_scope_count: 0,
            out_of_scope_count: 0,
            path_set_sha256: "f".repeat(64),
        };
        assert!(invalid_revision.validate().is_err());

        let mut overflowed_counts = plan.clone();
        overflowed_counts.scope = WriterCleanupScope::Known {
            workspace_revision: known_workspace(0, 'e').revision,
            changed_count: 0,
            in_scope_count: u32::MAX,
            out_of_scope_count: 1,
            path_set_sha256: "f".repeat(64),
        };
        assert!(overflowed_counts.validate().is_err());

        let mut unknown_owner_remove = plan.clone();
        unknown_owner_remove.ownership = WriterCleanupOwnership::Unknown {
            uncertainty_code: "writer_owner_unknown".to_owned(),
        };
        assert!(unknown_owner_remove.validate().is_err());

        let mut mismatched_retention = plan;
        mismatched_retention.ownership = WriterCleanupOwnership::Unknown {
            uncertainty_code: "writer_owner_unknown".to_owned(),
        };
        mismatched_retention.scope = WriterCleanupScope::Unknown {
            uncertainty_code: "writer_scope_unknown".to_owned(),
        };
        mismatched_retention.mode = WriterCleanupMode::RetainForRecovery {
            uncertainty_code: "writer_cleanup_unknown".to_owned(),
        };
        assert!(mismatched_retention.validate().is_err());

        assert!(
            WriterCleanupResult::Removed {
                worktree: WriterRemovalState::AlreadyAbsent,
                branch: WriterRemovalState::AlreadyAbsent,
            }
            .validate()
            .is_err()
        );
        assert!(
            WriterCleanupResult::Retained {
                worktree: WriterResourceState::Removed,
                branch: WriterResourceState::AlreadyAbsent,
                metadata: WriterCleanupMetadataState::Clear,
                uncertainty_code: "writer_cleanup_unknown".to_owned(),
            }
            .validate()
            .is_err()
        );
        assert!(
            WriterCleanupResult::Retained {
                worktree: WriterResourceState::Removed,
                branch: WriterResourceState::AlreadyAbsent,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code: "writer_cleanup_metadata_uncertain".to_owned(),
            }
            .validate()
            .is_ok()
        );

        let invalid_result = RuntimeEventKind::AgentCleanupCommitted {
            task_id: task.task_id,
            result: WriterCleanupResult::Retained {
                worktree: WriterResourceState::Unknown,
                branch: WriterResourceState::Unknown,
                metadata: WriterCleanupMetadataState::Uncertain,
                uncertainty_code: "/tmp/leaked path".to_owned(),
            },
        };
        assert!(invalid_result.validate_agent_lifecycle_payload().is_err());
    }

    #[test]
    fn canonical_reasoning_and_tool_call_project_exactly() {
        let call = ModelToolCall {
            id: "call-1".into(),
            name: "read".into(),
            arguments: ToolArguments::from_value(serde_json::json!({"path": "src/lib.rs"})),
        };
        let transcript = CanonicalTranscript {
            entries: vec![TranscriptEntry::Assistant {
                content: None,
                reasoning_content: Some("原始推理".into()),
                tool_calls: vec![call.clone()],
            }],
        };
        let projected = transcript.project_messages();
        assert_eq!(
            projected,
            vec![ModelMessage::Assistant {
                content: None,
                reasoning_content: Some("原始推理".into()),
                tool_calls: vec![call],
            }]
        );
    }

    #[test]
    fn settled_tool_history_binds_each_result_to_its_exact_assistant_turn() {
        let first = ModelToolCall {
            id: "reused-call".into(),
            name: "read_file".into(),
            arguments: ToolArguments::from_value(serde_json::json!({"path": "first.rs"})),
        };
        let second = ModelToolCall {
            id: "reused-call".into(),
            name: "read_file".into(),
            arguments: ToolArguments::from_value(serde_json::json!({"path": "second.rs"})),
        };
        let valid = CanonicalTranscript {
            entries: vec![
                TranscriptEntry::Assistant {
                    content: None,
                    reasoning_content: Some("first reasoning".into()),
                    tool_calls: vec![first.clone()],
                },
                TranscriptEntry::Tool {
                    call_id: first.id,
                    name: first.name,
                    outcome: Box::new(ToolOutcome::success("first result")),
                },
                TranscriptEntry::Assistant {
                    content: None,
                    reasoning_content: Some("second reasoning".into()),
                    tool_calls: vec![second.clone()],
                },
                TranscriptEntry::Tool {
                    call_id: second.id,
                    name: second.name,
                    outcome: Box::new(ToolOutcome::success("second result")),
                },
            ],
        };
        assert!(valid.validate_complete_tool_history().is_ok());

        let mut duplicate = valid.clone();
        duplicate.entries.push(TranscriptEntry::Tool {
            call_id: "reused-call".into(),
            name: "read_file".into(),
            outcome: Box::new(ToolOutcome::success("duplicate")),
        });
        assert!(
            duplicate
                .validate_complete_tool_history()
                .unwrap_err()
                .contains("orphan or duplicate")
        );

        let mut mismatch = valid;
        let TranscriptEntry::Tool { name, .. } = mismatch.entries.last_mut().unwrap() else {
            unreachable!();
        };
        *name = "grep_files".into();
        assert!(
            mismatch
                .validate_complete_tool_history()
                .unwrap_err()
                .contains("instead of")
        );
    }

    #[test]
    fn malformed_tool_arguments_preserve_raw_bytes() {
        let arguments = ToolArguments::parse("{not-json");
        assert_eq!(arguments.raw, "{not-json");
        assert!(arguments.parsed.is_none());
    }

    #[test]
    fn user_interaction_validates_prompt_specific_responses() {
        let request = UserInteractionRequest {
            interaction_id: InteractionId::from("interaction-1"),
            operation_id: OperationId::from("operation-1"),
            call_id: "call-1".to_owned(),
            tool_name: REQUEST_USER_INPUT_TOOL_NAME.to_owned(),
            prompt: UserInteractionPrompt::UserInput {
                request: UserInputRequest {
                    questions: vec![UserInputQuestion {
                        header: "范围".to_owned(),
                        id: "scope".to_owned(),
                        question: "选择范围".to_owned(),
                        options: vec![
                            UserInputOption {
                                label: "A".to_owned(),
                                description: "选项 A".to_owned(),
                            },
                            UserInputOption {
                                label: "B".to_owned(),
                                description: "选项 B".to_owned(),
                            },
                        ],
                        allow_free_text: true,
                        multi_select: false,
                    }],
                },
            },
        };
        assert!(
            request
                .validate_response(&UserInteractionResponse::Answered {
                    answers: vec![UserInputAnswer {
                        id: "scope".to_owned(),
                        label: "自定义".to_owned(),
                        value: "只改 Runtime".to_owned(),
                    }],
                })
                .is_ok()
        );
        assert!(
            request
                .validate_response(&UserInteractionResponse::Approved)
                .is_err()
        );
        assert!(
            request
                .validate_response(&UserInteractionResponse::Answered {
                    answers: vec![UserInputAnswer {
                        id: "missing".to_owned(),
                        label: "A".to_owned(),
                        value: "A".to_owned(),
                    }],
                })
                .is_err()
        );
    }

    #[test]
    fn user_input_contract_rejects_ambiguous_questions_and_answers() {
        let question = UserInputQuestion {
            header: "范围".to_owned(),
            id: "scope".to_owned(),
            question: "选择范围".to_owned(),
            options: vec![
                UserInputOption {
                    label: "A".to_owned(),
                    description: "选项 A".to_owned(),
                },
                UserInputOption {
                    label: "B".to_owned(),
                    description: "选项 B".to_owned(),
                },
            ],
            allow_free_text: false,
            multi_select: true,
        };
        let request = UserInputRequest {
            questions: vec![question.clone()],
        };
        assert!(request.validate().is_ok());
        assert!(
            validate_user_input_answers(
                &request,
                &[UserInputAnswer {
                    id: "scope".to_owned(),
                    label: "A".to_owned(),
                    value: "伪造值".to_owned(),
                }]
            )
            .is_err()
        );
        assert!(validate_user_input_answers(&request, &[]).is_err());
        let duplicate = UserInputAnswer {
            id: "scope".to_owned(),
            label: "A".to_owned(),
            value: "A".to_owned(),
        };
        assert!(validate_user_input_answers(&request, &[duplicate.clone(), duplicate]).is_err());

        let duplicate_question_ids = UserInputRequest {
            questions: vec![question.clone(), question.clone()],
        };
        assert!(duplicate_question_ids.validate().is_err());
        let mut duplicate_options = question;
        duplicate_options.options[1].label = "A".to_owned();
        assert!(
            UserInputRequest {
                questions: vec![duplicate_options]
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn valid_tool_outcome_axes_round_trip() {
        let outcome = ToolOutcome {
            failure_code: Some(ToolFailureCode::OperationFailed),
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Failed,
            side_effect: ToolSideEffectStatus::Applied,
            retry: ToolRetryDisposition::NotRetryable,
            evidence: ToolEvidence {
                status: ToolEvidenceStatus::Produced,
                references: vec!["test://cargo/unit".into()],
            },
            artifacts: vec![ToolArtifact {
                id: "artifact-1".into(),
                status: ToolArtifactStatus::Available,
                sha256: Some("abc123".into()),
                media_type: Some("text/plain".into()),
                byte_len: Some(7),
                inline_content: None,
            }],
            workspace_revision: Some("revision-7".into()),
            verifier_observation: None,
            content: "操作失败，且副作用已经发生".into(),
            metadata: Some(serde_json::json!({"tool_specific": true})),
        };

        assert!(!outcome.is_success());
        assert!(outcome.validate().is_ok());
        let encoded = serde_json::to_value(&outcome).unwrap();
        assert_eq!(encoded["invocation"], "accepted");
        assert_eq!(encoded["transport"], "succeeded");
        assert_eq!(encoded["operation"], "failed");
        assert_eq!(encoded["side_effect"], "applied");
        assert_eq!(encoded["retry"], "not_retryable");
        assert_eq!(encoded["evidence"]["status"], "produced");
        assert_eq!(encoded["artifacts"][0]["status"], "available");
        assert_eq!(encoded["workspace_revision"], "revision-7");
        assert_eq!(
            serde_json::from_value::<ToolOutcome>(encoded).unwrap(),
            outcome
        );

        let rejected =
            ToolOutcome::rejected("invalid arguments", ToolRetryDisposition::AfterCorrection);
        assert_eq!(rejected.invocation, ToolInvocationStatus::Rejected);
        assert_eq!(rejected.transport, ToolTransportStatus::NotStarted);
        assert_eq!(rejected.operation, ToolOperationStatus::NotStarted);
        assert_eq!(rejected.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(rejected.retry, ToolRetryDisposition::AfterCorrection);

        let ambiguous = ToolOutcome::recovery_ambiguous("unknown completion");
        assert_eq!(ambiguous.transport, ToolTransportStatus::Indeterminate);
        assert_eq!(ambiguous.operation, ToolOperationStatus::Indeterminate);
        assert_eq!(ambiguous.side_effect, ToolSideEffectStatus::Indeterminate);
        assert_eq!(ambiguous.retry, ToolRetryDisposition::Unsafe);
    }

    #[test]
    fn model_tool_failure_feedback_is_typed_concise_and_chinese() {
        let mut failure = ToolOutcome::error("文件自上次读取后已改变")
            .with_failure_code(ToolFailureCode::StaleRead);
        failure.side_effect = ToolSideEffectStatus::NotApplied;
        failure.retry = ToolRetryDisposition::AfterCorrection;
        assert!(failure.validate().is_ok());
        assert_eq!(
            failure.model_content(),
            "工具失败：code=stale_read; operation=failed; side_effect=not_applied; retry=after_correction。\n恢复建议：先重新读取相关文件或工作区状态，再根据最新内容修正调用。\n文件自上次读取后已改变"
        );

        let success = ToolOutcome::success("原样成功结果");
        assert!(success.validate().is_ok());
        assert_eq!(success.model_content(), "原样成功结果");

        let mut invalid = failure.clone();
        invalid.failure_code = None;
        assert!(invalid.validate().is_err());
        let invalid = success.with_failure_code(ToolFailureCode::OperationFailed);
        assert!(invalid.validate().is_err());

        let invalid = ToolOutcome::error("unknown after execution")
            .with_failure_code(ToolFailureCode::UnknownTool);
        assert!(invalid.validate().is_err());
        let invalid = ToolOutcome::rejected(
            "transport cannot be a preflight rejection",
            ToolRetryDisposition::NotRetryable,
        )
        .with_failure_code(ToolFailureCode::TransportFailed);
        assert!(invalid.validate().is_err());
        let mut invalid = ToolOutcome::recovery_ambiguous("unsafe side effect");
        invalid.side_effect = ToolSideEffectStatus::NotApplied;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn read_only_workspace_authority_removes_only_write_ambiguity() {
        let transport = ToolOutcome::transport_failure("执行器连接中断")
            .with_workspace_access_guarantee(WorkspaceAccess::ReadOnly);
        assert_eq!(
            transport.failure_code,
            Some(ToolFailureCode::TransportFailed)
        );
        assert_eq!(transport.transport, ToolTransportStatus::Failed);
        assert_eq!(transport.operation, ToolOperationStatus::Indeterminate);
        assert_eq!(transport.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_eq!(transport.retry, ToolRetryDisposition::Safe);
        transport.validate().unwrap();
        assert!(
            transport
                .model_content()
                .contains("传输失败，但已确认没有工作区副作用，可以安全重新调用。")
        );
        assert!(!transport.model_content().contains("副作用状态无法安全确认"));

        let ambiguous = ToolOutcome::recovery_ambiguous("执行状态未知")
            .with_workspace_access_guarantee(WorkspaceAccess::ReadOnly);
        assert_eq!(
            ambiguous.failure_code,
            Some(ToolFailureCode::OperationFailed)
        );
        assert_eq!(ambiguous.transport, ToolTransportStatus::Indeterminate);
        assert_eq!(ambiguous.operation, ToolOperationStatus::Indeterminate);
        assert_eq!(ambiguous.side_effect, ToolSideEffectStatus::NotApplicable);
        assert_eq!(ambiguous.retry, ToolRetryDisposition::Safe);
        ambiguous.validate().unwrap();

        let mut impossible = ToolOutcome::error("错误声明已写入");
        impossible.side_effect = ToolSideEffectStatus::Applied;
        let impossible = impossible.with_workspace_access_guarantee(WorkspaceAccess::ReadOnly);
        assert_eq!(impossible.side_effect, ToolSideEffectStatus::Applied);

        let writer = ToolOutcome::recovery_ambiguous("写入状态未知")
            .with_workspace_access_guarantee(WorkspaceAccess::MayWrite);
        assert_eq!(
            writer.failure_code,
            Some(ToolFailureCode::SideEffectAmbiguous)
        );
        assert_eq!(writer.side_effect, ToolSideEffectStatus::Indeterminate);
        assert_eq!(writer.retry, ToolRetryDisposition::Unsafe);
        writer.validate().unwrap();
        assert!(writer.model_content().contains("副作用状态无法安全确认"));
    }

    #[test]
    fn inline_verification_artifact_is_replay_verifiable() {
        let artifact = ToolArtifact::inline_verification(VerificationArtifactPayload {
            summary: "fixture failed".to_owned(),
            verifier: VerifierSpec {
                verifier_id: "fixture".to_owned(),
                parameters: serde_json::json!({}),
                plan: VerifierPlan {
                    steps: vec![VerifierStep {
                        id: "fixture".to_owned(),
                        program: "false".to_owned(),
                        args: Vec::new(),
                        cwd: String::new(),
                        env: std::collections::BTreeMap::new(),
                        timeout_ms: 1_000,
                    }],
                },
            },
            verdict: VerifierVerdict::Failed,
            workspace_revision: WorkspaceRevision::Known {
                sha256: "sha256:fixture".to_owned(),
            },
        });
        artifact.validate_inline_verification().unwrap();
        assert_eq!(
            artifact.media_type.as_deref(),
            Some("application/vnd.dse.verification+json")
        );

        let mut corrupt = artifact;
        corrupt.byte_len = Some(0);
        assert!(corrupt.validate_inline_verification().is_err());
    }

    #[test]
    fn verifier_revision_stability_rejects_workspace_mutation_and_applied_side_effects() {
        let revision_a = WorkspaceRevision::Known {
            sha256: "sha256:a".to_owned(),
        };
        let revision_b = WorkspaceRevision::Known {
            sha256: "sha256:b".to_owned(),
        };
        let verifier = VerifierSpec {
            verifier_id: "fixture".to_owned(),
            parameters: serde_json::json!({}),
            plan: VerifierPlan {
                steps: vec![VerifierStep {
                    id: "fixture".to_owned(),
                    program: "true".to_owned(),
                    args: Vec::new(),
                    cwd: String::new(),
                    env: std::collections::BTreeMap::new(),
                    timeout_ms: 1_000,
                }],
            },
        };
        let mut outcome = ToolOutcome::success("passed");
        outcome.side_effect = ToolSideEffectStatus::Indeterminate;
        outcome.workspace_revision = Some("sha256:a".to_owned());
        outcome.verifier_observation = Some(VerifierObservation {
            spec: verifier,
            verdict: VerifierVerdict::Passed,
            workspace_revision: revision_a.clone(),
            artifact_ids: vec!["artifact".to_owned()],
        });

        assert!(outcome.has_stable_verifier_revision(&revision_a, &revision_a));
        assert!(!outcome.has_stable_verifier_revision(&revision_a, &revision_b));
        outcome.side_effect = ToolSideEffectStatus::Applied;
        assert!(!outcome.has_stable_verifier_revision(&revision_a, &revision_a));
    }

    #[test]
    fn run_limits_missing_event_idle_uses_production_default() {
        let limits: RunLimits = serde_json::from_value(serde_json::json!({
            "max_turns": 4,
            "max_model_requests": 10,
            "max_model_retries": 1,
            "max_tool_calls": 20,
            "max_depth": 2,
            "wall_time_ms": null
        }))
        .unwrap();
        assert_eq!(limits.model_event_idle_ms, Some(900_000));
        assert_eq!(limits.max_concurrent_children, 64);

        let mut value = serde_json::to_value(limits).unwrap();
        value["model_event_idle_ms"] = Value::Null;
        let disabled: RunLimits = serde_json::from_value(value).unwrap();
        assert_eq!(disabled.model_event_idle_ms, None);
    }

    #[test]
    fn model_failure_and_prepared_retry_round_trip_as_one_event() {
        let run_id = RunId("run-atomic-retry".into());
        let event = RuntimeEventKind::ModelRequestFailed {
            attempt_id: AttemptId("attempt-0".into()),
            failure: ModelAttemptFailure {
                code: "deepseek_transport".into(),
                category: ModelErrorCategory::Transport,
                message: "connection reset".into(),
                retryable: true,
                retry_safe: true,
                actionable_output: false,
                response: ModelResponseEvidence::default(),
            },
            accounting: Box::new(ModelAccounting::default()),
            retry: ModelRetryDecision::Retry {
                prepared: PreparedModelRetry {
                    attempt_id: AttemptId("attempt-1".into()),
                    request: Box::new(ModelRequest {
                        run_id,
                        parent_run_id: None,
                        actor: AgentActor::default(),
                        model: "deepseek-v4-pro".into(),
                        system_prompt: SystemPrompt::from_text("system"),
                        messages: Vec::new(),
                        tools: Vec::new(),
                        reasoning_effort: ReasoningEffort::High,
                        max_output_tokens: None,
                        streaming: true,
                        request_number: 1,
                        attempt: 1,
                    }),
                },
            },
        };

        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["kind"], "model_request_failed");
        assert_eq!(encoded["retry"]["decision"], "retry");
        assert_eq!(encoded["retry"]["prepared"]["request"]["attempt"], 1);
        assert_eq!(
            serde_json::from_value::<RuntimeEventKind>(encoded).unwrap(),
            event
        );
    }

    #[test]
    fn request_budget_failures_have_disjoint_v6_wire_kinds() {
        let logical = RuntimeFailure::ModelRequestBudgetExceeded { limit: 8 };
        let physical = RuntimeFailure::ApiRequestBudgetExceeded { limit: 10 };

        let logical_json = serde_json::to_value(&logical).unwrap();
        let physical_json = serde_json::to_value(&physical).unwrap();
        assert_eq!(
            logical_json,
            serde_json::json!({
                "kind": "model_request_budget_exceeded",
                "limit": 8
            })
        );
        assert_eq!(
            physical_json,
            serde_json::json!({
                "kind": "api_request_budget_exceeded",
                "limit": 10
            })
        );
        assert_eq!(
            serde_json::from_value::<RuntimeFailure>(logical_json).unwrap(),
            logical
        );
        assert_eq!(
            serde_json::from_value::<RuntimeFailure>(physical_json).unwrap(),
            physical
        );
        assert!(
            serde_json::from_value::<RuntimeFailure>(serde_json::json!({
                "kind": "request_budget_exceeded",
                "limit": 8
            }))
            .is_err(),
            "RuntimeEvent v6 must not accept the deleted ambiguous wire kind"
        );
    }
}
