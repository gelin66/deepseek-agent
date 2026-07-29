//! Host-owned one-shot local HTTP application verification.
//!
//! `application_probe` is deliberately not part of the model-visible tool
//! catalog. `AgentApplication` resolves it into an exact `VerifierSpec`; the
//! existing Host-verifier path then executes it after a completion proposal.
//! Every spawned process is bound to a pre-persisted lease token so a later
//! reopen can recover an in-flight process tree without a PID sidecar.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use dse_protocol::agent_runtime::{
    ToolFailureCode, ToolOperationStatus, ToolRetryDisposition, ToolSideEffectStatus,
};
use dse_protocol::task::{VerifierPlan, VerifierSpec, VerifierStep, VerifierVerdict};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};

use crate::child_env;
use crate::sandbox::{CommandSpec, SandboxManager, SandboxPolicy};
use crate::shell::{
    ExecShellOptions, ProcessTreeOwner, configure_process_tree, shutdown_tokio_process_tree,
};
use crate::verification_artifact::{
    attach_verifier_observation, capture_workspace_revision, reject_verification_artifact,
};
use crate::{ProductionToolContext, ToolError, ToolOutcome};

pub(crate) const APPLICATION_PROBE_VERIFIER_ID: &str = "application_probe";

const LOOPBACK_HOST: &str = "127.0.0.1";
const PORT_PLACEHOLDER: &str = "{dse_loopback_port}";
const HOST_PLACEHOLDER: &str = "{dse_loopback_host}";
const LEASE_ENV: &str = "DSE_APPLICATION_PROBE_LEASE";
const LEASE_ARG0_PREFIX: &str = "dse-application-probe:";
const LEASE_PLACEHOLDER: &str = "{dse_probe_lease}";
const MAX_ARGS: usize = 64;
const MAX_ENV: usize = 32;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ENV_VALUE_BYTES: usize = 16 * 1024;
const MAX_PATH_BYTES: usize = 2 * 1024;
const MAX_LOG_BYTES_LIMIT: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES_LIMIT: usize = 1024 * 1024;
const MAX_BODY_EXCERPT_CHARS: usize = 4_000;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_STARTUP_TIMEOUT_MS: u64 = 30_000;
const MAX_HEALTH_TIMEOUT_MS: u64 = 60_000;
const MAX_OVERALL_TIMEOUT_MS: u64 = 120_000;
const HTTP_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(50);
const TEARDOWN_GRACE: Duration = Duration::from_millis(500);
const RECOVERY_SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

fn default_host_env() -> String {
    "HOST".to_owned()
}

fn default_port_env() -> String {
    "PORT".to_owned()
}

fn default_health_path() -> String {
    "/health".to_owned()
}

fn default_assertion_path() -> String {
    "/".to_owned()
}

const fn default_expected_status() -> u16 {
    200
}

const fn default_startup_timeout_ms() -> u64 {
    5_000
}

const fn default_health_timeout_ms() -> u64 {
    10_000
}

const fn default_overall_timeout_ms() -> u64 {
    30_000
}

const fn default_max_log_bytes() -> usize {
    64 * 1024
}

