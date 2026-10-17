# B88: at two harts on rv32, erofsd:system takes nobody's call under two spinner VMs

Node B88 (parent AGENTS). Tier A (the stuck party is a shared server, the kernel's scheduler
or the steward). Size M. Reviewer: kernel-red-3 (steward-red-4 if the cause is the steward's).
Base: origin/main at d1c199e20 or later (STEWARD3U's bounded console writes and STEWARD3W's
parked cleanup steps are both in; the symptom survived both). Related and open beside you:
B87 (the two-hart oracle rejections, wp-B87, kernel-red-3) and STEWARD3V (servers' budgets
from the manifest). Coordinate through the orchestrator before touching anything either owns.

## Context rules (read these first)

Work in your own worktree: `git worktree add .worktrees/B88 -b wp-B88 origin/main` from the
repo root, after `git fetch`. Every build and boot goes through `scripts/q run` or
`make -f scripts/jobs.mk` (see docs/testbench.md); never a bare cargo or QEMU. No edits in the
worktree while prebuilt builds or cases run (they fail "tree changed"). Scratch under
`$REDOUBT_TMP` (the repo's `.tmp/B88`), never /tmp. Messages at most 2000 bytes. Do not read
whole console logs or QA base64 tails into context: grep them, read ranges. Pages carry no
dates or package ids; a case or comment names no package id (the docs case C11 fires under
`make -f scripts/jobs.mk set CASES=docs` only, so run the gate that way). Never push; never
`git add -A`; never `git stash`. Report by `member_update` with the assignment result and a
report file `.wash/local/B88-report.md`, plus a message to the orchestrator with the head.

## The symptom

Seen by STEWARD3W's steward-latency N = 3 round on rv32 at two harts, twice, before and after
STEWARD3U (so not the console-hold wedge first suspected): the steward's cleanup disconnects
at `erofsd:system` time out (now parked after four), the shell's `start_agent` for the next
lease gets the client's 5 s timeout while the steward had taken no call, and nothing prints
for about 570 s until the spinners' deadline kills the run. rv64 at two harts and rv32 at one
hart complete the same round. Logs: `.tmp/STEWARD3W/latency-logs/stall2-*` and the
`rv32-steward-latency.smp2*.log` files beside them (console and session logs; grep, do not
cat). STEWARD3W's reading: erofsd:system (the read-only system volume server, which the two
spinner VMs read code from) takes nobody's call under two spinner VMs at two harts on rv32.

## Work

1. Reproduce: `TESTBENCH_QEMU_SEED=3`, `cargo testbench steward-latency --arch rv32
   --keep-failed <dir>` at two harts under q with LATENCY_ROUNDS 3 (tests/steward-latency.toml
   says how the hart count and rounds are set). Keep the run dir; bench run dirs are pruned.
2. Find who is stuck, from the trace and the logs, and name it: erofsd's serving loop (a receive
   that never returns at two harts: IPC3's per-endpoint delivery, the hub's waiters, verityd's
   block checks under two readers), the kernel (a lost wake, a timer on the wrong hart: SCHED4,
   B87's interrupt-claim shape), or the steward (a call of its own outside any batch). The
   kernel's trace and the scheduler oracle (tests/sched_oracle) are the instruments; add a
   trace point only where none shows the answer, and say so.
3. The fix where the cause lives (servers/erofsd, libs/rt serving, kernel/src), with a two-hart
   case on both widths, or one width if rv64 cannot show it, said why. A libs/rt contract change
   is built against every bin linking redoubt_rt before any whole bench.
4. The steward-latency rv32 two-hart row regained: the case's table (tests/steward-latency.toml
   and docs/servers/steward.md's responsiveness section) says met where it is now met. A miss is
   a finding, never a tuning: no target moves, no timeout grows to hide the stall.
5. Docs where the rule lives (serving.md, scheduling.md or erofsd.md), one paragraph, no dates.

## Gates before the report

Both widths: steward-latency N = 3 at two harts and one hart (rv32 seeds 3 and 4 at least);
the steward set and init-boot; erofsd's cases; the new case; docs (as above); size-budget
(the sum rule: adjust the toml by the measured delta only); unsafe-budget; fmt. Then the report:
cause named with the trace lines, fix, cases, measured rows, residuals.
