# Star Jar

A passive counter in the left sidebar that earns a star every time the user completes a task, resets at local midnight, and persists within the day across app restarts. Read-only; there are no actions on the jar itself — only the side effect of task completion fills it.

## Behavior

- Every successful task completion (the `item_close` enqueue path — both non-recurring and recurring completions) adds **one star** to today's jar.
- Reopening a task (`item_reopen`) does **not** subtract a star. The jar is a one-way tally of effort put in, not a reconciled net score — undoing a completion is still work done.
- The jar resets when the local date rolls over. Detection is lazy: on any read (render, increment, load) the stored `star_date` is compared against today's local date; if they differ, the count is replaced with zero and the date updated.
- Count and date persist in `ui_settings.json` under `star_count` and `star_date` (ISO `YYYY-MM-DD`). Survives app restart — close and reopen within the same day and the jar keeps its count.
- Ephemeral mode (the `--ephemeral` harness used in tests and throwaway sessions) skips the save, like every other ui_settings write.
- **Every star also appends a timestamped line to the events log** (see [events-log.spec.md](events-log.spec.md)) — `{"ts": "...", "kind": "task_complete", "task_id": "<todoist task id>"}`. The current day's jar is still derived from `star_count` for rendering speed; the log is the durable record for future retrospective stats.

## Display

- Rendered as a dedicated block at the bottom of the left sidebar, below the Stats block. Uses the same rounded-border block styling as Stats.
- Title: ` Star jar `.
- Content: one ★ glyph per earned star (styled yellow), packed across the block's inner width with a single space between glyphs. Rows fill left-to-right, top-to-bottom — the block height equals `ceil(count / stars_per_row).max(1)` body lines plus two borders, so the jar **grows upward** into the project list as the day's count climbs. When count is zero, the body is a single muted `—` row so the block doesn't disappear at the start of a day.
- No numeric count rendered — users read the jar visually. Individual stars are the point.
- No selection, no highlight, no keybinding. The block is purely informational.
- **Overflow collapse — 5 yellow stars → 1 purple star.** When the jar would otherwise grow taller than the available budget in the sidebar (computed as `left_area.height − stats − borders − a small floor reserved for the project list`), the rendering switches to a *collapsed* representation: every five completions are drawn as one purple ★, with any remainder (count mod 5) shown as trailing yellow stars. The switch is all-or-nothing — once collapse is active, every star in the jar renders as purple-or-yellow, not a mix where some groups of five are collapsed and others aren't.
- If collapse itself runs out of room (enough purple stars to overflow the budget), the block simply clips at its allotted height. The plan for this case is: figure it out when we hit it. A second collapse tier (e.g. 25 yellows → 1 gold star) is an obvious next step.

## Actions

- None. The jar has no focusable behavior — `Tab` / `Shift+Tab` skip it.

## Out of Scope (v1)

- Streaks across days, weekly totals, history views, or any notion of "yesterday's jar."
- Per-project or per-priority star weighting. One completion = one star, regardless of the task.
- Export, sharing, or sync of the count between machines — it's purely local UI state.
- Animations, sounds, or reward cues when a star lands. The count simply ticks up on the next render.
- A Michelin-inspired "rating" interpretation. It's a jar that fills; not a ranking.
