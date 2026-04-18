# Star Jar

A passive counter in the left sidebar that earns a star every time the user completes a task, resets at local midnight, and persists within the day across app restarts. Read-only; there are no actions on the jar itself — only the side effect of task completion fills it.

## Behavior

- Every successful task completion (the `item_close` enqueue path — both non-recurring and recurring completions) adds **one star** to today's jar.
- Reopening a task (`item_reopen`) does **not** subtract a star. The jar is a one-way tally of effort put in, not a reconciled net score — undoing a completion is still work done.
- The jar resets when the local date rolls over. Detection is lazy: on any read (render, increment, load) the stored `star_date` is compared against today's local date; if they differ, the count is replaced with zero and the date updated.
- Count and date persist in `ui_settings.json` under `star_count` and `star_date` (ISO `YYYY-MM-DD`). Survives app restart — close and reopen within the same day and the jar keeps its count.
- Ephemeral mode (the `--ephemeral` harness used in tests and throwaway sessions) skips the save, like every other ui_settings write.

## Display

- Rendered as a dedicated block at the bottom of the left sidebar, below the Stats block. Uses the same rounded-border block styling as Stats.
- Title: ` Star jar `.
- Content: a single line `★ <count> today` when count > 0; when count is zero, `—` rendered in the muted style so the block isn't distracting at the start of a day.
- No selection, no highlight, no keybinding. The block is purely informational.

## Actions

- None. The jar has no focusable behavior — `Tab` / `Shift+Tab` skip it.

## Out of Scope (v1)

- Streaks across days, weekly totals, history views, or any notion of "yesterday's jar."
- Per-project or per-priority star weighting. One completion = one star, regardless of the task.
- Export, sharing, or sync of the count between machines — it's purely local UI state.
- Animations, sounds, or reward cues when a star lands. The count simply ticks up on the next render.
- A Michelin-inspired "rating" interpretation. It's a jar that fills; not a ranking.
