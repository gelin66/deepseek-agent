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
3. Capture current state:
   - `ls ~/.codewhale/sessions`
   - `ls ~/.codewhale/sessions/checkpoints`
   - `ls ~/.codewhale/tasks`

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

## Incident: Network Outage / Offline Behavior

Expected behavior:
- New prompts are queued while offline mode is active
- Queue state persists to `~/.codewhale/sessions/checkpoints/offline_queue.json`

Checks:
1. Open queue in TUI: `/queue list`
2. Confirm persisted queue file exists and updates timestamp

Actions:
1. Restore connectivity
2. Re-send queued entries (from `/queue edit <n>` + Enter, or normal input flow)
3. Ensure queue file clears when queue is empty

## Incident: Crash Recovery Needed

Expected behavior:
- Checkpoint stored at `~/.codewhale/sessions/checkpoints/latest.json`
- Startup begins a fresh session unless `--resume`/`--continue` is supplied

Actions:
1. Resume prior work explicitly via `codewhale --resume <id>` or `Ctrl+R` in TUI
2. If checkpoint inspection is needed, inspect `latest.json` for schema mismatch/details
3. If schema is newer than binary supports, upgrade binary or remove stale checkpoint

## Incident: Persistent State Schema Errors

Symptoms:
- Errors like `schema vX is newer than supported vY`

Affected stores:
- sessions (`~/.codewhale/sessions/*.json`)
- runtime thread/turn/item records
- tasks (`~/.codewhale/tasks/tasks/*.json`)

Actions:
1. Confirm binary version and migration expectations
2. Back up the state directory before editing
3. Either:
   - run with a newer compatible binary, or
   - archive incompatible records and regenerate state

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
