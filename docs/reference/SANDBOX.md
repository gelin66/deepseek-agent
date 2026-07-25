# DSE sandbox and command-execution boundary

> Category: current implementation reference.

DSE classifies commands, applies the configured approval policy, and prepares
local execution through the canonical `crates/tools` sandbox owner. A policy
describes the requested restriction; the implementation must not claim that a
platform enforces a restriction when no enforcing backend is wired.

## Policy modes

The user-facing modes are:

| Mode | Meaning |
| --- | --- |
| `read-only` | Request local read-only execution. |
| `workspace-write` | Request writes only in the workspace and explicitly allowed roots. This is the default. |
| `danger-full-access` | Run without a local filesystem sandbox after the applicable approval decision. |
| `external-sandbox` | Declare that the process is already contained by an external environment. |

The isolated Writer uses an additional Host-only `isolated-writer` policy. It
binds writes to the admitted Git worktree, keeps its control paths protected,
disables network access, and must fail closed when DSE cannot provide an
enforcing local sandbox.

Approval and sandboxing are separate controls. Approval decides whether a
command may start; a sandbox constrains the process after it starts. Neither
one turns model output into trusted code.

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
sandbox_mode = "external-sandbox"
sandbox_backend = "opensandbox"
sandbox_url = "http://127.0.0.1:8080"
# sandbox_api_key = "..."
```

Equivalent environment overrides are `DSE_SANDBOX_MODE`,
`DSE_SANDBOX_BACKEND`, `DSE_SANDBOX_URL`, and
`DSE_SANDBOX_API_KEY`. The remote service becomes part of the trust boundary;
DSE records a non-secret endpoint fingerprint rather than the URL or
credential in replay identity.

Do not set `external-sandbox` merely to bypass local enforcement. Use it only
when the configured external environment actually owns isolation.

## Local configuration

The canonical file is `~/.dse/config.toml`:

```toml
sandbox_mode = "workspace-write"
prefer_bwrap = false
```

`prefer_bwrap = true` asks ordinary Linux commands to use bubblewrap when it is
available. It has no environment override. The isolated Writer does not depend
on this preference: it requires an enforcing backend regardless.

The non-interactive CLI can override the mode for one run:

```bash
dse exec --auto --sandbox read-only "Inspect this repository."
dse exec --auto --sandbox workspace-write "Fix and verify the defect."
```

`--allow-sandbox-elevation` is an explicit authorization for a sandbox-rejected
tool to retry with `danger-full-access`; it is not enabled by `--auto`.

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
