use serde::{Deserialize, Serialize};

pub mod agent_runtime;
pub mod run_api;
pub mod task;

/// Action to take for a network policy rule.
///
/// This remains a shared protocol type because the execution-policy owner has
/// a real production consumer. It is unrelated to model-provider selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicyRuleAction {
    /// Allow network access to the host.
    Allow,
    /// Deny network access to the host.
    Deny,
}

/// A proposed amendment to the network access policy for a specific host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkPolicyAmendment {
    /// The host to amend the policy for.
    pub host: String,
    /// The action to apply.
    pub action: NetworkPolicyRuleAction,
}
