# LAT4 report (irq1-implementer)

Branch `wp-LAT4` (worktree .worktrees/LAT4) on main a5e387664: one commit, ec361337b (the
kernel red's second round folded into fe0a7d6f3, which folded its first round into 063ad50d7)
"testbench: the 4-hart driver wake is judged where the harts run at once, in searches of its own run".
Tier B (bench, test program, pages; no kernel change). Design: .wash/local/LAT4-design.md.

## Delivered
- tests/programs/src/sched.rs: `lock_contention` times 21 of the hammer's refused searches alone,
  before any child starts, and prints `one search alone: p50 N µs, max M µs (21 searches)`; the
  area fill is shared with the hammer and gives back only pages it mapped.
- tests/sched-lock-contention-4-mttcg.toml: the 4-hart program under multi-threaded TCG (no
  icount), checked, lock-trace; post-check `driver_wake_p50_us=15000 driver_wake_p99_us=50000
  driver_wake_p50_searches=3 driver_wake_p99_searches=8`: all four gated.
- scripts/jobs.mk: the quiet class gains sched-lock-contention-4-mttcg, smp-evict-mttcg and
  smp-shootdown-mttcg, so 'a verdict only alone' is enforced (q --quiet: 4 cores for 4 vCPUs and
  QEMU's main thread; the toml says to look there first if it flakes).
- tools/testbench/src/sched_oracle.rs: `driver_wake_p50_searches=K` and `driver_wake_p99_searches=K`,
  gated at any hart count, from the `one search alone` line; host test
  `the_driver_wake_is_judged_in_searches_of_its_own_run` (met at 7.0, missed at 7.01, gate_harts
  never ungates it, the p50 bound, a missing line is an error).
- Pages: scheduling.md R78 (the MTTCG judge, why a ratio, what 50 ms means on hardware: four
  searches, so it holds while R12's costliest call stays under ~12 ms), Responsiveness and R78
  statuses, residuals (QEMU's turns now point to the MTTCG judge; the herd its own residual with
  numbers); testbench.md (MTTCG host-time list, verdict-only-alone list, oracle prose, status);
  SECURITY.md R78 row; sched-lock-contention-4.toml comment.

## Measurements (alone, q --cores 6 pinned)
20 runs: p99 / search-alone p50 = rv64 3.8-6.2 (mean 4.6), rv32 3.2-5.1 (mean 4.1); search alone
1.9 / 1.3 ms; net p99 rv64 7.3-11.9 ms, rv32 4.2-6.8 ms. Gate runs: 3.7-5.1, then 1.8 and 2.1.
K = 8 = worst (6.2) and a margin. Raw: .worktrees/LAT4/.tmp/rep/.
Herd (Part B decision): 518-577 empty claims a run (every alarm traps the other three); an empty
claim holds the lock ~31 µs p50, 50 µs p90 (upper bound from the trace, rv64 run 11); at most three
ahead of a wake = ~0.15 ms of a 4-12 ms p99 (<= ~3%). Part B not done; residual on the page.

## Quiet-core and twin runs (kernel red rounds)
20 runs on q's quiet cores: p50 1.6 to 1.9 searches, p99 2.0 to 3.0 but one run a width at 6.1
(rv64) and 5.1 (rv32); raw .worktrees/LAT4/.tmp/repq/. The irq-boot-hart-only kernel under the
same case (temporary toml, removed): 3 runs x 2 widths on the quiet cores, p99 5.4 to 7.1 searches
and 7.5 to 10.9 ms (passes the p99 ratio and the absolute 50 ms), p50 3.7 to 4.9 searches: fails
`driver_wake_p50_searches=3` in all four runs with that bound. Hence the p50 bound; a tighter p99 K
would have failed IRQ1's own outliers. Not a must_fail case: under MTTCG the twin's miss depends on
where its hammer lands. After the folds: the case x2 per width on the quiet cores PASS (p50
1.6-1.8, p99 2.0-2.9 searches), docs, formatting, no-cruft, host-tests PASS.

## Gate (both widths, exit 0 each): build rv64+rv32, formatting, no-cruft, docs, size-budget,
unsafe-budget, host-tests, sched-lock-contention, sched-lock-contention-4, irq-boot-hart-only
(still fails as it must), sched-lock-contention-4-mttcg. Not run: smoke set (no kernel change;
the program change reaches only lock_contention's callers, all run), whole bench.

## Documentation check
Checked README.md, GETTING-STARTED.md (nothing on the 4-hart wake), docs/kernel/README.md (no
latency claim), plan m2 several-harts (no LAT4 text needed). Flag, not changed: testbench.md's
"145 of the 226" boot cases in guest time and "68" without icount are stale on main (241 boot,
151 sleep=off, 87 without icount before this case).

## Notes
- No negative twin for the ratio bound: the oracle bound has its host test; a kernel that breaks
  R78 (test-and-set) trips the checked build's FIFO assert first.
- The 21 searches run before the window in the icount cases too; they still pass, the twin still
  fails as required (its hammer still lands on the boot hart).
