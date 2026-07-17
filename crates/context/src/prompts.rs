//! System prompts for different modes.
//!
//! Prompts are assembled from composable layers loaded at compile time:
//!   constitution.md + personality overlay → message[0] (byte-stable).
//!   mode delta + tool taxonomy + approval policy → request-time runtime metadata.
//!
//! This keeps each concern in its own file and makes prompt tuning
//! a single-file operation.

use crate::project_context::{ProjectContext, load_project_context_with_parents};
use codewhale_config::PromptPreferences;
use codewhale_protocol::agent_runtime::{
    PromptCacheControl, SystemPrompt, SystemPromptBlock as SystemBlock,
};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone)]
pub struct PromptSessionContext<'a> {
    pub user_memory_block: Option<&'a str>,
    pub goal_objective: Option<&'a str>,
    pub project_context_pack_enabled: bool,
    /// Active model identifier. The bundled constitution is model-agnostic,
    /// but embedders may still provide a prompt override containing
    /// `{model_id}`. Defaults to `"codewhale"` when the caller doesn't supply one.
    pub model_id: &'a str,
    /// Route-effective context window, when known. Prompt composition no
    /// longer prints context-window facts, but the field remains part of the
    /// session context contract for embedders and future runtime metadata.
    pub context_window_override: Option<u32>,
    /// Whether the user-visible transcript renders thinking blocks.
    pub show_thinking: bool,
    /// Optional output-verbosity mode. `concise` appends a short output
    /// discipline block; unset keeps the normal conversational prompt.
    pub verbosity: Option<&'a str>,
    /// Restrict skill discovery to CodeWhale-owned roots plus explicit
    /// `skills_dir` configuration.
    pub skills_scan_codewhale_only: bool,
    /// Caller-resolved shell binary. Host detection stays outside prompt
    /// composition so this crate has no presentation-process singleton.
    pub shell_binary: &'a str,
}

impl Default for PromptSessionContext<'_> {
    fn default() -> Self {
        Self {
            user_memory_block: None,
            goal_objective: None,
            project_context_pack_enabled: true,
            model_id: "codewhale",
            context_window_override: None,
            show_thinking: true,
            verbosity: None,
            skills_scan_codewhale_only: false,
            shell_binary: "sh",
        }
    }
}

/// Complete input for the canonical production system prompt.
#[derive(Debug)]
pub struct ProductionPromptRequest<'a> {
    pub workspace: &'a Path,
    pub model: &'a str,
    pub preferences: &'a PromptPreferences,
    pub instructions: &'a [InstructionSource],
    pub skills_dir: Option<&'a Path>,
    pub project_context_pack_enabled: bool,
    pub verbosity: Option<&'a str>,
    pub skills_scan_codewhale_only: bool,
    pub shell_binary: &'a str,
    pub tool_mode: bool,
}

/// Build the prompt shared by root and child runtimes, including the final
/// request-volatile execution-posture block.
#[must_use]
pub fn production_system_prompt(request: ProductionPromptRequest<'_>) -> SystemPrompt {
    let mut prompt = system_prompt_for_mode_with_context_skills_and_session(
        request.workspace,
        None,
        request.skills_dir,
        Some(request.instructions),
        PromptSessionContext {
            user_memory_block: None,
            goal_objective: None,
            project_context_pack_enabled: request.project_context_pack_enabled,
            model_id: request.model,
            context_window_override: None,
            show_thinking: request.preferences.show_thinking,
            verbosity: request.verbosity,
            skills_scan_codewhale_only: request.skills_scan_codewhale_only,
            shell_binary: request.shell_binary,
        },
    );
    prompt.blocks.push(SystemBlock {
        text: if request.tool_mode {
            "你正在唯一 AgentRuntime 中执行编码任务。只使用本次请求实际提供的工具；先读取再修改，修改后运行最相关验证。`agent` 会启动同一 Runtime 的后台子 Agent，运行时会自动等待并把结构化结果回注；不要轮询或调用不存在的等待工具。".to_owned()
        } else {
            "本次是无工具执行。直接给出准确、简洁、可操作的最终答案，不要声称执行了文件或命令操作。".to_owned()
        },
        cache_control: PromptCacheControl::Volatile,
    });
    prompt
}

/// Conventional location for the structured session relay artifact (#32).
/// A previous session writes it on exit / `/compact`; the next session reads
/// it back on startup and prepends it to the system prompt so a fresh agent
/// doesn't have to re-discover open blockers from scratch.
pub const HANDOFF_RELATIVE_PATH: &str = ".codewhale/handoff.md";
/// Legacy handoff path for reading from existing installs.
const LEGACY_HANDOFF_RELATIVE_PATH: &str = ".deepseek/handoff.md";

/// Per-file size cap for `instructions = [...]` entries (#454). Mirrors
/// the existing project-context cap in `project_context::load_context_file`
/// so a malicious / oversized include can't blow the prompt budget on
/// its own. Files larger than this are truncated with an explicit `[…truncated: N bytes omitted]`
/// marker rather than skipped entirely so the model still sees the head.
const INSTRUCTIONS_FILE_MAX_BYTES: usize = 100 * 1024;

fn concise_output_discipline_instruction() -> &'static str {
    "\
## Concise Output Discipline

To minimize token usage and optimize speed:
- Output only direct, actionable code, technical steps, or final answers.
- Eliminate all conversational filler, fluff, introductions, transitions, or summarizing conclusions.
- Do NOT explain what you are about to do or what you have just completed.
- Do NOT provide conversational status updates before or after running tools.
- Keep explanations and comments extremely brief and technical, explaining only non-obvious reasoning."
}

fn is_concise_verbosity(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("concise"))
}

