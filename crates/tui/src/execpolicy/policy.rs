use super::rule::RuleMatch;
use super::rule::RuleRef;
use multimap::MultiMap;

#[derive(Clone, Debug)]
pub struct Policy {
    rules_by_program: MultiMap<String, RuleRef>,
}

impl Policy {
    pub fn new(rules_by_program: MultiMap<String, RuleRef>) -> Self {
        Self { rules_by_program }
    }

    #[cfg(target_env = "ohos")]
    pub fn empty() -> Self {
        Self::new(MultiMap::new())
    }

    pub fn matches_for_command(&self, cmd: &[String]) -> Vec<RuleMatch> {
        match cmd.first() {
            Some(first) => self
                .rules_by_program
                .get_vec(first)
                .map(|rules| rules.iter().filter_map(|rule| rule.matches(cmd)).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
}
