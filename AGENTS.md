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
- `codewhale exec`, `codewhale app-server`, and the retained interactive TUI
  foreground now share `crates/app::AgentApplication`,
  `crates/runtime::AgentRuntime`, the fixed `crates/tools` catalog,
  `crates/deepseek::DeepSeekModelPort`, and the SQLite `RunStore` implemented
  in `crates/state`.
- app-server is only an HTTP/SSE/stdio projection of the canonical Run API. It
  has no `core`/`tui` dependency, private lifecycle store, model loop, tool
  implementation, or sibling TUI process.
- `crates/core`, the fake prompt loop, raw model proxy, direct tool route, and
  the retired remote/mobile bridge chain have been deleted.
- The interactive TUI submits canonical Run commands through `TuiRunClient`
  and projects root/child facts through `CanonicalRunProjection`. Its old
  foreground Engine, private runtime-thread owners, `SessionManager`, child
  display cache, and registry-driven slash-command system have been deleted.
- M4 is closed at code checkpoint `65fa88ba`: hidden
  Workflow/ACP/direct-review model paths, the old TUI SubAgent runtime, Classic
  shell, duplicate tool/state/model owners, and unwired Goal/Memory facades
  have been physically deleted. Underwater is the sole interactive shell.
  Production root/child execution uses canonical `AgentRuntime` and `RunStore`.
- Current Run API is v12, State schema is v24, RuntimeEvent is v18, and the
  compact exec stream is v3. State v21 introduced exact advertised tool
  catalogs and typed tool-failure/retry truth; v22 added immutable Host route
  audit and retired materialized runs that could not reconstruct it. State v23
  deleted only the legacy `threads` metadata table. State v24 retires
  pre-v18 materialized runs whose Auto/omitted-reasoning wire plan cannot be
  mapped losslessly and preserves only v18-safe pending Start intents. No
  compatibility reader or dual write exists.
- M5-A established the only canonical TaskContract/EvidenceReceipt/Host
  completion owner. M5-B retained the evidence-aware ContextBroker and
  hard-limit local compaction, then deleted manual/early compaction, the
  independent compaction root, and the model-summary lifecycle at shrink
  checkpoint `e2c870b0`. Its formal DeepSeek A/B does not support an
  efficiency claim.
- M6-A established the unique production `Orchestrator` and a single
  isolated Writer worktree lifecycle at code/canary checkpoint `a982a9a8`.
  Root, read-only child, and Writer child use the same `AgentRuntime`; Host
  seals diff/outcome, verifies in the worktree, integrates fast-forward with
  Git CAS/lease, verifies the latest root revision, and cleans up. The live
  canary is mechanism evidence only. M6-B1's formal 18-pair/36-arm A/B decided
  `reject_and_rework`: keep the isolated mechanism and make Writer admission
  explicit-only. That cutover and verifier/evidence rework are complete, but
  the subsequent formal v3 stopped on unknown billing; Writer remains
  explicit-only and multi-Writer is not admitted.
- M7-A3 established typed incomplete-stream evidence, incremental usage
  accounting, and replay-safe retry at production checkpoint `e98ca5ae`.
  Focused, fmt, workspace clippy/test, crash/reopen, and Harness gates pass.
  A live M7-A reevaluation is inadmissible because the old v13/v18 control and
  v14/v19 treatment cannot receive a byte-equivalent v15/v20 correctness patch
  while preserving the original production delta. No Key or API was used;
  the M7-A product conclusion remains `hold`.
- M7-B kept the official Beta Strict planner, whole-catalog compatibility
  diagnostics, and atomic lossless Standard fallback. All six frozen default
  executable actor catalogs still select Standard Chat when Strict is
  requested, so the formal live A/B is `inadmissible_no_surface_delta`: no
  credential read, API request, release binary, or product metric. The inert
  user Strict toggle is deleted. RuntimeEvent v16/State v21 now require a
  stable failure code for every unsuccessful ToolOutcome, preserve exact
  actor tool/reasoning replay, and pass root/read-only/Writer plus SIGKILL
  recovery conformance. Do not weaken schemas, add a second wire catalog, or
  restore a Strict product mode to force Beta admission. Post-decision
  checkpoint `4e3536f1` removes redundant derived decision state and proves
  exact RequestPlan reconstruction after SQLite reopen without changing the
  frozen candidate, manifest, raw, or wire behavior.
