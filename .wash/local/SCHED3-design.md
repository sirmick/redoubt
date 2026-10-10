# SCHED3 analysis checkpoint (sched3-implementer, 2026-10-09)

Base: main bf3fa75d2 (SMP4 merged). Logs: SMP4's `ta` set (IRQ1 + SMP4, hold-trace, 2 harts),
`.tmp/SMP4/logs/ta/`, and `tb` (IRQ1 before SMP4) for the bimodal case. Tools (scratch, not
committed): `.tmp/SCHED3/uncap.py` (each `u`: the pass forfeited, in ticks at the budget's weight),
`.tmp/SCHED3/where.py` (kernel sections and destructions by hart and the hart's runner),
`.tmp/SCHED3/billed.py` (timer/device entries: the ticks billed to each payer, by the budget
interrupted). No code written, no case run.

## Summary

The four misses come from three mechanisms. Two of them are the kernel giving the victim less
than R12 promises, and one is the lock's residual, which R12 already concedes:

| Miss | Mechanism | Verdict |
| --- | --- | --- |
| budget-churn-shell rv64 387 (rv32 509) | A1: a carve caps the victim while its thread waits; the return uncaps it and lifts it to the shell's pass, erasing what it was owed | kernel bug (lift rule) |
| deadline-flood rv64 413/402, bimodal 875 | B: every deadline destruction runs on the victim's hart; billed to the creator, but at 2 harts x 1 thread nothing can repay it | kernel gap: R12's "no pattern of arming deadlines gets it more" fails |
| sched-exit-churn threads-exit rv64 441 (rv32 483) | A2 (a stale cap set mid-switch, 366 lifts) and B (the attacker's poll timeouts expiring on the victim's hart) | kernel bug and gap, both as above |
| kernel-containment bystander rv32 442 (rv64 463) | C: lock waits behind the attacker's non-audit sections (bystander 26.6M ticks of waits vs attacker 19.6M on rv32); no uncap of the bystander, nobody else's billed work on its hart | the target overstates R12 under one lock (R78 residual) |

## A. The uncap lift erases credit the victim was owed

R12: "A budget that stops being capped is lifted to `max(own pass, floor)`, as a waker is: it
cannot bank what it had no thread to run." This lift is right only for a lag the budget built up
while it was capped *and running every thread it had*. The trace shows two ways a budget is
lifted for a lag it built up otherwise.

**A1, a carve caps a waiting victim (budget-churn-shell).** rv64, records 1742-1811, passes in
ticks at weight 100:
- 1742: the victim (32, w100, 1 thread) reaches a slice end at 253530 and is requeued. The shell
  (31, w100, 2 threads) is lower (239992 and 252859), so it takes both harts (1748, 1763). Fair so far.
- 1773: the shell carves 50 (G: 100 -> 50, lead doubled, 262811 -> 272092). Cap set: victim
  `100 x 2 > 1 x 150`, so it is **capped although its thread is waiting** (J 32 1). The floor leaves it out
  and follows the shell, which runs at w50, charged double: 309028, then 327216.
- 1791: the victim is picked at 253530. 1802: the carve returns (50 -> 100, floor 327216 = the
  shell's pass), so the victim is uncapped. 1811: `u 32`, lifted 253530 -> 327216. **73,686 ticks
  forfeited**, all of it what the victim was owed (the shell's double charge plus its two-hart
  slice).
- In the window: 82 lifts, 5.78M ticks forfeited (rv64; the charged pool is 5.86M); 45 lifts and
  6.31M on rv32 (pool 13.4M). All 82 come 9 records after the carve's return.

**A2, a stale cap set mid-switch (sched-exit-churn).** `libs/stride/src/lib.rs:725-731`:
`switch` decrements the old runner's running count (`ran_by(c, false)`) before `fold(c)`, and the
fold raises the floor and recomputes the cap set. A one-thread budget leaving its hart then has
`k = 0` (waiting is recounted only at the reconcile). `cap()` skips it in `W`, but `raise_floor`
still counts its pass in the minimum. rv64, records 1903-1934: the attacker (31) leaves at 281499
with `k = 0`, so the victim (32, w100 k1, `200 > 1 x 100`) is capped and the floor takes 281499.
When the attacker wakes, the victim is uncapped and lifted 267895 -> 281499. 366 lifts and 6.30M
ticks on rv64, 200 and 2.75M on rv32 (pools 20.3M and 24.2M).

**Fix (proposed, R12 restated through the model):**
- A2: fold before `ran_by(c, false)` (the charge sees the counts the budget ran with). Or
  equivalently, a queued budget with `k = 0` counts in neither `W` nor the floor's minimum.
- A1: the lift at uncap removes only the lag gained while the budget was capped with no thread
  waiting. Rule text: "A budget that stops being capped is lifted by what the floor rose while it
  was capped and ran every thread it had; a lag it had when capped, or gained while a thread of it
  waited for a hart, it keeps." Implementation: one `owed: u128` per queue slot (or in `State`).
  At each raise, if the budget is not capped or has a thread waiting, set
  `owed = (floor - pass)+`. At uncap, set `pass = max(pass, floor - owed)`. A simpler variant
  (F2) skips the lift when the budget had a thread waiting at any raise while capped: more
  generous to the budget, one bool.
- Model (`model/src/sched.rs` `raise_floor`/`cap_set`): the same rule; new mutation
  `R12UncapForfeitsWait` (today's lift), caught by a new `scheduler_fairness` scenario: a 1-thread
  victim beside a 2-thread budget that carves half and returns it while the victim waits. The
  differential agrees step by step. `R12UncapBanksCredit` must still be caught (its scenario has
  the capped budget running).
- Oracle: it records `u` but does not check the value. Add `u <= floor` at least; the full
  recomputation needs the cap set, which the oracle does not rebuild.

## B. Billed kernel work on the victim's hart cannot be repaid at 2 harts

deadline-flood rv64 (records 1606-8502): **all 90 deadline destructions run on hart 0 while it
runs the victim** (1.11M ticks of sections net of audits, against the victim's 1.27M charged).
rv32: 73 of 73 (1.42M). They are billed to the creator, correctly. With one thread each on two
harts, though, no pick can repay the victim. It cannot use a second hart, and denying the creator
its hart would leave that hart idle. **The bimodality is placement:** in the `tb` run that read
875, 85 of 91 destructions ran on the creator's own hart. The creator's hart is often in the kernel
with interrupts off, so whichever hart is in user mode takes the deadline's timer. P2 keeps an
excused waiter on its hart, which made that the victim's more often (875 -> 413).

sched-exit-churn rv64: timer entries interrupting the victim billed 1.24M ticks to the attacker
(its 1 ms poll's expiries); 0.53M on rv32.

R12 says "no pattern of ... arming timeouts and deadlines gets it more". Here a creator that arms
deadlines takes ~17 % of the victim's hart, so this is a kernel gap, not a target to restate.
Options:
- **B1 (my recommendation): billed work runs on its payer's hart when the payer runs on one.** A
  hart arms its timer for a deadline or timeout only if it runs that item's payer (the deadline's
  payer as R10 names it; a timeout's thread's budget), or if no hart runs the payer. The cost: a
  deadline then waits for the payer's hart's next entry, up to a section (R10's notice latency;
  timer.md's arming rule changes). Device interrupts billed to an owner are left as they are
  (IRQ1 routing; a residual).
- **B2: restate.** Across harts R12 shares the picks, and kernel work billed to a budget other
  than a hart's runner is a residual. The oracle would take it out of the victim's want as it
  does waits. That keeps a real loss on hardware (a 1-thread victim loses its hart to a flooder),
  so I advise against it.

## C. The containment bystander: lock waits, the R78 residual

kernel-containment rv32 (records 469682-579350): the bystander (44, w100, 1 thread) is never
uncapped and takes no one's billed work. Its billed lock waits are 26.6M ticks against the
attacker subtree's 19.6M. Gross of the waits it would read 477. Under one lock, one budget's
sections cost the other hart time that no billing gives back (scheduling.md, residuals). R12 does
not promise more than that, so the target overstates it. Proposal: keep `@1`, and say so with
these numbers. SMP6 (zeroing outside the lock) shortens the sections, so re-measure after it. The
gate config has no hold-trace, so I have not split the waits by cause. A run with it would name
the attacker's longest sections.

## D. sched-wake-no-preempt at several harts

The property is that a wake never ends another thread's slice. At several harts a timeout is
answered at the first entry on any hart: another hart's call (11 of 20), a slice end (7) or
mid-slice (2). Proposed reading: "the entry that answers a timeout, on whatever hart, deschedules
no runner whose slice has not ended for its own reason (its slice's end, a block, an exit, a
fault, a deadline); the woken thread first runs at a pick: an idle hart's, or one after its
runner left for such a reason." Oracle: for each nap's wake (`W` of the sleeper's budget), take the
entry that recorded it. No `R`/`D` of that hart's runner in that entry unless the entry is its
slice end (`I..O` returning to kmain with the slice spent), a block or an exit. No other hart's
runner is descheduled by an IPI from it. Require at least, say, 5 of the 20 naps answered at an
entry that is not a slice end (a witness count, like `stale_waits_in`), so the check cannot pass
for want of them. Judged at 2 harts, `keep_smp` dropped. Negative: a test-only kernel feature
whose timeout wake ends the answering hart's slice must fail it (the model already has
`R12TimeoutWakePreempts`).

## Proposed order (after the ruling)

1. A1 + A2 in `libs/stride` and the model, with the mutation and scenario; differential;
   prototype run of budget-churn-shell and exit-churn at 2 harts (hold-trace), before and after.
2. B1 if ruled (kernel timer arming plus timer.md and scheduling.md), then deadline-flood and
   exit-churn at 2 harts.
3. The wake-no-preempt oracle check and case at 2 harts.
4. Judge at 2 harts and drop `keep_smp`/`@1` where they pass: budget-churn-shell,
   deadline-flood-billed-traced, exit-churn threads-exit, wake-no-preempt. Containment stays `@1`
   per C. Update the keep table and residuals.

Questions: (1) Is A's rule restatement (keep the lag gained while waiting) right, or should the
uncap lift go? (2) B1 (payer's hart) or B2 (restate)? B1 changes timer arming, which the Architect
may want to rule on. (3) Containment stays `@1` until SMP6: agreed?

## B1 trigger (architect-10 ruling, 2026-10-09, thread sched3-b1-evidence)

B1 is not built in SCHED3. It returns as a plan node if the oracle's report-only count of timer
interrupts charging a budget other than the one they interrupted reaches 1 % of a victim's charged
pool in any gate case, or a gate fails with destructions on the victim's hart. Met on the final
SCHED3 build (deadline-flood-billed-traced, rv64, 16 deadlines: 87 of 96 destructions on the
victim's hart, 89 entries, 833,543 ticks, ~17 % of 4.99M; victim 482, passing). Filed as SCHED4.
The B1 code is reachable as commit 33eaaf326.