/// Render a `## Environment` block listing the fixed product locale,
/// runtime version, host platform, login shell, and current working directory.
///
/// The block is appended to the workspace-static portion of the
/// system prompt (after mode prompt + project context, before
/// configured instructions / skills).
fn render_environment_block(_workspace: &Path, shell: &str) -> String {
    let codewhale_version = env!("CARGO_PKG_VERSION");
    let platform = std::env::consts::OS;

    // The workspace path (`pwd`) is intentionally delivered per-turn via the
    // `<turn_meta>` block (see `turn_metadata_block`) rather than embedded here.
    //
    // Rationale: when the workspace path changes between sessions (e.g. an
    // ephemeral per-session workspace), a volatile value inside the otherwise
    // static system prefix invalidates the inference server's prefix cache at
    // that exact point. The cache then only partially matches and the tail must
    // be re-prefilled from the divergence boundary. On backends that pair prefix
    // caching with speculative decoding, this partial re-prefill can perturb the
    // logits at the boundary enough to degrade structured tool-call emission
    // (the model regresses to bare text). Keeping the static system prefix
    // byte-identical across sessions lets the prefix cache be reused; the live
    // workspace path still reaches the model every turn through `turn_meta`.
    format!(
        "## Environment\n\
         \n\
         - lang: zh-Hans\n\
         - codewhale_version: {codewhale_version}\n\
         - platform: {platform}\n\
         - shell: {shell}"
    )
}

/// Source for an `EngineConfig.instructions` entry. Either a disk file (loaded
/// at render time, original semantics) or an inline string (content baked into
/// `EngineConfig`, no disk I/O at render time).
///
/// The inline variant is useful for embedders that compute instructions at
/// runtime (e.g. rendering a template with workspace-specific substitutions)
/// and don't want to stage the content to a disk file just to satisfy a path
/// API. Staging adds two problems the inline path avoids:
///
///   1. The disk file looks like editable config but gets overwritten on
///      every launch — confusing for users browsing the install dir.
///   2. Multi-engine setups need per-engine paths to avoid `rehydrate`
///      reading another session's instructions; with inline sources the
///      content lives in the per-engine `EngineConfig` and the race
///      surface goes away.
///
/// `From<PathBuf>` is provided so existing callers passing `Vec<PathBuf>` can
/// keep working with a `.into()` upgrade at the call site.
#[derive(Debug, Clone)]
pub enum InstructionSource {
    /// Load this file from disk at prompt-render time. Original behavior:
    /// missing files are skipped with a warning, oversized files are
    /// truncated to `INSTRUCTIONS_FILE_MAX_BYTES` with an `[…elided]`
    /// marker.
    File(PathBuf),
    /// Use the provided string directly. `name` becomes the
    /// `<instructions source="…">` attribute (typically a synthetic
    /// identifier like `embedded:my-template` or a logical path).
    Inline { name: String, content: String },
}

impl From<PathBuf> for InstructionSource {
    fn from(path: PathBuf) -> Self {
        InstructionSource::File(path)
    }
}

impl From<&PathBuf> for InstructionSource {
    fn from(path: &PathBuf) -> Self {
        InstructionSource::File(path.clone())
    }
}

/// Render the `instructions = [...]` config array as a single
/// system-prompt block (#454). Each source is processed in declared order;
/// missing `File` sources are skipped with a tracing warning so a stale entry
/// doesn't fail the launch. Empty input (or all sources missing/empty)
/// returns `None` so callers append nothing.
fn render_instructions_block(sources: &[InstructionSource]) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();
    for source in sources {
        let (raw_source_name, raw_content): (String, String) = match source {
            InstructionSource::File(path) => match std::fs::read_to_string(path) {
                Ok(raw) => (path.display().to_string(), raw),
                Err(err) => {
                    tracing::warn!(
                        target: "instructions",
                        ?err,
                        ?path,
                        "skipping unreadable instructions file"
                    );
                    continue;
                }
            },
            InstructionSource::Inline { name, content } => (name.clone(), content.clone()),
        };
        let trimmed = raw_content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let body = if trimmed.len() > INSTRUCTIONS_FILE_MAX_BYTES {
            let head_end = (0..=INSTRUCTIONS_FILE_MAX_BYTES)
                .rev()
                .find(|&i| trimmed.is_char_boundary(i))
                .unwrap_or(0);
            format!(
                "{}\n[…truncated: {} of {} bytes omitted — consider splitting this instructions file]",
                &trimmed[..head_end],
                trimmed.len() - head_end,
                trimmed.len()
            )
        } else {
            trimmed.to_string()
        };
        sections.push(format!(
            "<instructions source=\"{raw_source_name}\">\n{body}\n</instructions>"
        ));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

/// Read the workspace-local relay artifact, if present, and format it as a
/// system-prompt block. Returns `None` when the file is absent or empty so
/// callers can keep the default-uncluttered prompt for fresh workspaces.
fn load_handoff_block(workspace: &Path) -> Option<String> {
    let primary = workspace.join(HANDOFF_RELATIVE_PATH);
    let path = if primary.exists() {
        primary
    } else {
        workspace.join(LEGACY_HANDOFF_RELATIVE_PATH)
    };
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(format!(
        "## Previous Session Relay\n\nThe previous session in this workspace left a relay artifact at `{HANDOFF_RELATIVE_PATH}`. Consider it the first artifact to read on this turn — open blockers, in-flight changes, and recent decisions live there. Update or rewrite it before exiting if state changes materially.\n\n{trimmed}"
    ))
}

/// Load the structured user-global constitution, if present, and render it as
/// its own model-facing block.
fn load_user_constitution_block() -> Option<String> {
    if user_constitution_disabled_by_setup_state() {
        return None;
    }

    let path = match codewhale_config::UserConstitution::path() {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                target: "prompts",
                "could not resolve user-global constitution path: {err:#}"
            );
            return None;
        }
    };

    match codewhale_config::UserConstitution::load_from(&path) {
        codewhale_config::UserConstitutionLoad::Loaded(constitution) => {
            constitution.render_block(None)
        }
        codewhale_config::UserConstitutionLoad::Missing
        | codewhale_config::UserConstitutionLoad::Empty => None,
        codewhale_config::UserConstitutionLoad::Invalid(err) => {
            tracing::warn!(
                target: "prompts",
                "skipping invalid user-global constitution {}: {err}",
                path.display()
            );
            None
        }
        codewhale_config::UserConstitutionLoad::Unreadable(err) => {
            tracing::warn!(
                target: "prompts",
                "skipping unreadable user-global constitution {}: {err}",
                path.display()
            );
            None
        }
    }
}

