# Add Task Modal

The modal that opens when the user presses `a` from the task list to create a new task.

## Fields

- **Content** — free-text task title. Opens in edit mode on modal open.
- **Priority** — 1–4, opens the priority picker popup on edit.
- **Due date** — natural-language string (e.g. `tomorrow`, `next friday`, `every monday`, `3pm today`). Todoist parses it server-side.
- **Project** — cycles through available projects; defaults to the currently selected project.

## Navigation

- `j` / `k` (Vim) or arrow keys (Standard) move between fields when not editing.
- `Enter`, `i`, or space enters edit mode for the focused field.
- `Tab` submits the form; empty content cancels silently.
- `Esc` on the Content field cancels the whole modal; on any other field it returns focus to Content for edit.
- `q` (Vim normal) cancels the modal.

## Submission

- Form sends the Todoist Sync API `item_add` command with an optimistic task inserted into the local list immediately.
- Due date is sent as `due: { string: <user input> }`. The REST-style top-level `due_string` shorthand is **not** accepted by the Sync API and must not be used — sending it silently drops the date on the server side.
- Priority is only included when greater than 1.
- On server error the optimistic task is reverted.

## Out of Scope (v1)

- Labels, description, section, or parent selection from the modal.
- Recurring-date preview before submission.
