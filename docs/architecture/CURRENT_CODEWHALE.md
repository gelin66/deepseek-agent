# Current CodeWhale Architecture (Migration Snapshot)

> Category: migration evidence. This describes the imported implementation,
> not the accepted target architecture.

This document describes the implementation that exists today. It is not the
target product architecture. The accepted target, migration order, and ability
gates live in [PRODUCT_PLAN.md](../product/PRODUCT_PLAN.md),
[ROADMAP.md](../product/ROADMAP.md), and
[EVALUATION.md](../product/EVALUATION.md).

- Snapshot date: 2026-07-16
- Imported baseline: `352e86a611fdf3cd8bd27c36d24d482c06a71117`
- Workspace version: `0.8.68`

The file remains useful during migration because it records which CodeWhale
capabilities are currently wired. Sections must be removed or updated when the
corresponding old path is deleted; they must not be treated as justification
for preserving duplicate runtimes or product concepts.

Current boundary note:
- `codewhale exec` runs through the UI-independent
  `crates/runtime::AgentRuntime`. Its concrete DeepSeek, tool, persistence, and
  output composition remains in `crates/tui` while that production entry is
  migrated vertically. Official request planning plus physical request/usage
  accounting already live in `crates/deepseek`; HTTP/SSE and the concrete
  `DeepSeekModelPort` still live in `crates/tui`.
- Production `exec` persists schema-v3 canonical runtime events through the
  schema-v6 `crates/state::StateStore` SQLite `RunStore`.
- The interactive TUI, runtime API, app-server, and task manager have not
  migrated. Their live loops and private runtime/session/task state still live
  under `crates/tui`; they must not be described as projections of the new
  `RunStore` yet.
- Root and child runs inside the new `AgentRuntime` share one execution
  implementation. The unmigrated TUI child-agent path still has different
  execution semantics.
- `crates/core` is not the production model loop.
- The LSP subsystem (`crates/tui/src/lsp/`) is fully wired into the engine's post-tool-execution path
  (`core/engine/lsp_hooks.rs`), providing inline diagnostics after every edit_file/apply_patch/write_file.
- The swarm agent system was removed in v0.8.5. The active sub-agent surface is the single `agent` tool; persistent RLM sessions remain available through `rlm_open` / `rlm_eval` / `rlm_configure` / `rlm_close`.
  No model-visible swarm tool remains in the active codebase.

## High-Level Overview

```text
codewhale exec
  -> crates/tui exec composition + output projection
  -> crates/runtime::AgentRuntime
       -> TUI DeepSeekModelPort -> TUI HTTP/SSE client
            -> crates/deepseek request plan + shared physical accounting
       -> ProductionToolExecutor -> fixed 11-tool catalog
       -> crates/state::StateStore as SQLite RunStore
            -> canonical events -> reducer/snapshot -> terminal replay

interactive TUI / runtime API / app-server / task manager
  -> legacy crates/tui engine, runtime_threads, session and task state
```

This is deliberately a split migration snapshot, not the target architecture.
Only `exec` has crossed the new runtime/persistence boundary. The second path is
scheduled for later vertical cutovers; its existence does not mean a second
runtime or state truth is accepted as the final design.

## Module Organization

### Entry Point

- **`main.rs`** - CLI argument parsing (clap), configuration loading, entry point routing

### Core Components

- **`core/`** - Legacy interactive TUI/app-server engine components; this is
  not the production `codewhale exec` loop
  - `engine.rs` - Engine state, operation handling, message processing
  - `engine/turn_loop.rs` - Streaming turn loop and tool execution orchestration
  - `session.rs` - Session state management
  - `turn.rs` - Turn-based conversation handling
  - `events.rs` - Event system for UI updates
  - `ops.rs` - Core operations

### Configuration

- **`config.rs`** - Configuration loading, profiles, environment variables
- **`settings.rs`** - Runtime settings management

### Workspace Crates

- **`crates/tools`** - Shared tool invocation primitives and capabilities. It
  re-exports the canonical `ToolOutcome`; the fixed `exec` handlers are still
  physically composed from `crates/tui/src/tools` during this migration.
