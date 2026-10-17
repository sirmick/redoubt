# B80 fold — kernel-red-3 P2, P3 (text only)

Worktree /home/mcloonan/redoubt/.worktrees/B80, branch wp-B80, one commit amended:
4a0af33da → 338d014f6, on main 68c4b2904. `git status --short | wc -l` = 0. Files touched by
the fold: docs/userland/beamlet.md ("Several schedulers") and the commit message. The other
files of the commit (docs/testbench.md, scripts/jobs.mk, tests/beamlet-schedulers-one-hart.toml)
are unchanged.

## P2 — "within 100 µs" held for one side only

The gates (B80-report.md): one-scheduler side rv64 3,459,56x, rv32 3,790,70x across boots
(within 100 µs); two-scheduler side rv64 3,601,5xx / 3,603,83x (2.3 ms apart), rv32
3,996,2xx / 3,999,0xx (2.8 ms). 2.3 / 3,601 = 0.064 %, 2.8 / 3,996 = 0.070 %.

- beamlet.md now: "across boots the one-scheduler side's fastest runs lie within 100 µs of each
  other and the two-scheduler side's in two clusters 2.3 ms (rv64) and 2.8 ms (rv32) apart,
  within 0.1 % of a 3.6 or 4.0 s run".
- Commit, Gates paragraph: the same, in place of "(fastest runs within 100 µs across boots)".

## P3 — the E-core clock ramp was stated as the cause

- beamlet.md: the host clock's spread is stated first as the finding (a side's ten host times
  spread by a tenth, the fastest of each side met by chance); then "Sampled beside those runs,
  the vCPU thread moved among the four cores of `q`'s quiet set 13 to 28 times a run, and those
  cores' clocks read anywhere from 0.8 to 4.7 GHz; pinned to one host core the case read 0.93 on
  rv32 and 0.96 on rv64, and a side's ten times still spread by 3 to 9 %". The causal "each
  clocked ... as it woke, so ..." is gone. The page's "up to 26 times" became "13 to 28", the
  report's figure. The pinned spreads from the report: rv32 one 287–313 ms (9.1 %), two 306–327
  (6.9 %); rv64 one 405–418 (3.2 %), two 421–459 (9.0 %).
- Commit, bullet 2: the clause "each wake lands on a core at whatever speed it had" is removed;
  the sampling is named (every ~150 ms from /proc) and marked "a sample beside the spread, not
  its proof", followed by the pinned control (both widths, both sides' ranges, "a side's ten
  times still spread 3 to 9 %"), which absorbs the old bullet 3. "the host-clock spread and why"
  → "and what was sampled beside it".
- The duplicated Co-Authored-By trailer (two copies, one indented) is now one.

## Summaries checked

- docs/testbench.md hunks of the commit: "read 81 to 95 for a ratio that is 94 in guest time"
  states the spread, no 100 µs claim, no cause: no change.
- README.md, GETTING-STARTED.md, docs/plan/*.md, tests/beamlet-schedulers-mttcg.toml,
  scripts/jobs.mk: name none of the claims (B80-report.md's check stands): no change.

## Gates

`make -f scripts/jobs.mk set CASES=docs`: PASS docs 3.8 s, rv64/docs rc=0. No toml moved, so
formatting was not rerun.
