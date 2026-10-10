# SMP3 report: one process's threads on several harts at once

Branch `wp-SMP3`, worktree `.worktrees/SMP3`, base main `d5f1af40b`, head `44fa4d2cf`:

1. `246366e81` testbench: `--smp N` boots every case at N harts, but a case that keeps its own counts
   (`keep_smp`, `timeout_secs_smp`; budget-deadline keeps its counts; redoubt-ipc-attack 90 s at `--smp`)
2. `d1abd3021` testbench: the scheduling trace names each record's hart, and `smp_fence` reads its shootdowns
3. `2622fc23a` kernel, stride: one process's threads run on several harts at once
4. `44fa4d2cf` tests, docs: `smp-shootdown`, `smp-fence`, the pages

## What was delivered

- **The pick** (`libs/stride`, `kernel/src/sched.rs`): the lowest-ranked budget with a runnable thread
  no hart runs; a budget whose runnable threads all run on harts is passed over and stays queued.
  RECON1's ranks untouched; the pick compares them alone. Stride state one per budget: each hart's
  runner charges its own run to the one pass; a settle folds every runner; a hart leaving a budget
  another hart runs requeues it (`back + 1`). Wake IPI also for a ready thread of a running budget.
- **The process state** (`kernel/src/ptable.rs`): `Running` while any hart runs a thread; the ready
  mask never holds a thread a hart runs; the hart blocks are the record of each hart's thread
  (`off_hart`, the merged Ready/Running branches). Checked build: `audit_harts` at every pick (blocks
  against the table and runners), the reverse in `audit_marks` once a slice.
