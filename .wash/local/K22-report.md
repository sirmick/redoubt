# K22 report

Branch wp-k22: af95ab385 (kernel, stride: reconcile visits only the budgets that changed) and
2b4dd2e29 (docs: reconcile no longer walks every process), on 53bcd9704. Clean.

## Delivered

- `kernel/src/ptable.rs`: every `ProcessState` change goes through `Process::set_state`, which
  calls `sched::mark(pid)` when `ready_count()` changes. `ready_count` is 1 for Setup and the
  mask's size for Ready/Running, the same predicate `fill` used.
- `libs/stride/src/marks.rs` (new): `Marks<B, N>`, the `Ready` trait and `Missed`.
  - `settle` moves each marked process's count to its budget and lists the budgets that lost and
    gained.
  - `check_visited` is the per-reconcile check.
  - `audit_due` decides when the full walk runs: at most once a slice, plus at the idle pick.
  - `audit` is the full walk over live PIDs.
- `libs/stride/src/lib.rs`: `Queue::reconcile(bs, running, lost, gained, runnable)`.
  - Leaves over `lost`, in one pass over the slots.
  - Wakes over `gained`, sorted once by descending id, using the `queued` flag with no search
    (`push`).
- `kernel/src/sched.rs`: `Runnable` and `fill` are gone.
  - `settle` runs before the deschedule's `ready(b) > 0` and before reconcile.
  - `check_visited` is called as a debug panic at every reconcile, unstamped.
  - The full walk (`AUDIT_MARKS = 4`, stamped) is called from `leave` and `pick`, as `audit_due`
    rules.
- `kernel/src/budget.rs`: `READY_WORD = 104`, the budget's ready count.
- The differential drives the crate with only the budgets whose threads changed.
- Each `next_thread` walk of the budget's processes stays, bounded by `MAX_PROCESS_COUNT` (R12's
  constants clause).

## Departures, accepted by the orchestrator, the Architect ruling

Per-process marks settled to budgets; the marks in redoubt-stride; the budget scratch word; the
size ceilings, kernel 8115 -> 8162 and libs/stride 332 -> 522, with the split in the commit;
.bss +22.5 KB on rv32 and +24.5 KB on rv64, with the breakdown in the commit.

## Leave order

Before: queue slot order. After: the order of `lost`. Nothing reads it: the oracle's D is a map
removal, the model and the differential compare states and picks, and in both versions the leaves
come before the floor raise and the wakes. Wakes keep descending id.

## Point 4 (a missed mark never runs a wrong thread)

`kernel/src/sched.rs:429`: `let tids = p.ready_threads().unwrap_or(TidMask::of(0));`.
`next_thread` takes only ready threads; tid 0 is only a process being set up. A queued budget with
none returns None, and `Cpu::pick` takes it out and picks again.

## Measurements

worst-walk is a checked build at seed 3, with times net of audits from the trace's M/m records.
The script is `/tmp/k22-walks.py`.

**rv64** (`cargo testbench --arch rv64 worst-walk`): PASS in 1366 s; the must_fail on R10 matched.
- 129,796 threads.
- "250 holders waited for one deadline: true".
- SCHED-TRACE-END 1,801,408 records, dropped 0.
- Reconcile, 540,831 of them: p50 35, p99 62, max **6,912 µs**.
- The max is the deadline's reconcile: it woke 233 budgets in one entry, against K16's 0.32 s.
  The other 17 holders' budgets were presumably already queued.
- Full walks: 13,336.