const fn default_max_response_bytes() -> usize {
    64 * 1024
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationProbeInput {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default = "default_host_env")]
    host_env: String,
    #[serde(default = "default_port_env")]
    port_env: String,
    #[serde(default = "default_health_path")]
    health_path: String,
    #[serde(default = "default_assertion_path")]
    assertion_path: String,
    #[serde(default = "default_expected_status")]
    expected_status: u16,
    body_contains: String,
    #[serde(default = "default_startup_timeout_ms")]
    startup_timeout_ms: u64,
    #[serde(default = "default_health_timeout_ms")]
    health_timeout_ms: u64,
    #[serde(default = "default_overall_timeout_ms")]
    overall_timeout_ms: u64,
    #[serde(default = "default_max_log_bytes")]
    max_log_bytes: usize,
    #[serde(default = "default_max_response_bytes")]
    max_response_bytes: usize,
    #[serde(
        default,
        rename = "_lease_token",
        skip_serializing_if = "Option::is_none"
    )]
    lease_token: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedProbe {
    input: ApplicationProbeInput,
    cwd: PathBuf,
    spec: VerifierSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamCapture {
    bytes_read: usize,
    bytes_returned: usize,
    truncated: bool,
    excerpt: String,
}

impl StreamCapture {
    fn empty() -> Self {
        Self {
            bytes_read: 0,
            bytes_returned: 0,
            truncated: false,
            excerpt: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HttpObservation {
    url: String,
    status: u16,
    bytes_read: usize,
    bytes_returned: usize,
    truncated: bool,
    body_sha256: String,
    body_excerpt: String,
}

struct FetchedResponse {
    observation: HttpObservation,
    body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TeardownObservation {
    attempted: bool,
    forced: bool,
    direct_child_reaped: bool,
    process_tree_settled: bool,
}

impl TeardownObservation {
    fn not_started() -> Self {
        Self {
            attempted: false,
            forced: false,
            direct_child_reaped: true,
            process_tree_settled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApplicationProbeOutput {
    success: bool,
    failure_code: Option<String>,
    summary: String,
    loopback_host: String,
    port: Option<u16>,
    health_ready: bool,
    assertion: Option<HttpObservation>,
    stdout: StreamCapture,
    stderr: StreamCapture,
    process_exit_code: Option<i32>,
    process_exited_early: bool,
    timed_out: bool,
    cancelled: bool,
    duration_ms: u64,
    teardown: TeardownObservation,
    trust: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationProbeRecovery {
    pub matched_processes: usize,
    pub killed_process_groups: usize,
}

/// Resolve caller parameters into the one exact Host-owned plan. A caller
/// cannot choose the durable lease identity; it is generated before
/// `RunCreated` and persisted as part of the canonical verifier parameters.
pub(crate) fn resolve_application_probe_spec(
    parameters: Value,
    context: &ProductionToolContext,
) -> Result<VerifierSpec, ToolError> {
    let mut input = parse_input(parameters)?;
    if input.lease_token.is_some() {
        return Err(ToolError::invalid_input(
            "application_probe _lease_token is Host-owned",
        ));
    }
    input.lease_token = Some(uuid::Uuid::new_v4().simple().to_string());
    resolve_with_token(input, context).map(|resolved| resolved.spec)
}

/// Re-resolve a persisted spec without creating a second lease token.
pub(crate) fn validate_application_probe_spec(
    spec: &VerifierSpec,
    context: &ProductionToolContext,
) -> Result<(), ToolError> {
    if spec.verifier_id != APPLICATION_PROBE_VERIFIER_ID {
        return Err(ToolError::invalid_input(
            "not an application_probe verifier specification",
        ));
    }
    let input = parse_input(spec.parameters.clone())?;
    validate_lease_token(input.lease_token.as_deref())?;
    let resolved = resolve_with_token(input, context)?;
    if resolved.spec != *spec {
        return Err(ToolError::invalid_input(
            "persisted application_probe plan differs from the production resolver",
        ));
    }
    Ok(())
}

fn parse_input(parameters: Value) -> Result<ApplicationProbeInput, ToolError> {
    serde_json::from_value(parameters).map_err(|error| ToolError::invalid_input(error.to_string()))
}

fn resolve_with_token(
    input: ApplicationProbeInput,
    context: &ProductionToolContext,
) -> Result<ResolvedProbe, ToolError> {
    validate_input(&input)?;
    let lease_token = input
        .lease_token
        .as_deref()
        .expect("validated application probe has a lease token");
    let cwd = if input.cwd.is_empty() {
        context.workspace().to_path_buf()
    } else {
        context.resolve_path(&input.cwd)?
    };
    if !cwd.is_dir() || cwd.strip_prefix(context.workspace()).is_err() {
        return Err(ToolError::invalid_input(
            "application_probe cwd must be an existing directory inside the workspace",
        ));
    }
    let relative_cwd = cwd.strip_prefix(context.workspace()).map_err(|_| {
        ToolError::invalid_input("application_probe cwd must stay inside the workspace")
    })?;
    let mut env = input.env.clone();
    env.insert(input.host_env.clone(), HOST_PLACEHOLDER.to_owned());
    env.insert(input.port_env.clone(), PORT_PLACEHOLDER.to_owned());
    env.insert(LEASE_ENV.to_owned(), lease_token.to_owned());
    let parameters = serde_json::to_value(&input)
        .map_err(|error| ToolError::invalid_input(error.to_string()))?;
    let spec = VerifierSpec {
        verifier_id: APPLICATION_PROBE_VERIFIER_ID.to_owned(),
        parameters,
        plan: VerifierPlan {
            steps: vec![VerifierStep {
                id: APPLICATION_PROBE_VERIFIER_ID.to_owned(),
                program: input.program.clone(),
                args: input.args.clone(),
                cwd: relative_cwd.to_string_lossy().to_string(),
                env,
                timeout_ms: input.overall_timeout_ms,
            }],
        },
    }
    .canonicalized();
    spec.validate().map_err(ToolError::invalid_input)?;
    Ok(ResolvedProbe { input, cwd, spec })
}

fn validate_input(input: &ApplicationProbeInput) -> Result<(), ToolError> {
    if input.program.trim().is_empty()
        || input.program.len() > MAX_ARG_BYTES
        || input.program.contains('\0')
        || input.program.contains(PORT_PLACEHOLDER)
        || input.program.contains(HOST_PLACEHOLDER)
        || input.program.contains(LEASE_PLACEHOLDER)
    {
        return Err(ToolError::invalid_input(
            "application_probe program must be one bounded exact executable name",
        ));
    }
    if input.args.len() > MAX_ARGS {
        return Err(ToolError::invalid_input(format!(
            "application_probe args may contain at most {MAX_ARGS} values"
        )));
    }
    for argument in &input.args {
        if argument.len() > MAX_ARG_BYTES || argument.contains('\0') {
            return Err(ToolError::invalid_input(
                "application_probe argv value exceeds its byte limit or contains NUL",
            ));
        }
        let stripped = argument
            .replace(PORT_PLACEHOLDER, "")
            .replace(HOST_PLACEHOLDER, "")
            .replace(LEASE_PLACEHOLDER, "");
        if stripped.contains("{dse_") {
            return Err(ToolError::invalid_input(
                "application_probe argv contains an unsupported Host placeholder",
            ));
        }
    }
    if input
        .args
        .iter()
        .filter(|argument| argument.as_str() == LEASE_PLACEHOLDER)
        .count()
        != 1
    {
        return Err(ToolError::invalid_input(
            "application_probe args must contain the exact Host lease placeholder once",
        ));
    }
    if input.env.len() > MAX_ENV {
        return Err(ToolError::invalid_input(format!(
            "application_probe env may contain at most {MAX_ENV} values"
        )));
    }
    validate_env_name(&input.host_env)?;
    validate_env_name(&input.port_env)?;
    if input.host_env == input.port_env {
        return Err(ToolError::invalid_input(
            "application_probe host_env and port_env must differ",
        ));
    }
    for (key, value) in &input.env {
        validate_env_name(key)?;
        if key == LEASE_ENV || key == &input.host_env || key == &input.port_env {
            return Err(ToolError::invalid_input(format!(
                "application_probe env key '{key}' is Host-owned"
            )));
        }
        if value.len() > MAX_ENV_VALUE_BYTES || value.contains('\0') {
            return Err(ToolError::invalid_input(format!(
                "application_probe env value '{key}' exceeds its byte limit or contains NUL"
            )));
        }
    }
    validate_http_path("health_path", &input.health_path)?;
    validate_http_path("assertion_path", &input.assertion_path)?;
    if !(100..=599).contains(&input.expected_status) {
        return Err(ToolError::invalid_input(
            "application_probe expected_status must be between 100 and 599",
        ));
    }
    if input.body_contains.is_empty()
        || input.body_contains.len() > MAX_RESPONSE_BYTES_LIMIT
        || input.body_contains.contains('\0')
    {
        return Err(ToolError::invalid_input(
            "application_probe body_contains must be non-empty and bounded",
        ));
    }
    validate_timeout(
        "startup_timeout_ms",
        input.startup_timeout_ms,
        MAX_STARTUP_TIMEOUT_MS,
    )?;
    validate_timeout(
        "health_timeout_ms",
        input.health_timeout_ms,
        MAX_HEALTH_TIMEOUT_MS,
    )?;
    validate_timeout(
        "overall_timeout_ms",
        input.overall_timeout_ms,
        MAX_OVERALL_TIMEOUT_MS,
    )?;
    if input.startup_timeout_ms > input.health_timeout_ms
        || input.health_timeout_ms > input.overall_timeout_ms
    {
        return Err(ToolError::invalid_input(
            "application_probe timeouts must satisfy startup <= health <= overall",
        ));
    }
    if !(1..=MAX_LOG_BYTES_LIMIT).contains(&input.max_log_bytes) {
        return Err(ToolError::invalid_input(format!(
            "application_probe max_log_bytes must be between 1 and {MAX_LOG_BYTES_LIMIT}"
        )));
    }
    if !(1..=MAX_RESPONSE_BYTES_LIMIT).contains(&input.max_response_bytes) {
        return Err(ToolError::invalid_input(format!(
            "application_probe max_response_bytes must be between 1 and {MAX_RESPONSE_BYTES_LIMIT}"
        )));
    }
    if input.body_contains.len() > input.max_response_bytes {
        return Err(ToolError::invalid_input(
            "application_probe body_contains cannot exceed max_response_bytes",
        ));
    }
    validate_lease_token(input.lease_token.as_deref())
}

fn validate_lease_token(token: Option<&str>) -> Result<(), ToolError> {
    let Some(token) = token else {
        return Err(ToolError::invalid_input(
            "application_probe is missing its Host-owned lease token",
        ));
    };
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ToolError::invalid_input(
            "application_probe lease token is invalid",
        ));
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), ToolError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with("DSE_")
        || !name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
        })
    {
        return Err(ToolError::invalid_input(format!(
            "application_probe environment name '{name}' is invalid or reserved"
        )));
    }
    Ok(())
}

fn validate_http_path(label: &str, path: &str) -> Result<(), ToolError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['\r', '\n', '\0', '\\', '#'])
        || path.contains("://")
    {
        return Err(ToolError::invalid_input(format!(
            "application_probe {label} must be one bounded loopback origin-form path"
        )));
    }
    Ok(())
}

fn validate_timeout(label: &str, value: u64, maximum: u64) -> Result<(), ToolError> {
    if !(MIN_TIMEOUT_MS..=maximum).contains(&value) {
        return Err(ToolError::invalid_input(format!(
            "application_probe {label} must be between {MIN_TIMEOUT_MS} and {maximum}"
        )));
    }
    Ok(())
}

fn replace_host_placeholders(value: &str, port: u16, lease_token: &str) -> String {
    value
        .replace(HOST_PLACEHOLDER, LOOPBACK_HOST)
        .replace(PORT_PLACEHOLDER, &port.to_string())
        .replace(
            LEASE_PLACEHOLDER,
            &format!("{LEASE_ARG0_PREFIX}{lease_token}"),
        )
}

/// Execute the exact probe as a foreground Host verifier. The process is
/// always torn down before this future resolves.
pub(crate) async fn execute_application_probe(
    parameters: Value,
    context: &ProductionToolContext,
    shell: &ExecShellOptions,
) -> Result<ToolOutcome, ToolError> {
    let input = parse_input(parameters)?;
    let resolved = resolve_with_token(input, context)?;
    let revision_before = capture_workspace_revision(context.workspace()).await;
    let started_at = Instant::now();
    let mut output = ApplicationProbeOutput {
        success: false,
        failure_code: None,
        summary: String::new(),
        loopback_host: LOOPBACK_HOST.to_owned(),
        port: None,
        health_ready: false,
        assertion: None,
        stdout: StreamCapture::empty(),
        stderr: StreamCapture::empty(),
        process_exit_code: None,
        process_exited_early: false,
        timed_out: false,
        cancelled: false,
        duration_ms: 0,
        teardown: TeardownObservation::not_started(),
        trust: "external_untrusted".to_owned(),
    };

    let probe_sandbox_policy = match shell.elevated_sandbox_policy.as_ref() {
        Some(policy) => match policy.for_host_loopback_probe() {
            Some(policy) => Some(policy),
            None => {
                output.failure_code = Some("application_probe_network_denied".to_owned());
                output.summary = "actor sandbox denies the loopback application probe".to_owned();
                output.duration_ms = elapsed_millis(started_at);
                return finalize_probe_outcome(output, resolved.spec, revision_before, context)
                    .await;
            }
        },
        None => None,
    };

    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .map_err(|error| {
            ToolError::execution_failed(format!("cannot reserve loopback port: {error}"))
        })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            ToolError::execution_failed(format!("cannot inspect loopback port: {error}"))
        })?
        .port();
    output.port = Some(port);
    drop(listener);

    let mut env = resolved.input.env.clone();
    env.insert(resolved.input.host_env.clone(), LOOPBACK_HOST.to_owned());
    env.insert(resolved.input.port_env.clone(), port.to_string());
    env.insert(
        LEASE_ENV.to_owned(),
        resolved
            .input
            .lease_token
            .clone()
            .expect("resolved probe has a lease token"),
    );
    let lease_token = resolved
        .input
        .lease_token
        .as_deref()
        .expect("resolved probe has a lease token");
    let args = resolved
        .input
        .args
        .iter()
        .map(|argument| replace_host_placeholders(argument, port, lease_token))
        .collect::<Vec<_>>();

    let spawn_result = spawn_probe_process(
        &resolved.input.program,
        &args,
        &resolved.cwd,
        env,
        resolved.input.overall_timeout_ms,
        probe_sandbox_policy,
        resolved.input.max_log_bytes,
    );
    let (mut child, mut owner, stdout_task, stderr_task) = match spawn_result {
        Ok(spawned) => spawned,
        Err(error) => {
            output.failure_code = Some("application_probe_spawn_failed".to_owned());
            output.summary = error.to_string();
            output.duration_ms = elapsed_millis(started_at);
            return finalize_probe_outcome(output, resolved.spec, revision_before, context).await;
        }
    };

    let workflow = run_probe_workflow(&resolved.input, port, context, &mut child, started_at).await;
    match workflow {
        Ok(assertion) => {
            output.success = true;
            output.health_ready = true;
            output.assertion = Some(assertion);
            output.summary = "application probe HTTP assertion passed".to_owned();
        }
        Err(failure) => {
            output.failure_code = Some(failure.code.to_owned());
            output.summary = failure.message;
            output.health_ready = failure.health_ready;
            output.assertion = failure.assertion;
            output.process_exited_early = failure.process_exited_early;
            output.timed_out = failure.timed_out;
            output.cancelled = failure.cancelled;
        }
    }

    let exited_before_teardown = child.try_wait().ok().flatten();
    output.process_exit_code = exited_before_teardown
        .as_ref()
        .and_then(std::process::ExitStatus::code);
    let teardown = shutdown_probe_process(&mut child, &mut owner).await;
    if output.process_exit_code.is_none() {
        output.process_exit_code = child
            .try_wait()
            .ok()
            .flatten()
            .and_then(|status| status.code());
    }
    output.teardown = teardown;
    output.stdout = join_capture(stdout_task).await;
    output.stderr = join_capture(stderr_task).await;
    output.duration_ms = elapsed_millis(started_at);

    if !output.teardown.process_tree_settled || !output.teardown.direct_child_reaped {
        output.success = false;
        output.failure_code = Some("application_probe_teardown_failed".to_owned());
        output.summary = "application probe could not prove process-tree teardown".to_owned();
    }
    finalize_probe_outcome(output, resolved.spec, revision_before, context).await
}

type CaptureTask = tokio::task::JoinHandle<StreamCapture>;

fn spawn_probe_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: BTreeMap<String, String>,
    timeout_ms: u64,
    sandbox_policy: Option<SandboxPolicy>,
    max_log_bytes: usize,
) -> Result<(Child, ProcessTreeOwner, CaptureTask, CaptureTask), ToolError> {
    let policy = sandbox_policy.unwrap_or(SandboxPolicy::DangerFullAccess);
    let spec = CommandSpec::program(
        program,
        args.to_vec(),
        cwd.to_path_buf(),
        Duration::from_millis(timeout_ms),
    )
    .with_policy(policy)
    .with_env(env.into_iter().collect::<HashMap<_, _>>());
    let prepared = SandboxManager::new()
        .prepare_enforced(&spec)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;
    let mut command = Command::new(prepared.program());
    command
        .args(prepared.args())
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    child_env::apply_to_tokio_command(&mut command, child_env::string_map_env(&prepared.env));
    configure_process_tree(command.as_std_mut());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let lease = prepared
            .env
            .get(LEASE_ENV)
            .expect("application probe prepared environment has its durable lease");
        command
            .as_std_mut()
            .arg0(format!("{LEASE_ARG0_PREFIX}{lease}"));
    }
    let mut child = command.spawn().map_err(|error| {
        ToolError::execution_failed(format!("cannot spawn application: {error}"))
    })?;
    let owner =
        ProcessTreeOwner::attach_tokio(&child, APPLICATION_PROBE_VERIFIER_ID).map_err(|error| {
            let _ = child.start_kill();
            ToolError::execution_failed(format!("cannot own application process tree: {error}"))
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::execution_failed("application stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::execution_failed("application stderr was not captured"))?;
    let stdout_task = tokio::spawn(capture_stream(stdout, max_log_bytes));
    let stderr_task = tokio::spawn(capture_stream(stderr, max_log_bytes));
    Ok((child, owner, stdout_task, stderr_task))
}

#[derive(Debug)]
struct ProbeFailure {
    code: &'static str,
    message: String,
    health_ready: bool,
    assertion: Option<HttpObservation>,
    process_exited_early: bool,
    timed_out: bool,
    cancelled: bool,
}

impl ProbeFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            health_ready: false,
            assertion: None,
            process_exited_early: false,
            timed_out: false,
            cancelled: false,
        }
    }
}

