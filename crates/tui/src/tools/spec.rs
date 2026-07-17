//! Tool specification traits for the CodeWhale agent system.
//!
//! This module defines the core abstractions for tools:
//! - `ToolSpec`: The main trait that all tools must implement
//! - `ToolContext`: Execution context passed to tools
//! - `ToolOutcome`: Unified result type for tool execution
//! - `ToolCapability`: Capabilities and requirements of tools

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::features::Features;
use crate::lsp::LspManager;
use crate::network_policy::NetworkPolicyDecider;
use crate::rlm::session::SessionObjectSnapshot;
use crate::rlm::session::{SharedRlmSessionStore, new_shared_rlm_session_store};
use crate::tools::handle::{SharedHandleStore, new_shared_handle_store};
use crate::worker_profile::ShellPolicy;
use codewhale_tools::ProductionToolContext;
use codewhale_tools::sandbox::backend::SandboxBackend;
use codewhale_tools::shell::{SharedShellManager, new_shared_shell_manager};
#[allow(unused_imports)]
pub use codewhale_tools::{
    ApprovalRequirement, ToolCapability, ToolError, ToolOutcome, optional_bool, optional_str,
    optional_u64, required_str, required_u64,
};

#[async_trait]
pub trait DynamicToolExecutor: Send + Sync {
    async fn execute_dynamic_tool(
        &self,
        thread_id: Option<String>,
        namespace: Option<String>,
        name: String,
        input: Value,
    ) -> Result<ToolOutcome, ToolError>;
}

/// Optional process-local services made available to model-visible tools.
#[derive(Clone)]
pub struct RuntimeToolServices {
    pub shell_manager: Option<SharedShellManager>,
    pub dynamic_tool_executor: Option<Arc<dyn DynamicToolExecutor>>,
    /// Hook executor for `shell_env` injection (#456) and any future
    /// tool-side hook events. `None` outside the live engine — test
    /// contexts that don't care about hooks get a no-op.
    pub hook_executor: Option<std::sync::Arc<crate::hooks::HookExecutor>>,
    /// Per-session backing store for `var_handle` payloads. Cloned tool
    /// contexts share this Arc so handles survive across turns.
    pub handle_store: SharedHandleStore,
    /// Per-session persistent RLM kernels, keyed by caller-chosen context name.
    pub rlm_sessions: SharedRlmSessionStore,
}

impl Default for RuntimeToolServices {
    fn default() -> Self {
        Self {
            shell_manager: None,
            dynamic_tool_executor: None,
            hook_executor: None,
            handle_store: new_shared_handle_store(),
            rlm_sessions: new_shared_rlm_session_store(),
        }
    }
}

impl std::fmt::Debug for RuntimeToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeToolServices")
            .field("shell_manager", &self.shell_manager.is_some())
            .field(
                "dynamic_tool_executor",
                &self.dynamic_tool_executor.is_some(),
            )
            .field("hook_executor", &self.hook_executor.is_some())
            .field("handle_store", &true)
            .field("rlm_sessions", &true)
            .finish()
    }
}

/// Sandbox policy for command execution.
#[derive(Debug, Clone, Default)]
pub enum SandboxPolicy {
    /// No sandboxing (dangerous but sometimes needed)
    #[default]
    None,
}

