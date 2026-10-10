# BEAM18 report: a session's VM runs its schedulers on several harts

Branch `wp-BEAM18`, worktree `.worktrees/BEAM18`, base main `98b866854` (rebased from 050357e8a),
head `a528fede7` (gates on `86f1a102d`; the head differs only in one sentence of beamlet.md's measured range, docs re-run rc 0). Design: `.wash/local/BEAM18-design.md` (go with option (a), 2026-10-09).

## Commits

1. `4114c4037` rt: a semaphore, mutex and condvar whose waiters sleep on an endpoint, and scoped
   threads (`libs/rt/src/sync.rs`, `thread.rs`; tests `libs/rt/tests/sync.rs`; rt-miri; native.md)
   Unsafe budget 11 -> 15, size budget libs/rt 3629 -> 3861 (lines in the commit).
2. `4de7e5272` beamlet: a helper going offline hands the timers to a sleeper, and a yielding
   process keeps its scheduler (`vm/src/sched.rs`, `vm.rs`; `vm/tests/schedulers.rs` + fixture)
3. `08ba089ee` beamlet: on Redoubt the VM runs schedulers=N scheduler threads (VM feature
   `redoubt`, `sync.rs` backend, `Vm::run` via `rt::thread::scope`; platform `schedulers=`,
   I/O report; `redoubt/tests/schedulers.rs`)
4. `a9970537c` steward: a session's VM runs two schedulers, and a session is 11,264 pages
   (steward.rs; image/manifest.json and the three test manifests' sizes; footprint manifest;
   driver_test.exs; budgets.md). Size budget servers/steward 1493 -> 1499.
5. `a528fede7` tests, docs: the two mttcg cases, jobs.mk quiet list, reduction-rate keep_smp,
   beamlet.md, steward.md, sessions.md, testbench.md, m2-usable-shell.md

## Design as built

- N: `schedulers=N` (absent/malformed 1, cap 8), the steward passes 2; no kernel change.
- One shared run queue under the System lock (the VM's existing std design); per-process heaps,
  per-scheduler caches; platform locked after System; only a scheduler with nothing running
  anywhere idles in the platform.
- rt locks without futex/TLS: a semaphore on a private endpoint (benaphore: count below zero ->
  receive; release from below zero -> one send); Mutex = spin 64 then handoff on its semaphore;
  Condvar = a semaphore per waiter (queued, pooled), so a later waiter cannot steal an earlier
  one's wake-up (the first design, one shared semaphore, deadlocked in the ping-pong test).
- Two VM bugs found and fixed (std build too): (a) a helper going offline while the other
  scheduler slept left nobody to fire timers -> hang (host std 2/5 runs; mttcg case hung);
  (b) every lock release with a non-empty queue woke a sleeper, so a lone yielding process
  changed threads every slice (rv64 rate 43,511, then 46,081 after a first partial fix; the
  remaining wake came from the main scheduler's result check between finish and next).

## Measurements

- beamlet-schedulers-mttcg (2 harts, mttcg, host time, ratio = fastest one-online / fastest two):
  alone rv64 1.77, 1.85, 1.90, 1.88, 2.14, 1.83, 1.86; rv32 1.89, 1.94, 1.90, 1.88, 1.83.
  Beside the whole set (no verdict by the case's rule): rv32 1.53 once, rerun alone 4/4 PASS.
  Floor 1.55 (lowest alone 1.77 less a tenth, rounded down to 0.05).
- beamlet-schedulers-one-hart-mttcg: 0.99 both widths (floor 0.90).
- beamlet-reduction-rate (icount, 1 hart, schedulers=2): rv64 52,936, rv32 46,959; main
  98b866854 rv64 52,602 (BEAM19's page 52,647 / 47,052). With the helper parked: 52,727.
  Before the wake fixes: 43,511 (bounce every slice), 46,081 (one wake per slice left).
- At --smp 2 (icount): main rv64 45,354 / rv32 37,157 (FAIL, under 42,000); branch 45,778 /
  36,967. So the case now keeps its 1 hart (keep_smp) and the page says why.
- beamlet-footprint at schedulers=2: rv64 peak 5,490-5,491 pages (unchanged from main), cap now
  11,092 (slack 110 pages; share moves again above 5,546); passes both widths at 1 and 2 harts.
- Module loads under the System lock (probe build, to the first prompt): 116 loads, rv64 8.23 s
  total, max 877 ms (unicode_util); rv32 8.94 s, max 969 ms.

## Gates (head 86f1a102d, logs .tmp/BEAM18/logs2, logs3)

- `q run --cores 8 -- cargo test` in userland/otp: rc 0 (every suite ok).
- `cargo test -p beamlet-redoubt --features fake`: rc 0 (76 pass, the new schedulers suite in).
- `cargo test -p beamlet-vm --features std --test schedulers`: pass; without the offline fix it
  fails at its watchdog (checked).
- `./test-shell`: rc 1 in the full run (beamlet-redoubt system
  servers_bound_and_ended_one_after_another_never_run_out_of_waiters), rc 0 rerun. That test
  fails 2/30 on main 98b866854 as on the branch (2/30): a pre-existing flake, its first handle
  sample is taken without waiting for the ended waiter to close its handle. Not changed here.
- tools/difftest (q, 8 cores): 527/527 passed, 21 skipped by design, rc 0.
- docs rc 0; prebuilt rc 0 (281 rv64 / 267 rv32 cases built).
- `make set` of 73 cases on both widths (the shell set from scripts/shell-cases, every steward-,
  sshd-, userland-, pipe-, boot-profile- and beamlet- case, rt-host-tests, rt-miri,
  size-budget, unsafe-budget, formatting, no-cruft): 124 PASS, 1 FAIL (rv32
  beamlet-schedulers-mttcg 1.53 beside the bench; 4/4 PASS alone).
- At --smp 2: userland-boot and beamlet-footprint rv64 and rv32 PASS.
- rt: `cargo test -p redoubt-rt --test sync` 7 pass (30 repeats clean); under Miri 5 pass, 2
  scope tests ignored (thread_create's closure crosses the fake as an integer, as thread.rs).
- `cargo build` beamlet (riscv64gc, riscv32imac) and steward server (both): no warnings.
- Not run: the whole bench (Tier A's full bench is the train's).

## Affected summaries checked

- README.md, GETTING-STARTED.md: no scheduler or session-size claims; no change.
- docs/kernel/README.md "Several harts, one lock": still true (kernel); no change.
- docs/TENETS.md "Harts": says a beamlet VM's schedulers run in parallel once threads may run on
  several harts: now true; its "budget on one hart at a time" clause is SMP3's to retire (left).
- userland/otp/README.md: no scheduler claim; no change.
- docs/userland/beamlet.md, docs/servers/steward.md, docs/userland/sessions.md,
  docs/kernel/budgets.md, docs/testbench.md, docs/userland/native.md,
  docs/plan/m2-usable-shell.md (Progress): updated.
- testbench.md's case counts (226 boot / 145 icount) were already stale on main (266 boot);
  not changed.

## Open / residual

- Module loads hold the System lock: to the first prompt 116 loads, 8.2 s guest time rv64
  (8.9 s rv32), longest unicode_util 877 ms (969 ms) (probe build, not committed).
- rt heap: one spin lock; a holder preempted by the kernel makes the other scheduler spin.
- beamlet-reduction-rate is not judged at 2 harts under icount (main fails rv32 too).
- CTX3 (wp-CTX3) derives per-principal context caps from the session size: rebase onto whichever
  merges first (orchestrator informed it).
