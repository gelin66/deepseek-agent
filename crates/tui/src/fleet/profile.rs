//! Fleet profile vocabulary, local profile discovery, and config-facing aliases.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use codewhale_config::{
    FleetDelegationHints, FleetLoadout, FleetProfile, FleetProfilePermissions, FleetRole, FleetSlot,
};

pub use super::roster::ProfileOrigin;

pub const WORKSPACE_AGENT_PROFILE_DIR: &str = ".codewhale/agents";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub profile: FleetProfile,
    pub source: PathBuf,
    /// Roster layer this profile came from (#fleet-roster cutover (v0.8.67)).
    /// File-based loading in this module always yields `Workspace`; the
    /// roster stamps `BuiltIn` / `Config` for the other layers.
    pub origin: ProfileOrigin,
}

/// The minimum profile information needed to prevent a save from clobbering
/// another file.  Identity discovery intentionally accepts otherwise legacy
/// profile keys: an old route-policy field must not block authoring an
/// unrelated, current profile, but malformed TOML or an invalid id still fails
/// closed because the collision check cannot be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfileIdentity {
    pub id: String,
    pub source: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileToml {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    role_hint: Option<String>,
    #[serde(default)]
    base_role: Option<String>,
    #[serde(default)]
    persona: Option<String>,
    #[serde(default)]
    loadout: Option<String>,
    #[serde(default, alias = "model_hint", alias = "model_id")]
    model: Option<String>,
    /// Explicit provider id for `model` (#4093), e.g. `"deepseek"` or
    /// `"openrouter"`. Validated against the known `ApiProvider` vocabulary at
    /// load time — never inferred by sniffing `model` for a provider-shaped
    /// substring (EPIC #2608). `deny_unknown_fields` no longer needs to guard
    /// this name: it is now a first-class, validated field instead of a
    /// smuggled one.
    #[serde(default)]
    provider: Option<String>,
    /// Optional saved thinking tier for this profile (#4137). TOML may use
    /// the canonical `reasoning_effort` spelling or the UI-facing `thinking`
    /// / `reasoning` aliases; loading normalizes to a canonical setting label.
    #[serde(default, alias = "thinking", alias = "reasoning")]
    reasoning_effort: Option<String>,
    #[serde(default)]
    instructions: Option<AgentProfileInstructions>,
    #[serde(default)]
    tools: Option<AgentProfileTools>,
    #[serde(default)]
    permissions: Option<AgentProfilePermissionsToml>,
}

#[derive(Debug, Deserialize)]
struct AgentProfileIdentityToml {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileInstructions {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfileTools {
    #[serde(default)]
    posture: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProfilePermissionsToml {
    #[serde(default)]
    allow_shell: Option<bool>,
    #[serde(default)]
    trust: Option<bool>,
    #[serde(default)]
    approval_required: Option<bool>,
}

/// Load every valid workspace profile while reporting invalid neighbors
/// individually.  The runtime roster uses this path so one stale profile does
/// not hide a newly-authored valid profile (or the rest of the party).
pub fn load_workspace_agent_profiles_tolerant(
    workspace: impl AsRef<Path>,
) -> Result<(Vec<AgentProfile>, Vec<String>)> {
    let dir = workspace.as_ref().join(WORKSPACE_AGENT_PROFILE_DIR);
    let paths = agent_profile_paths(&dir)?;
    let mut profiles = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let mut identified = Vec::new();

    // Resolve identities first so duplicate ids fail closed as a group rather
    // than allowing whichever filename happens to sort first to win.
    for path in paths {
        match load_agent_profile_identity_file(&path) {
            Ok(identity) => {
                let canonical_id = identity.id.to_ascii_lowercase();
                if !seen.insert(canonical_id.clone()) {
                    duplicates.insert(canonical_id.clone());
                }
                identified.push((path, identity, canonical_id));
            }
            Err(err) => issues.push(format!("{err:#}")),
        }
    }

    for (path, _identity, canonical_id) in identified {
        if duplicates.contains(&canonical_id) {
            issues.push(format!(
                "duplicate agent profile id {} includes {}",
                canonical_id,
                path.display()
            ));
            continue;
        }
        match load_agent_profile_file(&path) {
            Ok(profile) => profiles.push(profile),
            Err(err) => issues.push(format!("{err:#}")),
        }
    }

    Ok((profiles, issues))
}

fn agent_profile_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    if !dir.is_dir() {
        bail!("agent profile path {} is not a directory", dir.display());
    }

