# Header policy migration

Implement a canonical bounded forwarded-member parser across `lib.rs` and
`policy.rs`.

`parse_forwarded_chain(header, max_hops)` returns `Result<Vec<String>,
HeaderError>` and must:

- reject missing or empty headers and any `max_hops` outside `1..=8`;
- split on commas, trim ASCII spaces/tabs, and reject empty members;
- accept lowercase ASCII member identifiers of 1–63 bytes made from
  alphanumeric segments separated by single hyphens;
- reject uppercase, dots, whitespace inside members, leading/trailing/doubled
  hyphens, duplicates, and chains longer than `max_hops`;
- preserve member order.

Use stable errors `Missing`, `InvalidLimit`, `InvalidMember`, `Duplicate`, and
`TooManyHops`. Do not add dependencies or compatibility parsing.
