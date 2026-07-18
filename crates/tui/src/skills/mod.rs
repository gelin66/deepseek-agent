//! Bundled system-skill management.
//!
//! Discovery, parsing, and prompt rendering are owned by
//! `codewhale-context`; this module only installs first-party skills bundled
//! into the local binary.

mod system;

pub use system::install_system_skills;
