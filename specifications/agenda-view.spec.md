# Agenda View

A virtual sidebar entry below Jira Cards that lists today's events blended across all of the user's subscribed Google Calendars, backed by the [`gws`](https://github.com/google/go-workspace-cli) CLI. Read-only in v1.

## Behavior

- **Only renders in the sidebar if `gws` is configured to read the calendar** (detected once at startup by running `gws auth status` and checking that its JSON output includes `https://www.googleapis.com/auth/calendar` — or the `.readonly` variant — in the `scopes` array). If `gws` is missing from PATH, the command fails, or the calendar scope hasn't been granted, the entry stays hidden. Users who installed `gws` but never ran `gws auth login` (or who granted only non-calendar scopes) don't see a broken Agenda entry that can't fetch.
- Appears immediately below Jira Cards with a count badge (number of events today).
- Mutually exclusive with Today, Upcoming, Pull Requests, and Jira Cards.
- Backed by a two-phase fetch:
  1. **Enumerate calendars:** `gws calendar calendarList list` — filter to entries where `selected == true` and `hidden != true` (i.e. the ones the user has checked on in the Google Calendar UI and hasn't hidden). This matches "subscribed and visible" in Calendar web.
  2. **Fetch events in parallel:** for each enumerated calendar, run
     ```
     gws calendar events list --params '{
       "calendarId": "<calendar id>",
       "timeMin": "<today 00:00 local>",
       "timeMax": "<tomorrow 00:00 local>",
       "singleEvents": true,
       "orderBy": "startTime",
       "maxResults": 100
     }'
     ```
     concurrently. Results are merged into one list, sorted by start time, and rendered as a single timeline. Each event keeps a reference to its source calendar's display name so the UI can surface the origin.
  No token is stored in ratatoist; it uses whatever `gws auth login` has configured (OAuth credentials live in `~/.config/gws/`). The GCP project baked into those credentials is the user's own — ratatoist never picks one.
- `timeMin` / `timeMax` are computed in the user's **local timezone** so "today" matches what the user sees in Google Calendar web. `singleEvents=true` expands recurring series into concrete occurrences.
- **Partial failures are non-fatal.** If enumerating calendars fails, the whole fetch errors. If individual per-calendar event fetches fail, successful ones still render and failures are logged; only when *every* calendar errors is the view put in the error state.
- Fetch runs at app startup and then **polls in the background every `agenda_poll_interval_secs` seconds** (default **300s**, minimum 30s). Calendars change slowly compared to tasks and PRs; a 5-minute cadence stays well under the Calendar API quota without feeling stale. Idle-gated: if the user is idle past `idle_timeout_secs`, the poll is deferred and fires on their next keystroke.
- Cancelled instances of recurring events (API status `cancelled`) are filtered out before render.
- Loading / error / empty states render in the pane:
  - **Loading:** "Fetching today's agenda…"
  - **Error:** stderr from gws is surfaced, with a hint to run `gws auth login` if unauthenticated.
  - **Empty:** "No events scheduled for today."

## Display

- Events ordered by start time after merging across calendars. No grouping — the list is short enough that sections would add noise.
- Each row shows: compact local time (`9`, `9:30`, or `all day`), summary, optional calendar name (`· Work`), optional location (`· Main St.`). Calendar name is muted and only shown when more than one distinct calendar contributes events to the current list — if everything today is on one calendar, the chip is redundant and hidden.
- Time column is padded to the widest label so summaries line up.
- `j` / `k` / arrows navigate the list. Selection is highlighted.

## Actions

- **`Enter`** — open the selected event in the default browser via `open <htmlLink>` (macOS). No edit in v1.
- **`r`** — re-run the gws fetch. Disabled while a fetch is in progress.
- **`Esc`** — returns focus to the sidebar.

## All View Integration

- Today's agenda events appear at the **top** of the All view under a bold `▸ Agenda` section, ahead of the Today tasks section. Events are time-bound and typically the most schedule-sensitive thing in the dashboard, so they lead.
- Each All-view agenda row shows the compact time label and summary (location omitted in the tight dashboard layout).
- `Enter` on an agenda row in the All view opens the event in the browser, same as the focused Agenda view.

## Non-goals (v1)

- Creating, editing, responding to, or deleting events.
- Per-calendar filtering or color-coding in the agenda view — all subscribed calendars blend into one list. Users manage inclusion in Google Calendar web by checking / hiding calendars there.
- Week / month views. This is "today" only; Upcoming (for tasks) has no calendar counterpart yet.
- Showing attendees, description, or other event details in a detail pane.
- Cross-platform browser open (macOS only in v1 — Linux / Windows would need `xdg-open` / `start`).
