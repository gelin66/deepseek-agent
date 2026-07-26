# Wire-version scope

This rule applies to `src/wire/**` and its regression tests.

- A wire version is a non-empty sequence of ASCII alphanumeric segments.
- `.`, `_`, and `-` are accepted separators and normalize to one `-`.
- Empty segments, leading/trailing separators, non-ASCII text, whitespace
  inside the value, and every other punctuation are invalid.
- Output is lowercase ASCII.
- Regressions must include tests named `normalizes_supported_separators` and
  `rejects_empty_segments_and_non_ascii`.
