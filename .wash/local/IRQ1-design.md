# IRQ1 design note: device interrupts on every hart's PLIC context

irq1-implementer, 2026-10-08, against main 19e3439b9. For the orchestrator, before code.

## What happens now (read from the code)

- The loader finds every listed hart's S-mode context (`loader/src/dt.rs` `s_context`, used only
  to park a hart whose context is past the window) but passes the boot hart's alone, in `Plic`
  word 4. `intc_plic.rs` keeps it in one `CONTEXT`; enable, threshold, claim and complete all use
  it. Only the boot hart sets `sie.SEIE` (`arch::init`); other harts set STIE and SSIE
  (`timer::init_hart`).
- The kernel runs with `sstatus.SIE` clear, so an interrupt is taken only from user mode or in
  `kmain`'s idle window (`arch::idle`), and its handling runs under `KERNEL_LOCK`: a trap from
  user takes the lock first (`irq.rs` trap entry); the idle window already holds it. Claim and
  complete happen in one kernel section (`irq_fired`, or the unowned path), so claims are already
  serialized by the lock.
- At two harts in `sched-lock-contention` (hart A hammer, hart B idle with the driver blocked):
  the alarm reaches A only; A finishes its 9.6 ms search, returns to user, traps, claims, wakes
  the driver and IPIs B; B, woken through the firmware, draws its ticket after A's next
  `map_anon` has: two sections, 18.5/19.2 ms p50/p99.

## Design

1. **Loader: each listed hart's context.** `Hart` gains one word per listed hart after the ids:
   its S-mode context (the loader computes it already), by boot index. `Plic` word 4 stays the
   boot hart's (same value as `Hart`'s first). The kernel's `listed()` reads the new words; the
   arg layout check grows to `2 + 3 * listed`. Host test: the fixture's contexts reach the
   `Hart` tag in boot-index order (loader tests already build 2-hart trees with odd contexts).
   No PLIC: no words change meaning (contexts written 0, unused).
2. **Kernel PLIC backend, per hart.** `CONTEXTS: [AtomicUsize; MAX_HARTS]`, each set at boot
   from `Hart`. Each hart, when it comes online (boot: `intc::init`; others: `hart_main`, before
   it sets `sie.SEIE`): threshold 0 on its own context, and every source's enable bit set on it
   (32 word writes; WARL ignores sources that do not exist). Claim and complete use the calling
   hart's context (`hart::index()`). `CLAIMED` stays one word: claim and complete are in one
   section under the lock; a checked build asserts it is 0 at a claim.
3. **Masking by priority, not enable bits.** `disable_irq` writes priority 0, `enable_irq`
   priority 1: one write whatever the hart count, no per-context bits to keep in step (a hart
   coming online needs no copy of the current mask), and a priority write is what makes QEMU's
   PLIC re-evaluate (today's comment on `enable_irq`), so the re-arm of a source that went
   pending while masked keeps working. A complete is never ignored: the enable bit stays set.
   `intc::init` writes priority 0 to sources 1..1023 once, so a source no device object holds
   never delivers (today: never enabled). R5 is unchanged: the source is masked from the fire to
   the next `receive`. boot.md's controller contract and R5's "complete while still enabled"
   sentence are reworded to the mechanism.
4. **Which hart takes it.** Every hart that can take it: any hart in user mode at once, an idle
   hart from its `wfi` (it draws a ticket, then takes the trap in the idle window), never a hart in
   a kernel section or a lock wait. The first to hold the lock claims; the PLIC drops the line on
   every context at the claim. At two harts in the case above, B is idle when the alarm fires
   while A searches: B draws its ticket during the search, A's next call draws behind B, B claims,
   wakes the driver and picks it itself (`kmain` on B), with no IPI: one partial section, p50
   expected about 5 ms + handling. No routing policy (threshold steering) in this package: see
   residuals.
5. **Losers: a claim that finds nothing.** A hart that trapped for the same line and claims after
   the winner gets 0 (already handled: `None` resumes). From user mode today that entry is billed
   to the interrupted budget (`begin_billing` for every non-timer entry). Proposal: an external
   entry that claims nothing is billed to nobody, as a timer entry that finds nothing and does not
   end a slice already is (scheduling.md "Charging"); and a checked-build count printed at
   `system_reset` beside the lock's evidence: `external interrupts: N claimed, M found nothing`.
   QUESTION 2 below.
6. **Lock waits must not be woken by a pending SEIP (and STIP).** `halt_for_lock` and the
   shootdown wait halt with `wfi`, which ends at once while any interrupt enabled in `sie` is
   pending, taken or not. A device line held while a hart waits for the lock (it cannot claim
   without the lock) would turn every waiting hart's wait into a spin, which under icount takes
   the holder's time (R78's 250 s spin negative). This exists today on the boot hart (SEIE set)
   and on every hart for a timer that fired during a wait (STIP stays until the next arming).
   Fix: mask `sie` to SSIE alone around those two halts and restore it after; two CSR writes per
   halt. Touches hart.rs only (`halt_for_lock`, `shootdown`), not cell.rs or irq.rs's lock entry.
   QUESTION 3: in scope, or the timer half to SMP4/a bug node?

