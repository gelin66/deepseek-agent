//! Canonical allow/deny matching for `execpolicy.toml`.
//!
//! Permission presets are owned by the Run protocol. This crate deliberately
//! owns only the existing explicit Shell allow/deny rules: it does not define
//! an approval mode, session allow-list, sandbox bypass, network amendment, or
//! a second permission language.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One named allow/deny group in `execpolicy.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecPolicyRuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Immutable policy snapshot frozen into a Run execution fingerprint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecPolicy {
    #[serde(default)]
    pub rules: BTreeMap<String, ExecPolicyRuleSet>,
}

/// Explicit disposition returned by the canonical matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecPolicyDisposition {
    Allow,
    Deny,
}

/// Stable match evidence consumed by tools and `dse execpolicy check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecPolicyMatch {
    pub disposition: ExecPolicyDisposition,
    pub group: String,
    pub pattern: String,
}

impl ExecPolicyMatch {
    #[must_use]
    pub fn rule_label(&self) -> String {
        let action = match self.disposition {
            ExecPolicyDisposition::Allow => "allow",
            ExecPolicyDisposition::Deny => "deny",
        };
        format!("{}:{action}:{}", self.group, self.pattern)
    }
}

impl ExecPolicy {
    pub fn parse_toml(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse execpolicy.toml")
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read execpolicy file {}", path.display()))?;
        Self::parse_toml(&contents)
    }

    pub fn from_optional_path(path: &Path) -> Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        Self::from_path(path).map(Some)
    }

    /// Match one Shell command. Explicit deny always wins. Allow applies only
    /// to a single command segment; it cannot bless a chained suffix.
    #[must_use]
    pub fn evaluate(&self, command: &str) -> Option<ExecPolicyMatch> {
        let segments = command_segments(command);
        for (group, rules) in &self.rules {
            for pattern in &rules.deny {
                if segments
                    .iter()
                    .any(|segment| command_pattern_matches(pattern, segment))
                    || deny_pattern_occurs(pattern, command)
                {
                    return Some(ExecPolicyMatch {
                        disposition: ExecPolicyDisposition::Deny,
                        group: group.clone(),
                        pattern: pattern.clone(),
                    });
                }
            }
        }

        if segments.len() != 1 || has_dynamic_shell_composition(command) {
            return None;
        }
        let command = segments.first().map_or(command, String::as_str);
        for (group, rules) in &self.rules {
            for pattern in &rules.allow {
                if command_pattern_matches(pattern, command) {
                    return Some(ExecPolicyMatch {
                        disposition: ExecPolicyDisposition::Allow,
                        group: group.clone(),
                        pattern: pattern.clone(),
                    });
                }
            }
        }
        None
    }
}

fn command_segments(command: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    fn push_segment(segments: &mut Vec<String>, current: &mut String) {
        let normalized = normalize_command(current);
        if !normalized.is_empty() {
            segments.push(normalized);
        }
        current.clear();
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = Quote::None;
    let mut escaped = false;

    while let Some(character) = chars.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match quote {
            Quote::Single => {
                current.push(character);
                if character == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => {
                current.push(character);
                if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    quote = Quote::None;
                }
            }
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    current.push(character);
                }
                '"' => {
                    quote = Quote::Double;
                    current.push(character);
                }
                '\\' => {
                    escaped = true;
                    current.push(character);
                }
                ';' | '\n' => push_segment(&mut segments, &mut current),
                '|' => {
                    if chars.peek() == Some(&'|') {
                        chars.next();
                    }
                    push_segment(&mut segments, &mut current);
                }
                '&' if chars.peek() == Some(&'&') => {
                    chars.next();
                    push_segment(&mut segments, &mut current);
                }
                '&' if !current.ends_with('>')
                    && !current.ends_with('<')
                    && chars.peek() != Some(&'>') =>
                {
                    push_segment(&mut segments, &mut current);
                }
                _ => current.push(character),
            },
        }
    }
    push_segment(&mut segments, &mut current);
    segments
}

fn has_dynamic_shell_composition(command: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut quote = Quote::None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                }
            }
            Quote::Double => {
                if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    quote = Quote::None;
                } else if character == '`'
                    || matches!(character, '$' | '<' | '>') && chars.peek() == Some(&'(')
                {
                    return true;
                }
            }
            Quote::None => match character {
                '\\' => escaped = true,
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '`' => return true,
                '$' | '<' | '>' if chars.peek() == Some(&'(') => return true,
                _ => {}
            },
        }
    }
    false
}

fn normalize_command(command: &str) -> String {
    shlex::split(command).map_or_else(
        || command.split_whitespace().collect::<Vec<_>>().join(" "),
        |tokens| tokens.join(" "),
    )
}

