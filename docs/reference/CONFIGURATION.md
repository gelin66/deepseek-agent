# Configuration

CodeWhale uses one DeepSeek-only model configuration and a separate set of
local execution, TUI, search, MCP, and Fleet settings.

The canonical user file is:

```text
~/.codewhale/config.toml
```

Override it with `--config` or `CODEWHALE_CONFIG_PATH`. An explicit
`CODEWHALE_HOME` is an isolation boundary: CodeWhale does not fall back to
ambient state outside that directory.

See [`config.example.toml`](../../config.example.toml) for a complete current
example.

## DeepSeek model settings

There are exactly three persisted model-bearing keys:

```toml
# Prefer `codewhale auth set` instead of editing this value.
api_key = "..."
base_url = "https://api.deepseek.com"
default_text_model = "deepseek-v4-pro"
```

The backend is always official DeepSeek. There is no Provider selector,
Provider table, Provider fallback, model catalog, gateway registry, OAuth
model route, custom model header, TLS-bypass setting, or path-suffix setting.

Credential precedence is fixed:

```text
CLI --api-key
  -> config.toml api_key
  -> secret store / OS keyring
  -> DEEPSEEK_API_KEY
```

Model and endpoint CLI/environment overrides use:

```text
--model
  -> CODEWHALE_MODEL
  -> DEEPSEEK_MODEL
  -> DEEPSEEK_DEFAULT_TEXT_MODEL
  -> default_text_model
  -> deepseek-v4-pro
```

```text
--base-url
  -> CODEWHALE_BASE_URL
  -> DEEPSEEK_BASE_URL
  -> base_url
  -> https://api.deepseek.com
```

Foreign model ids and non-official remote endpoints fail before a model
request. Explicit loopback endpoints are retained for offline fixtures.
`codewhale app-server` always requires the official HTTPS endpoint.

## Deleted configuration

The following keys are rejected rather than ignored:

```text
provider
providers
fallback_providers
model
auth
auth_mode
http_headers
insecure_skip_tls_verify
path_suffix
harness_profiles
model_catalog
models
```

Replaced camel-case keys such as `apiKey`, `baseUrl`, and
`defaultTextModel` are also rejected. There is no compatibility reader or
dual write. A previous DeepSeek Provider table:

```toml
provider = "deepseek"

[providers.deepseek]
api_key = "..."
model = "deepseek-v4-pro"
```

must become:

```toml
api_key = "..."
default_text_model = "deepseek-v4-pro"
```

## CLI configuration commands

```bash
codewhale config path
codewhale config list
codewhale config get default_text_model
codewhale config set default_text_model deepseek-v4-flash
codewhale config unset default_text_model
```

Sensitive values are redacted in `get`, `list`, Doctor, and logs.

Credential commands:

```bash
codewhale auth status
codewhale auth set
codewhale auth clear
codewhale auth migrate
```

Model commands:

```bash
codewhale model list
codewhale model resolve
codewhale model set deepseek-v4-pro
```

These commands have no Provider argument.

## Runtime and presentation

```toml
output_mode = "text"
verbosity = "normal"       # normal | concise
log_level = "info"
telemetry = false
approval_policy = "on-request"
sandbox_mode = "workspace-write"
allow_shell = true
yolo = false
reasoning_effort = "max"
```

Important environment overrides include:

```text
CODEWHALE_VERBOSITY
CODEWHALE_OUTPUT_MODE
CODEWHALE_LOG_LEVEL
CODEWHALE_TELEMETRY
CODEWHALE_APPROVAL_POLICY
CODEWHALE_SANDBOX_MODE
CODEWHALE_YOLO
```

`--yolo` is an explicit startup override for automatic approval and
workspace-external trust. It does not alter the model backend.

## Retry policy

```toml
[retry]
enabled = true
max_retries = 3
initial_delay = 1.0
max_delay = 60.0
exponential_base = 2.0
```

Model transport retries remain subject to typed retry disposition and
side-effect safety. A failure with ambiguous side effects is not blindly
retried.

## TUI settings

