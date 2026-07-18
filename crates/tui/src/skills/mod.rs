//! Interactive skill installation and system-skill management.
//!
//! Discovery, parsing, and prompt rendering are owned by
//! `codewhale-context`; this module retains only TUI mutation surfaces.

pub mod install;
mod system;

#[allow(unused_imports)]
pub use install::{
    DEFAULT_MAX_SIZE_BYTES, DEFAULT_REGISTRY_URL, INSTALLED_FROM_MARKER, InstallOutcome,
    InstallSource, InstalledSkill, RegistryDocument, RegistryEntry, RegistryFetchResult,
    SkillSyncOutcome, SyncResult, UpdateResult, default_cache_skills_dir,
};
pub use system::install_system_skills;
