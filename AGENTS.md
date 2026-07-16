# Repository Agent Guidance

## Read first

Before changing this repository, read in order:

1. `docs/product/PRODUCT_PLAN.md` — fixed product scope and architecture.
2. `docs/decisions/` — accepted long-term decisions.
3. `docs/product/ROADMAP.md` — current milestone, migration, and deletion plan.
4. `docs/product/EVALUATION.md` — evidence required to keep a capability.
5. `docs/architecture/CURRENT_CODEWHALE.md` — current implementation facts,
   not the target design.

If older CodeWhale docs conflict with these files, the product plan and ADRs win.

## Product north star

This repository is becoming a Rust-native, local-first coding agent dedicated
to the official DeepSeek API. CodeWhale is the sole source base. Cline and
other agents are capability references only; do not add a TypeScript runtime,
sidecar, source-code splice, or permanent bridge.

Optimize for:

```text
verified task success / tokens / time / code complexity
```

Do not treat more abstractions, tools, modes, or lines of code as progress.

## Fixed architecture constraints

- One DeepSeek backend.
- One `AgentRuntime` for root and child agents.
- One canonical runtime event protocol.
- One `RunStore` as persistent truth.
- Multi-agent is `AgentRuntime x N + Orchestrator`.
- Writing agents use worktrees and converge through diff/review/verify/merge.
- CLI, TUI, and local API are thin clients of the same application service.
- Completion depends on evidence for the latest workspace revision.
- A replacement slice deletes its old path after cutover.
- No new Provider ecosystem, cloud platform, marketplace, or chat bridge.

Changing one of these constraints requires evidence and a new ADR.

## Current repository truth

- Imported CodeWhale baseline: `352e86a611fdf3cd8bd27c36d24d482c06a71117`.
- `codewhale exec` and `codewhale app-server` now share
  `crates/app::AgentApplication`, `crates/runtime::AgentRuntime`, the fixed
  `crates/tools` catalog, `crates/deepseek::DeepSeekModelPort`, and the SQLite
  `RunStore` implemented in `crates/state`.
- app-server is only an HTTP/SSE/stdio projection of the canonical Run API. It
  has no `core`/`tui` dependency, private lifecycle store, model loop, tool
  implementation, or sibling TUI process.
- `crates/core`, the fake prompt loop, raw model proxy, direct tool route, and
  the retired remote/mobile bridge chain have been deleted.
- The interactive TUI has not migrated. Its live engine and private
  session/task/runtime-thread state remain under `crates/tui` and are the M4-C
  replacement target; do not route new callers through them.
- Root and child runs inside `AgentRuntime` use the same execution
  implementation. The unmigrated interactive TUI child-agent path still has
  different execution semantics and is the M4-C replacement target.
- Existing DeepSeek work was preserved in WIP commit `2ccccdd4` and local
  branch `archive/pre-product-plan-20260715`.
- That WIP is not automatically accepted as stable behavior. It must be split
  into protocol, agent reliability, verify experiment, and local-development
  slices and evaluated independently.
- `crates/tui/src/tools/verify.rs` is wired production code, not disposable
  scratch. It is also an unproven model-critic experiment, not equivalent to
  deterministic test evidence.

Never use broad `git clean`, `git restore`, reset, or file moves to make a
dirty tree look tidy. Inspect consumers with `rg`, preserve unrelated changes,
and remove code only after its replacement is running.

## Development method

Each implementation slice must state:

1. the real problem;
2. acceptance criteria;
3. the single owning module;
4. the old path it replaces;
5. tests and evaluation evidence;
6. what is deleted at cutover.

Build vertical behavior before broad module moves. A normal slice is:

```text
contract/test
  -> implementation
  -> real caller migration
  -> old path deletion
  -> benchmark and docs update
```

Temporary adapters may span at most one roadmap milestone and must have a
named deletion point. Do not create empty crates or speculative traits to make
the directory tree resemble the target architecture.

## DeepSeek protocol rules

Audit against the official DeepSeek protocol, not generic OpenAI assumptions:

- ordinary Chat and ordinary tool calls are standard Chat capabilities;
- Strict Function Calling is a Beta Chat capability and requires the whole
  request tool catalog to be strict-compatible;
- if one tool is incompatible, fall back to ordinary tool calling without
  dropping tools;
- FIM is a distinct Beta Completions surface;
- context cache is automatic for ordinary Chat and depends on stable prefixes;
- replay assistant reasoning and tool-call history exactly as required;
- finish reason, SSE errors, usage, retries, and output limits are typed
  protocol outcomes, not warnings hidden in text.

Transient model names, prices, limits, and Beta behavior must be covered by
fixtures and revalidated with official documentation before changing them.

## Runtime and multi-agent rules

- Keep canonical transcript separate from per-request projection.
- Root and child agents must eventually pass the same conformance suite.
- A model may propose completion; the host accepts a terminal state.
- `ToolOutcome` must distinguish invocation, operation, retry, evidence, and
  artifact state.
- Model self-review is advisory unless backed by deterministic evidence.
- Agent roles are profiles, not different runtimes.
- Read-only agents may share a read view; writing agents require worktrees.
- Do not copy Cline-style team tool proliferation or free-chat swarm behavior.

## Repository work conventions

- Confirm `git branch --show-current` and `git status --short` before editing.
- Use `rg`/`rg --files` for discovery.
- Preserve existing user and agent changes that are outside the active slice.
- Keep a commit to one reviewable concern and give it an honest body.
- Use WIP wording when behavior has not been verified.
- Do not push, release, force-push, or repoint remotes unless explicitly asked.
- Keep credentials, logs, generated outputs, and local runtime state untracked.
- Do not mix product branding, UI polish, provider deletion, and runtime
  refactoring in the same slice.

## Validation

Minimum for documentation-only work:

```bash
git diff --check
```

Focused current WIP gate:

```bash
./scripts/dev-deepseek-agent.sh focused
```

Targeted Rust work:

```bash
cargo fmt --all -- --check
cargo test -p <owning-crate> --locked <filter>
cargo check -p <owning-crate> --locked
```

Full pre-integration gate:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Credentialed DeepSeek tests are opt-in, cost-bounded canaries. Protocol
behavior must first be covered by offline fixtures.

## Documentation discipline

- Update `PRODUCT_PLAN.md` only for accepted scope or architecture changes.
- Update `ROADMAP.md` for milestone status and execution adjustments.
- Update `EVALUATION.md` for task, metric, and keep/delete evidence.
- Add an ADR only for a long-lived architectural decision.
- Do not create parallel roadmaps, handoffs, version trackers, or speculative
  design documents when an existing authority file can be updated.
