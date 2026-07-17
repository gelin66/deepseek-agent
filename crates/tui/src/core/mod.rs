//! Remaining legacy protocol types awaiting their owning replacement slices.

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod authority;
pub mod events;
// The first production consumer of the staged runtime contract is the
// provider-neutral model boundary. Keep the remaining contract files staged
// until their own consumers land instead of compiling dead scaffolding.
#[path = "runtime_contract/model.rs"]
pub mod model_client;
pub mod ops;
#[path = "runtime_contract/termination.rs"]
pub mod termination;
// The rest of `runtime_contract/` stays on disk as staged Core-runtime
// scaffolding and remains deliberately uncompiled until it has production
// consumers (TUI-DOG-017).
