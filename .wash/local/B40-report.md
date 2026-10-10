# B40 report

Branch wp-B40, head aaac46839, two commits on origin/main 415c86ad6. Worktree .worktrees/B40.

## Cause

The fault is in the test, not the code. The escaping path is untouched, so this is Tier B.

- The shell's prompt shows the working directory (`Redoubt.Shell.prompt/2`: `"#{dir} (#{n})> "`).
  On the BEAM, `mix test` runs from the checkout's `userland/shell`, so the prompt is that absolute
  path. On beamlet the VM's / is the test root, so the prompt is `/ (N)> `.
- driver_test draws on a 120-column terminal model and asserts by row index. The hostile-text
  line typed is 56 bytes, so with a path of 59 bytes or more the prompt plus the line passes 120
  columns. edlin wraps the tail onto row 1, which reads `F")` (the end of `\x7F")`).
  - `.worktrees/SHELL6-files/userland/shell` is 61 bytes. That fails, 4/4.
  - `.worktrees/SHELL8/userland/shell` is 55 bytes, 117 columns, so it passes.
  - The root checkout (37 bytes) and `.worktrees/B40` (52 bytes) pass too.
- Reproduced in a scratch worktree `.worktrees/B40-wrapcheck` (62 bytes): the BEAM failed with
  row 1 = `7F")`. The crash-report test types longer lines and can wrap the same way, depending on
  the path.

## Why SHELL8's gate passed

It was neither flaky nor order-dependent, and the BEAM stage was not skipped. SHELL8 ran in
`.worktrees/SHELL8`, whose path is short enough that nothing wraps. The failure follows the
checkout's path length.

## Fix

- `803fdd099` driver_test: a `setup` runs each test with the VM's working directory at /, as on
  beamlet, and restores it with `on_exit` (the module is async: false). The prompt is then
  `/ (N)> ` on both VMs. screen_test also drives the shell, but it is skipped on the BEAM (no
  natives). shell_test uses StringIO (no wrapping).
- `aaac46839` scripts/shell-cases: it reads the git checkout it is run in when that is a Redoubt
  tree (has scripts/shell-cases), else the script's own. Its own tree is resolved before any cd,
  so a relative invocation works. `--check` makes two scratch git trees under REDOUBT_TMP: one
  with the script, which must be read, and one without, which must fall back to the script's own.
  The usage header says which tree is read.

## Gates

- `./test-shell` whole, 3 runs in .worktrees/B40 (`q run --cores 8`): all rc=0, "every stage
  passed", beamlet 155/155 each.
- The long path, the real repro: driver_test on the BEAM in .worktrees/B40-wrapcheck with the fix,
  4 runs, exit 0. Without the fix, 1 run failed as reported. The scratch worktree is removed.
- `scripts/shell-cases --check`: pass. It was run as the worktree's copy from the root checkout
  and from a worktree subdirectory. A real run from .worktrees/B40/userland/shell read B40's own
  change.
- docs: PASS. formatting (bench case): PASS. mix format is checked inside ./test-shell.
- Machine shell-cases set: not run. No shell or beamlet code changed (a test and a script only).

## Docs

GETTING-STARTED.md names scripts/shell-cases; it is still true, and no change is needed. No page
states the BEAM tests' working directory beyond test-shell's header, which is unchanged and still
true.
