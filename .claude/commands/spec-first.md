---
description: Spec-first change workflow — update specifications/ before touching code
argument-hint: <short feature or change description>
---

The user wants to make this change: $ARGUMENTS

Follow this order strictly. Do not skip steps.

## 1. Identify the spec

Read `specifications/` and pick the file(s) this change belongs to.

- If an existing per-view spec fits (`all-view`, `today-view`, `upcoming-view`, `github-prs-view`, `jira-cards-view`, `agenda-view`, `add-task-modal`, `recurring-complete`, etc.), use it.
- If none fit, propose a new `<feature>.spec.md` modeled on the existing ones — ~30–50 lines with sections: **Behavior**, **Display**, **Actions**, **Out of Scope**.
- For cross-cutting changes, update the relevant per-view spec AND note the change in `ratatoist.spec.md` if it affects the front-page description.

## 2. Draft the spec change first

Show the proposed spec diff to the user before any code edits. Stop and wait for approval. If the user pushes back, revise the spec — do not start coding around an unapproved spec.

## 3. Implement to match

Once the spec is approved, write the code. The code must match the spec, not the other way around. If during implementation you discover the spec is wrong or incomplete, **stop and revise the spec** before continuing — do not let the code drift ahead of the spec.

Add a regression test in the relevant `mod tests` block if the change touches a key binding or view (see existing `fn test_app()` / `press()` / `pending_cmd_types()` helpers).

## 4. Verify sync before finishing

Re-read the final spec and diff it against the code. Any behavior in the code that isn't described in the spec is a bug in one or the other — flag it to the user.

## 5. Commit both together

The spec edit and code edit go in the same commit (or at minimum the same PR). Never ship code without its spec update.

## Escape hatch

If this is a pure bug fix with **no** behavior, display, or action change — i.e. the current spec is already correct and the code is just being made to match it — say so explicitly in step 1 and skip to step 3. Err on the side of updating the spec when in doubt.

The repo has a PreToolUse hook (`scripts/spec-first-check.sh`) that blocks edits under `crates/**/src/**` when no spec changes exist on this branch. To bypass it for a pure bug fix, create the override file before editing:

```
touch .claude/spec-override
```

Remove `.claude/spec-override` after the change is committed.
