//! Terminal UI (TUI) module for `DeepSeek` CLI.

// The rendering layer runs inside the alt-screen. Raw stdio prints
// produce the scroll demon (see `runtime_log` for full context). Use
// `tracing::*` for diagnostics — `runtime_log` captures it to disk.
// `ui::run_event_loop` legitimately prints a post-exit resume hint
// AFTER `LeaveAlternateScreen`; that single site uses
// `#[allow(clippy::print_stdout)]` locally.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

// === Submodules ===

pub mod active_cell;
pub mod app;
pub mod approval;
pub mod auto_review;
mod canonical_commands;
pub mod child_agents;
pub mod clipboard;
pub mod color_compat;
pub mod composer_chrome;
pub mod composer_ui;
pub mod diff_render;
pub mod file_frecency;
pub mod file_mention;
pub mod footer_ui;
pub mod format_helpers;
pub mod history;
pub mod key_shortcuts;
pub mod keybindings;
pub mod markdown_render;
mod mcp_routing;
pub mod notifications;
pub mod ocean;
pub mod onboarding;
pub mod osc8;
pub mod output_rows_cache;
pub mod pager;
pub mod paste;
pub mod paste_burst;
pub mod phase_strip;
pub mod run_client;
pub mod run_presenter;
pub mod run_projection;
pub mod scrolling;
pub mod selection;
pub mod sidebar;
pub mod slash_menu;
pub mod spinner;
pub mod transcript;
pub mod ui;
mod ui_text;
pub mod underwater;
pub mod user_input;
pub mod views;
pub mod vim_mode;
pub mod widgets;
pub mod work_surface;
pub mod workspace_context;

// === Re-exports ===

pub use app::{InitialInput, TuiOptions};
pub use ui::run_tui;
