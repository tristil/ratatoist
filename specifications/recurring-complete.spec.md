# Recurring Task Completion

Pressing `x` on a recurring task advances the series to its next occurrence; the task must remain on the active list with an updated due date, not disappear.

## Behavior

- Sync command sent: `item_close`. This mirrors what the official Todoist clients do: recurring tasks advance to the next occurrence; non-recurring tasks close normally. `item_complete` is *not* used — it marks only the current instance complete without advancing the series, so the task sticks with its old due date.
- Optimistic UI does **not** flip `checked` on recurring tasks. Instead the task ID is added to a `pending_close_recurring` set that the Today and Upcoming views filter against, so the task disappears from those views instantly. When the sync response arrives (success or failure), the ID is removed from the set and the task re-emerges with the server-advanced due date (or, on failure, its original state via `revert_optimistic`).
- The optimistic hide prevents a double-tap of `x` from advancing the series twice: after the first press, the task is no longer in the visible list, so the second press targets the next task instead.
- Undoing (pressing `x` on a task that is already checked) sends `item_reopen` and flips `checked` optimistically.
- Non-recurring tasks flip `checked` immediately for instant feedback and also send `item_close`.

## Rationale

Flipping `checked` optimistically on a recurring task hides it from the active filter, giving the user the false impression the entire recurring series was deleted. The server's response will arrive moments later with the advanced due date — no local advance is attempted because the recurrence grammar (natural-language strings like `every weekday`) is parsed server-side.
