# M17-B DSE config/state/protocol identity

## Decision

`keep_and_cut_over` for the current config/state/protocol identity at production
checkpoint `2b6dd276d`.

This is an identity and replay-correctness result, not a model-quality, token,
latency, cost, billing, or release result. No credential, official DeepSeek API,
network, push, tag, release, or remote mutation was used.

## Slice contract

1. **Real problem:** the DSE binary/Cargo cutover still consumed `.codewhale` and
   `CODEWHALE_*`, and canonical prompt/artifact/machine streams still emitted the
   retired current product identity.
2. **Acceptance:** new config and local state use only DSE paths/env; prompt,
   artifact, runtime handoff and compact exec streams emit only DSE identity;
   pending Start and exact replay remain crash-safe; frozen historical evidence
   remains byte-historical.
3. **Owners:** existing `crates/config`, `crates/state`, and `crates/protocol`
   owners. `AgentRuntime` and `RunStore` remain unique.
4. **Replaced path:** active `.codewhale`, `CODEWHALE_*`, CodeWhale prompt
   wrappers, keychain service, verification media type, runtime-event handoff and
   exec-stream namespace.
5. **Evidence:** isolated env/path contract tests, canonical JSON/NDJSON,
   root/read-only/Writer conformance, pending Start, SQLite reopen, process
   SIGKILL recovery, CLI/TUI/app-server parity, focused and full workspace gates.
6. **Deleted at cutover:** old readers, serialization names, compatibility
   aliases and dual-write opportunity. No permanent migration manager was added.

## Exact current contract

- home: `~/.dse`
- config override: `DSE_CONFIG_PATH`
- home override: `DSE_HOME`
- other active product env: `DSE_*`
- Secret file owner and keychain service: DSE
- verification media type: `application/vnd.dse.verification+json`
- runtime handoff: `<dse:runtime_event>`
- compact stream schema: `dse.exec-stream`, version 4
- Run API: v12
- RuntimeEvent: v19
- State schema: v25

The Run API stays v12 because the command envelope did not change. RuntimeEvent
v19 and exec-stream v4 make changed machine identities explicit. State v25
atomically keeps only replay-safe pending Start intents and retires v18
materialized runs: rewriting an old handoff/media-type transcript under a new
identity would violate exact replay. There is no compatibility reader, dual
write, second Store, or alternate protocol truth.

## One-time local migration evidence

The existing `/Users/gelin/.codewhale` was inspected by path and permission only;
Secret content was never printed. No canonical current `state.db` was present.
Six byte-preservable facts were copied to a new mode-0700
`/Users/gelin/.dse`, then checked with `cmp`:

- `config.toml` (0600)
- `settings.toml` (0644)
- `setup_state.json` (0600)
- `permissions.toml` (0600)
- `.onboarded` (0644)
- `secrets/secrets.json` under a mode-0700 directory (0600)

The original `.codewhale` directory remains intact as the backup. Historical
sessions, logs, task queues, catalogs and tool outputs were not copied and are
not represented as canonical DSE state.

## Frozen-history allowlist

The cutover deliberately does not rename:

- M7-A/M7-A2 `DeepSeek Agent` evaluation titles;
- frozen `codewhale.eval.*` and canonical JSON fixture schemas;
- historical manifests, hashes, summaries and raw evidence;
- literal source/workspace paths that still name the repository directory;
- negative assertions proving the retired current product name is absent;
- Git history, including `83d05775`, which records the superseded pre-public
  DSA plan.

DSA was replaced before public release because DeepSeek uses it for DeepSeek
Sparse Attention. That correction is an auditable new DSE decision, not a
rewrite of frozen evidence.

## Verification

All Cargo commands used:

```text
CARGO_INCREMENTAL=0
CARGO_TARGET_DIR=/private/tmp/dse-m17-target
```

Passed:

- targeted config, secrets, protocol, state, context, tools, runtime and TUI
  checks/tests;
- `./scripts/dev-codewhale.sh focused` (the old script filename is retained only
  until M17-C);
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- 38/38 State RunStore tests;
- 38 pass / 1 helper ignored process crash/reopen tests;
- root/read-only/Writer and writer-worktree conformance;
- canonical CLI/TUI/app-server JSON, NDJSON, HTTP/SSE and replay parity;
- `git diff --check`.

## Remaining boundary

M17-C must migrate the existing delivery/dev/CI owner, release manifests,
artifacts, install links and library paths to DSE and physically delete their
old active paths. M17-B does not claim release lifecycle completion.
