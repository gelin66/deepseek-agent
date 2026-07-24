//! Canonical, transport-neutral model context and system-prompt composition.
//!
//! This crate owns prompt bytes and their stable/volatile block boundary. It
//! intentionally has no dependency on a presentation client, model loop, or
//! tool executor.

mod project_context_cache;
mod safe_read;

pub mod compaction;
pub mod model_context;
pub mod project_context;
pub mod prompts;
pub mod skills;
pub mod working_set;

pub use prompts::{
    InstructionSource, ProductionPromptBuild, ProductionPromptRequest, PromptContextLayer,
    PromptContextLedger, PromptContextLedgerEntry, PromptContextScope, PromptContextStability,
    production_system_prompt, production_system_prompt_with_ledger,
};
pub use working_set::{
    WorkingSetBudget, WorkingSetError, WorkingSetProjection, WorkingSetReason, WorkingSetRegion,
    WorkingSetRequest, select_working_set,
};
