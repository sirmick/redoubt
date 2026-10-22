# B112 report

Branch wp-B112 in .worktrees/B112, base main 78664b727, head **a13a80c59**:
- 14a778d96 kernel: a slice's end that only moves later re-arms no timer (`Size budget: kernel` +17,
  11114 -> 11131), with timer.md and scheduling.md;
- a13a80c59 bench: launch-idle-probes' ceilings are idle ceilings, with testbench.md and ipd.md.

## Cause, by trace (before the fix)

QEMU `-d int` logs of launch-idle-probes, the SBI `ecall` sites resolved against each run's release
kernel (the TIME extension 0x54494D45 has one site, `rearm`'s `timer::set`). Each set_timer is
attributed to the last trap its hart took that was not itself an SBI call or an M-mode interrupt
(scripts in .tmp/B112: classify.py, timesite.sh, perframe.py).

| set_timers after (rv64 / rv32) | before K39 (b6d28a6b6) | main 78664b727 (K38's logs) | fix |
|---|---|---|---|
| user ecall (962-974 a window) | 286 / 291 | 992 / 997 | 290 / 294 |
| s_external or s_software from user | 0 / 0 | 19 / 25 | 0 / 0 |
| s_timer from user | 176 / 183 | 98 / 85 | 80 / 77 |
| all | 662 / 674 | 1300 / 1300 | 568 / 553 |
| SBI calls a frame | 50.1 / 50.4 | 71.0 / 70.3 | 45.2-50.3 (below) |

So the re-arm was at nearly every return to user mode, **calls included**, not only at an
interrupt's return: the resume after a call's expiry moves the end by >= 1 µs under the host
clock, and `rearm` saw a changed target. Logs: .tmp/B112/pre, .tmp/K38/main-r1/int, .tmp/B112/fix.

## Fix (as ruled: a return re-arms only when the hart's armed target changes)

- `time::move_slice_end(at)`: the running slice's end moves later; if the timer was armed for the
  old end, that end is remembered (`Timer::moved`, per hart).
- `rearm`: while the timer is armed for the remembered end and the new target is later, no change.
  Anything earlier (a timeout, a deadline) re-arms as before. A new slice (`set_slice_end`) clears
  it. Coverage for other harts (`Hint::due_on`) is unaffected: an early timer still comes by.
- Callers: `resume_slice` (calls and interrupts), `audit`'s move, the excused-time move.
- The early interrupt: once at most a slice, only if the thread runs to its old end; `slice_over`
  finds it not over, `on_interrupt` zeroes `armed`, the return re-arms.

## Measurements on the fix (quiet cores, alone, alternating with launch-idle)

| round | rv64 ints / SBI / ue a frame | rv32 |
|---|---|---|
| 1 | 35.3 / 46.5 / 32.1 | 38.8 / 50.3 / 32.1 |
| 2 | 36.1 / 47.1 / 31.1 (31 frames) | 35.9 / 47.3 / 32.1 |
| 3 | 34.6 / 45.8 / 32.1 | 33.9 / 45.2 / 32.1 |

Ceilings = most + spread, rv32's (wider), rounded up: interrupts 38.8 + 4.9 -> **44**; SBI 50.3 +
5.1 -> **56** (main 70.3/71.0 fails; before K39 50.1/50.4 passes); user ecalls kept 33.1. Note:
main's interrupts (34.8/35.4) pass 44; the SBI ceiling is the one red on main.

Per-probe cost (probes window less launch-idle's over the frames between, 26-27):
rv64 22.6/26.3/22.7 interrupts and 29.4/32.5/29.5 SBI; rv32 27.5/26.4/26.4 and 34.6/33.3/32.7;
ue 14.4-15.0. ipd.md: 23-26 and 29-33 (rv64), 26-28 and 33-35 (rv32), from 26-30/32-36, 26-27/33-34.

launch-idle: PASS x3 a width, busiest hart 2.5-2.9 /s (rv64), 2.0-2.5 (rv32).

Nobody shares (of 1000, rv64/rv32), re-recorded on scheduling.md (K39's in brackets):
exit-churn 36/65 (35/61), sched-latency 1 hart 79/122 (76/117), timer-flood 35/74 (29/61),
budget-churn 73/116 (71/110), kernel-containment 132/186 (116/162). The rise is the early
interrupt at a moved slice end, nobody's, at most once a slice; stated on the page. No case bounds it.

## Gates (exit codes; `make -f scripts/jobs.mk set CASES=...`, both widths)

- PASS: docs (on the final tree), formatting, size-budget (after the raise), unsafe-budget,
  no-cruft, host-tests.
- Smoke: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page
  (smp 1, 4): all PASS.
- Focused: sched-slice-own-time, sched-timer-flood, budget-deadline, timeouts, sched-timer-entry
  (smp 1, 2), timeout-hint-exit, sched-exit-churn, timer-flood-at-rest, sched-share,
  kernel-containment, sched-latency (smp 1, 2), sched-budget-churn: all PASS.
- Quiet: launch-idle-probes x3, launch-idle x3 a width on the fix: PASS (rc 0, 12/12).
- rv32 builds (every rv32 boot case built it).
- Not run: whole sched-*/smp-* classes, -mttcg/quiet sched cases, steward-latency*; the model is
  unchanged (no mutation added: the re-arm is not modelled). The kernel commit alone was not
  docs-checked in a scratch worktree (the bench commit touches no status line).
- After the docs fold only scheduling.md changed (the docs gate rerun PASS); the kernel is
  unchanged since every case above.

## Summaries checked

- timer.md "The hart timer": re-arm rule + figures, slice end bullet, status +launch-idle-probes (16).
- scheduling.md: slice paragraph (lazy re-arm, figures), residual return cost wording, nobody
  shares re-recorded.
- testbench.md "The machine at rest": the probe case's ceilings and per-probe figure.
- ipd.md "What a probe costs": restated.
- No change needed: README.md, GETTING-STARTED.md (no re-arm/SBI claims); beamlet.md's re-arm
  figure is the preemption's, unrelated; launch-idle's ceilings and testbench text unchanged.

## Open

- The residual return cost on scheduling.md (6.6/11.9 µs) was measured with an arming at every
  return; kept as an upper bound, not re-measured.

## Round 1 fold (2026-10-22), head cc0eaac31

- 8039f07d3 kernel, cc0eaac31 bench (docs and one doc comment only; no code change).
- Red P2: the bound restated as one early interrupt an arming, each after an entry since it
  (falls geometrically with the slice's paused share) on timer.md, scheduling.md (slice paragraph
  and nobody-share residual), time.rs's `move_slice_end` comment, and the kernel commit message.
- Editor P2: testbench.md says the ceilings are idle ceilings, tighter than launch-idle's twice the
  most. History wording removed from scheduling.md, timer.md and testbench.md; the defect's
  figures stay in the commit messages and the case's toml comment.
- Editor P3: "under the case's ceiling of 56" (timer.md) moved into the bench commit.
- Gates: docs PASS at 8039f07d3 (scratch worktree) and at cc0eaac31; formatting PASS at cc0eaac31.

## Train 101 fix (2026-10-22), head c99838233

- sched-irq-wake failed both widths on train 101 (proofs {32: 1} / {32: 2}, need 40); reproduced
  on the branch. Trace (rv64, records 1634-1656, .tmp/B112/irq/rv64-pre): `x 33 ... W 32 ... O 1`,
  then `I 33 = 420285 / entry 419660` (not over, O 1, nothing requeued: the timer armed for the old
  end), then `I 33 = 420299 / 420304 R 33 K 32 O 0`. The oracle read the first `I` as the slice's end.
- Fix, oracle only (kernel unchanged): `check_irq_wake_no_preempt` passes over the runner's timer
  interrupts that return O 1 with no K/R/D/X inside; unit test cases (early passed over; pick after
  it still fails). testbench.md's sentence on the check updated. Folded into the bench commit
  (orchestrator's ruling); 8039f07d3 unchanged.
- Gates on c99838233 (`make -f scripts/jobs.mk set CASES=...`): host-tests PASS, docs PASS,
  sched-irq-wake PASS rv64 + rv32 (60 of 60 proofs, scheduling.md's figure stands); formatting PASS
  on the same oracle content before the fold (rc 0). No family load run (orchestrator: not needed).
