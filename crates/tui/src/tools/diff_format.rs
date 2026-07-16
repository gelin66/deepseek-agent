//! Temporary TUI re-export of the production unified-diff formatter.

// M4-C deletes this module after the remaining TUI-owned writer moves to its
// production owner. There is only one formatter implementation.
pub use codewhale_tools::make_unified_diff;