/// Context passed to tools during execution.
#[derive(Clone)]
pub struct ToolContext {
    /// TUI-independent state owned by the production tools crate.
    production: ProductionToolContext,
    /// Read-only snapshot of the active Goal acceptance contract at the start
    /// of this tool-execution context. Verifiers bind receipts to this exact
    /// generation; a later Goal can never inherit an earlier result.
    pub goal_contract: Option<crate::tools::goal::TaskContract>,
    /// Shared shell manager for background tasks and streaming IO.
    pub shell_manager: SharedShellManager,
    /// Sub-agent that owns tool work started through this context. Root user
    /// turns leave this unset; child contexts stamp it so long-running shell
    /// jobs can be attributed in UI surfaces.
    pub owner_agent_id: Option<String>,
    pub owner_agent_name: Option<String>,
    /// Current sandbox policy
    #[allow(dead_code)]
    pub sandbox_policy: SandboxPolicy,
    /// Path for notes file
    pub notes_path: PathBuf,
    /// MCP configuration path
    #[allow(dead_code)]
    pub mcp_config_path: PathBuf,
    /// Explicit skills directory used for model-visible skill discovery.
    pub skills_dir: Option<PathBuf>,
    /// Restrict skill discovery to CodeWhale-owned roots plus `skills_dir`.
    pub skills_scan_codewhale_only: bool,
    /// Elevated sandbox policy override (used when retrying after sandbox denial).
    /// This overrides the default sandbox behavior for shell commands.
    pub elevated_sandbox_policy: Option<codewhale_tools::sandbox::SandboxPolicy>,
    /// Optional user-facing hint for shell commands that fail because the
    /// active sandbox policy intentionally denies outbound network access.
    pub shell_network_denied_hint: Option<String>,
    /// Effective shell policy for this execution context.
    pub shell_policy: ShellPolicy,
    /// Effective feature flag set for the running session.
    pub features: Features,
    /// Namespace for tool state that should be scoped to the current session/thread.
    pub state_namespace: String,
    /// Per-domain network policy (#135). When `None`, network tools fall back
    /// to a permissive default that mirrors pre-v0.7.0 behavior so tests and
    /// other contexts that don't construct a real policy keep working.
    pub network_policy: Option<NetworkPolicyDecider>,
    /// Durable runtime services for task, gate, PR-attempt, GitHub evidence,
    /// and automation tools.
    pub runtime: RuntimeToolServices,
    /// Snapshot of the active prompt/session/history exposed as symbolic RLM
    /// objects. Tools only receive compact cards unless explicitly opening a
    /// bounded object through `rlm_open`.
    pub session_objects: Option<SessionObjectSnapshot>,
    /// Optional external sandbox backend for shell execution.
    /// When set, exec_shell routes commands through this instead of spawning
    /// a local process.
    pub sandbox_backend: Option<std::sync::Arc<dyn SandboxBackend>>,
    /// Path to the user memory file. `None` when the user-memory feature
    /// (#489) is disabled — tools that read or write the file should
    /// short-circuit on `None` rather than fall back to a workspace-local
    /// default.
    pub memory_path: Option<PathBuf>,
    /// LSP manager for post-edit diagnostics injection (#428). `None` when
    /// LSP is disabled or the context is constructed in a test that does not
    /// need diagnostics. Edit tools append a `<diagnostics>` block to their
    /// result when this is present and the manager is enabled.
    pub lsp_manager: Option<Arc<LspManager>>,

    /// Large-output router (#548). When `Some`, tool results that exceed the
    /// configured token threshold are routed through a V4-Flash synthesis
    /// sub-agent before being returned to the parent context. `None` disables
    /// routing (e.g. in sub-agents and test contexts to avoid recursion).
    pub large_output_router: Option<crate::tools::large_output_router::LargeOutputRouter>,

    /// Which search backend `web_search` should use. Default: DuckDuckGo. Set via
    /// `[search] provider` in config.toml.
    pub search_provider: crate::config::SearchProvider,
    /// API key for Tavily, Bocha, Metaso, or Baidu. `None` for Bing or DuckDuckGo.
    /// Metaso also falls back to `METASO_API_KEY` env var, then a built-in key.
    /// Baidu also falls back to `BAIDU_SEARCH_API_KEY`.
    pub search_api_key: Option<String>,
    /// Optional DuckDuckGo-compatible HTML endpoint override for `web_search`.
    pub search_base_url: Option<String>,

