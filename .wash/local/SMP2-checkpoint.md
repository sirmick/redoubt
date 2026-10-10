# SMP2 design checkpoint: the starting list, the spin cost, R12 across harts

Branch wp-SMP2 at main 36d1450f9 (worktree .worktrees/SMP2), nothing committed. The prototype
behind every number here is uncommitted in the worktree, also saved as
`.tmp/SMP2/proto-wfi-wait.patch`. Logs are in `.tmp/SMP2/` (`proto2-run/`, `proto4-logs/`,
`sw-rv64.log`, `sw-rv32.log` and their `.d/` copies).

## 1. The spin cut: a hart waiting for the lock halts (wfi), and the release wakes it

**Why the spin costs so much.** Under `-icount` QEMU runs the harts in turn on one host thread,
and its virtual clock counts every instruction any hart runs. So at H running harts each hart
runs at 1/H of the machine's rate (125 M instructions per virtual second), and a hart spinning
on the lock takes that rate from every hart doing work. A hart halted in `wfi` gives up its
turn. QEMU 10.2 treats `pause` and `wrs.nto` as no-ops that only end the translation block,
so neither gives up the turn. That is why the Zawrs hook alone does not help.

**The prototype** (kernel/src/cell.rs and hart.rs, ~40 lines):
- A waiter sets its bit in a `SLEEPERS` mask (SeqCst), re-reads `serving`, and if it is still
  not its turn runs `wfi`, then clears `sip.SSIP` and its bit. It still runs `serve()` on each
  turn of the loop.
- `release()` stores `serving + 1`, then (SeqCst fence) reads `SLEEPERS` and sends the IPI to
  each sleeper.
- Lost wakes are excluded by the Dekker pair: waiter (set bit, read serving) against releaser
  (write serving, read bits), all SeqCst.
- Clearing SSIP loses nothing, for two reasons. A shootdown is carried by the block's `shoot`
  word, which the next turn's `serve()` reads. A reschedule IPI goes only to a hart marked idle,
  and that hart is awake and picks once it holds the lock.
- FIFO (R78) is unchanged: tickets still decide the order.

**Measured.** All under icount (shift=3, sleep=off) unless marked. Times are guest time except
the host-time cells, as marked.

| | 1 hart | 2 harts, spin (main) | 2 harts, wfi wait | 2 harts, `wrs.nto` |
| --- | --- | --- | --- | --- |
| ipc-client, 1000 lend round trips, rv64 | 2.018 s | 17.361 s | 2.068 s | 17.361 s |
| same, rv32 | 2.005 s | 20.000 s | 2.065 s | 20.002 s |
| same, no icount (MTTCG, host clock, 1 run), rv64 / rv32 | | 480 / 449 ms | 500 / 467 ms | |
| sched-latency N=1 driver wake p50/p99 (gross, rv64) | p99 1.7 net, 1.9 gross (page) | 74.8 / 99.1 ms | 3.5 / 4.5 ms | |
| sched-latency N=16 driver wake p50/p99 (gross, rv64) | | 91.8 / 344.8 ms | 3.7 / 5.8 ms | |
| sched-latency N=16 steward timer wake p99 (gross, rv64) | | 595 ms | 18.5 ms | |
| sched-share, counted of the calibrated window (rv64) | | 19 / 1000 | 831 / 1000 (rv32) | |
| boot-profile, first console read (rv64) | | 51 s (SMP3) | 12.2 s (target 20 s) | |
| kernel-containment (rv64, host time) | | 35 of 112 terminations by 1800 s | all, expects PASS in 374 s | |

The table's sched-latency 2-hart rows are gross numbers, audits included. All its 2-hart wakes
are inside the targets (p50 15 ms, p99 50 ms). The deadline notice is 75-85 ms gross at p99;
its target is 40 ms net, and the net figure needs the oracle at 2 harts. At 1 hart a release
costs one fence and one load more; I'd skip both when `started() == 1`.

**Recommendation: the wfi wait first, on every platform,** in `wait_for_change` (the named place
where Zawrs `wrs.nto` goes later), so QEMU runs the same code that ships. Its cost without
icount is ~4 % on a contended IPC ping-pong (one run each), from one IPI per contended release.
On the strict barrel a spinning hart takes no cycles from its sibling, so there the gain is
power only.
- **The ticket lock** is already built (SMP1: `TicketLock`, R78). There is nothing to add; the
  M2 page's step 3 describes it as built.
