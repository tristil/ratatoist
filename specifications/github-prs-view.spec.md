# GitHub Pull Requests View

A virtual sidebar entry below Upcoming that lists open GitHub pull requests authored by the current `gh` user. Read-only in v1.

## Behavior

- Appears in the sidebar immediately below Upcoming with a count badge (number of open PRs).
- Mutually exclusive with Today and Upcoming — selecting one deactivates the others.
- Backed by `gh search prs --author @me --state open --limit 100 --json ...`. Requires `gh` on the user's PATH and an authenticated session; no token is stored in ratatoist.
- Fetch happens on **view activation** and on **manual refresh** (`r` while the view is focused). No background polling, no refresh on every sync.
- While the fetch is in flight the pane shows "Fetching pull requests…". If gh fails the stderr is surfaced in the pane with a "Press r to retry." hint.
- Empty state: "No open pull requests."

## Display

- PRs grouped by repository (`owner/name` heading).
- Each row shows: status icon (● open / ◌ draft), `#number`, title, `@author`, relative update time (`3h ago`, `2d ago`).
- `j` / `k` (or arrow keys) navigate the list. Selection is visually highlighted; repo headers and blank rows are skipped.

## Actions

- **`Enter`** — open the selected PR in the default browser via `gh pr view <url> --web`. No other interactions in v1.
- **`r`** — re-run the gh fetch. Disabled while a fetch is already in progress.
- **`Esc`** — returns focus to the sidebar (same as elsewhere).

## Non-goals (v1)

- Reviewing, commenting, or merging from the TUI.
- Showing PRs where the user is only a reviewer or mentioned.
- Repo-scoped filtering based on ratatoist's working directory.
- Background refresh or websocket-driven updates.
- Showing checks, reviewers, or diff details inline (a future detail pane could).
