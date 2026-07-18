//! Fleet worker launch input and route receipt projection.
//!
//! Fleet workers execute only through the canonical `codewhale exec`
//! subprocess path in `fleet::executor`. This module composes the task prompt
//! and the route/reasoning arguments that the executor actually places on argv.
//! Task-level role/tool permissions are not projected as enforced facts.

use anyhow::{Result, bail};
use codewhale_protocol::fleet::{FleetResolvedRoute, FleetTaskSpec, FleetTaskWorkerProfile};

use super::profile::AgentProfile;
use crate::config::ApiProvider;
use crate::route_runtime::resolve_route_candidate;

/// Validate that every task referencing a workspace agent profile can resolve it.
///
/// This is intended to run at Fleet run creation time, before leasing any
/// worker or appending lifecycle events.
pub fn validate_task_agent_profiles(
    tasks: &[FleetTaskSpec],
    agent_profiles: &[AgentProfile],
) -> Result<()> {
    for task in tasks {
        resolve_task_agent_profile(task, agent_profiles)?;
    }
    Ok(())
}

/// Mint a [`FleetResolvedRoute`] snapshot for a fleet task (#3154).
///
/// This calls the existing hermetic resolver bridge
/// ([`resolve_route_candidate`]) so the persisted route reflects the same
/// resolution semantics the runtime would use, then records only non-sensitive
/// shape (provider id/kind, model ids, protocol) combined with the already
/// computed effective role/loadout/model-class intent. `source` is
/// `"resolver"`.
///
/// Honesty rules:
/// - `canonical_model` stays `None` when the resolver could not pin one.
/// - The provider comes from the resolved agent profile's own explicit
///   `provider` field when it has one (#4093) — a Fleet worker profile can be
///   pinned to a route independent of the parent/current session provider.
///   Absent an explicit pin, the worker profile carries no provider authority
///   and resolution falls back to the existing default scope. Either way, the
///   provider is NEVER inferred by sniffing a substring/prefix out of `model`
///   (EPIC #2608: explicit config only). A task-level `model` selector is
///   forwarded as the model selector. No reasoning/pricing fields are
///   fabricated.
///
/// Returns `None` (never a fabricated route) when resolution fails, so callers
/// degrade gracefully without inventing detail.
pub(crate) fn resolve_fleet_route(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
    session_model: Option<&str>,
) -> Option<FleetResolvedRoute> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)
        .ok()
        .flatten();
    let worker_profile = task_spec.worker.as_ref();
    let (role, role_source) = effective_fleet_role_with_source(worker_profile, agent_profile);
    let (loadout, loadout_source) =
        effective_fleet_loadout_with_source(worker_profile, agent_profile);
    let (model_class, model_class_source) = task_model_class_with_source(worker_profile);

    // Task/profile model pins are visible route intent; next the session
    // route (the operator's model) applies as the run-level fallback; only
    // then does the resolver pick the provider default.
    let (model_selector, model_source) =
        fleet_route_model_selector_with_source(worker_profile, agent_profile, session_model);
    let model_selector = model_selector.as_deref();

    // Resolve within the profile's own explicit provider scope when it has
    // one (#4093); otherwise fall back to the existing default scope (mirrors
    // `ProviderKind::default()`). The resolver is fully offline/hermetic and
    // never reads secrets, env, or config. User-named custom providers need the
    // session Config to resolve their table, so this receipt path omits the
    // route instead of fabricating DeepSeek details for them (#3965).
    let provider = match explicit_fleet_provider_id(agent_profile).as_deref() {
        Some(provider_id) => ApiProvider::parse(provider_id)?,
        None => ApiProvider::Deepseek,
    };
    let candidate = resolve_route_candidate(provider, model_selector, None, None, None).ok()?;

    Some(FleetResolvedRoute {
        provider_id: candidate.provider_id.as_str().to_string(),
        provider_kind: candidate.provider_kind.as_str().to_string(),
        canonical_model: candidate
            .canonical_model
            .as_ref()
            .map(|model| model.as_str().to_string()),
        wire_model_id: candidate.wire_model_id.as_str().to_string(),
        protocol: route_protocol_label(candidate.protocol).to_string(),
        role,
        loadout: loadout_intent_label(&loadout),
        model_class,
        model_route: Some(
            fleet_model_route_label(model_selector.unwrap_or("auto"), &loadout).to_string(),
        ),
        reasoning_effort: effective_fleet_reasoning_effort(agent_profile),
        role_source: role_source.map(str::to_string),
        loadout_source: loadout_source.map(str::to_string),
        model_class_source: model_class_source.map(str::to_string),
        model_source: Some(model_source.to_string()),
        source: "resolver".to_string(),
    })
}

