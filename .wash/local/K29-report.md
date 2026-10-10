# K29: R10's p99 grew ~6 ms with K19 (kernel-containment)

Branch wp-K29, worktree /home/mcloonan/redoubt/.worktrees/K29. Head 4333bc71c: ONE commit on
main cc51f76ad (on B24, RECON1 and K28; the gate below ran on 2599e0d5b, the same kernel on
56a77fcf3: cc51f76ad adds no kernel change), "kernel: a destruction takes its own endpoints
off the to-pump list in one walk of the list". Files: kernel/src/message.rs,
docs/kernel/budgets.md, tests/size-budget.toml (kernel ceiling main's 9455 -> 9460, measured:
9460 of 9460; Size budget line in the message). Rebased three times (K28, RECON1, B24); each
conflict was only the kernel ceiling; RECON1 and B24 touch neither message.rs nor budgets.md. Never
pushed. Every committed file was read in full by the committing member (message.rs whole, by the
k29-implementer, before the final amend). The measurement worktree .worktrees/K29-base is removed.

## The cause

K19's step-4 hunk in `message::budgets_dying`'s owner walk asked, for every endpoint the dying
subtree owns, `List::pumps().contains(w, frame_word(o))` (and removed it when listed), so that
"a dying one leaves [the to-pump list] as the owner walk meets it". `contains` is O(1) but it is
one or two checked kframe reads through `object_phys` per endpoint, and the containment gate's
lease owns thousands: step 4 took 16.06 ms where it took 9.45 ms before the list existed (~1.6 µs
each over ~4,100 endpoints = 6.6 ms). The pumps themselves moved from the kills to the end (1.6 ms
either way); nothing else grew. Phase split (rv64, longest destruction, stamps at
`destroy_subtree`'s phase boundaries, ms; the stamps cost ~0.5 ms of the whole):

| phase | pre-K19 (bdb38430e) | K19 + K28 (5e7c5170c) | K29 |
| --- | --- | --- | --- |
| kills (step 2) | 8.04 (three pumps inside, 1.6) | 6.29 | 6.29 |
| process::budgets_dying (step 3) | 0.29 | 0.13 | 0.13 |
| message::budgets_dying (step 4) | 9.45 | 16.06 | 9.49 |
| lift_dying | 0.44 | 0.44 | 0.44 |
| destroy_marked | 5.51 | 5.45 | 5.45 |
| end pumps (pump_listed) | — | 1.55 | 1.55 |
| whole | 23.9 | 30.0 | 23.4 |

Dismissed: `unchain`'s `assert!(contains && contains)` (O(1) link reads); "0.7 ms per pump" (the
same per-pump cost pre-K19). With walk-trace on, kernel-containment overflows its 192 MiB trace
ring, so a measurement run's verdict means nothing by construction; every verdict below is from a
run without stamps or walk-trace. Measurement procedure and traps:
.wash/local/handoffs/k28-implementer.md.

## The change

The per-endpoint check is gone from the owner walk. At the end of `message::budgets_dying`, after
the dying budgets' queued and taken chains are failed (the last point anything can list an
endpoint: `fail_wait` -> `pump_endpoint` lists while deferring), one walk of `List::pumps()`
removes each entry whose owner budget is dying (`frame_of`, `mm.endpoint(frame).owner`,
`mm.budget(..).dying`): a read per listed endpoint (a handful), never one per endpoint the subtree
owns. `destroy_marked`'s `free_owned_endpoints` frees their frames after; `pump_listed`'s debug
assert (every listed owner live and not dying) is unchanged and checks it. K19's rule (deliveries
at the boundary, none inside) and the model (model/src/kernel.rs:2258 says only that the list
drains at the end) are unchanged. Design approved by the orchestrator before the commit.

## Gates

All on 2599e0d5b, through jobs.mk / `q run --tenant K29`, from target/prebuilt of that tree
(prebuilt rc 0). Logs in the worktree's target/jobs/: make logs rv{64,32}-<case>.log, chain
summaries k29-gates-*.txt, k29-static.txt. Earlier full runs on cc5872a42 (on K28) and
4de07ad1d (on RECON1) were superseded by the rebases; the cc5872a42 numbers agree within 40 µs.

R10 over the gate's 20 destructions (qemu_seed 13, icount, deterministic):