**rv32**: the local, uncommitted override `arch = ["rv32"]`, `memory_mib = 2032` (Sv32's physmap
limit), since reverted. PASS in 1401 s.
- 129,796 threads, "waited for one deadline: true".
- 1,821,095 records, dropped 0.
- Reconcile, 544,119 of them: p50 43, p99 87, max 338 µs.
- Full walks: 15,463.
- Caveat: on rv32 every wake came in its own kernel entry, and the expiry's max is 374 µs (rv64:
  26.2 s). So the rv32 run never reconciles 250 wakes at once, and its max is not a 250-waker
  figure. Waits ending one per interrupt is the timer expiry path (time.rs, IPC3's), not the
  reconcile. Main has never run this case on rv32, so there is no baseline.

**Gate** (`cargo testbench --arch rv64 kernel-containment`): PASS in 224 s. Every target is met.

| Figure | Value | Target | K16 |
|---|---|---|---|
| Share | 829/1000 | floor 783 | 829 |
| R10 p50/p99 | 20,746/25,171 µs | | 25,253 µs p99 |
| Lease end | 32,884 µs | <= 125,000 µs | 32,961 µs |
| Driver wake p50/p99 | 8,672/10,913 µs | | |
| Timer wake p50/p99 | 7,759/8,832 µs | | |
| Decision wake p50/p99 | 7,695/7,713 µs | | |
| Deadline notice p99 | 28,716 µs | <= 40,000 µs | |

The trace kept 562,417 records and dropped 0. There were 44,467 full walks; most are idle-pick
walks, which are not rate-limited.

**sched-\*** (`cargo testbench sched-`): 32/32 PASS on both widths, exit 0.

## Host gates (all exit 0 on af95ab385)

- stride-host-tests: 16 unit tests plus the differential. Includes a_missed_mark_trips_the_audit
  (the full walk after a slice, and the idle walk), a_wrong_reconcile_trips_its_own_check,
  the_marked_reconcile_matches_a_full_one, the_crate_and_the_model_agree and
  a_broken_model_disagrees.
- model-host-tests: run earlier at 7418c46d5; the model itself is unchanged.
- size-budget, unsafe-budget (no new unsafe), formatting, docs.
- Builds: `./build --arch rv64` and `--arch rv32`; checked kernel builds with qemu-virt, and with
  sched-trace,walk-trace, on rv64 and rv32.
- `./build --arch rv32 --debug` already fails to link on main (FLASH overflow).

## Pages

- `scheduling.md`, "The current minimum and ties": "The reconcile visits only the budgets whose
  runnable state changed in the entry, with the same wakes and leaves in the same order: each
  budget counts its ready threads, and a change of a process's ready threads marks the process,
  whose count then moves to its budget. Leaves have no order a pick can see."
- `scheduling.md`, the audits paragraph: the Architect's text, verbatim.
- `scheduling.md`, residual risks: the bullet now reads "A delivery and a timer expiry walk every
  thread" and adds that a reconcile visits only the changed budgets. I wrote it. If IPC3 merges
  second and its fixes make that bullet obsolete, IPC3 should drop it.
- The status line: reconcile is now described as measured at 250 wakers.
- `docs/todo/reconcile-walks-every-process.md` is deleted, with its SUMMARY line and its
  m1-separation link.
- The budgets.md line is dropped, per the Architect.

## Risks

- A miss that a later change undoes within a slice is not seen. It costs a turn, never a wrong
  run, and the page states it.
- The rv32 250-waker reconcile is unmeasured (see the caveat above).

## Next

The whole bench, which is the orchestrator's.

## Review fold and rebase (k22-implementer-2)

Tip **c1d235317** (docs) on 68b43da7f (code), rebased with `git rebase --onto f9135ce8a 53bcd9704
wp-k22`, no conflicts. Each change is folded into the commit that owns it; no fix-up commits.

- (A) scheduling.md: "with the same wakes and leaves, the wakes in the same order" (code commit);
  the R12 status now ends "...and break it there ([residual risks](#residual-risks)); `worst-walk`
  also measures a reconcile with 250 budgets waking at once, on rv64 only (on rv32 the deadline's
  waits end one per entry)". The stray comma before the link is dropped (docs commit).
- (A, B) The audit paragraph ("Targets exclude the checked build's audits") is reflowed to 100
  columns, and so is the tie paragraph after the text change. The residual bullet's 103-column line
  is split before its link.
- (B) The code commit's Size budget lines are now in CONTRIBUTING's form:
  "kernel: 8,115 -> 8,157" and "libs/stride: 332 -> 522", wrapped. "The scheduling page states the
  checks." is gone.
- (C) `ready_count` is now `self.ready_threads().map_or(1, |x| x.iter().count() as u32)`, which
  behaves the same (Setup is the only None). The kernel loses 5 lines, so its ceiling is lowered to
  the count, 8,157 rather than 8,162, and the message's counts follow (kernel +42, ptable.rs 10).
- (C) The two failure-only scans in `Marks::audit` are kept. They run only when the audit has
  already failed, and they name the slot or budget in `Missed`, the one fact a failing bench log
  needs.
- (D) red: "costs a budget a turn" now reads "a turn late or lost", on the page and in the commit
  message. Red found no path that misses a mark.

Host gates on c1d235317, every one through in-dev, all exit 0:
- `cargo testbench stride-host-tests`, `size-budget` (kernel 8157 of 8157, libs/stride 522 of
  522), `unsafe-budget`, `formatting` and `docs`.
- `./build --arch rv64` and `./build --arch rv32`.
- `cargo build --profile checked -p redoubt-kernel`, for riscv64imac and for riscv32imac, with
  `--features qemu-virt` and with `qemu-virt,sched-trace,walk-trace`.
- model-host-tests was not run: the fold did not change the stride crate's code.

No QEMU was run, and the whole bench is the orchestrator's.
