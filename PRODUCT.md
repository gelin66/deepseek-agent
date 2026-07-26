# Product

> Design context only. Product scope, architecture, milestones, and evaluation
> authority remain in `docs/product/PRODUCT_PLAN.md`, `docs/decisions/`,
> `docs/product/ROADMAP.md`, and `docs/product/EVALUATION.md`.

## Register

product

## Users

Software developers who want DeepSeek to complete real repository work from a
macOS terminal. They work in long-running coding sessions and need to
understand what the agent is doing, what changed, what was verified, and when
their input is required without reading implementation internals.

## Product Purpose

DSE is a Rust-native, local-first coding agent dedicated to the official
DeepSeek API. Its interface makes the canonical Run lifecycle legible while
keeping the execution engine, evidence, recovery, and permission truth in their
single owning modules. Success means verified task completion with low
cognitive overhead, not a busier dashboard.

## Brand Personality

Professional, calm, precise.

## Anti-references

- Dense dashboards that expose internal machinery instead of task progress.
- Blind activity indicators that say the agent is busy without explaining the
  current phase or result.
- Novel shortcuts, permission concepts, or mouse behavior that conflict with
  established macOS terminal and Codex conventions.
- Decorative panels, excessive borders, or visual effects that compete with
  the transcript.
- The retired CodeWhale underwater world: ocean gradients, fish, bubbles,
  ambient motion, and appearance knobs that make the user finish the product's
  design work.

## Design Principles

1. Make the task lifecycle obvious: intent, activity, change, verification,
   and terminal result must form one visible loop.
2. Show canonical facts only; never invent progress, file counts, plans, or
   confidence.
3. Keep the transcript primary and reveal detail progressively.
4. Prefer mature Codex and macOS interaction conventions over novel
   affordances.
5. Optimize for comfort during sustained use through restrained hierarchy,
   stable placement, and concise bilingual copy.
6. Keep one DSE-native terminal grammar across the main shell, bottom sheets,
   full-screen reading rooms, and inline approval; do not preserve a legacy
   visual mode.

## Standing Visual Direction

Use the category-standard professional terminal interaction at full fidelity:
transcript first, canonical task summary always legible, familiar keyboard and
mouse behavior, restrained semantic color, and no decorative identity layer.
Codex and mature macOS terminal tools set the craft and interaction bar, not a
source-code or branding template. DSE remains visually and architecturally
native to its own Run, evidence, recovery, and permission model.

There is no General Settings surface. Language and the three permission modes
keep their existing dedicated owners; layout, theme, spacing, borders, and
motion are coherent product defaults or terminal capability adaptations.

## Accessibility & Inclusion

Maintain readable contrast through the single terminal-adaptive token set,
never encode state by color alone, preserve keyboard-only operation, provide
equivalent mouse interaction where the terminal supports it, respect terminal
width and Unicode display width, and keep English and Simplified Chinese
message catalogs complete.