| kernel | rv64 p50/p99 | rv32 p50/p99 | threads' ending p99 rv64 / rv32 |
| --- | --- | --- | --- |
| main bdb38430e, pre-K19 (train-8) | — / 23,394 | — / 24,104 | 3.5 ms |
| K19 + K28 (5e7c5170c) | 28,715 / 29,500 | — / 30,576 FAIL | 2.5 ms |
| K29 (2599e0d5b) | 22,139 / 22,895 | 22,873 / 23,509 | 2,563 / 2,760 µs |

- rv64/kernel-containment PASS 400.9 s, rv32 PASS 408.4 s ("no audit inside one" both)
- rv64/worst-walk PASS 546.5 s: R10 p50/p99 16,348/16,353 µs, threads' ending 13,073; walks
  net of audits: pump max 1,024, expiry max 26,836, reconcile max 7,184 µs (bound 8,900)
- rv32/worst-walk PASS 477.4 s: R10 17,350/17,375 µs, threads' ending 13,763; pump max 937,
  expiry max 27,642, reconcile max 7,863 µs (bound 8,900)
- rv64/sched-latency PASS 66.6 s, rv32 PASS 46.9 s; rv64/sched-latency-tcg PASS 92.4 s, rv32
  PASS 89.2 s
- the 21 K19 cases + budget-reap, both widths, 52 runs, all PASS rc 0: endpoint-destroy-full,
  endpoint-destroy-open-calls, budget-destroy-kills, ending-pumps-once, destroy-keeps-notices,
  destroy-keeps-notices-creator, process-lifecycle, redoubt-dead, sched-destroy-billing,
  pid-pinning-attack, handle-chain-attack, handle-chain-fault, process-chain-fault,
  budget-deadline, timeouts, userland-boot, init-boot, bench-net-peer, ipc-outcomes, budget,
  budget-destroy-attack, budget-destroy-growth, deadline-flood-billed, redoubt-revoke,
  process-attack, budget-reap
- build-rv64 rc 0, build-rv32 rc 0
- rv64/model-host-tests PASS 46.3 s; rv64/model-mutations PASS 17.9 s (fanned)
- redoubt-kernel has no host tests (test = false)
- formatting PASS, docs PASS, size-budget PASS (kernel 9460 of 9460), unsafe-budget PASS,
  no-cruft PASS
- Not run: the 16-seed containment sweep and the whole bench (the train's).

## Summaries checked

- docs/kernel/budgets.md, Residual risks item 4 (step 4): rewritten to follow the code ("when
  nothing more can be listed, one walk of that list takes the subtree's own endpoints off it (a
  read per listed endpoint, never a read per endpoint the subtree owns)"); the gate paragraph
  takes the gate's numbers (R10 p50/p99 22.9/23.5 ms rv32, 22.1/22.9 ms rv64, 20 destructions);
  the item's last lines rewrapped (the WIP's hunk had left a 124-character line).
- kernel/src/message.rs: `budgets_dying`'s and `pump_listed`'s comments follow the code.
- docs/kernel/scheduling.md:576 "R10's p99 is 26,379 µs (rv64) and 27,407 µs (rv32) on every
  seed": the 16-seed sched-latency sweep's record, not this gate's; refreshed only by a sweep,
  which the package does not run. No change.
- docs/kernel/README.md, the containment table (16-seed sweep; seed 13 R10 22,517 rv32 /
  22,380 rv64): a sweep record from before train-8 (which measured 24,104/23,394 on that seed);
  one run does not refresh a sweep. No change.
- model/src/kernel.rs:2258 (the to-pump list drains at the end of each destruction): still what
  the kernel does. No change.
- docs/testbench.md (the oracle's rule, r10_p99_us=30000), docs/kernel/invariants.md, README.md,
  GETTING-STARTED.md: no claim touched.

## Open risks

- Folded (orchestrator's instruction): budgets.md's worst-walk sentence takes the figures
  measured on 2599e0d5b, 16.35/16.35 ms rv64 and 17.35/17.38 ms rv32, the rest about 3.3 and
  3.6 ms; docs PASS and size-budget PASS on 4333bc71c.
- budgets.md's older sentence "pumping after each thread instead, the same kernel measures 24.5
  and 21.2 ms on rv32" is K13's comparison and stays as history.
- The to-pump list is walked once per destruction, whatever its length: the survivors' endpoints a
  kill or a failed caller touched (a handful at the gate's fill), bounded by the endpoints that
  exist.
