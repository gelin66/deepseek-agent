//! Remaining legacy protocol types awaiting their owning replacement slices.

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod authority;
pub mod events;
pub mod ops;
#[path = "runtime_contract/termination.rs"]
pub mod termination;
// The rest of `runtime_contract/` stays on disk as staged Core-runtime
// scaffolding and remains deliberately uncompiled until it has production
// consumers (TUI-DOG-017).
