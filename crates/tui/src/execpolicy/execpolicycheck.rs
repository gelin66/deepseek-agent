use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use serde::Serialize;

use super::decision::Decision;
#[cfg(not(target_env = "ohos"))]
use super::parser::PolicyParser;
#[cfg(target_env = "ohos")]
use super::parser_ohos::PolicyParser;
use super::policy::Policy;
use super::rule::RuleMatch;

/// Arguments for evaluating a command against one or more execpolicy files.
#[derive(Debug, Parser, Clone)]
pub struct ExecPolicyCheckCommand {
    /// Paths to execpolicy rule files to evaluate (repeatable).
    #[arg(short = 'r', long = "rules", value_name = "PATH", required = true)]
    pub rules: Vec<PathBuf>,

    /// Pretty-print the JSON output.
    #[arg(long)]
    pub pretty: bool,

    /// Command tokens to check against the policy.
    #[arg(
        value_name = "COMMAND",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub command: Vec<String>,
}

impl ExecPolicyCheckCommand {
    /// Load the policies for this command, evaluate the command, and render JSON output.
    pub fn run(&self) -> Result<()> {
        let policy = load_policies(&self.rules)?;
        let matched_rules = policy.matches_for_command(&self.command);

        let json = format_matches_json(&matched_rules, self.pretty)?;
        println!("{json}");

        Ok(())
    }
}

pub fn format_matches_json(matched_rules: &[RuleMatch], pretty: bool) -> Result<String> {
    let output = ExecPolicyCheckOutput {
        matched_rules,
        decision: matched_rules.iter().map(RuleMatch::decision).max(),
    };

    if pretty {
        serde_json::to_string_pretty(&output).map_err(Into::into)
    } else {
        serde_json::to_string(&output).map_err(Into::into)
    }
}

pub fn load_policies(policy_paths: &[PathBuf]) -> Result<Policy> {
    let mut parser = PolicyParser::new();

    for policy_path in policy_paths {
        let policy_file_contents = fs::read_to_string(policy_path)
            .with_context(|| format!("failed to read policy at {}", policy_path.display()))?;
        let policy_identifier = policy_path.to_string_lossy().to_string();
        parser
            .parse(&policy_identifier, &policy_file_contents)
            .with_context(|| format!("failed to parse policy at {}", policy_path.display()))?;
    }

    Ok(parser.build())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecPolicyCheckOutput<'a> {
    #[serde(rename = "matchedRules")]
    matched_rules: &'a [RuleMatch],
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<Decision>,
}

#[cfg(all(test, not(target_env = "ohos")))]
mod tests {
    use super::*;

    #[test]
    fn starlark_check_loads_matches_and_renders_the_cli_contract() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("readonly.star");
        fs::write(
            &path,
            r#"prefix_rule(
    pattern = ["git", "status"],
    decision = "allow",
    match = [["git", "status"]],
    not_match = [["git", "push"]],
    justification = "只读检查",
)"#,
        )
        .unwrap();

        let policy = load_policies(&[path]).expect("Starlark policy loads");
        let matched =
            policy.matches_for_command(&["git".into(), "status".into(), "--short".into()]);
        let rendered = format_matches_json(&matched, false).expect("CLI JSON renders");
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(value["decision"], "allow");
        assert_eq!(value["matchedRules"].as_array().map(Vec::len), Some(1));
        assert!(rendered.contains("只读检查"));
    }
}
