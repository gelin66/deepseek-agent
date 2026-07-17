//! Canonical domain protocol for the single Agent runtime.
//!
//! These types deliberately describe model turns and runtime facts without
//! depending on a UI, an HTTP transport, or a database representation.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const AGENT_RUNTIME_EVENT_SCHEMA_VERSION: u32 = 4;
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
    #[default]
    Auto,
    Low,
    Medium,
    High,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActorKind {
    Root,
    Child,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiSurface {
    StandardChat,
    StrictChat,
    Fim,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<RunId>,
    pub model: String,
    pub input: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix_ms: Option<u64>,
    #[serde(default)]
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub limits: RunLimits,
    #[serde(default)]
    pub environment: RunEnvironment,
    /// Physical model accounting already owned by the fresh run's
    /// `ModelPort`. Persisting it in `run_created` closes the crash window for
    /// pre-runtime calls such as DeepSeek auto-routing. A reopened process
    /// adds only its new local accounting to this durable baseline.
    #[serde(default)]
    pub accounting_baseline: ModelAccounting,
}

impl RunRequest {
    #[must_use]
    pub fn new(input: impl Into<String>, system_prompt: impl Into<SystemPrompt>) -> Self {
        Self {
            run_id: None,
            parent_run_id: None,
            model: "deepseek-v4-flash".to_owned(),
            input: input.into(),
            system_prompt: system_prompt.into(),
            transcript: CanonicalTranscript::default(),
            reasoning_effort: ReasoningEffort::Auto,
            max_output_tokens: None,
            streaming: true,
            actor: AgentActor::default(),
            deadline_unix_ms: None,
            tool_policy: ToolPolicy::default(),
            limits: RunLimits::default(),
            environment: RunEnvironment::default(),
            accounting_baseline: ModelAccounting::default(),
        }
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelStreamEvent {
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
    Safe,
    Unsafe,
    NotRetryable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEvidenceStatus {
    NotApplicable,
    Missing,
    Produced,
    Verified,
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
}

/// The one tool outcome shared by tool implementations, AgentRuntime,
/// transcript projection, persistence, and every client surface.
///
/// `metadata` is tool-specific structured payload only. Host decisions must
/// use the typed lifecycle, retry, evidence, artifact, and revision fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutcome {
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
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl ToolOutcome {
    #[must_use]
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Succeeded,
            side_effect: ToolSideEffectStatus::NotApplicable,
            retry: ToolRetryDisposition::NotNeeded,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Failed,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::NotRetryable,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn rejected(content: impl Into<String>, retry: ToolRetryDisposition) -> Self {
        Self {
            invocation: ToolInvocationStatus::Rejected,
            transport: ToolTransportStatus::NotStarted,
            operation: ToolOperationStatus::NotStarted,
            side_effect: ToolSideEffectStatus::NotApplied,
            retry,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn transport_failure(content: impl Into<String>) -> Self {
        Self {
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Failed,
            operation: ToolOperationStatus::Indeterminate,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::Unsafe,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn recovery_ambiguous(content: impl Into<String>) -> Self {
        Self {
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Indeterminate,
            operation: ToolOperationStatus::Indeterminate,
            side_effect: ToolSideEffectStatus::Indeterminate,
            retry: ToolRetryDisposition::Unsafe,
            evidence: ToolEvidence::default(),
            artifacts: Vec::new(),
            workspace_revision: None,
            content: content.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn is_success(&self) -> bool {
        self.invocation == ToolInvocationStatus::Accepted
            && self.transport == ToolTransportStatus::Succeeded
            && self.operation == ToolOperationStatus::Succeeded
    }

    pub fn validate(&self) -> Result<(), String> {
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
        outcome: ToolOutcome,
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
                    content: outcome.content.clone(),
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
    pub exhausted_denied: u64,
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
    RequestBudgetExceeded {
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
    Completed { message: String },
    Blocked { reason: String },
    Failed { failure: RuntimeFailure },
    Cancelled,
    Interrupted,
    RecoveryRequired { ambiguity: RecoveryAmbiguity },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentOutcome {
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
    pub terminal: TerminalState,
    pub accounting: ModelAccounting,
    pub runtime_model_requests: u32,
    pub runtime_retries: u32,
    pub tool_calls: u32,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAttemptFailure {
    pub code: String,
    pub category: ModelErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub actionable_output: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRetryStopReason {
    ActionableOutput,
    NotRetryable,
    FailureChanged,
    RetryLimitReached,
    RequestBudgetExceeded,
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
        outcome: ToolOutcome,
    },
    ChildStarted {
        call_id: String,
        child_run_id: RunId,
        depth: u8,
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
    fn tool_outcome_axes_are_typed_independent_and_round_trip() {
        let outcome = ToolOutcome {
            invocation: ToolInvocationStatus::Accepted,
            transport: ToolTransportStatus::Succeeded,
            operation: ToolOperationStatus::Failed,
            side_effect: ToolSideEffectStatus::Applied,
            retry: ToolRetryDisposition::Safe,
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
            }],
            workspace_revision: Some("revision-7".into()),
            content: "operation failed after applying a side effect".into(),
            metadata: Some(serde_json::json!({"tool_specific": true})),
        };

        assert!(!outcome.is_success());
        let encoded = serde_json::to_value(&outcome).unwrap();
        assert_eq!(encoded["invocation"], "accepted");
        assert_eq!(encoded["transport"], "succeeded");
        assert_eq!(encoded["operation"], "failed");
        assert_eq!(encoded["side_effect"], "applied");
        assert_eq!(encoded["retry"], "safe");
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
                actionable_output: false,
            },
            accounting: Box::new(ModelAccounting::default()),
            retry: ModelRetryDecision::Retry {
                prepared: PreparedModelRetry {
                    attempt_id: AttemptId("attempt-1".into()),
                    request: Box::new(ModelRequest {
                        run_id,
                        parent_run_id: None,
                        actor: AgentActor::default(),
                        model: "deepseek-chat".into(),
                        system_prompt: SystemPrompt::from_text("system"),
                        messages: Vec::new(),
                        tools: Vec::new(),
                        reasoning_effort: ReasoningEffort::Auto,
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
}
