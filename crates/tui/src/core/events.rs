//! Remaining route metadata used by the retained TUI shell.

use crate::config::ApiProvider;

/// Provider/model route resolved for a model-backed turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRoute {
    pub provider: ApiProvider,
    pub model: String,
    pub auto_model: bool,
}
