mod deepseek;
mod paths;

pub mod persistence;
pub mod prompt_preferences;
pub mod setup_state;
pub mod user_constitution;

pub use codewhale_secrets::Secrets;
pub use deepseek::*;
pub use paths::*;
pub use prompt_preferences::{
    PromptPreferences, SettingsSource, load_prompt_preferences, load_settings_source, settings_path,
};
pub use setup_state::{
    ConstitutionAuthoring, ConstitutionChoice, ConstitutionSource, ConstitutionValidity,
    InheritedConfigFacts, RuntimePostureSource, SetupState, SetupStep, StepEntry, StepStatus,
};
pub use user_constitution::{
    AutonomyPreference, UntrustedDraftParse, UserConstitution, UserConstitutionLoad,
};

pub const CONFIG_FILE_NAME: &str = "config.toml";

#[cfg(test)]
mod tests;