    /// Per-session workshop variable store (#548). Holds the raw content of
    /// the most recent large-tool routing event so the parent can call
    /// `promote_to_context` later. `None` when the router is disabled.
    pub workshop_vars: Option<
        std::sync::Arc<tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>>,
    >,
}

impl ToolContext {
    /// Create a new `ToolContext` with default settings.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        // Prefer .codewhale, fall back to .deepseek for project-local state
        let notes_path = codewhale_config::resolve_project_state_dir(&workspace, "notes.md")
            .expect("hardcoded project notes state path is valid")
            .1;
        let mcp_config_path = codewhale_config::resolve_project_state_dir(&workspace, "mcp.json")
            .expect("hardcoded project MCP state path is valid")
            .1;
        Self {
            production: ProductionToolContext::new(workspace),
            goal_contract: None,
            shell_manager,
            owner_agent_id: None,
            owner_agent_name: None,
            sandbox_policy: SandboxPolicy::None,
            notes_path,
            mcp_config_path,
            skills_dir: None,
            skills_scan_codewhale_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            sandbox_backend: None,
            memory_path: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Create a `ToolContext` with all settings specified.
    #[allow(dead_code)]
    pub fn with_options(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        let production = ProductionToolContext::new(workspace).with_trust_mode(trust_mode);
        Self {
            production,
            goal_contract: None,
            shell_manager,
            owner_agent_id: None,
            owner_agent_name: None,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            skills_dir: None,
            skills_scan_codewhale_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            sandbox_backend: None,
            memory_path: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Create a `ToolContext` with auto-approve mode (YOLO).
    pub fn with_auto_approve(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
        auto_approve: bool,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        let production = ProductionToolContext::new(workspace)
            .with_trust_mode(trust_mode)
            .with_auto_approve(auto_approve);
        Self {
            production,
            goal_contract: None,
            shell_manager,
            owner_agent_id: None,
            owner_agent_name: None,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            skills_dir: None,
            skills_scan_codewhale_only: false,
            elevated_sandbox_policy: None,
            shell_network_denied_hint: None,
            shell_policy: ShellPolicy::Full,
            features: Features::with_defaults(),
            state_namespace: "workspace".to_string(),
            network_policy: None,
            runtime: RuntimeToolServices::default(),
            session_objects: None,
            sandbox_backend: None,
            memory_path: None,
            lsp_manager: None,
            large_output_router: None,
            search_provider: crate::config::SearchProvider::default(),
            search_api_key: None,
            search_base_url: None,
            workshop_vars: None,
        }
    }

    /// Workspace root owned by the TUI-independent production context.
    #[must_use]
    pub fn workspace(&self) -> &Path {
        self.production.workspace()
    }

    /// Whether unrestricted path access is active.
    #[must_use]
    pub fn trust_mode(&self) -> bool {
        self.production.trust_mode()
    }

    /// User-approved roots outside the workspace.
    #[must_use]
    pub fn trusted_external_paths(&self) -> &[PathBuf] {
        self.production.trusted_external_paths()
    }

    /// Whether workspace symlinks may resolve outside the workspace.
    #[must_use]
    pub fn follow_symlinks(&self) -> bool {
        self.production.follow_symlinks()
    }

    /// Cancellation signal for the current tool invocation.
    #[must_use]
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.production.cancellation_token()
    }

    /// Whether eligible operations may bypass approval prompts.
    #[must_use]
    pub fn auto_approve(&self) -> bool {
        self.production.auto_approve()
    }

    /// TUI-independent context used by production tool operations.
    #[must_use]
    pub(crate) fn production_context(&self) -> &ProductionToolContext {
        &self.production
    }

    /// Replace the invocation cancellation signal while preserving file-read
    /// freshness for the current workspace.
    pub fn set_invocation_cancellation(&mut self, cancellation: CancellationToken) {
        self.production = self.production.for_invocation(cancellation);
    }

    /// Replace the approval posture in the production context snapshot.
    pub fn set_auto_approve(&mut self, auto_approve: bool) {
        self.production = self.production.clone().with_auto_approve(auto_approve);
    }

    /// Replace the explicit external roots in the production context snapshot.
    pub fn set_trusted_external_paths(&mut self, paths: Vec<PathBuf>) {
        self.production = self.production.clone().with_trusted_external_paths(paths);
    }

    /// Replace workspace symlink traversal policy.
    pub fn set_follow_symlinks(&mut self, follow_symlinks: bool) {
        self.production = self
            .production
            .clone()
            .with_follow_symlinks(follow_symlinks);
    }

    /// Rebind a child context to its worktree and discard parent read evidence.
    pub fn rebind_workspace(&mut self, workspace: impl Into<PathBuf>) {
        self.production.rebind_workspace(workspace);
    }

    /// Resolve and validate a tool path through the production context.
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, ToolError> {
        self.production.resolve_path(raw)
    }

    /// Record a successful file read in the production freshness tracker.
    pub fn note_file_read(&self, path: &Path) {
        self.production.note_file_read(path);
    }

    /// Attach a per-domain network policy to this context (#135).
    #[must_use]
    pub fn with_network_policy(mut self, policy: NetworkPolicyDecider) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Attach durable runtime services to tools.
    #[must_use]
    pub fn with_runtime_services(mut self, runtime: RuntimeToolServices) -> Self {
        self.runtime = runtime;
        self
    }

    /// Bind tool execution to one active Goal acceptance contract.
    #[must_use]
    pub fn with_goal_contract(
        mut self,
        contract: Option<crate::tools::goal::TaskContract>,
    ) -> Self {
        self.goal_contract = contract;
        self
    }

    /// Stamp tool work with the sub-agent that owns it.
    #[must_use]
    pub fn with_owner_agent(
        mut self,
        agent_id: impl Into<String>,
        agent_name: impl Into<String>,
    ) -> Self {
        let agent_id = agent_id.into();
        let agent_name = agent_name.into();
        self.owner_agent_id = (!agent_id.trim().is_empty()).then_some(agent_id);
        self.owner_agent_name = (!agent_name.trim().is_empty()).then_some(agent_name);
        self
    }

    /// Attach skill discovery settings for tools that need to resolve
    /// model-visible skills by name.
    #[must_use]
    pub fn with_skills_config(
        mut self,
        skills_dir: impl Into<PathBuf>,
        scan_codewhale_only: bool,
    ) -> Self {
        self.skills_dir = Some(skills_dir.into());
        self.skills_scan_codewhale_only = scan_codewhale_only;
        self
    }

    /// Attach active prompt/history/session symbolic objects for RLM tools.
    #[must_use]
    pub fn with_session_objects(mut self, snapshot: SessionObjectSnapshot) -> Self {
        self.session_objects = Some(snapshot);
        self
    }

    /// Attach the effective shell policy for this turn.
    #[must_use]
    pub fn with_shell_policy(mut self, policy: ShellPolicy) -> Self {
        self.shell_policy = policy;
        self
    }

    /// Attach an external sandbox backend for remote shell execution.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_sandbox_backend(mut self, backend: std::sync::Arc<dyn SandboxBackend>) -> Self {
        self.sandbox_backend = Some(backend);
        self
    }

    /// Attach an LSP manager so that edit tools can auto-inject diagnostics
    /// into their results after a successful file modification (#428).
    #[must_use]
    #[allow(dead_code)]
    pub fn with_lsp_manager(mut self, manager: Arc<LspManager>) -> Self {
        self.lsp_manager = Some(manager);
        self
    }

    /// Set the sandbox policy.
    #[allow(dead_code)]
    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    /// Set feature flags for tool execution.
    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    /// Override the shared shell manager.
    pub fn with_shell_manager(mut self, shell_manager: SharedShellManager) -> Self {
        self.shell_manager = shell_manager;
        self
    }

    /// Set the elevated sandbox policy override.
    ///
    /// This is used when retrying a tool after a sandbox denial, to run
    /// with elevated permissions.
    pub fn with_elevated_sandbox_policy(
        mut self,
        policy: codewhale_tools::sandbox::SandboxPolicy,
    ) -> Self {
        self.elevated_sandbox_policy = Some(policy);
        self
    }

    /// Set the shell network-denial hint used by network-restricted modes.
    pub fn with_shell_network_denied_hint(mut self, hint: impl Into<String>) -> Self {
        self.shell_network_denied_hint = Some(hint.into());
        self
    }

    /// Set the namespace used for session-scoped tool state.
    pub fn with_state_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.state_namespace = namespace.into();
        self
    }