- **`crates/agent`** - Model/provider registry (ModelRegistry) for resolving model IDs to provider endpoints.
- **`crates/app-server`** - HTTP/SSE + JSON-RPC app server transport for
  headless workflows. It has not migrated to the new `AgentRuntime`/`RunStore`.
- **`crates/config`** - Config loading, profiles, environment variable precedence, CLI runtime overrides.
- **`crates/core`** - Older application/session scaffolding still used by
  unmigrated callers; it is not the production `exec` model loop.
- **`crates/execpolicy`** - Approval/sandbox policy engine for tool execution decisions.
- **`crates/hooks`** - Lifecycle hooks (stdout, jsonl, webhook) for pre/post tool events.
- **`crates/mcp`** - MCP client + stdio server for Model Context Protocol tool servers.
- **`crates/protocol`** - Request/response framing plus schema-v3 canonical
  `AgentRuntime` events, transcript, `ToolOutcome`, accounting, and terminal
  types.
- **`crates/runtime`** - The one UI/HTTP/database-independent root/child Agent
  execution kernel, canonical reducer, small ports, and test-only in-memory
  `RunStore`.
- **`crates/deepseek`** - Official Standard/Strict/FIM request planning plus
  the shared physical request budget, root/child attribution, exact usage
  completeness ledger, surface buckets, and official V4 first-party pricing.
  It has no TUI/config/tool dependency; HTTP/SSE has not migrated into it yet.
- **`crates/secrets`** - OS keyring integration for API key storage.
- **`crates/state`** - Schema-v6 SQLite state database. It implements the
  production canonical `RunStore` for `exec` alongside still-unmigrated legacy
  thread/session tables.
- **`crates/workflow`** / **`crates/workflow-js`** - Workflow engine and its
  QuickJS scripting layer (renamed from the whaleflow crates).
- **`crates/lane`** - Lane runtime: durable, attachable running instances of
  Fleet/Workflow work (`codewhale lane list/status/attach/logs/stop`).
- **`crates/release`** / **`crates/build-support`** - Release checks and build
  plumbing.

### LLM Integration

- **`crates/deepseek`** - Pure official DeepSeek planner and physical
  request/usage accounting owner for Standard Chat, Beta Strict Chat, and
  Beta FIM
- **`client.rs` / `client/chat.rs`** - Still-owning production HTTP/SSE client
  and response parser used by the `AgentRuntime` DeepSeek adapter
- **`client/deepseek.rs`** - Temporary TUI projection adapter into
  `crates/deepseek`; it is not a second planner
- **`llm_client.rs`** - Abstract LLM client trait with retry logic
- **`models.rs`** - Data structures for API requests/responses

#### DeepSeek API Endpoints

The official DeepSeek planner currently owns these distinct surfaces:

- ordinary Chat and ordinary tool calls use Standard
  `/chat/completions`;
- only a whole tool catalog that is strict-compatible uses Beta Strict
  `/beta/chat/completions`;
- if any tool is incompatible, the entire catalog falls back to ordinary tool
  calling without dropping tools;
- FIM is a separate Beta Completions request at `/beta/completions`;
- model discovery/health checks use the models endpoint independently of the
  selected Chat surface.

An official-base `RequestPlan` fixes the surface, URL, wire model, response
mode, exact reasoning replay, and request body before transport. Custom bases
and path suffixes still have legacy compatibility branches pending later
provider cleanup; they are not evidence that ordinary official Chat should use
the Beta endpoint.

### Tool System

Production `codewhale exec` exposes this fixed audited catalog:

```text
apply_patch  edit_file    exec_shell   file_search
git_diff     git_status   grep_files   list_dir
read_file    run_tests    run_verifiers
```

All eleven handlers return the canonical `ToolOutcome`, which separately
records invocation, transport, operation, side-effect, retry, evidence,
artifact, and workspace-revision state. `run_tests` and `run_verifiers` may
produce revision-bound evidence/artifact descriptors, but this is not yet the
M5 production `EvidenceReceipt`/Host-completion migration. Wire-level names
such as `ContentBlock::ToolResult` and the stable NDJSON `tool_result` event do
not represent a second domain result type.