## Races between two harts

- Two harts claim the same line: PLIC claim is atomic and both claims are under the kernel lock
  anyway; the second gets 0 (point 5).
- A claim on one hart, the complete on another: cannot happen, one section does both. A checked
  assert ties them (point 2).
- Mask on hart X while hart Y is mid-trap for that source: Y waits for the lock, then claims;
  priority 0 means it gets 0. Unmask (`receive`) races nothing: it is under the lock.
- A hart started after a source was unmasked: enable bits are all set at its online, priority is
  global, so it takes the source at once. No catch-up.
- `wake_idle` from the claiming idle hart: it is no longer marked idle (cleared after its
  acquire), so the woken driver is picked by that hart, not IPI'd elsewhere.

## Effect on the cases (measured before and after, both widths)

- `sched-lock-contention` (2) and `-4` (4): drop `gate_harts=1`; gate the driver wake at 15/50 ms
  at 2 and 4 harts; edit the toml comment and expect line, the program's header, scheduling.md
  (R78 paragraph at line ~1100, the residual at ~1247: delete the "removes the second section"
  plan sentence, state what remains). At four harts the bound is the sections ahead at the
  draw (up to three of 9.6 ms, 29 ms < 50 p99); p50 against 15 ms at four harts is the risk I
  cannot predict (today 4.3 rv64 / 7.1 rv32 p50): measured first, reported before gating.
- `kernel-containment` and the share cases: the handling is billed to the owner wherever it runs
  (`bill_irq` is hart-blind); losers' entries billed to nobody (point 5) leave shares unchanged;
  lock waits are net. Expect no change; I compare the `lock waits N of 1000` and bystander lines
  at 2 harts before and after. `sched-latency` at 2/4 harts: the driver wake should improve
  (rv32 N=16 17.2 ms against 15 today); recorded.
- Point 6 shortens any wait that spun; it may move the lock-wait shares SMP4 is measuring.
- Unchanged at one hart: same context, same mask writes.
- Tests: `uart-irq`, `irq-first-receive`, `receive-bad-record` at 1 and 2 harts; a new boot case,
  `irq-any-hart` (2 harts, checked build): the RTC alarm fired while the boot hart is held in a
  kernel section by a hammer, the trace or a checked-build counter shows the claim on a hart other
  than the boot hart (verdict from the kernel's record, not the program). The mutations R5NoMask
  and R5NoUnmask stay valid (masking by priority is still the model's mask).

## Residuals (written on the pages if accepted)

- An interrupt that arrives while a hammer is in user mode can be claimed by it; the driver then
  goes to an idle hart by IPI and can wait a second section, as today. Rare (user time between
  calls is small); steering by threshold (a busy hart's threshold 1 while an idle hart exists) is
  a follow-up, not built.
- Every hart in user mode when a line rises enters the kernel once (one claims): N-1 short
  entries per interrupt at most, counted by point 5's line.

## Questions

1. Loader wire change (`Hart` gains contexts) and masking by priority: agreed?
2. Bill an external entry that claims nothing to nobody (a Charging sentence; no model rule I can
   find bills it): agreed, or keep it the interrupted budget's?
3. The halt mask (point 6): in IRQ1 for both SEIP and STIP, or SEIP only and STIP filed?
4. Four-hart p50: if measured above 15 ms after the change, gate p99 only at four harts and
   record p50 as a residual, or come back to you?
