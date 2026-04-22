# Jira Cards View

A virtual sidebar entry below Pull Requests that lists open Jira work items assigned to the current user, backed by Atlassian's [`acli`](https://developer.atlassian.com/cloud/acli/). Read-only in v1.

## Behavior

- **Only renders in the sidebar if `acli` is on PATH** (detected once at startup via an `acli --version` probe). Users without acli never see the entry.
- Appears immediately below Pull Requests with a count badge (number of open cards).
- Mutually exclusive with Today, Upcoming, and Pull Requests.
- Backed by:
  ```
  acli jira workitem search \
    --jql "assignee = currentUser() AND statusCategory != Done AND status != Backlog ORDER BY updated DESC" \
    --fields key,summary,status,priority,issuetype \
    --limit 100 --json
  ```
  Backlog cards are filtered at the JQL layer rather than client-side — the view is for cards that are actually in flight or queued for active work, not the long tail of unprioritized items. No token is stored in ratatoist; it uses whatever `acli jira auth login` has configured.
- Fetch runs at app startup and then **polls in the background every `jira_cards_poll_interval_secs` seconds** (default **10s**, minimum 5s). Idle-gated: if the user is idle past `idle_timeout_secs`, the poll is deferred and fires on their next keystroke. Manual `r` still triggers an immediate fetch.
- Loading / error / empty states render in the pane:
  - **Loading:** "Fetching Jira cards…"
  - **Error:** stderr from acli is surfaced, with a hint to run `acli jira auth login` if unauthenticated.
  - **Empty:** "No open Jira cards assigned to you."

## Display

- Cards grouped by project key (`PROJ`, parsed from the issue key's prefix).
- Each row shows: issue-type glyph (✦ Bug / ◈ Story / ☐ Task / ▼ Epic / ⤷ Sub-task), `PROJ-123` key, summary, status, priority (omitted if `Medium` to reduce noise).
- `j` / `k` / arrows navigate the list. Selection is highlighted; project headers and spacers are skipped.

## Actions

- **`Enter`** — open the selected card in the default browser via `acli jira workitem view <KEY> --web`.
- **`r`** — re-run the acli fetch. Disabled while a fetch is in progress.
- **`Esc`** — returns focus to the sidebar.

## Non-goals (v1)

- Transitioning cards, commenting, assigning, or logging work from the TUI.
- Showing cards where the user is only a reporter or watcher.
- Sprint / board / filter picker.
- Per-project JQL customization (the query is hard-coded to "assigned to me, not Done").
- Showing subtasks, comments, or custom fields in a detail pane.
