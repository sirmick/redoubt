# K28: no audit inside a destruction

Branch wp-K28, worktree /home/mcloonan/redoubt/.worktrees/K28, from main b60c7cc5c.

## The failure

RECON1 on main f4d3b41d9 (K19 merged): `FAIL sched-latency [rv64] sched_oracle: record 20081: an
audit inside a destruction`, the record `U 2` (AUDIT_PROCESS_INDEX) between a destruction's `X`
and `Y`; the same on sched-latency-tcg (both widths) and kernel-containment (both).

## The cause

K19's "a destruction delivers nothing until its end" (386ed86d0) pumps the endpoints a
destruction listed after `end_destruction` clears `objects.deferring` (a pump asserts it is clear)
and before the `Y` record. A notice delivered there frees its process object
(`message.rs` pump → `process::free_object` → `index_process(pid, None)`), and
`index_process`'s audit guard read `deferring`, so the index audit (and `message::check_all`)
ran inside the window.

## The fix

One debug-only flag, `Objects::destroying` (kernel/src/budget.rs), set in `begin_destruction`
and cleared by the new `MemoryManager::destroyed()` after `pump_listed`, before the `Y` record;
`index_process`'s guard reads it instead of `deferring`. `deferring` keeps its meaning (frame
frees defer, sweeps fold, no pump) and its span. The destruction still audits once after `Y`
(`audit_destruction`, or the expiry's). Release build: identical code (the field and both
stores are `cfg(debug_assertions)`). Kernel order, the oracle's rule (`sched_oracle`: an audit
inside R10 fails) and the model: unchanged.

Head: wp-K28 5e7c5170c (one commit on main b60c7cc5c; `tests/size-budget.toml` kernel ceiling
9360 → 9377 with its `Size budget:` line, the fix's 17 lines).

## Gates

All through jobs.mk / `q run --tenant K28`; logs in the worktree's target/jobs/.

Kernel source identical since the first prebuilt (only size-budget.toml changed after):
- rv64/sched-latency PASS 62.3 s, rv32/sched-latency PASS 56.2 s
- rv64/sched-latency-tcg PASS 91.5 s, rv32/sched-latency-tcg PASS 88.5 s
- rv64/kernel-containment PASS 416.4 s (R10 p50/p99/max 28715/29500/29500 µs, 20 destructions)
- rv32/kernel-containment FAIL 416.8 s: `sched_oracle: R10's p99 is 30576 µs over 20
  destructions, above 30000`; rerun alone on `--quiet`: FAIL, the same 30,576 µs (icount:
  deterministic). The finding below; K29.
- formatting PASS 18.5 s
- (kernel-containment's `qemu_seed = 13` is its only seed: "seed 13 and the default" are one run)

On the final tree 5e7c5170c:
- docs PASS, no-cruft PASS, unsafe-budget PASS, size-budget PASS (after the raise; FAIL at
  9377/9360 before it), build-rv64 rc 0, build-rv32 rc 0
- redoubt-kernel has no host tests (test = false)
- the 21 K19 cases + budget-reap, both widths (52 runs): all PASS (target/jobs/k28-gates.txt):
  endpoint-destroy-full, endpoint-destroy-open-calls, budget-destroy-kills, ending-pumps-once,
  destroy-keeps-notices, destroy-keeps-notices-creator, process-lifecycle, redoubt-dead,
  sched-destroy-billing, pid-pinning-attack, handle-chain-attack, handle-chain-fault,
  process-chain-fault, budget-deadline, timeouts, userland-boot, init-boot, bench-net-peer,
  ipc-outcomes, budget, budget-destroy-attack, budget-destroy-growth, deadline-flood-billed,
  redoubt-revoke, process-attack, budget-reap
- rv64/model-host-tests PASS 135.2 s; rv64/model-mutations PASS 726.5 s (fanned)

## Finding: R10's p99 grew ~6 ms with K19 (filed as K29)

kernel-containment's R10 p99 over 20 destructions (~9,338 object frames):
- train-8 on main bdb38430e (pre-K19): rv64 23,394 / rv32 24,104 µs, threads' ending 3.5 ms
- K19 + this fix: rv64 29,500 / rv32 30,576 µs, threads' ending 2.5 ms

One rv64 run with walk-trace added to the case's kernel_features (local edit, reverted, never
committed), the 29.5 ms window:
- 0.11-4.46 ms: the kills (two T/t spans)
- 4.46-28.05 ms: the walk proper (budgets_dying's frees over the charged chains, endpoint and
  device destruction, lift_dying, destroy_marked), no stamps inside: 23.6 ms
- 28.05-28.71 and 28.74-29.48 ms: the two end pumps (`M 1`/`m 1`): 0.66 + 0.74 = 1.4 ms
- Y at 29.50 ms
So the pumps are 1.4 ms of the growth and ~3.5-4.5 ms is in the walk itself (K19's unchain,
two list removes per freed process object, and the chain walks). The analysis script is
/tmp/k28-r10.py (reads the console log, pairs X..Y and M..m).

## Summaries checked

- docs/kernel/budgets.md, R10's checked-build paragraph ("run once off the destruction walk,
  never scaling it"): true of the code; no change.
- docs/kernel/scheduling.md, "Targets exclude the checked build's audits" ("full audit scans
  after each destruction and at each process-object free"): true outside a destruction, which
  is what it describes; no change.
- docs/testbench.md, the oracle's rule ("an audit that ... runs inside a destruction fails the
  check"): unchanged, the kernel now meets it again.
- docs/kernel/invariants.md:124, README.md, GETTING-STARTED.md, docs/kernel/README.md: no claim
  touched by a debug-only audit guard.