    /// Attach the large-output router (#548). When set, tool results that
    /// exceed the configured token threshold are synthesised by a V4-Flash
    /// sub-agent before being returned to the parent context.
    #[must_use]
    pub fn with_large_output_router(
        mut self,
        router: crate::tools::large_output_router::LargeOutputRouter,
        vars: std::sync::Arc<
            tokio::sync::Mutex<crate::tools::large_output_router::WorkshopVariables>,
        >,
    ) -> Self {
        self.large_output_router = Some(router);
        self.workshop_vars = Some(vars);
        self
    }
}

/// Gather LSP diagnostics for `paths` using the manager stored in `context`,
/// and return the rendered `<diagnostics …>` blocks joined by newlines.
///
/// Returns an empty string when:
/// - `context.lsp_manager` is `None`
/// - the manager's `enabled` flag is `false`
/// - none of the files produce diagnostics (e.g. all clean, or language unknown)
///
/// This function is non-blocking by design: every failure mode (missing LSP
/// binary, timeout, unknown language) degrades to an empty string rather than
/// propagating an error to the caller.
pub async fn lsp_diagnostics_for_paths(context: &ToolContext, paths: &[PathBuf]) -> String {
    use crate::lsp::render_blocks;

    let manager = match context.lsp_manager.as_ref() {
        Some(m) if m.config().enabled => m,
        _ => return String::new(),
    };

    let mut blocks = Vec::new();
    for (idx, path) in paths.iter().enumerate() {
        if let Some(block) = manager.diagnostics_for(path, idx as u64).await {
            blocks.push(block);
        }
    }

    render_blocks(&blocks)
}