    let mut paths = std::fs::read_dir(dir)
        .with_context(|| format!("reading agent profile dir {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading agent profile entries in {}", dir.display()))?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn load_agent_profile_identity_file(path: &Path) -> Result<AgentProfileIdentity> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent profile identity {}", path.display()))?;
    let parsed: AgentProfileIdentityToml = toml::from_str(&raw)
        .map_err(|err| anyhow!("parsing agent profile identity {}: {err}", path.display()))?;
    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("profile");
    let id = first_present([parsed.id.as_deref(), parsed.name.as_deref()])
        .unwrap_or(fallback_id)
        .to_string();
    validate_agent_profile_token(path, "id/name", &id)?;
    Ok(AgentProfileIdentity {
        id,
        source: path.to_path_buf(),
    })
}

fn load_agent_profile_file(path: &Path) -> Result<AgentProfile> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading agent profile {}", path.display()))?;
    let parsed: AgentProfileToml = toml::from_str(&raw)
        .map_err(|err| anyhow!("parsing agent profile {}: {err}", path.display()))?;
    agent_profile_from_toml(path, parsed)
}

fn agent_profile_from_toml(path: &Path, parsed: AgentProfileToml) -> Result<AgentProfile> {
    reject_permission_expansion(path, parsed.tools.as_ref(), parsed.permissions.as_ref())?;

    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("profile");
    let id = first_present([parsed.id.as_deref(), parsed.name.as_deref()])
        .unwrap_or(fallback_id)
        .to_string();
    validate_agent_profile_token(path, "id/name", &id)?;

    let role_name = first_present([
        parsed.base_role.as_deref(),
        parsed.role_hint.as_deref(),
        parsed.name.as_deref(),
    ])
    .unwrap_or(&id)
    .to_string();
    validate_agent_profile_token(path, "base_role/role_hint", &role_name)?;

    let loadout = first_present([parsed.loadout.as_deref()])
        .map(FleetLoadout::from_name)
        .unwrap_or_default();
    let model = non_empty_trimmed(parsed.model.as_deref()).map(str::to_string);
    validate_agent_profile_model_hint(path, model.as_deref())?;

    let provider = non_empty_trimmed(parsed.provider.as_deref())
        .map(str::to_string)
        .map(|provider| validate_agent_profile_provider(path, &provider).map(|()| provider))
        .transpose()?;
    let reasoning_effort =
        normalize_agent_profile_reasoning_effort(path, parsed.reasoning_effort.as_deref())?;

    let instructions = parsed
        .instructions
        .as_ref()
        .and_then(|instructions| non_empty_trimmed(instructions.text.as_deref()))
        .or_else(|| non_empty_trimmed(parsed.persona.as_deref()))
        .map(str::to_string);

    let description = non_empty_trimmed(parsed.description.as_deref()).map(str::to_string);
    let profile = FleetProfile {
        slot: FleetSlot::from_name(&role_name),
        role: FleetRole {
            name: role_name,
            description: description.clone(),
            instructions,
        },
        loadout,
        model,
        provider,
        reasoning_effort,
        permissions: FleetProfilePermissions::default(),
        delegation: FleetDelegationHints::default(),
    };

    Ok(AgentProfile {
        id,
        display_name: non_empty_trimmed(parsed.display_name.as_deref()).map(str::to_string),
        description,
        profile,
        source: path.to_path_buf(),
        origin: ProfileOrigin::Workspace,
    })
}

fn reject_permission_expansion(
    path: &Path,
    tools: Option<&AgentProfileTools>,
    permissions: Option<&AgentProfilePermissionsToml>,
) -> Result<()> {
    if let Some(posture) = tools
        .and_then(|tools| tools.posture.as_deref())
        .and_then(trimmed_non_empty)
    {
        match posture {
            "read-only" | "readonly" | "read_only" => {}
            other => bail!(
                "agent profile {} tools.posture={other:?} would widen permissions; use FleetProfile policy for grants",
                path.display()
            ),
        }
    }

    if let Some(permissions) = permissions {
        if permissions.allow_shell.unwrap_or(false) {
            bail!(
                "agent profile {} may not request allow_shell=true",
                path.display()
            );
        }
        if permissions.trust.unwrap_or(false) {
            bail!(
                "agent profile {} may not request trust=true",
                path.display()
            );
        }
        if permissions.approval_required == Some(false) {
            bail!(
                "agent profile {} may not disable approval_required",
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_agent_profile_token(path: &Path, field: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("agent profile {} {field} cannot be empty", path.display());
    }
    if trimmed != value || !trimmed.chars().all(is_agent_profile_token_char) {
        bail!(
            "agent profile {} {field} must be a simple token",
            path.display()
        );
    }
    Ok(())
}

fn validate_agent_profile_model_hint(path: &Path, value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if !is_model_hint(value) {
        bail!(
            "agent profile {} model must be a visible model id without whitespace or secrets",
            path.display()
        );
    }
    Ok(())
}

/// Validate an explicit `provider` field as a safe provider id (#4093).
///
/// Built-in providers are accepted by the runtime vocabulary, and user-named
/// OpenAI-compatible custom providers are accepted as simple tokens so the
/// launch path can resolve `[providers.<id>]` from the session config (#3965).
/// This field remains the ONLY place a profile's provider is established:
/// callers never infer it from `model` (EPIC #2608).
fn validate_agent_profile_provider(path: &Path, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("agent profile {} provider cannot be empty", path.display());
    }
    if trimmed != value || !trimmed.chars().all(is_agent_profile_token_char) {
        bail!(
            "agent profile {} provider must be a simple provider id",
            path.display()
        );
    }
    Ok(())
}

fn normalize_agent_profile_reasoning_effort(
    path: &Path,
    value: Option<&str>,
) -> Result<Option<String>> {
    let Some(value) = non_empty_trimmed(value) else {
        return Ok(None);
    };
    let normalized = match value.to_ascii_lowercase().as_str() {
        "inherit" | "parent" | "same" | "current" | "default" | "unset" => return Ok(None),
        "off" | "disabled" | "none" | "false" => "off",
        "low" | "minimal" => "low",
        "medium" | "mid" => "medium",
        "high" => "high",
        "auto" | "automatic" => "auto",
        "max" | "maximum" | "xhigh" | "ultracode" => "max",
        _ => bail!(
            "agent profile {} reasoning_effort {value:?} must be one of: inherit, auto, off, low, medium, high, max",
            path.display()
        ),
    };
    Ok(Some(normalized.to_string()))
}

fn is_agent_profile_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')
}

fn is_model_hint(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed == value
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_graphic() && !matches!(ch, '=' | '\'' | '"'))
}

fn first_present<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values.into_iter().flatten().find_map(trimmed_non_empty)
}

