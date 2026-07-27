# Route contract migration

Migrate the fixed actor route codec to canonical v2 while retaining strict
read-only decoding of exact v1 records.

- `encodeRoute(model, effort, scopes)` returns a detached
  `{version: 2, route: {model, effort}, scopes}` record.
- `decodeRoute(payload)` accepts exact v1
  `{version:1, model, reasoning, paths}` and exact v2 records only.
- Models are `deepseek-v4-pro` or `deepseek-v4-flash`; efforts are `high` or
  `max`; scopes are unique, lexically sorted, non-empty relative POSIX paths.
- Reject booleans as versions, aliases, unknown keys, absolute/escaping paths,
  duplicates, unsorted v2 scopes, invalid model/effort, and non-object input.
- Return detached `{model, effort, scopes}` values; do not mutate caller data
  and do not add a compatibility writer.