fn user_constitution_disabled_by_setup_state() -> bool {
    match codewhale_config::SetupState::load() {
        Ok(Some(state)) => matches!(
            state.constitution_choice,
            codewhale_config::ConstitutionChoice::Bundled
                | codewhale_config::ConstitutionChoice::Deferred
                | codewhale_config::ConstitutionChoice::ExpertOverride
        ),
        Ok(None) => false,
        Err(err) => {
            tracing::warn!(
                target: "prompts",
                "could not resolve setup-state path while loading user constitution: {err:#}"
            );
            false
        }
    }
}

// ── Prompt layers loaded at compile time ──────────────────────────────

/// Core: task execution, tool-use rules, output format, toolbox reference,
/// "When NOT to use" guidance, sub-agent sentinel protocol.
///
/// This markdown is the single hand-maintained source of the constitutional
/// system prompt. The earlier YAML + Python-renderer generation pipeline
/// (`constitution.yaml` / `render_constitution.py`) was retired because it
/// had drifted from this file since the v4 "zero ceremony" adoption and the
/// renderer could no longer reproduce it byte-for-byte. The layered runtime
/// assembly composes this core with mode / approval / skills /
/// context-management / compaction / authority-recap layers at runtime (see
/// `system_prompt_for_mode_with_context_skills_and_session`). Edit this file
/// directly; `constitution_md_carries_required_structure` guards its skeleton.
pub const BASE_PROMPT: &str = include_str!("prompts/constitution.md");
/// Language mirroring law, split from the compact constitution in 0.9.0.
pub const LANGUAGE_PROMPT: &str = include_str!("prompts/language.md");
/// Terminal-facing output formatting law, split from the compact constitution.
pub const OUTPUT_PROMPT: &str = include_str!("prompts/output.md");

// ── Embedder prompt overrides ──
// Let an embedder replace these compile-time prompt constants at startup,
// so brand / slimming customizations live in the embedder crate instead of
// editing these files in-tree. Unset → the bundled constant (fully
// backward compatible). Intended to be set once at process start, before
// any engine spawns; later sets return the rejected override string.
static BASE_PROMPT_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_PREAMBLE_ZH_HANS_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static LOCALE_CLOSER_ZH_HANS_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static AUTHORITY_RECAP_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static STATIC_PROMPT_COMPOSER: std::sync::OnceLock<Box<StaticPromptComposer>> =
    std::sync::OnceLock::new();
static PROMPT_OVERRIDE_NOTICES: LazyLock<Mutex<Vec<String>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Context passed to an embedder-provided static prompt composer.
///
/// This hook only replaces the byte-stable base/personality prompt segment.
/// Mode deltas, approval policy, tool taxonomy, Core Execution, and the
/// Compaction Relay stay owned by CodeWhale's system prompt assembly.
#[non_exhaustive]
#[derive(Debug)]
pub struct StaticPromptCtx<'a> {
    /// Active model identifier after caller-side routing.
    pub model_id: &'a str,
    /// Personality overlay requested for the base static prompt.
    pub personality: Personality,
    /// Default base/personality prompt layers that would be used without an
    /// override.
    pub default_layers: &'a str,
}

/// Embedder hook for replacing CodeWhale's byte-stable base/personality prompt
/// segment.
pub type StaticPromptComposer = dyn Fn(&StaticPromptCtx<'_>) -> String + Send + Sync + 'static;

/// Replace `BASE_PROMPT` for all subsequent prompt composition. First call
/// wins; later calls return the rejected string. Set before spawning any
/// engine.
pub fn set_base_prompt_override(s: String) -> Result<(), String> {
    set_prompt_override(&BASE_PROMPT_OVERRIDE, s)
}

/// Replace the Simplified-Chinese locale preamble (`## 语言要求`).
pub fn set_locale_preamble_zh_hans_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_PREAMBLE_ZH_HANS_OVERRIDE, s)
}

/// Replace the Simplified-Chinese locale closer (`## 语言再次提醒`).
pub fn set_locale_closer_zh_hans_override(s: String) -> Result<(), String> {
    set_prompt_override(&LOCALE_CLOSER_ZH_HANS_OVERRIDE, s)
}

/// Replace the trailing `## Authority Recap` block.
pub fn set_authority_recap_override(s: String) -> Result<(), String> {
    set_prompt_override(&AUTHORITY_RECAP_OVERRIDE, s)
}

/// Replace the byte-stable base/personality prompt segment for subsequent
/// prompt composition. First call wins; later calls return the rejected
/// composer so embedders can preserve ownership.
pub fn set_static_prompt_composer_override(
    f: Box<StaticPromptComposer>,
) -> Result<(), Box<StaticPromptComposer>> {
    set_static_prompt_composer(&STATIC_PROMPT_COMPOSER, f)
}

// ── Config-directory prompt overrides (issue #3638) ──
// Bridge the embedder override hooks above to a user-facing source: an
// optional file in the CodeWhale config directory. This lets users repurpose
// the TUI for non-software use cases (e.g. long-form writing) by swapping the
// constitutional base prompt, without editing in-tree files or shipping a
// custom embedder build.
//
// Scope is deliberately narrow: only the byte-stable base prompt segment is
// user-overridable. Mode deltas, approval policy, tool taxonomy, Core
// Execution, and the Compaction Relay stay owned by the runtime assembly (see
// `StaticPromptCtx`), so an override cannot strip safety-relevant guidance.
// A missing or empty file is a no-op — the bundled constant is used — so this
// is fully backward compatible.
//
// Because replacing the base prompt is a trust-boundary action (per maintainer
// review on #3638), the override file alone is NOT sufficient: the user must
// also set an explicit opt-in flag (`CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE`).
// This keeps replacing the global Constitution a deliberate, auditable act
// rather than something a stray file can do.

