# DSE sandbox and command-execution boundary

> Category: current implementation reference.

DSE classifies commands, applies the Run-frozen permission mode, and prepares
local execution through the canonical `crates/tools` sandbox owner. Permission
authorization decides whether an invocation may start; the Host-owned sandbox
profile constrains the process after it starts. Neither is a free-form user
configuration language.

## Host-owned execution profiles

These are internal execution profiles, not user-selectable permission presets:

| Mode | Meaning |
| --- | --- |
| `read-only` | Request local read-only execution. |
| `workspace-write` | Writes in the workspace plus execution-required temporary/cache roots. Ask uses this profile. |
| `danger-full-access` | No local filesystem sandbox after authorization. Agent decides and Full access use this for root execution. |
| `external-sandbox` | An explicitly configured external backend owns isolation. |

The isolated Writer uses an additional Host-only `isolated-writer` policy. It
binds writes to the admitted Git worktree, keeps its control paths protected,
disables network access, and must fail closed when DSE cannot provide an
enforcing local sandbox.

The three user-facing permission modes are Ask for approval, Agent decides,
and Full access. Ask cannot currently enforce a one-shot network or external
filesystem grant, so canonical path-bearing tools, explicit Shell working
directories, and recognized network invocations fail closed instead of
receiving an unscoped global exception. Arbitrary file I/O performed inside a
spawned program is not derivable from a Shell string and remains bounded only
by the active OS sandbox profile; DSE does not claim otherwise. An isolated
Writer remains worktree-only regardless of the parent mode.

## Current platform enforcement

| Platform | Current local enforcement |
| --- | --- |
| macOS | DSE uses `/usr/bin/sandbox-exec` with a generated Seatbelt profile when the executable is available and usable. |
| Linux | Bubblewrap is the enforcing filesystem path when `/usr/bin/bwrap` is installed. The isolated Writer requires it and otherwise fails closed. |
| Windows | DSE does not currently advertise a local OS sandbox. |

The Linux tree contains Landlock and seccomp implementation modules, but they
are not wired into the spawned child process. Without bubblewrap, ordinary
`read-only` or `workspace-write` local execution therefore does **not** gain
kernel-enforced filesystem or syscall isolation. Internal detection/marker
names are not an enforcement guarantee.

DSE does not vendor bubblewrap. Install it with the platform package manager
when Linux filesystem enforcement is required, for example:

```bash
sudo apt install bubblewrap
```

The interactive binary applies Linux process hardening to itself before the
Tokio runtime starts (`PR_SET_DUMPABLE=0`, `PR_SET_NO_NEW_PRIVS=1`, and
`RLIMIT_CORE=0`). That protects the DSE process posture; it is not a substitute
for constraining each spawned command.

## External execution backend

DSE can route shell execution to the optional OpenSandbox adapter:

```toml
sandbox_backend = "opensandbox"
sandbox_url = "http://127.0.0.1:8080"
# sandbox_api_key = "..."
```

Equivalent environment overrides are
`DSE_SANDBOX_BACKEND`, `DSE_SANDBOX_URL`, and
`DSE_SANDBOX_API_KEY`. The remote service becomes part of the trust boundary;
DSE records a non-secret endpoint fingerprint rather than the URL or
credential in replay identity.

Do not configure an external backend merely to bypass local enforcement. Use
it only when that environment actually owns isolation.

## Local configuration

The canonical file is `~/.dse/config.toml`:

```toml
prefer_bwrap = false
```

`prefer_bwrap = true` asks ordinary Linux commands to use bubblewrap when it is
available. It has no environment override. The isolated Writer does not depend
on this preference: it requires an enforcing backend regardless.

There is no `sandbox_mode`, `--sandbox`, or
`--allow-sandbox-elevation` product input. `dse exec --auto` selects the
Agent decides permission preset; only explicit process-local `--yolo` selects
Full access. A sandbox rejection is never converted into a retry by a legacy
elevation flag. The old `dse-tui sandbox run` direct-execution utility is also
deleted: sandbox profiles are Host internals, not a parallel user permission
language.

## Security expectations

- Treat every generated command as untrusted.
- Keep the HTTP Run API on loopback with authentication.
- Do not rely on a policy label alone; verify the platform backend reported by
  `dse doctor`.
- Do not grant `danger-full-access` to compensate for a missing tool or broken
  environment without understanding the command.
- An isolated Writer that cannot obtain enforcing containment must remain
  blocked rather than run unrestricted.
- Network controls differ by backend. Do not assume Linux or Windows local
  execution blocks outbound traffic.

## Canonical owners

- [`crates/tools/src/sandbox/`](../../crates/tools/src/sandbox/) owns local
  policy preparation and denial classification.
- [`crates/tools/src/shell/`](../../crates/tools/src/shell/) owns the managed
  foreground process lifecycle.
- [`crates/tui/src/sandbox_backend/`](../../crates/tui/src/sandbox_backend/)
  owns the optional OpenSandbox transport adapter.
- [`crates/config/src/deepseek.rs`](../../crates/config/src/deepseek.rs) and
  [`crates/tui/src/config.rs`](../../crates/tui/src/config.rs) own the current
  persisted and environment configuration projections.
