# DeepSeek Agent specialization

## Objective

This local branch has one product target: make CodeWhale a dependable coding
agent on the official DeepSeek API. New work must improve either DeepSeek
runtime fidelity or the agent loop that plans, edits, verifies, and continues
work. Provider expansion, visual redesign, release operations, social features,
and security scanning are outside this branch's scope.

The upstream multi-provider implementation remains in place. Keeping it avoids
a destructive fork and makes upstream updates reviewable; it does not mean this
branch will spend development time expanding those surfaces.

## Current DeepSeek contract

The contract below was checked on 2026-07-15 and should be revalidated before a
model or protocol change:

- Use `deepseek-v4-pro` for maximum capability and `deepseek-v4-flash` for a
  faster/cost-conscious lane. The official V4 models expose a 1M context window
  and up to 384K output. Legacy `deepseek-chat` and `deepseek-reasoner` aliases
  are scheduled to retire on 2026-07-24. See the
  [DeepSeek model and pricing page](https://api-docs.deepseek.com/quick_start/pricing)
  and [DeepSeek changelog](https://api-docs.deepseek.com/updates/).
- Thinking mode accepts `high` and `max`. When an assistant turn emits tool
  calls, its `reasoning_content` must be replayed with that tool-call history on
  the next request. See the
  [thinking-mode guide](https://api-docs.deepseek.com/guides/thinking_mode/).
- Ordinary tool calls are available on the standard Chat API. The stricter
  `strict = true` schema guarantee is the Beta capability: it requires the
  `/beta` endpoint and every function in a request must be strict-compatible.
  Strict schema validation is independent of `tool_choice`, and DeepSeek's
  documented strict subset includes nested `anyOf`. See the
  [tool-calls guide](https://api-docs.deepseek.com/guides/tool_calls/).
- Prefix caching is automatic and depends on an exact shared prefix. Stable,
  deterministic system/tool prefixes are therefore an agent performance
  invariant. See the
  [context-cache guide](https://api-docs.deepseek.com/guides/kv_cache/).
- FIM completion is a beta API with a maximum `max_tokens` value of 4096 and
  is available only in non-thinking mode. See the
  [FIM guide](https://api-docs.deepseek.com/guides/fim_completion/) and
  [model capability table](https://api-docs.deepseek.com/quick_start/pricing/).

## Capability work in this branch

The working tree currently contains these focused improvements, each backed by
unit or integration coverage:

### DeepSeek runtime fidelity

- Keep strict chat requests on `/beta/chat/completions` in both the native
  client and app-server proxy; model discovery and health checks still use
  `/v1/models`.
- Preserve strict schemas only as an all-or-none request capability, avoiding a
  mixed request that DeepSeek rejects. Validate the documented subset by
  allowlist; an incompatible catalog falls back to ordinary tool calling with
  its schemas unchanged.
- Use a route-aware output budget: official DeepSeek V4 endpoints get an
  explicit 262,144-token product ceiling under the provider's 384K maximum,
  while custom/third-party/self-hosted routes stay at the conservative 65,536
  ceiling (and at most half their resolved context window). User overrides can
  lower these bounds but cannot raise them.
- Treat `length`, `content_filter`, and `insufficient_system_resource` finish
  reasons as incomplete turns instead of successful completion.
- Treat a reasoning-only or empty provider response as a failed turn rather
  than emitting a warning and then reporting completion.
- Reject malformed SSE data rather than converting it into empty deltas, and
  continue draining buffered burst data instead of stranding the tail.
- Accept FIM output only when the provider reports `finish_reason = "stop"`,
  enforce the 4096-token limit, and refuse to overwrite a file that changed
  while generation was in flight.
- Preserve historical tool-call reasoning across a later thinking-mode change.

### Agent-loop reliability

- Project the live todo/plan state into every model request without persisting
  duplicate runtime blocks into the transcript.
- Fail turns that exhaust the step budget before producing a final response.
- Preserve native `ToolResult.success` and metadata across root and sub-agent
  execution, so a failed tool result cannot masquerade as a successful call.
- Make large memory injection retain both stable head context and recent tail
  updates.
- Make bounded code search report truncation truthfully and ask the model to
  refine broad searches.
- Add an opt-out `verify` tool that runs a separate, tool-less DeepSeek critic
  over bounded diff/file evidence and returns a structured verdict.

## Priority roadmap

Only the following capability debt is in scope for the next development slices.
Order is intentional.

1. Add a completion gate that reconciles pending todos/plan steps with a final
   answer, while still allowing a truthful blocked outcome.
2. Count active tool schemas in request budgeting and expose their cost in
   context diagnostics.
3. Make the core editing catalog strict-compatible, especially schemas with
   root alternatives, so `/beta` strict mode can stay enabled instead of safely
   downgrading the entire request.
4. Bind completion claims to real verifier/test receipts, including native
   failure status from test and verifier tools.
5. Compact long-running sub-agent histories and return structured evidence to
   the parent without flooding its context.
6. Add offline protocol fixtures and an optional credentialed DeepSeek smoke
   lane for thinking, tool replay, strict calls, cache metrics, FIM, and retry
   behavior.

Each slice should land with a focused regression test and an entry in this
document. If a proposed change cannot be mapped to the objective or this
roadmap, keep it off `deepseek-agent`.

## Cleanup policy

Cleanup means reducing active complexity without destroying the upstream base:

- remove experiments and documentation unrelated to DeepSeek/agent capability;
- do not add new provider-specific branches unless a shared abstraction is
  required by a DeepSeek fix;
- isolate specialization in configuration, tests, and narrow runtime changes;
- keep generated files, credentials, local logs, and build outputs untracked;
- periodically rebase or merge from upstream in a clean tree and resolve only
  conflicts that touch the specialization surface.

Large-scale deletion of upstream providers, UI modules, release tooling, or
documentation needs a separate explicit decision because it raises maintenance
cost and can silently break shared agent infrastructure.
