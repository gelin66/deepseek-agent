# crates/localization/locales — agent guidance

`en.json` and `zh-Hans.json` are the complete user-facing catalog set accepted
by ADR-0010. Do not add locale aliases, automatic environment detection, a
third catalog, or post-hoc translation.

## Adding or changing a string

1. Add the `MessageId` variant, the `ALL_MESSAGE_IDS` entry, and the same key
   to `en.json` and `zh-Hans.json`.
2. Keep the exact named-placeholder multiset identical in both values.
3. Write concise native English and native Simplified Chinese; do not use one
   language as the runtime fallback text for the other.
4. Changing the admitted language set requires a new product decision and ADR.

## Message conventions

- `{named}` placeholders stay literal; call sites substitute with
  `.replace()`.
- Product terms may remain English when that is the clearest established name.
  Plain explanatory text should be natural Simplified Chinese and stay short
  enough for footers and row controls.
- Key names (`Enter`, `Alt+?`), commands (`/agent`), and glyphs are
  never in translations; they are composed in code.
- Preserve intentional leading/trailing spaces (pane titles, `Rule  `,
  the slash-menu hint).