- **`tools/`** - Built-in tool implementations; the broad registry below is
  still used by the unmigrated interactive runtime, while `exec` selects only
  the fixed catalog above
  - `mod.rs` - Tool registry and common types
  - `shell.rs` - Shell command execution
  - `file.rs` - File read/write operations
  - `todo.rs` - Checklist tools plus legacy todo aliases
  - `tasks.rs` - Model-visible durable task, gate, background shell, and PR-attempt tools
  - `github.rs` - Read-only GitHub context and guarded comment/closure tools backed by `gh`
  - `automation.rs` - Model-visible scheduling tools over `AutomationManager`
  - `plan.rs` - Planning tools
  - `subagent.rs` - Persistent sub-agent sessions
  - `spec.rs` - Tool specifications
  - `rlm.rs` - Persistent Recursive Language Model (RLM) sessions — sandboxed Python REPLs with semantic helper calls and `var_handle` output support

### Extension Systems

- **`mcp.rs`** - Model Context Protocol client for external tool servers
- **`skills.rs`** - Plugin/skill loading and execution
- **`hooks.rs`** - Pre/post execution hooks with conditions

### User Interface

- **`tui/`** - Terminal UI components (ratatui-based)
  - `app.rs` - Application state and message handling
  - `ui.rs` - Event handling, streaming state, and rendering logic
  - `approval.rs` - Tool approval dialog
  - `clipboard.rs` - Clipboard handling
  - `streaming.rs` - Streaming text collector

- **`ui.rs`** - Legacy/simple UI utilities

### LSP Integration