/// Plain-string label for a resolved wire protocol (no config type leaks).
fn route_protocol_label(protocol: codewhale_config::route::RequestProtocol) -> &'static str {
    use codewhale_config::route::RequestProtocol;
    match protocol {
        RequestProtocol::ChatCompletions => "chat_completions",
        RequestProtocol::Responses => "responses",
        RequestProtocol::AnthropicMessages => "anthropic_messages",
    }
}

/// Collapse an `inherit` (no-op) loadout to `None` for the receipt.
fn loadout_intent_label(loadout: &codewhale_config::FleetLoadout) -> Option<String> {
    if *loadout == codewhale_config::FleetLoadout::Inherit {
        None
    } else {
        Some(loadout.as_str().to_string())
    }
}

fn fleet_model_route_label(model: &str, loadout: &codewhale_config::FleetLoadout) -> &'static str {
    let model = model.trim();
    if !model.is_empty() && !model.eq_ignore_ascii_case("auto") {
        return "fixed";
    }
    match loadout {
        codewhale_config::FleetLoadout::Inherit => "inherit",
        codewhale_config::FleetLoadout::Fast => "faster",
        codewhale_config::FleetLoadout::Custom(_) => "auto",
    }
}

pub(crate) fn fleet_task_prompt(task_spec: &FleetTaskSpec) -> String {
    fleet_task_prompt_with_profile(task_spec, None)
}

pub(crate) fn fleet_task_prompt_with_profiles(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
) -> Result<String> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)?;
    Ok(fleet_task_prompt_with_profile(task_spec, agent_profile))
}

fn fleet_task_prompt_with_profile(
    task_spec: &FleetTaskSpec,
    agent_profile: Option<&AgentProfile>,
) -> String {
    let role = task_spec
        .worker
        .as_ref()
        .and_then(|worker| worker.role.as_deref())
        .or_else(|| agent_profile.map(|profile| profile.profile.role.name.as_str()))
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("general");
    let mut prompt = String::new();
    prompt.push_str("You have been summoned as a CodeWhale Fleet member (");
    prompt.push_str(role);
    prompt.push_str(") by the Fleet orchestrator.\n\n");
    prompt.push_str("Fleet operating contract:\n");
    prompt.push_str("- Work only the assigned slice; keep sibling or topology assumptions out of your answer.\n");
    prompt.push_str("- Use the policy-gated tools available in this headless worker run.\n");
    prompt.push_str("- Treat the active provider/model route as inherited unless this task or profile pins a model.\n");
    prompt.push_str(
        "- Return concise evidence, gaps, and next actions; the orchestrator will integrate and verify.\n\n",
    );
    prompt.push_str("Fleet task: ");
    prompt.push_str(&task_spec.name);

    if let Some(objective) = task_spec.objective.as_deref() {
        prompt.push_str("\n\nObjective:\n");
        prompt.push_str(objective);
    } else if let Some(description) = task_spec.description.as_deref() {
        prompt.push_str("\n\nObjective:\n");
        prompt.push_str(description);
    }

    prompt.push_str("\n\nInstructions:\n");
    prompt.push_str(&task_spec.instructions);

    if !task_spec.context.is_empty() {
        prompt.push_str("\n\nContext:\n");
        for item in &task_spec.context {
            prompt.push_str("- ");
            prompt.push_str(item);
            prompt.push('\n');
        }
    }

    if !task_spec.input_files.is_empty() {
        prompt.push_str("\nInput files:\n");
        for path in &task_spec.input_files {
            prompt.push_str("- ");
            prompt.push_str(&path.display().to_string());
            prompt.push('\n');
        }
    }

    if let Some(agent_profile) = agent_profile {
        prompt.push_str("\nFleet profile: ");
        prompt.push_str(&agent_profile.id);
        if let Some(display_name) = agent_profile.display_name.as_deref() {
            prompt.push_str(" (");
            prompt.push_str(display_name);
            prompt.push(')');
        }
        if let Some(description) = agent_profile.description.as_deref() {
            prompt.push_str("\nProfile description:\n");
            prompt.push_str(description);
        }
        if let Some(instructions) = agent_profile.profile.role.instructions.as_deref() {
            prompt.push_str("\nProfile instructions:\n");
            prompt.push_str(instructions);
        }
    }

    prompt
}

