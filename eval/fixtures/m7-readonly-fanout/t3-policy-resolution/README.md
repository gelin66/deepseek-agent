# Policy resolution

`resolve_policy()` reads `policies/base.json`, `policies/region.json`, and
`policies/tenant.json` in that precedence order and recursively merges nested
objects. Keys listed by `locked_keys` in the base policy are immutable at every
later layer. The returned mapping must omit the `locked_keys` metadata.

Do not hard-code the fixture values and do not mutate parsed inputs.
