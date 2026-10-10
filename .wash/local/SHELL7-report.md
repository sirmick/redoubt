# SHELL7 report

Branch `wp-SHELL7`, worktree `.worktrees/SHELL7`, base origin/shell 36d1450f9, two commits, for
merge into `shell`. Not pushed. The first version (one commit, f4df8d61e) is replaced, as the
orchestrator asked, by the registry fix first:

1. `d8368f3a6` shell: a command's module is loaded when the command is first called, not at the
   shell's start
2. `271374aec` shell: resource use: free, uptime, ps and top over the session's own budget and its VM

## 1. The registry

- `userland/shell/mix.exs`: a `commandlets` compiler, run after `:elixir` and before `:app`. It
  reads each `.beam`'s exports with `beam_lib:chunks` (nothing is loaded to find them), asks the
  modules that export `__commandlets__/0`, and writes `Redoubt.Commandlet.Index` into ebin. The
  index has `__modules__/0`, `__imports__/0`, `__commands__/0` (name to module) and a delegating
  function per command and arity. The `.app` lists the index, since Mix's app compiler scans ebin
  and the directory's mtime changed. A rebuild that changes nothing is a no-op: the stripped bytes
  are compared, because Elixir's ExCk chunk differs between compiles.
- `lib/redoubt/commandlet/registry.ex`: the prompt imports the index alone; `fetch/1` loads only
  the named module; `all/0` (used by help) loads them all.
- `image/userland.toml`, with its copy `tests/data/boot-profile/userland-unverified.toml`: the
  boot pack drops the 15 entries the prompt no longer loads, adds the Index, and drops Evaluator
  and erl_eval. Those two load with the first command, and an untaken entry holds the pack (about
  400 pages) through that command. 94 entries become 78. I checked with a scratch, uncommitted
  boot-stats print of the untaken entries: none are left at the first prompt.
  `tests/boot-profile*.toml` now expect 78 entries.
- Tests: registry_test (imports from the index; the index's modules equal the app's declaring
  modules; a delegate calls through).

## 2. The commands

`Redoubt.Shell.Resources`: free, uptime, ps, top. top's screen is `Redoubt.Screen.Top`, loaded
only by top(). Display only, as ruled. resources_test has 5 tests. df and jobs are named as
unbuilt in shell.md#resource-use.

## Footprint (beamlet-footprint, prebuilt, jobs.mk)

| | rv64 | rv32 |
| --- | ---: | ---: |
| peak, origin/shell | 5,905 | 5,706 |
| peak after 1 (registry and pack) | 5,430 | 5,251 |
| peak after 2 (commands) | 5,432 | 5,253 |
| accounted at the prompt: base, then 1, then 2 | 4,318 → 3,864 → 3,866 | 4,243 → 3,799 → 3,801 |
| modules at the prompt | 119 → 90 → 90 | |

- Footprint fake-kernel check: 4,348 → 3,892 pages at the prompt, before the pack fix.
- Without the pack fix the peak only fell to 5,863, because the pack stayed held.
- Boot: the first prompt comes at 13.1 s on rv64 (was 14.7) and 14.7 s on rv32 (was 16.3);
  unverified, 10.9 and 12.4 s (were 12.2 and 13.7).
- Session size: 2 × 5,432 + 18 = 10,882, which rounds to 11,008. The rule would lower the
  session from 11,904 to 11,008, freeing 896 pages. The manifests are main's, so the pages record
  it as a residual. 5,430 or less would allow 10,880.
- The Term change was skipped (it saves 0 pages), and consolidation was dropped. Both are noted on
  beamlet.md.

## Gates on the head (271374aec)

- `./test-shell`: rc=0. beamlet 155 passed, 0 failures; BEAM 146 passed, 9 skipped.
- doccheck: rc=0.
- prebuilt, then `make set CASES="$(scripts/shell-cases 36d1450f9)"`: beamlet's set of 30 cases
  (image and tests changed), on both widths: 60 PASS, rc=0. It includes beamlet-footprint,
  boot-profile and boot-profile-unverified.
- Commit 1 alone: test-shell rc=0 (150 / 142 + 8 skipped), doccheck rc=0, and footprint and both
  boot-profile cases PASS on both widths.

## Paths outside userland/shell

- `image/userland.toml` and `tests/` changed: the boot pack list, and the cases' entry count and
  comments.
- The pages: beamlet.md (footprint table, budget paragraph, the eager-loading bullet, the
  boot-timing row and sentence, the consolidation residual), budgets.md, testbench.md, shell.md
  (Commands, Resource use) and the m2 page's Progress.
- No userland/otp or drawing-path code changed. The scratch beamlet diagnostic was reverted.

## Summaries checked

- README.md and GETTING-STARTED.md: no mention, no change.
- shell.md: intro (`top()`) is true now.
- m2-usable-shell.md: Progress updated; the attack-suite goal is unchanged.
- beamlet.md, budgets.md and testbench.md: updated.

## Open

- Lowering the session to 11,008: a manifests change, on main.
- df: its own Tier A package.
- Jobs in ps/top: with the jobs package.
- The pack still lacks prompt modules it could hold (the driver, Term, group, edlin): boot time
  only, not in scope.
