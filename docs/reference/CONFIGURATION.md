# DSE configuration

> Category: current user reference.

DSE has one official DeepSeek model backend and a separate set of local
execution, TUI, search, Skills, MCP, and subagent settings. The complete
commented example is [`config.example.toml`](../../config.example.toml).

## Files and ownership

The canonical user file is:

```text
~/.dse/config.toml
```

Resolution is:

```text
--config
  -> DSE_CONFIG_PATH
  -> $DSE_HOME/config.toml
  -> ~/.dse/config.toml
```

`DSE_HOME` is a hard isolation boundary. When it is set, DSE does not read
ambient product state from another home directory.

The canonical SQLite RunStore is `~/.dse/state.db`, or
`$DSE_HOME/state.db` when `DSE_HOME` is set. TUI settings, setup state, and
optional extension files live under the same resolved DSE home. Permission
selection is process-local and frozen into each Run; it is not a config file.

## Official DeepSeek identity

There are exactly three persisted model-bearing root keys:

```toml
# Prefer `dse login` or `dse auth set`.
# api_key = "..."
base_url = "https://api.deepseek.com"
default_text_model = "deepseek-v4-pro"
```

DSE posts ordinary production requests to the official
`/chat/completions` path. It accepts `deepseek-v4-pro`,
`deepseek-v4-flash`, and the bounded convenience forms `pro` and `flash`.
The retired `deepseek-chat`, `deepseek-reasoner`, model `auto`, foreign model
ids, and injected/invalid values fail before a request is created.

The product has no model Provider selector, Provider table, fallback Provider,
gateway registry, custom model header, TLS-bypass setting, model catalog, or
dynamic route mode.

### Credential precedence

```text
explicit CLI API key
  -> config.toml api_key
  -> DSE platform credential store
  -> DEEPSEEK_API_KEY
```

`dse login` and `dse auth set` save the official DeepSeek credential without
echoing it. These commands report only source/status facts:

```bash
dse auth status
dse auth set
dse auth get
dse auth clear
dse auth migrate --dry-run
```

`auth migrate` is an explicit advanced operation for moving an existing
config-file key into the platform credential store. Inspect its dry run first.

### Model and endpoint precedence

```text
--model
  -> DSE_MODEL
  -> DEEPSEEK_MODEL
  -> DEEPSEEK_DEFAULT_TEXT_MODEL
  -> default_text_model
  -> deepseek-v4-pro
```

```text
--base-url
  -> DSE_BASE_URL
  -> DEEPSEEK_BASE_URL
  -> base_url
  -> https://api.deepseek.com
```

Production accepts the official DeepSeek HTTPS roots. Explicit loopback URLs
remain available to deterministic offline fixtures and local production-path
tests. App-server rejects a foreign remote model endpoint before starting a
canonical Run.

Model commands:

```bash
dse model list
dse model resolve
dse model set deepseek-v4-pro
```

An omitted model uses the fixed actor profile: root and explicit Writer use
Pro/high, ordinary isolated read-only children use Flash/high, and typed
recovery/recheck/rework uses Pro/max. This is not Auto routing and does not send
a classifier request.

## Configuration commands

```bash
dse config path
dse config list
dse config get default_text_model
dse config set default_text_model deepseek-v4-flash
dse config unset default_text_model
dse config set ui.language en
dse config set ui.language zh-Hans
```

Sensitive values are redacted in `get`, `list`, Doctor, and logs.

## Human-interface language

The only product languages are:

```text
en
zh-Hans
```

Persist one with:

```toml
[ui]
language = "en"
```

Resolution is `--language`, then `[ui].language`, then the first-run bilingual
choice; a fresh noninteractive environment defaults to English. Locale changes
human projection only. It does not change the system prompt, model route,
commands, paths, code, diffs, stable error codes, or canonical machine
protocols.

## Runtime and presentation

```toml
output_mode = "text"
verbosity = "normal"
log_level = "info"
telemetry = false
allow_shell = true
reasoning_effort = "max"
```

Environment overrides:

```text
DSE_OUTPUT_MODE
DSE_VERBOSITY
DSE_LOG_LEVEL
DSE_TELEMETRY
DSE_ALLOW_SHELL
```

### Run permissions

DSE has exactly three Run-frozen presets:

| Interactive label | Protocol value | Behavior |
| --- | --- | --- |
| Ask for approval | `ask` | Workspace work proceeds; typed external-path/recognized-network calls fail closed and elevated calls ask. |
| Agent decides | `agent` | External/network work may proceed; Host-classified critical calls ask. |
| Full access | `full_access` | No dynamic prompt; explicit deny and hard safety invariants still apply. |

The interactive TUI starts at Ask. `/permissions` or the permission chip
changes only future Runs in the current process. `dse exec --auto` selects
Agent decides; a required approval fails closed in headless execution.
The explicit process-only `--yolo` override selects Full access and is never
persisted.