fn resolve_task_agent_profile<'a>(
    task_spec: &FleetTaskSpec,
    agent_profiles: &'a [AgentProfile],
) -> Result<Option<&'a AgentProfile>> {
    let Some(profile_id) = task_spec
        .worker
        .as_ref()
        .and_then(|worker| worker.agent_profile.as_deref())
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let Some(profile) = agent_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
    else {
        bail!(
            "fleet task {} references unknown agent profile {profile_id:?}",
            task_spec.id
        );
    };
    Ok(Some(profile))
}

fn effective_fleet_role_with_source(
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> (Option<String>, Option<&'static str>) {
    worker_profile
        .and_then(|worker| worker.role.as_deref())
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string)
        .map(|role| (Some(role), Some("task.role")))
        .unwrap_or_else(|| {
            agent_profile
                .map(|profile| {
                    (
                        Some(profile.profile.role.name.clone()),
                        Some("agent_profile.role"),
                    )
                })
                .unwrap_or((None, None))
        })
}

fn effective_fleet_loadout_with_source(
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> (codewhale_config::FleetLoadout, Option<&'static str>) {
    if let Some(model_class) = worker_profile
        .and_then(|worker| worker.model_class.as_deref())
        .and_then(non_empty_trimmed)
    {
        return (
            codewhale_config::FleetLoadout::from_name(model_class),
            Some("task.model_class"),
        );
    }
    if let Some(loadout) = worker_profile
        .and_then(|worker| worker.loadout.as_deref())
        .and_then(non_empty_trimmed)
    {
        return (
            codewhale_config::FleetLoadout::from_name(loadout),
            Some("task.loadout"),
        );
    }
    if let Some(loadout) = agent_profile
        .map(|profile| profile.profile.loadout.clone())
        .filter(|loadout| *loadout != codewhale_config::FleetLoadout::Inherit)
    {
        return (loadout, Some("agent_profile.loadout"));
    }
    (codewhale_config::FleetLoadout::Inherit, None)
}

fn effective_fleet_model(
    run_model: &str,
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> String {
    effective_fleet_model_with_source(run_model, worker_profile, agent_profile).0
}

fn effective_fleet_model_with_source(
    run_model: &str,
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
) -> (String, &'static str) {
    if let Some(model) = worker_profile
        .and_then(|worker| worker.model.as_deref())
        .and_then(non_empty_trimmed)
    {
        return (model.to_string(), "task.model");
    }
    if let Some(model) = agent_profile
        .and_then(|profile| profile.profile.model.as_deref())
        .and_then(non_empty_trimmed)
    {
        return (model.to_string(), "agent_profile.model");
    }
    (run_model.to_string(), "run.model")
}

/// The provider id a resolved agent profile EXPLICITLY pins, if any (#4093).
///
/// This preserves user-named OpenAI-compatible custom providers such as
/// `lm-studio` instead of collapsing them through [`ApiProvider`]. Runtime
/// launch paths can set `Config.provider` to this exact id so the normal config
/// resolver finds `[providers.<id>]` (#3965).
///
/// Returns `None` when no profile names a provider — never invents a DeepSeek
/// default — so launch paths can omit `--provider` and leave profile-less
/// workers on their own session default. EPIC #2608: never inferred from
/// `model`.
pub(crate) fn explicit_fleet_provider_id(agent_profile: Option<&AgentProfile>) -> Option<String> {
    agent_profile
        .and_then(|profile| profile.profile.provider.as_deref())
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_string)
}

pub(crate) fn effective_fleet_reasoning_effort(
    agent_profile: Option<&AgentProfile>,
) -> Option<String> {
    agent_profile
        .and_then(|profile| profile.profile.reasoning_effort.as_deref())
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
        .map(str::to_string)
}

/// The explicit reasoning/thinking tier a fleet worker should launch with.
///
/// This is the launch-side twin of the receipt/runtime-profile field: it reads
/// only the resolved AgentProfile tier, so task model overrides can change the
/// model without accidentally inventing a thinking tier.
pub(crate) fn fleet_worker_launch_reasoning_effort(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
) -> Option<String> {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)
        .ok()
        .flatten();
    effective_fleet_reasoning_effort(agent_profile)
}

