//! Thin composition and CLI projection for the canonical execpolicy engine.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use dse_execpolicy::{ExecPolicy, ExecPolicyMatch};
use serde::Serialize;

#[derive(Debug, Parser, Clone)]
pub(crate) struct ExecPolicyCheckCommand {
    /// Paths to `execpolicy.toml` files to evaluate (repeatable).
    #[arg(short = 'r', long = "rules", value_name = "PATH", required = true)]
    rules: Vec<PathBuf>,

    /// Pretty-print the JSON output.
    #[arg(long)]
    pretty: bool,

    /// Command tokens to check against the same matcher used by production.
    #[arg(
        value_name = "COMMAND",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    command: Vec<String>,
}

#[derive(Serialize)]
struct ExecPolicyCheckOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    matched: Option<ExecPolicyMatch>,
}

impl ExecPolicyCheckCommand {
    pub(crate) fn run(&self) -> Result<()> {
        let mut policy = ExecPolicy::default();
        for path in &self.rules {
            let loaded = ExecPolicy::from_path(path)
                .with_context(|| format!("failed to load policy at {}", path.display()))?;
            for (group, rules) in loaded.rules {
                policy.rules.insert(group, rules);
            }
        }
        let command = shlex::try_join(self.command.iter().map(String::as_str))
            .context("command contains an invalid NUL byte")?;
        let output = ExecPolicyCheckOutput {
            matched: policy.evaluate(&command),
        };
        if self.pretty {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("{}", serde_json::to_string(&output)?);
        }
        Ok(())
    }
}

pub(crate) fn load_default_policy() -> Result<Option<ExecPolicy>> {
    let path = dse_config::dse_home()?.join("execpolicy.toml");
    ExecPolicy::from_optional_path(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_uses_the_same_toml_matcher_as_production() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("execpolicy.toml");
        std::fs::write(
            &path,
            "[rules.shell]\nallow = [\"git status\"]\ndeny = [\"git push --force\"]\n",
        )
        .expect("fixture");
        let policy = ExecPolicy::from_path(&path).expect("parse");
        let matched = policy.evaluate("git status --short").expect("allow match");
        assert_eq!(matched.rule_label(), "shell:allow:git status");
    }
}