async fn run_probe_workflow(
    input: &ApplicationProbeInput,
    port: u16,
    context: &ProductionToolContext,
    child: &mut Child,
    started_at: Instant,
) -> Result<HttpObservation, ProbeFailure> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_ATTEMPT_TIMEOUT)
        .build()
        .map_err(|error| ProbeFailure::new("application_probe_http_client", error.to_string()))?;
    let health_url = loopback_url(port, &input.health_path);
    let assertion_url = loopback_url(port, &input.assertion_path);
    let startup_deadline = started_at + Duration::from_millis(input.startup_timeout_ms);
    let health_deadline = started_at + Duration::from_millis(input.health_timeout_ms);
    let overall_deadline = started_at + Duration::from_millis(input.overall_timeout_ms);

    // A short, explicit startup phase catches immediate exec/bind failures
    // before an unrelated listener could be mistaken for readiness.
    while Instant::now() < startup_deadline.min(health_deadline) {
        if cancellation_requested(context) {
            let mut failure = ProbeFailure::new(
                "application_probe_cancelled",
                "application probe cancelled during startup",
            );
            failure.cancelled = true;
            return Err(failure);
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            ProbeFailure::new("application_probe_process_wait", error.to_string())
        })? {
            let mut failure = ProbeFailure::new(
                "application_probe_early_exit",
                format!("application exited before readiness with {status}"),
            );
            failure.process_exited_early = true;
            return Err(failure);
        }
        let deadline = startup_deadline.min(health_deadline).min(overall_deadline);
        match fetch_bounded_before(
            &client,
            &health_url,
            input.max_response_bytes,
            deadline,
            "application_probe_startup_timeout",
            "application probe startup HTTP attempt exceeded its deadline",
        )
        .await
        {
            Ok(response) if (200..=299).contains(&response.observation.status) => break,
            Ok(_) | Err(_) => poll_pause(deadline).await,
        }
    }

    let mut ready = false;
    while Instant::now() < health_deadline.min(overall_deadline) {
        if cancellation_requested(context) {
            let mut failure = ProbeFailure::new(
                "application_probe_cancelled",
                "application probe cancelled while waiting for health",
            );
            failure.cancelled = true;
            return Err(failure);
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            ProbeFailure::new("application_probe_process_wait", error.to_string())
        })? {
            let mut failure = ProbeFailure::new(
                "application_probe_early_exit",
                format!("application exited before readiness with {status}"),
            );
            failure.process_exited_early = true;
            return Err(failure);
        }
        let deadline = health_deadline.min(overall_deadline);
        match fetch_bounded_before(
            &client,
            &health_url,
            input.max_response_bytes,
            deadline,
            "application_probe_health_timeout",
            "application health request exceeded its bounded deadline",
        )
        .await
        {
            Ok(response) if (200..=299).contains(&response.observation.status) => {
                ready = true;
                break;
            }
            Ok(_) | Err(_) => poll_pause(deadline).await,
        }
    }
    if !ready {
        let mut failure = ProbeFailure::new(
            "application_probe_health_timeout",
            "application did not become healthy before the bounded deadline",
        );
        failure.timed_out = true;
        return Err(failure);
    }
    if Instant::now() >= overall_deadline {
        let mut failure = ProbeFailure::new(
            "application_probe_overall_timeout",
            "application probe exhausted its overall deadline before assertion",
        );
        failure.health_ready = true;
        failure.timed_out = true;
        return Err(failure);
    }
    let assertion = fetch_bounded_before(
        &client,
        &assertion_url,
        input.max_response_bytes,
        overall_deadline,
        "application_probe_overall_timeout",
        "application assertion exceeded the overall deadline",
    )
    .await
    .map_err(|mut failure| {
        failure.health_ready = true;
        failure.timed_out = matches!(
            failure.code,
            "application_probe_overall_timeout" | "application_probe_http_timeout"
        );
        failure
    })?;
    if assertion.observation.status != input.expected_status {
        let mut failure = ProbeFailure::new(
            "application_probe_status_mismatch",
            format!(
                "application returned HTTP {}, expected {}",
                assertion.observation.status, input.expected_status
            ),
        );
        failure.health_ready = true;
        failure.assertion = Some(assertion.observation);
        return Err(failure);
    }
    let lease_marker = format!(
        "{LEASE_ARG0_PREFIX}{}",
        input
            .lease_token
            .as_deref()
            .expect("validated probe has its Host lease token")
    );
    if !assertion.body.contains(&lease_marker) {
        let mut failure = ProbeFailure::new(
            "application_probe_identity_mismatch",
            "application response did not echo the Host-owned process lease identity",
        );
        failure.health_ready = true;
        failure.assertion = Some(assertion.observation);
        return Err(failure);
    }
    if !assertion.body.contains(&input.body_contains) {
        let mut failure = ProbeFailure::new(
            "application_probe_body_mismatch",
            "application response did not contain the pre-registered bounded assertion",
        );
        failure.health_ready = true;
        failure.assertion = Some(assertion.observation);
        return Err(failure);
    }
    if let Some(status) = child
        .try_wait()
        .map_err(|error| ProbeFailure::new("application_probe_process_wait", error.to_string()))?
    {
        let mut failure = ProbeFailure::new(
            "application_probe_early_exit",
            format!("application exited during HTTP assertion with {status}"),
        );
        failure.health_ready = true;
        failure.assertion = Some(assertion.observation);
        failure.process_exited_early = true;
        return Err(failure);
    }
    Ok(assertion.observation)
}