- **Shootdowns** (`arch/riscv/hart.rs`, `mem.rs`, `message.rs`, `process.rs`): three kinds, `Flush`,
  `Fetch` (fence for new code), `Leave` (an ending: the hart leaves the space). Sites:
  unmap (mem.rs `unmap`); narrowing set_flags (`set_flags`); executable install (`map_anon`,
  `map_fixed`, `set_flags`, `process_map`'s child via `sync_if_executable`); process_map's source;
  `take_buffer` (lend/transfer sent); `return_lend`; `free_abandoned_lend` (not in the brief);
  transfer delivery's freed tables in `move_buffer` (not in the brief: a freed table under a cached
  pointer); `abandon`'s freed tables (same); `undo_run` (not in the brief: a page another hart may
  have touched); `terminate_process` with a sibling elsewhere (not in the brief; `Leave`);
  `kill_process` (SMP1's, now `Leave` via `evict`). No flush: `drop_lent`, `lend_back`
  (entries already invalid / additions), `map_range` (boot only, kernel). Checked build: every
  removal is recorded and the return stops if the process runs on another hart unshot.
- **Trace**: `S` record (target, why, asked, acked) and a hart field on every record.
- **Cases**: `smp-shootdown` (unmap, lend returned, lend within one process; icount and mttcg),
  `smp-fence` (`post_check = "smp_fence"`). Recorded negative `smp-no-shootdown`: all six runs
  (both widths, icount and mttcg, fence) fail at the checked audit; with the audit silenced
  `smp_fence` fails it alone on both widths, and QEMU without icount showed one stale write
  (rv64, a lend within one process). Each verdict is the kernel's: exit notices (cause, code 15),
  the checked audit, the trace's acknowledgement record.
- **Pages**: scheduling.md (pick, cursor, floor, charging, status lists, the cross-hart call
  residual with its numbers), memory.md (fence paragraph and status, lending status, several-harts
  residual, lend-within-one-process residual deleted, freed-frame residual), memory-layout.md
  (flush residual), kernel/README.md (several harts), m2-usable-shell.md (step 4, Progress),
  testbench.md (`--smp N`, `keep_smp`, `timeout_secs_smp`, `smp_fence`, the mttcg list).
- **Deleted**: the one-hart `Running` branch of `switch_to_thread`, `leave_previous`'s and
  `destroy_thread`'s special cases, the WARNING print; still a net add (below).

## Gates on 44fa4d2cf (alone on the machine's q leases)

`make -k -f scripts/jobs.mk -C .worktrees/SMP3 <82 targets>`: exit 0, 82 PASS: kernel-containment,
userland-boot, init-boot, ipc-outcomes, bench-net-peer, sum-clear, lend-untouched-page, worst-walk,
all 21 sched-*, smp-boot, smp-evict, smp-evict-mttcg, smp-fence, smp-shootdown,
smp-shootdown-mttcg, each rv64 and rv32; model-host-tests, model-mutations, stride-host-tests,
docs, formatting, size-budget, unsafe-budget, no-cruft. `./build --arch rv64|rv32`: exit 0, no
warnings. `cargo test -p testbench`: 157 pass. `cargo test -p redoubt-stride`: 22 + 2 pass,
`the_crate_and_the_model_agree` included.

- Size budget: kernel 9460 -> 9599, libs/stride 694 -> 699 (reasons in commit 3).
- Unsafe: no new site in the kernel (`unsafe-budget` PASS); `smp-fence`'s test program has one
  (a call into the page it made), with a SAFETY comment.
- unmap's cost at one hart (`asid-cost`, 10,000 maps+touches+unmaps, release, icount):
  rv64 2,812,305 -> 2,813,907 µs (+0.06 %), rv32 3,217,688 -> 3,221,678 µs (+0.12 %).
- Model: untouched; it has no harts (scheduling.md: "the model has one hart"), so nothing to
  violate. Two stride host tests changed with the pick (one-runner test replaced; one added).

## The whole bench at `--smp 2` (head 44fa4d2cf)

`q run --cores 8 -- target/prebuilt/testbench --prebuilt target/prebuilt --arch <w> --smp 2`:
rv64 258 PASS / 26 FAIL; rv32 247 PASS / 23 FAIL. Every failure classified against main:

- **SMP2's starting list (R12 shares and the oracle, one-hart rules; fail on main too):**
  deadline-flood-billed, endpoint-destroy-full, sched-budget-churn, sched-carve-return,
  sched-cluster, sched-debt-lift, sched-destroy-billing, sched-exit-churn, sched-idle-gap,
  sched-large-weight, sched-large-weight-release, sched-latency, sched-latency-tcg,
  sched-lift-delay, sched-server-busy (rv64; flips at its tolerance), sched-share,
  sched-share-release, sched-sleep-gaming, sched-ties, sched-timer-flood, sched-wake-no-preempt.
- **Measurements disturbed by the second hart (fail on main too):** expiry-deadline-then-timeout
  (rv64), scan-bounds.
- **Fail on main + the option at 2 harts too (246366e81, rv64), so not SMP3's:**
  boot-profile and boot-profile-unverified (first console read at 51 s on main, 92 s here,
  against a 20 s bound; both show two logins, the first session replaced: worth a node);
  kernel-containment (no progress to its 1800 s timeout after a run of terminations, on main
  too: a stall worth a node).
- Made to hold at two harts here: receive-bad-record, timeouts, timeouts-tcg. `keep_smp`:
  bench-poweroff-missing, map-anon-search-bound, budget-deadline (reasons in their tomls).
  B27 fixed init's six fresh-connection refusals; K30 explains budget-deadline and
  redoubt-ipc-attack.

## Findings

- The brief's caller table missed five sites (above, "not in the brief").
- docs/plan/m2-usable-shell.md says this package also covers step 4's steps 5 and 6 (lock to
  decide, magazines); the plan node does not. Not changed.
- TENETS "Harts" still reads "with a budget on one hart at a time until one process's threads may
  run on several": left as the brief says.
- A call within one budget crosses harts at every reply (IPI + lock hand-off): free without icount
  (ipc 2.5 s both), 8.2 s vs 1.3 s under icount. On the page; wake affinity is a later package.

## Documentation check

Checked: README.md, GETTING-STARTED.md (no hart claims; unchanged), docs/kernel/README.md
(several harts: updated), docs/plan/m2-usable-shell.md (step 4, Progress: updated),
docs/kernel/scheduling.md, memory.md, memory-layout.md (updated), docs/testbench.md (updated),
docs/TENETS.md (left, per the brief), libs/stride crate docs (updated).

## Reading

Every hunk committed read in full, and the functions around it; not every line of message.rs or
sched_oracle.rs.