- **Wake affinity** is not needed after the wfi wait: ipc at 2 harts is +2.5 % on 1 hart. The
  waker could not know it blocks next anyway, because log-server's `reply` and `receive` are two
  calls. I'd drop it, and record the remaining cost in the residual "A call within one budget
  crosses harts".

## 2. The starting list at `--smp 2`, with the wfi wait (prebuilt from this tree, both widths)

| case | rv64 / rv32 at 2 harts | proposal |
| --- | --- | --- |
| expiry-deadline-then-timeout, scan-bounds, receive-bad-record, timeouts, timeouts-tcg | PASS / PASS | **cost cut**; nothing to restate |
| map-anon-search-bound, budget-deadline (copies without keep_smp) | PASS / PASS | **cost cut**; drop their `keep_smp` (SMP3's reason was the spin) |
| boot-profile (-unverified not run, same boot) | 12.2 s first read, PASS (rv64) | **cost cut**; the 20 s target stands |
| kernel-containment | expects PASS in 374 s host (rv64); post-check not run | **cost cut**; post-check after the oracle work |
| sched-destroy-billing, sched-server-busy | PASS / PASS | judge from charges like the rest (below) |
| endpoint-destroy-full, sched-budget-churn, sched-carve-return, sched-exit-churn, sched-timer-flood, sched-ties | oracle: "rank clauses put budget N first" (one-runner replay) | **restate**: the oracle replays H runners; the case's own bounds stand (equal attacker and victim: the victim's water-filling share at 2 harts is one hart, 500 of 1000, unchanged) |
| sched-debt-lift, sched-lift-delay | oracle, as above | **restate**: counted in picks across all harts (a round is N picks); the prediction is unchanged |
| sched-wake-no-preempt | oracle, as above | **restate**: a timeout never preempts a running thread. It runs at once on an idle hart, else at the next slice end of any hart. The program runs one spinner per hart: I'd start 4 always (harmless at 1 hart, where the verdict is a lower bound on delay) |
| sched-latency | oracle; gross numbers above | **restate nothing**: the targets are gated at 2 harts (the owner's ruling) after the oracle work; a 16-seed sweep at 2 harts; 4 harts recorded |
| sched-latency-tcg | rv64/rv32: the N=16 server-share line missing (the guest exits first) | oracle; I'll find the lost line (two harts on the UART?) |
| sched-cluster | oracle, as above | **keep one-hart** (`keep_smp`). Its envelope calibrates the retained-debt cluster of one queue against a one-runner slice period. Across harts the rank of every pick is checked by the oracle in every other case. Or restate it, if you prefer |
| sched-share | rv64 FAIL (100s got 252 of counts, want 200), rv32 PASS | **restate**: water-filling. At 2 harts 300 is capped at one hart, so 500/250/250, judged from the trace's charges. Counts stay the 1-hart verdict and a 2-hart note |
| sched-large-weight | rv64 FAIL (server 437 vs 555), rv32 PASS | **restate**: the server is capped at one hart of 2, so 500 of 1000 and the users 62.5 each, from charges |
| sched-idle-gap | rv64 PASS, rv32 FAIL (B 271, want >= 283) | **restate**: from charges; the want, 333 (3 budgets, 2 harts, 2/3 hart each), is the same number |
| sched-sleep-gaming | FAIL both (victim 447 / 425, want >= 450) | **restate** from charges, then see Q4 |
| deadline-flood-billed (release) | FAIL both (victim 391 / 358, want >= 450) | see Q4. Then either a traced twin judged from charges, or keep one-hart |
| sched-share-release, sched-large-weight-release | FAIL (shares by counts) | **keep one-hart**: they measure the release build's slice-end cost by counts, with no trace; their checked twins carry the 2-hart shares |

**Why the shares are judged from charges, not counts, across harts.** Under icount a hart's rate
depends on what the other harts run. A hart that halts (lock wait, idle, a nap) gives its rate to
the busy ones, and kernel time on one hart moves rate between harts. So a budget's count against
the one-loop calibration measures its share of the machine's instructions, not of hart time.
Example, sched-share with the wfi wait: the 300 got 523 (rv64) and 569 (rv32) of the counts
against water-filling's 500. The kernel charges each hart's runner by `rdtime` (hart time), which
is what R12 shares. The oracle already judges charged shares (`check_charged_share`). The
post-check knows H from `case::Boot`, and the trace carries each record's hart. No ABI is needed.

**What 1 ms means at H harts under icount:** 125,000 instructions of the whole machine. A hart
running beside another busy hart executes about 62,500 of them. Kernel-time bounds measured in
virtual time are therefore dilated by the number of harts running user code. The measurement
cases above pass at 2 harts anyway, because their other hart mostly halts once the spin is cut.

## 3. R12 across harts

I'd state it as the earlier SMP2 brief's design does, §1 and §2: water-filling, and the floor
that leaves out capped budgets (the Architect's ruling). It needs one correction and one addition:
- **Correction.** The brief says each hart "runs at a rate that does not depend on what the
  others run, as on QEMU". That holds on QEMU without icount, but not under icount, which every
  gated case uses. The shares stay shares of hart time as the kernel charges it (each hart's
  runner, by that hart's clock). The pages say counts under icount are machine instructions, so
  they are judged from charges.
- **Addition (Q4).** Who pays for a hart's wait for the lock.

## Questions

- **Q1.** Is the wfi wait with an IPI at release the first cut, on every platform (recommended),
  or under `qemu-virt` only?
- **Q2.** Do the earlier brief's R12 text, the capped floor ruling and the four scenarios
  (late join, second cap, uncap, spread) still stand as SMP2's design, with the hart-rate
  correction above?
- **Q3.** May I add `keep_smp` to sched-cluster, sched-share-release and
  sched-large-weight-release, with the reasons above, and drop it from map-anon-search-bound and
  budget-deadline?
- **Q4 (needs the Architect).** Today a hart's wait for the lock is billed to its runner and
  counts against its slice. `irq.rs` takes the lock before `sched::from_user` reads the clock, so
  the wait is folded into the runner's user time. At 1 hart there is no wait. At H harts a victim
  waiting behind an attacker's long kernel sections pays for them. This is my hypothesis for
  deadline-flood-billed (391 / 358) and sleep-gaming (447 / 425); I have not checked it against
  a trace. My proposal: stamp the clock at trap entry before the lock. The wait is then nobody's,
  billed to no budget and not counted against the slice, as an audit is. It is bounded by R78's
  count times R12's per-call bound, and counted in the traced `nobody` share. The alternative is
  to bill the holder's payer.

## After the answers: the Q1 and Q4 prototypes

**Q1, a bounded spin before the wfi (300 `pause` turns).** ipc, 1000 lend round trips at 2 harts,
in µs.
- Without icount: each boot run alone on the quiet cores (`q run --quiet`), five runs each,
  mean (runs):
  - spin (main): rv64 505,446 (498k-525k); rv32 507,981 (498k-513k)
  - wfi: rv64 545,708, +8 % (522k-569k); rv32 562,874, +11 % (548k-578k)
  - spin 300 then wfi: rv64 537,906, +6 % (521k-563k); rv32 512,046, +1 % (504k-520k)
- Under icount: wfi 2,069,821 / 2,065,095, and spin-then-wfi 2,224,605 / 2,218,431 (+7.5 %).

So the bounded spin recovers the no-icount cost on rv32 but not on rv64. It also costs 7.5 %
under icount, which every gated case uses. Per the ruling, I take **plain wfi**. The figure for
the page is +8 % (rv64) and +11 % (rv32) on this ping-pong without icount, from one IPI per
contended release.

**Q4, the clock stamped before the lock (rv64, 2 harts).**
- Kernel features:
  - `proto-wait-unbilled`: the user time ends at the raw `rdtime` taken before the lock, so the
    wait is billed to nobody.
  - `proto-wait-slice`: the same, and the wait also moves the slice's end.
- Victim counts, of 1000:
  - deadline-flood-billed: billed 389 (390, 391 earlier), unbilled 389 (392), slice 392 (393).
    rv32 earlier: 358 / 359 / 377.
  - sched-sleep-gaming, near-slice phase: billed 415 (447), unbilled 384, slice 440. That is
    noise of +-30 between runs, with no direction.
- So the billing of the wait does not explain them.

**What does:** the wait itself. A per-hart counter of the time from a user trap to holding the
lock printed, for deadline-flood-billed, that one hart waited ~1000 ms and the other ~100 ms by
t ≈ 2.2 s, in a 2.0 s window. For sleep-gaming it was ~300 and ~100-400 ms. With one kernel
lock, the creator's kernel sections run on its own hart: deadline destructions, billed to its
parent. Every entry of the victim's hart, at least its 1 ms slice end, then waits behind them. At
one hart that kernel work only took the creator's turns. At H harts it also stalls the other
harts' entries, so it costs up to H harts' time. Moving the bill cannot give the victim back the
hart time it spent waiting. R78 bounds the wait by count, not by share.
