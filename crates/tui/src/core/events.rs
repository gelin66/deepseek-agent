//! Events emitted by the core engine to the UI.
//!
//! These events flow from the engine to the TUI via a channel,
//! enabling non-blocking, real-time updates.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::config::ApiProvider;
use crate::error_taxonomy::ErrorEnvelope;
use crate::models::{Tool, Usage};
use crate::tools::goal::GoalSnapshot;
use crate::tools::spec::{ToolError, ToolOutcome};
use crate::tools::subagent::SubAgentResult;
use crate::tools::user_input::UserInputRequest;

/// Final status for a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOutcomeStatus {
    Completed,
    Interrupted,
    Failed,
}

/// Provider/model route resolved for a model-backed turn.
///
/// Carried with `TurnStarted` so hosts can retain provenance until the matching
/// `TurnComplete` without relying on mutable global selection state. Non-model
/// turns such as composer `!` shell commands use no route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRoute {
    pub provider: ApiProvider,
    pub model: String,
    pub auto_model: bool,
}

/// Events emitted by the engine to update the UI.
#[derive(Debug, Clone)]
pub enum Event {
    // === Streaming Events ===
    /// A new message block has started
    MessageStarted {
        #[allow(dead_code)]
        index: usize,
    },

    /// Incremental text content delta
    MessageDelta {
        #[allow(dead_code)]
        index: usize,
        content: String,
    },

    /// Message block completed
    MessageComplete {
        #[allow(dead_code)]
        index: usize,
    },

    /// Thinking block started
    ThinkingStarted {
        #[allow(dead_code)]
        index: usize,
    },

    /// Incremental thinking content delta
    ThinkingDelta {
        #[allow(dead_code)]
        index: usize,
        content: String,
    },

    /// Thinking block completed
    ThinkingComplete {
        #[allow(dead_code)]
        index: usize,
    },

    // === Tool Events ===
    /// Tool call initiated
    ToolCallStarted {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool call completed
    ToolCallComplete {
        id: String,
        name: String,
        result: Result<ToolOutcome, ToolError>,
    },

    // === Turn Lifecycle ===
    /// A new turn has started (user sent a message)
    TurnStarted {
        turn_id: String,
        created_at: DateTime<Utc>,
        route: Option<TurnRoute>,
    },

    /// The turn is complete (no more tool calls)
    TurnComplete {
        usage: Usage,
        status: TurnOutcomeStatus,
        error: Option<String>,
        /// Tool catalog sent with this turn's model request.
        tool_catalog: Option<Vec<Tool>>,
        /// API base URL used by this turn's client.
        base_url: Option<String>,
    },

    /// The next `TurnComplete` is also a run-level terminal candidate: no
    /// direct child is running or claiming completion, no child completion is
    /// queued for parent integration, and no Goal continuation is scheduled.
    /// Interactive hosts ignore this marker; Headless requires it before
    /// sealing request accounting and publishing `Done`.
    RunTerminalCandidate,

    /// The completed turn is not the run terminal: an active Goal has queued
    /// another canonical Engine turn. Headless hosts must keep consuming.
    GoalContinuationScheduled,

    /// Runtime goal state changed inside the engine, usually from model-visible
    /// `create_goal` or `update_goal` tool calls.
    GoalUpdated { snapshot: Box<GoalSnapshot> },

    /// Context compaction started.
    CompactionStarted {
        id: String,
        auto: bool,
        message: String,
    },

    /// Context compaction completed.
    CompactionCompleted {
        id: String,
        auto: bool,
        message: String,
        /// Number of messages before compaction.
        #[allow(dead_code)]
        messages_before: Option<usize>,
        /// Number of messages after compaction.
        #[allow(dead_code)]
        messages_after: Option<usize>,
        /// Rendered text of the accumulated compaction summary prompt, if any.
        /// Retained by old compaction consumers while this event surface is
        /// awaiting deletion.
        summary_prompt: Option<String>,
    },

    /// Context compaction failed.
    CompactionFailed {
        id: String,
        auto: bool,
        message: String,
    },

    // === Sub-Agent Events ===
    /// A sub-agent has been spawned
    AgentSpawned {
        id: String,
        prompt: String,
        parent_run_id: Option<String>,
        spawn_depth: u32,
    },

    /// Sub-agent progress update
    AgentProgress {
        id: String,
        status: String,
        parent_run_id: Option<String>,
        spawn_depth: u32,
    },

    /// Sub-agent completed
    AgentComplete { id: String, result: String },

    /// Sub-agent listing
    AgentList { agents: Vec<SubAgentResult> },

    /// Structured sub-agent mailbox envelope (issue #128). Carries the
    /// monotonic seq + the typed `MailboxMessage` so the UI can route each
    /// envelope to the correct in-transcript card.
    SubAgentMailbox {
        seq: u64,
        message: crate::tools::subagent::MailboxMessage,
    },

    // === System Events ===
    /// An error occurred
    Error {
        envelope: ErrorEnvelope,
        #[allow(dead_code)]
        recoverable: bool,
    },

    /// Status message for UI display
    Status { message: String },

    /// Pause terminal input events (for interactive subprocesses).
    PauseEvents {
        /// Optional one-shot notification fired after the UI has actually
        /// released the terminal to the child process.
        ack: Option<Arc<tokio::sync::Notify>>,
    },

    /// Resume terminal input events after subprocess completion
    ResumeEvents,

    /// Request user approval for a tool call
    ApprovalRequired {
        id: String,
        tool_name: String,
        description: String,
        /// Tool parameters for approval display. Carried on the event so the
        /// TUI does not need to reconstruct them from `pending_tool_uses`.
        input: Value,
        /// Exact-argument fingerprint, used to scope *denials* (#1617).
        approval_key: String,
        /// Lossy / arity-aware fingerprint, used to scope *approvals* so an
        /// "approve for session" covers later flag variants (v0.8.37).
        approval_grouping_key: String,
        /// The model's explanation of intent before invoking write tools (#2381).
        /// Displayed in the approval view so users understand *why* the change
        /// is being made before reviewing *what* will change.
        intent_summary: Option<String>,
        /// When true, the UI must show the prompt instead of consuming
        /// session/auto approval shortcuts.
        approval_force_prompt: bool,
    },

    /// Request user input for a tool call
    UserInputRequired {
        id: String,
        request: UserInputRequest,
    },

    /// Request user decision after sandbox denial
    #[allow(dead_code)]
    ElevationRequired {
        tool_id: String,
        tool_name: String,
        command: Option<String>,
        denial_reason: String,
        blocked_network: bool,
        blocked_write: bool,
    },

    /// Observable LSP repair-loop update for the Turn Inspector (#4107).
    /// Carries only summary counts/state — never raw prompt internals.
    LspRepairUpdate {
        diagnostics_found: usize,
        files: usize,
        injected: bool,
    },
}

impl Event {
    /// Create an error event from a categorized envelope. The envelope's own
    /// `recoverable` flag controls whether the UI flips into offline mode.
    pub fn error(envelope: ErrorEnvelope) -> Self {
        let recoverable = envelope.recoverable;
        Event::Error {
            envelope,
            recoverable,
        }
    }

    /// Create a new status event
    pub fn status(message: impl Into<String>) -> Self {
        Event::Status {
            message: message.into(),
        }
    }
}