fn cancellation_requested(context: &ProductionToolContext) -> bool {
    context
        .cancellation_token()
        .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
}

fn loopback_url(port: u16, path: &str) -> String {
    format!("http://{LOOPBACK_HOST}:{port}{path}")
}

async fn fetch_bounded(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
) -> Result<FetchedResponse, ProbeFailure> {
    let response = client.get(url).send().await.map_err(|error| {
        if error.is_timeout() {
            ProbeFailure::new(
                "application_probe_http_timeout",
                "application HTTP attempt exceeded its bounded timeout",
            )
        } else {
            ProbeFailure::new("application_probe_http_failed", error.to_string())
        }
    })?;
    let status = response.status().as_u16();
    if response.status().is_redirection() {
        return Err(ProbeFailure::new(
            "application_probe_redirect_denied",
            format!("application returned redirect status {status}; redirects are disabled"),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ProbeFailure::new(
            "application_probe_response_too_large",
            format!("application response exceeds the {max_bytes}-byte limit"),
        ));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::with_capacity(max_bytes.min(16 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            ProbeFailure::new("application_probe_http_failed", error.to_string())
        })?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ProbeFailure::new(
                "application_probe_response_too_large",
                format!("application response exceeds the {max_bytes}-byte limit"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body_sha256 = format_sha256(&bytes);
    let full = String::from_utf8_lossy(&bytes);
    let (body_excerpt, truncated) = truncate_chars(&full, MAX_BODY_EXCERPT_CHARS);
    Ok(FetchedResponse {
        observation: HttpObservation {
            url: url.to_owned(),
            status,
            bytes_read: bytes.len(),
            bytes_returned: body_excerpt.len(),
            truncated,
            body_sha256,
            body_excerpt,
        },
        body: full.into_owned(),
    })
}

async fn fetch_bounded_before(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    deadline: Instant,
    timeout_code: &'static str,
    timeout_message: &'static str,
) -> Result<FetchedResponse, ProbeFailure> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ProbeFailure::new(timeout_code, timeout_message));
    }
    match tokio::time::timeout(
        remaining.min(HTTP_ATTEMPT_TIMEOUT),
        fetch_bounded(client, url, max_bytes),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(ProbeFailure::new(timeout_code, timeout_message)),
    }
}

async fn poll_pause(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        tokio::time::sleep(remaining.min(HEALTH_POLL_INTERVAL)).await;
    }
}

