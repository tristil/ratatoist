# Today View: Add Defaults

When the user opens the add-task modal from the Today view, the Due date field is pre-filled with `today` so pressing Tab without touching it creates a task due today. The user can still edit or clear the field.

## Behavior

- Pressing `a` while the Today view is active opens the add modal with `form.due_string = "today"`.
- The Due date field renders `today` (not `none`) on open.
- Editing or clearing the field works exactly as in the normal add modal — the default is only the initial value.
- Submitting without editing sends `item_add` with `due: { string: "today" }`.
- **Same rule applies when adding from inside the Today section of the All view.** If the All view is active and the currently-selected row is a Task (which by construction sits in the Today section — Agenda, PRs, and Jira Cards are the other sections), the add modal opens with `form.due_string = "today"` and the new task routes to Inbox, exactly as if the user were on the Today view itself. This keeps the "I'm looking at today's work, and I want to add another thing for today" flow one keystroke.
- Adding from any other sidebar entry or All-view section (Inbox, a project, an Agenda row, a PR row, a Jira row) keeps the existing empty default.

## Project assignment

- The new task's `project_id` is the user's Inbox, same as today's behavior for Today-view-triggered adds — the virtual view has no project of its own.

## Out of Scope

- Defaulting priority, labels, or other fields based on view context.
- Per-project "smart defaults" (e.g. #work → tomorrow).
