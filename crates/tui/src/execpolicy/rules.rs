//! Execpolicy rules loaded from TOML configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct ExecPolicyConfig {
    #[serde(default)]
    pub rules: BTreeMap<String, RuleSet>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ExecPolicyConfig {
    pub fn production_snapshot(&self) -> dse_tools::ProductionExecPolicySnapshot {
        dse_tools::ProductionExecPolicySnapshot {
            rules: self
                .rules
                .iter()
                .map(|(name, rules)| {
                    (
                        name.clone(),
                        dse_tools::ProductionExecPolicyRuleSet {
                            allow: rules.allow.clone(),
                            deny: rules.deny.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn from_str(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse execpolicy.toml")
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read execpolicy file {}", path.display()))?;
        Self::from_str(&contents)
    }
}

pub fn default_execpolicy_path() -> Option<PathBuf> {
    dse_config::dse_home()
        .ok()
        .map(|home| home.join("execpolicy.toml"))
}

pub fn load_default_policy() -> Result<Option<ExecPolicyConfig>> {
    let Some(path) = default_execpolicy_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    ExecPolicyConfig::from_path(&path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_rules_become_the_canonical_production_snapshot() {
        let config = ExecPolicyConfig::from_str(
            r#"
[rules.shell]
allow = ["git status"]
deny = ["git push --force"]
"#,
        )
        .expect("parse production execpolicy fixture");

        let snapshot = config.production_snapshot();
        let shell = snapshot.rules.get("shell").expect("shell rules");

        assert_eq!(shell.allow, ["git status"]);
        assert_eq!(shell.deny, ["git push --force"]);
    }
}