/// The core trait that all tools must implement.
#[async_trait]
pub trait ToolSpec: Send + Sync {
    /// Returns the unique name of this tool (used in API calls).
    fn name(&self) -> &str;

    /// Returns a human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// Returns the JSON Schema for the tool's input parameters.
    fn input_schema(&self) -> Value;

    /// Returns the capabilities this tool has.
    fn capabilities(&self) -> Vec<ToolCapability>;

    /// Returns the approval requirement for this tool.
    fn approval_requirement(&self) -> ApprovalRequirement {
        let caps = self.capabilities();
        if caps.contains(&ToolCapability::ExecutesCode) {
            ApprovalRequirement::Required
        } else if caps.contains(&ToolCapability::WritesFiles) {
            ApprovalRequirement::Suggest
        } else {
            ApprovalRequirement::Auto
        }
    }

    /// Returns the approval requirement for this concrete tool input.
    fn approval_requirement_for(&self, _input: &Value) -> ApprovalRequirement {
        self.approval_requirement()
    }

    /// Returns whether this tool is sandboxable.
    #[allow(dead_code)]
    fn is_sandboxable(&self) -> bool {
        self.capabilities().contains(&ToolCapability::Sandboxable)
    }

    /// Returns whether this tool is read-only.
    fn is_read_only(&self) -> bool {
        let caps = self.capabilities();
        caps.contains(&ToolCapability::ReadOnly)
            && !caps.contains(&ToolCapability::WritesFiles)
            && !caps.contains(&ToolCapability::ExecutesCode)
    }

    /// Returns whether this concrete tool input is read-only.
    fn is_read_only_for(&self, _input: &Value) -> bool {
        self.is_read_only()
    }

