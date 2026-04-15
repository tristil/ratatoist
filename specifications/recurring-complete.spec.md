# Recurring Task Completion

Pressing `x` on a recurring task advances the series to its next occurrence; the task must remain on the active list with an updated due date, not disappear.

## Behavior

- Sync command sent: `item_close`. This mirrors what the official Todoist clients do: recurring tasks advance to the next occurrence; non-recurring tasks close normally. `item_complete` is *not* used — it marks only the current instance complete without advancing the series, so the task sticks with its old due date.
- Optimistic UI does **not** flip `checked` on recurring tasks. The task stays visible and unchecked until the server's response updates its due date.
- Undoing (pressing `x` on a task that is already checked) sends `item_reopen` and flips `checked` optimistically.
- Non-recurring tasks flip `checked` immediately for instant feedback and also send `item_close`.

## Rationale

Flipping `checked` optimistically on a recurring task hides it from the active filter, giving the user the false impression the entire recurring series was deleted. The server's response will arrive moments later with the advanced due date — no local advance is attempted because the recurrence grammar (natural-language strings like `every weekday`) is parsed server-side.
