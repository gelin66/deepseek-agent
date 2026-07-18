# MCP (External Tool Servers)

> Category: current implementation reference; MCP remains optional and lazy.

codewhale can load additional tools via MCP (Model Context Protocol). MCP servers can be local stdio processes that the TUI starts, or remote URL-based servers that speak Streamable HTTP with legacy SSE fallback.

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
codewhale-tui mcp init
```

`codewhale-tui setup --mcp` performs the same MCP bootstrap alongside skills setup.

Common management commands:

```bash
codewhale-tui mcp list
codewhale-tui mcp tools [server]
codewhale-tui mcp add <name> --command "<cmd>" --arg "<arg>"
codewhale-tui mcp add <name> --url "http://localhost:3000/mcp"
codewhale-tui mcp add <name> --url "https://example.com/mcp" --bearer-token-env-var MCP_TOKEN
codewhale-tui mcp login <name>
codewhale-tui mcp logout <name>
codewhale-tui mcp enable <name>
codewhale-tui mcp disable <name>
codewhale-tui mcp remove <name>
codewhale-tui mcp validate
```

## In-TUI Manager

Inside the interactive TUI, `/mcp` opens a compact manager for the resolved
MCP config path. It shows each configured server, whether it is enabled or
disabled, its transport, command or URL, timeout values, connection errors,
and discovered tools/resources/prompts when discovery has been run.

Supported in-TUI actions:

```text
/mcp init
/mcp init --force
/mcp add stdio <name> <command> [args...]
/mcp add http <name> <url>
/mcp login <name> [--scope scope]
/mcp logout <name>
/mcp enable <name>
/mcp disable <name>
/mcp remove <name>
/mcp validate
/mcp reload
```

`/mcp validate` and `/mcp reload` reconnect for UI discovery and refresh the
manager snapshot. Config edits made from the TUI are written immediately, but
the model-visible MCP tool pool is not hot-reloaded; the manager marks this as
restart-required until the TUI is restarted.

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
codewhale-tui mcp add remote --url "https://example.com/mcp"
codewhale-tui mcp login remote
```

CodeWhale discovers the server OAuth metadata, opens the authorization URL in
your browser, listens on a local callback, exchanges the code, and stores the
token response through the CodeWhale secrets backend. Stored OAuth tokens are
looked up by server name plus URL and refreshed when possible before requests.
During login, the CLI prints the authorization URL and a waiting status while
the local callback listener is active. If a URL-based server returns 401 or
Unauthorized during connect/discovery, `codewhale mcp connect <name>` reports
that OAuth authentication is required and points to
`codewhale mcp login <name>`. Resource helper listings also surface an
`authentication_required` entry for auth-shaped failures instead of silently
looking empty.

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
datasets, Spaces, and community tools. CodeWhale does not call Hugging Face's
Hub HTTP APIs from `/hf`; it only helps you inspect and set up the MCP config
that the regular MCP manager will load.

The recommended setup path is Hugging Face's settings-generated configuration:

1. Visit <https://huggingface.co/settings/mcp> while signed in.
2. Choose the MCP client closest to your CodeWhale config shape and copy the
   generated server snippet.
3. Paste the Hugging Face server entry into your resolved MCP config file.
4. Restart CodeWhale, or run `/mcp reload` for the manager snapshot and restart
   if the model-visible tool pool still needs to rebuild.

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

Interactive helpers:

```text
/hf mcp status
/hf mcp setup
/hf concepts
```

`/hf mcp status` checks the configured MCP file for common Hugging Face server
names or Hugging Face MCP URLs. `/hf concepts` explains the difference between
the Hugging Face provider route, Hugging Face MCP, and explicit Hub workflows.

Official docs: <https://huggingface.co/docs/hub/hf-mcp-server>

## Config File Location

Default path:

- `~/.codewhale/mcp.json` (`~/.deepseek/mcp.json` is still read when the CodeWhale file is absent)

Overrides:

- Config: `mcp_config_path = "/path/to/mcp.json"`
- Env: `DEEPSEEK_MCP_CONFIG=/path/to/mcp.json`

`codewhale-tui mcp init` (and `codewhale-tui setup --mcp`) writes to this resolved path.

After changing `mcp_config_path` in `~/.codewhale/config.toml`, restart the TUI
so the model-visible MCP tool pool is rebuilt.

## Tool Naming

Discovered MCP tools are exposed to the model as:

- `mcp_<server>_<tool>`

Example: a server named `git` with a tool named `status` becomes `mcp_git_status`.

The command palette includes MCP entries grouped by server. It shows disabled
and failed servers instead of hiding them, and uses the same runtime tool names
shown to the model.

## Resource and Prompt Helpers

The CLI also exposes helper tools when MCP is enabled:

- `list_mcp_resources` (optional `server` filter)
- `list_mcp_resource_templates` (optional `server` filter)
- `mcp_read_resource` / `read_mcp_resource` (aliases)
- `mcp_get_prompt`

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

- `command` (string, required)
- `args` (array of strings, optional)
- `env` (object, optional)
- `connect_timeout`, `execute_timeout`, `read_timeout` (seconds, optional)
- `disabled` (bool, optional)
- `enabled` (bool, optional, default `true`)
- `required` (bool, optional): startup/connect validation fails if this server cannot initialize.
- `enabled_tools` (array, optional): allowlist of tool names for this server.
- `disabled_tools` (array, optional): denylist applied after `enabled_tools`.
- `url` (string, optional): Streamable HTTP endpoint for a remote MCP server.
- `transport` (string, optional): set to `"sse"` for legacy SSE endpoints.
- `headers` (object, optional): literal HTTP headers for URL-based servers.
- `env_headers` or `env_http_headers` (object, optional): header names mapped to environment variable names.
- `bearer_token_env_var` (string, optional): environment variable containing a bearer token.
- `scopes` (array, optional): default OAuth scopes for `mcp login`.
- `oauth.client_id` (string, optional): pre-registered OAuth client ID.
- `oauth_resource` (string, optional): resource parameter appended to the authorization URL.

## Safety Notes

MCP tools now flow through the same tool-approval framework as built-in tools. Read-only MCP helpers (resource/prompt listing and reads) can run without prompts in suggestive approval modes, while side-effectful MCP tools require approval.

You should still only configure MCP servers you trust, and treat MCP server configuration as equivalent to running code on your machine.
Avoid committing literal `Authorization` headers. Prefer `env_headers`,
`bearer_token_env_var`, or OAuth login so secrets stay outside the MCP file.

## Troubleshooting

- Run `codewhale-tui doctor` to confirm the MCP config path it resolved and whether it exists.
- In the TUI, run `/mcp validate` to refresh the visible server/tool snapshot.
- If the MCP config is missing, run `codewhale-tui mcp init --force` to regenerate it.
- If tools don’t appear, verify the server command works from your shell and that the server supports MCP `tools/list`.