```toml
[tui]
alternate_screen = "auto"  # auto | always | never
mouse_capture = true
terminal_probe_timeout_ms = 250
stream_chunk_timeout_secs = 900
osc8_links = true
```

The interactive TUI is a thin client of the canonical Run API. These values
change terminal behavior only; they do not create a second runtime, model
loop, Store, or tool catalog.

## Feature flags

```toml
[features]
subagents = true
exec_policy = true
```

Unknown feature keys fail validation.

## Skills and instructions

```toml
skills_dir = "~/.codewhale/skills"
instructions = ["~/.codewhale/AGENTS.md"]

[skills]
scan_codewhale_only = false
```

Environment overrides:

```text
CODEWHALE_SKILLS_DIR / DEEPSEEK_SKILLS_DIR
```

## MCP

```toml
mcp_config_path = "~/.codewhale/mcp.json"
mcp_oauth_callback_port = 8765
# mcp_oauth_callback_url = "http://127.0.0.1:8765/callback"
```

MCP server definitions and MCP transport credentials live in the referenced
JSON document. MCP OAuth is unrelated to DeepSeek model authentication.

Environment override:

```text
CODEWHALE_MCP_CONFIG
```

## Subagents

```toml
[subagents]
enabled = true
max_concurrent = 4
max_depth = 3
```

Root, read-only child, and explicitly admitted Writer children use the same
`AgentRuntime`. Disabling subagents or setting a zero concurrency/depth limit
fails closed at the canonical command projection.

## Fleet

```toml
[fleet]
default_trust_level = "sandbox"
require_identity_verification = true
max_trust_level = "operator"

[fleet.exec]
allowed_tools = []
disallowed_tools = []
max_turns = 4294967295
max_spawn_depth = 3
append_system_prompt = ""
output_format = "text"
```

Role presets:

```toml
[fleet.roles.reviewer]
description = "Read-only review"
tool_profile = "read-only"
timeout_seconds = 600
trust_level = "sandbox"
```

Profiles:

```toml
[fleet.profiles.scout]
slot = "scout"
role = "scout"
loadout = "fast"
model = "deepseek-v4-flash"
reasoning_effort = "high"

[fleet.profiles.scout.permissions]
allow_shell = false
trust = false
approval_required = true

[fleet.profiles.scout.delegation]
max_spawn_depth = 2
max_concurrency = 2
```

A Fleet profile may omit `model` to inherit the run model or pin an official
DeepSeek model. A `provider` field is unknown and rejected. Writing agents
still require isolated worktrees and explicit admission.

## Search

Search adapters are independent of the model backend:

```toml
[search]
provider = "duckduckgo"
# base_url = "..."
# api_key = "..."
```

The word `provider` under `[search]` selects a retrieval adapter only. It
cannot change the DeepSeek model route.

## Named profiles

Named profiles can override non-model TUI/local settings:

```toml
[profiles.quiet]
verbosity = "concise"
allow_shell = false
```

Profile tables cannot contain `api_key`, `base_url`,
`default_text_model`, `provider`, or `providers`. Model identity remains
root-owned.

## Project-local configuration

CodeWhale may read:

```text
$WORKSPACE/.codewhale/config.toml
```

Repo-local config is untrusted. It may tighten approval and sandbox posture
and apply safe local tool/Fleet settings. It cannot change:

- API credentials;
- DeepSeek endpoint;
- model identity;
- Provider identity;
- telemetry;
- unrelated unknown global settings.

Values that would weaken the current approval or sandbox posture are ignored
or rejected.

## State

Canonical state lives under `$CODEWHALE_HOME` or `~/.codewhale`. Retired
`.deepseek` product state and product environment aliases are not read,
migrated, or modified. State and config paths reject symlinked files, absolute
injected subdirectories, and `..` traversal.

Config writes are atomic, owner-only, preserve comments where possible, and
create one `.bak` copy before replacing an existing file.

## Minimal configuration

For most users:

```bash
codewhale auth set
codewhale
```

The generated file needs only:

```toml
default_text_model = "deepseek-v4-pro"
```

Doctor reports the resolved fixed DeepSeek route, credential source class,
redacted endpoint, model, TLS posture, and canonical runtime facts without
making Provider/catalog choices.