async fn capture_stream(mut stream: impl AsyncRead + Unpin, limit: usize) -> StreamCapture {
    let mut kept = Vec::with_capacity(limit.min(16 * 1024));
    let mut bytes_read = 0_usize;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                bytes_read = bytes_read.saturating_add(read);
                let remaining = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    let lossy = String::from_utf8_lossy(&kept);
    let excerpt = truncate_utf8_bytes(&lossy, limit).to_owned();
    let replacement_expansion_truncated = excerpt.len() < lossy.len();
    StreamCapture {
        bytes_read,
        bytes_returned: excerpt.len(),
        truncated: bytes_read > kept.len() || replacement_expansion_truncated,
        excerpt,
    }
}

async fn join_capture(task: CaptureTask) -> StreamCapture {
    task.await.unwrap_or_else(|_| StreamCapture::empty())
}

async fn shutdown_probe_process(
    child: &mut Child,
    owner: &mut ProcessTreeOwner,
) -> TeardownObservation {
    let direct_was_running = child.id().is_some();
    let settled = shutdown_tokio_process_tree(child, owner, TEARDOWN_GRACE).await;
    TeardownObservation {
        attempted: direct_was_running,
        forced: direct_was_running,
        direct_child_reaped: child.id().is_none(),
        process_tree_settled: settled,
    }
}

