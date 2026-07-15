# Contributing

This repository is currently a local product-development line based on
CodeWhale. Read [AGENTS.md](AGENTS.md), the
[product plan](docs/product/PRODUCT_PLAN.md), and the
[roadmap](docs/product/ROADMAP.md) before changing code.

## Prerequisites

- Rust 1.88 or newer;
- Cargo and Git;
- a DeepSeek API key only for explicitly enabled live canaries.

## Setup

```bash
rustup default stable
cargo build -p codewhale-cli -p codewhale-tui --locked
```

Current focused gate:

```bash
./scripts/dev-deepseek-agent.sh focused
```

## Change requirements

Every change must:

- solve a product-scope problem;
- identify the owning module and old path it replaces;
- include deterministic regression coverage;
- preserve unrelated worktree changes;
- report validation honestly;
- update Roadmap/Evaluation when it changes a milestone or capability decision.

Prefer one vertical behavior per commit. Do not combine provider cleanup,
runtime migration, UI redesign, branding, and release work in one change.

## Validation

Run the narrowest relevant checks first:

```bash
cargo fmt --all -- --check
cargo test -p <owning-crate> --locked <filter>
cargo check -p <owning-crate> --locked
```

Before integrating a completed slice:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Live API checks must be opt-in, have explicit Token/cost/time limits, and never
replace offline protocol fixtures.

## Scope

Do not add other model providers, cloud orchestration, chat integrations,
plugin marketplaces, enterprise control planes, or permanent compatibility
layers. External Agent projects are references; implementations belong in the
Rust architecture defined by this repository.