fn non_empty_trimmed(value: Option<&str>) -> Option<&str> {
    value.and_then(trimmed_non_empty)
}

fn trimmed_non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn profile_loader_normalizes_reasoning_aliases() {
        let dir = TempDir::new().unwrap();
        let path = write_profile(
            dir.path(),
            "scout.toml",
            r#"
id = "scout"
role_hint = "scout"
thinking = "xhigh"

[instructions]
text = "Scout deeply."
"#,
        );

        let profile = load_agent_profile_file(&path).expect("profile TOML loads");
        assert_eq!(profile.profile.reasoning_effort.as_deref(), Some("max"));
    }

    #[test]
    fn profile_loader_rejects_unknown_reasoning_effort() {
        let dir = TempDir::new().unwrap();
        let path = write_profile(
            dir.path(),
            "scout.toml",
            r#"
id = "scout"
role_hint = "scout"
reasoning = "expensive"
"#,
        );

        let err = load_agent_profile_file(&path).expect_err("invalid effort must fail");
        assert!(
            err.to_string().contains("reasoning_effort"),
            "unexpected error: {err}"
        );
    }

    fn write_profile(dir: &Path, filename: &str, contents: &str) -> PathBuf {
        let path = dir.join(filename);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn fleet_profile_round_trips_through_serde_with_safe_defaults() {
        let profile = FleetProfile::default();

        let serialized = toml::to_string(&profile).expect("profile serializes");
        let round_tripped: FleetProfile =
            toml::from_str(&serialized).expect("profile deserializes");

        assert_eq!(round_tripped, profile);
        assert_eq!(round_tripped.role.name, "general");
        assert_eq!(round_tripped.loadout, FleetLoadout::Inherit);
        assert!(!round_tripped.permissions.allow_shell);
        assert!(!round_tripped.permissions.trust);
        assert!(round_tripped.permissions.approval_required);
        assert_eq!(round_tripped.delegation.max_spawn_depth, None);
        assert_eq!(round_tripped.delegation.max_concurrency, None);
    }

    #[test]
    fn fleet_profile_explicit_toml_parses_role_loadout_permissions() {
        let profile: FleetProfile = toml::from_str(
            r#"
slot = "reviewer"
loadout = "deep-reasoning"

[role]
name = "verifier"
instructions = "Review the patch and produce verification evidence."

[permissions]
allow_shell = true
trust = true
approval_required = false

[delegation]
max_spawn_depth = 1
concurrency = 2
"#,
        )
        .expect("explicit fleet profile parses");

        assert_eq!(profile.slot, FleetSlot::Reviewer);
        assert_eq!(profile.role.name, "verifier");
        assert_eq!(
            profile.role.instructions.as_deref(),
            Some("Review the patch and produce verification evidence.")
        );
        assert_eq!(
            profile.loadout,
            FleetLoadout::Custom("deep-reasoning".to_string())
        );
        assert!(profile.permissions.allow_shell);
        assert!(profile.permissions.trust);
        assert!(!profile.permissions.approval_required);
        assert_eq!(profile.delegation.max_spawn_depth, Some(1));
        assert_eq!(profile.delegation.max_concurrency, Some(2));
    }

    #[test]
    fn fleet_profile_accepts_compact_role_string() {
        let profile: FleetProfile = toml::from_str(
            r#"
role = "scout"
loadout = "fast"
model = "deepseek-v4-flash"
"#,
        )
        .expect("compact fleet profile parses");

        assert_eq!(profile.role.name, "scout");
        assert_eq!(profile.loadout, FleetLoadout::Fast);
        assert_eq!(profile.model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(profile.permissions, FleetProfilePermissions::default());
    }

    #[test]
    fn agent_profile_loader_returns_empty_for_missing_workspace_dir() {
        let tmp = TempDir::new().unwrap();

        let (profiles, issues) = load_workspace_agent_profiles_tolerant(tmp.path()).unwrap();

        assert!(profiles.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn profile_identity_loader_accepts_legacy_route_policy_fields() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        let source = write_profile(
            &agents_dir,
            "reviewer.toml",
            r#"
id = "reviewer"
role_hint = "reviewer"
model_class_hint = "heavy"
models = ["glm-5.2", "deepseek-v4-pro"]
"#,
        );

        let identity = load_agent_profile_identity_file(&source)
            .expect("legacy fields do not obscure identity");

        assert_eq!(
            identity,
            AgentProfileIdentity {
                id: "reviewer".to_string(),
                source,
            }
        );
    }

    #[test]
    fn profile_identity_loader_fails_closed_for_malformed_toml() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        let path = write_profile(&agents_dir, "broken.toml", "id = [\n");

        let err = load_agent_profile_identity_file(&path)
            .expect_err("malformed TOML cannot prove collision safety")
            .to_string();

        assert!(err.contains("broken.toml"), "unexpected error: {err}");
        assert!(err.contains("profile identity"), "unexpected error: {err}");
    }

    #[test]
    fn tolerant_loader_keeps_valid_profile_beside_legacy_profile() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        write_profile(
            &agents_dir,
            "reviewer.toml",
            "id = \"reviewer\"\nmodel_class_hint = \"heavy\"\n",
        );
        write_profile(
            &agents_dir,
            "scout.toml",
            "id = \"scout\"\nrole_hint = \"scout\"\nprovider = \"deepseek\"\nmodel = \"deepseek-v4-flash\"\n",
        );

        let (profiles, issues) = load_workspace_agent_profiles_tolerant(tmp.path())
            .expect("directory discovery succeeds");

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "scout");
        assert_eq!(
            profiles[0].profile.model.as_deref(),
            Some("deepseek-v4-flash")
        );
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("reviewer.toml"), "{issues:?}");
        assert!(issues[0].contains("model_class_hint"), "{issues:?}");
    }

    #[test]
    fn tolerant_loader_skips_every_duplicate_id_but_keeps_unique_neighbors() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        write_profile(&agents_dir, "a.toml", "id = \"reviewer\"\n");
        write_profile(&agents_dir, "b.toml", "name = \"reviewer\"\n");
        write_profile(&agents_dir, "scout.toml", "id = \"scout\"\n");

        let (profiles, issues) = load_workspace_agent_profiles_tolerant(tmp.path())
            .expect("directory discovery succeeds");

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["scout"]
        );
        assert_eq!(issues.len(), 2);
        assert!(
            issues
                .iter()
                .all(|issue| issue.contains("duplicate agent profile id reviewer")),
            "{issues:?}"
        );
    }

    #[test]
    fn profile_identity_loader_fails_closed_for_invalid_id_token() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        let path = write_profile(&agents_dir, "broken.toml", "id = \"bad id\"\n");

        let err = load_agent_profile_identity_file(&path)
            .expect_err("invalid identity tokens cannot prove collision safety")
            .to_string();

        assert!(err.contains("broken.toml"), "unexpected error: {err}");
        assert!(err.contains("simple token"), "unexpected error: {err}");
    }

    #[test]
    fn agent_profile_loader_normalizes_project_agent_toml() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join(WORKSPACE_AGENT_PROFILE_DIR);
        std::fs::create_dir_all(&agents_dir).unwrap();
        let source = write_profile(
            &agents_dir,
            "reviewer.toml",
            r#"
name = "adversarial_reviewer"
display_name = "Adversarial Reviewer"
description = "Skeptical read-only review posture"
role_hint = "reviewer"
loadout = "balanced"
model = "deepseek-v4-pro"

[instructions]
text = "Focus on regressions, missing tests, and fragile assumptions."

[tools]
posture = "read-only"
"#,
        );

        let (profiles, issues) = load_workspace_agent_profiles_tolerant(tmp.path()).unwrap();

        assert_eq!(profiles.len(), 1);
        assert!(issues.is_empty());
        let profile = &profiles[0];
        assert_eq!(profile.id, "adversarial_reviewer");
        assert_eq!(
            profile.display_name.as_deref(),
            Some("Adversarial Reviewer")
        );
        assert_eq!(
            profile.description.as_deref(),
            Some("Skeptical read-only review posture")
        );
        assert_eq!(profile.profile.slot, FleetSlot::Reviewer);
        assert_eq!(profile.profile.role.name, "reviewer");
        assert_eq!(
            profile.profile.role.instructions.as_deref(),
            Some("Focus on regressions, missing tests, and fragile assumptions.")
        );
        assert_eq!(
            profile.profile.loadout,
            FleetLoadout::Custom("balanced".to_string())
        );
        assert_eq!(profile.profile.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(
            profile.profile.permissions,
            FleetProfilePermissions::default()
        );
        assert_eq!(profile.source, source);
    }

    #[test]
    fn agent_profile_loader_rejects_retired_model_policy_aliases() {
        for (field, value) in [("model_class_hint", "balanced"), ("route_tier", "fast")] {
            let tmp = TempDir::new().unwrap();
            let path = write_profile(
                tmp.path(),
                "reviewer.toml",
                &format!(
                    r#"
name = "reviewer"
role_hint = "reviewer"
{field} = "{value}"
"#
                ),
            );

            let err = load_agent_profile_file(&path).unwrap_err().to_string();

            assert!(
                err.contains(field) || err.contains("unknown field"),
                "unexpected error for {field}: {err}"
            );
        }
    }

    #[test]
    fn agent_profile_loader_accepts_and_round_trips_explicit_provider_field() {
        // #4093: `provider` is now a first-class, validated field — a Fleet
        // profile can name its own route explicitly, independent of whatever
        // provider is active when the profile is later loaded/launched.
        let tmp = TempDir::new().unwrap();
        let path = write_profile(
            tmp.path(),
            "reviewer.toml",
            r#"
name = "reviewer"
provider = "openrouter"
model = "deepseek/deepseek-v4-pro"
"#,
        );

        let profile = load_agent_profile_file(&path).expect("profile loads");
        assert_eq!(profile.profile.provider.as_deref(), Some("openrouter"));
        assert_eq!(
            profile.profile.model.as_deref(),
            Some("deepseek/deepseek-v4-pro")
        );
    }

    #[test]
    fn agent_profile_loader_accepts_custom_provider_name() {
        // #3965: LM Studio and other user-named OpenAI-compatible providers
        // are resolved from `[providers.<id>]` at launch time, so the profile
        // loader must preserve the safe id instead of requiring a built-in.
        let tmp = TempDir::new().unwrap();
        let path = write_profile(
            tmp.path(),
            "reviewer.toml",
            r#"
name = "reviewer"
provider = "lm-studio"
model = "qwen-2.5-7b"
"#,
        );

        let profile = load_agent_profile_file(&path).expect("profile loads");

        assert_eq!(profile.profile.provider.as_deref(), Some("lm-studio"));
        assert_eq!(profile.profile.model.as_deref(), Some("qwen-2.5-7b"));
    }

    #[test]
    fn agent_profile_loader_rejects_malformed_provider_name() {
        let tmp = TempDir::new().unwrap();
        let path = write_profile(
            tmp.path(),
            "reviewer.toml",
            r#"
name = "reviewer"
provider = "lm studio"
model = "some-model"
"#,
        );

        let err = load_agent_profile_file(&path).unwrap_err().to_string();

        assert!(
            err.contains("provider must be a simple provider id"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn agent_profile_loader_rejects_permission_expansion() {
        let tmp = TempDir::new().unwrap();
        let path = write_profile(
            tmp.path(),
            "builder.toml",
            r#"
name = "builder"

[tools]
posture = "read-write"
"#,
        );

        let err = load_agent_profile_file(&path).unwrap_err().to_string();

        assert!(
            err.contains("would widen permissions"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn agent_profile_loader_rejects_secret_like_model_hint() {
        let tmp = TempDir::new().unwrap();
        let path = write_profile(
            tmp.path(),
            "reviewer.toml",
            r#"
name = "reviewer"
model = "deepseek-v4-pro api_key=secret"
"#,
        );

        let err = load_agent_profile_file(&path).unwrap_err().to_string();

        assert!(
            err.contains("model must be a visible model id"),
            "unexpected error: {err}"
        );
    }
}
