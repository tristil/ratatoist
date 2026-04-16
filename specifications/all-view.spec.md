# All View

A combined dashboard sidebar entry that merges Today tasks, non-hidden GitHub PRs, and Jira cards into a single scrollable list with contextual actions.

## Behavior

- Appears first among the virtual entries (before Today) with a count badge totalling all items.
- Sections render in order: **Today** tasks, **Pull Requests** (non-hidden orgs), **Jira Cards**, each under a bold header row (`▸ Today`, `▸ Pull Requests`, `▸ Jira Cards`). Empty sections are omitted.
- `j` / `k` navigate across sections seamlessly. Section headers and spacers are skipped by the cursor.

## Contextual Actions

The action triggered by a key depends on the type of the currently selected item:

| Key | Task | Pull Request | Jira Card |
|-----|------|-------------|-----------|
| `Enter` | Open detail pane | Open PR in browser | Open card in browser |
| `x` | Complete task | No-op | No-op |
| `r` | Refresh PRs + Jira | Refresh PRs + Jira | Refresh PRs + Jira |

## Data Sources

- **Tasks:** Same Today filter (due today or overdue, not deleted/checked/subtask, assigned to me or unassigned, excluding pending-close-recurring).
- **PRs:** All entries from `github_prs` whose owner is not in `hidden_pr_orgs`. Indices reference the global `github_prs` vec.
- **Jira Cards:** All entries from `jira_cards`. Indices reference the global vec.

## Out of Scope (v1)

- Collapsible sections within the All view.
- Drag reordering of sections.
- Configuring which sources appear.
- Inline PR merge, Jira transitions, or other write operations beyond task completion.
