# Star Jar

A passive counter in the left sidebar that earns a star every time the user completes a task, resets at local midnight, and persists within the day across app restarts. Read-only; there are no actions on the jar itself — only the side effect of task completion fills it.

## Behavior

- **During the day, Todoist is the source of truth.** The jar reflects the number of tasks the user has actually completed today (local time) across every project, not a local tally. A background poll refetches completed tasks on `star_jar_poll_interval_secs` (default **60s**, minimum 30s) and replaces `star_count` with the fresh count. Idle-gated: if the user is idle past `idle_timeout_secs`, the poll is deferred and fires on their next keystroke.
- **Optimistic local updates.** To keep the UI snappy between polls, `item_close` increments the local count immediately and `item_reopen` decrements it (saturating at zero). These are not authoritative — the next poll reconciles with Todoist's actual count, so a rapid close-then-reopen settles at the right number even if the optimistic math drifts briefly.
- **After the day is over, the books are closed and the local store becomes the source of truth.** Once the date rolls over, Todoist is no longer consulted for that date's count — the frozen number is what we keep.
- **On the first tick in a new local day, one final pass closes the books on the previous day.** `roll_star_jar_if_new_day` detects the rollover lazily (called from main-loop ticks, render paths, and on startup), captures the previous date and local count, resets `star_count` to 0 and `star_date` to today, and spawns a close-books fetch for the previous date. When that fetch returns, a `star_jar_close` record is appended to the events log with the authoritative count; if the fetch fails (offline, API error), the local count we captured is written instead so there's always a per-day record.
- **Historical record lives in the events log**, not a separate store. See [events-log.spec.md](events-log.spec.md) — `star_jar_close` gives one row per completed day. No Todoist API access is needed to read history: a future stats view aggregates the log.
- Count and date persist in `ui_settings.json` under `star_count` and `star_date` (ISO `YYYY-MM-DD`). Survives app restart — close and reopen within the same day and the jar keeps its count.
- Ephemeral mode (the `--ephemeral` harness used in tests and throwaway sessions) skips both the save and the events-log append, like every other ui_settings write.
- **Every increment also appends a timestamped line to the events log** — `{"ts": "...", "kind": "task_complete", "task_id": "<todoist task id>"}`. The log is the durable record; the jar's count is ephemeral state that can be reconstructed from the log plus a Todoist poll.

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