/// Relative path, under the config directory, of the optional base-prompt
/// (constitution) override file.
pub const CONSTITUTION_OVERRIDE_FILE: &str = "prompts/constitution.md";

/// Env flag that must be set (`1`/`true`/`on`/`yes`) to enable config-dir base
/// prompt overrides. Required in addition to the override file so the global
/// base prompt can never be replaced by file presence alone.
pub const BASE_PROMPT_OVERRIDE_OPT_IN_ENV: &str = "CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE";

/// Whether the user has explicitly opted in to base-prompt overrides.
pub fn base_prompt_override_opt_in() -> bool {
    match std::env::var(BASE_PROMPT_OVERRIDE_OPT_IN_ENV) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ),
        Err(_) => false,
    }
}

/// Read an optional prompt-override file rooted at `config_dir`.
///
/// Returns the file contents when it exists and is non-empty after trimming;
/// otherwise `None` so the caller falls back to the embedded default. Pure
/// over `config_dir`, so it is unit-testable without touching the global
/// override cells.
fn read_prompt_override_file(config_dir: &Path, relative: &str) -> Option<String> {
    let path = config_dir.join(relative);
    let raw = std::fs::read_to_string(&path).ok()?;
    if raw.trim().is_empty() {
        tracing::warn!(
            target: "prompts",
            "ignoring empty prompt override file {}",
            path.display(),
        );
        return None;
    }
    tracing::info!(
        target: "prompts",
        "loaded prompt override from {}",
        path.display(),
    );
    Some(raw)
}

fn push_prompt_override_notice(message: String) {
    if let Ok(mut notices) = PROMPT_OVERRIDE_NOTICES.lock() {
        notices.push(message);
    }
}

pub fn take_prompt_override_notices() -> Vec<String> {
    PROMPT_OVERRIDE_NOTICES
        .lock()
        .map(|mut notices| std::mem::take(&mut *notices))
        .unwrap_or_default()
}

/// Load user prompt overrides from `config_dir` and install them through the
/// existing override hooks. Returns the names of the overrides that were
/// applied (for logging/diagnostics).
///
/// Call once at startup, before any engine spawns, because the underlying
/// override cells are first-call-wins. Missing files are a no-op, preserving
/// the bundled defaults.
pub fn load_config_dir_prompt_overrides(config_dir: &Path) -> Vec<&'static str> {
    let mut applied = Vec::new();
    if let Some(text) = read_prompt_override_file(config_dir, CONSTITUTION_OVERRIDE_FILE) {
        if !base_prompt_override_opt_in() {
            // A file exists but the user hasn't opted in. Don't silently
            // replace the base prompt — surface the gate instead.
            let warning = format!(
                "Custom Constitution override found at {}/{} but {} is not set; using the bundled Constitution. Set {}=1 to opt in.",
                config_dir.display(),
                CONSTITUTION_OVERRIDE_FILE,
                BASE_PROMPT_OVERRIDE_OPT_IN_ENV,
                BASE_PROMPT_OVERRIDE_OPT_IN_ENV,
            );
            tracing::warn!(
                target: "prompts",
                "{warning}",
            );
            push_prompt_override_notice(warning);
        } else if set_base_prompt_override(text).is_ok() {
            applied.push("constitution");
        }
    }
    applied
}

/// Resolve the CodeWhale config directory and load any prompt overrides found
/// there. Convenience wrapper around [`load_config_dir_prompt_overrides`] for
/// startup wiring; silently does nothing when the config home cannot be
/// resolved.
pub fn load_prompt_overrides_from_config_home() {
    let Ok(home) = codewhale_config::codewhale_home() else {
        return;
    };
    let applied = load_config_dir_prompt_overrides(&home);
    if !applied.is_empty() {
        tracing::info!(
            target: "prompts",
            "applied {} config-directory prompt override(s): {}",
            applied.len(),
            applied.join(", "),
        );
    }
}

fn set_prompt_override(cell: &std::sync::OnceLock<String>, s: String) -> Result<(), String> {
    cell.set(s)
}

fn set_static_prompt_composer(
    cell: &std::sync::OnceLock<Box<StaticPromptComposer>>,
    f: Box<StaticPromptComposer>,
) -> Result<(), Box<StaticPromptComposer>> {
    cell.set(f)
}

fn effective_prompt_override<'a>(
    cell: &'a std::sync::OnceLock<String>,
    fallback: &'static str,
) -> &'a str {
    cell.get().map(String::as_str).unwrap_or(fallback)
}

fn effective_base_prompt() -> &'static str {
    effective_prompt_override(&BASE_PROMPT_OVERRIDE, BASE_PROMPT)
}

fn effective_static_prompt_composer() -> Option<&'static StaticPromptComposer> {
    STATIC_PROMPT_COMPOSER.get().map(Box::as_ref)
}

fn effective_locale_preamble_zh_hans() -> &'static str {
    effective_prompt_override(&LOCALE_PREAMBLE_ZH_HANS_OVERRIDE, LOCALE_PREAMBLE_ZH_HANS)
}

fn effective_locale_closer_zh_hans() -> &'static str {
    effective_prompt_override(&LOCALE_CLOSER_ZH_HANS_OVERRIDE, LOCALE_CLOSER_ZH_HANS)
}

fn effective_authority_recap() -> &'static str {
    effective_prompt_override(&AUTHORITY_RECAP_OVERRIDE, AUTHORITY_RECAP)
}

/// Cache-stable Simplified-Chinese reinforcement at the start of the prompt.
fn simplified_chinese_reinforcement_preamble() -> &'static str {
    effective_locale_preamble_zh_hans()
}

/// Volatile Simplified-Chinese reinforcement closest to the next user turn.
fn simplified_chinese_reinforcement_closer() -> &'static str {
    effective_locale_closer_zh_hans()
}

