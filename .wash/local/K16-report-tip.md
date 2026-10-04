# K16: c8 committed, bench key, tip for round 2 (k16-implementer-7)

Tip b81d94dde on base f8c1543f3 (not rebased; the rebase onto c2283a825 comes after round 2).

## Commits (new or changed this session)
- 08519e700 c5: + the satp/model.md reflows; the message's gate at 511; + `Size budget: model:
  10,239 -> 10,240` (FLOOD_THREADS). c5 had raised the model by one code line without it, so
  size-budget failed from c5 on, a gap from an earlier session.
- e7bad61f0 testbench: `whole_run = false` (case.rs `Case::chosen` + unit test
  `a_case_out_of_the_whole_run_runs_only_by_name`; main.rs; testbench.md: usage, case file
  example and paragraph, status line names the host test).
- b81d94dde c8: walk-trace (kernel), oracle M/m walks (+ the unit test's missing assert),
  worst-walk program and case (`whole_run = false`, must_fail
  `^sched_oracle: R10.s p99 is \d+ µs over 2 destructions, above 30000$`, the oracle's line in
  its description), residuals in ipc.md, timer.md, scheduling.md (new bullet, "Reconcile walks
  every process"; R12's partly-tested clause), budgets.md (measured list); three todo pages
  (delivery-walks-every-thread, expiry-walks-once-per-wait, reconcile-walks-every-process) in
  SUMMARY.md and m1's Remaining work; testbench.md names worst-walk as the by-name case.
  Size budget: kernel 8,081 -> 8,115 (walk-trace).

## Checks (in-dev; no QEMU)
- docs (doccheck): exit 0, PASS. size-budget: PASS. unsafe-budget: PASS (kernel 13/13/18).
- rustfmt +nightly --check on every changed .rs: clean.
- cargo test -p testbench and --list: see the report message (queued until INIT5's bench ends).
- worst-walk itself: not rerun after the key (no QEMU); its run at 511 is the one in
  K16-report-c8.md, same kernel and program.

## Not in the trace
The trace does not part the expiry's walks from its pumps, nor R10's 4.8 s besides its pump;
the pages say so rather than attributing them.

## Correction to my question on the expiry
Each walk resets a process's earliest timeout, so a process leaves the walks once its wait ended:
the walks visit 250, 249 .. 1 due processes, about 250 x 251 / 2 x 255 = 8M thread visits, not
16M. The todo page states that as an estimate from the code.

## Round 2 fold (rebased tip f450ea7ea on c2283a825)
- P2-4 kept, one line: the two TID masks are not one: allocated_threads lives in the process's own
  header page (read for the current process with no MemoryManager borrow), Account.live is read for
  any PID, and they differ between destroy_thread and thread_ended.
- left() (sched.rs:114) is only a trace record (`D`): the stride queue's Budgets::left is a no-op
  default, the model has none, and the oracle drops the budget from a set, so its order within an
  entry matters to nothing.
- Red 1: a misspelt top-level key (`whole_rum`) was already refused: Case's unknown keys reach the
  flattened kind, and every kind is deny_unknown_fields. The whole_run unit test now pins it.
- Editor 2, not reflowed: todo/kernel-attack-gaps.md keeps one line per bullet (48 of its lines
  are over 100 columns on main, up to 372); the two bullets follow the page. Its other lines over
  100 (Status summaries) are single-line by doccheck's parse.
- Editor 1: PID 1 is the kernel's, not init's (init is PID 2); the pages say so.

## Rebased onto bfdb471b3 (INIT5): tip 8e730782d
- Rebase: no textual conflicts. Host checks at the tip found two K16 consequences in tests main
  added since f8c1543f3, fixed in c5 (lines in its message):
  - fsd-host-tests hung: `labels=1,...,9` was over the old MAX_LABELS (8) and is valid at 16, so
    fsd served and never exited. The test now builds MAX_LABELS + 1 labels.
  - init-host-tests: more_servers_than_init_has_threads_to_watch_are_refused at 254 servers hit
    init's page bound (INIT_PAGES 1,024 since INIT5) before the watcher check succeeded; the test
    lifts root's page limit as it already lifts system's process limit.
  - init's bound test reads objects.md's row "| thread IPC page |": c2's row name went back to it
    (the paragraph under the table says the saved registers are in the IPC page).
- Checks at 8e730782d (in-dev): docs, formatting, no-cruft, size-budget, unsafe-budget PASS;
  build: 24 PASS; host-tests: 16 PASS (fsd and init after the fixes; the other 14 in the
  full run before them, unchanged since).
- Not yet: the gate (kernel-containment rv64) at this tip, the whole bench, and worst-walk by
  name. docs and formatting overlapped FSD3's whole bench start by ~11 s.

## Gate at 8e730782d (red's 3): kernel-containment --arch rv64, qemu seed 13: exit 0, PASS 218.2 s
- share 829 of 1000 (floor 783); R10 18 destructions p50/p99/max 20,740/25,253/25,253 µs, no
  audit inside one; budget_destroy call to return 55,344/55,590 µs
- driver_wake net p50/p99 8,635/11,103; timer_wake 7,760/8,817; decision_wake 7,691/7,708;
  deadline_notice net p99 28,680 (<= 40,000); lease end 7,708 + 25,253 = 32,961 (<= 125,000)
- log .wash/local/K16-gate-8e730782d.log

## Whole bench at 8e730782d: in progress at handoff (.wash/local/K16-bench-8e730782d.log)
- So far: FAIL bundle-mapped [rv64]: it pins the 64-PID boot split (system 15, users 47); at 511
  it is 127 and 382. A K16 consequence in a test from main; fix in c5 (see K16-handoff-7.md).

## Whole bench at 8e730782d: FINISHED (after the handoff was registered)
`cargo testbench --allow-skip`, exit 1: 359 PASS, 1 SKIP (bench-ssh-loopback-openssh: podman
not installed, a host lack), 2 FAIL, both bundle-mapped (rv64 and rv32), the 64-PID boot split
pin above. Run dir target/testbench/run-1-1791047978421737188. Nothing else failed: after the
bundle-mapped fix in c5, rerun `cargo testbench --allow-skip bundle-mapped` (both widths) and
quote it; the rest of the bench stands.

## Final (k16-implementer-8)
- bundle-mapped fix folded into c5 (now fb61f6fcf before the rebase): the test derives the split
  from PROCESSES = 510 (MAX_PROCESS_COUNT - 1), system /4 = 127, users the rest less init's 1 =
  382; tests/bundle-mapped.toml's expected line says 127/382. c5's message has a line for it in
  "The cases follow the values". `in-dev cargo testbench --allow-skip bundle-mapped`: exit 0,
  PASS rv64 and rv32 (log .wash/local/K16-bundle-mapped.log). Pre-rebase tip 4794add1d.
- `git rebase --onto 75245a114 bfdb471b3 wp-k16`: clean, no conflicts. Tip b14e41c91 (11 commits).
  size-budget.toml: FSD3's blkd/init/fsd lines and K16's kernel/stride/model lines merge apart.
- Host gates at b14e41c91, each `in-dev cargo testbench --allow-skip <name>`: docs 0,
  size-budget 0, unsafe-budget 0, formatting 0, host-tests 0 (20 PASS, no FAIL/SKIP); log
  .wash/local/K16-hostgates-b14e41c91.log.
- Whole bench at b14e41c91, `in-dev cargo testbench --allow-skip` (both widths): exit 0, 379
  PASS, 0 FAIL, 1 SKIP (bench-ssh-loopback-openssh: podman not installed, a host lack); log
  .wash/local/K16-bench-b14e41c91.log. Gate numbers at the code-identical pre-rebase tip stand:
  kernel-containment rv64 PASS, share 829, R10 p50/p99 20,740/25,253 us, lease end 32,961 us.
- State: wp-k16 at b14e41c91 on 75245a114, clean worktree, not pushed. Ready to merge.