- **`lsp/`** - Post-edit diagnostics injection (#136)
  - `mod.rs` - `LspManager` — lazy per-language transport pool + config
  - `client.rs` - `StdioLspTransport` — JSON-RPC over stdio with `didOpen`/`didChange`/`publishDiagnostics`
  - `diagnostics.rs` - Diagnostic types, severity, and HTML-block renderer
  - `registry.rs` - Language detection and default server map (rust-analyzer, pyright, gopls, clangd, typescript-language-server, jdtls, vue-language-server)
  - Wired into the engine via `core/engine/lsp_hooks.rs` — called after every successful edit

### Security

- **`sandbox/`** - platform sandbox policy preparation and denial reporting
  - `mod.rs` - Sandbox type definitions
  - `policy.rs` - Sandbox policy configuration
  - `seatbelt.rs` - macOS Seatbelt profile generation
  - `landlock.rs` - Linux Landlock detection and future helper contract
  - `windows.rs` - Windows helper contract; not advertised until a Job
    Object process-containment helper exists

### Utilities

- **`utils.rs`** - Common utilities
- **`logging.rs`** - Logging infrastructure
- **`compaction.rs`** - Context compaction for long conversations
- **`purge.rs`** - Agent-driven context purging (surgical message removal/rewriting)
- **`pricing.rs`** - Cost estimation
- **`prompts.rs`** - System prompt templates
- **`project_doc.rs`** - Project documentation handling
- **`session.rs`** - Session serialization
- **`runtime_api.rs`** - HTTP/SSE runtime API (`codewhale serve --http`)
- **`runtime_threads.rs`** - Durable thread/turn/item store + replayable event timeline
- **`task_manager.rs`** - Durable queue, worker pool, task timelines and artifacts

## Data Flow

### Headless `codewhale exec`

1. CLI parsing resolves a new prompt or an exact `--resume <run_id>` request.
2. The entry opens `StateStore` as the production `RunStore` before creating
   the model client or Runtime.
3. A new run durably appends `RunCreated`; resume loads the same canonical
   event log and reducer snapshot.
4. The composition creates one `AgentRuntime` with `DeepSeekModelPort`, the
   fixed `ProductionToolExecutor`, the SQLite Store, and an output event sink.
5. External actions use store-first phase events: model
   `prepared -> in_flight -> response_committed`, and tool
   `prepared -> execution_started -> outcome_committed`.
6. Canonical events are committed before the output projection observes them.
   Transcript, usage/accounting, pending action state, tool artifacts, and the
   terminal outcome are rebuilt by the shared reducer.
7. SQLite enforces one canonical terminal event per run. Reopening a terminal
   run replays it rather than starting another model request.

### Legacy Interactive Session

1. User input received in TUI
2. Input processed by `core/engine.rs`
3. Message sent to LLM via `llm_client.rs`
4. Response streamed back, parsed in `client.rs`
5. Tool calls extracted and executed via `tools/`
6. Hooks triggered before/after tool execution
7. Results aggregated and sent back to LLM
8. Final response rendered in TUI

### Headless RunStore Recovery

- Events have per-run monotonic sequences and idempotent event IDs. A lease
  epoch fences stale writers; a second live process cannot concurrently resume
  the same non-terminal run.
- A prepared action that has not crossed the external side-effect boundary can
  continue after reopen. An in-flight model request or tool side effect is
  ambiguous and fails closed as typed recovery-required state; it is not
  silently reissued.
- A committed model response resumes from its durable transcript and usage
  without duplicating the request, assistant entry, or accounting.
- A committed terminal is immutable. It can be replayed without an API Key,
  without model I/O, and without appending a second terminal or other event.
- Workspace, provider, tool-catalog, or execution-fingerprint mismatch fails
  before model I/O rather than resuming under different semantics.
- “Exactly once” refers to the durable canonical terminal. stdout/NDJSON is a
  replayable projection and may be delivered again when a completed run is
  explicitly resumed.

### Legacy TUI Crash Recovery + Offline Queue

1. Before sending user input, the TUI writes a checkpoint snapshot to `~/.codewhale/sessions/checkpoints/latest.json`
2. Startup remains fresh by default; prior sessions are resumed explicitly via `--resume`/`--continue` (or `Ctrl+R` in TUI)
3. While degraded/offline, new prompts are queued in-memory and mirrored to `~/.codewhale/sessions/checkpoints/offline_queue.json`
4. Queue edits (`/queue ...`) are persisted continuously so drafts and queued prompts survive restarts
5. Successful turn completion clears the active checkpoint and writes a durable session snapshot
6. Agent/Yolo turns also take pre/post-turn side-git workspace snapshots under `~/.codewhale/snapshots/<project_hash>/<worktree_hash>/.git`; `/restore N` and `revert_turn` restore file state without changing conversation history or the user's `.git`

### Headless Tool Execution

1. `AgentRuntime` durably records the canonical invocation and operation ID.
2. `ProductionToolExecutor` validates exact membership in the fixed 11-tool
   catalog and executes through the existing audited handler.
3. The handler returns one typed `ToolOutcome`; it is not converted through an
   old domain `ToolResult`.
4. Runtime commits the outcome, transcript projection, revision/evidence, and
   artifact descriptors before the next model request can observe them.
5. If a process dies after execution starts but before the outcome is durable,
   recovery records ambiguity and does not repeat a potentially applied side
   effect.

### Legacy TUI Tool Execution

1. LLM requests tool via `tool_use` content block
2. Tool registry looks up handler
3. Pre-execution hooks run
4. Approval requested if needed (non-yolo mode)
5. Tool executed (possibly sandboxed on macOS)
6. Post-execution hooks run
7. Result metadata is retained on runtime item records
8. **LSP post-edit hook**: if the tool was `edit_file`/`apply_patch`/`write_file` and LSP is enabled, the engine runs `run_post_edit_lsp_hook()` to collect diagnostics
9. **Diagnostics flush**: before the next API request, `flush_pending_lsp_diagnostics()` injects any collected errors as a synthetic user message
10. Result returned to agent loop

### Legacy TUI/app-server Background Tasks

1. Client enqueues task (`/task add ...` or `POST /v1/tasks`)
2. `task_manager.rs` persists task + queue entry under `~/.codewhale/tasks`
3. Worker picks queued task (bounded pool), transitions to `running`
4. Task creates/uses a runtime thread and starts a runtime turn
5. `runtime_threads.rs` persists thread/turn/item records + monotonic event sequence
6. Timeline/tool summaries/artifact references are persisted incrementally
7. Checklist state, verifier gates, PR attempts, and guarded GitHub events are applied from tool metadata to the active task
8. Final state (`completed|failed|canceled`) is durable and queryable via TUI/API

Within the legacy path, model-visible durable task tools are a surface over this
same manager: `task_create` enqueues normal tasks, `checklist_*` updates
task-local progress, `task_gate_run` and completed `task_shell_wait` attach
verification evidence, and automation runs enqueue ordinary durable tasks.
Relative to the new `AgentRuntime`, however, this manager and its files remain
an unmigrated state truth; M4-A does not claim they are canonical projections.

### Legacy Runtime Thread/Turn Timeline

1. API/TUI creates or resumes a thread (`/v1/threads*`)
2. Turn starts on the thread (`/v1/threads/{id}/turns`)
3. Engine events are mapped to item lifecycle events (`item.started|item.delta|item.completed`)
4. Interrupt/steer operations apply to the active turn only
5. Compaction (auto/manual) is emitted as `context_compaction` item lifecycle
6. Purge (agent-driven) is emitted as `context_purge` item lifecycle
7. Clients replay history and resume with `/v1/threads/{id}/events?since_seq=<n>`

These private item lifecycle events are not schema-v3 canonical
`RuntimeEvent`s. The app-server and TUI cutovers must replace this translation
and state path rather than bridge it permanently.

### Durable Schema Gates

- The SQLite `StateStore` is at schema version 6. Its `agent_run_events` rows
  carry canonical AgentRuntime event schema version 3; a unique terminal index,
  event IDs, sequence checks, writer leases, and reducer validation fail closed
  on conflicts or corrupt replay.
- The canonical event log is append-only. `agent_runs` and
  `agent_run_snapshots` are derived projections updated transactionally for
  acquisition and fast reads; they are not independent semantic truth.
- `session_manager.rs`, `runtime_threads.rs`, and `task_manager.rs` embed `schema_version` on persisted records.
- On load, newer schema versions are rejected with explicit errors instead of silently truncating/overwriting data.
- This allows safe forward migrations and prevents corruption when binaries and stored state are out of sync.

## Extension Points

### Adding a New Tool

1. Create handler in `tools/`
2. Register in `tools/registry.rs`
3. Add tool specification (name, description, input schema)

### Adding an MCP Server

1. Configure in `~/.codewhale/mcp.json`
2. Server auto-discovered at startup
3. Tools exposed to LLM automatically

### Creating a Skill

1. Create skill directory with `SKILL.md`
2. Define skill prompt and optional scripts
3. Place in `~/.codewhale/skills/`

### Adding Hooks

Configure in `~/.codewhale/config.toml`:

```toml
[[hooks]]
event = "tool_call_before"
command = "echo 'Running tool: $TOOL_NAME'"
```

## Key Design Decisions

1. **Streaming-first**: All LLM responses stream for responsiveness
2. **Tool safety**: Non-YOLO mode requires approval for destructive operations, including side-effectful MCP tools
3. **Extensibility**: MCP, skills, and hooks allow customization without code changes
4. **Cross-platform**: Core works on Linux/macOS/Windows. Sandbox guarantees
   are platform-specific: macOS Seatbelt is the active policy path; Linux and
   Windows require helper enforcement before they should be treated as full OS
   sandboxing.
5. **Minimal dependencies**: Careful dependency selection for build speed
6. **Local-first runtime API**: HTTP/SSE endpoints are intended for trusted localhost access and are served by the `crates/tui` runtime today

## Configuration Files

- `~/.codewhale/config.toml` - Main configuration (`~/.deepseek/config.toml` is still read as a legacy fallback)
- `~/.codewhale/state.db` - Default SQLite state database. For `exec`, its
  schema-v6 Agent run tables are the canonical event/replay truth; legacy
  tables used by unmigrated callers still coexist in the same database.
- `$CODEWHALE_HOME/state.db` - Exact database location when
  `CODEWHALE_HOME` overrides the application data root; no extra
  `.codewhale/` component is inserted.
- `/etc/deepseek/managed_config.toml` - Optional managed defaults layer (Unix)
- `/etc/deepseek/requirements.toml` - Optional allowed-policy constraints (Unix)
- `~/.codewhale/mcp.json` - MCP server configuration
- `~/.codewhale/skills/` - User skills directory
- `~/.codewhale/sessions/` - Session history
- `~/.codewhale/sessions/checkpoints/` - Crash checkpoint + offline queue persistence
- `~/.codewhale/snapshots/` - Side-git pre/post-turn workspace snapshots for `/restore` and `revert_turn`
- `~/.codewhale/tasks/` - Background task records, queue, timelines, artifacts
- `~/.codewhale/audit.log` - Append-only audit events for credential + approval/elevation actions
