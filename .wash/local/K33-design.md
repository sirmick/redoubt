# K33 design checkpoint: a preemption picks where it is (2026-10-12, k33-implementer)

Base: main db6575a3f (wp-K33, no change yet). No kernel edit before the ruling.

## 1. Measured before (main db6575a3f)

Method as K31's (.wash/local/K31-report.md): release image (`launch --system --print-only --smp 1`),
QEMU `-icount shift=3,sleep=on -rtc clock=vm`, one hart, alice's console session running
`Enum.reduce(1..5_000_000, 0, &max/2)` alone on the hart. Exec log (`log exec,nochain,int`) for
2 s, every executed block weighted from the disassembly. Tools and raw data:
/home/mcloonan/redoubt/.tmp/K33/ (tools/ = K31's, repointed, plus phases.py; m64/, m32/:
exec.log, entries.txt, tl50.txt = entry 50's timeline).

| per timer entry (every one a slice end that re-picks the same VM thread) | rv64 | rv32 |
| --- | ---: | ---: |
| entries in the window | 87 | 198 |
| kernel instructions, mean / median | 8,987 / 8,987 | 12,984 / 13,002 |
| SBI set_timer calls / firmware instructions | 1 / 52 | 1 / 57 |

Entry 50, by phase (tools/phases.py on tl50.txt):

| phase | rv64 | rv32 |
| --- | ---: | ---: |
| A. trap to `preempt` (trap, lock, from_user, expiry hint, billing, slice_over) | 952 | 1,797 |
| B. `preempt`: the thread made ready (`leave_previous`, `set_state`), kmain's context and satp | 960 | 1,302 |
| C. leave(kmain): billing, settle, deschedule (fold, requeue), reconcile, slice NEVER | 2,011 | 2,522 |
| D. into kmain: its expiry, the pick (settle, reconcile, queue, `next_thread`, cursor) | 2,070 | 3,114 |
| E. the switch: S-mode ecall trap, handler, `set_redoubt_result`, activate the thread | 1,534 | 1,918 |
| F. leave(picked): billing, settle, switch on, owed, reconcile, slice end, rearm, return | 1,512 | 2,455 |
| total | 9,039 | 13,108 |
| the round trip (B..F) | 8,087 (89 %) | 11,311 (86 %) |
| of D, `next_thread`'s walk of every live PID (self) | 669 | 840 |

What the rules need of a slice end that re-picks its own thread: close the billing, fold what it
ran (MIN_CHARGE), requeue behind its equals (tie = back + 1), raise the floor, reconcile, see the
queue pick it again, set the slice end, arm the timer, return. Estimated (to be measured, not a
promise) at about 1,200-1,600 after phase A: an entry of about 2,300-2,600 on rv64 and 3,800-4,500
on rv32, against 8,987 and 12,984. Of that 8,987 the round trip's own overhead is so about 6,500
rv64 and 8,500 rv32 (72 % and 66 % of the entry), not "about half" (K31's estimate).

## 2. The question: can the hart re-pick in place?

Yes, when the slice ended and the queue picks the same thread again, which is every slice end of a
thread alone on its hart (the VM: beamlet-reduction-rate; each spinner at two harts in
sched-timer-entry). Nothing in R12 needs `kmain`'s context: the model already states a slice end as
a deschedule then a pick (`Scheduler::slice_end_on`, `pick_on`), and the kmain hop is not in it.

### Recommended: one mechanism, two exits

At a slice's end (`irq.rs`, `slice_over`; not a budget deadline's preemption, which keeps the round
trip: it is rare, and its destruction's lifts are what the pick must follow), the trap runs
`kmain`'s step itself, on the trap's stack, with no switch to `kmain`'s context, no S-mode ecall and
one leave:

1. Billing closed, the runner folded and requeued exactly as `leave(kmain)` does today:
   `Harts::switch(h, None, requeued = true)`, the reconcile; `kmain`'s expiry hint (`expire_due`'s
   early return; anything due is expired as `kmain` does, its billing paused); the pick
   (`Harts::pick`). The same stride calls in the same order as the round trip, so the queue's
   states, ranks, ties, floor and cap set are the round trip's, and the trace records the same
   events (H 0, R, K, H b).
2. **In place**: the pick is the preempted budget and its only runnable thread is the one
   preempted (`mm.ready(b) == 0` with that thread uncounted): no process-table change, no
   context or satp change, no `next_thread` walk (the cursor is set to that thread, the walk's only
   answer). `switch(h, Some(b))`,
   the slice end, rearm, return.
3. **Direct switch**: anything else (another budget, a sibling thread): the preempted thread is
   made ready and the picked one activated straight from it (`activate_process_thread`, which
   already switches between any two threads), then one leave. `kmain`'s context is used only
   when the pick finds nothing (idle), after a block, exit or fault, and for a deadline.

The counts: in the round trip the settle in leave(kmain) counts the preempted thread as waiting
(`leave_previous` marked it ready). In place it is never marked, so the kernel gives the stride the
same count for the deschedule (`set_waiting(b, ready + 1)` at several harts; `still_runnable` true
for `b`), and the count returns to `ready` at the switch on, as the round trip's leave(picked)
settle does. A checked build runs the marks' audit (`audit_marks`, with the cap set's
water-filling) at the same once-a-slice cadence after the in-place switch, when the counts are back
in step.

