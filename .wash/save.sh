#!/usr/bin/env bash
# save.sh -- push the project's whole state so work can continue from another clone.
#
# What it does, in order, and what it refuses:
#   1. Refuses if any package worktree (.worktrees/*) has uncommitted changes: commit them as a
#      WIP commit on that package's branch first (never stash; SWARM.md, staging).
#   2. Commits .wash/plan.toml and .wash/qa/ on main if they changed, as one `plan:` commit,
#      when main is checked out in the project root and nothing else is staged there.
#   3. Pushes main, fast-forward only (refuses if origin/main is not an ancestor).
#   4. Pushes every local wp-* branch to origin, fast-forward only.
#   5. Copies each worktree's .wash/local/*.md into the root's .wash/local/ (newer wins), then
#      snapshots .wash/local's working files (under 1,000 KiB, no *.console, *.log or *-logs/)
#      onto the wash-local branch through a temporary index, and pushes it. main's index and
#      working tree are never touched by this step.
#
# It never force-pushes, never rewrites history and never deletes anything.
#   .wash/save.sh            save
#   .wash/save.sh --dry-run  say what it would do
set -euo pipefail

DRY=0
[[ ${1:-} == --dry-run ]] && DRY=1
ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

run() { if ((DRY)); then echo "would: $*"; else "$@"; fi; }
say() { echo "save: $*"; }

git fetch -q origin

# 1. Package worktrees must be committed.
dirty=0
while read -r wt; do
    [[ $wt == "$ROOT" ]] && continue
    if [[ -n $(git -C "$wt" status --porcelain) ]]; then
        say "uncommitted changes in $wt: commit them as a WIP commit on its branch first"
        dirty=1
    fi
done < <(git worktree list --porcelain | sed -n 's/^worktree //p')
((dirty)) && exit 1

# 2. The plan and the QA threads on main.
[[ $(git symbolic-ref --short HEAD) == main ]] || { say "the project root is not on main"; exit 1; }
if [[ -n $(git diff --cached --name-only) ]]; then
    say "something is already staged in the project root; commit or unstage it first"; exit 1
fi
changed=$(git status --porcelain -- .wash/plan.toml .wash/qa)
if [[ -n $changed ]]; then
    say "committing the plan and QA threads:"; echo "$changed"
    run git add -- .wash/plan.toml .wash/qa
    run git commit -s -q -m "plan: the plan and its threads as the work is saved"
fi

# 3. main, fast-forward only.
if git merge-base --is-ancestor origin/main main; then
    run git push -q origin main
    ((DRY)) || say "main pushed ($(git rev-parse --short main))"
else
    say "origin/main has commits main lacks: fetch and merge them first; nothing pushed"; exit 1
fi

# 4. Every package branch, fast-forward only.
for b in $(git for-each-ref --format='%(refname:short)' refs/heads/wp-*); do
    if git rev-parse -q --verify "refs/remotes/origin/$b" >/dev/null &&
       ! git merge-base --is-ancestor "origin/$b" "$b"; then
        say "$b has diverged from origin/$b (a rebase?): push it by hand with --force-with-lease after checking; skipped"
        continue
    fi
    run git push -q -u origin "$b"
    ((DRY)) || say "$b pushed ($(git rev-parse --short "$b"))"
done

# 5. .wash/local onto wash-local.
for wt in .worktrees/*/; do
    [[ -d $wt.wash/local ]] || continue
    for f in "$wt".wash/local/*.md; do
        [[ -e $f ]] && run cp -u "$f" .wash/local/
    done
done
list=$(mktemp)
(cd .wash/local && find . -type f -size -1000k ! -name '*.console' ! -name '*.log' \
    ! -path './*-logs/*' | sed 's#^\./##' | sort) > "$list"
if [[ ! -s $list ]]; then say ".wash/local is empty; wash-local not updated"; rm -f "$list"; exit 0; fi
if (cd .wash/local && tr '\n' '\0' < "$list" | xargs -0 grep -l -I -E \
    'BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY|ghp_[A-Za-z0-9]{20}|sk-ant-[a-z0-9]' 2>/dev/null); then
    say "a file above looks like it holds a key or token; wash-local not updated"; rm -f "$list"; exit 1
fi
idx=$(mktemp -u)
GIT_INDEX_FILE=$idx bash -c 'sed "s#^#.wash/local/#" "$1" | tr "\n" "\0" | xargs -0 git add -f --' _ "$list"
tree=$(GIT_INDEX_FILE=$idx git write-tree)
rm -f "$idx" "$list"
parent=$(git rev-parse -q --verify refs/remotes/origin/wash-local || true)
if [[ -n $parent && $(git rev-parse "$parent^{tree}") == "$tree" ]]; then
    say "wash-local unchanged"; exit 0
fi
if ((DRY)); then say "would commit and push wash-local"; exit 0; fi
commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} \
    -m "wash: the workspace's local state at $(git rev-parse --short main)" \
    -m "Briefs, reports, rulings and handoffs from .wash/local, kept on this branch only.")
git update-ref refs/heads/wash-local "$commit"
git push -q -u origin wash-local
say "wash-local pushed ($(git rev-parse --short wash-local))"