async fn finalize_probe_outcome(
    output: ApplicationProbeOutput,
    verifier: VerifierSpec,
    revision_before: Result<String, String>,
    context: &ProductionToolContext,
) -> Result<ToolOutcome, ToolError> {
    let revision_after = capture_workspace_revision(context.workspace()).await;
    let summary = output.summary.clone();
    let success = output.success;
    let cleanup_proven =
        output.teardown.process_tree_settled && output.teardown.direct_child_reaped;
    let mut outcome = ToolOutcome::json(&output)
        .map_err(|error| ToolError::execution_failed(error.to_string()))?;
    outcome.side_effect = if cleanup_proven {
        ToolSideEffectStatus::NotApplied
    } else {
        ToolSideEffectStatus::Indeterminate
    };
    outcome.metadata = Some(json!({
        "application_probe_failure": output.failure_code,
        "loopback_only": true,
        "trust": "external_untrusted",
        "teardown": output.teardown,
    }));
    if success && cleanup_proven {
        attach_verifier_observation(
            &mut outcome,
            verifier,
            VerifierVerdict::Passed,
            summary,
            revision_before,
            revision_after,
        );
    } else if cleanup_proven {
        outcome.failure_code = Some(ToolFailureCode::VerifierFailed);
        outcome.operation = if output.cancelled {
            ToolOperationStatus::Cancelled
        } else {
            ToolOperationStatus::Failed
        };
        outcome.retry = ToolRetryDisposition::Safe;
        attach_verifier_observation(
            &mut outcome,
            verifier,
            VerifierVerdict::Failed,
            summary,
            revision_before,
            revision_after,
        );
    } else {
        outcome.failure_code = Some(ToolFailureCode::SideEffectAmbiguous);
        outcome.operation = ToolOperationStatus::Indeterminate;
        outcome.retry = ToolRetryDisposition::Unsafe;
        reject_verification_artifact(&mut outcome);
    }
    Ok(outcome)
}

fn elapsed_millis(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn format_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{digest}")
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut indices = text.char_indices();
    let Some((index, _)) = indices.nth(max_chars) else {
        return (text.to_owned(), false);
    };
    (text[..index].to_owned(), true)
}

fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Kill every process group whose argv carries the exact lease marker already
/// persisted in `HostVerificationPrepared`. The scan never falls back to PID,
/// port, executable-name, environment heuristics, or fuzzy command matching.
pub(crate) async fn recover_application_probe(
    spec: &VerifierSpec,
    context: &ProductionToolContext,
) -> Result<ApplicationProbeRecovery, ToolError> {
    validate_application_probe_spec(spec, context)?;
    let input = parse_input(spec.parameters.clone())?;
    let token = input
        .lease_token
        .as_deref()
        .expect("validated persisted probe has a lease token");
    let token = token.to_owned();
    tokio::task::spawn_blocking(move || recover_process_groups(&token))
        .await
        .map_err(|_| ToolError::execution_failed("application probe recovery task failed"))?
        .map_err(ToolError::execution_failed)
}

#[cfg(unix)]
fn recover_process_groups(token: &str) -> Result<ApplicationProbeRecovery, String> {
    let marker = format!("{LEASE_ARG0_PREFIX}{token}");
    let pids = processes_with_identity_marker(&marker)?;
    let current_group = unsafe { libc::getpgrp() };
    let mut groups = BTreeSet::new();
    for pid in &pids {
        let group = unsafe { libc::getpgid(*pid) };
        if group > 1 && group != current_group {
            groups.insert(group);
        }
    }
    for group in &groups {
        let result = unsafe { libc::kill(-*group, libc::SIGKILL) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(format!(
                    "cannot kill application probe process group {group}: {error}"
                ));
            }
        }
    }
    let deadline = Instant::now() + RECOVERY_SETTLE_TIMEOUT;
    loop {
        let live = processes_with_identity_marker(&marker)?;
        if live.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{} application probe process(es) retained the durable lease after SIGKILL",
                live.len()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(ApplicationProbeRecovery {
        matched_processes: pids.len(),
        killed_process_groups: groups.len(),
    })
}

#[cfg(target_os = "linux")]
fn processes_with_identity_marker(marker: &str) -> Result<Vec<libc::pid_t>, String> {
    let mut matches = Vec::new();
    let entries =
        std::fs::read_dir("/proc").map_err(|error| format!("cannot scan /proc: {error}"))?;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        let Ok(bytes) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if bytes
            .split(|byte| *byte == 0)
            .any(|field| field == marker.as_bytes())
        {
            matches.push(pid);
        }
    }
    Ok(matches)
}