Why not in place alone: sched-timer-entry is gated at one hart, where two spinners alternate, so
every gated slice end switches budgets. In place alone leaves that bound where it is; the direct
switch is what lowers it. If the ruling wants in place alone, the bound to lower is a new
one-spinner phase or case (see 4).

### What the model needs

No rule changes, no new mutation. The stride's call sequence is the round trip's, so
`redoubt-stride`'s differential against the model (1, 2 and 4 harts) still covers it. What is new
is the kernel's in-place count (the preempted thread counted as waiting without being marked).
Two ways to prove it, I recommend (b):
- (a) kernel only: the checked build's marks and cap audits, plus the sched-* oracle.
- (b) one wiring op in `libs/stride` (`Harts::repick(h, ...)`: the switch off with its thread
  counted, the reconcile, the pick, and the switch back on when it picks `b`), driven by the
  differential at random slice ends in place of the switch pair, and a host test that it leaves
  queue and runners equal to the pair on random states at 1, 2 and 4 harts. **libs/stride is not in
  my owned paths: (b) needs libs/stride/src and libs/stride/tests/differential.rs.**
The preemption points do not change: a slice still ends only by `slice_over`, a wake still never
preempts (the direct pick runs only where the round trip picked anyway), and a deadline keeps its
path.

## 3. Which verdicts could move, and why

- sched-timer-entry: 1 hart (gated) takes the direct switch, so its sections fall (a rough estimate:
  35-45 %, not measured); 2 harts (recorded) take in place. The bound is lowered to the new p99/max with the margin
  stated (deliverable 2).
- beamlet-reduction-rate: rises; about 6,500 rv64 / 8,500 rv32 fewer instructions a ms of a
  125 M-instruction hart: about +5 % and +7 % (floor 42,000 unchanged).
- Under icount (K31's turn artefact): every slice end gets much shorter, so QEMU's turns move.
  sched-capped's 2-hart shares (recorded since `hart_shares_from=3`; the turn landed in
  leave(kmain)'s audit or kmain's expiry walk, both of which leave a re-pick in place),
  sched-wake-no-preempt-harts' witness count (CALL_US 50 gave rv32 11-15; a cheaper slice end
  brought slice ends ahead of calls before; floor 5), sched-latency's p99s, the lock-contention
  waits. Each would be shown against its -mttcg twin before any bound moves.
- nobody/1000 in the oracle: `kmain`'s expiry walk (nobody's) leaves the re-picks; it falls.
- sched-share, sched-exit-churn, sched-capped(-mttcg), lock-contention(-4)(-mttcg),
  irq-boot-hart-only, kernel-containment, launch-idle: no rule change; idle and kmain paths
  unchanged; expected unchanged in verdict.
- The oracle (sched_oracle.rs) should need no change if the records keep their order; if its
  kernel-time bracketing (kmain's `kernel_from`) must change, that is kernel-red's to agree.

## 4. For the ruling

1. Both exits (recommended), or in place alone (then sched-timer-entry's gated bound stays and a
   one-spinner phase or case carries the new bound)?
2. Proof (b) with libs/stride/src and libs/stride/tests/differential.rs added to my paths, or (a)?
3. A budget deadline's preemption keeps the round trip (recommended), or takes the same path?