const LOCALE_PREAMBLE_ZH_HANS: &str = "## 语言要求\n\n\
这是 DeepSeek 专用的简体中文产品。所有自然语言内容，包括 \
`reasoning_content`、最终回复、解释、总结和面向人的代码注释，都必须使用简体中文。\
代码、文件路径、标识符、工具名（例如 `read_file`、`exec_shell`）、JSON Schema 与 API \
字段、模型 ID、环境变量、命令行参数、URL、diff、stdout/stderr 和原始日志保持原样。";

// ── Closing bookends (appended to the very end of the system prompt) ──

const LOCALE_CLOSER_ZH_HANS: &str = "## 语言再次提醒\n\n\
**重要：你的 `reasoning_content`（内部思考）和最终回复必须保持简体中文。** \
无论你在这次会话中读到了多少英文代码、错误日志或文档，无论项目上下文 \
是英文，思考和回答都不能漂移到其他自然语言。机器协议、代码和原始技术输出继续保持原样。";

/// Personality overlays — voice and tone.
pub const CALM_PERSONALITY: &str = include_str!("prompts/personalities/calm.md");
pub const PLAYFUL_PERSONALITY: &str = include_str!("prompts/personalities/playful.md");

/// Mode deltas — permissions, workflow expectations, mode-specific rules.
pub const AGENT_MODE: &str = include_str!("prompts/modes/agent.md");
pub const PLAN_MODE: &str = include_str!("prompts/modes/plan.md");
pub const YOLO_MODE: &str = include_str!("prompts/modes/yolo.md");
pub const OPERATE_MODE: &str = include_str!("prompts/modes/operate.md");

/// Approval-policy overlays — whether tool calls are auto-approved,
/// require confirmation, or are blocked.
pub const AUTO_APPROVAL: &str = include_str!("prompts/approvals/auto.md");
pub const SUGGEST_APPROVAL: &str = include_str!("prompts/approvals/suggest.md");
pub const NEVER_APPROVAL: &str = include_str!("prompts/approvals/never.md");

/// Shell policy guidance for `allow_shell=false`. Referenced from the
/// Runtime Policy Reference so the model can adapt without mutating the
/// static system-prompt prefix (preserves DeepSeek prefix cache across
/// shell-access toggles).
pub const SHELL_POLICY_DISABLED: &str = "Shell tools unavailable. For mandatory-use items referencing \
`exec_shell`, use `code_execution` (Python sandbox). For GitHub triage, use \
`github_issue_context` / `github_pr_context` as primary route.";

/// Compaction relay template — written into the system prompt so the
/// model knows the format to use when writing `.codewhale/handoff.md`.
pub const COMPACT_TEMPLATE: &str = include_str!("prompts/compact.md");

/// Goal continuation audit template — injected by the engine when a runtime
/// goal is active and the assistant tries to end a turn without closing it.
pub const GOAL_CONTINUATION_PROMPT: &str = include_str!("prompts/continuation.md");

/// Memory hygiene guidance — appended to the system prompt only when the
/// session has a non-empty user-memory block. Steers the model toward
/// writing durable memories as declarative facts ("User prefers concise
/// responses") rather than imperatives ("Always respond concisely"),
/// because imperatives get re-read as directives in later sessions and
/// can override the user's current request (#725).
pub const MEMORY_GUIDANCE: &str = include_str!("prompts/memory_guidance.md");

/// Lean execution layer shared by the default agent runtime. Product/UI
/// tutorials remain outside the model-facing coding contract.
pub const CORE_EXECUTION_PROFILE_PROMPT: &str = include_str!("prompts/core_execution.md");

// ── Legacy prompt constants (kept for backwards compatibility) ────────

/// Legacy base prompt (agent.txt — now decomposed into constitution.md + overlays).
/// Still available for callers that haven't migrated to the layered API.
pub const AGENT_PROMPT: &str = include_str!("prompts/agent.txt");

// ── Personality selection ─────────────────────────────────────────────

/// Which personality overlay to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    /// Cool, spatial, reserved — the default.
    Calm,
    /// Warm, energetic, playful — alternative for fun mode.
    Playful,
}

impl Personality {
    /// Resolve from the `calm_mode` settings flag.
    /// When `calm_mode` is true → Calm; when false → Playful (future).
    /// For now, always returns Calm — Playful is wired but opt-in.
    #[must_use]
    pub fn from_settings(calm_mode: bool) -> Self {
        if calm_mode {
            Self::Calm
        } else {
            // Future: when playful mode is exposed in settings, return Playful here.
            // For now, calm is the only default.
            Self::Calm
        }
    }
}

// ── Composition ───────────────────────────────────────────────────────

/// Compose the full system prompt in deterministic order:
///   1. tool taxonomy  — compact hints generated from the eager core tools
///   2. constitution.md — core identity, toolbox, execution contract
///   3. personality    — voice and tone overlay
///   4. mode delta     — mode-specific permissions and workflow
///   5. approval policy — tool-approval behavior
///
/// Each layer is separated by a blank line for readability in the
/// rendered prompt (the model sees them as contiguous sections).
/// Substitute the model id for embedder-supplied prompt overrides that still
/// template it. The bundled constitution is deliberately model-agnostic and
/// carries no model-fact placeholders.
fn apply_model_template(
    prompt: &str,
    model_id: &str,
    _context_window_override: Option<u32>,
) -> String {
    prompt.replace("{model_id}", model_id)
}

/// Authority recap block — appended at the end of the system prompt,
/// just before the user's first message. Uses recency bias constructively:
/// this is the last thing the model reads before generating, so it
/// reinforces the Constitutional hierarchy without occupying cache-stable
/// prefix space.
const AUTHORITY_RECAP: &str = "\
## Authority Recap

CodeWhale's constitution governs your behavior. Ground truth underlies the
whole list: the user may override a fact, but no one may invent one. When
guidance conflicts, the user's request this turn outranks this constitution,
which outranks nearest-scope project law and instructions, which outrank
standing user-global preferences, which outrank memory and previous-session
handoffs. When in doubt, consult ### Whose word wins.";

