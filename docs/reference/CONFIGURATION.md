# Configuration

> Category: current implementation reference; obsolete product surfaces are
> removed as their callers are migrated.

codewhale reads configuration from a TOML file plus environment variables.
At process startup it also loads a workspace-local `.env` file when present.
Use the tracked `.env.example` as the template; copy it to `.env`, then edit
only the provider and safety knobs you need.

## Constitution, project instructions, and repo authority

CodeWhale has several instruction surfaces. They are deliberately separate so a
personal constitution, repo policy, project instructions, and runtime security
controls do not blur together.

- **Bundled global Constitution** — the compiled base law in the binary. It is
  the default floor for every session.
- **User-global constitution** — an existing structured file at
  `$CODEWHALE_HOME/constitution.json` (default `~/.codewhale/constitution.json`).
  CodeWhale renders an enabled file into a separate
  `<codewhale_user_constitution>` prose block.
  This can express preferences and stop conditions, but it does not change
  runtime approval policy, sandbox, shell, network, trust, or MCP permissions.
- **Repo-local constitution** — optional project policy in
  `.codewhale/constitution.json`, described below.
- **`AGENTS.md`** — cross-agent **project instructions** (prose). This is the
  canonical file for "how should an agent work in this repo." Run `/init` to
  scaffold one. `CLAUDE.md` and `.claude/instructions.md` are read as
  compatibility fallbacks.
- **Memory and handoffs** — recalled state. Useful, but lower authority than
  constitutions and project instructions.

Historical setup verification is archived in
[`docs/evidence/v0867-constitution-setup-qa-matrix.md`](../evidence/v0867-constitution-setup-qa-matrix.md).
It does not describe the current canonical command surface.

### User-global constitution

On first launch CodeWhale uses a fixed Simplified Chinese onboarding path:
Welcome → API-key gate (when required) → workspace-trust gate (when required)
→ tips. There is no language screen or runtime language choice. The canonical
command surface does not expose `/setup` or `/constitution`; the retired TUI
setup wizard has been physically removed. Existing `constitution.json` and
`setup_state.json` files are not deleted, and the prompt context loader still
honors an enabled user constitution. Edit or remove those files explicitly if
you choose to keep using this legacy instruction layer.

Each repo can carry two distinct, complementary files:

- **`AGENTS.md`** — ordinary project working instructions.
- **`.codewhale/constitution.json`** — CodeWhale-specific **repo authority /
  prioritization policy**: when local sources conflict, which should CodeWhale
  trust first, and what to verify before claiming a task is done. `.codewhale/`
  lives inside the repo (like `.github/`). Example:

  ```json
  {
    "schema_version": 1,
    "authority": [
      "current user request",
      "live code and tests",
      "GitHub issue/PR details",
      "AGENTS.md",
      "memory",
      "old handoffs"
    ],
    "protected_invariants": [
      "do not break old-session transcript replay"
    ],
    "branch_policy": "PRs target the integration branch, not main",
    "verification_policy": {
      "before_claiming_done": ["run focused tests", "read changed files back"]
    },
    "escalate_when": [
      "a destructive action was not explicitly authorized"
    ]
  }
  ```

  All fields are optional. When present, the file is rendered into the system
  prompt as concise prose in a higher-authority block. Legacy `WHALE.md` files
  are ignored and reported as migration-only diagnostics.

  Each `protected_invariants` entry may be either a plain string (advisory
  prose, the historical shape) or an object carrying path globs, which is
  additionally **mechanically enforced** in the tool gate. See
  [Enforced repo-law invariants](#enforced-repo-law-invariants) below.

  This is the **repo-local law** layer in CodeWhale's hierarchy: *bundled global
  Constitution* → *user-global constitution* (`$CODEWHALE_HOME/constitution.json`,
  rendered as prose) → *repo constitution* (`.codewhale/constitution.json`, this
  file) → *AGENTS/project instructions* → *memory and handoffs* → *current
  request and live evidence for the active turn*. Runtime policy
  (permissions/sandbox/cost limits enforced in code) is separate from all of
  these prompt layers. The repo constitution gives project decision rules; it
  does not replace the bundled Constitution, the user-global constitution, or
  the current user request.

> **`WHALE.md` is deprecated.** It overlapped confusingly with `AGENTS.md`.
> CodeWhale no longer reads `WHALE.md` as project or global context. If one is
> present, it is ignored; the retired manual context-report command is not a
> supported migration surface. Move ordinary instructions to `AGENTS.md` and
> CodeWhale-specific authority policy to `.codewhale/constitution.json`.
> Personal standing guidance can remain in
> `$CODEWHALE_HOME/constitution.json`. (The global
> CodeWhale Constitution shipped in the model prompt is a separate thing and is
> unaffected.)

### Enforced repo-law invariants

By default a `protected_invariants` entry is advisory prose: it is rendered into
the prompt as guidance the agent should honor, but nothing stops a write. An
entry written as an **object with `paths`** is different — it compiles into a
mechanical write hold that the engine's tool gate evaluates before the write
runs. The law becomes mechanism, not just a request.

An enforced entry has this shape:

```json
{
  "schema_version": 1,
  "protected_invariants": [
    "Keep DeepSeek support first-class.",
    {
      "text": "The wire format is frozen; protocol changes need a human.",
      "paths": ["crates/protocol/**"],
      "action": "block"
    },
    {
      "text": "Release notes need human review.",
      "paths": ["CHANGELOG.md"],
      "action": "ask"
    }
  ]
}
```

- `text` — required. The reason surfaced on the hold. An empty `text` is skipped.
- `paths` — workspace-relative globs (globset syntax, e.g. `crates/protocol/**`,
  `**/secrets.toml`, `CHANGELOG.md`). An object with no usable `paths` stays
  advisory-only despite the object shape.
- `action` — optional, defaults to `ask`. `ask` **force-prompts** for approval;
  `block` **denies the write outright**.

Semantics:

- **Tighten-only.** The schema has no allow/widen shape, so law can only *add*
  holds — a crafted constitution can never grant authority or weaken a gate
  above it.
- **Not bypassable by mode.** Like the built-in safety floor, an `ask` hold
  force-prompts in every mode, including Full Access; `block` always denies. Mode
  cannot turn a hold off.
- **Repo-local only.** Only the repo's `.codewhale/constitution.json`
  participates. The user-global constitution stays advisory prose and never
  reaches this mechanism.
- **Fails safe.** A missing file, parse error, or invalid glob degrades to
  fewer or zero rules — never a hold on unprotected paths and never a poisoned
  gate. Across matches the strongest action wins, so `block` outranks `ask`.
- **Leaves a receipt.** Every hold emits a `tool.repo_law_decision` tool-audit
  event naming the invariant, the matched path, and the source file; the
  approval/denial reason names the invariant too.

**Coverage is deliberately limited.** Holds are evaluated only for the write
tools `write_file`, `edit_file`, `apply_patch`, and `fim_edit`, and only
against the filesystem targets named in their inputs (`path`/`target`/
`destination`/`file_path`, `changes[].path`, and unified-diff /
`apply_patch`-envelope headers). A shell command that writes a protected path is **not** held by
repo law — those writes are still governed by the ordinary approval, sandbox,
and shell-write gates, not by this mechanism.

### Expert full base-prompt override (#3638)

The global Constitution (the base system prompt, normally compiled in from
`prompts/constitution.md`) can be replaced per-user without rebuilding. This is
an expert escape hatch, not the normal `/constitution` guided setup output.
Because this is a prompt trust boundary, it takes **two deliberate steps** — a
file alone is not enough:

1. Drop the replacement at `~/.codewhale/prompts/constitution.md` (under
   `$CODEWHALE_HOME` when set).
2. Set the explicit opt-in flag `CODEWHALE_ALLOW_BASE_PROMPT_OVERRIDE=1`
   (`true`/`on`/`yes` also accepted).

If the file exists but the flag is unset, the override is **ignored** (with a
log line pointing to the flag) and the bundled Constitution stays in place.
This is intended for repurposing the TUI beyond software engineering — e.g.
long-form writing or document review — where the engineering-oriented base
prompt is a poor fit. It is loaded once at startup; a **missing or empty file
is a no-op**, so existing installs keep the bundled prompt.

Scope is deliberately narrow: only the byte-stable **base prompt segment** is
overridable. Mode deltas, the approval policy, the tool taxonomy, Context
Management, and the Compaction Relay are still owned by CodeWhale's runtime
assembly, so an override **cannot remove safety-relevant guidance** (sandbox,
approvals) — it only swaps the task/voice framing. To customize ordinary
personal behavior, prefer `/constitution`; to customize per-repo behavior,
prefer `AGENTS.md` + `.codewhale/constitution.json` above.

## Where It Looks

Default config path:

- `~/.codewhale/config.toml`
- Legacy fallback: `~/.deepseek/config.toml`

Overrides:

- CLI: `codewhale --config /path/to/config.toml`
- Env: `CODEWHALE_CONFIG_PATH=/path/to/config.toml`
- Legacy env alias: `DEEPSEEK_CONFIG_PATH=/path/to/config.toml`

If both are set, `--config` wins. Environment variable overrides are applied after the file is loaded.

### 修改配置

当前交互 TUI 不提供配置编辑器。直接编辑 `~/.codewhale/config.toml`；模型路由、
`approval_policy`、`allow_shell`、`base_url`、`mcp_config_path` 和
`[subagents]` 等运行配置应在重启后生效。界面偏好保存在
`~/.codewhale/settings.toml`。

### User workspace entries

Interactive Agent sessions expose shell tools by default with approval gating
unless you explicitly disable them. For a shell opt-in that should live in the
user's global config for noninteractive or durable-task profiles rather than in
the repository, add a workspace-scoped entry:

```toml
[workspace.'/absolute/path/to/project']
allow_shell = true
```

The entry applies only when the launched workspace path matches the table key.
The legacy `[projects."/absolute/path/to/project"]` table is also accepted for
this user-owned override.

In interactive mode, the per-project overlay
`<workspace>/.codewhale/config.toml` is applied after this user entry. A
project-level `allow_shell = false` can still tighten the session; project-level
`allow_shell = true` is ignored.

### Per-project overlay (#485)

When the TUI starts in a workspace that contains a regular-file
`<workspace>/.codewhale/config.toml`, the safe values declared in that file are
merged on top of the global config. Legacy
`<workspace>/.deepseek/config.toml` files are still read when the CodeWhale path
is absent. Symlinked project config files are rejected. This lets a repo suggest
a model or tighten local safety posture without touching the user's
`~/.codewhale/config.toml`. Pass `--no-project-config` to skip the overlay for
one launch.

Supported keys in the project overlay (top-level fields only):

| Key | Effect |
|---|---|
| `model` | override `default_text_model` |
| `reasoning_effort` | force `"high"` / `"max"` for a complex repo |
| `approval_policy` | only values that tighten the user's current approval posture |
| `sandbox_mode` | only values that tighten the user's current sandbox posture |
| `notes_path` | keep notes in-repo |
| `max_subagents` | clamp sub-agent concurrency for a constrained repo (clamped to 1..=20) |
| `allow_shell` | `false` can disable shell access; `true` is ignored |

The overlay is intentionally narrow — it covers the fields a repo
maintainer is most likely to want to standardize across contributors.
Credential, endpoint, provider-selection, MCP config, skills, capacity,
retry, and `instructions = [...]` settings stay user-global.
If a repo-local config declares `api_key`, `base_url`, `provider`,
`mcp_config_path`, `allow_shell = true`, or `instructions`,
CodeWhale ignores that key and keeps the user's global setting.

The `codewhale` facade and `codewhale-tui` binary share the same config file for
DeepSeek auth and model defaults. `codewhale auth set --provider deepseek` (and
the legacy `codewhale login --api-key ...` alias) saves the key to
`~/.codewhale/config.toml` (migrating legacy `~/.deepseek/config.toml` on first
launch when needed), and `codewhale --model deepseek-v4-flash` is forwarded to
the TUI as `DEEPSEEK_MODEL`.

Credential lookup uses `config -> keyring -> env` after any explicit CLI
`--api-key`. Run `codewhale auth status` to inspect the active provider's config
file, OS keyring backend, environment variable, winning source, and last-four
label without printing the key itself. The command only probes the active
provider's keyring entry.

For hosted, generic OpenAI-compatible, self-hosted, OpenAI Responses, or native
Anthropic providers, set `provider = "<id>"` or pass
`codewhale --provider <id>`. The canonical provider IDs are `deepseek`,
`nvidia-nim`, `openai`, `atlascloud`, `wanjie-ark`, `volcengine`,
`openrouter`, `xiaomi-mimo`, `novita`, `fireworks`, `siliconflow`, `arcee`,
`siliconflow-CN`, `moonshot`, `sglang`, `vllm`, `ollama`, `huggingface`,
`together`, `qianfan`, `openai-codex`, `anthropic`, `openmodel`, `zai`,
`stepfun`, `minimax`, and `deepinfra`.
For the provider-by-provider registry, including wire protocol, auth variables,
default base URLs, model IDs, and capability metadata, see
[PROVIDERS.md](PROVIDERS.md).
The facade saves provider credentials to the shared user config and forwards
the resolved key, base URL, provider, and model to the TUI process. Use
`codewhale auth set --provider nvidia-nim --api-key "YOUR_NVIDIA_API_KEY"` or
`codewhale auth set --provider openai --api-key "YOUR_OPENAI_COMPATIBLE_API_KEY"` or
`codewhale auth set --provider atlascloud --api-key "YOUR_ATLASCLOUD_API_KEY"` or
`codewhale auth set --provider wanjie-ark --api-key "YOUR_WANJIE_API_KEY"` or
`codewhale auth set --provider xiaomi-mimo --api-key "YOUR_XIAOMI_KEY"` or
`codewhale auth set --provider fireworks --api-key "YOUR_FIREWORKS_API_KEY"` or
`codewhale auth set --provider siliconflow --api-key "YOUR_SILICONFLOW_API_KEY"` or
`codewhale auth set --provider arcee --api-key "YOUR_ARCEE_API_KEY"` or the
matching provider ID from [PROVIDERS.md](PROVIDERS.md) to save provider keys
through the facade. The generic `openai` provider defaults
to `https://api.openai.com/v1`, accepts `OPENAI_BASE_URL`, and defaults to
`deepseek-v4-pro` for OpenAI-compatible gateways. `atlascloud` defaults to
`https://api.atlascloud.ai/v1`, accepts `ATLASCLOUD_BASE_URL`, and uses
`deepseek-ai/deepseek-v4-flash` as its default model. `wanjie-ark` targets
Wanjie Ark's OpenAI-compatible endpoint at
`https://maas-openapi.wanjiedata.com/api/v1`, defaults to `deepseek-reasoner`,
and passes model IDs through unchanged because Wanjie model access is
account-scoped. SGLang, vLLM, and Ollama are
self-hosted and can run without an API key by default. Ollama defaults to
`http://localhost:11434/v1` and sends model tags such as `codewhale-coder:1.3b`
or `qwen2.5-coder:7b` unchanged. Self-hosted providers and loopback custom
URLs (`localhost`, `127.0.0.1`, `[::1]`, `0.0.0.0`) do not read the secret store
unless API-key auth is explicitly requested; use an env var or config-file key
when a local server does require bearer auth.
SiliconFlow defaults to `https://api.siliconflow.com/v1`, accepts
`SILICONFLOW_BASE_URL`, and uses `deepseek-ai/DeepSeek-V4-Pro` by default.
`provider = "siliconflow-CN"` selects the China regional default
`https://api.siliconflow.cn/v1` with the `[providers.siliconflow_cn]` table and
`SILICONFLOW_API_KEY` credential slot.
Arcee AI defaults to `https://api.arcee.ai/api/v1`, accepts `ARCEE_BASE_URL`,
and uses `trinity-large-thinking` by default for CodeWhale agent work.
`trinity-large-preview` is also listed as a direct Arcee API model; OpenRouter's
`arcee-ai/trinity-large-thinking` remains the OpenRouter namespaced form, while
the direct Arcee provider uses the bare `trinity-large-thinking` ID. Direct
Arcee large-model API calls are tracked as 256K-context BF16 serving; Thinking
is reasoning-capable, while Preview is not marked as a thinking model.

### Custom OpenAI-Compatible Gateways

For a single third-party service that implements the OpenAI Chat Completions
API, the simplest setup is the built-in `openai` provider name pointed at the
gateway:

```toml
provider = "openai"
default_text_model = "your-model-id"

[providers.openai]
api_key = "YOUR_OPENAI_COMPATIBLE_API_KEY"
base_url = "https://your-gateway.example/v1"
```

Put the endpoint under `[providers.openai]`, not the legacy top-level
`base_url`, so the OpenAI-compatible provider receives it. `default_text_model`
is the model ID sent to the gateway; `[providers.openai].model` can be used as
the OpenAI-provider-specific override.

If you keep several OpenAI-compatible gateways, or need a stable name for an
AgentProfile provider pin, define a user-named custom provider table:

```toml
provider = "lm-studio"

[providers.lm-studio]
kind = "openai-compatible"
base_url = "http://127.0.0.1:1234/v1"
api_key = "lm-studio"
model = "qwen-2.5-7b"
```

Custom provider names may be selected with `provider = "<name>"`,
`--provider <name>`, or an AgentProfile `provider = "<name>"` when the matching
`[providers.<name>]` table exists.

StepFun has a first-class provider entry, so keep Coding Plan credentials and
base URL scoped to `[providers.stepfun]`:

```toml
provider = "stepfun"

[providers.stepfun]
api_key = "YOUR_STEPFUN_API_KEY"
base_url = "https://api.stepfun.ai/step_plan/v1"
model = "step-3.7-flash"
```

Alibaba Bailian / Model Studio DashScope Qwen routes use the same OpenAI
provider shape:

```toml
provider = "openai"

[providers.openai]
api_key = "YOUR_DASHSCOPE_API_KEY"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
context_window = 1000000
```

Use the regional DashScope `compatible-mode/v1` base URL that matches the
region of your API key. CodeWhale keeps `qwen-plus` scoped to the `openai`
provider route and does not infer a different provider from the model prefix.
The same rule applies to all provider-prefixed model strings: a prefix such as
`deepseek-ai/...` or `deepseek/...` is a provider-owned wire ID under the
selected provider, not an automatic switch to the DeepSeek provider.
Set `context_window` to the gateway/model's real total context window when it
differs from CodeWhale's static model metadata.

If the gateway accepts `POST /chat/completions` but rejects
`/v1/chat/completions`, set a provider-local `path_suffix`:

```toml
[providers.openai]
base_url = "https://your-gateway.example/v1"
path_suffix = "/chat/completions"
```

The suffix applies only to chat-completion requests. Model listing and
DeepSeek beta paths keep their built-in routing so a generic gateway override
does not accidentally rewrite `/models` or `/beta/completions`.

For private gateways with broken or intercepted certificates, use
`SSL_CERT_FILE` with a trusted CA bundle. The legacy provider-table key
`insecure_skip_tls_verify = true` is still parsed so `codewhale doctor` can
report stale configs, but provider clients reject it instead of disabling TLS
certificate verification.

Local HTTP endpoints such as Ollama, SGLang, and vLLM are allowed by default
when they use localhost or loopback addresses. For a non-local `http://`
gateway, launch with `DEEPSEEK_ALLOW_INSECURE_HTTP=1` only on a trusted network:

```bash
DEEPSEEK_ALLOW_INSECURE_HTTP=1 codewhale
```

Third-party OpenAI-compatible gateways that need extra request headers can set
`http_headers = { "X-Model-Provider-Id" = "your-model-provider" }` at the top
level or under a provider table such as `[providers.deepseek]`. When configured,
codewhale sends those custom headers on model API requests. The equivalent
environment override is `DEEPSEEK_HTTP_HEADERS`, using comma-separated
`name=value` pairs such as
`X-Model-Provider-Id=your-model-provider,X-Gateway-Route=dev`. `Authorization`
and `Content-Type` are managed by the client and are not overridden by this
setting.

### DeepSeek strict tool schemas

The official DeepSeek API exposes strict function-schema validation on the
`/beta` chat route:

```toml
provider = "deepseek"
base_url = "https://api.deepseek.com"
strict_tool_mode = true
```

CodeWhale preflights the complete active tool catalog. When every schema is
compatible it sends every function with `strict = true` to Beta Chat; if one
schema is not compatible, the complete request uses Standard Chat and the
original tool catalog stays available. The official root remains stable in
both cases. Strict validation does not imply `tool_choice = "required"`: the
model may still answer without a tool call. Model discovery and health checks
continue to use `/v1/models`.

This setting controls the Beta strict-schema guarantee, not basic tool-call
availability. Standard DeepSeek chat routes can still call tools; CodeWhale
removes only the unsupported `strict` flag when the final route is not Beta.
Nested `anyOf` remains available because DeepSeek explicitly includes it in the
Beta strict subset.

To bootstrap MCP and skills directories at their resolved paths, run `codewhale-tui setup`.
To only scaffold MCP, run `codewhale-tui mcp init`.

Note: setup, doctor, mcp, features, sessions, resume/fork, exec, review, and eval
are subcommands of the `codewhale-tui` binary. The `codewhale` dispatcher exposes a
distinct set of commands (`auth`, `config`, `model`, `thread`, `sandbox`,
`app-server`, `completion`) and forwards plain prompts to
`codewhale-tui`.

## Profiles

You can define multiple profiles in the same file:

```toml
api_key = "PERSONAL_KEY"
default_text_model = "deepseek-v4-pro"

[profiles.work]
api_key = "WORK_KEY"
base_url = "https://api.deepseek.com"

[profiles.nvidia-nim]
provider = "nvidia-nim"
api_key = "NVIDIA_KEY"
base_url = "https://integrate.api.nvidia.com/v1"
default_text_model = "deepseek-ai/deepseek-v4-pro"

[profiles.fireworks]
provider = "fireworks"
default_text_model = "accounts/fireworks/models/deepseek-v4-pro"

[profiles.siliconflow]
provider = "siliconflow"
default_text_model = "deepseek-ai/DeepSeek-V4-Pro"

[profiles.siliconflow.providers.siliconflow]
base_url = "https://api.siliconflow.com/v1"

[profiles.openai-compatible]
provider = "openai"

[profiles.openai-compatible.providers.openai]
base_url = "https://openai-compatible.example/v4"
model = "glm-5"

[profiles.atlascloud]
provider = "atlascloud"

[profiles.atlascloud.providers.atlascloud]
base_url = "https://api.atlascloud.ai/v1"
model = "deepseek-ai/deepseek-v4-flash"

[profiles.sglang]
provider = "sglang"
base_url = "http://localhost:30000/v1"
default_text_model = "deepseek-ai/DeepSeek-V4-Pro"

[profiles.vllm]
provider = "vllm"
base_url = "http://localhost:8000/v1"
default_text_model = "deepseek-ai/DeepSeek-V4-Pro"

[profiles.ollama]
provider = "ollama"
base_url = "http://localhost:11434/v1"
default_text_model = "codewhale-coder:1.3b"
```

Select a profile with:

- CLI: `codewhale --profile work`
- Env: `DEEPSEEK_PROFILE=work`

If a profile is selected but missing, codewhale exits with an error listing available profiles.

## Harness Profiles

v0.9 adds a config data model for model-specific harness posture. This is a
preview schema: it can be parsed and tested, but runtime provider/model
selection and prompt/tool behavior are wired in later v0.9 slices.
When no configured profile matches, the resolver falls back to built-in seed
profiles for the model families listed in the cutline doc. Configured profiles
always take precedence over those seeds.

```toml
[[harness_profiles]]
provider_route = "deepseek"
model_pattern = "deepseek-v4.*"

[harness_profiles.posture]
kind = "cache-heavy"          # standard | cache-heavy | lean | custom
max_subagents = 10            # 0 means runtime default
prefer_codebase_search = false
compaction_strategy = "prefix-cache" # default | prefix-cache | aggressive
tool_surface = "full"              # full | read-only | auto
safety_posture = "standard"        # standard | strict | permissive
```

Unknown posture names or unknown keys inside a harness profile fail config
deserialization instead of silently becoming `custom`. That is intentional:
once runtime wiring consumes these profiles, a typo should be visible.
The v0.9 implementation order and automatic-creator boundary are documented in
[`HARNESS_PROFILE_CUTLINE.md`](../legacy/rfcs/HARNESS_PROFILE_CUTLINE.md).

## Environment Variables

Most runtime environment variables override config values. API-key variables are
fallbacks after saved config and keyring credentials.

The three user-facing slots — provider, model, base URL — expose `CODEWHALE_*`
aliases. When both forms are set the `CODEWHALE_*` value wins; the
`DEEPSEEK_*` form is kept for older shells:

- `CODEWHALE_PROVIDER` (preferred) / `DEEPSEEK_PROVIDER` (legacy alias) —
  `deepseek|deepseek-anthropic|nvidia-nim|openai|atlascloud|wanjie-ark|volcengine|openrouter|xiaomi-mimo|novita|fireworks|siliconflow|arcee|siliconflow-CN|moonshot|sglang|vllm|ollama|huggingface|together|qianfan|openai-codex|anthropic|openmodel|zai|stepfun|minimax|deepinfra`
- `CODEWHALE_MODEL` (preferred) / `DEEPSEEK_MODEL` (legacy alias) — default model for the active provider
- `CODEWHALE_BASE_URL` (preferred) / `DEEPSEEK_BASE_URL` (legacy alias) — base URL for the active provider

Remaining variables:

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_ANTHROPIC_BASE_URL`
- `DEEPSEEK_HTTP_HEADERS` (custom model request headers, comma-separated `name=value` pairs)
- `DEEPSEEK_DEFAULT_TEXT_MODEL` (extra legacy alias of `DEEPSEEK_MODEL`)
- `DEEPSEEK_STREAM_IDLE_TIMEOUT_SECS` (stream idle timeout in seconds; default `900`, clamped to `1..=3600`)
- `DEEPSEEK_STREAM_OPEN_TIMEOUT_SECS` (connection setup + response-header wait in seconds; default `45`, clamped to `5..=300`; distinct from the per-chunk idle timeout)
- `CODEWHALE_CACHE_MAXIMAL` (`1`/`true`/`on`/`yes`) — cache-maximal context mode (#528). When on, the Repo Working Set block materializes the **full current contents** of the top active files into the system prompt each turn (deterministic order, byte-bounded), instead of only listing their paths. The block stays byte-stable while those files are unchanged so DeepSeek's KV prefix cache keeps hitting; editing a file cache-misses from its block onward. Off by default (path list only). Byte caps default to 24 KB per file / 96 KB total.
- `NVIDIA_API_KEY` or `NVIDIA_NIM_API_KEY` (preferred when provider is `nvidia-nim`; falls back to `DEEPSEEK_API_KEY`)
- `NVIDIA_NIM_BASE_URL`, `NIM_BASE_URL`, or `NVIDIA_BASE_URL`
- `NVIDIA_NIM_MODEL`
- `OPENAI_API_KEY`
- `OPENAI_BASE_URL`
- `OPENAI_MODEL`
- `ATLASCLOUD_API_KEY`
- `ATLASCLOUD_BASE_URL`
- `ATLASCLOUD_MODEL`
- `WANJIE_ARK_API_KEY`, `WANJIE_API_KEY`, or `WANJIE_MAAS_API_KEY`
- `WANJIE_ARK_BASE_URL`, `WANJIE_BASE_URL`, or `WANJIE_MAAS_BASE_URL`
- `WANJIE_ARK_MODEL`, `WANJIE_MODEL`, or `WANJIE_MAAS_MODEL`
- `VOLCENGINE_API_KEY`, `VOLCENGINE_ARK_API_KEY`, or `ARK_API_KEY`
- `VOLCENGINE_BASE_URL`, `VOLCENGINE_ARK_BASE_URL`, or `ARK_BASE_URL`
- `VOLCENGINE_MODEL` or `VOLCENGINE_ARK_MODEL`
- `OPENROUTER_API_KEY`
- `OPENROUTER_BASE_URL`
- `OPENROUTER_MODEL`
- `XIAOMI_MIMO_TOKEN_PLAN_API_KEY`, `MIMO_TOKEN_PLAN_API_KEY`, `XIAOMI_MIMO_API_KEY`, `XIAOMI_API_KEY`, or `MIMO_API_KEY`
- `XIAOMI_MIMO_BASE_URL` or `MIMO_BASE_URL`
- `XIAOMI_MIMO_MODEL` or `MIMO_MODEL`
- `XIAOMI_MIMO_MODE` or `MIMO_MODE` (`token-plan-sgp`, `token-plan-cn`,
  `token-plan-ams`, or `pay-as-you-go`)
- `NOVITA_API_KEY`
- `NOVITA_BASE_URL`
- `NOVITA_MODEL`
- `FIREWORKS_API_KEY`
- `FIREWORKS_BASE_URL`
- `FIREWORKS_MODEL`
- `HUGGINGFACE_API_KEY` or `HF_TOKEN` (`HF_TOKEN` is a fallback alias accepted when provider is `huggingface`)
- `HUGGINGFACE_BASE_URL` or `HF_BASE_URL`
- `HUGGINGFACE_MODEL` or `HF_MODEL`
- `SILICONFLOW_API_KEY`
- `SILICONFLOW_BASE_URL`
- `SILICONFLOW_MODEL`
- `ARCEE_API_KEY`
- `ARCEE_BASE_URL`
- `ARCEE_MODEL`
- `TOGETHER_API_KEY`
- `TOGETHER_BASE_URL`
- `TOGETHER_MODEL`
- `QIANFAN_API_KEY` or `BAIDU_QIANFAN_API_KEY`
- `QIANFAN_BASE_URL` or `BAIDU_QIANFAN_BASE_URL`
- `QIANFAN_MODEL` or `BAIDU_QIANFAN_MODEL`
- `OPENAI_CODEX_ACCESS_TOKEN` or `CODEX_ACCESS_TOKEN`
- `OPENAI_CODEX_BASE_URL` or `CODEX_BASE_URL`
- `OPENAI_CODEX_MODEL` or `CODEX_MODEL`
- `OPENAI_CODEX_ACCOUNT_ID` or `CODEX_ACCOUNT_ID`
- `ANTHROPIC_API_KEY`
- `ANTHROPIC_BASE_URL`
- `ANTHROPIC_MODEL`
- `ZAI_API_KEY` or `Z_AI_API_KEY`
- `ZAI_BASE_URL` or `Z_AI_BASE_URL`
- `ZAI_MODEL` or `Z_AI_MODEL`
- `STEPFUN_API_KEY` or `STEP_API_KEY`
- `STEPFUN_BASE_URL` or `STEP_BASE_URL`
- `STEPFUN_MODEL` or `STEP_MODEL`
- `MINIMAX_API_KEY`
- `MINIMAX_BASE_URL`
- `MINIMAX_MODEL`
- `DEEPINFRA_API_KEY` or `DEEPINFRA_TOKEN`
- `DEEPINFRA_BASE_URL`
- `DEEPINFRA_MODEL`
- `MOONSHOT_API_KEY` or `KIMI_API_KEY`
- `MOONSHOT_BASE_URL` or `KIMI_BASE_URL`
- `MOONSHOT_MODEL`, `KIMI_MODEL_NAME`, or `KIMI_MODEL`
- `SGLANG_BASE_URL`
- `SGLANG_MODEL`
- `SGLANG_API_KEY` (optional; many localhost SGLang servers do not require auth)
- `VLLM_BASE_URL`
- `VLLM_MODEL`
- `VLLM_API_KEY` (optional; many localhost vLLM servers do not require auth)
- `OLLAMA_BASE_URL`
- `OLLAMA_MODEL`
- `OLLAMA_API_KEY` (optional; many localhost Ollama servers do not require auth)
- `DEEPSEEK_LOG_LEVEL` or `RUST_LOG` (`info`/`debug`/`trace` enables lightweight verbose logs)
- `DEEPSEEK_SKILLS_DIR`
- `DEEPSEEK_MCP_CONFIG`
- `DEEPSEEK_NOTES_PATH`
- `DEEPSEEK_MEMORY` (`1|on|true|yes|y|enabled` turns user memory on)
- `DEEPSEEK_MEMORY_PATH`
- `DEEPSEEK_ALLOW_SHELL` (`1`/`true` enables)
- `DEEPSEEK_APPROVAL_POLICY` (`on-request|untrusted|never`)
- `DEEPSEEK_SANDBOX_MODE` (`read-only|workspace-write|danger-full-access|external-sandbox`)
- `DEEPSEEK_MANAGED_CONFIG_PATH`
- `DEEPSEEK_REQUIREMENTS_PATH`
- `DEEPSEEK_MAX_SUBAGENTS` (clamped to `1..=128`)
- `DEEPSEEK_TASKS_DIR` (runtime task queue/artifact storage, default
  `~/.codewhale/tasks`, with legacy `~/.deepseek/tasks` fallback when only the
  legacy directory exists)
- `DEEPSEEK_ALLOW_INSECURE_HTTP` (`1`/`true` allows non-local `http://` base URLs; default is reject)
- `DEEPSEEK_FORCE_HTTP1` (`1|true|yes|on` pins the HTTP client to HTTP/1.1, disabling HTTP/2; useful on Windows or behind proxies that mishandle long-lived H2 streams)
- `CODEWHALE_HOME` (override the base data directory; defaults to `~/.codewhale`).
  If you previously exported `DEEPSEEK_HOME`, rename it to `CODEWHALE_HOME`;
  the old env var is not used for new CodeWhale state paths.
- `DEEPSEEK_AUTOMATIONS_DIR` (override the automations storage directory; uses
  `~/.codewhale/automations` by default, with legacy `~/.deepseek/automations`
  fallback when only the legacy directory exists)
- `DEEPSEEK_CAPACITY_ENABLED`
- `DEEPSEEK_CAPACITY_LOW_RISK_MAX`
- `DEEPSEEK_CAPACITY_MEDIUM_RISK_MAX`
- `DEEPSEEK_CAPACITY_SEVERE_MIN_SLACK`
- `DEEPSEEK_CAPACITY_SEVERE_VIOLATION_RATIO`
- `DEEPSEEK_CAPACITY_REFRESH_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_REPLAN_COOLDOWN_TURNS`
- `DEEPSEEK_CAPACITY_MAX_REPLAY_PER_TURN`
- `DEEPSEEK_CAPACITY_MIN_TURNS_BEFORE_GUARDRAIL`
- `DEEPSEEK_CAPACITY_PROFILE_WINDOW`
- `DEEPSEEK_CAPACITY_PRIOR_CHAT`
- `DEEPSEEK_CAPACITY_PRIOR_REASONER`
- `DEEPSEEK_CAPACITY_PRIOR_V4_PRO`
- `DEEPSEEK_CAPACITY_PRIOR_V4_FLASH`
- `DEEPSEEK_CAPACITY_PRIOR_FALLBACK`
- `NO_ANIMATIONS` (`1|true|yes|on` forces `low_motion = true` and
  `fancy_animations = false` at startup, regardless of the saved
  settings; see [`docs/reference/ACCESSIBILITY.md`](./ACCESSIBILITY.md)).
- `SSL_CERT_FILE` — corporate-proxy / TLS-inspecting MITM users
  point this at a PEM bundle (or single DER cert) and the cert(s)
  get added alongside the platform's system trust store. Failures
  log a warning and continue — the existing system roots still
  apply.

### Instruction sources (`instructions = [...]`, #454)

Add a list of additional system-prompt sources that get
concatenated, in declared order, alongside the auto-loaded
`AGENTS.md`:

```toml
instructions = [
    "./AGENTS.md",
    "~/.codewhale/global.md",
    "~/team/agents-shared.md",
]
```

Rules:

- Paths run through `expand_path` so `~` and env vars work.
- Each file is capped at 100 KiB; oversized files are
  truncated with a `[…elided]` marker rather than skipped.
- Missing files are skipped with a tracing warning so a stale
  entry doesn't fail the launch.
- Only user-owned config, profiles, and managed config may set this array.
  Project config (`<workspace>/.codewhale/config.toml`, or legacy
  `<workspace>/.deepseek/config.toml`) ignores `instructions` so a cloned repo
  cannot choose arbitrary local files to place into the prompt.

## Settings File (Persistent UI Preferences)

codewhale also stores user preferences in:

- `~/.codewhale/settings.toml` on new installs
- `~/.deepseek/settings.toml` or the legacy platform config-dir
  `deepseek/settings.toml` when an existing settings file is present

These settings control presentation and input behavior. Automatic compaction is
owned by the canonical AgentRuntime context policy rather than a TUI
preference; the TUI exposes no enable or threshold setting. Manual `/compact`
remains available. Edit `~/.codewhale/settings.toml` to change retained
preferences.

Common settings keys:

- `theme` (`system`, `dark`, `light`, `grayscale`, `catppuccin-mocha`,
  `tokyo-night`, `dracula`, `gruvbox-dark`; default `system`): `system`
  follows terminal background detection, `dark`/`light` use the DeepSeek
  palettes, `grayscale` is the low-opinion black/white theme, and the named
  community presets apply across the TUI. Aliases such as `whale`, `mono`,
  `black-white`, `tokyonight`, and `gruvbox` are accepted.
- `bracketed_paste` (on/off, default on): request terminal paste events so
  pasted newlines remain composer text instead of ordinary Enter key events.
  Disable only for a terminal that mishandles bracketed-paste mode.
- `work_surface_placement` (`top`, `left`, or `right`; default `top`): places
  Ocean's Tasks / To-do / Workers surface above the transcript or in a side
  rail. Side choices fall back to the top layout on narrow terminals and in
  Classic without changing the saved Ocean preference. Set it in
  `~/.codewhale/settings.toml` and restart the TUI.
- `mention_menu_limit` (integer, default `128`): maximum number of
  `@`-mention popup candidates retained before the composer renders the
  visible window. The visible rows still depend on terminal height.
- `mention_walk_depth` (integer, default `6`): maximum workspace depth for
  `@`-mention completion walks. Set to `0` for unlimited depth in deeply
  nested workspaces; keep the default in very large repos unless needed.
- `mention_menu_behavior` (`fuzzy`, `browser`; default `fuzzy`): controls how
  `@`-mention completions are populated. `fuzzy` uses deterministic workspace
  ranking. `browser` lists only the immediate children of the currently typed
  directory segment in deterministic alphabetical order.
- `show_thinking` (on/off)
- `show_tool_details` (on/off)
- `background_color` (`#RRGGBB`, `RRGGBB`, or `default`): optional main TUI
  background color applied to the root, header, transcript, and footer
  surfaces while preserving panel contrast.
- `cost_currency` (`usd`, `cny`; default `usd`): currency used by the footer,
  `/cost`, and long-turn notification summaries. The
  aliases `rmb` and `yuan` normalize to `cny`.
- `default_mode` (`agent`, `plan`, or `operate`; legacy values are accepted for migration but are not live mode vocabulary)
- `sidebar_focus` (`pinned`, `auto`, `tasks`, `agents`, `context`, `hidden`; default
  `pinned`): selects the right sidebar focus. `pinned` keeps the right sidebar
  visible when the terminal is wide enough and composes Work, Tasks, Agents,
  and optional Context as they have live content. `auto` uses the same composed
  panels but collapses while idle. Saving
  `/sidebar auto --save` records an explicit auto-collapse opt-in so upgraded
  settings files that only captured the old default can migrate back to `pinned`.
  `hidden` disables the right sidebar entirely so raw terminal selection cannot
  cross from the transcript into sidebar borders. Legacy `plan` and `todos`
  values, plus the old `work` name, are accepted and normalized to `pinned`.
- `max_history` (number of submitted input history entries; cleared drafts are
  also kept locally for composer history search)
- `default_model` (model name override)

The composer has one direct-editing path. `composer_vim_mode`, `vim_mode`, and
`vim` are not settings; printable characters, including `v`, remain ordinary
composer input. Vim-style `j`/`k`/`g`/`G`/`y`/`q` bindings belong only to the
separate Pager modal.

Plan and Act are the everyday visible modes in the UI; Operate is an explicit
preview entry while its Workflow control surface is still being built. Switch
between them with `/mode`. For compatibility, older settings files with
`default_mode = "normal"` still load as `agent`.

The human-facing interface is fixed to Simplified Chinese. `locale` and
`language` are not supported settings; `LANG`, `LC_ALL`, and `LC_MESSAGES` do
not select UI or model-prompt language. The only UI message catalog is
`zh-Hans`, resolved through `tr(MessageId)`. Commands, configuration keys,
protocol fields, model IDs, paths, code, diffs, stdout/stderr, and raw logs
retain their machine contract or original bytes.

Readability semantics:

- Selection uses a unified style across transcript, composer menus, and modals.
- Footer hints use a dedicated semantic role (`FOOTER_HINT`) so hint text stays readable across themes.

### Token Quantities and Drivers

DeepSeek V4 prefix caching makes token labels matter. These quantities are kept
separate:

| Quantity | Meaning | Allowed to drive |
|---|---|---|
| Active request input estimate | Conservative estimate of the next request's live system prompt and transcript payload. | Header/footer context percent and canonical Runtime context safeguards. |
| Reserved response headroom | The internal turn budget plus safety headroom. v0.8.16 keeps normal turns at `262144` reserved output tokens and adds `1024` safety tokens for context-window checks, even though V4 capability metadata reports the official `384000` max output. | Emergency overflow budget checks only. |
| Cumulative API usage | Provider-reported input plus output tokens summed across completed API calls; multi-tool turns may count the same stable prefix more than once. | Session usage and approximate cost telemetry only. |
| Prompt cache hit/miss | Provider cache telemetry for the most recent call when available. | Cache-hit display and cost estimation only; never compaction triggers. |
| Context percent | Active request input estimate divided by the model context window. | Display only; it mirrors the active-input basis used by context safeguards. |
| Cost estimate | Approximate spend from provider usage and configured DeepSeek rates. | Display only. |

Automatic replacement compaction is a fixed canonical Runtime policy. It
replays the generated summary through the stable system prompt on the next
request and is accounted as a real model request. The TUI does not implement a
second pre-send compaction path or user override. Protocol and recovery tests
prove the mechanism; product benefit still requires the compaction on/off A/B
defined in the evaluation plan.

### Command Migration Notes

If you are upgrading from older releases:

- Old: `/codewhale`
  New: `/links` (aliases: `/dashboard`, `/api`)
- Old: `/set model deepseek-reasoner`
  New: set `model = "deepseek-v4-pro"` or `model = "deepseek-v4-flash"` in
  `~/.codewhale/config.toml`, then restart
- Old: visible `Normal` mode or `default_mode = "normal"`
  New: use `Agent` / `default_mode = "agent"`; legacy `normal` still maps to `agent`
- Old: discover `/set` in slash UX/help
  New: edit the configuration and settings files directly

## Key Reference

### Core keys (used by the TUI/engine)

- `provider` (string, optional): `deepseek` (default), `deepseek-anthropic`, `nvidia-nim`, `openai`, `atlascloud`, `wanjie-ark`, `volcengine`, `openrouter`, `xiaomi-mimo`, `novita`, `fireworks`, `siliconflow`, `arcee`, `siliconflow-CN`, `moonshot`, `sglang`, `vllm`, `ollama`, `huggingface`, `together`, `qianfan`, `openai-codex`, `anthropic`, `openmodel`, `zai`, `stepfun`, `minimax`, `deepinfra`, or `sakana`. Legacy `deepseek-cn` configs are still accepted as an alias for `deepseek`; DeepSeek uses the same official host [`https://api.deepseek.com`](https://api-docs.deepseek.com/) worldwide. `deepseek-anthropic` targets DeepSeek's Anthropic Messages-compatible endpoint at `https://api.deepseek.com/anthropic` using `DEEPSEEK_API_KEY`; `nvidia-nim` targets NVIDIA's NIM-hosted DeepSeek endpoints through `https://integrate.api.nvidia.com/v1`; `openai` targets a generic OpenAI-compatible endpoint, defaulting to `https://api.openai.com/v1`; `atlascloud` targets AtlasCloud's OpenAI-compatible endpoint at `https://api.atlascloud.ai/v1`; `wanjie-ark` targets Wanjie Ark's OpenAI-compatible endpoint at `https://maas-openapi.wanjiedata.com/api/v1`; `volcengine` targets Volcengine Ark's OpenAI-compatible coding endpoint at `https://ark.cn-beijing.volces.com/api/coding/v3`; `openrouter` targets `https://openrouter.ai/api/v1`; `xiaomi-mimo` targets Xiaomi MiMo's OpenAI-compatible endpoint, using `https://token-plan-sgp.xiaomimimo.com/v1` by default for Token Plan keys (`tp-...`) and `https://api.xiaomimimo.com/v1` for pay-as-you-go keys. For Token Plan accounts outside the Singapore default, set `base_url` explicitly or use `mode = "token-plan-cn"` for China and `mode = "token-plan-ams"` for Europe/Amsterdam; `novita` targets `https://api.novita.ai/openai/v1`; `fireworks` targets `https://api.fireworks.ai/inference/v1`; `siliconflow` targets SiliconFlow, defaulting to `https://api.siliconflow.com/v1`; `arcee` targets Arcee AI's OpenAI-compatible endpoint at `https://api.arcee.ai/api/v1`; `siliconflow-CN` targets the SiliconFlow China regional endpoint through `[providers.siliconflow_cn]`; `moonshot` targets Moonshot/Kimi, defaulting to `https://api.moonshot.ai/v1`; `sglang` targets a self-hosted OpenAI-compatible endpoint, defaulting to `http://localhost:30000/v1`; `vllm` targets a self-hosted vLLM OpenAI-compatible endpoint, defaulting to `http://localhost:8000/v1`; `ollama` targets Ollama's OpenAI-compatible endpoint, defaulting to `http://localhost:11434/v1`; `huggingface` targets Hugging Face Inference Providers at `https://router.huggingface.co/v1`; `together` targets Together AI at `https://api.together.xyz/v1`; `qianfan` targets Baidu Qianfan at `https://api.baiduqianfan.ai/v1`; `openai-codex` targets ChatGPT/Codex OAuth; `anthropic` targets Claude's native Messages API; `openmodel` targets OpenModel's Anthropic-compatible Messages API at `https://api.openmodel.ai`; `zai` targets Z.ai at `https://api.z.ai/api/coding/paas/v4`; `stepfun` targets StepFun at `https://api.stepfun.ai/v1`; `minimax` targets MiniMax at `https://api.minimax.io/v1`; `deepinfra` targets DeepInfra at `https://api.deepinfra.com/v1/openai`; `sakana` targets Sakana AI Fugu at `https://api.sakana.ai/v1`.
- `minimax-anthropic` (string provider value): selects MiniMax's Anthropic-compatible Messages route through `[providers.minimax_anthropic]`. The default Base URL is `https://api.minimax.io/anthropic`; set `https://api.minimaxi.com/anthropic` for China. Keep the `/anthropic` suffix because CodeWhale appends `/v1/messages`. The route uses `MINIMAX_API_KEY` and defaults to `MiniMax-M3`; `MiniMax-M2.7` is also registered. Official M3 input modalities are text, image, and video, with adaptive or disabled thinking. M2.7 is text-only and always keeps thinking enabled.
- `api_key` (string, required for hosted providers): must be non-empty for DeepSeek/hosted providers (or set the provider API key env var). Self-hosted SGLang, vLLM, and Ollama can omit it.
- `base_url` (string, optional): defaults to the official root `https://api.deepseek.com` for DeepSeek, including legacy `provider = "deepseek-cn"` configs. The DeepSeek request planner keeps ordinary Chat on the Standard surface and selects Beta only for an all-compatible Strict Chat catalog or FIM; callers do not switch the configured root to select a capability. Other defaults are `https://api.deepseek.com/anthropic` for `deepseek-anthropic`, `https://integrate.api.nvidia.com/v1` for `nvidia-nim`, `https://api.openai.com/v1` for `openai`, `https://api.atlascloud.ai/v1` for `atlascloud`, `https://maas-openapi.wanjiedata.com/api/v1` for `wanjie-ark`, `https://ark.cn-beijing.volces.com/api/coding/v3` for `volcengine`, `https://openrouter.ai/api/v1` for `openrouter`, `https://token-plan-sgp.xiaomimimo.com/v1` for `xiaomi-mimo` when the API key starts with `tp-...` and `https://api.xiaomimimo.com/v1` otherwise, `https://api.novita.ai/openai/v1` for `novita`, `https://api.fireworks.ai/inference/v1` for `fireworks`, `https://api.siliconflow.com/v1` for `siliconflow`, `https://api.siliconflow.cn/v1` for `siliconflow-CN`, `https://api.arcee.ai/api/v1` for `arcee`, `https://api.moonshot.ai/v1` for `moonshot`, `https://api.minimax.io/v1` for `minimax`, `https://api.openmodel.ai` for `openmodel`, `https://api.z.ai/api/coding/paas/v4` for `zai`, `https://api.stepfun.ai/v1` for `stepfun`, `https://api.deepinfra.com/v1/openai` for `deepinfra`, `https://api.sakana.ai/v1` for `sakana`, `https://router.huggingface.co/v1` for `huggingface`, `https://api.together.xyz/v1` for `together`, `https://api.baiduqianfan.ai/v1` for `qianfan`, `https://chatgpt.com/backend-api` for `openai-codex`, `https://api.anthropic.com` for `anthropic`, `http://localhost:30000/v1` for `sglang`, `http://localhost:8000/v1` for `vllm`, and `http://localhost:11434/v1` for `ollama`. Set `base_url = "https://token-plan-cn.xiaomimimo.com/v1"` for China-region Xiaomi MiMo Token Plan accounts or `base_url = "https://token-plan-ams.xiaomimimo.com/v1"` for Europe/Amsterdam accounts.
- `context_window` (integer, optional provider-table key): override the total context window for the active `[providers.<name>]` route when an OpenAI-compatible gateway, hosted model alias, or self-hosted runtime has a different limit than CodeWhale's static model table. For example, `[providers.openai] context_window = 1000000` lets an OpenAI-compatible DashScope/Qwen route budget against a 1M-token window instead of the conservative fallback. The value must be greater than 0 and affects prompt context notes, compaction thresholds, context-pressure checks, and request output caps.
- `path_suffix` (string, optional provider-table key): override the chat-completions path for OpenAI-compatible gateways that do not serve `/v1/chat/completions`. For example, `[providers.openai] path_suffix = "/chat/completions"` sends chat requests to the unversioned base URL plus `/chat/completions`; `models` and `beta/*` requests keep their normal routing.
- `reasoning_stream_style` (string, optional provider-table key): override how streaming reasoning is separated from answer text for the active provider route. Use `separate_field` for `reasoning_content` / `reasoning` deltas, `inline_tags` for gateways that stream `<think>...</think>` inside `delta.content`, or `none` to render incoming content exactly as answer text.
- `[providers.<name>.auth]` (table, optional): provider-scoped auth source metadata. `source = "command"` stores a command argv plus optional `timeout_ms`; `source = "secret"` stores a `secret_id`. This slice lets provider readiness, `/provider`, and doctor JSON report the auth source class without exposing command argv output or secret values; executing commands and resolving external secret material is handled by the follow-up resolver work.
- `insecure_skip_tls_verify` (bool, optional provider-table key): legacy compatibility key, disabled by default. When true on the active provider table, provider clients reject the configuration instead of skipping TLS certificate verification. Use `SSL_CERT_FILE` for corporate or private CA bundles; `codewhale doctor` reports stale uses of this setting.
- `default_text_model` (string, optional): defaults to `deepseek-v4-pro` for DeepSeek, `deepseek-anthropic`, and generic OpenAI-compatible endpoints, `deepseek-ai/deepseek-v4-pro` for NVIDIA NIM, `deepseek-ai/deepseek-v4-flash` for AtlasCloud, `deepseek-reasoner` for Wanjie Ark, `DeepSeek-V4-Pro` for Volcengine Ark, `deepseek/deepseek-v4-pro` for OpenRouter and Novita, `mimo-v2.5-pro` for Xiaomi MiMo, `accounts/fireworks/models/deepseek-v4-pro` for Fireworks, `deepseek-ai/DeepSeek-V4-Pro` for SiliconFlow and DeepInfra, `trinity-large-thinking` for Arcee AI, `kimi-k2.7-code` for Moonshot, `MiniMax-M3` for MiniMax, `GLM-5.2` for Z.ai, `step-3.7-flash` for StepFun, `ernie-4.0-turbo-8k` for Qianfan, `fugu` for Sakana AI, `deepseek-ai/DeepSeek-V4-Pro` for SGLang/vLLM, and `deepseek-coder:1.3b` for Ollama. Hugging Face and Together AI both default to `deepseek-ai/DeepSeek-V4-Pro`; `openai-codex` defaults to `gpt-5.5`; `anthropic` defaults to `claude-sonnet-4-6`; `openmodel` defaults to `deepseek-v4-flash`. Current public DeepSeek IDs are `deepseek-v4-pro` and `deepseek-v4-flash`, both with 1M context windows, 384K max output, and thinking mode enabled by default. Legacy `deepseek-chat` and `deepseek-reasoner` remain compatibility aliases for `deepseek-v4-flash` until July 24, 2026, except SiliconFlow maps `deepseek-reasoner` and `deepseek-r1` to its Pro model while `deepseek-chat` and `deepseek-v3` map to Flash. Provider-specific mappings translate `deepseek-v4-pro` / `deepseek-v4-flash` to each provider's model ID where supported. OpenRouter also recognizes recent large IDs such as `arcee-ai/trinity-large-thinking`, `minimax/minimax-m3`, `minimax/minimax-m2.7`, `xiaomi/mimo-v2.5-pro`, `qwen/qwen3.6-flash`, `qwen/qwen3.6-35b-a3b`, `qwen/qwen3.6-max-preview`, `qwen/qwen3.6-27b`, `qwen/qwen3.6-plus`, `qwen/qwen3.7-max`, `google/gemma-4-31b-it`, `moonshotai/kimi-k2.7-code`, `moonshotai/kimi-k2.6`, `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free`, and `nvidia/nemotron-3-ultra-550b-a55b`; direct Arcee uses bare IDs such as `trinity-large-thinking` and `trinity-large-preview`; direct Moonshot recognizes `kimi-k2.7-code`, `kimi-k2.6`, and Kimi Code's stable `kimi-for-coding`; direct MiniMax recognizes `MiniMax-M3` and the documented M2.x chat model IDs; direct Sakana recognizes `fugu` and `fugu-ultra-20260615`; direct Xiaomi MiMo recognizes chat IDs `mimo-v2.5-pro`, `mimo-v2.5-pro-ultraspeed`, and `mimo-v2.5`. Generic `openai`, `atlascloud`, `wanjie-ark`, `xiaomi-mimo`, `arcee`, `moonshot`, `minimax`, `openmodel`, `zai`, `stepfun`, `qianfan`, `sakana`, and Ollama model IDs are passed through unchanged after known aliases are normalized. OpenRouter and SiliconFlow provider configs with a custom `base_url` also preserve explicit model values, which lets OpenAI-compatible gateways accept bare model IDs. Use `/models` or `codewhale models` to discover live IDs from your configured endpoint. `CODEWHALE_MODEL` overrides this for a single process; `DEEPSEEK_MODEL` is the legacy alias.
- `reasoning_effort` (string, optional): `off`, `low`, `medium`, `high`, `max`, `xhigh`, or `ultracode`; defaults to the configured UI tier. DeepSeek Platform receives top-level `thinking` / `reasoning_effort` fields. OpenAI Codex normalizes stale `off` to `low` and sends `max` / `ultracode` as Responses `xhigh`. Z.ai receives documented `thinking` controls and treats enabled thinking as the GLM coding high/max lane. NVIDIA NIM receives equivalent settings through `chat_template_kwargs`.
- `verbosity` (string, optional): `normal` or `concise`. `normal` keeps the
  default conversational prompt. `concise` appends a prompt discipline block
  for direct, low-chatter output; CLI noninteractive commands (`exec` and
  `eval`) default to `concise` unless config/env/CLI overrides it.
  Override per process with `CODEWHALE_VERBOSITY` or the legacy
  `DEEPSEEK_VERBOSITY` alias.
- `allow_shell` (bool, optional): in interactive TUI Agent sessions, omitting
  this keeps shell tools available with approval prompts; setting it to `false`
  hides shell tools. Headless, durable-task, and other noninteractive profiles
  keep the conservative omitted-field default and require `allow_shell = true`
  to expose shell. Plan mode always hides shell; Full Access enables shell and
  auto-approval.
- `approval_policy` (string, optional): `on-request`, `untrusted`, or `never`.
- `sandbox_mode` (string, optional): `read-only`, `workspace-write`, `danger-full-access`, `external-sandbox`.
  Platform support is not identical. macOS uses Seatbelt for policy
  enforcement. Linux support is helper-gated around Landlock. Windows does not
  currently advertise an OS sandbox; the planned Windows helper contract starts
  with process-tree containment only and must not be described as read-only
  filesystem isolation, workspace-write enforcement, network blocking,
  registry isolation, or AppContainer isolation until those are implemented.
- `permissions.toml` (sibling file, optional): typed permission rule records
  loaded next to `config.toml`, for example `~/.codewhale/permissions.toml`.
  Manually authored `[[rules]]` entries accept `tool`, optional `command` or
  `path`, and optional `action = "deny" | "ask" | "allow"`; omitted `action`
  defaults to `"ask"`. `deny` blocks matching invocations before mode-based
  approval handling, `allow` skips approval for matching invocations, and
  `ask` forces approval only in modes that can prompt. Outside the TUI
  auto-approve path, a matching `ask` rule under `approval_policy = "never"`
  is rejected because no prompt can be shown. In Full Access / auto-approval sessions,
  `ask` rules do not downgrade the session into prompting or blocking; explicit
  `deny` rules still block according to the current execution-policy logic.

  In a supported approval card, press `S` to approve the request once and
  append exact `action = "ask"` rules to this file. Supported saves are
  intentionally narrow:
  `exec_shell` stores the exact approved command string; `write_file` and
  `edit_file` store the exact workspace-relative file path; `apply_patch`
  stores one exact workspace-relative `path` rule per validated touched file
  from apply-patch preflight. Existing exec command matching remains
  arity-aware, and file paths are normalized to the same workspace-relative
  form used by runtime matching.

  `read_file` rules can still be authored manually when you want future reads
  of a specific path to ask, allow, or deny, but the approval UI does not save
  `read_file` rules. The UI is not a policy editor: it does not save
  `allow`/`deny`, edit or delete rules, expand globs, or create broad
  directory/recursive rules.
- `managed_config_path` (string, optional): managed config file loaded after user/env config.
- `requirements_path` (string, optional): requirements file used to enforce allowed approval/sandbox values.
- `max_subagents` (int, optional): defaults to `64` and is clamped to `1..=128`.
- `subagents.*` (optional): availability, concurrency, and depth controls for
  the canonical `agent` runtime. Supported keys are `enabled`,
  `max_concurrent`, and `max_depth`. The `[subagents] max_concurrent` value
  overrides top-level `max_subagents` and is also clamped to `1..=128`.
  `[subagents.providers.<provider>]` accepts the same availability, fanout,
  and depth knobs (`enabled`, `max_concurrent`, `max_depth`) and inherits the global
  `[subagents]` value for any key you omit. Provider keys accept canonical
  names such as `deepseek`, `zai`, `openrouter`, `anthropic`, plus convenience
  aliases such as `glm` for Z.ai and `deepseek_api` for direct DeepSeek:

  ```toml
  [subagents]
  max_concurrent = 20
  max_depth = 6

  [subagents.providers.deepseek]
  max_concurrent = 20

  [subagents.providers.glm]
  max_concurrent = 4
  max_depth = 2

  [subagents.providers.openrouter]
  max_concurrent = 5
  ```

- `skills_dir` (string, optional): defaults to `~/.codewhale/skills` (each skill is
  a directory containing `SKILL.md`). Workspace-local `.agents/skills` or
  `./skills` are preferred when present; the runtime also discovers global
  agentskills.io-compatible `~/.agents/skills` and the broader Claude-ecosystem
  `~/.claude/skills`. First launch installs versioned bundled skills for common
  workflows including skill creation, delegation, MCP/plugin scaffolding,
  documents, presentations, spreadsheets, PDFs, and Feishu/Lark. This imported
  compatibility surface is not part of the target DeepSeek-only product and is
  retained only until the skills boundary is migrated or removed.
- `[skills].scan_codewhale_only` (bool, default `false`): when `true`, session
  skill discovery ignores cross-tool roots such as `.claude/skills`,
  `.opencode/skills`, `.cursor/skills`, and `~/.agents/skills`. CodeWhale still
  scans `<workspace>/.codewhale/skills`, `~/.codewhale/skills`, and any explicit
  `skills_dir` override.
- `mcp_config_path` (string, optional): defaults to `~/.codewhale/mcp.json`, with
  legacy `~/.deepseek/mcp.json` fallback when the CodeWhale path is absent.
  It is the only write target for `setup --mcp` and the MCP configuration
  commands. MCP inventory, connection checks, OAuth, and tool discovery use
  the resolved global file plus trusted workspace/plugin additions where
  applicable, but configuration writes never copy those additions into the
  global file. The canonical Agent does not currently advertise discovered
  MCP tools to the model.
- `notes_path` (string, optional): defaults to `~/.codewhale/notes.txt`, with
  legacy `~/.deepseek/notes.txt` fallback when the CodeWhale path is absent, and
  is used by the model-visible `note` tool.
- `[memory].enabled` (bool, optional): defaults to `false`. When `true`,
  the TUI loads the user memory file into a `<user_memory>` prompt block,
  enables `# foo` quick-capture in the composer, surfaces the `/memory`
  slash command, and registers the `remember` tool. The same toggle is
  available via `DEEPSEEK_MEMORY=on`.
- `memory_path` (string, optional): defaults to `~/.codewhale/memory.md`, with
  legacy `~/.deepseek/memory.md` fallback when the CodeWhale path is absent.
  Used by the user-memory feature when enabled — see
  [`MEMORY.md`](MEMORY.md) for the full feature surface (`# foo`
  composer prefix, `/memory` slash command, `remember` tool, opt-in
  toggle).
- `context.*` (optional): deterministic project context in the stable prompt:
  - `[context].project_pack` (bool, default `true`)
- `retry.*` (optional): retry/backoff settings for API requests:
  - `[retry].enabled` (bool, default `true`)
  - `[retry].max_retries` (int, default `3`)
  - `[retry].initial_delay` (float seconds, default `1.0`)
  - `[retry].max_delay` (float seconds, default `60.0`)
  - `[retry].exponential_base` (float, default `2.0`)
- `capacity.*` (optional): runtime context-capacity controller. This is opt-in
  because its active interventions can rewrite the live transcript.
  - `[capacity].enabled` (bool, default `false`)
  - `[capacity].low_risk_max` (float, default `0.50`)
  - `[capacity].medium_risk_max` (float, default `0.62`)
  - `[capacity].severe_min_slack` (float, default `-0.25`)
  - `[capacity].severe_violation_ratio` (float, default `0.40`)
  - `[capacity].refresh_cooldown_turns` (int, default `6`)
  - `[capacity].replan_cooldown_turns` (int, default `5`)
  - `[capacity].max_replay_per_turn` (int, default `1`)
  - `[capacity].min_turns_before_guardrail` (int, default `4`)
  - `[capacity].profile_window` (int, default `8`)
  - `[capacity].deepseek_v3_2_chat_prior` (float, default `3.9`)
  - `[capacity].deepseek_v3_2_reasoner_prior` (float, default `4.1`)
  - `[capacity].deepseek_v4_pro_prior` (float, default `3.5`)
  - `[capacity].deepseek_v4_flash_prior` (float, default `4.2`)
  - `[capacity].fallback_default_prior` (float, default `3.8`)
- `tui.alternate_screen` (string, optional): `auto`, `always`, or `never`. This is retained for config compatibility, but interactive sessions now always use the TUI-owned alternate screen so host terminal scrollback cannot hijack the viewport.
- `tui.mouse_capture` (bool, optional, default `true` on non-Windows terminals and on Windows Terminal/ConEmu/Cmder when the alternate screen is active; `false` on legacy Windows console and inside JetBrains JediTerm — PyCharm/IDEA/CLion/etc. — where mouse-event escapes leak into the input stream as garbled text, see #878 / #898): deliver terminal mouse events to internal transcript wheel scrolling and supported modal interactions. The TUI does not implement transcript drag selection or automatic copying. Set this to `false` or run with `--no-mouse-capture` when the host terminal should own raw text selection end-to-end; set it to `true` or run with `--mouse-capture` to opt in anywhere it is defaulted off.
- `tui.terminal_probe_timeout_ms` (int, optional, default `500`): startup terminal-mode probe timeout in milliseconds. Values are clamped to `100..=5000`; timeout emits a warning and aborts startup instead of hanging indefinitely.
- `tui.stream_chunk_timeout_secs` (int, optional, default `900`): per-SSE-chunk idle timeout for streamed model responses. Set it in the configuration file; `0` maps to the default and explicit values must be `1..=3600`. The legacy `DEEPSEEK_STREAM_IDLE_TIMEOUT_SECS` env var is still honored when this key is omitted.
- `tui.osc8_links` (bool, optional, default on for macOS/Linux, off for Windows): emit OSC 8 escape sequences around URLs in transcript output so supporting terminals (iTerm2, Terminal.app 13+, Ghostty, Kitty, WezTerm, Alacritty, recent gnome-terminal/konsole) can open them with the terminal's link gesture—usually Cmd-click on macOS and Ctrl-click on Linux/Windows. Terminals without OSC 8 support render the plain label and ignore the escape. The escapes are emitted out-of-band (not inside buffer cells), so column corruption is not a concern; set `false` only for terminals that misrender the OSC 8 terminator itself. Windows legacy consoles default off; opt in with `true`.
- `features.*` (optional): feature flag overrides (see below).

### Workspace notes

`/note` manages a simple notes file in the current workspace at
`.deepseek/notes.md`. Existing `/note <text>` usage still appends a note.
The management forms are:

| Command | Action |
|---|---|
| `/note <text>` | Append a note (legacy shorthand) |
| `/note add <text>` | Append a note explicitly |
| `/note list` | List notes with temporary 1-based numbers |
| `/note show <n>` | Show the full note at number `n` |
| `/note edit <n> <text>` | Replace note `n` with new text |
| `/note remove <n>` | Delete note `n`; `rm` and `delete` are aliases |
| `/note clear` | Empty the workspace notes file |
| `/note path` | Show the resolved workspace notes path |

The numbers shown by `/note list` are not stored in the file; they are derived
from the current order each time notes are read. This keeps the file format
compatible with the existing `---`-separated notes.

### User memory

User memory is split across one top-level path setting and one opt-in
toggle table:

```toml
memory_path = "~/.codewhale/memory.md"

[memory]
enabled = true
```

Notes:

- `memory_path` stays at the top level beside `notes_path` and
  `skills_dir`; it is not nested under `[memory]`.
- `DEEPSEEK_MEMORY_PATH` overrides the file path from the environment.
- `DEEPSEEK_MEMORY=on` (also `1`, `true`, `yes`, `y`, or `enabled`)
  flips the feature on without editing `config.toml`.
- The feature is inert when disabled: no file is injected, `# foo`
  falls through to normal message submission, and the model does not
  see the `remember` tool.
- See [`MEMORY.md`](MEMORY.md) for examples and the full `/memory`
  command surface.

### Parsed but currently unused (reserved for future versions)

These keys are accepted by the config loader but not currently used by the interactive TUI or built-in tools:

- `tools_file`

## Feature Flags

Feature flags live under the `[features]` table and are merged across profiles.
Defaults are enabled for built-in tooling, so you only need to set entries you
want to force on or off.

```toml
[features]
shell_tool = true
subagents = true
web_search = true # enables canonical web.run plus the compatibility web_search alias
apply_patch = true
mcp = true
exec_policy = true
```

You can also override features for a single run:

- `codewhale-tui --enable web_search`
- `codewhale-tui --disable subagents`

Use `codewhale-tui features list` to inspect known flags and their effective state.
Change feature flags in `[features]` or with `--enable` / `--disable`.

## Web Search Provider

`web_search` uses DuckDuckGo by default and does not require an API key. The
DuckDuckGo path keeps a Bing fallback when DDG returns a bot challenge or no
parseable results. Bing remains selectable for users who explicitly want it,
and Tavily, Bocha, Metaso, SearXNG, Baidu, Volcengine, or Sofya can be selected
when an API-backed provider is preferred.

For a private/internal search service that serves DuckDuckGo-compatible HTML,
keep `provider = "duckduckgo"` and set `base_url`; CodeWhale appends the `q`
query parameter to that endpoint and applies network policy to its host.
Custom endpoints do not fall back to public Bing. `CODEWHALE_SEARCH_BASE_URL`
can override this per process; `DEEPSEEK_SEARCH_BASE_URL` remains accepted as
the legacy alias.

**SearXNG** ([docs](https://docs.searxng.org/dev/search_api.html)) uses the
configured instance's JSON API. Set `provider = "searxng"` and
`base_url = "https://your-searxng.example"`; CodeWhale calls
`/search?q=...&format=json`. CodeWhale does not use a public SearXNG instance
by default because public instances often disable JSON output or rate-limit API
traffic.

**Metaso** ([metaso.cn](https://metaso.cn)) has a 100 searches/day free quota;
set `METASO_API_KEY` or `[search] api_key` for a higher quota.

**Baidu** uses Baidu AI Search at
`https://qianfan.baidubce.com/v2/ai_search/web_search`. Set
`BAIDU_SEARCH_API_KEY` or `[search] api_key`. This is a search-tool backend
only; it does not add a Baidu model provider.

**Sofya** ([sofya.co](https://sofya.co)) returns full extracted page content
rather than snippets. Set `[search] api_key` to your `ay_live_...` key, or the
`SOFYA_API_KEY` env var. This is a search-tool backend only; it does not add a
Sofya model provider.

```toml
[search]
provider = "searxng" # duckduckgo | bing | tavily | bocha | metaso | searxng | baidu | volcengine | sofya
# base_url = "https://search.example/" # optional with provider = "duckduckgo"; required with "searxng"
# api_key = "YOUR_KEY" # required for tavily, bocha, baidu, volcengine, and sofya; optional for metaso; unused by searxng
```

## Local File Context

Use `@path/to/file` in the composer as a visible local path reference. The popup
only completes composer text; the next submit preserves the exact `@path` and
does not read or inline the file, directory, image, or hidden XML context.
DeepSeek can inspect a path through the canonical `read_file` tool; supported
local images route to OCR only when that tool is invoked. Terminal paste
continues through normal text paste events.

## Managed Configuration and Requirements

codewhale supports a policy layering model:

1. user config + profile + env overrides
2. managed config (if present)
3. requirements validation (if present)

By default on Unix:
- managed config: `/etc/deepseek/managed_config.toml`
- requirements: `/etc/deepseek/requirements.toml`

Requirements file shape:

```toml
allowed_approval_policies = ["on-request", "untrusted", "never"]
allowed_sandbox_modes = ["read-only", "workspace-write"]
```

If configured values violate requirements, startup fails with a descriptive error.

## Notes On `codewhale-tui doctor`

`codewhale-tui doctor` follows the same config resolution rules as the rest of the
TUI. That means `--config`, `CODEWHALE_CONFIG_PATH`, and the legacy
`DEEPSEEK_CONFIG_PATH` are respected, and MCP/skills
checks use the resolved `mcp_config_path` / `skills_dir` (including env overrides).

To bootstrap missing MCP/skills paths, run `codewhale-tui setup --all`. You can
also run `codewhale-tui setup --skills --local` to create a workspace-local
`./skills` dir.

`codewhale-tui doctor --json` prints a machine-readable report that skips the
live API connectivity probe. Top-level keys: `version`, `config_path`,
`config_present`, `workspace`, `api_key.source`, `base_url`,
`default_text_model`, `mcp`, `skills`, `tools`, `plugins`, `sandbox`,
`platform`, `api_connectivity`, `capability`. CI consumers should rely on `api_key.source`
(`env`/`config`/`missing`) rather than parsing the human-readable `doctor`
text.

The `capability` key contains per-provider capability info derived from
static knowledge (release docs, API guides) rather than live API probes.
Top-level sub-keys: `resolved_provider`, `resolved_model`, `context_window`,
`max_output`, `thinking_supported`, `cache_telemetry_supported`,
and `request_payload_mode`.

Use `capability.context_window` and `capability.max_output` for model-limit
checks in CI scripts; do not treat `capability.max_output` as the per-turn
request budget. Use `capability.thinking_supported` to decide whether to
configure reasoning effort.

## Setup status and extension dirs

`codewhale-tui setup` accepts a few flags beyond the existing `--mcp`,
`--skills`, `--local`, `--all`, and `--force`:

- `--status` — print a compact one-screen status (api key, base URL, model,
  MCP/skills/plugins counts, sandbox, `.env` presence). Read-only and
  network-free; safe to run in CI. If `.env` is missing and `.env.example` is
  present in the workspace, the status output points at `cp .env.example .env`.
- `--plugins` — scaffold `~/.codewhale/plugins/` with a `README.md` and an
  `example/PLUGIN.md` sample using the same frontmatter shape as
  `SKILL.md`. Plugins are not loaded automatically either; reference them
  from a skill or MCP wrapper when you want them active.
- `--all` scaffolds MCP + skills + plugins together.
`--status` is mutually exclusive with the scaffold flags.

## Why the engine strips XML/`[TOOL_CALL]` text

codewhale sends and receives tool calls only over the API tool channel
(structured `tool_use` / `tool_call` items). The streaming loop in
`crates/tui/src/core/engine.rs` recognizes a fixed set of fake-wrapper start
markers — `[TOOL_CALL]`, `<codewhale:tool_call`, `<tool_call`, `<invoke `,
`<function_calls>` — and scrubs them from visible assistant text without ever
turning them into structured tool calls. When a wrapper is stripped, the loop
emits one compact `status` notice per turn so the user can see why their
visible text shrank. Treat any change that re-enables text-based tool
execution as a regression; the protocol-recovery tests in
`crates/tui/tests/protocol_recovery.rs` lock the contract.
