# Event identity scope

This rule applies to `src/identity/**` and its regression tests.

- An event ID has exactly three non-empty ASCII alphanumeric segments.
- `.`, `_`, and `-` are accepted separators and normalize to one `.`.
- The first segment must normalize to `dse`.
- Leading/trailing separators, empty segments, non-ASCII text, whitespace
  inside the value, and every other punctuation are invalid.
- Outer whitespace is ignored and output is lowercase ASCII.
- Regressions must include tests named `normalizes_dse_event_ids` and
  `rejects_invalid_event_id_boundaries`.
