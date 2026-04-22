# Today View

A virtual sidebar entry below Inbox showing all overdue and due-today tasks across all projects in one place.

## Behavior

- Appears in the sidebar immediately below Inbox with a count badge (overdue + due today).
- Navigating to it shows tasks from all projects, not just the selected one.
- Overdue tasks appear in a collapsible "▼ Overdue (N)" section at the top, sorted oldest first.
- Today's tasks appear below, sorted by project then creation order.
- Each task row shows its source project name.
- Task completion (`x`) and detail pane (`Enter`) work the same as in any project.
- Empty state: "All caught up for today" when no tasks qualify.
- Updates in real time as sync events arrive.

## Filters

- Only includes tasks where `responsible_uid` is `None` (personal) or matches the current user (assigned to me in shared projects).
- Excludes completed and deleted tasks.
- Excludes subtasks (parent tasks only).
- **Hides tasks labeled `evening` until 17:00 local time.** Evening tasks clutter daytime planning; they reappear on Today (and the All view) once the local hour hits 17. Exact lowercase match only (`Evening` is not filtered). Other views (Upcoming, project lists) always show them.
- **Hides tasks labeled `work` on Saturdays and Sundays.** Weekend time is for non-work tasks; work items shouldn't pile on the Today list when the user isn't working. They reappear Monday morning automatically when the local weekday ticks back to a weekday. Exact lowercase match only (`Work` is not filtered). Other views (Upcoming, project lists) always show them.
- **Hides tasks whose label parses as a time-of-day until that time arrives.** A label like `2pm`, `9am`, `9:30am`, or `11:30pm` (lowercase, no space, optional `:MM`) is read as a clock time and the task is hidden until the local clock reaches it. Examples: `2pm` → hidden until 14:00, `9:30am` → hidden until 09:30. `12am` is midnight (never hides), `12pm` is noon. If multiple time labels are present, the task stays hidden until the latest one arrives. Labels that don't fit the format (`afternoon`, `5pmm`, `pm`) are treated as ordinary labels and ignored by this filter. Other views (Upcoming, project lists) always show the task.

## Out of Scope (v1)

- Quick-add from the Today view.
- Bulk reschedule of overdue tasks.
