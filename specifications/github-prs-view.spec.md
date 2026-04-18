# GitHub Pull Requests View

One virtual sidebar entry per GitHub owner (user or org) that has open pull requests either **authored by** or **assigned to** the current `gh` user. Read-only in v1.

## Behavior

- **Only renders in the sidebar if `gh` is on PATH** (detected once at startup via a `gh --version` probe). Users without gh never see any PR entry.
- Fetch runs at app startup and then **polls in the background every `github_prs_poll_interval_secs` seconds** (default **20s**, clamped to a 5s minimum). Each poll makes three API calls:
  1. `gh search prs --author @me --state open --archived=false --limit 100 --json id,...` — PRs you opened.
  2. `gh search prs --assignee @me --state open --archived=false --limit 100 --json id,...` — PRs assigned to you (even if someone else opened them).
  3. One batched `gh api graphql` request that looks up `statusCheckRollup.state` for every PR returned above via aliased `node(id:…)` selections — one round-trip regardless of PR count.

  Author and assignee results are merged and deduped by URL. The sidebar populates with one entry per owner as soon as the response lands; before then, no PR entries appear.
- Both search queries must succeed — a partial failure surfaces as a fetch error rather than a silently truncated list. The GraphQL call is best-effort: if it fails the PRs still render, just without CI glyphs.
- Polling honors idle state: if the user has been idle longer than `idle_timeout_secs`, the poll is skipped and fires on their next keystroke instead (same pattern as the Todoist WebSocket sync).
- Rate-limit math: GitHub's search endpoint allows 30 req/min for authenticated users. At a 20s interval that's 2 search + 1 graphql = 9 req/min — 30% of the budget, leaving plenty of room for `r` refreshes and retries.
- One entry per owner, sorted alphabetically. An owner with zero open PRs gets no entry — so `cxrlos` shows up alongside `appfolio`, each with its own list. Personal accounts are just another owner.
- Each sidebar row shows the owner name and an open-PR count badge for that owner.
- Mutually exclusive with Today, Upcoming, Jira Cards, and Agenda.
- Archived repos are excluded (`--archived=false`) so unmergeable PRs don't linger.
- A manual refresh (`r` while the view is focused) re-runs the fetch immediately — useful after closing a PR if you don't want to wait for the next poll.
- While the fetch is in flight the pane shows "Fetching pull requests…". If gh fails the stderr is surfaced in the pane with a "Press r to retry." hint.
- Empty state within an org's view: "No open pull requests in this org."

## Display

- PRs within an org view are grouped by repository (`owner/name` heading).
- Each row shows: draft/open icon (● open / ◌ draft), CI status glyph (✓ success / ✗ failure / · pending, blank when unknown), `#number`, title, `@author`, relative update time (`3h ago`, `2d ago`).
- CI status maps from GitHub's `StatusState` enum: `SUCCESS` → ✓, `FAILURE`/`ERROR` → ✗, `PENDING`/`EXPECTED` → ·. Other states render blank.
- `j` / `k` (or arrow keys) navigate the list. Selection is visually highlighted; repo headers and blank rows are skipped.

## Actions

- **`Enter`** — open the selected PR in the default browser via `gh pr view <url> --web`. No other interactions in v1.
- **`r`** — re-run the gh fetch. Disabled while a fetch is already in progress.
- **`h`** (with cursor on a PR org row in the sidebar) — hide that organization from the sidebar. Persisted in `ui_settings.json` under `hidden_pr_orgs`. **No unhide UI in v1** — to bring an org back, edit `~/Library/Application Support/ratatoist/ui_settings.json` and remove the entry from the `hidden_pr_orgs` array.
- **`Esc`** — returns focus to the sidebar.

## Non-goals (v1)

- Reviewing, commenting, or merging from the TUI.
- Showing PRs where the user is only a reviewer or mentioned.
- Per-check status detail (only the rollup is shown — no individual check names or URLs).
- Repo-scoped filtering based on ratatoist's working directory.
- Websocket-driven PR updates.
- Showing reviewers or diff details inline (a future detail pane could).