/// The route (model selector + optional explicit provider id) that a fleet
/// worker's actual `codewhale exec` subprocess should launch on (#4093 AC #4).
///
/// This is the launch-side twin of [`resolve_fleet_route`] (the receipt): both
/// read the worker's model from the same task/profile/run precedence
/// ([`effective_fleet_model`]) and the provider from the same explicit-only
/// source ([`explicit_fleet_provider_id`]), so a worker whose profile is pinned
/// to provider B launches on provider B even when the parent session is on
/// provider A.
///
/// - `model`: never empty in practice — falls back to `run_model` when neither
///   the task nor the profile pins a model, matching pre-#4093 dispatch.
/// - `provider`: `Some(provider_id)` ONLY when the resolved agent profile
///   explicitly pins a provider. `None` means "no provider authority" — the
///   caller omits `--provider` and the worker keeps its own session default,
///   preserving today's behavior for profile-less workers. Built-ins use their
///   canonical ids; user-named custom providers preserve the profile's id so
///   `codewhale exec --provider <id>` can resolve `[providers.<id>]`.
pub(crate) fn fleet_worker_launch_route(
    task_spec: &FleetTaskSpec,
    agent_profiles: &[AgentProfile],
    run_model: &str,
) -> (String, Option<String>) {
    let agent_profile = resolve_task_agent_profile(task_spec, agent_profiles)
        .ok()
        .flatten();
    let worker_profile = task_spec.worker.as_ref();
    let model = effective_fleet_model(run_model, worker_profile, agent_profile);
    let provider = explicit_fleet_provider_id(agent_profile);
    (model, provider)
}

fn task_model_class_with_source(
    worker_profile: Option<&FleetTaskWorkerProfile>,
) -> (Option<String>, Option<&'static str>) {
    worker_profile
        .and_then(|worker| worker.model_class.as_deref())
        .and_then(non_empty_trimmed)
        .map(|model_class| (Some(model_class.to_string()), Some("task.model_class")))
        .unwrap_or((None, None))
}

