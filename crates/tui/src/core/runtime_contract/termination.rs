use serde::{Deserialize, Serialize};

/// Typed terminal reasons consumed by the Headless machine-facing projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunTerminationReason {
    Resolved,
    Unresolved,
    Canceled,
    Stuck,
    Timeout,
    BudgetExhausted,
    ApprovalRequired,
    ModelError,
    ToolError,
    InfrastructureError,
    EvidenceMissing,
}