There is no `[permissions]` table, Custom preset, free-form permission
combination, or config editor. Retired `approval_policy`, `sandbox_mode`,
`auto_approve`, `trust_mode`, and `allow_sandbox_elevation` inputs fail closed.
Permission choice does not change the DeepSeek model, prompt, or Host
completion evidence.

## Retry policy

Model-request retries are not configurable. `AgentRuntime` owns one bounded
policy: an initial request plus at most two replay-safe retries, with durable
1-second then 2-second backoff and a typed `Retry-After` extension when a
rate-limited response supplies it. DeepSeek transport performs exactly one
physical attempt per Runtime request. Partial output, usage, an in-flight
crash, unknown billing, or other replay-unsafe evidence stops automatic
resending. The retired `[retry]` table and
`dse app-server --transport-max-retries` fail closed instead of being accepted
as inert settings.

## TUI settings

```toml
[tui]
alternate_screen = "auto"
mouse_capture = true
terminal_probe_timeout_ms = 250
stream_chunk_timeout_secs = 900
osc8_links = true
```

These values change terminal behavior only. The interactive TUI remains a thin
client of the canonical application/runtime/store.

Compatibility and model-visibility settings such as `low_motion`,
`synchronized_output`, `bracketed_paste`, and `show_thinking` live in
`~/.dse/settings.toml`; see
[ACCESSIBILITY.md](ACCESSIBILITY.md).

## Skills, instructions, and tools

```toml
skills_dir = "~/.dse/skills"
instructions = ["~/.dse/AGENTS.md"]

[skills]
scan_dse_only = false

[tools]
always_load = []
```

Environment override:

```text
DSE_SKILLS_DIR
```

Skills use progressive disclosure. Their startup projection contains metadata;
the body is read on demand. Tool names are selected from the fixed Host-owned
catalog, not from a model Provider.

## MCP

```toml
mcp_config_path = "~/.dse/mcp.json"
mcp_oauth_callback_port = 8765
# mcp_oauth_callback_url = "http://127.0.0.1:8765/callback"
```

Environment override:

```text
DSE_MCP_CONFIG
```

MCP server definitions and transport credentials live in the referenced JSON
file. MCP OAuth authenticates that optional transport, not the DeepSeek model
backend. See [MCP.md](MCP.md).

## Subagents

```toml
[features]
subagents = true

[subagents]
enabled = true
max_concurrent = 4
max_depth = 3
```

Root, read-only child, and explicitly admitted Writer children use the same
`AgentRuntime`. A Writer still requires explicit admission and an isolated Git
worktree. No configuration enables default multi-Writer or a second runtime.

## Web retrieval

The canonical `web_fetch` tool reads a user-supplied public HTTP(S) URL and has
no selectable provider. Public HTTP is limited to port 80, remains
`external_untrusted`, and is returned with explicit plaintext/unprotected
transport provenance; HTTPS downgrade redirects are rejected. DSE does not currently expose a production
`web_search` tool. The former `[search]` table and `DSE_SEARCH_*` environment
variables had no executor and are rejected instead of being silently treated
as an Agent capability.

## Named profiles

Named profiles may override non-model local/TUI settings:

```toml
[profiles.quiet]
verbosity = "concise"
allow_shell = false
```

They cannot contain `api_key`, `base_url`, `default_text_model`, `provider`, or
`providers`.

## Project-local configuration

A trusted workspace may contain:

```text
$WORKSPACE/.dse/config.toml
```

Project-local configuration is untrusted input. It may set safe local tool and
presentation values. It cannot select a permission mode or change credentials,
the DeepSeek endpoint, model identity, telemetry, UI language, or global
extension paths. A permission key is rejected rather than treated as a hidden
fourth mode.

## Deleted and rejected keys

These root keys fail closed rather than being ignored:

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
fleet
approval_policy
sandbox_mode
auto_approve
trust_mode
allow_sandbox_elevation
permissions
```

Replaced camel-case spellings such as `apiKey`, `baseUrl`, and
`defaultTextModel` are also rejected. `DSE_PROVIDER` and
`DEEPSEEK_PROVIDER` fail closed. There is no compatibility reader or dual
write.

## Atomicity and recovery

Configuration writes are owner-only, atomic, preserve comments where possible,
and create one `.bak` before replacing an existing file. Config and state paths
reject unsafe symlinks, injected absolute subdirectories, and `..` traversal.

Do not edit individual SQLite event or snapshot rows. Back up `state.db` plus
its `-wal` and `-shm` siblings together before investigating persistent state.

## Minimal setup

```bash
dse login
dse --language en
```

The generated configuration can be as small as:

```toml
default_text_model = "deepseek-v4-pro"

[ui]
language = "en"
```

`dse doctor` reports the resolved fixed DeepSeek route, credential source
class, redacted endpoint, model, TLS posture, paths, and canonical runtime
facts without making an extra model request.
