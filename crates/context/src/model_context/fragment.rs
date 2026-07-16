//! One capped, marker-stable prompt-context fragment.

/// Default hard byte cap per volatile fragment.
pub const DEFAULT_FRAGMENT_MAX_BYTES: usize = 4 * 1024;

/// Stable identity and order for the existing volatile prompt blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FragmentId {
    Workspace,
    Permissions,
    Route,
    AgentTopology,
    SkillsTools,
    TokenBudget,
}

impl FragmentId {
    #[must_use]
    pub fn marker(self) -> &'static str {
        match self {
            Self::Workspace => "<!-- cw:ctx:workspace -->",
            Self::Permissions => "<!-- cw:ctx:permissions -->",
            Self::Route => "<!-- cw:ctx:route -->",
            Self::AgentTopology => "<!-- cw:ctx:agent_topology -->",
            Self::SkillsTools => "<!-- cw:ctx:skills_tools -->",
            Self::TokenBudget => "<!-- cw:ctx:token_budget -->",
        }
    }
}

/// Existing semantic role of a volatile prompt block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FragmentRole {
    Workspace,
    Permissions,
    Route,
    AgentTopology,
    SkillsTools,
    TokenBudget,
}

/// One capped block below the cache-stable constitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContextFragment {
    pub id: FragmentId,
    pub marker: &'static str,
    pub content: String,
}

impl ModelContextFragment {
    #[must_use]
    pub fn new(id: FragmentId, _role: FragmentRole, raw: impl Into<String>) -> Self {
        Self {
            id,
            marker: id.marker(),
            content: enforce_byte_cap(raw.into(), DEFAULT_FRAGMENT_MAX_BYTES),
        }
    }

    #[must_use]
    pub fn render_marked(&self) -> String {
        format!("{}\n{}", self.marker, self.content.trim_end())
    }
}

fn enforce_byte_cap(raw: String, max_bytes: usize) -> String {
    if raw.len() <= max_bytes {
        return raw;
    }
    let omitted = raw.len().saturating_sub(max_bytes);
    let marker = format!("\n[…truncated: {omitted} bytes omitted]");
    let keep = max_bytes.saturating_sub(marker.len());
    let mut end = keep;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = raw[..end].to_owned();
    out.push_str(&marker);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_marker_and_cap_are_stable() {
        let fragment = ModelContextFragment::new(
            FragmentId::AgentTopology,
            FragmentRole::AgentTopology,
            "x".repeat(DEFAULT_FRAGMENT_MAX_BYTES + 64),
        );
        assert_eq!(fragment.marker, "<!-- cw:ctx:agent_topology -->");
        assert!(fragment.content.len() <= DEFAULT_FRAGMENT_MAX_BYTES);
        assert!(fragment.content.contains("[…truncated:"));
    }
}
