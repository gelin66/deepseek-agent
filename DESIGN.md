# DSE TUI Design System

> Durable presentation authority for the interactive terminal product.
> Product scope and architecture remain owned by `docs/product/PRODUCT_PLAN.md`
> and accepted ADRs. Implementation sequencing remains owned by
> `docs/product/ROADMAP.md`.

## Thesis

DSE is a quiet engineering instrument that makes one canonical task loop
legible. The transcript is the work; chrome only explains the current state,
the latest observed change, verification, required user action, and terminal
outcome.

The interface is terminal-native, not an imitation desktop app and not the
retired CodeWhale underwater world.

## Usage scene

- Mode: **Operate**.
- Primary environment: a developer working for a long time in macOS Terminal,
  iTerm2, or Ghostty.
- Primary need: understand what DSE is doing and whether the repository is
  actually verified without watching internal machinery.
- Default posture: calm, compact, and evidence-first; detail is progressive.

## Information hierarchy

Every Run follows one visible reading order:

```text
task
  -> current activity
  -> observed changes
  -> Host verification
  -> required action or terminal outcome
```

1. The transcript is always the largest region.
2. A canonical Run summary remains stable while transcript rows move.
3. The composer remains spatially stable and available whenever input is
   valid.
4. Cost, cache, model, reasoning, child details, raw output, and long evidence
   are secondary and disclosed on demand.
5. A model claim never receives the same visual status as Host evidence.

## Surface grammar

The product has three containers and one inline interruption:

### Main work surface

- One-line header: DSE identity, current task state, frozen permission.
- Transcript: user intent, model activity, tool receipts, Agent handoffs, and
  terminal outcome.
- Canonical Run summary: task, phase, observed changes, verification, Agents,
  permission, recovery.
- Composer: input plus only currently valid actions.
- Status line: one current phase, concise notice, cost/detail hints.

### Bottom sheet

Use for short, bounded decisions: permission selection and structured user
questions. Keep the transcript visible. The sheet grows only to its content
budget and becomes scrollable before it obscures the whole workspace.

### Full-screen room

Use for onboarding and sustained reading: help, long logs, evidence, and
diffs. Use a title hairline and a bottom action rail, not a centered card.
Returning closes the room and restores the previous focus and scroll state.

### Inline interruption

Approval lives between transcript and composer. It states the exact operation,
reason, boundary, and available decisions. It never becomes a generic modal.

No other renderer may invent a panel type. New capabilities join one of these
surfaces.

## Layout

### Wide

At comfortable widths, the transcript owns the main column and the canonical
Run summary forms a quiet right rail. The rail is narrower than the transcript
and has no independent business state.

### Medium

The same summary becomes a short strip above the transcript. Rows collapse
labels and secondary values before hiding primary facts.

### Narrow or short

Use one column. Shed duplicated headings, captions, borders, blank rows, and
secondary metrics in that order. Never shed the task, current phase, required
user action, terminal outcome, verification state, or composer first.

Layout is automatic. There is no user-selectable left/right/top placement,
composer density, border style, or transcript spacing.

## Color

- The terminal background is the default surface.
- Body text follows the terminal foreground.
- Muted text is used for provenance, timestamps, secondary counts, and hints.
- One DSE signal accent marks focus, current activity, selected rows, and
  navigational emphasis.
- Green means deterministic success or verified evidence only.
- Yellow means attention, approval, or recoverable warning only.
- Red means failure, denial, destructive risk, or invalid state only.
- Color is always paired with text or a stable symbol.
- ANSI-16 must remain fully usable; truecolor may refine contrast but may not
  introduce information unavailable in ANSI-16.

No gradients, ocean fields, ambient glyphs, decorative backgrounds, shadows,
community theme personalities, or filled cards.

## Type and rhythm

The host terminal owns the font. DSE does not recommend, download, or simulate
a typeface.

- Use bold for the task, current focus, and terminal outcome.
- Use dim/muted roles for metadata and hints.
- Use one blank-line rhythm between semantic groups; avoid decorative spacing.
- Keep labels short and sentence case in both languages.
- Align only when it improves scanning and remains CJK-width safe.
- Truncation is semantic and visibly marked; paths and raw output preserve
  exact bytes in their detail view.