fn fleet_route_model_selector_with_source(
    worker_profile: Option<&FleetTaskWorkerProfile>,
    agent_profile: Option<&AgentProfile>,
    session_model: Option<&str>,
) -> (Option<String>, &'static str) {
    // The session route (operator model) is the run-level fallback, matching
    // the dispatch path where FleetManager::run_model() feeds
    // `effective_fleet_model_with_source`. Empty/"auto" stays resolver-default.
    let run_model = session_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .unwrap_or("auto");
    let (model, source) =
        effective_fleet_model_with_source(run_model, worker_profile, agent_profile);
    if model.trim().is_empty() || model.eq_ignore_ascii_case("auto") {
        (None, "resolver.default")
    } else {
        (Some(model), source)
    }
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet_task(id: &str, worker: Option<FleetTaskWorkerProfile>) -> FleetTaskSpec {
        FleetTaskSpec {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            objective: Some(format!("Complete {id}")),
            instructions: format!("do {id}"),
            worker,
            workspace: None,
            input_files: Vec::new(),
            context: Vec::new(),
            budget: None,
            tags: Vec::new(),
            expected_artifacts: Vec::new(),
            scorer: None,
            retry_policy: None,
            alert_policy: None,
            timeout_seconds: None,
            metadata: Default::default(),
        }
    }

    fn worker_profile(
        agent_profile: Option<&str>,
        role: Option<&str>,
        loadout: Option<&str>,
        model_class: Option<&str>,
        model: Option<&str>,
    ) -> FleetTaskWorkerProfile {
        FleetTaskWorkerProfile {
            agent_profile: agent_profile.map(str::to_string),
            role: role.map(str::to_string),
            loadout: loadout.map(str::to_string),
            model_class: model_class.map(str::to_string),
            model: model.map(str::to_string),
            tool_profile: None,
            tools: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    fn agent_profile(
        id: &str,
        role: &str,
        instructions: Option<&str>,
        loadout: codewhale_config::FleetLoadout,
    ) -> AgentProfile {
        AgentProfile {
            id: id.to_string(),
            display_name: Some(format!("{role} profile")),
            description: Some(format!("{role} description")),
            profile: codewhale_config::FleetProfile {
                slot: codewhale_config::FleetSlot::from_name(role),
                role: codewhale_config::FleetRole {
                    name: role.to_string(),
                    description: Some(format!("{role} role")),
                    instructions: instructions.map(str::to_string),
                },
                loadout,
                model: None,
                provider: None,
                reasoning_effort: None,
                permissions: codewhale_config::FleetProfilePermissions::default(),
                delegation: codewhale_config::FleetDelegationHints::default(),
            },
            source: std::path::PathBuf::from(format!("{id}.toml")),
            origin: crate::fleet::roster::ProfileOrigin::Workspace,
        }
    }

    #[test]
    fn unknown_agent_profile_fails_before_worker_launch() {
        let task = fleet_task(
            "review",
            Some(worker_profile(Some("missing"), None, None, None, None)),
        );

        let error = validate_task_agent_profiles(&[task], &[])
            .expect_err("unknown profile must fail before worker launch");
        assert!(
            error
                .to_string()
                .contains("references unknown agent profile \"missing\"")
        );
    }

    #[test]
    fn task_prompt_preserves_task_and_profile_input() {
        let profile = agent_profile(
            "reviewer",
            "reviewer",
            Some("Focus on regressions."),
            codewhale_config::FleetLoadout::Inherit,
        );
        let mut task = fleet_task(
            "review",
            Some(worker_profile(Some("reviewer"), None, None, None, None)),
        );
        task.context.push("Keep the report concise.".to_string());
        task.input_files
            .push(std::path::PathBuf::from("crates/runtime/src/agent.rs"));

        let prompt = fleet_task_prompt_with_profiles(&task, &[profile]).unwrap();

        assert!(prompt.contains("Fleet member (reviewer)"));
        assert!(prompt.contains("Complete review"));
        assert!(prompt.contains("do review"));
        assert!(prompt.contains("Keep the report concise."));
        assert!(prompt.contains("crates/runtime/src/agent.rs"));
        assert!(prompt.contains("Focus on regressions."));
    }

    #[test]
    fn route_receipt_records_only_resolved_launch_facts() {
        let task = fleet_task(
            "route",
            Some(worker_profile(
                None,
                Some("builder"),
                Some("fast"),
                None,
                None,
            )),
        );

        let route = resolve_fleet_route(&task, &[], None).expect("offline route resolves");

        assert_eq!(route.role.as_deref(), Some("builder"));
        assert_eq!(route.loadout.as_deref(), Some("fast"));
        assert_eq!(route.model_route.as_deref(), Some("faster"));
        assert_eq!(route.model_source.as_deref(), Some("resolver.default"));
        assert_eq!(route.source, "resolver");
        let json = serde_json::to_string(&route).unwrap().to_ascii_lowercase();
        for marker in ["api_key", "authorization", "bearer ", "password", "secret"] {
            assert!(!json.contains(marker), "route leaked {marker}: {json}");
        }
    }

    #[test]
    fn explicit_model_is_reported_as_fixed_route() {
        let task = fleet_task(
            "fixed",
            Some(worker_profile(
                None,
                None,
                Some("fast"),
                None,
                Some("deepseek-v4-flash"),
            )),
        );

        let route = resolve_fleet_route(&task, &[], Some("deepseek-v4-pro"))
            .expect("explicit model resolves");

        assert_eq!(route.model_source.as_deref(), Some("task.model"));
        assert_eq!(route.model_route.as_deref(), Some("fixed"));
        assert_eq!(route.wire_model_id, "deepseek-v4-flash");
    }

    #[test]
    fn explicit_profile_route_and_reasoning_reach_launch() {
        let mut profile = agent_profile(
            "scout",
            "scout",
            None,
            codewhale_config::FleetLoadout::Inherit,
        );
        profile.profile.model = Some("glm-5.2".to_string());
        profile.profile.provider = Some("openrouter".to_string());
        profile.profile.reasoning_effort = Some("high".to_string());
        let task = fleet_task(
            "launch",
            Some(worker_profile(Some("scout"), None, None, None, None)),
        );
        let profiles = vec![profile];

        let (model, provider) = fleet_worker_launch_route(&task, &profiles, "deepseek-v4-pro");

        assert_eq!(model, "glm-5.2");
        assert_eq!(provider.as_deref(), Some("openrouter"));
        assert_eq!(
            fleet_worker_launch_reasoning_effort(&task, &profiles).as_deref(),
            Some("high")
        );
        let route = resolve_fleet_route(&task, &profiles, Some("deepseek-v4-pro"))
            .expect("explicit provider route resolves");
        assert_eq!(route.provider_id, "openrouter");
        assert_eq!(route.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn provider_is_never_inferred_from_model() {
        let mut profile = agent_profile(
            "model-only",
            "scout",
            None,
            codewhale_config::FleetLoadout::Inherit,
        );
        profile.profile.model = Some("deepseek-v4-flash".to_string());
        let task = fleet_task(
            "launch",
            Some(worker_profile(Some("model-only"), None, None, None, None)),
        );

        let (model, provider) = fleet_worker_launch_route(&task, &[profile], "deepseek-v4-pro");

        assert_eq!(model, "deepseek-v4-flash");
        assert_eq!(provider, None);
    }

    #[test]
    fn route_label_uses_existing_loadout_without_runtime_profile() {
        assert_eq!(
            fleet_model_route_label("auto", &codewhale_config::FleetLoadout::Inherit),
            "inherit"
        );
        assert_eq!(
            fleet_model_route_label("auto", &codewhale_config::FleetLoadout::Fast),
            "faster"
        );
        assert_eq!(
            fleet_model_route_label(
                "auto",
                &codewhale_config::FleetLoadout::Custom("strong".to_string())
            ),
            "auto"
        );
        assert_eq!(
            fleet_model_route_label("deepseek-v4-flash", &codewhale_config::FleetLoadout::Fast),
            "fixed"
        );
    }
}
