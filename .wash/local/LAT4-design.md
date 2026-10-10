# LAT4 design note: the 4-hart driver-wake p99 where harts run in parallel

irq1-implementer, 2026-10-08, on main a5e387664 (IRQ1 merged). For the orchestrator, before code.

## What is measured now, and why it is not the claim
`sched-lock-contention-4` (icount, 4 harts) reads p99 40-59 ms across builds: two causes, both in
.wash/local/IRQ1-numbers-2.md. (1) QEMU's single-threaded round-robin under `-icount`: a woken
hart runs only when the running hart's turn ends, and a hart in a kernel section keeps its turn to
the section's end. (2) The herd: an unmasked source raises SEIP on every context, every user-mode
or idle hart traps, all but one claim nothing, each takes a ticket (1-149 a run at 4 harts).
R78's claim is a count: a wake waits behind at most the harts less one sections plus the one in
progress. On parallel harts that is at most ~4 searches (4 x 9.6 ms virtual), under 50 ms.

## Part A (bench only, Tier B): a multi-threaded TCG case
`sched-lock-contention-4-mttcg`: the same program, `smp = [4]`, no `icount` (QEMU's MTTCG runs
each hart on its own host thread, as `smp-evict-mttcg` does), checked build, `lock-trace` kept so
the ticket-order check and the trace's `x`/`c` herd counts come with it, both widths.

Time is the host's here, so the 9.6 ms search is whatever the host's TCG makes it. Two bounds,
both from the same run, so a slower host moves both sides:
1. **Structural (the verdict):** the driver wake's p99 is at most 5 of the run's own search
   lengths (4 sections plus the handling and the pick, rounded up), each search's length taken from
   the trace (a `map_anon` refusal's Q-to-next-Q on its hart, or a hold record if SMP4's
   `hold-trace` lands first; else the program times its own calls and reports p50/p99 of them).
   The oracle judges `driver_wake_p99 <= 5 x search_p50` (a new `sched_oracle` argument,
   `driver_wake_p99_sections=5`), with a host test.
2. **Absolute (recorded):** p50/p99 against 15/50 ms, printed, not gated: on this host the TCG
   search is likely shorter than 9.6 ms, so 50 ms is easy here and says nothing elsewhere.

Host load: a host-clock case is a verdict only alone (docs/testbench.md, "On a shared host"). It
runs under `q run --cores 6` (4 vCPU threads, QEMU's I/O thread, the bench), pinned; a failure
beside other work is rerun alone before it counts, as for every host-clock bound. A preempted vCPU
thread lengthens a wake without lengthening the searches, which is what the ratio would catch, so
the rule matters. Before setting 5, a sweep alone of 20 boots per width (`--sweep 1..20`, no
`--jobs`) gives the spread of `p99 / search_p50`; the factor is the sweep's worst plus a margin,
written on the page with the sweep. If the sweep's worst is near the factor, the case keeps only
the record and the residual stays.

The icount case keeps gating p50 and recording p99; its residual on scheduling.md is rewritten to
point at the MTTCG case as the four-hart p99's judge.

## Part B (kernel, Tier A, only if A shows it matters): route a source to one idle hart
Mechanism: each hart's context threshold is 0 while it is open and 1 (= the sources' priority, so
nothing is delivered) while closed. A hart opens when it idles and closes when it leaves idle, but
never closes if it would leave no hart open: an `OPEN` bitmask, cleared by CAS only if another bit
stays set. So a fully busy machine keeps one busy hart open (it takes interrupts in user mode, as
now), and an idle hart, when there is one, takes them.
- Cost: one threshold write (MMIO) at idle entry and one at exit, plus a CAS; no lock (own
  context, an atomic mask). QEMU re-evaluates its output on a threshold write, so a source pending
  while every open hart was busy is delivered when one opens.
- Correctness: the claim path is unchanged (claim and complete on the claiming hart's context,
  under the lock); a hart that trapped and closed before it claims claims nothing, as now. R5 is
  untouched (masking stays by priority). The race to watch: two harts leaving idle at once must not
  both close (the CAS keeps the last bit). A checked-build audit at each claim: at least one context
  open.
- Herd: with one idle hart open, the empty claims at 4 harts should fall from 1-149 to ~0; the
  checked count shows it, and the case can require it (`found nothing` below a bound).
- What it does not fix: QEMU's turns under icount. It removes the tickets the herd puts ahead of the
  woken hart, so the icount p99 may drop too; measured, not promised.
- It changes IRQ1's text (boot.md contract: "any hart in user mode or idle takes a raised source").

## Plan
1. Part A, measure first on main: the MTTCG case recorded only, sweep alone both widths; report the
   ratio's spread, the absolute p99 and the herd counts to you.
2. Decide with you: gate the ratio (Tier B, bench only), and whether Part B is wanted.
3. Part B only if asked, as its own Tier A step with its own measurements.

## Questions
1. Part A's verdict as a ratio to the run's own search length (structural, load-tolerant) with the
   absolute 50 ms recorded: agreed, or do you want 50 ms gated under MTTCG on this host?
2. Where the search length comes from: SMP4's `hold-trace` if it is merged first, else the
   program's own timing of its calls. Preference?
3. Part B: only if the measurement shows the herd matters under MTTCG, or wanted regardless (it
   removes a cost the page now names)?
