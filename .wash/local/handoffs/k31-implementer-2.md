K31 HANDOFF / RECORD (k31-implementer-2, updated 2026-10-10 after train 21's wake-no-preempt fix). For K33 (the kmain round trip), B58, and anyone touching the scheduler oracle.

## Branch state
- wp-K31 in /home/mcloonan/redoubt/.worktrees/K31. Base 1fcdd656d (= origin/main). Head 58ef21b66, tree clean. FROZEN; kernel-red reviews 58ef21b66, then the orchestrator remerges and rebenches train 21.
- Commits:
  1. ff9e64a55 kernel: a slice's end pays for its work, not for checked calls, whole budgets and three timer arms.
  2. 168403835 tests, docs: sched-timer-entry bounds what a slice's end costs the kernel.
  3. bab03e8de kernel, tests: the claim count proves each hart's context takes the alarm (per-hart PLIC proof).
  4. be945cce6 tests, docs: sched-capped's two-hart shares are judged where the harts run at once (includes scripts/jobs.mk:54 quiet line).
  5. 58ef21b66 tests, docs: sched-wake-no-preempt-harts supplies its witnesses at another budget's call (CALL_US 100 -> 50).

## 1. The timer-entry change (ff9e64a55)
- A compute-only thread's slice end was 15,022 kernel instructions on rv64 and 20,692 on rv32 (release, exec log), with 3 SBI set_timer calls. Now 8,846 and 12,869, with one. No billing rule changed.
- Arming rule (time.rs docs): the timer is armed only on the way to where its interrupt is taken:
  - leave() to user;
  - kmain before idle;
  - a timer interrupt taken in the idle (on_interrupt(from_user=false)).
  note_timeout / note_budget_deadline / set_slice_end only record.
- Consequence: a timeout noted while the other hart runs in user mode is not armed on that hart until its next return to user.
- Kernel ceiling 10106 -> 10110 (Size budget lines).
- K33 (report item 5): a preemption round-trips through kmain (deschedule, pick, switch back on), ~7,000 of the 15,000 instructions. When the same budget is re-picked, both halves are wasted. Changing it touches redoubt-stride Harts::switch and the model.
- sched-timer-entry: `timer_section_p99_ticks=2000 timer_section_max_ticks=5000 gate_harts=1`.
  - Head at 1 hart: p99 1,073 / 1,597; max 3,431 / 4,234.
  - Before-tree fails both bounds: p99 2,241 / 3,006, max 5,807 / 6,831.
  - Recorded at 2 harts.

## 2. PLIC proof (bab03e8de)
- Per-hart claimed / found nothing / taken (checked build).
- The cases:
  - slc and slc-4 (icount): some non-boot hart taken >= 10.
  - slc-4-mttcg: every hart >= 10, taken [195,201,196,197] rv64 / [177,176,179,198] rv32.
  - irq-boot-hart-only: non-boot claimed 0.
- Recorded negative: the irq-boot-hart-only kernel prints taken [202,0,0,0] and fails.
- slc-4's icount p50 on rv32 is bimodal, 22.3/5.1 ms (main 7.3): the turn artefact, recorded not gated.

## 3. sched-capped (be945cce6): hart_shares_from, and why per-hart netting was refused
- rv32 smp2 late-a 449 < 450 (SMP6 465). Under icount QEMU ends hart 0's turn inside nobody's time at every slice end: the leave()->kmain audit (327 vs 81 µs) or kmain's expiry walk, ~2,600 ticks a slice.
- Proof:
  - no icount: late-a 502;
  - icount shift=4: late-a 434.
- Fix:
  - oracle argument hart_shares_from=N records the shares below N harts;
  - sched-capped uses =3;
  - new sched-capped-mttcg (smp 2, quiet) judges the 2-hart shares. 5 runs a width: late-a 497-501, b/c 249-251, uncap 818-819/89-91, spread 496-503.
  - Negative: holds-floor without icount misses late-b/c 30/470 and 31/469.
- Per-hart netting (B) refused by kernel-red: it hides work no budget pays for landing on a one-thread budget's hart, and the trace can't tell an icount turn from kernel time.

## 4. sched-wake-no-preempt-harts (58ef21b66) and B58
- Train 21 rv32 failed the precondition, not the property: 4 witnesses < 5, none 'took hart'. Main read 0 mid-slice / 21 at a call / 41 at slice end; K31 4-14 at a call.
- Mid-slice witnesses are structurally 0: the 300 µs nap ends while the sleeper's hart is still in the kernel, and the other hart isn't armed for the new timeout. Every witness is a spinner's call beating a slice end: phase.
- Fix: CALL_US 50. 12 runs a width: rv32 11-15, rv64 32-45 at a call, 0 mid-slice. The wake-preempts negative fails 'took hart' on both widths (recorded in the toml; run via a temp toml adding kernel_features "wake-preempts"). kernel-red's floor: rv32 min >= 10.
- B58 (filed, not K31): restore mid-slice witnesses with stepped naps (130 + 17k µs, k mod 1000), 37 µs calls, maybe 120 naps.
  - It needs the sched-trace race fixed first. trace::timer_entry samples slice_over at the entry's start; irq.rs decides after the expiry, and only its decision preempts. A slice ending during a long entry gives O pass 2 and a false 'took hart' (seen on MAIN too, rv64 2/3 runs; main-rv64-2 records 3013-3025).
  - Trial patch: /home/mcloonan/redoubt/.tmp/K31/b58-trace-decision-stepped-naps.patch (git apply -p1 on 58ef21b66). It records the kernel's boolean at the decision.
  - With the patch: K31 rv32 15-23 / rv64 47-49; main rv32 7-10 / rv64 40-41; negative fails.
  - kernel-red's conditions for B58: record the decision's now and slice_end so the oracle judges 'over' itself (not the kernel's boolean); a host test of a slice ending mid-entry; sched-trace only; negative still fails; 5 runs a width, mid-slice and at-call apart; rv32 min >= 10.

## 5. Recognising the icount-turn artefact
- The oracle's nobody/1000 rises with no rule change.
- A 2-hart share is short with 0 wakes and the budget keeps its hart (count H per hart).
- Bimodal audit U..V spans with no other-hart records inside, or a bimodal latency p50.
- Per-slice: period = charges + audits + y + X, and X > 0.
- Witness counts that swing with entry cost.
- Confirm by moving the turns: no icount, and icount shift=4.
- Fix by judging where the harts run at once (an -mttcg twin, quiet, 5-run spread + negative in the toml), or by making the case phase-independent. Never tolerance or kernel tuning to QEMU.

## 6. Tools
- /home/mcloonan/redoubt/.tmp/K31/tools: exec-log tools: entries.py, timeline.py, cats.py, sections.py, env/build/run.
- /home/mcloonan/redoubt/.tmp/K31/replay: runs a copy of sched_oracle::run on a saved log (`target/release/replay LOG 'args'`). The copy predates hart_shares_from; recopy (strip #[cfg(test)]) if needed.
- /home/mcloonan/redoubt/.tmp/K31/an: sched-capped traces and scripts (win.py, slice.py, per.py, aud.py, long.py, short.py, b.py).
- /home/mcloonan/redoubt/.tmp/K31/wnp:
  - wake traces (main-*, k31-*, call50/, step17*/, head/);
  - naps.py (wake minus deadline);
  - trial.sh (runs an export x widths x runs + the negative; edit its loops).
- Scratch exports:
  - /home/mcloonan/redoubt/.tmp/K31/main-export (1fcdd656d + stepped program + trace fix);
  - /home/mcloonan/redoubt/.tmp/K31/k31-export (be945cce6 + CALL_US 50 + neg toml).
  Both are built; delete when done.
- Report: /home/mcloonan/redoubt/.wash/local/K31-report.md. Sections: measurement; fix; fix round; train 20 sched-capped; Ruling A; train 21 wake-no-preempt; trials; Outcome.

## Traps
- Run dirs under target/testbench are pruned within minutes: copy a trace out at once.
- No --sweep on unpinned cases; repeat runs. Icount cases can still vary run to run (wake-no-preempt-harts is not seed-pinned).
- mttcg is host time: quiet class, alone, rerun flakes alone.
- q: 4 cores per job, never 16 on a full machine.
- Never edit the worktree while cases run.
- Stable rustfmt shows false diffs (nightly config); trust the formatting case.
- docs checker: a new test in a status list needs the count bumped (R12 'tested (N)') and the SECURITY.md row.
- Quote ranges from all replayed runs. One rv64 head run read 32 after I had sent 37-45.
- Messages to the orchestrator are <= 2000 bytes (the send fails otherwise).
- Stage by path, no stash, never push; amend into the right commit; a commit touching docs says 'tests, docs:'.
