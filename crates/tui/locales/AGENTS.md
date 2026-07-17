# crates/tui/locales — agent guidance

`zh-Hans.json` is the sole user-facing message catalog. This is a deliberate
single-user product boundary, not a fallback locale.

## Adding or changing a string

1. Add the `MessageId` variant, the `ALL_MESSAGE_IDS` entry, and the
   `zh-Hans.json` key. The exact-parity test must pass.
2. Write concise native Simplified Chinese. Do not add an English source key,
   fallback value, or a second catalog.
3. If a long-lived product decision later requires another language, record a
   new ADR before adding any runtime language machinery.

## Message conventions

- `{named}` placeholders stay literal; call sites substitute with
  `.replace()`.
- Product terms may remain English when that is the clearest established name.
  Plain explanatory text should be natural Simplified Chinese and stay short
  enough for footers and row controls.
- Key names (`Enter`, `Alt+?`), commands (`/fleet setup`), and glyphs are
  never in translations; they are composed in code.
- Preserve intentional leading/trailing spaces (pane titles, `Rule  `,
  the slash-menu hint).
