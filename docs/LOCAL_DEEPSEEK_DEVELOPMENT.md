# Local DeepSeek Agent development

## Repository baseline

- Upstream: `https://github.com/Hmbown/CodeWhale`
- Local specialization branch: `deepseek-agent`
- Imported upstream baseline: `352e86a611fdf3cd8bd27c36d24d482c06a71117`
  (`v0.8.68` workspace)
- Last upstream equality check: 2026-07-15

`origin` intentionally remains the official repository so `git fetch origin
main` is the source-of-truth update check. Add a separate `fork` remote later if
this branch will be published; do not repoint `origin` just to make local work
possible.

## One-time setup

Use stable Rust (the workspace currently requires Rust 1.88 or newer):

```bash
rustup default stable
cargo build -p codewhale-cli -p codewhale-tui --locked
```

Start from the focused sample without putting a credential in the repository:

```bash
mkdir -p ~/.codewhale
# Merge config.deepseek-agent.example.toml into ~/.codewhale/config.toml.
export DEEPSEEK_API_KEY='your-key'
```

The sample selects the official `/beta` route, `deepseek-v4-pro`, maximum
reasoning, core coding tools, sub-agents, memory, and on-demand independent
verification. Keep the key in the environment or the platform credential store.

Run the local binaries:

```bash
cargo run -p codewhale-cli --locked --
cargo run -p codewhale-cli --locked -- exec --auto "inspect this repository and fix one failing test"
```

The dispatcher expects `codewhale-tui` beside it. A workspace build produces
both under `target/debug`; when running them from different locations, set
`DEEPSEEK_TUI_BIN` to the local `codewhale-tui` binary.

## Daily loop

Confirm branch and scope before editing:

```bash
git branch --show-current
git status --short
./scripts/dev-deepseek-agent.sh focused
```

Use one reviewable concern per commit. A useful sequence is protocol fix,
agent-state fix, tool fix, then documentation. Do not mix provider expansion or
UI churn into those commits.

The development gate supports three modes:

```bash
./scripts/dev-deepseek-agent.sh focused  # focused Agent/DeepSeek regressions + check
./scripts/dev-deepseek-agent.sh crate    # complete codewhale-tui unit suite
./scripts/dev-deepseek-agent.sh full     # fmt, clippy, and entire workspace suite
```

Credentialed API tests must be opt-in and must never run as part of the default
offline gate. Protocol behavior should first be covered with local fixtures or
WireMock.

## Updating from upstream

First finish or stash a coherent local slice; never rebase a dirty tree. Then:

```bash
git fetch --prune origin main
git log --oneline --left-right deepseek-agent...origin/main
git rebase origin/main
./scripts/dev-deepseek-agent.sh focused
```

Use a merge instead of a rebase once the branch is shared with other people.
After every sync, recheck the DeepSeek beta URL, thinking/tool replay, tool
schema sanitation, context limits, and turn completion semantics; those are the
highest-conflict specialization seams.

No commit, push, force-push, tag, or release should be performed automatically.
Those remain explicit developer actions.

## Definition of done

A local capability change is ready only when:

1. it maps directly to `docs/DEEPSEEK_AGENT.md`;
2. failure behavior is explicit rather than reported as success;
3. a deterministic regression test covers the protocol or state transition;
4. `./scripts/dev-deepseek-agent.sh focused` passes;
5. the changed files have been read back and `git diff --check` is clean;
6. any live DeepSeek assumption is linked to the current official API docs.