- M8-B established the single CodeWhale product and local delivery identity at
  candidate `307f6c09`: the shipped binary set is exactly `codewhale` plus
  `codewhale-tui`, Rust is pinned to 1.97.0, and one locked/offline delivery
  owner binds full source/Cargo.lock/toolchain identity and inner/outer
  checksums before immutable install, atomic upgrade/rollback, or
  data-preserving uninstall. The real macOS release lifecycle and the same
  Linux fixture lifecycle pass with network denied. `crates/release`, imported
  updater/CNB discovery, `codew`, legacy product env/path readers, duplicate
  metrics state, and unconnected deployment assets are deleted. No model,
  Runtime, Store, or protocol surface changed; no Key or DeepSeek API was used.
- M8-I replaced the pre-run Flash prompt classifier with the Host-owned typed
  policy in `crates/app`. Root and explicit Writer remain Pro; an ordinary
  read-only child may use Flash and a typed recheck uses Pro. Every Run keeps
  immutable route audit in the canonical Runtime/Store. ADR-0008/M9-D later
  retired and deleted Auto as a product direction: model/reasoning Auto is no
  longer accepted by config, CLI, TUI, API, protocol or app policy. Omitted
  root uses Pro/high; fixed-profile read-only child uses Flash/high, Writer
  uses Pro/high, and typed recovery/recheck/rework uses Pro/max. Explicit
  Pro/Flash/reasoning remains exact and replayable. Do not restore Auto,
  classifiers, keyword routing, dynamic routers or an extra model request.
- M9-E bounded the billing-evidence audit to official documentation plus the
  existing corrected Harness and production accounting. Successful responses
  expose completion id/usage, but official balance and monthly per-Key export
  do not document request-level reconciliation, settlement bounds, or
  pre-header failure billing. The slice therefore closed before credential
  access as `infeasible_exact_pre-header_request_reconciliation_under_current_official_contract`.
  Keep `billing_unknown -> formal campaign stop`; changing that admission rule
  requires a separate ADR.
- M10-A proved the default project context pack duplicates the generated
  fallback overview offline, but its formal same-binary pack-on/off campaign
  stopped after 21 complete arms when arm 22 received headers/reasoning without
  finish or usage. The result is `stop_incomplete_accounting`, not a product
  comparison. Pack-off was not admitted: `context.project_pack`, its prompt
  bool/branch, and the M10-A-only Harness path are deleted; production stays
  fixed pack-on. The derived prompt ledger and frozen evidence remain. A
  frozen observer false-positive also showed that temporal recovery must accept
  the canonical committed Host `failed_write_pass` receipt, not demand a
  duplicate model-owned final verifier call.
- M10-B's deterministic budgeted Working-Set selector passed its offline
  localization benchmark, but the formal fixed-Pro campaign produced one
  treatment read-only actor-contract false-success label in the first five
  measurement-valid arms. The sixth arm then failed before response usage with
  `billing_unknown=true`. The decision is
  `reject_quality_veto_and_delete`, with acquisition
  `stop_incomplete_accounting`: selector, safe-read helper, production caller,
  temporary config/user surface, fixture/benchmark, and Harness treatment
  branch are deleted. Frozen manifests, ignored 0600 raw, Git history, and the
  decision summary remain audit evidence only. Production has no budgeted
  Working-Set map; do not restore it without a new independent slice.
- M10-C's request-local Acceptance Progress candidate passed its offline
  mechanism, replay, compaction, actor, and production-loopback tests, but
  failed the credential-front viability gate. Every measured model-request-
  visible state increased estimated context tokens (`235 -> 356` pending and
  `538 -> 547` verifier rejection); the only reduction was terminal-only, while
  the candidate added 1,501 net lines and current fixed-Pro already had two
  verified accounting-complete recovery samples. The decision is
  `reject_offline_viability_and_delete`: projection, prompt marker, config/user
  wiring, and treatment tests are physically deleted; no Key or API was used.
  Production has no acceptance-progress product branch or second plan truth.
