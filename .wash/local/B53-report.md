# B53 report (b57-implementer-3, 2026-10-15)

Node B53: `.wash/save.sh` gains `--local-only` and ignores scratch worktrees under `.tmp/`.
Worktree `/home/mcloonan/redoubt/.worktrees/B53`, branch `wp-B53` from main 92b84a5b8, one
commit **f55203dbf**. Tooling, Tier B; the owner owns `save.sh`, so this is a proposal for the
owner's approval before it merges. Nothing was pushed by anything run here.

## Delivered (3 files, +52/-29)

- `.wash/save.sh`:
  - Arguments are parsed in any order: `--dry-run`, `--local-only`; anything else is refused
    with exit 2 and the two names.
  - Step 1 skips a worktree whose path is under `$ROOT/.tmp/` (scratch by the project's rule:
    measurements, negatives) and still refuses a dirty `.worktrees/*`.
  - `--local-only` runs steps 1, 4 and 5 only: the `wp-*` branches fast-forward and the
    `wash-local` snapshot. Steps 2 and 3 (the `plan:` commit, the push of `main`, and the
    check that the root is on `main`) are inside `if ((LOCAL)) ... else`; it says
    `save: local only: main is neither committed to nor pushed`. Step 5's commit message still
    names `main`'s short hash, as before.
  - The header comment and the usage lines say all of it. Nothing else in the script changed.
- `.wash/SWARM.md` "Saving and resuming", step 3: the `.tmp/` rule and `--local-only` (when
  `main` holds a train whose bench has not run, or the plan commit is not wanted yet), and that
  `--dry-run` composes with it.
- `.wash/PROJECT.md`, the save bullet: save with `--local-only` when `main` may not be
  published yet; scratch worktrees under `.tmp/` never block a save.

## Tests (dry-run only; `git fetch` is the script's only remote access, a read)

