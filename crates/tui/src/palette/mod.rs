//! The single terminal-native DSE presentation token owner.
//!
//! Renderers consume these semantic roles directly. Terminal color-depth
//! adaptation happens once in the backend; there is no selectable theme,
//! background override, or second palette reader.

mod adapt;
mod tokens;

#[cfg(test)]
mod tests;

pub use adapt::*;
pub use tokens::*;
