## Problem and owner

Describe the user-visible problem, the single canonical owner, and the current
path being replaced.

## Acceptance and evidence

List deterministic tests, verifier/evaluation evidence, and exact outcomes.
State explicitly when no live DeepSeek evaluation applies.

## Cutover and deletion

List migrated real callers, deleted old paths, remaining risks, and how this
change can be removed if later evidence rejects it.

## Checklist

- [ ] I read [CONTRIBUTING.md](../CONTRIBUTING.md) and the product authority.
- [ ] This change keeps one official DeepSeek backend, one Runtime, one Store,
      and one completion authority.
- [ ] I did not restore model Auto, a provider fallback, a second prompt branch,
      or an unbounded compatibility path.
- [ ] Focused and owning-crate tests pass.
- [ ] Workspace formatting, strict Clippy, tests, and `git diff --check` pass
      when this is an integration checkpoint.
- [ ] Current public docs remain consistent with
      [README.zh-CN.md](../README.zh-CN.md).
- [ ] No credential, private source, raw provider payload, local state,
      generated package, or external Cargo target is tracked.
