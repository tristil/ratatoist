# Events Log

Append-only log of stat-worthy events (task completions, pomodoro starts / completions / cancellations) so a future "history" or "insights" view can aggregate them. No reader exists yet — this spec is purely about producing a durable record as events happen.

## Storage

- **File:** `<config_dir>/events.jsonl` — same directory as `ui_settings.json`. One record per line (JSON Lines / NDJSON).
- **Append-only.** Every event opens the file with `append + create` and writes one line. No reader merges, no rewrites, no compaction.
- **No rotation, no retention cap in v1.** Even a heavy user writes ~50 events/day → the file stays small (tens of KB/year). Revisit when the stats view ships.
- **Ephemeral sessions skip the write entirely**, mirroring `save_ui_settings`. Tests and throwaway runs never litter disk.
- **Write failures are silent** (logged via `tracing::warn`) — the log is a nice-to-have, not critical state. Losing one event because of a disk-full or permission error never blocks the user.

## Record shape

Each line is a single JSON object. **Required** fields on every record:

- `ts` — RFC 3339 local-time string with offset (`2026-04-18T14:32:17-04:00`), produced by `chrono::Local::now().to_rfc3339()`. Local rather than UTC so stats-by-hour-of-day work without timezone math downstream.
- `kind` — enum string. Current values: `task_complete`, `pomodoro_start`, `pomodoro_complete`, `pomodoro_cancel`. New kinds may be added; readers must ignore unknown kinds.

**Per-kind fields:**

- `task_complete` adds `task_id` — the Todoist task ID that was closed. Lets a future view link a completion back to the originating task (title, project, priority, labels — all live in the Todoist sync state). When the completion happened *during* an active pomodoro, the record also carries `pomodoro_session_id` — the `session_id` of the running pomodoro, as an RFC 3339 timestamp. Completions outside a pomodoro omit the field.
- `pomodoro_start`, `pomodoro_complete`, `pomodoro_cancel` each carry a `session_id` — the RFC 3339 timestamp of the pomodoro's start, serving as a stable identifier across its lifecycle. A reader can pair a `start` with its matching `complete` or `cancel` by equal `session_id`, and join all `task_complete` rows whose `pomodoro_session_id` matches.

Example lines:

```jsonl
{"ts":"2026-04-18T09:14:02-04:00","kind":"pomodoro_start","session_id":"2026-04-18T09:14:02-04:00"}
{"ts":"2026-04-18T09:21:04-04:00","kind":"task_complete","task_id":"7891234501","pomodoro_session_id":"2026-04-18T09:14:02-04:00"}
{"ts":"2026-04-18T09:39:02-04:00","kind":"pomodoro_complete","session_id":"2026-04-18T09:14:02-04:00"}
{"ts":"2026-04-18T09:41:17-04:00","kind":"task_complete","task_id":"7891234567"}
{"ts":"2026-04-18T10:05:00-04:00","kind":"pomodoro_start","session_id":"2026-04-18T10:05:00-04:00"}
{"ts":"2026-04-18T10:12:44-04:00","kind":"pomodoro_cancel","session_id":"2026-04-18T10:05:00-04:00"}
```

## Non-goals (v1)

- Reading the log from inside ratatoist — no history view, no weekly summary, no charts. The log is written now so the data is available when that feature is built.
- Deduplication or idempotence. If the user manages to trigger a double-emit (e.g. rapid key-repeat), both lines land; stats consumers can dedupe by `(ts, kind, task_id)` if they care.
- Cross-device sync. The log is machine-local. Two machines produce two independent logs; we do not attempt to merge them.
- Backfilling past completions from Todoist's completed-items API. The log begins empty and fills from the moment of this feature shipping.
