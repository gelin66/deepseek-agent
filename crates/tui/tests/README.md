# `crates/tui/tests/`

Integration tests for the TUI binary. Per `CONTRIBUTING.md`, each crate's
integration tests live in its own `tests/` directory; the repository-root
`tests/` directory is unused.

## `--record` mode for `deepseek eval`

The offline `deepseek eval` harness now accepts `--record <DIR>`. When set,
each tool step appends one JSON Lines record to `<DIR>/<scenario>.jsonl`
(default scenario: `offline-tool-loop.jsonl`). Each line is a self-contained
JSON object with the schema:

```json
{ "request":  { "step": "list_dir", "kind": "List" },
  "response_events": [ { "type": "ok", "output": "…" } ] }
```

`eval_harness.rs` deserializes these records and verifies the stable evidence
schema. The records do not drive a model loop and should remain untracked local
evaluation output.

Quick example:

```bash
cargo run --bin codewhale -- eval --record /tmp/codewhale-eval-records
jq . /tmp/codewhale-eval-records/offline-tool-loop.jsonl
```

The scenario name is sanitized to `[A-Za-z0-9_-]` before forming the filename,
so unusual scenario strings stay portable across platforms.
