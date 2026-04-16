# Repo Workflow

## Shipping a change

This is the standard flow for every change in this repo. Follow it unless the
user says otherwise.

1. **Work on a feature branch**, never directly on `develop`.
   - Claude worktrees land on `claude/<worktree-name>` automatically.
   - Human-named features use `NNN-short-description` (see `git log` for the numbering in flight).
2. **Commit** with a Conventional Commits prefix: `fix:`, `feat:`, `refactor:`, `docs:`, `chore:`, etc.
   Subject is imperative, under ~70 chars. Body explains *why*, not *what*.
3. **Push** the feature branch to `origin`.
4. **Merge into `develop` with `--no-ff`** from the main checkout at `/Users/method/Projects/ratatoist`:
   ```
   git -C /Users/method/Projects/ratatoist merge --no-ff <branch> -m "Merge branch '<branch>' into develop"
   ```
   The explicit merge commit is load-bearing — `git log --oneline` on `develop` should show a
   `Merge branch '…' into develop` entry for every feature.
5. **Push `develop`** to `origin`.
6. **Reinstall the binary** from the main checkout so the local `ratatoist` reflects the merge:
   ```
   cd /Users/method/Projects/ratatoist && cargo install --path crates/ratatoist-tui
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

## Edition & syntax

- Workspace is Rust **edition 2024**; let-chains (`if let Some(x) = … && cond`) are in use, prefer them over nested `if let`.
