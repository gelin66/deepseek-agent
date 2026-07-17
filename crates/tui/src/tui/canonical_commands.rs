//! Interactive commands that have real canonical Run semantics.
//!
//! This is the single source for command discovery, parsing, help text, and
//! dispatch intent. Commands from the retired TUI engine must not be surfaced
//! here until they have an equivalent `AgentApplication` command.

use crate::localization::{MessageId, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalSlashCommand {
    Help,
    Compact,
    Cost,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CanonicalSlashCommandInfo {
    pub command: CanonicalSlashCommand,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description_id: MessageId,
}

const COMMANDS: &[CanonicalSlashCommandInfo] = &[
    CanonicalSlashCommandInfo {
        command: CanonicalSlashCommand::Help,
        name: "help",
        aliases: &[],
        description_id: MessageId::CmdHelpDescription,
    },
    CanonicalSlashCommandInfo {
        command: CanonicalSlashCommand::Compact,
        name: "compact",
        aliases: &[],
        description_id: MessageId::CmdCompactDescription,
    },
    CanonicalSlashCommandInfo {
        command: CanonicalSlashCommand::Cost,
        name: "cost",
        aliases: &[],
        description_id: MessageId::CmdCostDescription,
    },
    CanonicalSlashCommandInfo {
        command: CanonicalSlashCommand::Exit,
        name: "exit",
        aliases: &["quit"],
        description_id: MessageId::CmdExitDescription,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CanonicalSlashParse {
    NotCommand,
    Command(CanonicalSlashCommand),
    Error(String),
}

pub(crate) fn command_infos() -> &'static [CanonicalSlashCommandInfo] {
    COMMANDS
}

#[cfg(test)]
pub(crate) fn command_info(command: CanonicalSlashCommand) -> &'static CanonicalSlashCommandInfo {
    COMMANDS
        .iter()
        .find(|info| info.command == command)
        .expect("every canonical slash command has metadata")
}

pub(crate) fn looks_like_command_input(input: &str) -> bool {
    let trimmed = input.trim_start();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    if rest.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let Some(command) = rest.split_whitespace().next() else {
        return rest.is_empty();
    };
    !command.contains('/')
}

pub(crate) fn parse(input: &str) -> CanonicalSlashParse {
    if !looks_like_command_input(input) {
        return CanonicalSlashParse::NotCommand;
    }
    let trimmed = input.trim();
    let body = trimmed
        .strip_prefix('/')
        .expect("command syntax was checked above");
    let mut parts = body.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().to_ascii_lowercase();
    let arguments = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(info) = COMMANDS.iter().find(|info| {
        info.name.eq_ignore_ascii_case(&name)
            || info
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(&name))
    }) else {
        return CanonicalSlashParse::Error(if name.is_empty() {
            tr(MessageId::CanonicalCommandRequired).into_owned()
        } else {
            tr(MessageId::CanonicalCommandUnknown).replace("{name}", &name)
        });
    };

    if arguments.is_some() {
        return CanonicalSlashParse::Error(
            tr(MessageId::CanonicalCommandNoArguments).replace("{name}", info.name),
        );
    }
    CanonicalSlashParse::Command(info.command)
}

pub(crate) fn matching_command_infos(
    input: &str,
    limit: usize,
) -> Vec<&'static CanonicalSlashCommandInfo> {
    if limit == 0 {
        return Vec::new();
    }
    let trimmed = input.trim_start();
    let Some(prefix) = trimmed.strip_prefix('/') else {
        return Vec::new();
    };
    if prefix.contains(char::is_whitespace) {
        return Vec::new();
    }
    let prefix = prefix.to_ascii_lowercase();
    let mut matches = COMMANDS
        .iter()
        .filter(|info| {
            info.name.starts_with(&prefix)
                || info.aliases.iter().any(|alias| alias.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|info| {
        let exact = info.name == prefix || info.aliases.iter().any(|alias| *alias == prefix);
        (!exact, info.name)
    });
    matches.truncate(limit);
    matches
}

pub(crate) fn help_text() -> String {
    let mut lines = vec![tr(MessageId::CanonicalCommandListTitle).into_owned()];
    for info in COMMANDS {
        let aliases = if info.aliases.is_empty() {
            String::new()
        } else {
            let aliases = info
                .aliases
                .iter()
                .map(|alias| format!("/{alias}"))
                .collect::<Vec<_>>();
            tr(MessageId::CanonicalCommandAliases).replace("{aliases}", &aliases.join("、"))
        };
        lines.push(format!(
            "/{}{} — {}",
            info.name,
            aliases,
            tr(info.description_id)
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_discovered_command_is_parseable_by_the_same_contract() {
        for info in command_infos() {
            assert_eq!(
                parse(&format!("/{}", info.name)),
                CanonicalSlashParse::Command(info.command)
            );
        }
    }

    #[test]
    fn aliases_parse_to_their_canonical_command() {
        assert_eq!(
            parse("/quit"),
            CanonicalSlashParse::Command(CanonicalSlashCommand::Exit)
        );
        assert_eq!(
            matching_command_infos("/qui", 10),
            vec![command_info(CanonicalSlashCommand::Exit)]
        );
    }

    #[test]
    fn normal_input_unknown_commands_and_arguments_are_distinct() {
        assert_eq!(parse("修复这个错误"), CanonicalSlashParse::NotCommand);
        assert_eq!(
            parse("/Users/gelin/Desktop/codewhale"),
            CanonicalSlashParse::NotCommand
        );
        assert_eq!(
            parse("$skill-name 处理任务"),
            CanonicalSlashParse::NotCommand
        );
        assert!(matches!(parse("/provider"), CanonicalSlashParse::Error(_)));
        assert!(matches!(
            parse("/compact now"),
            CanonicalSlashParse::Error(_)
        ));
    }

    #[test]
    fn help_is_derived_from_the_complete_command_contract() {
        let help = help_text();
        for info in command_infos() {
            assert!(help.contains(&format!("/{}", info.name)));
        }
    }

    #[test]
    fn names_aliases_and_localized_descriptions_are_complete_and_unique() {
        let mut names = HashSet::new();
        for info in command_infos() {
            assert!(names.insert(info.name), "duplicate command: {}", info.name);
            for alias in info.aliases {
                assert!(names.insert(*alias), "duplicate command alias: {alias}");
            }
            assert!(!tr(info.description_id).trim().is_empty());
        }
    }
}
