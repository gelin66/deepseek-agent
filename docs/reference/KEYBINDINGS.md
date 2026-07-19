# Keybindings

> Category: current user reference.

This is the source-of-truth catalog of every keyboard shortcut the TUI recognizes. Bindings are grouped by **context** — the focus or modal state they fire in. A binding listed under "Composer" only takes effect when the composer is focused; one under "Transcript" only when the transcript has focus; and so on.

Global key chords are not yet user-configurable — tracked for a future release (#436, #437).

## Global (any context)

| Chord                | Action                                                        |
|----------------------|---------------------------------------------------------------|
| `F1` or `Ctrl-/`     | Toggle the help overlay                                       |
| `Ctrl-C`             | Interrupt current turn / dismiss modal / quit when idle        |
| `Ctrl-D`             | Quit (only when the composer is empty)                         |
| `Ctrl-T`             | Cycle reasoning effort for the active provider. DeepSeek-style providers cycle off → high → max → off; OpenAI Codex cycles low → medium → high → xhigh → low. |
| `Ctrl-R`             | Open the resume-session picker                                 |
| `Ctrl-L`             | Refresh / clear the screen                                     |
| `Alt-G`              | Scroll transcript to top when the composer is empty             |
| `Alt-!` / `Alt-@` / `Alt-#` / `Alt-$` / `Alt-0` | Focus Pinned / Tasks / Agents / Context / Auto sidebar |
| `Ctrl-Alt-0`         | Hide/show the pinned sidebar                                    |
| `Esc`                | Close topmost modal · cancel slash menu · dismiss toast        |

## Composer

Editing the message you're about to send.

| Chord                       | Action                                                  |
|-----------------------------|---------------------------------------------------------|
| `Enter`                     | Start a run, or submit a canonical steer to the active run |
| `Shift-Enter` / `Alt-Enter` | Insert a newline without sending                         |
| `Ctrl-W`                    | Delete previous word                                    |
| `Ctrl-A` / `Home`           | Move to start of input                                  |
| `Ctrl-E` / `End`            | Move to end of input                                    |
| `Ctrl-V` / `Cmd-V`          | Terminal text paste (`Event::Paste` or normal text input)|
| `Tab`                       | Slash-command / `@`-mention completion (popup-aware)    |

### `@` mentions

Type `@<partial>` to open the file mention popup. `↑`/`↓` select an entry,
`Tab` or `Enter` completes the composer text, and `Esc` hides the popup without
changing the input. Completion uses deterministic workspace ranking. Accepting
a candidate does not send a request; press `Enter` again to submit the exact
`@path` text.

### `#` quick-add (memory)

When `[memory] enabled = true`, typing `# foo` and pressing `Enter` appends `foo` as a timestamped bullet to your memory file *without* sending a turn. See `docs/reference/MEMORY.md`.

## Transcript (when transcript has focus)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Scroll one line (v0.8.13+: bare arrows also scroll when composer empty) |
| `PgUp` / `PgDn`      | Scroll one page                                    |
| `Home` / `g`         | Jump to top                                         |
| `End` / `G`          | Jump to bottom                                     |
| `Esc`                | Return focus to composer                           |
| `y`                  | Yank selected region to clipboard                  |
| `v`                  | Begin / extend visual selection                    |
| `Cmd-click` (macOS) / `Ctrl-click` (Linux/Windows) | Open an OSC 8 link in a supporting terminal (terminal-owned) |

## Sidebar (when sidebar has focus)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Move selection                                     |
| `Enter`              | Activate the selected item (open / focus / cancel) |
| `Tab`                | Cycle to next sidebar panel (Work → Tasks → Agents → Context) |
| `Esc`                | Return focus to composer                           |

## Slash-command menu (after typing `/`)

| Chord                          | Action                                              |
|--------------------------------|-----------------------------------------------------|
| `↑` / `↓` / `Ctrl+P` / `Ctrl+N`| Move selection                                     |
| `Enter` / `Tab`                | Run / complete the highlighted command             |
| `Esc`                          | Dismiss palette                                     |

## Session Picker (`Ctrl-R` or `/sessions`)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Move selection in the session list                 |
| `1`-`9`              | Open the visible session history at that list slot |
| `PgUp` / `PgDn`      | Page the history pane                              |
| `Enter`              | Resume the selected session                        |
| `/`                  | Search sessions                                    |
| `s`                  | Cycle sort order                                   |
| `a`                  | Toggle current-workspace scope vs all workspaces   |
| `d`                  | Delete selected session after confirmation         |
| `Esc` / `q`          | Close the picker                                   |

## Approval modal (when a tool requests approval)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Select an available decision                       |
| `Enter`              | Submit the selected decision                        |
| `y` / `Y` / `1`      | Approve once                                        |
| `n` / `N` / `d` / `D` / `2` | Deny the tool call                         |
| `v` / `V`            | View parameters in the pager                        |
| `Esc`                | Abort the current turn                              |

## Onboarding (first-run flow)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `Enter`              | Advance to the next required step (Welcome → API-key gate → workspace-trust gate → setup tips/checkpoint; optional gates are skipped when already satisfied) |
| `Esc`                | Step back one screen                                |
| `y` / `Y`            | Trust the workspace (Trust step)                   |
| `n` / `N`            | Skip the trust prompt                              |

The onboarding interface is always Simplified Chinese. It has no language
selection step or runtime language shortcut.

## v0.8.29 audit notes

- **`Shift+Enter` / `Alt+Enter` newlines now work in VSCode on Windows (#1359).** crossterm's `PushKeyboardEnhancementFlags` command unconditionally returns `Unsupported` on Windows (`is_ansi_code_supported() == false`), so the Kitty keyboard protocol escape was never written to the terminal. Without it, VSCode's xterm.js stays in legacy mode where `Shift+Enter` is indistinguishable from plain `Enter`, causing the composer to send the message instead of inserting a newline. The fix writes the push/pop escapes (`\x1b[>1u` / `\x1b[<1u`) directly on Windows, bypassing crossterm's capability gate. VSCode integrated terminal and Windows Terminal ≥1.17 both honour the Kitty keyboard protocol; terminals that do not understand the sequences silently discard them.

## v0.8.13 audit notes

- **Phantom `Alt+Up` removed.** The "Edit last queued message" binding was listed in README but never existed in the key dispatch code.
- **Bare Up/Down only scroll the transcript when the composer is empty.** The current canonical input loop has no prompt-recall binding.
- **Configurable keymap (#436) and `tui.toml` (#437) remain deferred.** The `TuiPrefs` struct and loader exist in `settings.rs` but are not wired at startup. The named-binding registry that would let `~/.codewhale/tui.toml` override individual entries is still pending.
