//! Tool-run grouping for transcript collapse.

use super::{GenericToolCell, HistoryCell};
use dse_localization::{MessageId, ProductLanguage, tr_in};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRun {
    /// Original index of the first tool cell in `App::history`.
    pub start: usize,
    /// Number of collapsed cells in the run.
    pub count: usize,
    /// Dominant tool names, deduplicated and capped for summary rendering.
    pub tool_families: Vec<String>,
    /// Human-facing activity buckets for Cursor-style metadata rows.
    pub activity: ToolRunActivitySummary,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ToolRunActivitySummary {
    pub files: usize,
    pub searches: usize,
    pub commands: usize,
    pub edits: usize,
    pub delegates: usize,
    pub metadata: usize,
    pub other: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolRunActivity {
    File,
    Search,
    Command,
    Edit,
    Delegate,
    Metadata,
    Other,
}

impl ToolRunActivitySummary {
    fn record(&mut self, tool: &GenericToolCell) {
        match classify_tool_run_activity(tool) {
            ToolRunActivity::File => self.files += 1,
            ToolRunActivity::Search => self.searches += 1,
            ToolRunActivity::Command => self.commands += 1,
            ToolRunActivity::Edit => self.edits += 1,
            ToolRunActivity::Delegate => self.delegates += 1,
            ToolRunActivity::Metadata => self.metadata += 1,
            ToolRunActivity::Other => self.other += 1,
        }
    }
}

/// Detect contiguous runs of successful, low-risk tool cells.
///
/// Failed, running, patch, review, diff, and plan-update cells split runs so
/// important state never disappears into a summary row. Successful command
/// cells can join dense runs while the canonical record keeps their raw details
/// available without making routine verifier/shell work dominate the default
/// transcript.
pub fn detect_tool_runs(history: &[HistoryCell], min_size: usize) -> Vec<ToolRun> {
    if min_size == 0 {
        return Vec::new();
    }

    let mut runs = Vec::new();
    let mut index = 0;
    while index < history.len() {
        if !history.get(index).is_some_and(is_collapsible_tool_cell) {
            index += 1;
            continue;
        }

        let start = index;
        let mut names: Vec<String> = Vec::new();
        let mut activity = ToolRunActivitySummary::default();
        while index < history.len() && history.get(index).is_some_and(is_collapsible_tool_cell) {
            if let Some(HistoryCell::Tool(tool)) = history.get(index) {
                let name = tool.name.as_str();
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
                activity.record(tool);
            }
            index += 1;
        }

        let count = index - start;
        if count >= min_size {
            names.truncate(3);
            runs.push(ToolRun {
                start,
                count,
                tool_families: names,
                activity,
            });
        }
    }

    runs
}

fn is_collapsible_tool_cell(cell: &HistoryCell) -> bool {
    matches!(cell, HistoryCell::Tool(tool) if tool.is_success() && !tool.is_collapsible_guard())
}

pub(super) fn tool_name_is_collapse_guard(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "exec_shell"
        || normalized.contains("patch")
        || normalized.contains("write")
        || normalized.contains("edit")
        || normalized.contains("delete")
        || normalized.contains("remove")
        || normalized.contains("commit")
        || normalized.contains("push")
        || normalized.contains("review")
}

fn classify_tool_run_activity(tool: &GenericToolCell) -> ToolRunActivity {
    classify_tool_name_activity(&tool.name)
}

fn classify_tool_name_activity(name: &str) -> ToolRunActivity {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "read_file" | "list_dir" | "git_status" | "git_log" | "git_show" | "git_blame" => {
            ToolRunActivity::File
        }
        "grep_files" | "file_search" | "web_search" | "fetch_url" => ToolRunActivity::Search,
        "shell"
        | "exec_shell"
        | "exec_shell_wait"
        | "exec_shell_interact"
        | "exec_shell_cancel"
        | "task_shell_start"
        | "task_shell_wait"
        | "run_tests"
        | "run_verifiers"
        | "wait_for_dev_server"
        | "task_gate_run"
        | "validate_data" => ToolRunActivity::Command,
        "edit_file" | "apply_patch" | "write_file" | "git_diff" | "diff" => ToolRunActivity::Edit,
        "agent" => ToolRunActivity::Delegate,
        _ if normalized.contains("search")
            || normalized.contains("grep")
            || normalized.contains("find") =>
        {
            ToolRunActivity::Search
        }
        _ if normalized.contains("read")
            || normalized.contains("list")
            || normalized.contains("view")
            || normalized.contains("open") =>
        {
            ToolRunActivity::File
        }
        _ if normalized.contains("patch")
            || normalized.contains("write")
            || normalized.contains("edit")
            || normalized.contains("diff") =>
        {
            ToolRunActivity::Edit
        }
        _ if normalized.contains("run")
            || normalized.contains("exec")
            || normalized.contains("shell")
            || normalized.contains("test")
            || normalized.contains("check") =>
        {
            ToolRunActivity::Command
        }
        _ if normalized.contains("agent")
            || normalized.contains("delegate")
            || normalized.contains("fanout") =>
        {
            ToolRunActivity::Delegate
        }
        _ if normalized.contains("metadata")
            || normalized.contains("session")
            || normalized.contains("context") =>
        {
            ToolRunActivity::Metadata
        }
        _ => ToolRunActivity::Other,
    }
}

#[must_use]
pub fn tool_run_summary(run: &ToolRun, language: ProductLanguage) -> String {
    let activity = &run.activity;
    let mut parts = Vec::new();
    if activity.files > 0 {
        parts.push(counted(
            language,
            activity.files,
            MessageId::ToolRunFileSingular,
            MessageId::ToolRunFilePlural,
        ));
    }
    if activity.searches > 0 {
        parts.push(counted(
            language,
            activity.searches,
            MessageId::ToolRunSearchSingular,
            MessageId::ToolRunSearchPlural,
        ));
    }

    let mut clauses = Vec::new();
    if !parts.is_empty() {
        let mut explore_clause = tr_in(language, MessageId::ToolRunExplored)
            .replace("{items}", &parts.join(locale_separator(language)));
        if let Some(families) =
            activity_family_summary(run, &[ToolRunActivity::File, ToolRunActivity::Search])
        {
            explore_clause.push_str(": ");
            explore_clause.push_str(&families);
        }
        clauses.push(explore_clause);
    }
    if activity.commands > 0 {
        let command_count = counted(
            language,
            activity.commands,
            MessageId::ToolRunCommandSingular,
            MessageId::ToolRunCommandPlural,
        );
        let mut command_clause =
            tr_in(language, MessageId::ToolRunRan).replace("{items}", &command_count);
        if let Some(families) = activity_family_summary(run, &[ToolRunActivity::Command]) {
            command_clause.push_str(": ");
            command_clause.push_str(&families);
        }
        clauses.push(command_clause);
    }
    if activity.edits > 0 {
        let edit_count = counted(
            language,
            activity.edits,
            MessageId::ToolRunFileSingular,
            MessageId::ToolRunFilePlural,
        );
        clauses.push(tr_in(language, MessageId::ToolRunEdited).replace("{items}", &edit_count));
    }
    if activity.delegates > 0 {
        let task_count = counted(
            language,
            activity.delegates,
            MessageId::ToolRunTaskSingular,
            MessageId::ToolRunTaskPlural,
        );
        clauses.push(tr_in(language, MessageId::ToolRunDelegated).replace("{items}", &task_count));
    }
    if activity.metadata > 0 || activity.other > 0 {
        clauses.push(tr_in(language, MessageId::ToolRunUpdatedMetadata).into_owned());
    }

    if clauses.is_empty() {
        let summary = tr_in(language, MessageId::ToolRunUpdatedMetadata).into_owned();
        return sentence_case_activity(summary);
    }

    let summary = clauses.join(locale_separator(language));
    sentence_case_activity(summary)
}

fn activity_family_summary(run: &ToolRun, activities: &[ToolRunActivity]) -> Option<String> {
    let mut families = Vec::new();
    for family in &run.tool_families {
        if activities.contains(&classify_tool_name_activity(family))
            && !families.iter().any(|existing| existing == family)
        {
            families.push(family.as_str());
        }
    }

    (!families.is_empty()).then(|| families.join(", "))
}

fn counted(
    language: ProductLanguage,
    count: usize,
    singular: MessageId,
    plural: MessageId,
) -> String {
    tr_in(language, if count == 1 { singular } else { plural })
        .replace("{count}", &count.to_string())
}

fn locale_separator(language: ProductLanguage) -> &'static str {
    match language {
        ProductLanguage::English => ", ",
        ProductLanguage::SimplifiedChinese => "，",
    }
}

fn sentence_case_activity(text: String) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return text;
    };
    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    out
}