- M10-D found no safe independent app-level recovery-controller delta.
  ToolOutcome feedback, tools side-effect/retry truth, Runtime verifier
  transitions and replay-safe retry, DeepSeek accounting, Harness billing
  stop, and fixed Pro/max recheck already have distinct canonical owners.
  Across the current M9-C/M10-A/M10-B frozen trajectories, eight verifier
  failures recovered through the existing path; the only other typed-failure
  arm failed its frozen child-call contract rather than lacking a recovery
  action. `context_missing`, `reasoning_insufficient`, and generic environment
  failure are not stable causal facts. The decision is
  `reject_no_safe_independent_controller_delta`: no production candidate, Key,
  API request, classifier, automatic rollback, or second loop was created.
- M8-J closed V12 at candidate `bcbc1616` by deleting the visible
  `codewhale thread` path, the SQLite `threads` table,
  `session_index.jsonl`, and the no-consumer Thread/App/Prompt/EventFrame
  protocol island. Canonical `runs`/`resume`, current run replay, pending Start,
  accounting, and `RunEnvironment.provider="deepseek"` replay safety remain.
  The current V1 successor matrix is 10 pass / 6 blocked; no Key or API was
  used for this deterministic deletion slice.
- M8-K accepted ADR-0005 at candidate `6a99cb79`: V1 now requires the complete
  lifecycle of one explicit isolated Writer, current Standard Chat plus Strict
  whole-catalog admission/lossless fallback, and bounded verified cross-file
  work through the canonical search/read/diff tools and ContextBroker.
  Multi-Writer, FIM, and a named RepoGraph implementation are evidence-gated
  post-V1 candidates, not literal V1 requirements. The current matrix is
  13 pass / 3 blocked; no production surface changed and no Key or API was
  used.
- M8-L accepted candidate `d27553c4` independently proved that imported
  `352e86a6` can share the official
  ChatCompletions/model, task fixture, external verifier, and binary identity,
  but cannot natively report physical request count, retry, incomplete usage,
  root/child ledger, cost completeness, or exact reopen truth. The paid
  cross-revision A/B is therefore
  `inadmissible_incomplete_baseline_accounting` before credential access.
  ADR-0006 retains the qualified M5-A 12/12 official DeepSeek coding/
  false-success evidence, adds exact-current production retention gates, and
  proves five common user workflows remain 5 -> 5 actions. V13/V16 are closed;
  the current matrix is 15 pass / 1 blocked, with only V15 remaining. This is
  not a current-vs-imported quality or cost claim.
- Existing DeepSeek work was preserved in WIP commit `2ccccdd4` and local
  branch `archive/pre-product-plan-20260715`.
- That WIP is not automatically accepted as stable behavior. It must be split
  into protocol, agent reliability, verify experiment, and local-development
  slices and evaluated independently.
- The orphan TUI `verify` model critic, `FimEditTool`, and the duplicate
  `RunTestsTool`/`RunVerifiersTool` wrappers have been physically deleted.
  Deterministic evidence remains in `crates/tools`. M8-H also deleted the
  unreachable DeepSeek FIM planner/surface/accounting half-branch after proving
  it had no canonical caller, sender ownership, response parser, Host apply, or
  reopen lifecycle. Do not restore either path as compatibility code; FIM may
  re-enter only as a complete evidence-backed vertical slice.
- The TUI-local Goal/Hunt loop, private TaskContract/receipt store, Slop ledger,
  fake custom-command pause state, and their UI/config surfaces have been
  physically deleted. They had no canonical production consumer. The only
  TaskContract/EvidenceReceipt owner now lives in protocol/runtime/state; do
  not adapt or restore this prototype.

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
./scripts/dev-codewhale.sh focused
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
