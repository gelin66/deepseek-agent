# DeepSeek backend

CodeWhale has one production model backend: the official DeepSeek API.
“Provider” remains only a descriptive field in canonical run receipts and
Doctor output; it is not a selectable configuration, runtime abstraction, or
extension point.

## Fixed production identity

| Fact | Value |
| --- | --- |
| Provider id | `deepseek` |
| Default endpoint | `https://api.deepseek.com` |
| Chat surface | official Chat Completions |
| Beta Strict | request-planner capability; no user mode |
| Beta FIM | distinct Completions capability; no canonical production editor |
| Credential | DeepSeek API Key |
| Default model | `deepseek-v4-pro` |
| Retained public model ids | `deepseek-v4-pro`, `deepseek-v4-flash` |

`codewhale exec`, interactive TUI, app-server HTTP/SSE, and app-server stdio
all enter the same `AgentApplication -> AgentRuntime -> RunStore` composition.
No client has a private Provider route or model loop.

## Credentials

Credential resolution is:

```text
explicit CLI --api-key
  -> root api_key in config.toml
  -> CodeWhale secret store / OS keyring
  -> DEEPSEEK_API_KEY
```

Use:

```bash
codewhale auth set
codewhale auth status
codewhale auth clear
```

`codewhale auth set` writes the canonical root `api_key` and attempts to save
the same DeepSeek credential in the selected secret backend. Status output
reports only the source class and never prints the secret.

The following model-auth paths are deleted:

- Provider-specific credential tables;
- model-provider OAuth adapters;
- command-auth and secret-id Provider records;
- cross-Provider keyring lookup;
- Provider fallback chains.

MCP OAuth remains supported for authenticating MCP transports. It does not
authenticate the model backend and is not part of this contract.

## Endpoint policy

Production defaults to `https://api.deepseek.com`. The official `/v1` and
`/beta` roots are recognized when a canonical caller needs that DeepSeek
surface. Loopback URLs are accepted only when explicitly supplied by offline
fixtures and local production-path tests.

`codewhale app-server` admits only the official HTTPS endpoint. Production
model requests do not accept:

- arbitrary gateway or self-hosted endpoints;
- custom model HTTP headers;
- TLS verification bypass;
- Provider-specific path suffixes.

These rules are checked before model construction or a canonical Run is
created.

## Models

Config and CLI accept `auto`, the two retained public DeepSeek ids, and the
bounded convenience forms `pro`, `flash`, `deepseek-v4pro`, and
`deepseek-v4flash`. Retired aliases, speculative future ids, foreign ids, and
values containing whitespace, secret material, or delimiter injection are
rejected before a model request.

Aliases normalize as follows:

| Input | Canonical request id |
| --- | --- |
| `pro`, `deepseek-v4pro` | `deepseek-v4-pro` |
| `flash`, `deepseek-v4flash` | `deepseek-v4-flash` |
| `auto` | Host-selected retained DeepSeek model |

Transient limits and protocol behavior belong to `crates/deepseek` fixtures,
not to a generic model catalog or user config. CodeWhale does not fetch
`models.dev`, maintain cross-provider prices, translate aliases for gateways,
or infer a Provider from a model string.

## Configuration cutover

The only model-bearing root keys are:

```toml
api_key = "..."
base_url = "https://api.deepseek.com"
default_text_model = "deepseek-v4-pro"
```

The following keys fail closed, including values that say `deepseek`:

```text
provider
providers
fallback_providers
model
auth / auth_mode
http_headers
insecure_skip_tls_verify
path_suffix
harness_profiles
model_catalog / models
```

Camel-case compatibility spellings for the replaced model configuration are
also rejected. There is no compatibility reader or dual write. Move retained
values to the three canonical root keys before launch.

Named local profiles may change non-model TUI settings only. Fleet profiles
may optionally pin an official DeepSeek model, but cannot carry a Provider
field. Project-local config cannot change credentials, endpoint, model,
telemetry, or Provider identity.

## Protocol ownership

`crates/deepseek` owns:

- Chat/FIM request planning and endpoint choice;
- streaming and non-streaming response parsing;
- reasoning/tool-call replay;
- finish/error/retry classification;
- usage, cache, request, retry, and cost accounting;
- official model capability fixtures.

`crates/config` owns only the root DeepSeek credential/endpoint/model
resolution plus retained local non-model settings. `crates/tools` owns the
fixed Host tool catalog. The TUI and CLI only project these facts.

## Official references

Protocol changes must be checked against DeepSeek primary documentation:

- [API introduction](https://api-docs.deepseek.com/)
- [Models and pricing](https://api-docs.deepseek.com/quick_start/pricing)
- [Thinking mode](https://api-docs.deepseek.com/guides/thinking_mode)
- [Tool calls](https://api-docs.deepseek.com/guides/tool_calls)
- [FIM completion](https://api-docs.deepseek.com/guides/fim_completion)
- [Beta API](https://api-docs.deepseek.com/guides/beta_version)

M8-A revalidated ownership and entry behavior offline on 2026-07-23. It did
not change protocol fixtures, model limits, prices, or request behavior, so it
did not perform a credentialed DeepSeek request.
