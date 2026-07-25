# Contributing to DSE

Thank you for helping improve DSE (DeepSeek Engineer). The English
[README](README.md) is the canonical public entry; a complete Simplified
Chinese entry is available in [README.zh-CN.md](README.zh-CN.md).

## Start with product authority

Before changing code, read in this order:

1. [AGENTS.md](AGENTS.md)
2. [Product plan](docs/product/PRODUCT_PLAN.md)
3. [Architecture decisions](docs/decisions/)
4. [Roadmap](docs/product/ROADMAP.md)
5. [Evaluation contract](docs/product/EVALUATION.md)
6. [Current architecture facts](docs/architecture/CURRENT_CODEWHALE.md)

The product plan and accepted ADRs win when older documents conflict. Historical
evaluation artifacts are evidence from their tested revisions; do not rename or
rewrite their schemas, hashes, titles, manifests, summaries, or raw journals.

## Contribution contract

Every implementation slice must state:

1. the real problem;
2. measurable acceptance criteria;
3. the single owning module;
4. the old path it replaces;
5. deterministic tests and evaluation evidence;
6. what is deleted at cutover;
7. how a rejected treatment will be removed.

Build one vertical behavior before broad module movement. Migrate real callers
before deleting an old path, and physically remove the old path after cutover.
Do not keep a compatibility bridge without a named, near-term deletion point.

## Fixed scope

DSE has one official DeepSeek Chat Completions backend, one `AgentRuntime`, one
canonical RuntimeEvent protocol, one `RunStore`, one Host completion authority,
and one canonical tool catalog. Root and child roles are profiles of that same
runtime. Writers are explicit-only and use isolated Git worktrees.

Do not add another model provider, model Auto router, Anthropic Messages, a
second Runtime or Store, a production FIM surface, default multi-Writer/swarm,
cloud-agent platform, plugin marketplace, or parallel product-state tracker
without an accepted ADR and the required evidence.

## Development setup

Use the exact toolchain in `rust-toolchain.toml` and locked dependencies:

```bash
cargo build -p dse-cli -p dse-tui --locked
```

Offline fixtures and loopback transports are the default. A DeepSeek credential
is never required for ordinary unit, integration, protocol, or public-repository
checks.

## Validation

Use an external Cargo target for repository work:

```bash
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/private/tmp/dse-development-target
```

Run the smallest owning-crate checks first, for example:

```bash
cargo fmt --all -- --check
cargo test -p dse-context --locked
cargo check -p dse-context --locked
```

Before integrating a completed Rust slice:

```bash
./scripts/dev-dse.sh focused
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

Documentation and governance changes must also pass:

```bash
./scripts/check-public-repository.py
```

Live DeepSeek tests are opt-in, cost-bounded, and allowed only after the offline
contract, immutable identity, budget, accounting, external verifier, and stop
conditions are frozen. Never print, commit, log, or copy a credential or raw
provider payload into a tracked file.

## Pull requests

Keep each commit to one reviewable concern and use an honest subject/body. A
pull request should include:

- the problem and owner;
- the current behavior and replacement path;
- tests run with exact outcomes;
- evaluation evidence or an explanation of why no live evaluation applies;
- deleted code and remaining risks;
- screenshots only when a human UI changed;
- confirmation that secrets, generated outputs, local state, raw data, and
  external Cargo targets are not tracked.

Quality and verified completion are gates. Lower token use, cost, or latency
cannot compensate for a verified-success regression or a false success.

## Documentation

Update the existing authority file that owns the fact:

- `PRODUCT_PLAN.md` only for accepted scope or architecture changes;
- an ADR only for a long-lived decision;
- `ROADMAP.md` for execution and deletion status;
- `EVALUATION.md` for keep/delete evidence;
- `CURRENT_CODEWHALE.md` for current implementation facts.

Do not add a parallel roadmap, handoff, version tracker, translated authority
tree, or speculative design document. Current public product identity is DSE;
CodeWhale and “DeepSeek Agent” remain only where provenance or historical
accuracy requires them.

## Conduct and security

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Potential vulnerabilities and credential exposure must be reported privately
as described in [SECURITY.md](SECURITY.md), not in a public issue.
