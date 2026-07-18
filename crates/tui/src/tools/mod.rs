//! Tool system modules and re-exports.

// Tools run inside the TUI alt-screen runtime. Raw `print!` / `eprintln!`
// inside this module tree leaks into ratatui's diff-renderer buffer and
// produces the "scroll demon" regression (#1085 / v0.8.27 follow-up).
// Route status/error reporting through `tracing::*` instead — the
// `runtime_log` subscriber captures it to `~/.deepseek/logs/`.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod approval_cache;

pub mod handle;
pub mod plan;
pub mod spec;
pub mod todo;
pub mod truncate;
