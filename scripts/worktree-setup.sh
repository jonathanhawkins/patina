#!/usr/bin/env bash
# Prepare a git worktree (e.g. a parallel subagent's .claude/worktrees/agent-*)
# so it can run the same tests as the main checkout.
#
#   scripts/worktree-setup.sh [--base <branch>] [<worktree-dir>]
#
# A fresh worktree is missing inputs that some tests read, so 2-3 tests go red
# for reasons unrelated to the agent's change (pat-hukbz):
#   - upstream/godot is a submodule, and it is EMPTY in a new worktree. The
#     gdobject light_doc_oracle tests read upstream/godot/doc/classes/*.xml.
#   - engine-rs/fixtures/editor_oracle/godot_extension_api.json is gitignored.
#     gdscene's classdb_instantiability_matches_godot reads it.
# This script symlinks both from the main checkout; it never copies them, so it
# writes nothing tracked. With --base, it also moves a CLEAN worktree's branch
# onto <base>, because agent worktrees have been created on a stale commit.
set -euo pipefail

base=""
if [[ "${1:-}" == "--base" ]]; then
  base="${2:?--base needs a branch}"
  shift 2
fi
wt="$(cd "${1:-.}" && pwd)"

# The main checkout is the first entry of `git worktree list`.
main="$(git -C "$wt" worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
if [[ "$main" == "$wt" ]]; then
  echo "worktree-setup: $wt is the main checkout; nothing to do"
  exit 0
fi

if [[ -n "$base" ]]; then
  if [[ -n "$(git -C "$wt" status --porcelain --untracked-files=no)" ]]; then
    echo "worktree-setup: $wt has uncommitted changes; not moving its branch" >&2
    exit 1
  fi
  branch="$(git -C "$wt" rev-parse --abbrev-ref HEAD)"
  git -C "$wt" checkout -q -B "$branch" "$base"
  echo "worktree-setup: $branch now at $(git -C "$wt" log --oneline -1)"
fi

link() { # link <relative path> - symlink main's copy if the worktree lacks one
  local rel="$1" src="$main/$1" dst="$wt/$1"
  if [[ ! -e "$src" ]]; then
    echo "worktree-setup: main checkout has no $rel; skipped"
    return
  fi
  if [[ -d "$dst" && ! -L "$dst" && -n "$(ls -A "$dst" 2>/dev/null)" ]]; then
    echo "worktree-setup: $rel already present"
    return
  fi
  if [[ -f "$dst" || -L "$dst" ]]; then
    echo "worktree-setup: $rel already present"
    return
  fi
  mkdir -p "$(dirname "$dst")"
  [[ -d "$dst" && ! -L "$dst" ]] && rmdir "$dst" # the empty submodule dir
  ln -sfn "$src" "$dst"
  echo "worktree-setup: linked $rel"
}

# A submodule path itself can't be a symlink ("expected submodule path
# 'upstream/godot' not to be a symbolic link"), so keep its empty directory
# and link each entry inside it. Git doesn't look inside an uninitialised
# submodule, so `git status` stays clean.
link_children() { # link_children <relative dir>
  local rel="$1" src="$main/$1" dst="$wt/$1"
  if [[ ! -d "$src" || -z "$(ls -A "$src" 2>/dev/null)" ]]; then
    echo "worktree-setup: main checkout has no $rel contents; skipped"
    return
  fi
  if [[ -n "$(ls -A "$dst" 2>/dev/null)" ]]; then
    echo "worktree-setup: $rel already populated"
    return
  fi
  mkdir -p "$dst"
  local e
  for e in "$src"/* ; do
    ln -s "$e" "$dst/$(basename "$e")"
  done
  echo "worktree-setup: linked the contents of $rel"
}

link_children upstream/godot
link engine-rs/fixtures/editor_oracle/godot_extension_api.json
