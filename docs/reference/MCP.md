# MCP (External Tool Servers)

> Category: current implementation reference; MCP remains optional and lazy.

CodeWhale provides a top-level MCP client CLI for configuring servers,
checking connections, and discovering advertised tools. Servers can be local
stdio processes started by the client, or remote URL-based servers that speak
Streamable HTTP with legacy SSE fallback.

Browsing note:
- `web.run` is the canonical built-in browsing tool.
- `web_search` remains available as a compatibility alias for older prompts and integrations.

CodeWhale only consumes external MCP tool servers; it no longer exposes itself
as an MCP server. The canonical local Agent API is `codewhale app-server` over
HTTP/SSE or stdio. The former ACP editor adapter has been deleted because it
owned an independent model/session loop instead of projecting the canonical Run
API.

## MCP readiness

The retired TUI `/setup` wizard has been removed. MCP remains optional and is
configured through the explicit CLI commands below. An empty inventory is not
an error. `codewhale doctor` reports paths, counts and static configuration
problems without starting servers or installing packages; summaries redact
commands, args, environment values, headers and tokens.

## Bootstrap MCP Config

Create a starter MCP config at your resolved MCP path:

```bash
codewhale mcp init
```

`codewhale setup --mcp` performs the same MCP bootstrap through the broader
setup command.

Common management commands:

```bash
codewhale mcp list
codewhale mcp connect [server]
codewhale mcp tools [server]
codewhale mcp add <name> --command "<cmd>" --arg "<arg>"
codewhale mcp add <name> --url "http://localhost:3000/mcp"
codewhale mcp add <name> --url "https://example.com/mcp" --bearer-token-env-var MCP_TOKEN
codewhale mcp login <name>
codewhale mcp logout <name>
codewhale mcp enable <name>
codewhale mcp disable <name>
codewhale mcp remove <name>
codewhale mcp validate
```

## Management Boundary

The interactive TUI does not currently expose a `/mcp` manager, reload
command, validation view, or manager snapshot. Manage MCP from a shell with
the top-level `codewhale mcp ...` commands.

The diagnostic commands have distinct behavior:

- `codewhale mcp list` reads the resolved global configuration plus any
  trusted workspace MCP configuration and reports configured servers.
- `codewhale mcp connect [server]` performs a live connection check for one
  server or all enabled servers.
- `codewhale mcp tools [server]` connects and prints discovered tools.
- `codewhale mcp validate` connects all enabled servers and exits with an
  error if any connection fails; it is not merely a JSON syntax check.

Configuration-changing commands write the resolved global MCP file. They do
not hot-reload an already running process. Each subsequent `codewhale mcp ...`
invocation reloads the configuration it needs. The current interactive and
canonical Agent paths do not load an MCP pool or advertise MCP tools to the
model.

`setup --mcp`, `mcp init`, `add`, `remove`, `enable`, and `disable` all use the
same config owner and atomic writer. Updates load only the resolved global
file, preserve complete server entries (including headers, bearer-token,
OAuth, scopes, resource, timeouts, tool filters, and transport), and never
persist the merged workspace/plugin inventory back into that file.

## Remote HTTP Auth

URL-based MCP servers can use static headers, env-derived headers, bearer-token
env vars, or OAuth. Authorization precedence is conservative:

1. `headers` and `env_headers` are applied first.
2. `bearer_token_env_var` adds `Authorization: Bearer <env value>` when no
   Authorization header was already set.
3. Stored OAuth credentials are used only when no Authorization header exists.

For bearer-token auth, prefer env-backed config:

```json
{
  "servers": {
    "remote": {
      "url": "https://example.com/mcp",
      "bearer_token_env_var": "EXAMPLE_MCP_TOKEN"
    }
  }
}
```

For generic remote MCP OAuth, add the URL server and run login:

```bash
codewhale mcp add remote --url "https://example.com/mcp"
codewhale mcp login remote
```

CodeWhale discovers the server OAuth metadata, opens the authorization URL in
your browser, listens on a local callback, exchanges the code, and stores the
token response through the CodeWhale secrets backend. Stored OAuth tokens are
looked up by server name plus URL and refreshed when possible before requests.
During login, the CLI prints the authorization URL and a waiting status while
the local callback listener is active. If a URL-based server returns 401 or
Unauthorized during connect/discovery, `codewhale mcp connect <name>` reports
that OAuth authentication is required and points to
`codewhale mcp login <name>`.

Optional OAuth fields:

```json
{
  "servers": {
    "remote": {
      "url": "https://example.com/mcp",
      "scopes": ["tools/read"],
      "oauth": {
        "client_id": "public-client-id"
      },
      "oauth_resource": "https://example.com"
    }
  }
}
```

User-level config can set callback behavior when the provider requires a fixed
redirect:

```toml
mcp_oauth_callback_port = 1455
mcp_oauth_callback_url = "http://127.0.0.1:1455/callback"
```

These callback fields are ignored from project-scope config overlays.

## Hugging Face MCP

Hugging Face provides a hosted MCP server for Hub resources, documentation,
datasets, Spaces, and community tools. CodeWhale can connect to the configured
Hugging Face endpoint and discover its advertised tools through the same MCP
transport as other URL-based servers.

