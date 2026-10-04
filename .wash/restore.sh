#!/usr/bin/env bash
# restore.sh -- bring a fresh clone (or a stale one) up to the saved state of the work.
#
#   1. Fetches origin and fast-forwards main (refuses if main has local commits origin lacks).
#   2. Restores .wash/local/ from the wash-local branch, never overwriting a newer local file.
#   3. Makes a worktree at .worktrees/<package> for every origin/wp-<package> branch that has
#      none, tracking it.
# It never deletes, resets or force-checks-out anything.
#   .wash/restore.sh
set -euo pipefail
ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
say() { echo "restore: $*"; }

git fetch -q origin
[[ $(git symbolic-ref --short HEAD) == main ]] || { say "the project root is not on main"; exit 1; }
if git merge-base --is-ancestor main origin/main; then
    git merge -q --ff-only origin/main
    say "main at $(git rev-parse --short main)"
else
    say "main has commits origin/main lacks: push or reconcile them first"; exit 1
fi

if git rev-parse -q --verify refs/remotes/origin/wash-local >/dev/null; then
    mkdir -p .wash/local
    tmp=$(mktemp -d)
    git archive origin/wash-local .wash/local | tar -x -C "$tmp"
    cp -r -u "$tmp/.wash/local/." .wash/local/
    rm -rf "$tmp"
    say ".wash/local restored from wash-local ($(git rev-parse --short origin/wash-local))"
else
    say "no wash-local branch on origin"
fi

for ref in $(git for-each-ref --format='%(refname:short)' refs/remotes/origin/wp-*); do
    b=${ref#origin/}
    pkg=${b#wp-}
    pkg=$(echo "$pkg" | tr '[:upper:]' '[:lower:]')
    wt=.worktrees/$pkg
    if git worktree list --porcelain | grep -qx "branch refs/heads/$b"; then
        continue
    fi
    if git merge-base --is-ancestor "$ref" main; then
        say "$b is already merged into main; no worktree (delete it on origin if done)"
        continue
    fi
    if git rev-parse -q --verify "refs/heads/$b" >/dev/null; then
        git worktree add -q "$wt" "$b"
    else
        git worktree add -q --track -b "$b" "$wt" "$ref"
    fi
    say "worktree $wt on $b"
done