pub fn compose_prompt(personality: Personality) -> String {
    compose_prompt_with_approval_model_and_shell(personality, "codewhale")
}

pub fn compose_prompt_with_approval_model_and_shell(
    personality: Personality,
    model_id: &str,
) -> String {
    let default_layers = compose_default_static_layers(personality, model_id);
    apply_static_prompt_composer(
        effective_static_prompt_composer(),
        personality,
        model_id,
        &default_layers,
    )
}

fn compose_default_static_layers(_personality: Personality, model_id: &str) -> String {
    // Personality is folded into the constitutional preamble/articles — no
    // separate overlay is appended. Language and output rules are split into
    // their own static segments so the 0.9.0 constitution stays compact.
    let layers = format!(
        "{}\n\n{}\n\n{}",
        effective_base_prompt().trim(),
        LANGUAGE_PROMPT.trim(),
        OUTPUT_PROMPT.trim()
    );
    apply_model_template(&layers, model_id, None)
}

fn apply_static_prompt_composer(
    composer: Option<&StaticPromptComposer>,
    personality: Personality,
    model_id: &str,
    default_layers: &str,
) -> String {
    match composer {
        Some(composer) => composer(&StaticPromptCtx {
            model_id,
            personality,
            default_layers,
        }),
        None => default_layers.to_string(),
    }
}

// The full base prompt is always used; effective tool availability is enforced
// by the tool catalog and execution layer rather than by mutating message[0].

// ── Public API ────────────────────────────────────────────────────────

/// Get the system prompt for a specific mode with project context.
pub fn system_prompt_for_mode_with_context(
    workspace: &Path,
    working_set_summary: Option<&str>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_and_skills(workspace, working_set_summary, None, None, None)
}

/// Get the system prompt for a specific mode with project and skills context.
///
/// **Volatile-content-last invariant.** Blocks are appended in order from
/// most-static to most-volatile so DeepSeek's KV prefix cache hits the
/// longest possible byte prefix turn-over-turn:
///
///   1. mode prompt (compile-time constant)
///   2. project context / fallback (workspace-static)
///   3. skills block (skills-dir-static)
///   4. `## Core Execution` (compile-time constant)
///   5. compaction relay template (compile-time constant)
///   6. relay block — file-backed; rewritten by `/compact` and on exit
///
/// Anything appended after a volatile block forfeits the cache for the rest
/// of the request. New blocks belong above the relay boundary unless they
/// themselves are turn-volatile. Working-set metadata is now injected into the
/// latest user message as per-turn metadata instead of this system prompt.
pub fn system_prompt_for_mode_with_context_and_skills(
    workspace: &Path,
    working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    user_memory_block: Option<&str>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_skills_and_session(
        workspace,
        working_set_summary,
        skills_dir,
        instructions,
        PromptSessionContext {
            user_memory_block,
            goal_objective: None,
            project_context_pack_enabled: true,
            model_id: "codewhale",
            context_window_override: None,
            show_thinking: true,
            verbosity: None,
            skills_scan_codewhale_only: false,
            shell_binary: "sh",
        },
    )
}

pub fn system_prompt_for_mode_with_context_skills_and_session(
    workspace: &Path,
    _working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    session_context: PromptSessionContext<'_>,
) -> SystemPrompt {
    system_prompt_for_mode_with_context_skills_session_and_approval(
        workspace,
        _working_set_summary,
        skills_dir,
        instructions,
        session_context,
    )
}

