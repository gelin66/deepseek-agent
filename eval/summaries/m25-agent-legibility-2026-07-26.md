# M25 repository agent-legibility cutover

- Date: 2026-07-26
- Branch: `deepseek-agent`
- Baseline: `748ed47b8c463163e45f31ff3273ab394e0007fa`
- Candidate implementation: `6981f54fb304893612227c19282f0c84353e53a8`
- Candidate tree: `37cf7707cae91dceef34e4f9efd692111aacfbae`
- Decision: `keep_compact_agent_guide_contract`
- Rust/Runtime/Store/protocol/model/tool delta: none
- Key/API/network/GitHub/push/release: `0 / 0 / 0 / 0 / 0 / 0`

## Problem, owner and selection

M25 froze production behavior and first compared four repository-maintenance
signals. The exact baseline is
`eval/manifests/m25-agent-legibility-v1.json`.

| Signal | Baseline observation | Selection |
|---|---|---|
| rule discovery | root guide 379 lines / 21,307 bytes; 227 lines were mutable milestone history | selected |
| owner location | sampled owner anchor median at line 51, scattered through history | handled by the same compact guide |
| large Rust files | 54 tracked files >=1,000 lines, 24 >=2,000, 7 >=5,000 | no current wrong-owner or behavior loss |
| dependency direction | 16 workspace crates / 51 internal edges | no current violation or safe layer treatment |

The selected loss was directly reproducible. Since the product-plan baseline,
`AGENTS.md` had changed in 30 commits and gained 215 lines while losing 7.
It contained 42 milestone references and 12 commit identities that duplicated
the existing Roadmap, Evaluation and current-architecture authorities. Stable
work and validation anchors were therefore reached at median line 309.5 and
maximum line 375.

The repository-governance owner remains the existing root `AGENTS.md` plus
`scripts/check-public-repository.py`. M25 did not add a second checker, doc
hierarchy, crate, dependency policy, Runtime or Store.

## Cutover and deletion

The guide is now a bounded directory containing:

- the required authority read order;
- stable product and architecture boundaries;
- a compact canonical owner map;
- vertical-slice and repository-work rules;
- official DeepSeek, Runtime and multi-agent invariants;
- focused, targeted and full local gates;
- documentation ownership.

The old `Current repository truth` ledger, commit/checkpoint inventory and
duplicated historical evaluation conclusions were physically deleted. Their
single existing owners remain PRODUCT_PLAN/ADRs, ROADMAP, EVALUATION,
CURRENT_CODEWHALE and frozen `eval/` evidence.

The existing public repository checker now enforces:

- at most 140 lines and 9,000 UTF-8 bytes;
- all five authority links and their local resolution;
- stable architecture, work, protocol, replay, Git and validation rules;
- no `Current repository truth`, milestone identifier or commit hash ledger;
- negative contracts for oversized, missing-authority and mutable-history
  false greens.

There is no compatibility guide, second reader or dual source of truth.

## Deterministic A/B

All 12 pre-registered stable rule anchors were preserved:

| Metric | Baseline | Candidate | Delta |
|---|---:|---:|---:|
| lines | 379 | 134 | -64.64% |
| UTF-8 bytes | 21,307 | 5,488 | -74.24% |
| nonblank lines | 338 | 103 | -69.53% |
| stable-rule median first line | 309.5 | 76 | -75.44% |
| stable-rule maximum first line | 375 | 131 | -65.07% |
| sampled owner median first line | 51 | 45 | -11.76% |
| milestone / commit identities | 42 / 12 | 0 / 0 | deleted |
| authority links | implicit paths | 5/5 resolving links | pass |
| negative checker fixtures | none | 3/3 rejected | pass |

The candidate guide SHA-256 is
`329e9235897e94a26212a22da331a6b1b14c8fe11107ded8e94f6e728720170e`;
the checker SHA-256 is
`eff65088800a8a87c28ab57195511df8754d62d8cdd58b8a57e52db1e43a23d1`.

This proves a smaller stable repository-instruction surface and earlier rule
discovery. It does not claim a measured DeepSeek Token, cache, cost, wall-time
or coding-success improvement.

## Local gates

All required work ran locally with `CARGO_INCREMENTAL=0`,
`CARGO_NET_OFFLINE=true`, `--locked` and isolated
`/private/tmp/dse-m25-target`:

```text
scripts/check-public-repository.py
./scripts/dev-dse.sh focused
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

Focused, strict Clippy and the complete workspace suite passed. The first full
workspace invocation had one load-sensitive failure:
`stream_stall_remains_primary_when_retry_open_fails` observed the retry-open
network error before the expected primary stall code. The same test had passed
in focused, passed immediately in an exact single-thread invocation, and
passed again inside the complete second workspace run. No Rust file or timing
contract changed in M25; the first result is retained here rather than reported
as an uninterrupted first-run green.

The gates cover root/read-only/Writer fixed routes, typed Pro/max recovery,
canonical tool and completion outcomes, pending Start, exact RequestPlan and
accounting reopen, process SIGKILL/replay, M21 partial-response fail-closed,
M22 same-chunk convergence, and CLI/TUI/HTTP/SSE/stdio parity.

## Clean-checkout release regression

A clean detached checkout of implementation commit `6981f54fb` passed the
public checker and full delivery tamper/install/upgrade/rollback/uninstall
self-test. Its exact source then built with Rust 1.97.0, locked/offline Cargo
and isolated `/private/tmp/dse-m25-release-target` in 4m52s.

```text
artifact  dse-0.8.68-aarch64-apple-darwin-6981f54fb304.tar.gz
archive   bb86a0e435515f1d84dae7adb79b43d3ecda2665b60c7563b0c19ab6affac43f
dse       8b48b536d00b26ad706d8ca805fc1a683a429aaf53dcf3dc65684df02d43c246
dse-tui   9599e00a7ded76ca2402a87bce83ef6897670e9d79e1276f2ae7bc7923e799f8
manifest  aa12ef66896a0d549870baa8cbe37bcbc51e919e0c06a91fe347131e4c598085
```

Outer archive and all four internal checksums passed. Install/verify reported
`0.8.68 (6981f54fb304)` for both binaries; uninstall removed programs and
delivery metadata while preserving the isolated `DSE_HOME` marker with
SHA-256
`2db135e0513a175a88ba2a0e194a4ba0e225905fecb40cb507ec7876a8f930c8`.

The first manual outer-check command was run from the worktree instead of the
artifact directory, so the relative filename in the sidecar could not be
opened. Install/verify already validated the artifact, and the explicit outer
and internal checks were then rerun from their correct directories and passed.

## Decision and non-conclusions

The decision is `keep_compact_agent_guide_contract`. The candidate achieves a
large, attributable rule-discovery and maintenance reduction with no Rust
production change, and the existing checker mechanically prevents the deleted
history ledger from returning.

M25 does not authorize:

- splitting any large Rust file without an independently reproduced owner or
  review loss;
- inventing a global dependency layer model from edge count alone;
- changing prompt, model, reasoning, Runtime, Store, protocol or tools;
- Auto, another Provider, RepoGraph, FIM, swarm or multi-Writer;
- a DeepSeek quality, Token, cache, time or cost conclusion.

The next maintenance candidate must again begin from a concrete repeated loss;
file size or architectural aesthetics alone are insufficient.