- Do not use all caps, letter spacing, fake small caps, or ornamental Unicode.

## Borders and elevation

- Hairlines separate stable regions.
- A focused sheet or row may use one leading marker and a restrained surface
  tint.
- `Borders::ALL` is not used for ordinary pages, onboarding, transcript
  content, or generic cards.
- Nested boxes are prohibited.
- The active object has one obvious focus treatment; inactive regions recede.

## Motion and redraw

- Idle means still: no fish, bubbles, gradients, pulsing brand mark, or timer
  redraw.
- Running may show one low-frequency activity marker tied to canonical phase.
- New transcript content appears when a real event arrives.
- Completion may change state once; it does not play a decorative sequence.
- `NO_ANIMATIONS`, constrained terminals, SSH, and reduced-motion environments
  remove nonessential state motion without removing information.
- Rendering must be event-driven. A stationary idle frame produces no
  periodic disk writes and no periodic full-frame redraw.

## Interaction

- `↑/↓`: move through rows or scroll.
- `Enter`: submit, open, or confirm the focused object.
- `Esc`: close the current sheet/room or cancel the current bounded choice.
- `Shift+Enter` / `Alt+Enter`: newline in the composer.
- `Ctrl-C`: interrupt an active Run; otherwise clear non-empty input; otherwise
  exit.
- `Ctrl-D`: preserve the canonical cancel/exit behavior.
- `/`: open the canonical slash menu.
- `@`: open deterministic workspace mention completion.
- `/permissions` and the permission chip open the same bottom sheet.

Keyboard and mouse dispatch the same actions. Click targets come from
render-time hitboxes, focus is visible, scroll works on the region under the
pointer, and destructive actions retain their existing confirmation boundary.
DSE does not intercept macOS `Cmd` shortcuts or replace terminal selection,
paste, tabs, windows, or system accessibility.

## Copy and localization

- All human text comes from the single `en` / `zh-Hans` localization owner.
- Copy states a fact, consequence, or next action; avoid personality chatter
  and vague “working” labels.
- Protocol values, commands, flags, tool names, model IDs, paths, code, diff,
  stdout/stderr, and raw logs are not translated.
- English and Simplified Chinese must have identical message keys,
  placeholders, interaction order, and semantic emphasis.
- A narrow frame must remain usable with CJK wide characters, combining
  characters, emoji, and long repository paths.

## Component contracts

- **Header:** identity, typed state, permission; one line when possible.
- **Run summary:** canonical facts only; no guessed file count, plan, ETA, or
  confidence.
- **Transcript row:** settled receipts are still; only the current row can
  carry live activity.
- **Tool row:** concise action and result first; arguments/raw output expand.
- **Composer:** stable input, clear focus, no permanent toolbar.
- **Selection row:** name, one factual consequence, focus, current check.
- **Action rail:** only currently valid keys; no exhaustive shortcut legend.
- **Error:** what failed, whether work changed, what DSE can do next, and how
  the user can recover.
- **Empty state:** one sentence explaining what to enter; no illustration.

## Explicit exclusions

- Underwater, ocean, fish, bubbles, ambient life, or branded animation.
- General/Appearance/Theme/Layout/Advanced settings screens.
- Custom permission modes or rule editors in TUI.
- User-selectable WorkSurface placement, density, border, spacing, status
  ornament, or background color.
- Persistent calm/expanded presentation modes; individual tool details use
  one default collapse rule and process-local row expansion.
- Parallel theme/palette access in reachable renderers.
- Centered generic cards and modals.
- TUI-owned plan, progress, evidence, permission, or terminal truth.
- Hidden shortcut cycles, command palettes, or mouse-only behavior.

## Completion standard

The redesign is complete only when every reachable first-run, idle, running,
tool, approval, user-input, permission, failure, rework, completion, pager,
resume, and reopen frame uses this system, and all retired renderers, settings,
normalizers, messages, tests, and compatibility readers are physically
deleted.
