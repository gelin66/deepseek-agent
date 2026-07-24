//! Existing ordered WorldState prompt blocks.

use std::collections::BTreeMap;

use codewhale_protocol::agent_runtime::{PromptCacheControl, SystemPromptBlock};

use super::fragment::{FragmentId, FragmentRole, ModelContextFragment};

/// Ordered volatile prompt blocks below the stable constitution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldState {
    fragments: BTreeMap<FragmentId, ModelContextFragment>,
}

impl WorldState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    fn len(&self) -> usize {
        self.fragments.len()
    }

    fn with_fragment(
        mut self,
        id: FragmentId,
        role: FragmentRole,
        body: impl Into<String>,
    ) -> Self {
        self.fragments
            .insert(id, ModelContextFragment::new(id, role, body));
        self
    }

    #[must_use]
    pub fn with_workspace(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::Workspace, FragmentRole::Workspace, body)
    }

    #[must_use]
    pub fn with_permissions(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::Permissions, FragmentRole::Permissions, body)
    }

    #[must_use]
    pub fn with_route(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::Route, FragmentRole::Route, body)
    }

    #[must_use]
    pub fn with_agent_topology(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::AgentTopology, FragmentRole::AgentTopology, body)
    }

    #[must_use]
    pub fn with_skills_tools(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::SkillsTools, FragmentRole::SkillsTools, body)
    }

    #[must_use]
    pub fn with_token_budget(self, body: impl Into<String>) -> Self {
        self.with_fragment(FragmentId::TokenBudget, FragmentRole::TokenBudget, body)
    }
}

/// Cache-stable constitution plus the existing volatile block sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldStateSnapshot {
    pub constitution: String,
    pub world_state: WorldState,
}

impl WorldStateSnapshot {
    #[must_use]
    pub fn to_system_blocks(&self) -> Vec<SystemPromptBlock> {
        let mut blocks = Vec::with_capacity(1 + self.world_state.len());
        blocks.push(SystemPromptBlock {
            text: self.constitution.trim().to_owned(),
            cache_control: PromptCacheControl::Stable,
        });
        blocks.extend(
            self.world_state
                .fragments
                .values()
                .map(|fragment| SystemPromptBlock {
                    text: fragment.render_marked(),
                    cache_control: PromptCacheControl::Volatile,
                }),
        );
        blocks
    }
}