`bash -n` clean (no shellcheck on this host). The script's `ROOT` is `git rev-parse
--show-toplevel` of the caller's directory, so each run below was `cd <root> && bash <new
save.sh> ...`.

### In the project root (`/home/mcloonan/redoubt`)

A scratch worktree `$REDOUBT_TMP/B53/scratch` (detached, an untracked `UNTRACKED.txt`) was
added for each run and removed after; `git worktree prune` after.

1. **main's save.sh** (92b84a5b8, `--dry-run`), the same tree:
   ```
   save: uncommitted changes in /home/mcloonan/redoubt/.tmp/B53/scratch: commit them as a WIP commit on its branch first
   save: uncommitted changes in /home/mcloonan/redoubt/.worktrees/SCHED4: commit them as a WIP commit on its branch first
   exit=1
   ```
2. **the new save.sh**, `--local-only --dry-run`, the same tree:
   ```
   save: uncommitted changes in /home/mcloonan/redoubt/.worktrees/SCHED4: commit them as a WIP commit on its branch first
   exit=1
   ```
   The scratch under `.tmp/` is no longer looked at; `.worktrees/SCHED4` is another member's
   live worktree (2 uncommitted lines throughout, not mine to commit), so it refuses, which is
   the rule. Three tries over about ten minutes; SCHED4 stayed dirty.
3. **the new save.sh with `.worktrees/B53` dirtied too** (an untracked file, removed after),
   `--local-only --dry-run`:
   ```
   save: uncommitted changes in /home/mcloonan/redoubt/.worktrees/B53: commit them as a WIP commit on its branch first
   save: uncommitted changes in /home/mcloonan/redoubt/.worktrees/SCHED4: commit them as a WIP commit on its branch first
   exit=1
   ```
4. `--bogus`: `save: unknown argument --bogus (--dry-run, --local-only)`, exit 2.

### In a throwaway shared clone (`$REDOUBT_TMP/B53/clone`, removed after)

`git clone --shared --branch main`, the new `save.sh` copied in, a clean package worktree
`.worktrees/X` on `wp-X`, a scratch worktree `.tmp/scratch` with an untracked file, one file in
`.wash/local/`. Its `origin` is the local repository; dry-run pushes nothing.

1. `--local-only --dry-run` (scratch dirty, `.worktrees/X` clean): **passes**
   ```
   save: local only: main is neither committed to nor pushed
   would: git push -q -u origin wp-X
   save: would commit and push wash-local
   exit=0
   ```
2. `--dry-run` alone, the same tree: `main`'s step appears, so the difference is exactly step 3
   (step 2 had nothing to commit there)
   ```
   would: git push -q origin main
   would: git push -q -u origin wp-X
   save: would commit and push wash-local
   exit=0
   ```
3. `.worktrees/X` dirtied, `--local-only --dry-run`: **refuses**
   ```
   save: uncommitted changes in /home/mcloonan/redoubt/.tmp/B53/clone/.worktrees/X: commit them as a WIP commit on its branch first
   exit=1
   ```

Raw outputs: `$REDOUBT_TMP/B53/{old,A,A2,A3,B,clone-1,clone-2,clone-3}.out`.

## Pages and summaries checked

- `.wash/SWARM.md` "Saving and resuming" and "Publishing" (line 422 says the publishing check
  applies when `save.sh` publishes `main`: still true; `--local-only` publishes no `main`, so
  nothing to add there).
- `.wash/PROJECT.md`: the save bullet updated; the "Apply SWARM's publishing check ... before
  every push of `main`, including a save" sentence stays true.
- `.wash/README.md` line 14 (`save.sh`, `restore.sh`: "push the whole state of the work"): a
  one-line table entry; `--local-only` is a mode of the same script, left as is.
- `docs/`: no page names `save.sh` (grep); the docs checker does not read `.wash/`.

## Open risks

- `--local-only` still runs `git fetch -q origin` first and pushes `wp-*` and `wash-local`; it
  is not an offline mode. A diverged `wp-*` is skipped with the same message as before.
- The `.tmp/` rule keys on the path under the project root's `.tmp/`; a scratch worktree made
  elsewhere (`/tmp`, which the project forbids anyway) is still checked.

## Round 2 (steward-red-2 OK with notes on f55203dbf): folded into the one commit, head 2abeb0728

- P2-1: `ROOT` was `git rev-parse --show-toplevel`, the caller's worktree when run from one, and
  `--local-only` skips the on-`main` check that used to catch it, so the worktree's few
  `.wash/local` files could become `wash-local`'s tip. Now
  `ROOT=$(cd "$(git rev-parse --git-common-dir)/.." && pwd -P)`: the parent of the common `.git`
  directory, which every package worktree shares, so the project root from anywhere (the root,
  a worktree, a worktree's copy of the script), with the comment saying why.
- P3-1: the usage error goes to stderr (`--bogus`: stdout empty, stderr
  `save: unknown argument --bogus (--dry-run, --local-only)`, exit 2).
- `bash -n` clean. Dry-runs again (nothing pushed):
  - From inside `.worktrees/B53`, before the amend (B53 dirty with the edit):
    `uncommitted changes in /home/mcloonan/redoubt/.worktrees/B53` and `.../SCHED4`, exit 1:
    the paths are under the project root, so the root is the project's, not the caller's.
  - From inside `.worktrees/B53`, after the amend (clean): only `.../SCHED4` (still another
    member's dirty worktree), exit 1.
  - Throwaway shared clone again (`$REDOUBT_TMP/B53/clone`, removed after), a file in the
    root's `.wash/local` and one in `.worktrees/X/.wash/local`, the scratch under `.tmp` dirty:
    from inside `clone/.worktrees/X`:
    ```
    save: local only: main is neither committed to nor pushed
    would: git push -q -u origin wp-X
    would: cp -u .worktrees/X/.wash/local/X-report.md .wash/local/
    save: would commit and push wash-local
    exit=0
    ```
    the same four lines from the clone's root (so the worktree's file is copied into the root's
    `.wash/local`, and the root's is what would be snapshotted); `.worktrees/X` dirtied, from
    inside it: refused naming `.../clone/.worktrees/X`, exit 1. Raw outputs
    `$REDOUBT_TMP/B53/{wt-1,wt-2,err,clone-4,clone-5,clone-6}.out`.
