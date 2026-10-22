# K38 design note: a device interrupt is taken on one hart (k38-implementer, 2026-10-22)

Base main 78664b727. Nothing built. Read: plan node K38, rulings "K35 after the measurement" and
K34's "irq-boot-hart-only after the search is bounded", K35-report (Named, Naming), boot.md
"Hardware abstraction", scheduling.md R78 routing paragraphs and the residual "Every hart pays for
each device interrupt", ipd.md "What a probe costs", intc_plic.rs, arch idle/halt, hart.rs
wake_idle, sched::leave, tools/testbench/src/idle.rs.

## Where the ~5 hops come from (code reading, matches K35's naming)

Every started context has threshold 0 and every source enabled; a source raises SEIP on all of
them. (1) Each other hart in user mode or idle traps, takes the lock, claims nothing: a hand-off
(0.75-0.85 a frame). (2) When the claimer was a busy hart (from user), its `sched::leave` finds the
driver's budget queued and unrun and sends `wake_idle`: an IPI, an idle hart's reschedule
interrupt, its hand-off to pick netd's rx thread (the ~1.0 "card interrupt" idle wake). An idle
claimer instead runs the driver itself (kmain picks it; leave(1) sends no wake).

## The rule

One hart takes device interrupts while any hart idles: **the taker**, an idle hart. Thresholds
(sources stay at priority 1, masked/unmasked by priority as today, R5 unchanged):
- the taker's context threshold 0; every other started context threshold 1 (above the sources'
  priority, so the PLIC delivers nothing there);
- when no hart idles, every context threshold 0: every hart takes it, exactly as today ("all").

Transitions, all under the kernel lock, in `intc_plic` behind two new contract calls in irq.rs:
- **at idle** (`arch::idle`, before the lock's release): if the state is "all", this hart becomes
  the taker and writes threshold 1 to every other started context (harts - 1 writes). If a taker
  exists, nothing (this context is closed already).
- **at a pick** (`sched::leave` leaving for a thread, `next.is_some()`): if this hart is the
  taker, it hands the role to another hart marked idle (hart.rs's idle flag, which `wake_idle`
  clears on a hart it wakes, so a hart about to pick is never chosen): threshold 0 there, 1 here
  (2 writes); if none idles, threshold 0 on every other context and the state is "all" (harts - 1
  writes). A non-taker's pick writes nothing. The taker woken by a timer or IPI that finds nothing
  stays the taker: no writes.
- **online** (a hart's first lock): sources enabled as today; threshold 0 in "all", else 1.
- **one hart**: no routing at all (no state, no writes): every one-hart case is unchanged.
- `wake_idle` prefers an idle hart that is not the taker, the taker last (saves the 2 writes and
  keeps the role where the interrupts already land). [Q3]

Invariant: the taker's context (or every context) is open at every instant; the writes that move
it are one section. A source pending at a move is raised on the newly open context (QEMU
re-evaluates its output on a threshold write; the spec's level semantics the same), so nothing is
lost. A claim of nothing can still happen in one race: in "all" a busy hart traps, and before it
holds the lock another hart idles and closes its context. That needs every hart busy first, so not
at rest. No model change: the model has no PLIC contexts; R78 (lock order) and R5 unchanged.

## Which hart, and the latency against the residual's numbers

At rest every hart idles, so the taker is the first hart that idled with none taking; the card's
answer is claimed there, the hart runs netd's rx thread itself (no wake_idle), hands the role to
another idle hart (2 MMIO writes, no interrupt and no SBI call), and takes it back only if that
one picks. Expected per frame: s_external 1.0 (from 1.75-1.85), claimed nothing 0 (from 0.85),
the card's idle wake 0 (from 0.9-1.1), and the hand-offs that follow them, about 5 a probe.

Latency: the residual's cost of every-hart routing (mttcg 4 harts: up to three claims of nothing
ahead of a wake, ~31 us p50 each, about 0.15 ms of a p99 of 4-12 ms) goes to 0 while a hart idles.
Added: a PLIC threshold write per move (to measure: expected a few us each under mttcg, a few
hundred instructions under icount), 2 at the taker's pick, harts - 1 at a switch to/from "all".
Delivery goes to a halted idle hart, never to a busy hart in user mode while one idles; IRQ1's
analysis (the idle hart claims at once, the woken driver's receive behind one section) is the
same, and today's idle hart already wins the claims (193 of 200 rv64 in -4). Expected: no target
moves; figures re-recorded for sched-latency, sched-irq-wake (one hart: unchanged by construction),
the contention family, bench-net classes, both widths.

## Cases and the red-on-main witness

1. **launch-idle-probes: a new `[idle]` ceiling `externals_per_frame = 1.0`** (bench: idle.rs
   counts s_external a frame already in its clusters; one new key, its unit test). Main: 1.75-1.85
   a frame, red. 1.0 is "claimed nothing 0 a frame": at rest the card is the one source and each
   frame one claim. The interrupts and SBI ceilings re-derived at 2x the worst of three
   alternating alone rounds a width (head vs main, K35's procedure); user ecalls unchanged
   (no behaviour at the sockets). On K37's tip, after it lands.
2. **The contention family's routing clauses** test IRQ1's every-hart routing, which K38 removes
   by design: `sched-lock-contention` requires hart 1 (the hammer's) to take >= 10, `-4` some
   non-boot hart, `-4-mttcg` every hart; under K38 the hammers' contexts are closed and only the
   driver's idle hart takes. Proposal [Q1]: each requires every hart's "found nothing" to be a
   single digit (expected 0) and keeps "taken" summing to the alarms; main reads 518-577 found
   nothing in -4-mttcg (red on main, exact checked-build counts); `-4` and the 2-hart case red too
   where main's busy hart traps. Lock-order clauses, wake targets and ticket counts unchanged.
3. **irq-boot-hart-only** was retired by K34 (no code or case on main): the node's "stays the
   negative control" is stale [Q2]; the witnesses are 1 and 2.
4. Gates: launch-idle (ceilings unmoved), launch-idle-probes, sched-irq-wake, sched-latency,
   sched-lock-contention(-4, -4-mttcg), sched-wake-no-preempt(-harts), smp-boot, uart-irq,
   irq-first-receive, irq-attack, bench-net classes, both widths; rv32 build; unsafe ratchet
   (expected +0: the PLIC accessor exists); docs. Pages: boot.md contract paragraph (the rule),
   scheduling.md R78 paragraphs + residual rewritten (cost measured, remedy built), ipd.md's
   probe cost, testbench.md case text.

## Questions

- Q1: the contention family's "taken by" clauses become "found nothing a single digit"
  (node says "sched-lock-contention-4's counts keep": I read that as the lock-order counts).
- Q2: irq-boot-hart-only is gone since K34; no negative control beyond the red-on-main clauses.
- Q3: wake_idle preferring a non-taker: in scope (it is the rule's "at a pick")?
- Q4: build on main now, rebase on K37 when it lands, then the alone rounds (quiet slot: I will
  ask).
