# B54 report (b57-implementer-3, 2026-10-15)

Node B54: redoubt-doccheck's C11 fixture tests fail under a plain `cargo test` ("C11 did not
fire") while the bench's docs case passes. Worktree `/home/mcloonan/redoubt/.worktrees/B54`,
branch `wp-B54` from main 9d680b781, one commit (3 files). Tier B, no machine time.

## Reproduction (recorded before the fix)

Every ordinary invocation passed here: `cargo test -p redoubt-doccheck` from the worktree root
(through q and plain), from `/home/mcloonan` with `--manifest-path`, with `TMPDIR` and
`REDOUBT_TMP` unset, through q from another directory, and `make -f scripts/jobs.mk docs`
(PASS). So the dependence was not the working directory, the shell's exports, q, or the tmp
variables as such. Logs: `$REDOUBT_TMP/B54/{plain-root,plain-noq-root,plain-home}.log`.

Bisected on the built `rules` test binary and on the checker itself:
- `PATH` without git: only `untracked_code_is_not_checked` fails (git fails, so every file
  counts and the stray file is checked); C11 fires.
- `GIT_DIR` pointing at a non-repository: the same one failure; C11 fires.
- `env -i`, `HOME` unset, `GIT_CEILING_DIRECTORIES` at the root: all pass.
- The checker (`doccheck --code`, root = cwd) on the c11 fixture **in place**: 7 C11 findings;
  on a copy under **`/tmp`** (outside any repository): the same 7; on a copy under the
  project's **`.tmp/`** (`$REDOUBT_TMP/B54/c11`, inside the checkout, ignored): **0 C11
  findings**, exit 1 only for C7/C12. `git -C <copy> ls-files -z` there succeeds and prints
  nothing. (`$REDOUBT_TMP/B54/cli-copies.out`.)

That is the symptom: `tracked()` (tools/doccheck/src/lib.rs) took a failed git as "root is not
in a repository, every file counts", but under an ignored directory of a repository git answers
and names nothing, so every file was dropped and C11 was silent — the three tests that need C11
to fire (`c11_code`, `narrow_cases_fire`, `untracked_code_is_not_checked`) fail together,
exactly the reviewer's report. The project's scratch rule puts copies under the checkout's own
`.tmp/`, so a reviewer who copied the crate or the fixtures there (or ran a test binary built
from such a copy) saw it, while the bench's docs case runs in the worktree and passes.

## Fix (tools/doccheck/src/lib.rs `tracked`)

- An empty answer from `git ls-files` is `None` too: a root that is no tracked part of a
  repository, outside one or under an ignored directory, is checked whole.
- Git is asked about the root itself: the child runs with `GIT_DIR`, `GIT_WORK_TREE`,
  `GIT_INDEX_FILE` and `GIT_COMMON_DIR` removed, so a caller's environment cannot redirect the
  question to another repository.
- What C11 checks is unchanged: in a tracked tree the untracked files are still excluded
  (`untracked_code_is_not_checked` passes as before, and the real tree's tracked set is never
  empty).

New test `a_copy_under_an_ignored_directory_is_checked_whole` (tests/rules.rs): copies the c11
fixture under `CARGO_TARGET_TMPDIR` (`target/tmp`, ignored, inside the repository), runs
`check`, expects C11 at `libs/wire/elixir/proto/ping.ex:2`, `libs/wire/elixir/wire.ex:3` and
`src/lib.rs:1`, and removes the copy.

docs/testbench.md "The docs checker": a "Running its tests" paragraph (any directory and shell,
through q, the bench's docs case; the fixtures by the crate's path; what C11 asks git and the
rule for a tree that is no tracked part of a repository; the symptom this fixes); the status
line no longer claims "no bench case runs it yet" (bench:docs does, as the next subsection
says) and lists `untracked_code_is_not_checked` and the new test (8). `c11_code` cannot be
listed: C2 looks for `fn c11_code` and the name is macro-generated (the book check said so on
a first try).

## Gates (after the fix)

- `cargo test -p redoubt-doccheck` through q from the worktree root: docs 2 ok, rules 21 ok.
- The same plain from `/home/mcloonan` with `--manifest-path`, `TMPDIR`/`REDOUBT_TMP` unset:
  2 ok, 21 ok. (`$REDOUBT_TMP/B54/fix2-{root,home}.log`.)
- The built rules binary with `GIT_DIR` pointing elsewhere: 21 ok.
- The checker on the `.tmp` copy: 7 C11 findings (was 0).
- `make -f scripts/jobs.mk docs`: PASS.
- `rustfmt +nightly --check` on the two Rust files: clean. No unsafe.

## Pages and summaries checked

- docs/testbench.md "The docs checker" and "The docs checker in the bench": updated / consistent.
- docs/README.md, README.md, GETTING-STARTED.md: no claim about how doccheck's tests run (grep
  `doccheck`); GETTING-STARTED names `cargo testbench` only. No change.
- tools/doccheck/src/main.rs's crate doc ("run from the repository root") describes the CLI's
  root = cwd; unchanged and true.

## Open risks

- A root that is a tracked part of a repository but whose tracked set is empty (a tracked
  directory with only untracked files under it) is now checked whole; before, it was silently
  skipped. For C11 that is the safer side.