fn command_pattern_matches(pattern: &str, command: &str) -> bool {
    let pattern = normalize_command(pattern);
    let command = normalize_command(command);
    if pattern == "*" {
        return true;
    }
    if pattern.contains('*') {
        return wildcard_matches(&pattern, &command);
    }
    command == pattern
        || (command.starts_with(&pattern)
            && command
                .as_bytes()
                .get(pattern.len())
                .is_some_and(u8::is_ascii_whitespace))
}

fn deny_pattern_occurs(pattern: &str, command: &str) -> bool {
    let pattern = normalize_command(pattern);
    if pattern.is_empty() {
        return false;
    }
    if !pattern.contains('*') {
        return command.match_indices(&pattern).any(|(start, matched)| {
            let end = start + matched.len();
            let before = command[..start].chars().next_back();
            let after = command[end..].chars().next();
            before.is_none_or(shell_boundary) && after.is_none_or(shell_boundary)
        });
    }

    let mut starts = vec![0];
    let mut ends = Vec::new();
    for (index, character) in command.char_indices() {
        if shell_boundary(character) {
            ends.push(index);
            starts.push(index + character.len_utf8());
        }
    }
    ends.push(command.len());
    starts.sort_unstable();
    starts.dedup();
    ends.sort_unstable();
    ends.dedup();
    starts.into_iter().any(|start| {
        ends.iter().copied().filter(|end| *end > start).any(|end| {
            let candidate = command[start..end].trim();
            !candidate.is_empty() && wildcard_matches(&pattern, &normalize_command(candidate))
        })
    })
}

fn shell_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            ';' | '&' | '|' | '(' | ')' | '$' | '`' | '\'' | '"'
        )
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star_index, mut star_value_index) = (None, 0);

    while value_index < value.len() {
        if pattern.get(pattern_index) == value.get(value_index) {
            pattern_index += 1;
            value_index += 1;
        } else if pattern.get(pattern_index) == Some(&b'*') {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }
    while pattern.get(pattern_index) == Some(&b'*') {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ExecPolicy {
        ExecPolicy::parse_toml(
            r#"
[rules.shell]
allow = ["git status", "cargo test *"]
deny = ["git push --force", "npm publish", "touch *"]
"#,
        )
        .expect("policy fixture")
    }

    #[test]
    fn deny_wins_and_matches_chained_segments() {
        let policy = fixture();
        let decision = policy
            .evaluate("git status && npm publish --tag latest")
            .expect("deny match");
        assert_eq!(decision.disposition, ExecPolicyDisposition::Deny);
        assert_eq!(decision.rule_label(), "shell:deny:npm publish");
        let substitution = policy
            .evaluate("git status $(npm publish --tag latest)")
            .expect("deny inside command substitution");
        assert_eq!(substitution.disposition, ExecPolicyDisposition::Deny);
        let wildcard_substitution = policy
            .evaluate("echo $(touch /tmp/release-marker)")
            .expect("wildcard deny inside command substitution");
        assert_eq!(wildcard_substitution.rule_label(), "shell:deny:touch *");
    }

    #[test]
    fn allow_is_prefix_aware_but_never_blesses_a_chain() {
        let policy = fixture();
        assert_eq!(
            policy
                .evaluate("git status --short")
                .expect("allow")
                .disposition,
            ExecPolicyDisposition::Allow
        );
        assert!(
            policy
                .evaluate("git status; git push origin main")
                .is_none()
        );
        assert!(policy.evaluate("git statusx").is_none());
        assert_eq!(
            policy
                .evaluate("git status 'branch;still-one-argument'")
                .expect("quoted separator stays inside one command")
                .disposition,
            ExecPolicyDisposition::Allow
        );
        assert!(
            policy
                .evaluate("git status & git push origin main")
                .is_none()
        );
        assert_eq!(
            policy
                .evaluate("git status 2>&1")
                .expect("file descriptor redirection is not a background chain")
                .disposition,
            ExecPolicyDisposition::Allow
        );
        for composed in [
            "git status $(printf x)",
            "git status `printf x`",
            "git status <(printf x)",
            "git status \"$(printf x)\"",
        ] {
            assert!(
                policy.evaluate(composed).is_none(),
                "allow must not bless a nested command: {composed}"
            );
        }
        assert_eq!(
            policy
                .evaluate("git status '$(printf x)'")
                .expect("single-quoted text does not execute")
                .disposition,
            ExecPolicyDisposition::Allow
        );
    }

    #[test]
    fn parser_rejects_a_parallel_permission_language() {
        assert!(
            ExecPolicy::parse_toml(
                r#"
[rules.shell]
ask = ["git push"]
"#
            )
            .is_err()
        );
    }
}
