# Repo Workflow

## This repo is public

`ratatoist` is an open-source public repo on GitHub. Anything committed here is
world-readable forever via git history, even if later deleted. Before writing
to a tracked file, check for:

- **Machine-specific paths** (e.g. `/Users/<name>/…`, `/home/<name>/…`) — use
  relative paths, env vars, or instructions that resolve the path dynamically
  (`git rev-parse --show-toplevel`, `git worktree list`).
- **Personal identifiers** — real names, emails, usernames, hostnames beyond
  what's already intentionally public (the repo's author metadata).
- **Secrets** — API tokens, credentials, private URLs, internal tool names. The
  `.gitignore` already covers `.env*`, `config.toml`, `*.pem`, `*.key`, and
  `.claude/`; keep it that way.
- **Private context** — internal tickets, employer-specific workflows, notes
  that only make sense on this machine.

When in doubt, ask before committing. The PR template's "No secrets or tokens"
checkbox is load-bearing.

## Shipping a change

This is the standard flow for every change in this repo. Follow it unless the
user says otherwise.

1. **Work on a feature branch**, never directly on `develop`.
   - Claude worktrees land on `claude/<worktree-name>` automatically.
   - Human-named features use `NNN-short-description` (see `git log` for the numbering in flight).
2. **Commit** with a Conventional Commits prefix: `fix:`, `feat:`, `refactor:`, `docs:`, `chore:`, etc.
   Subject is imperative, under ~70 chars. Body explains *why*, not *what*.
3. **Push** the feature branch to `origin`.
4. **Merge into `develop` with `--no-ff`** from the main checkout (the primary
   worktree, not the `claude/<worktree-name>` one you're editing in). Resolve
   its path with `git worktree list` — it's the entry whose path does not
   contain `.claude/worktrees/`. Then:
   ```
   git -C <main-checkout> merge --no-ff <branch> -m "Merge branch '<branch>' into develop"
   ```
   The explicit merge commit is load-bearing — `git log --oneline` on `develop` should show a
   `Merge branch '…' into develop` entry for every feature.
5. **Push `develop`** to `origin`.
6. **Reinstall the binary** from the main checkout so the local `ratatoist` reflects the merge:
   ```
   cd <main-checkout> && cargo install --path crates/ratatoist-tui
   ```

Run all three — push, merge, reinstall — in sequence at the end of a change unless the user
says to stop earlier. Don't ask after each step.

## Before you push

- `cargo build` must succeed.
- `cargo test --package ratatoist-tui` (or `--workspace`) must pass.
- If you added or changed behavior reachable from a key binding or view, add a regression test
  in the relevant `mod tests` block — follow the existing `fn test_app()` / `press()` /
  `pending_cmd_types()` helpers.
- A pre-existing `clippy::items_after_test_module` warning in `crates/ratatoist-tui/src/ui/dates.rs`
  is known; don't treat it as a blocker for unrelated changes.

## Keep specifications in sync

`specifications/*.spec.md` is the authoritative description of each feature. Any behavior
change — new feature, new key binding, new filter, new background job, changed default —
must include a spec update in the same PR:

- **New feature** → new `<feature>.spec.md` modeled on the existing ones (~30–50 lines:
  Behavior, Display, Actions, Out of Scope).
- **Changed behavior** → edit the relevant existing spec. The top-level `ratatoist.spec.md`
  is the front page; the per-view specs (`all-view`, `today-view`, `upcoming-view`,
  `github-prs-view`, `jira-cards-view`, `agenda-view`, `add-task-modal`, `recurring-complete`)
  each cover one surface.

If unsure which spec to update, grep `specifications/` for keywords from the change.

## Edition & syntax

- Workspace is Rust **edition 2024**; let-chains (`if let Some(x) = … && cond`) are in use, prefer them over nested `if let`.