The recommended setup path is Hugging Face's settings-generated configuration:

1. Visit <https://huggingface.co/settings/mcp> while signed in.
2. Choose the MCP client closest to your CodeWhale config shape and copy the
   generated server snippet.
3. Paste the Hugging Face server entry into your resolved MCP config file.
4. Run `codewhale mcp validate` or `codewhale mcp connect huggingface` to test
   the saved configuration.

CodeWhale reads both `servers` and `mcpServers`, so settings-generated snippets
can be adapted without changing the rest of the MCP file. A placeholder-only
shape looks like this:

```json
{
  "servers": {
    "huggingface": {
      "url": "https://huggingface.co/mcp",
      "headers": {
        "Authorization": "Bearer ${HF_TOKEN}"
      }
    }
  }
}
```

The placeholder above is not a runnable secret. Use the settings-generated
value in your private MCP config and never commit real Hugging Face tokens.

Shell diagnostics:

```bash
codewhale mcp list
codewhale mcp connect huggingface
codewhale mcp tools huggingface
```

The current interactive TUI has no Hugging Face-specific MCP manager or reload
route. The server name in the commands above must match the name used in your
MCP config.

Official docs: <https://huggingface.co/docs/hub/hf-mcp-server>

## Config File Location

Default path:

- `~/.codewhale/mcp.json` (`~/.deepseek/mcp.json` is still read when the CodeWhale file is absent)
- A trusted workspace may add `.codewhale/mcp.json`; read-only inventory and
  live diagnostic commands merge it with the resolved global configuration.

Overrides:

- Config: `mcp_config_path = "/path/to/mcp.json"`
- Env: `DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

`codewhale mcp init` (and `codewhale setup --mcp`) writes to this resolved
path.

Subsequent `codewhale mcp ...` commands read the newly resolved path.

## Discovery Naming

When tools from all connected servers are listed together, the MCP client uses
server-prefixed names:

- `mcp_<server>_<tool>`

Example: a server named `git` with a tool named `status` becomes `mcp_git_status`.

`codewhale mcp tools [server]` is the current user-facing discovery surface.
There is no TUI MCP command-palette manager, persisted discovery snapshot, or
canonical Agent tool-catalog integration.

## Minimal Example

```json
{
  "timeouts": {
    "connect_timeout": 10,
    "execute_timeout": 60,
    "read_timeout": 120
  },
  "servers": {
    "example": {
      "command": "node",
      "args": ["./path/to/your-mcp-server.js"],
      "env": {},
      "disabled": false
    }
  }
}
```

You can also use `mcpServers` instead of `servers` for compatibility with other clients.

## Server Fields

Per-server settings:

- `command` (string, optional): executable for a local stdio server. A server
  must configure either `command` or `url`.
- `args` (array of strings, optional)
- `env` (object, optional)
- `connect_timeout`, `execute_timeout`, `read_timeout` (seconds, optional)
- `disabled` (bool, optional)
- `enabled` (bool, optional, default `true`)
- `required` (bool, optional): the MCP connection pool reports an error if
  this enabled server does not initialize.
- `enabled_tools` (array, optional): allowlist of tool names for this server.
- `disabled_tools` (array, optional): denylist applied after `enabled_tools`.
- `url` (string, optional): Streamable HTTP endpoint for a remote MCP server;
  use this instead of `command`.
- `transport` (string, optional): set to `"sse"` for legacy SSE endpoints.
- `headers` (object, optional): literal HTTP headers for URL-based servers.
- `env_headers` or `env_http_headers` (object, optional): header names mapped to environment variable names.
- `bearer_token_env_var` (string, optional): environment variable containing a bearer token.
- `scopes` (array, optional): default OAuth scopes for `mcp login`.
- `oauth.client_id` (string, optional): pre-registered OAuth client ID.
- `oauth_resource` (string, optional): resource parameter appended to the authorization URL.

`execute_timeout` remains a round-tripped configuration field, but the current
CLI does not execute discovered MCP tools, resources, or prompts, so no runtime
path consumes that timeout today.

## Safety Notes

Only configure MCP servers you trust. `codewhale mcp connect`, `tools`, and
`validate` can start configured stdio processes or contact configured remote
URLs, so treat MCP configuration as equivalent to running code on your
machine. The current CLI management path does not imply that discovered tools
are available to the canonical Agent or covered by its tool-approval flow.
Avoid committing literal `Authorization` headers. Prefer `env_headers`,
`bearer_token_env_var`, or OAuth login so secrets stay outside the MCP file.

## Troubleshooting

- Run `codewhale doctor` to confirm the MCP config path it resolved and whether it exists.
- Run `codewhale mcp list` to inspect the resolved server inventory.
- Run `codewhale mcp connect [server]` for a live connection check, or
  `codewhale mcp validate` to require every enabled server to connect.
- If the MCP config is missing, run `codewhale mcp init --force` to regenerate it.
- If tools don’t appear, verify the server command works from your shell and that the server supports MCP `tools/list`.
- The interactive TUI has no `/mcp` manager or hot reload, and the canonical
  Agent does not currently advertise the discovered MCP tools.
