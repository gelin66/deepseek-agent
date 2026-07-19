//! Remaining legacy protocol types awaiting their owning replacement slices.

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

#[path = "runtime_contract/termination.rs"]
pub mod termination;
// Typed exec termination remains here because exec output, runtime, and CLI
// presentation still consume it.
