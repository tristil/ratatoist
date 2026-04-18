#!/usr/bin/env bash
# PreToolUse hook: enforce spec-first workflow.
#
# Blocks Edit/Write/MultiEdit on crates/**/src/** when no changes under
# specifications/ exist in the working tree or on the current branch vs.
# develop, unless .claude/spec-override exists.
#
# Reads PreToolUse JSON payload from stdin. See
# https://docs.claude.com/en/docs/claude-code/hooks for the contract.

set -euo pipefail

input="$(cat)"
file_path="$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')"

if [[ -z "$file_path" ]]; then
  exit 0
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || echo "")"
if [[ -z "$repo_root" ]]; then
  exit 0
fi

rel_path="$file_path"
if [[ "$file_path" == "$repo_root/"* ]]; then
  rel_path="${file_path#"$repo_root"/}"
fi

# Only gate edits to Rust source under crates/.
# Cargo.toml, README.md, themes/ etc. don't need spec updates.
if [[ "$rel_path" != crates/*/src/* ]]; then
  exit 0
fi

# Override file — pure bug fix, no behavior change.
if [[ -f "$repo_root/.claude/spec-override" ]]; then
  exit 0
fi

# Spec changes in working tree?
if [[ -n "$(git -C "$repo_root" status --porcelain -- specifications/ 2>/dev/null)" ]]; then
  exit 0
fi

# Spec changes on this branch vs. develop?
if git -C "$repo_root" rev-parse --verify --quiet develop >/dev/null; then
  if [[ -n "$(git -C "$repo_root" diff --name-only develop...HEAD -- specifications/ 2>/dev/null)" ]]; then
    exit 0
  fi
fi

cat >&2 <<EOF
Spec-first check failed: editing $rel_path but no changes under
specifications/ exist on this branch or in the working tree.

Update the relevant specifications/*.spec.md first (see /spec-first and
CLAUDE.md "Keep specifications in sync"), then retry.

Escape hatch — for pure bug fixes with no behavior/display/action change:
  touch .claude/spec-override
Remove .claude/spec-override after the change is committed.
EOF
exit 2
