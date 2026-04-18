# All View

A combined dashboard sidebar entry that merges today's calendar events, Today tasks, non-hidden GitHub PRs, and Jira cards into a single scrollable list with contextual actions.

## Behavior

- Appears at the **top of the sidebar** — above the personal header and Inbox — with a count badge totalling all items.
- **Selected by default at startup** so the TUI lands on the dashboard view rather than on the first project.
- Sections render in order: **Agenda** (today's calendar events, most time-sensitive), **Today** tasks, **Pull Requests** (non-hidden orgs), **Jira Cards**, each under a bold header row (`▸ Agenda`, `▸ Today`, `▸ Pull Requests`, `▸ Jira Cards`). Empty sections are omitted.
- `j` / `k` navigate across sections seamlessly. Section headers and spacers are skipped by the cursor.

## Contextual Actions

The action triggered by a key depends on the type of the currently selected item:

| Key | Agenda Event | Task | Pull Request | Jira Card |
|-----|--------------|------|-------------|-----------|
| `Enter` | Open event in browser | Open detail pane | Open PR in browser | Open card in browser |
| `x` | No-op | Complete task | No-op | No-op |
| `r` | Refresh all sources | Refresh all sources | Refresh all sources | Refresh all sources |
| `a` | Open add modal (empty defaults) | Open add modal (due = `today`, project = Inbox — see [today-view-add.spec.md](today-view-add.spec.md)) | Open add modal (empty defaults) | Open add modal (empty defaults) |

## Data Sources

- **Agenda Events:** All entries from `agenda_events` — today's events from the user's primary Google Calendar (see `agenda-view.spec.md` for the fetch details). Indices reference the global vec.
- **Tasks:** Same Today filter (due today or overdue, not deleted/checked/subtask, assigned to me or unassigned, excluding pending-close-recurring). **Tasks labeled `evening` are hidden until 17:00 local time** so they don't clutter daytime planning; they reappear after 5 PM. **Tasks labeled `work` are hidden on Saturdays and Sundays** so work doesn't leak into weekend time; they reappear on Monday. Both label filters match case-sensitively on the exact lowercase string.
- Task rows render their **label chips** in the same style as the Today view (one chip per label, colored by the label's theme color), so label-based context (contexts like `home`, `@errand`, project tags) is visible in the dashboard, not only in the focused Today/project view.
- **PRs:** All entries from `github_prs` whose owner is not in `hidden_pr_orgs`. Indices reference the global `github_prs` vec.
- **Jira Cards:** All entries from `jira_cards`. Indices reference the global vec.

## Out of Scope (v1)

- Collapsible sections within the All view.
- Drag reordering of sections.
- Configuring which sources appear (beyond the CLI-presence gating that hides entire sections when `gh`, `acli`, or `gws` is missing).
- Inline PR merge, Jira transitions, or other write operations beyond task completion.
