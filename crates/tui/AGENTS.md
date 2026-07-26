# crates/tui — agent guidance

Scope: the TUI, the runtime engine embedded in it, and everything a user
sees. Read the repo-root `AGENTS.md` first; this file adds the rules that
are specific to this crate.

## The shell grammar (do not regress it)

The default shell is the underwater system (`src/tui/underwater.rs`,
`ocean.rs`, `widgets/`, `views/`). Its contract, in one list:

- **One owner per fact.** Route/mode/permission/context live in the header;
  Tasks/To-do in the top strip; receipts and the single live row in the
  transcript; phase/cost/detail keys in the footer. Never restate a fact in
  a second place.
- **One live row.** Settled receipts are still; only the active row and the
  footer phase mark move. Decorative motion exists only in empty idle water
  and stops the instant the user types or anything needs attention.
- **Phase is typed.** `ShellPhase::from_app` derives idle/typing/working/
  waiting/approval/done/failed from real app state. Never invent state in a
  renderer; never compare English strings to detect state (use the enums —
  the permission chip maps from canonical `RunPermissionMode`).
- **Treatment is typed.** `OceanTreatment` (ombre/flat) parses once
  from settings. Every underwater treatment keeps ambient life; appearance
  and motion (`low_motion`, `fancy_animations`) are independent axes.
- **Footer notices go through the toast system** (`push_status_toast` /
  `active_status_toast`), never the legacy `status_message` sink directly:
  toasts carry level + TTL, errors hold sticky, acknowledgements expire.
- **Compact tiers shed chrome, not content.** At small sizes a room drops
  titles/captions/spacers before it drops the object the user opened it to
  manipulate, and bodies budget from the footer's *wrapped* height
  (`wrapped_footer_lines` / `action_footer_lines`).
- **Rows are objects.** Anything selectable has a hitbox recorded at render
  time, keyboard + mouse parity, and visible focus. Destructive controls
  arm before they fire.

## Localization rules

- The complete human-facing language set is `en` and `zh-Hans`.
  `crates/localization::ProductLanguage` is the only locale owner. Resolution
  is process-wide: explicit `--language`, persisted `[ui].language`, first-run
  bilingual choice, then English for a fresh non-interactive environment.
  Do not add environment-language guessing, per-Run/per-Agent locale,
  post-hoc output translation, or another catalog.
- Every user-visible string goes through `tr(MessageId::…)`. No hardcoded
  English in render paths. Adding a string requires an enum variant,
  `ALL_MESSAGE_IDS` entry, and exact-key/placeholder-compatible entries in
  both `en.json` and `zh-Hans.json`; parity tests keep those sources
  synchronized.
- Glyphs (`▸ · ▾ ─`), key names (`Enter`, `Alt+?`), and commands
  (`/agent`) are composed in code, not embedded in translations.
- Protocol values, config keys, tool names, paths, source code, and raw tool
  output remain in their native machine-facing form.

## Verification

```sh
cargo test -p dse-tui --bins --locked            # full unit suite
cargo test -p dse-tui --test qa_pty --locked     # PTY snapshots
cargo test -p dse-tui --test release_runtime_qa --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run clippy with `--all-targets`: `--bin` alone skips test targets and lets
lints reach CI.

Real-terminal QA gotchas (learned the hard way):

- The local tmux **server** may carry `NO_COLOR=1` and `TERM=dumb` from old
  VHS runs — launch panes with `env -u NO_COLOR` or all color QA silently
  lies. tmux also force-enables the low-motion runtime overlay; prove full
  motion with `TMUX`/`TMUX_PANE` removed.
- Scripted PTY input: one Enter on the slash menu both accepts the
  highlighted match and runs it (#573). A scripted second Enter lands
  *inside* whatever modal just opened. Send one key, wait, capture.
- Judge motion from repeated captures diffed over time, never single
  screenshots. Layout gates: 40x12, 60x16, 80x24, 100x32, 140x40.
- `DSE_TUI_DEBUG=1` writes per-frame diff sizes to
  `~/.dse/logs/tui-render.log`. Streaming should be tens of cells per
  frame; a multi-thousand-cell frame is only acceptable on a genuine
  layout transition.

## Sharp edges

- `run_verifiers_background_*` can flake under full-suite parallelism;
  rerun in isolation before blaming a change.
- See the do-not-delete module list in the repo-root `AGENTS.md` before
  trusting any dead-code audit.