pub fn system_prompt_for_mode_with_context_skills_session_and_approval(
    workspace: &Path,
    _working_set_summary: Option<&str>,
    skills_dir: Option<&Path>,
    instructions: Option<&[InstructionSource]>,
    session_context: PromptSessionContext<'_>,
) -> SystemPrompt {
    let default_layers = apply_model_template(
        effective_base_prompt().trim(),
        session_context.model_id,
        session_context.context_window_override,
    );
    let mode_prompt = apply_static_prompt_composer(
        effective_static_prompt_composer(),
        Personality::Calm,
        session_context.model_id,
        &default_layers,
    );

    // Load project context from workspace
    let project_context = load_project_context_with_parents(workspace);

    // 1–2. Mode prompt + project context.
    // `load_project_context_with_parents` generates an in-memory bounded
    // overview when no context file exists, so the fallback should usually be
    // available without writing project-local files.
    let mut full_prompt = if let Some(project_block) = project_context.as_system_block() {
        format!("{mode_prompt}\n\n{project_block}")
    } else {
        // Extremely unlikely: context generation failed (e.g. filesystem error).
        // Use mode prompt alone rather than panic.
        tracing::warn!("No project context available and auto-generation failed");
        mode_prompt
    };

    full_prompt = format!(
        "{}\n\n{full_prompt}",
        simplified_chinese_reinforcement_preamble()
    );

    if let Some(user_constitution_block) = load_user_constitution_block() {
        full_prompt = format!("{full_prompt}\n\n{user_constitution_block}");
    }

    if session_context.project_context_pack_enabled
        && let Some(pack) = crate::project_context::generate_project_context_pack(workspace)
    {
        full_prompt = format!("{full_prompt}\n\n{pack}");
    }

    if is_concise_verbosity(session_context.verbosity) {
        full_prompt = format!(
            "{full_prompt}\n\n{}",
            concise_output_discipline_instruction()
        );
    }

    // 3. Skills block. #432: default discovery walks every compatible
    // workspace/global skill directory so skills installed for other AI-tool
    // conventions show up in the catalogue. Users can opt into a CodeWhale-only
    // scan with `[skills] scan_codewhale_only = true`. When an explicit
    // `skills_dir` is configured, union it with the workspace view instead of
    // treating it as a fallback; the workspace view often returns Some and
    // would otherwise shadow the configured directory entirely.
    let skill_discovery_mode = crate::skills::SkillDiscoveryMode::from_codewhale_only(
        session_context.skills_scan_codewhale_only,
    );
    let skills_block = match skills_dir {
        Some(dir) => {
            crate::skills::render_available_skills_context_for_workspace_and_dir_with_mode(
                workspace,
                dir,
                skill_discovery_mode,
                "zh-Hans",
            )
        }
        None => crate::skills::render_available_skills_context_for_workspace_with_mode(
            workspace,
            skill_discovery_mode,
            "zh-Hans",
        ),
    };
    if let Some(block) = skills_block {
        full_prompt = format!("{full_prompt}\n\n{block}");
    }

    // 4. Lean, runtime-only coding discipline. Context pressure, prompt-cache
    // accounting, footer presentation, and automatic compaction are host
    // responsibilities; teaching their UI to the model dilutes the task.
    full_prompt.push_str("\n\n");
    full_prompt.push_str(CORE_EXECUTION_PROFILE_PROMPT.trim());

    // 5. Compaction relay template — so the model knows the format to use
    //    when writing `.codewhale/handoff.md` on exit / `/compact`.
    full_prompt.push_str("\n\n");
    full_prompt.push_str(COMPACT_TEMPLATE);

    // ── Volatile-content boundary → WorldState fragments ──────────────────
    // Constitution (`full_prompt`) stays the cache-stable Blocks[0] prefix.
    // Everything below drifts mid-session and is assembled as marked
    // WorldState fragments so an env/memory/goal/handoff change can
    // `render_diff` without rebuilding unrelated material.

    // Workspace fragment: environment + mid-session memory/goal facts.
    let mut workspace_parts = vec![render_environment_block(
        workspace,
        session_context.shell_binary,
    )];
    if let Some(memory_block) = session_context.user_memory_block
        && !memory_block.trim().is_empty()
    {
        workspace_parts.push(format!("{memory_block}\n\n{MEMORY_GUIDANCE}"));
    }
    if let Some(goal_objective) = session_context.goal_objective
        && !goal_objective.trim().is_empty()
    {
        workspace_parts.push(format!(
            "## Current Goal\n\n<session_goal>\n{}\n</session_goal>",
            goal_objective.trim()
        ));
    }
    let workspace_body = workspace_parts.join("\n\n");

    // Permissions fragment: configured `instructions = [...]` files (#454).
    let permissions_body = instructions.and_then(render_instructions_block);

    // Route fragment: active model, verbosity, and thinking projection.
    let route_body = render_route_fragment(&session_context);

    // Token-budget / continuity fragment: prior-session handoff relay.
    let token_budget_body = load_handoff_block(workspace);

    let world_state = world_state_from_session_facts(
        Some(workspace_body.as_str()),
        permissions_body.as_deref(),
        Some(route_body.as_str()),
        None, // AgentTopology is updated by runtime callers when available.
        None, // Skills stay in the constitution prefix (skills-dir-static).
        token_budget_body.as_deref(),
    );

    let mut blocks = crate::model_context::WorldStateSnapshot {
        constitution: full_prompt,
        world_state,
    }
    .to_system_blocks();

    // Trailers keep recency bias after WorldState: authority, then language.
    blocks.push(SystemBlock {
        text: effective_authority_recap().trim().to_string(),
        cache_control: PromptCacheControl::Volatile,
    });
    blocks.push(SystemBlock {
        text: simplified_chinese_reinforcement_closer().trim().to_string(),
        cache_control: PromptCacheControl::Volatile,
    });

    SystemPrompt { blocks }
}

/// Flatten a system prompt to joined text (tests + debug inspectors).
#[must_use]
pub fn system_prompt_flat_text(prompt: &SystemPrompt) -> String {
    prompt
        .blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_route_fragment(session_context: &PromptSessionContext<'_>) -> String {
    let verbosity = session_context
        .verbosity
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    format!(
        "model: {}\nverbosity: {}\nshow_thinking: {}",
        session_context.model_id.trim(),
        verbosity,
        if session_context.show_thinking {
            "on"
        } else {
            "off"
        },
    )
}

/// Assemble a cache-stable constitution prefix with a typed WorldState layer.
///
/// This is the Codex-parity assembly point: constitution stays byte-stable for
/// prefix caching; volatile concerns live in `WorldState` fragments with
/// markers, caps, and `render_diff` retain-unchanged behavior. Callers that
/// still need a flat string can use [`WorldStateSnapshot::render_text`].
pub fn system_prompt_with_world_state(
    constitution: impl Into<String>,
    world_state: crate::model_context::WorldState,
) -> SystemPrompt {
    let snapshot = crate::model_context::WorldStateSnapshot {
        constitution: constitution.into(),
        world_state,
    };
    SystemPrompt {
        blocks: snapshot.to_system_blocks(),
    }
}

/// Build a WorldState from the common volatile session facts.
///
/// Does not load constitution — callers keep that as the stable base.
pub fn world_state_from_session_facts(
    workspace_body: Option<&str>,
    permissions_body: Option<&str>,
    route_body: Option<&str>,
    agent_topology_body: Option<&str>,
    skills_tools_body: Option<&str>,
    token_budget_body: Option<&str>,
) -> crate::model_context::WorldState {
    let mut state = crate::model_context::WorldState::new();
    if let Some(body) = workspace_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_workspace(body);
    }
    if let Some(body) = permissions_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_permissions(body);
    }
    if let Some(body) = route_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_route(body);
    }
    if let Some(body) = agent_topology_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_agent_topology(body);
    }
    if let Some(body) = skills_tools_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_skills_tools(body);
    }
    if let Some(body) = token_budget_body.filter(|s| !s.trim().is_empty()) {
        state = state.with_token_budget(body);
    }
    state
}

