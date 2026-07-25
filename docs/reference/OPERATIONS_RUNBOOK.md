# DSE operations runbook

> Category: current local-development reference.

This runbook covers practical diagnosis and recovery for the local CLI, TUI,
canonical Run API, and SQLite RunStore. Never paste a real credential, private
source, raw provider payload, or unredacted local path into a public report.

## Quick triage

1. Confirm binary and resolved configuration:

   ```bash
   dse --version
   dse config path
   dse doctor
   ```

2. Enable bounded local logs:

   ```bash
   DSE_LOG_LEVEL=debug RUST_LOG=dse_tui=debug dse
   ```

3. Inspect canonical files without modifying them:

   ```bash
   ls -l "${DSE_HOME:-$HOME/.dse}"/state.db*
   ```

4. Record the exact workspace revision and `dse runs` output before taking
   recovery action.

## Stream stops or a turn hangs

Symptoms include partial assistant content without a finish/usage outcome, a
Run that remains active, or a tool process that stops producing output.

1. Inspect the current tool card and stable failure code.
2. Press `Ctrl+C` once to interrupt the canonical Run. Managed child processes
   terminate with the Run; there is no private TUI job owner.
3. Reopen DSE and inspect the same Run. A prepared or started physical request
   may be retried only when the canonical sender recorded it as replay-safe.
4. If billing is unknown, do not blind-retry a formal evaluation arm.

## Network outage

DSE has no durable offline prompt queue. Composer text queued while a turn is
busy is process-local and does not survive restart.

1. Let the canonical Run reach a typed retry/failure state or interrupt it.
2. Restore connectivity and run `dse doctor`.
3. Resume the exact Run when it remains resumable.
4. Resubmit only input that was never durably accepted.

## Crash recovery

Canonical events and reducer snapshots share
`${DSE_HOME:-$HOME/.dse}/state.db`.

```bash
dse runs
dse resume <RUN_ID>
dse resume --last
```

Prefer an exact Run ID. Use `--last` only when the newest Run in the current
workspace is unquestionably the intended target. Do not delete or edit
individual event/snapshot rows; replay validates them against the append-only
log and immutable request/route facts.

## State schema errors

For errors such as `schema vX is newer than supported vY`:

1. Stop every DSE process using that state root.
2. Record the DSE binary revision.
3. Copy `state.db`, `state.db-wal`, and `state.db-shm` together when those
   siblings exist.
4. Reopen only with a binary that supports the recorded schema.

Do not delete selected rows or lower SQLite `user_version` to bypass migration
or replay checks.

## MCP diagnostics

MCP is an optional explicit management surface; discovered tools are not added
to the canonical Agent catalog.

```bash
dse doctor
dse mcp list
dse mcp connect <name>
dse mcp tools <name>
dse mcp validate
```

Correct the command, URL, environment-backed credential, OAuth login, or
transport error identified by the CLI. Configuration changes do not hot-reload
an already running process.

## Local HTTP Run API

HTTP/SSE defaults to loopback and requires authentication:

```bash
export DSE_APP_SERVER_TOKEN='replace-with-a-random-token'
dse app-server --host 127.0.0.1 --port 7878
```

Check that the token is process-local, the listen address is intended, and no
reverse proxy exposes the service unexpectedly. Use
`dse app-server --stdio` when a local parent process can own the transport.

## Post-incident checklist

1. Preserve redacted logs and a coherent copy of relevant state files.
2. Record trigger, impact, workspace revision, Run ID, and mitigation.
3. Add deterministic retry/recovery/schema coverage at the canonical owner.
4. Update this runbook or architecture facts when behavior changes.
5. Report security-relevant incidents privately through
   [../../SECURITY.md](../../SECURITY.md).
