# Recurring Task Completion

Pressing `x` on a recurring task advances the series to its next occurrence; the task must remain on the active list with an updated due date, not disappear.

## Behavior

- Sync command sent: `item_close`. This mirrors what the official Todoist clients do — for recurring tasks it advances to the next occurrence, for non-recurring tasks it closes normally. `item_complete` is *not* used because it marks the entire series as done.
- Optimistic UI does **not** flip `checked` on recurring tasks. The task stays visible and unchecked until the server's response updates its due date.
- Undoing (pressing `x` on a task that is already checked) always sends `item_reopen` regardless of recurrence and flips `checked` optimistically.
- Non-recurring tasks retain the old behavior: `checked` flips immediately for instant feedback and the command is `item_close`.

## Rationale

Flipping `checked` optimistically on a recurring task hides it from the active filter, giving the user the false impression the entire recurring series was deleted. The server's response will arrive moments later with the advanced due date — no local advance is attempted because the recurrence grammar (natural-language strings like `every weekday`) is parsed server-side.