/// Build a system prompt with explicit project context
pub fn build_system_prompt(base: &str, project_context: Option<&ProjectContext>) -> SystemPrompt {
    let full_prompt =
        match project_context.and_then(super::project_context::ProjectContext::as_system_block) {
            Some(project_block) => format!("{}\n\n{}", base.trim(), project_block),
            None => base.trim().to_string(),
        };
    SystemPrompt::from_text(full_prompt)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;
    use sha2::{Digest, Sha256};

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: this crate has one environment-mutating test and does
            // not start threads while the scoped fixture is active.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: see `EnvGuard::set`; restoration happens before the
            // single test exits.
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn normalized_fixture_text(text: &str) -> String {
        text.replace(
            &format!("- platform: {}", std::env::consts::OS),
            "- platform: <fixture-os>",
        )
    }

    #[test]
    fn production_prompt_fixture_freezes_body_order_and_cache_boundary() {
        let fixture = std::env::temp_dir().join("codewhale-context-production-prompt-fixture");
        let _ = fs::remove_dir_all(&fixture);
        let home = fixture.join("home");
        let workspace = fixture.join("workspace");
        fs::create_dir_all(home.join(".codewhale/skills")).expect("home skills");
        fs::create_dir_all(workspace.join(".codewhale/skills/frozen-skill"))
            .expect("workspace skill");
        fs::create_dir_all(workspace.join("src")).expect("source dir");
        fs::write(
            workspace.join("AGENTS.md"),
            "# Fixture law\n\nKeep the frozen prompt byte-identical.\n",
        )
        .expect("AGENTS.md");
        fs::write(
            workspace.join("README.md"),
            "# Frozen prompt fixture\n\nA deterministic project context pack.\n",
        )
        .expect("README");
        fs::write(workspace.join("src/lib.rs"), "pub fn fixture() {}\n").expect("source");
        fs::write(
            workspace.join(".codewhale/skills/frozen-skill/SKILL.md"),
            "---\nname: frozen-skill\ndescription: Deterministic fixture skill\ndescription_zh: 确定性测试技能\n---\nUse it.\n",
        )
        .expect("skill");
        fs::create_dir_all(workspace.join(".codewhale")).expect("codewhale dir");
        fs::write(
            workspace.join(HANDOFF_RELATIVE_PATH),
            "# Relay\n\nContinue the frozen fixture.\n",
        )
        .expect("handoff");
        let configured = workspace.join("configured.md");
        fs::write(&configured, "Configured instruction body.\n").expect("configured instruction");

        let _home = EnvGuard::set("HOME", &home);
        let _codewhale_home = EnvGuard::set("CODEWHALE_HOME", &home.join(".codewhale"));
        let preferences = PromptPreferences {
            show_thinking: false,
        };
        let instructions = vec![
            InstructionSource::File(configured),
            InstructionSource::Inline {
                name: "cli:append-system-prompt".to_owned(),
                content: "CLI appended instruction body.".to_owned(),
            },
        ];

        let prompt = production_system_prompt(ProductionPromptRequest {
            workspace: &workspace,
            model: "deepseek-v4-pro",
            preferences: &preferences,
            instructions: &instructions,
            skills_dir: Some(&workspace.join(".codewhale/skills")),
            project_context_pack_enabled: true,
            verbosity: Some("concise"),
            skills_scan_codewhale_only: true,
            shell_binary: "/fixture/bin/zsh",
            tool_mode: true,
        });

        assert_eq!(prompt.blocks.len(), 8);
        assert_eq!(prompt.blocks[0].cache_control, PromptCacheControl::Stable);
        assert!(
            prompt
                .blocks
                .iter()
                .skip(1)
                .all(|block| block.cache_control == PromptCacheControl::Volatile)
        );
        assert!(prompt.blocks[0].text.contains("# Fixture law"));
        assert!(prompt.blocks[0].text.contains("确定性测试技能"));
        assert!(prompt.blocks[1].text.contains("/fixture/bin/zsh"));
        assert!(prompt.blocks[1].text.contains("- lang: zh-Hans"));
        assert!(
            prompt.blocks[2]
                .text
                .contains("Configured instruction body.")
        );
        assert!(
            prompt.blocks[2]
                .text
                .contains("CLI appended instruction body.")
        );
        assert!(prompt.blocks[3].text.contains("model: deepseek-v4-pro"));
        assert!(!prompt.blocks[3].text.contains("translation:"));
        assert!(
            prompt.blocks[4]
                .text
                .contains("Continue the frozen fixture.")
        );
        assert!(prompt.blocks[5].text.starts_with("## Authority Recap"));
        assert!(prompt.blocks[6].text.starts_with("## 语言再次提醒"));
        assert!(prompt.blocks[7].text.starts_with("你正在唯一 AgentRuntime"));

        let block_hashes = prompt
            .blocks
            .iter()
            .map(|block| sha256(normalized_fixture_text(&block.text).as_bytes()))
            .collect::<Vec<_>>();
        assert_eq!(
            block_hashes,
            [
                "71cc614e4294958574dbd94e38037c2fa8c6440c3ab5b31ea299ad992590a55c",
                "657021acf824946ccfaf5f7445fe894c19b4631b55a52f1bb63885231b5d7964",
                "6fea08828ee251fd8682f0987f4beaf95bd9dc28a4f4dd80606709b0af41099f",
                "50f497cd9e457dacbe0e0b8ce8166bcaa7da57a705a5b3b781b2623a5a21dd00",
                "4c80b2b5e829efd024a0d6665b949a58a70e33909d3b544dd2d0f4c5e7ae6c14",
                "525923116dacb0c1014dfc9ea028e69391ec011c2bf4d8ebddd994cefedea3e8",
                "72f16fe4c56ba9cc163d75a3f9c063e85ebfbb86e93bc9f484ea1b8287dd1345",
                "c31d4de13a28a0ba5d9a01b0c09b37e8336ed6db67bf12d1b0b9adc6e72c87b5",
            ]
        );
        let normalized_prompt = prompt
            .blocks
            .iter()
            .map(|block| {
                format!(
                    "{:?}\0{}",
                    block.cache_control,
                    normalized_fixture_text(&block.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\0\0");
        assert_eq!(
            sha256(normalized_prompt.as_bytes()),
            "9c276031c2ead1c1bfc9295c0818817a55e8c28a187665722179936093641b73"
        );
        fs::remove_dir_all(&fixture).expect("remove fixture");
    }
}
