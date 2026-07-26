# Repository Agent Guidance

This is DSE's stable work directory. Milestone history, implementation
inventories, and evaluation results belong in the linked authorities, not here.

## Read first

Read completely, in order, before changing the repository:

1. [Product plan](docs/product/PRODUCT_PLAN.md) — fixed scope and architecture.
2. [Accepted decisions](docs/decisions/) — long-lived decisions and supersession.
3. [Roadmap](docs/product/ROADMAP.md) — current milestone and deletion plan.
4. [Evaluation](docs/product/EVALUATION.md) — evidence needed to keep a capability.
5. [Current architecture](docs/architecture/CURRENT_CODEWHALE.md) — current facts,
   not target design.

The product plan and accepted ADRs win over older documentation.

## Product and architecture boundary

Optimize for `verified task success / tokens / time / code complexity`. DSE is
Rust-native, local-first, DeepSeek-only, and built from one source tree. Do not
add another Provider, TypeScript runtime, sidecar, cloud platform, marketplace,
chat bridge, source splice, or speculative compatibility layer.

- One DeepSeek backend.
- One `AgentRuntime` for root and child agents.
- One canonical runtime event protocol and one `RunStore`.
- Multi-agent is `AgentRuntime x N + Orchestrator`.
- Writers use worktrees and converge through diff/review/verify/merge.
- CLI, TUI, and local API are clients of the same application service.
- Completion requires evidence for the latest workspace revision.
- A replacement slice deletes its old path after cutover.

Changing one of these constraints requires evidence and a new ADR.

## Owner map

| Concern | Owner |
|---|---|
| Composition and fixed actor routing | `crates/app` |
| Agent loop, completion, replay, children | `crates/runtime` |
| Commands, events, tool outcomes | `crates/protocol` |
| DeepSeek plan/transport/parser/accounting | `crates/deepseek` |
| Prompt, project context, compaction | `crates/context` |
| Tool catalog, edits, shell, verifier adapter | `crates/tools` |
| SQLite events, snapshots, reopen | `crates/state` |
| Task graph and Writer worktrees | `crates/orchestrator` |
| Human messages and locale | `crates/localization` |
| Human/API projections | `crates/cli`, `crates/tui`, `crates/app-server` |

Current versions, cutovers, rejected candidates, and frozen evidence live in
the authority documents and `eval/`, not in this guide.

## Development method

Each implementation slice must state:

1. real problem and measurable acceptance;
2. single owning module;
3. old path replaced;
4. tests and evaluation evidence;
5. cutover deletion.

Build contract/test, the smallest vertical implementation, real caller
migration, old-path deletion, then benchmark and authority updates. Temporary
adapters last at most one roadmap milestone and need a deletion point. Do not
create empty crates or speculative Manager/Factory/Service traits.

## Repository work conventions

- Confirm branch and `git status --short` before editing; use `rg`/`rg --files`.
- Never use broad `git clean`, `git restore`, reset, or file moves to tidy a
  dirty tree.
- Preserve existing user and agent changes outside the active slice.
- Keep each commit to one reviewable concern; use WIP wording if unverified.
- Do not push, release, force-push, or repoint remotes without explicit request.
- Keep credentials, logs, targets, generated output, and runtime state untracked.
- Never rewrite frozen manifests, summaries, raw, or Git history.
- Do not mix branding, UI polish, Provider deletion, and Runtime refactoring.

## DeepSeek and Runtime rules

Audit against the official DeepSeek protocol, not generic OpenAI assumptions:
ordinary Chat/tools are standard Chat; Strict is Beta Chat and requires the
whole catalog; one incompatible tool causes lossless Standard fallback; FIM is
a separate Beta Completions surface; cache depends on stable prefixes;
reasoning/tool history replays exactly; SSE, finish, usage, retry, limits, and
partial responses are typed outcomes. Changing models, limits, prices, or Beta
behavior requires current official fixtures.

- Keep canonical transcript separate from per-request projection.
- Root and child agents must eventually pass the same conformance suite.
- A model proposes completion; the Host accepts a terminal state.
- `ToolOutcome` separates invocation, operation, retry, evidence, and artifact.
- Model self-review is advisory without deterministic evidence.
- Roles are profiles, not runtimes; read-only agents may share a view and
  writers require worktrees.
- Do not add free-chat swarm behavior or duplicate team tools.

## Validation

Focused gate:

```bash
./scripts/dev-dse.sh focused
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
git diff --check
```

Credentialed DeepSeek tests are opt-in and cost-bounded; cover protocol behavior
with offline fixtures first.

## Documentation discipline

- Update `PRODUCT_PLAN.md` only for accepted scope/architecture changes.
- Update `ROADMAP.md` for milestone status and execution adjustments.
- Update `EVALUATION.md` for task, metric, and keep/delete evidence.
- Add an ADR only for a long-lived architectural decision.
- Do not create parallel roadmaps, handoffs, trackers, or speculative designs.