#[cfg(target_os = "macos")]
fn processes_with_identity_marker(marker: &str) -> Result<Vec<libc::pid_t>, String> {
    let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(format!(
            "cannot size macOS process table: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut processes = vec![0 as libc::pid_t; count as usize + 32];
    let bytes = i32::try_from(processes.len() * std::mem::size_of::<libc::pid_t>())
        .map_err(|_| "macOS process table is too large".to_owned())?;
    let actual = unsafe { proc_listallpids(processes.as_mut_ptr().cast(), bytes) };
    if actual < 0 {
        return Err(format!(
            "cannot read macOS process table: {}",
            std::io::Error::last_os_error()
        ));
    }
    processes.truncate(actual as usize);
    let mut matches = Vec::new();
    for pid in processes {
        if pid > 1 && macos_process_contains_marker(pid, marker.as_bytes()) {
            matches.push(pid);
        }
    }
    Ok(matches)
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_listallpids(buffer: *mut std::ffi::c_void, buffersize: libc::c_int) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn macos_process_contains_marker(pid: libc::pid_t, marker: &[u8]) -> bool {
    let mut size = unsafe { libc::sysconf(libc::_SC_ARG_MAX) };
    if size <= 0 {
        size = 1024 * 1024;
    }
    let mut buffer = vec![0_u8; size as usize];
    let mut actual = buffer.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buffer.as_mut_ptr().cast(),
            &mut actual,
            std::ptr::null_mut(),
            0,
        )
    };
    result == 0
        && buffer[..actual]
            .split(|byte| *byte == 0)
            .any(|field| field == marker)
}

#[cfg(windows)]
fn recover_process_groups(_token: &str) -> Result<ApplicationProbeRecovery, String> {
    // Every probe process is attached to a kill-on-close Job Object before
    // the Host can proceed to HTTP. Closing the Host process closes the job
    // handle and Windows terminates the entire tree.
    Ok(ApplicationProbeRecovery {
        matched_processes: 0,
        killed_process_groups: 0,
    })
}

#[cfg(not(any(unix, windows)))]
fn recover_process_groups(_token: &str) -> Result<ApplicationProbeRecovery, String> {
    Err("application probe crash recovery is unsupported on this platform".to_owned())
}

#[cfg(test)]
mod tests {
    use std::process::Command as StdCommand;
    use std::sync::OnceLock;

    use dse_protocol::agent_runtime::ToolEvidenceStatus;

    use super::*;
    use crate::shell::{ShellPolicy, new_shared_shell_manager};

    fn context(workspace: &Path) -> ProductionToolContext {
        ProductionToolContext::new(workspace)
    }

    fn probe_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn valid_parameters() -> Value {
        json!({
            "program": "python3",
            "args": ["-B", "server.py", LEASE_PLACEHOLDER],
            "body_contains": "ready"
        })
    }

    fn git_workspace() -> tempfile::TempDir {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("tracked.txt"), "stable\n").unwrap();
        std::fs::write(
            workspace.path().join("server.py"),
            r#"import http.server
import os
import sys
import time

mode = os.environ["PROBE_FIXTURE_MODE"]
if mode == "early_exit":
    sys.exit(23)
if mode == "slow":
    time.sleep(30)
    sys.exit(0)
if mode == "logs":
    print("L" * (16 * 1024), file=sys.stderr, flush=True)
if mode == "invalid_logs":
    sys.stderr.buffer.write(b"\xff" * (16 * 1024))
    sys.stderr.buffer.flush()

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            status, body, location = 200, b"healthy", None
        elif mode == "status":
            status, body, location = 418, b"ready", None
        elif mode == "body":
            status, body, location = 200, b"wrong " + sys.argv[-1].encode("utf-8"), None
        elif mode == "foreign":
            status, body, location = 200, b"ready", None
        elif mode == "redirect":
            status, body, location = 302, b"", "/other"
        elif mode == "large":
            status, body, location = 200, b"X" * (8 * 1024), None
        elif mode == "slow_assertion":
            time.sleep(10)
            status, body, location = 200, b"ready " + sys.argv[-1].encode("utf-8"), None
        else:
            if mode == "mutate":
                with open("tracked.txt", "w", encoding="utf-8") as changed:
                    changed.write("changed\n")
            lease = sys.argv[-1].encode("utf-8")
            status, body, location = 200, b"ready " + lease, None
        self.send_response(status)
        if location is not None:
            self.send_header("Location", location)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        pass

server = http.server.ThreadingHTTPServer((os.environ["HOST"], int(os.environ["PORT"])), Handler)
server.serve_forever()
"#,
        )
        .unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "tracked.txt", "server.py"],
            vec![
                "-c",
                "user.name=DSE Test",
                "-c",
                "user.email=test.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        ] {
            assert!(
                StdCommand::new("git")
                    .args(args)
                    .current_dir(workspace.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        workspace
    }

    fn shell(workspace: &Path) -> ExecShellOptions {
        let mut shell = ExecShellOptions::new(
            new_shared_shell_manager(workspace.to_path_buf()),
            ShellPolicy::Full,
        );
        shell.elevated_sandbox_policy = Some(SandboxPolicy::DangerFullAccess);
        shell
    }

    fn fixture_parameters(mode: &str) -> Value {
        json!({
            "program": "/usr/bin/python3",
            "args": ["-I", "-B", "server.py", LEASE_PLACEHOLDER],
            "env": {"PROBE_FIXTURE_MODE": mode},
            "body_contains": "ready",
            "startup_timeout_ms": 1000,
            "health_timeout_ms": 2500,
            "overall_timeout_ms": 5000,
            "max_log_bytes": 1024,
            "max_response_bytes": 4096
        })
    }

    async fn execute_fixture(
        workspace: &Path,
        mode: &str,
    ) -> (ToolOutcome, ApplicationProbeOutput) {
        let _guard = probe_test_lock().lock().await;
        let ctx = context(workspace);
        let spec = resolve_application_probe_spec(fixture_parameters(mode), &ctx).unwrap();
        let outcome = execute_application_probe(spec.parameters, &ctx, &shell(workspace))
            .await
            .unwrap();
        let output = serde_json::from_str(&outcome.content).unwrap();
        (outcome, output)
    }

    #[test]
    fn resolver_freezes_host_owned_lease_and_exact_plan() {
        let workspace = tempfile::tempdir().unwrap();
        let spec =
            resolve_application_probe_spec(valid_parameters(), &context(workspace.path())).unwrap();
        assert_eq!(spec.verifier_id, APPLICATION_PROBE_VERIFIER_ID);
        assert_eq!(spec.plan.steps.len(), 1);
        let token = spec.parameters["_lease_token"].as_str().unwrap();
        assert_eq!(token.len(), 32);
        assert_eq!(spec.plan.steps[0].env[LEASE_ENV], token);
        assert_eq!(spec.plan.steps[0].env["HOST"], HOST_PLACEHOLDER);
        assert_eq!(spec.plan.steps[0].env["PORT"], PORT_PLACEHOLDER);
        validate_application_probe_spec(&spec, &context(workspace.path())).unwrap();
    }

    #[test]
    fn resolver_rejects_escape_url_port_and_host_owned_fields() {
        let workspace = tempfile::tempdir().unwrap();
        for parameters in [
            json!({"program":"python3","cwd":"../outside","body_contains":"ok"}),
            json!({"program":"python3","health_path":"http://example.test/","body_contains":"ok"}),
            json!({"program":"python3","port":8080,"body_contains":"ok"}),
            json!({"program":"python3","env":{"PORT":"8080"},"body_contains":"ok"}),
            json!({"program":"python3","_lease_token":"00000000000000000000000000000000","body_contains":"ok"}),
        ] {
            assert!(
                resolve_application_probe_spec(parameters, &context(workspace.path())).is_err()
            );
        }
        let mut contradictory = valid_parameters();
        contradictory["body_contains"] = json!("ready");
        contradictory["max_response_bytes"] = json!(4);
        assert!(resolve_application_probe_spec(contradictory, &context(workspace.path())).is_err());
    }

    #[test]
    fn persisted_plan_tampering_fails_closed() {
        let workspace = tempfile::tempdir().unwrap();
        let ctx = context(workspace.path());
        let mut spec = resolve_application_probe_spec(valid_parameters(), &ctx).unwrap();
        spec.plan.steps[0].program = "sh".to_owned();
        assert!(validate_application_probe_spec(&spec, &ctx).is_err());
    }

    #[tokio::test]
    async fn success_produces_revision_bound_evidence_and_reaps_process() {
        let workspace = git_workspace();
        let (outcome, output) = execute_fixture(workspace.path(), "success").await;
        assert!(outcome.is_success(), "{}", outcome.content);
        assert!(output.success);
        assert!(output.health_ready);
        assert_eq!(output.assertion.as_ref().unwrap().status, 200);
        assert!(
            output
                .assertion
                .as_ref()
                .unwrap()
                .body_sha256
                .starts_with("sha256:")
        );
        assert!(output.teardown.direct_child_reaped);
        assert!(output.teardown.process_tree_settled);
        assert_eq!(output.trust, "external_untrusted");
        assert_eq!(outcome.evidence.status, ToolEvidenceStatus::Produced);
        assert!(outcome.verifier_observation.is_some());
    }

    #[tokio::test]
    async fn status_body_redirect_early_exit_and_bounds_are_typed_failures() {
        let workspace = git_workspace();
        for (mode, expected) in [
            ("status", "application_probe_status_mismatch"),
            ("body", "application_probe_body_mismatch"),
            ("foreign", "application_probe_identity_mismatch"),
            ("redirect", "application_probe_redirect_denied"),
            ("early_exit", "application_probe_early_exit"),
            ("large", "application_probe_response_too_large"),
        ] {
            let (outcome, output) = execute_fixture(workspace.path(), mode).await;
            assert!(!outcome.is_success(), "mode={mode}: {}", outcome.content);
            assert_eq!(
                output.failure_code.as_deref(),
                Some(expected),
                "mode={mode}"
            );
            assert!(output.teardown.process_tree_settled, "mode={mode}");
            assert_eq!(outcome.failure_code, Some(ToolFailureCode::VerifierFailed));
        }
    }

    #[tokio::test]
    async fn actor_without_network_authority_is_denied_before_spawn() {
        let workspace = git_workspace();
        let ctx = context(workspace.path());
        let spec = resolve_application_probe_spec(fixture_parameters("success"), &ctx).unwrap();
        let mut denied_shell = shell(workspace.path());
        denied_shell.elevated_sandbox_policy = Some(SandboxPolicy::default());
        let outcome = execute_application_probe(spec.parameters, &ctx, &denied_shell)
            .await
            .unwrap();
        let output: ApplicationProbeOutput = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(
            output.failure_code.as_deref(),
            Some("application_probe_network_denied")
        );
        assert!(!output.teardown.attempted);
        assert_eq!(outcome.side_effect, ToolSideEffectStatus::NotApplied);
        assert_eq!(outcome.failure_code, Some(ToolFailureCode::VerifierFailed));
    }

    #[tokio::test]
    async fn assertion_cannot_overrun_the_overall_deadline() {
        let _guard = probe_test_lock().lock().await;
        let workspace = git_workspace();
        let ctx = context(workspace.path());
        let mut parameters = fixture_parameters("slow_assertion");
        parameters["startup_timeout_ms"] = json!(1000);
        parameters["health_timeout_ms"] = json!(1500);
        parameters["overall_timeout_ms"] = json!(1800);
        let spec = resolve_application_probe_spec(parameters, &ctx).unwrap();
        let started = Instant::now();
        let outcome = execute_application_probe(spec.parameters, &ctx, &shell(workspace.path()))
            .await
            .unwrap();
        let output: ApplicationProbeOutput = serde_json::from_str(&outcome.content).unwrap();
        assert_eq!(
            output.failure_code.as_deref(),
            Some("application_probe_overall_timeout")
        );
        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(output.teardown.process_tree_settled);
    }

    #[tokio::test]
    async fn log_capture_is_bounded_and_revision_drift_rejects_receipt() {
        let workspace = git_workspace();
        let (logs_outcome, logs) = execute_fixture(workspace.path(), "logs").await;
        assert!(logs_outcome.is_success());
        assert!(logs.stderr.truncated);
        assert!(logs.stderr.bytes_read > logs.stderr.bytes_returned);
        assert!(logs.stderr.bytes_returned <= 1024);

        let (invalid_outcome, invalid_logs) =
            execute_fixture(workspace.path(), "invalid_logs").await;
        assert!(invalid_outcome.is_success());
        assert!(invalid_logs.stderr.truncated);
        assert!(invalid_logs.stderr.bytes_read > invalid_logs.stderr.bytes_returned);
        assert!(invalid_logs.stderr.bytes_returned <= 1024);

        let (drift_outcome, drift) = execute_fixture(workspace.path(), "mutate").await;
        assert!(drift.success);
        assert_eq!(drift_outcome.evidence.status, ToolEvidenceStatus::Stale);
        assert!(drift_outcome.verifier_observation.is_none());
    }

    #[tokio::test]
    async fn cancellation_and_health_timeout_teardown_the_owned_tree() {
        let workspace = git_workspace();
        let ctx = context(workspace.path());
        let token = tokio_util::sync::CancellationToken::new();
        let invocation_context = ctx.for_invocation(token.clone());
        let spec = resolve_application_probe_spec(fixture_parameters("slow"), &ctx).unwrap();
        let shell = shell(workspace.path());
        let outcome = {
            let _guard = probe_test_lock().lock().await;
            let execution = execute_application_probe(spec.parameters, &invocation_context, &shell);
            tokio::pin!(execution);
            tokio::select! {
                result = &mut execution => panic!("probe completed before cancellation: {result:?}"),
                () = tokio::time::sleep(Duration::from_millis(150)) => token.cancel(),
            }
            execution.await.unwrap()
        };
        let output: ApplicationProbeOutput = serde_json::from_str(&outcome.content).unwrap();
        assert!(output.cancelled);
        assert!(output.teardown.process_tree_settled);

        let (timeout_outcome, timeout) = execute_fixture(workspace.path(), "slow").await;
        assert!(!timeout_outcome.is_success());
        assert_eq!(
            timeout.failure_code.as_deref(),
            Some("application_probe_health_timeout")
        );
        assert!(timeout.timed_out);
        assert!(timeout.teardown.process_tree_settled);
    }

    #[tokio::test]
    async fn durable_lease_recovers_exact_process_group_without_pid_or_port() {
        let _guard = probe_test_lock().lock().await;
        let workspace = git_workspace();
        let ctx = context(workspace.path());
        let spec = resolve_application_probe_spec(fixture_parameters("slow"), &ctx).unwrap();
        let input = parse_input(spec.parameters.clone()).unwrap();
        let token = input.lease_token.clone().unwrap();
        let mut command = StdCommand::new("sleep");
        command.arg("30").env(LEASE_ENV, &token);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.arg0(format!("{LEASE_ARG0_PREFIX}{token}"));
        }
        configure_process_tree(&mut command);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let recovery = recover_application_probe(&spec, &ctx).await.unwrap();
        assert!(recovery.matched_processes >= 1);
        assert_eq!(recovery.killed_process_groups, 1);
        let status = child.wait().unwrap();
        assert!(!status.success());
        #[cfg(unix)]
        assert_ne!(unsafe { libc::kill(pid as libc::pid_t, 0) }, 0);
    }
}
