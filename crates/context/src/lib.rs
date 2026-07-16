//! Canonical, transport-neutral model context and system-prompt composition.
//!
//! This crate owns prompt bytes and their stable/volatile block boundary. It
//! intentionally has no dependency on a presentation client, model loop, or
//! tool executor.

mod project_context_cache;

pub mod model_context;
pub mod project_context;
pub mod prompts;
pub mod skills;

pub use prompts::{
    InstructionSource, ProductionPromptRequest, PromptSessionContext, production_system_prompt,
};
