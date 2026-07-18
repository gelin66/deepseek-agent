# codewhale Operations Runbook

> Category: current local-development reference.

This runbook covers practical debugging and incident response for the local CLI/TUI runtime.

## Quick Triage

1. Confirm binary + config:
   - `cargo run -- --version`
   - `cat ~/.codewhale/config.toml` (or inspect configured profile)
2. Enable verbose logs:
   - `RUST_LOG=deepseek_cli=debug cargo run`
   - For HTTP retries/reconnects: `RUST_LOG=deepseek_cli::client=debug cargo run`
3. Capture the canonical state files without modifying them:
   - `ls -l ~/.codewhale/state.db*`
   - When `CODEWHALE_HOME` is set, inspect `$CODEWHALE_HOME/state.db*` instead.

## Incident: Turn Hangs or Stream Stops

Symptoms:
- TUI remains in loading state
- partial assistant output with no completion

Checks:
1. Inspect retry/health logs (`deepseek_cli::client`)
2. Verify endpoint connectivity:
   - `curl -sS https://api.deepseek.com/v1/models -H "Authorization: Bearer $DEEPSEEK_API_KEY"`
3. Confirm no local sandbox/permission deadlock in tool output

Actions:
1. Inspect the running `exec_shell` card for the exact command and latest output.
2. Press `Ctrl+C` to interrupt the active canonical run. A managed shell process is terminated with the run; it is not detached into a TUI-local job center.
3. Retry the prompt; if it still fails, restart the TUI.
4. On restart, verify the previous queued/in-flight runtime turn is shown as interrupted rather than left in a running state.

## Incident: Network Outage

There is no durable offline prompt queue. Messages queued while an active turn
is busy are process-local composer state and do not survive a restart.

Actions:
1. Let the canonical run reach a typed retry/failure state or interrupt it.
2. Restore connectivity.
3. Resume the exact run if it remains resumable, then resubmit any composer
   input that was not durably accepted.

## Incident: Crash Recovery Needed

Expected behavior:
- Canonical events and reducer snapshots are stored together in
  `~/.codewhale/state.db` (or `$CODEWHALE_HOME/state.db`).
- Startup begins a fresh session unless `--resume`/`--continue` is supplied

Actions:
1. Resume interactive work with `codewhale resume <RUN_ID>`, or headless work
   with `codewhale exec --resume <RUN_ID>`.
2. Use `codewhale resume --last` only when the newest run in the workspace is
   the intended target; prefer an exact Run ID for incident recovery.
3. Do not delete or edit individual `agent_run_snapshots` rows. They are
   validated against the canonical append-only event log during replay.

## Incident: Persistent State Schema Errors

Symptoms:
- Errors like `schema vX is newer than supported vY`

Affected stores:
- canonical SQLite state (`~/.codewhale/state.db` or
  `$CODEWHALE_HOME/state.db`)

Actions:
1. Confirm binary version and migration expectations
2. Back up `state.db` and its `-wal`/`-shm` siblings before any manual action.
3. Run with a binary that supports the recorded schema. Do not delete a single
   snapshot or event row to bypass a version or replay error.

## Incident: MCP/Tool Execution Failures

Checks:
1. Run `codewhale doctor` and `codewhale mcp list` to confirm the resolved MCP
   config path and server inventory.
2. Run `codewhale mcp connect <name>` for an isolated live connection check,
   or `codewhale mcp validate` to require all enabled servers to connect.
3. Run `codewhale mcp tools <name>` to verify tool discovery, and inspect the
   CLI error for command, transport, or authentication details.

Actions:
1. Correct the server command, URL, environment-backed credentials, OAuth
   login, or transport setting identified by the CLI diagnostic.
2. Temporarily disable the failing server with
   `codewhale mcp disable <name>` and isolate the issue.
3. Re-enable it with `codewhale mcp enable <name>`, repeat the CLI connection
   checks, and inspect the reported connection or discovery error. The current
   TUI has no `/mcp` manager or hot-reload command, and the canonical Agent does
   not load the discovered MCP tools.

## Post-Incident Checklist

1. Preserve logs and relevant state files
2. Record trigger, impact, and mitigation
3. Add or update regression tests (retry/recovery/schema)
4. Update this runbook and architecture docs if behavior changed
