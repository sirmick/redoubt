# K38: one idle hart takes device interrupts (k38-implementer, 2026-10-22)

Base main 78664b727. Branch wp-K38, folded: 50d4f38ee kernel, 9ae0292ee bench, ac843bce2 docs.
Design: .wash/local/K38-design.md; rulings: wave-2026-10-18-rulings.md "K38 design: one idle hart
takes device interrupts; IRQ1 amended, not superseded" and "K38: the driver-wake shift under load
is the emulator's".

## What was built

- kernel/src/arch/riscv/intc_plic.rs: TAKER (a boot index or EVERY). `idling` (at arch::idle, and
  at `enable_irq`, i.e. receive's unmask): with every context open, this hart takes the role and
  closes every other context (threshold 1 = the sources' priority). `running` (sched::leave to a
  thread): the taker hands to another idle hart (open there, then close here) or opens every
  context if none idles. `online` opens a late hart only in EVERY. One hart: nothing moves.
  `report` prints the claims' total "(N in all)".
- hart.rs: `idle_other`; `wake_idle` wakes the taker last (Q3). irq.rs: `idling`, `running`, `taker`.
- Refinement beyond the design note (reported 2026-10-22, in the docs commit's rule): an unmask
  with every context open takes the role first. Without it rv32 sched-lock-contention-4 read found
  nothing [16, 34, 0, 2]: the driver's receive waits behind the hammers' sections, its alarm is
  already pending at the unmask, and with every context open every hammer in user mode trapped.
- Bench: `[idle] device_interrupts_per_frame` (idle.rs, case.rs, unit test), the summary prints
  device interrupts a frame. Oracle `check_irq_received`: `IRQ-RECEIVED <n>` (program) must equal
  the kernel's claims total (unit test device_interrupts_received_equal_the_kernel_s_claims). The
  contention program prints it (rtc::find_counting counts the probe's interrupts: 1 under icount,
  2 under MTTCG, so the total is not a constant).
- Cases: launch-idle-probes `device_interrupts_per_frame = 1.0`; the family's routing clause:
  every hart's found nothing `\d`, total `(\d+ in all)` + IRQ-RECEIVED and the post-check.
- Size budget kernel 11114 -> 11173 (Size budget: line in the kernel commit). Unsafe +0.

## Witnesses, red on main (main's kernel + this bench, scratch worktree .tmp/K38/wt-main)

- launch-idle-probes (quiet, one window a width): main device 1.60 (rv64) / 1.77 (rv32) a frame
  FAIL; head 1.00 / 1.00 PASS.
- Family found nothing on main: 2 harts [198, 0] rv64, [0, 100] rv32; -4 [0, 103, 92, 96] rv64,
  [104, 0, 112, 119] rv32; -4-mttcg [0, 121, 130, 128] and [116, 0, 112, 122]. Head: 0 on every
  hart in every run; claims 201 (icount) / 202 (MTTCG) = IRQ-RECEIVED.
- Framing correction (recorded by the Architect): on main the idle hart claims too (v0 MTTCG
  claimed [2, 190, 3, 7]); the red clauses stand on the found-nothing counts.

## Measured

- Per-frame at rest, launch-idle-probes, one window, B111 load on general cores:
  main: interrupts 35.4 / 34.8, SBI 71.0 / 70.3, device 1.60 / 1.77;
  head: interrupts 31.0 / 29.4, SBI 65.5 / 63.9, device 1.00 / 1.00. (rv64 / rv32)
  The SBI rise over the recorded 48-53 is K39's (main too; 1242 of head's rv64 window are
  time::rearm): B112. Ceilings not re-derived (wait for B112 and K37, alone rounds).
- Move cost (checked build, timing counter measured then removed): MTTCG 4.7-5.1 us a move to or
  from every hart (3 writes), 51-71 us a hand-off between idle harts (2-3 a run); icount 0.4-0.75 us.
- Driver wake, net p50/p99 ms: icount 2 harts 2.0/2.4 rv64, 2.6/3.6 rv32 (main 1.3/1.8, 1.5/2.4);
  4 harts 6.7/7.6, 9.7/10.8 (main 5.3/6.8, 7.9/9.2); MTTCG 1.20/1.81, 1.25/1.77 (main 0.89/1.70,
  0.88/1.61). All inside 15/50.
- Variants (scratch worktrees .tmp/K38/wt-v0, -v1, -v2), gross wake p10/p50/p90 us, -4-mttcg rv64:
  main 691/938/1246, v0 (no routing) 666/930/1181, v2 (routing, no moves) 1085/1263/1554, head
  1091/1243/1499. icount 2 harts p50: v0 1489 = main, v1 (no take at unmask) and v2 2301 = head.
  Trace (one wake, icount 2 harts): lock free +191 us after the alarm; head's halted taker enters
  at +686, main's idle hart at +197 (the hammer trapped, halted for the lock, gave up its turn).
  Ruled the emulator's (a); stated on scheduling.md.
- sched-latency both widths: N=1/4/16 driver and timer wakes all "target met" (e.g. rv64 N=1
  driver 1.15/1.83 ms, N=16 1.26/15.6). sched-irq-wake both widths PASS (one hart, unchanged).

## Gates (folded head ac843bce2; make -f scripts/jobs.mk set CASES=..., batches of <= 4 jobs)

rc 0: docs, formatting, size-budget, unsafe-budget, host-tests, sched-lock-contention,
sched-lock-contention-4, sched-irq-wake, sched-latency, smp-boot, uart-irq, irq-first-receive,
irq-attack (both widths where they have both); build-rv64, build-rv32.
Earlier on the pre-fold tree (same code): sched-lock-contention-4-mttcg both widths PASS,
launch-idle-probes both widths PASS, testbench unit tests (idle 13, oracle 68) pass.
Not yet run: net class (load run, ask), launch-idle (quiet), -4-mttcg on the folded head, the
alone rounds (after K37's rebase and B112).

## Summaries checked

- docs/kernel/boot.md contract paragraph: rewritten (taker rule, invariant, race, one sentence on
  the emulation shift, no numbers).
- docs/kernel/scheduling.md: R78 routing paragraphs (figures, counts, move cost), residual
  "Every hart pays for a device interrupt only when none idles" (cost measured, remedy built, the
  shift with its three figures and the trace), "A hart waits for QEMU's turns" (driver-wake cell),
  the billing sentence on a claim of nothing (context closed since).
- docs/servers/ipd.md "What a probe costs": the interrupt-shaped hops gone; the per-probe totals
  (26-30 / 32-36) and "Probes scale" are still the old measurement: re-derived with the alone rounds.
- docs/testbench.md: launch-idle-probes ceiling; the oracle's IRQ-RECEIVED; status list (40).
- docs/plan/m2-usable-shell.md item 4: routing sentence updated.
- docs/kernel/devices.md R5 text (priority masking): unchanged, still true. docs/kernel/README.md,
  README.md, GETTING-STARTED.md: no routing claims. kernel/src comments: hart_main's updated.
- tests/programs/src/sched.rs hammer comment: no longer says the hammers trap for the alarm.

## Notes

- Read in full: intc_plic.rs, the case files, mod.rs/irq.rs/hart.rs hunks. Large shared files
  (scheduling.md, testbench.md, sched_oracle.rs, case.rs, tests/programs/src/sched.rs) read at my
  hunks with their context, not whole.
- Quiet-core use: 2 head launch-idle-probes before asking (owned), then 6 + 2 + 2 granted.
  Once up to 8 icount boot jobs ran together (main witness + head), beyond the 4-job small set.
- Pre-existing warning: tools/testbench idle.rs `harts` field never read (not this package's).

## Next

Rebase on K37 when it lands; after B112: three alternating alone rounds a width (head vs main)
for launch-idle-probes' interrupt/SBI ceilings and ipd.md's per-probe cost; launch-idle (ceilings
unmoved); net class (load run, ask); -4-mttcg on the final head.