    /// Returns whether this tool can be executed in parallel with others.
    fn supports_parallel(&self) -> bool {
        false
    }

    /// Returns whether this concrete tool input can run in parallel.
    fn supports_parallel_for(&self, _input: &Value) -> bool {
        self.supports_parallel()
    }

    /// Returns whether this input starts durable/detached work and returns
    /// immediately. Detached starts are not read-only, but in auto-approved
    /// turns they do not need to block neighboring read-only inspections.
    fn starts_detached_for(&self, _input: &Value) -> bool {
        false
    }

    /// Returns whether this tool should be excluded from the model-visible
    /// tool catalog (deferred loading). Tools marked `true` are registered
    /// but not sent to the model until explicitly activated via tool search.
    fn defer_loading(&self) -> bool {
        false
    }

    /// Returns whether this tool should be advertised in the model-facing
    /// catalog. Hidden compatibility tools remain registered and executable
    /// by name so saved transcripts can replay without teaching new sessions
    /// the deprecated spelling.
    fn model_visible(&self) -> bool {
        true
    }

    /// Execute the tool with the given input and context.
    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError>;
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_outcome_success() {
        let result = ToolOutcome::success("hello");
        assert!(result.is_success());
        assert_eq!(result.content, "hello");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn test_tool_outcome_error() {
        let result = ToolOutcome::error("something failed");
        assert!(!result.is_success());
        assert_eq!(result.content, "something failed");
    }

    #[test]
    fn test_tool_outcome_json() {
        let data = json!({"key": "value"});
        let result = ToolOutcome::json(&data).unwrap();
        assert!(result.is_success());
        assert!(result.content.contains("key"));
    }

    #[test]
    fn test_tool_outcome_with_metadata() {
        let result = ToolOutcome::success("content").with_metadata(json!({"extra": true}));
        assert!(result.metadata.is_some());
    }

    #[test]
    fn tool_context_has_one_mutable_production_state() {
        let cancellation = CancellationToken::new();
        let mut context = ToolContext::new(".");
        context.set_auto_approve(true);
        context.set_invocation_cancellation(cancellation.clone());

        assert!(context.auto_approve());
        assert!(!context.trust_mode());
        cancellation.cancel();
        assert!(context.cancellation_token().unwrap().is_cancelled());
    }

    #[test]
    fn test_required_str() {
        let input = json!({"name": "test", "count": 42});
        assert_eq!(required_str(&input, "name").unwrap(), "test");
        assert!(required_str(&input, "missing").is_err());
        assert!(required_str(&input, "count").is_err()); // not a string
    }

    #[test]
    fn test_optional_str() {
        let input = json!({"name": "test"});
        assert_eq!(optional_str(&input, "name"), Some("test"));
        assert_eq!(optional_str(&input, "missing"), None);
    }

    #[test]
    fn test_required_u64() {
        let input = json!({"count": 42});
        assert_eq!(required_u64(&input, "count").unwrap(), 42);
        assert!(required_u64(&input, "missing").is_err());
    }

    #[test]
    fn test_optional_u64() {
        let input = json!({"count": 42});
        assert_eq!(optional_u64(&input, "count", 0), 42);
        assert_eq!(optional_u64(&input, "missing", 100), 100);
    }

    #[test]
    fn test_optional_bool() {
        let input = json!({"flag": true});
        assert!(optional_bool(&input, "flag", false));
        assert!(!optional_bool(&input, "missing", false));
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::missing_field("path");
        assert_eq!(
            format!("{err}"),
            "Failed to validate input: missing required field 'path'"
        );

        let err = ToolError::execution_failed("boom");
        assert_eq!(format!("{err}"), "Failed to execute tool: boom");
    }

    #[test]
    fn test_approval_requirement_default() {
        let level = ApprovalRequirement::default();
        assert_eq!(level, ApprovalRequirement::Auto);
    }
}
