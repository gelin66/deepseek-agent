//! Approval presentation metadata.
//!
//! Canonical risk comes from the runtime prompt. This module only classifies
//! tool names for display and impact summaries.

/// Categorizes tools for approval presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Free, read-only operations (`list_dir`, `read_file`, todo_*)
    Safe,
    /// File modifications (`write_file`, `edit_file`)
    FileWrite,
    /// Shell execution (`exec_shell`)
    Shell,
    /// Network-oriented built-in tools
    Network,
    /// Read-only MCP discovery and resource access
    McpRead,
    /// MCP actions that may change remote state
    McpAction,
    /// Sub-agent lifecycle (`agent` start/status/peek/cancel)
    Agent,
    /// Unknown or unclassified tool surface
    Unknown,
}

/// Presentation stakes copied from the canonical runtime approval risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStakes {
    Routine,
    Elevated,
    Critical,
}

/// Get the display category for a tool by name.
pub fn get_tool_category(name: &str) -> ToolCategory {
    if name == "agent" {
        ToolCategory::Agent
    } else if matches!(name, "write_file" | "edit_file" | "apply_patch") {
        ToolCategory::FileWrite
    } else if matches!(
        name,
        "web_run" | "web_search" | "fetch_url" | "wait_for_dev_server"
    ) {
        ToolCategory::Network
    } else if matches!(
        name,
        "exec_shell"
            | "task_shell_start"
            | "task_shell_wait"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "exec_wait"
            | "exec_interact"
    ) {
        ToolCategory::Shell
    } else if name.starts_with("list_mcp_")
        || name.starts_with("read_mcp_")
        || name.starts_with("get_mcp_")
    {
        ToolCategory::McpRead
    } else if name.starts_with("mcp_") {
        ToolCategory::McpAction
    } else if matches!(
        name,
        "read_file" | "list_dir" | "note" | "search" | "file_search" | "project" | "diagnostics"
    ) || name.starts_with("read_")
        || name.starts_with("list_")
        || name.starts_with("get_")
    {
        ToolCategory::Safe
    } else if name == "start_mcp_server" {
        ToolCategory::McpAction
    } else {
        ToolCategory::Unknown
    }
}
