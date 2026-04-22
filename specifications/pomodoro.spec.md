# Pomodoro Timer

A single-slot 25-minute pomodoro timer paired with a tomato counter. Hitting `p` starts it; hitting `p` again while it's running cancels it (no tomato). When the timer reaches zero, a tomato lands in the Pomodoro box that sits directly above the Star Jar. The tomato count resets at local midnight, matching the Star Jar's semantics.

## Behavior

- **Duration is fixed at 25:00.** Not configurable in v1 — the whole point of pomodoro is the canonical length.
- **`p` toggles** the timer: start if none is running, cancel if one is running. Cancellation throws away the elapsed time; no partial credit. There's no pause/resume.
- `p` is only meaningful outside of modal input. While the add-task modal, comment input, detail pane, help overlay, or settings panel has focus, `p` is ignored (or, where a field captures literal characters, typed as normal).
- Only one pomodoro can run at a time. There is no queue, no automatic break timer, and no cycle counter beyond the day's tomato total.
- When the clock elapses naturally, the app:
  1. Increments `tomato_count` by 1.
  2. Clears the running state.
  3. Persists the new count to `ui_settings.json` (alongside the Star Jar's fields).
  
  No sound, no modal — just the tomato appearing in the sidebar box.
- **Day rollover during a running pomodoro is fine:** the timer keeps counting. When it completes, the tomato is credited to the *current* local date — if that's the new day, so be it. The box's rollover is lazy, same as the Star Jar (compared on each tick).
- **The session survives app restart.** Start time is wall-clock and persisted (see below), so closing ratatoist 10 minutes in and reopening 5 minutes later resumes a session with 10 minutes remaining. If the user reopens after more than 25 minutes, the next `maybe_award_tomato` tick observes elapsed ≥ 25:00 and credits a tomato as if the app had been running the whole time — the work happened, even if the UI was off.
- **A second ratatoist instance picks up the running session.** Every main-loop tick reconciles in-memory state against `pomodoro_session.json`. If instance A starts a pomodoro, instance B (already running on the same machine) adopts the session on its next tick and shows the toaster with the correct countdown. Cancellation in either instance deletes the file, and the other instance clears its in-memory state silently on the following tick (no duplicate `pomodoro_cancel` event). Natural completion is best-effort coordinated: in the rare race where both instances tick past 25:00 simultaneously, both award a tomato and emit a `pomodoro_complete` event — `tomato_count` ends up correct (last writer wins on `ui_settings.json` with the same incremented value), but the events log may carry a duplicate row. Per-session task lists in the toaster are *not* shared — each instance shows the completions that happened in its own process.

## Display

### Session toaster (bottom-right of right pane)

- While a pomodoro is running, a bordered "toaster" block floats in the **bottom-right corner of the right-hand content area** (overlapping the tasks / detail pane without resizing it). Styled like the sidebar blocks — rounded borders, inactive-border color, title ` 🍅 session `.
- **Top body row: the countdown.** `🍅 24:59` — two-digit minutes and seconds, styled with the theme's `due_today()` so it reads as active. The countdown lives here; nothing in the status bar.
- **Body rows below: tasks completed during this session**, most-recent-first, one title per row. Each row is prefixed with `✓ ` in the success style. Titles are truncated to the toaster's inner width with a trailing `…` so the block never outruns its width.
- **The toaster grows downward (and the top stays anchored)** as tasks land: height is `1 (countdown) + N (tasks) + 2 (borders)`. Width is fixed at 40 columns unless the right-hand area is narrower, in which case the toaster uses whatever room is available.
- **Vanishes immediately** when the session ends — whether the timer elapsed naturally *or* the user hit `p` to cancel. There's no linger / celebration delay; the tomato lands in the sidebar box and the toaster disappears on the same frame.
- No focus, no keybinding. The toaster is read-only — purely a status surface.
- Task scope: **every completion that happens while the pomodoro is running** (any view — project, Today, All, doesn't matter) is associated with the session. If you complete 12 tasks in 25 minutes, 12 of them appear in the toaster (truncated to what fits on screen).

### Pomodoro box (left sidebar)

- Dedicated bordered block in the left column, positioned **immediately above the Star Jar** and styled the same way (rounded borders, inactive border color, title ` Pomodoros `).
- Always rendered — three rows fixed (top border + one body row + bottom border), regardless of count. When the count is zero, the body shows `—` in the muted style so the box doesn't feel broken at the start of a day.
- When the count is non-zero, the body renders one 🍅 glyph per completed tomato, space-separated, packed left-to-right. If the day's count exceeds what fits in one body row, overflow glyphs clip — "figure out a collapse tier when we hit it," same philosophy as the Star Jar's yellow-to-purple fallback.
- The pomodoro box does not grow dynamically. Unlike the Star Jar, a day with heavy tomato activity stays at three rows; we let the count clip rather than chew more sidebar space, since a typical day tops out well before the box fills up.

## Persistence

- `tomato_count: u64` and `tomato_date: YYYY-MM-DD` are stored in `ui_settings.json`, next to the Star Jar's keys.
- **Running-session state lives in `<config_dir>/pomodoro_session.json`** (next to `ui_settings.json`). Shape: `{"session_id": "<RFC3339 of start>", "started_at": <unix epoch seconds>}`. The file exists exactly when a session is running; it is created on `toggle_pomodoro` start, removed on cancel, and removed on natural completion. Ephemeral mode skips both the read and the write, like every other on-disk artifact.
- `pomodoro_remaining` is computed at render time from wall-clock (`now - started_at`), so it survives process restarts and is identical across instances reading the same file.
- **Every pomodoro start / complete / cancel also appends a timestamped line to the events log** (see [events-log.spec.md](events-log.spec.md)). Each of those three lines carries a `session_id` — the RFC 3339 timestamp of the pomodoro's start, used as a stable session identifier. The running count for today is still the in-memory `tomato_count`; the log is for cross-day retrospective stats (cancellation rate, time-of-day patterns, weekly totals).
- **Task completions during the session carry the same `session_id`** on their `task_complete` event (under the key `pomodoro_session_id`). This is how the pomodoro→tasks association is persisted: a future stats reader joins `task_complete` records to their parent pomodoro by matching `pomodoro_session_id` against the `session_id` on a `pomodoro_start`/`pomodoro_complete` pair. Completions outside a pomodoro simply don't carry the field.

## Out of Scope (v1)

- Configurable durations.
- Breaks — short (5 min) or long (after 4 tomatoes).
- Notifications, sounds, or system bell on completion.
- A history view ("tomatoes earned last week"), streaks, or weekly totals.
- Linking a tomato to the specific task you were working on.
- Per-project pomodoro counters.
