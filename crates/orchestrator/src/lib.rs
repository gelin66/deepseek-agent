//! Canonical multi-Agent orchestration primitives.
//!
//! M6-A begins with one concrete capability: an owned Git worktree lifecycle
//! for an isolated writer. Model loops, tools, protocol state, and persistence
//! remain in their existing canonical owners.

mod runtime;
mod workspace;

pub use runtime::ProductionAgentOrchestrator;
