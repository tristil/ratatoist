# Upcoming View

A virtual sidebar entry below Today showing every scheduled task — overdue, today, and all future days — grouped by day.

## Behavior

- Appears in the sidebar immediately below Today with a count badge (total visible tasks).
- Navigating to it shows tasks from **all** projects, not just the selected one.
- Tasks are grouped by their due date with a day header per date. Header format mirrors Todoist: `"15 Apr · Today · Wednesday"`, `"16 Apr · Tomorrow · Thursday"`, `"17 Apr · Friday"`. Past dates render as `"1 Apr · Overdue · Tuesday"`.
- No forward window — the view lists every scheduled task up to whatever dates the user has set. No pagination needed for a personal task list.
- Each task row shows its source project name (cross-project layout).
- Task completion (`x`) and detail pane (`Enter`) work the same as in any project.
- Updates in real time as sync events arrive.

## Filters

- Only includes tasks where `responsible_uid` is `None` (personal) or matches the current user (assigned to me in shared projects).
- Excludes completed, deleted, and undated tasks.
- Excludes subtasks (parent tasks only).

## Adding from Upcoming

- Pressing `a` opens the add modal targeting the user's Inbox (the Upcoming view has no "current project").
- Due date is **not** pre-filled — Upcoming spans many days, so the user picks a date explicitly. Contrast with Today, which defaults due to `today`.

## Out of Scope (v1)

- Horizontal date strip along the top of the pane (the week-view ribbon in the official clients). The list-with-day-headers layout is sufficient for keyboard-first use.
- Drag-and-drop reschedule or bulk "Reschedule overdue" action.
- Quick-add from a specific day header.
