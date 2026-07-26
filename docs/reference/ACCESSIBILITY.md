# Accessibility

> Category: current user reference.

DSE TUI runs in a terminal, so the platform's own accessibility
stack (screen readers, magnifiers, terminal-level themes) does most
of the work. DSE inherits terminal colors and keeps one fixed surface grammar.
Only real compatibility and motion-safety controls remain configurable.

## Quick reference

| Toggle | Default | Effect |
| --- | --- | --- |
| `NO_ANIMATIONS=1` env var | unset | At startup, forces `low_motion = true`. Overrides the saved value. |
| `DSE_ASCII_SAFE=1` env var | unset | Replaces decorative Unicode and box-drawing marks with narrow ASCII at the terminal backend. Labels, focus, state, and controls remain available. |
| `low_motion` setting | `false` | Freezes state markers without changing upstream model-text delivery. SSH, Termius, and legacy console detection may force it on. |
| `show_thinking` setting | `false` | Set to `true` to show model `reasoning_content` blocks. |

Appearance themes, backgrounds, status ornaments, and persistent tool-detail
modes are intentionally not settings. Tool runs use one compact default and
can be expanded only in the current process.

## Standard env-var surface

Set these in your shell profile so they apply to every session:

```bash
# Force low motion.
export NO_ANIMATIONS=1

# Force the terminal-safe ASCII rendering tier.
export DSE_ASCII_SAFE=1

# Optional: respect the wider terminal-color convention.
export NO_COLOR=1            # honored by the underlying ratatui backend
```

`NO_ANIMATIONS` accepts any of `1`, `true`, `yes`, or `on`
(case-insensitive). Any other value (including `0`, `false`, empty,
or unset) leaves your saved settings alone.

The override is applied once at startup. Changing the env var
mid-session has no effect — settings are only re-read on the next
launch.

## Configuring persisted settings

Edit `~/.dse/settings.toml` directly and restart the TUI. For example:

```toml
low_motion = true
```

The `NO_ANIMATIONS` env var still wins at startup if it's set, so
unsetting the env var is the way to honor your saved choice.

Termius and SSH sessions automatically start in low-motion mode because remote
rendering can exhibit visible redraw flicker during active turns. This runtime
override is reapplied on each launch.

## Notes for screen-reader users

* `low_motion` freezes state markers without synthesizing or throttling model
  text. The terminal-native shell has no idle animation or periodic content
  mutation.
* The transcript is pure text — no images or canvas rendering — so
  any terminal that integrates with the platform's accessibility
  service (e.g. macOS Terminal.app, iTerm2, Ghostty, Windows
  Terminal) will pass the rendered content straight through.
* If you find a UI surface that still produces motion when
  `low_motion = true`, report it in the DSE owner repository with a
  screenshot or terminal recording.

## Related issues / history

The imported upstream issue links were removed at the V1 product-identity
cutover. This page describes only current DSE behavior.
