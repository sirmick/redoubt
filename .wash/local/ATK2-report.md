# ATK2 — attack cases for scheduling.md's eight partly-tested lines

Base: main 985ec0047. Head: 0396a6edd (one commit, amended after review and after train 34's failure; tree clean). Branch wp-ATK2, worktree /home/mcloonan/redoubt/.worktrees/ATK2.
The kernel is unchanged (no file under kernel/ touched).

## Lines closed, and how

1. **Round-robin in a budget** — `tests/sched-round-robin.toml`, `tests/programs/src/bin/sched-round-robin.rs`,
   `Role::RoundRobin` in `tests/programs/src/sched.rs`. One budget, two processes × two threads, one hart,
   release build, icount. Each thread notes its resumptions (gaps > half a slice); the launcher checks the
   cycle set by the four first runs is (pid, tid) order from the cursor (each process's main thread then its
   second, the processes in either order), that twelve rounds run the cycle in order each thread once,
   wrapping, and counts within a tenth. rv64: slices 25/24/24/24, counts 589,056/570,624/564,480/568,832;
   rv32: 24/24/23/23, 199,936/201,984/198,912/193,280.
2. **An interrupt's wake does not preempt** — `tests/sched-irq-wake.toml`, `sched-irq-wake.rs`; oracle check
   `irq_wake_no_preempt=N` (`tools/testbench/src/sched_oracle.rs::check_irq_wake_no_preempt`): a device
   interrupt from user mode (`x`..`O`) that claimed a source, woke one budget, returned `O 1` with no pick
   inside, and whose hart's next pick/interrupt is the runner's slice end (`I`..`R runner`..`O≠1`); every
   claimed device interrupt without a destruction must return to user mode. 60 of 60 alarms both widths.
3. **Another budget's deadline preempts** — `tests/sched-deadline-preempt.toml`, `sched-deadline-preempt.rs`;
   oracle `deadline_preempts=N` (`check_deadline_preempts`): every `I` entry with an `X` of another budget
   must leave for `kmain` (`O 2`/`O 0`), the runner requeued, and the next event on the hart a pick; N of
   them. 94 (rv64) and 80 (rv32) over 2.5 s, all mid-slice (the checked build's audits eat most of the
   window: 78 audits = 356 ms in the first 300 ms tried, hence the 2.5 s window).
4. **Interrupt handling billed to the device object's owner** — same fixture, oracle `irq_billed_to=<mark>`
   (`check_irq_billed_to`): every claimed device interrupt charges the marked budget after its claim; only
   the owner and, for the return, the interrupted budget may be charged. The owner is `system` (the boot
   charges every device object to `system`, kernel/src/budget.rs:807; `mm.device(frame).owner` in
   `bill_irq`) — the page said "the owner of its device object" without naming it; now named. 60 alarms:
   system 286,980 ticks (rv64) / 208,903 (rv32), the spinner 19,476 / 31,488 for the returns.
5. **budget_destroy call-to-return asserted** — `Stats::DESTROY_CALL` (tag 7, measure `destroy_call`) in
   `sched.rs::steward`, held and handed over like the other windows; `MEASURES` in the oracle; bound
   `destroy_call_p99_us=51000` in `tests/sched-latency.toml` (R10's 30 ms + (17 + 2 + 2) slices, the
   page's formula at N = 16). Measured net p99 (gross in brackets): see gates below.
6. **R12 beyond the memory calls** — `tests/call-bounds.toml`, `call-bounds.rs` (checked, `hold-trace`);
   oracle `CALL-BOUNDS` line (`check_call_bounds`): per syscall cause, max lock section net of audits in the
   filled window ≤ 2 × max in the empty window, or 100 ticks; ≥ 8 calls judged. Hostile: 4,093 handles,
   2,000 endpoints, 190 senders queued at the victim's endpoint, 60 timed waiters. All 11 calls within the
   bound both widths; `receive` grows ~20 % (9,585→11,380 rv64; 8,070→10,033 rv32) with 190 queued senders
   and `thread_exit` ~45 % with 250 threads in the process (its own objects: the rule allows it); the rest
   flat. Sensitivity: a 100-tick floor × 2 catches a scan of the 4,096-slot table or the 2,000 endpoints
   (tens of µs); a term of ~2 µs per queued sender would not be caught — stated on the page.
   Trap found: the kernel's section ticks count from the kernel's boot (`BOOT_TICKS`), not `rdtime`; the
   firmware+loader take ~3.3 s of virtual time, so the first version matched no section; windows are now
   `time_now × tpu`.
7. **R23** — `tests/bench-no-trace.toml` (kind `build`, release `redoubt-kernel` with `qemu-virt`,
   `forbid_in_binary = ["SCHED-TRACE", "5sched5trace"]`), `tests/bench-trace-scan-catches.toml` (checked +
   `sched-trace`, same scan, `must_fail` on the console-line hit — the standing planted negative),
   `tests/bench-test-only-refused.toml` (release + `sched-trace`, `must_fail` on the kernel's compile_error
   — the status line's second half). Bench: `Build` gets `debug_assertions`, `forbid_in_binary`,
   `must_fail`; `main.rs::forbidden_in_binary` (byte-string scan of the whole ELF; the release profile keeps
   symbols, `strip = false`, so a compiled-in trace would name itself as `..5sched5trace..`).

## Lines left, and why (reworded on the todo page with the reason)

- Ties clauses 1 and 4: a waker ties a requeued budget, or two requeued tie, only at passes equal to the
  tick; charging makes that a coincidence (weights under `users` cannot make pass steps coarse enough). Host
  tests and the model cover them (existing). No case can force it on QEMU.
- Failure and restart: a picked thread dying before the switch needs another hart's destruction to land
  between kmain's pick and its switch (interrupts are off in the kernel, so at one hart it cannot happen) —
  a race no case can time; a full queue cannot be made (one slot per process).
- R12: lends/transfers, `process_map`, `set_flags`, device calls still not attacked (line reworded).

## Negatives (each run once, not in the tree unless stated)

- R23 scan: `bench-trace-scan-catches` is a standing must_fail case (passes = the scan caught the plant);
  run without `must_fail` it fails: `the binary holds "SCHED-TRACE" at offset 0x37ba1
  (get.rs..SCHED-TRACE-END....dropped......ker)` on rv64, `0x3b69e` on rv32 (.tmp/ATK2/neg-zz-atk2-scan-negative-*.log).
- `irq_wake_no_preempt=61` (60 alarms): FAIL "wanted one budget woken by at least 61 ...; 60 claimed a
  source, 60 woke one budget, proofs by budget {32: 60}". No kernel feature makes a device wake preempt
  (`wake-preempts` covers timeout wakes only), and the kernel must not change: the floor is the negative.
- `deadline_preempts=200`: FAIL "... 94 preempted mid-slice, 0 with the runner's slice over too".
- Program mutations, each run once on rv64 and reverted (`git checkout`), logs .tmp/ATK2/neg-*.log:
  - round-robin with the order check expecting each process's second thread first: FAIL
    `[round-robin] FAIL: the first runs follow (pid, tid) order: ...` (the forbidden FAIL line).
  - irq-wake with the mark on the spinner instead of `system`: FAIL `record 1269: a device interrupt that
    claimed source 11 charged 4783 ticks to budget 2, neither its device's owner 32 nor the budget it
    interrupted 32` (the owner really is `system`, budget 2, not whoever holds the handle).
  - call-bounds with factor 1 and floor 1: FAIL `` `thread_create` held the lock 778 ticks net of audits after
    the fill, above 770: 1 x its longest on the empty system, 770 (floor 1) `` (the bound bites on the
    smallest real growth).
- Oracle host tests carry each check's negative traces: `a_device_wake_that_preempts_is_caught`,
  `a_device_interrupt_bills_its_devices_owner`, `a_deadline_that_does_not_preempt_is_caught`,
  `call_bounds_hold_each_call_to_its_empty_system_section`.

## Gates (all through scripts/q; logs under .tmp/ATK2/ and .tmp/ATK2/logs/set*/)

- The 7 new cases, both widths, three passing runs each: set1+set3+set4 for sched-round-robin, sched-irq-wake,
  bench-no-trace, bench-trace-scan-catches, bench-test-only-refused; set2+set3+set4 for call-bounds and
  sched-deadline-preempt (set1 was their first version: call-bounds matched no section because its windows
  were on rdtime; deadline-preempt had a 300 ms window with 12 deadlines; both fixed as described). All PASS.
- Cases sharing the fixtures (set5, both widths): sched-latency smp 1 and 2 PASS with `destroy_call`
  N=16 net p99 9,504 / 11,923 µs (rv64 smp1/smp2), 11,456 / 18,468 µs (rv32), gross 52–68 ms, bound 51 ms
  net; sched-latency-tcg (smp 2) PASS; sched-carve-return, sched-destroy-billing, sched-wake-no-preempt,
  sched-ties PASS.
- Host tests: `cargo test -p testbench`: 203 passed (the 4 new oracle tests among them).
- docs gate (`make -f scripts/jobs.mk docs`): PASS. fmt gate (`formatting` case): PASS after
  `cargo +nightly fmt` on the four Rust files.
- Not run: kernel-containment (30 min; shares only `collect_words`'s array width, which it indexes by tag),
  the whole bench (the train's).

## Docs changed

- docs/kernel/scheduling.md: statuses for One flat stride queue (16), Preemption points (14), Charging (20),
  R12 (65, enumeration extended), R23 (details block, 3), Failure and restart (reworded reason); prose for
  each case with both widths' first measurements; the Responsiveness table's budget_destroy row (judged);
  the sched-trace/hold-trace case lists; two rows in the one-hart table (Residual risks).
- docs/todo/kernel-attack-gaps.md: scheduling.md section — five lines struck with their cases, three
  reworded with the reason.
- docs/testbench.md: Self-checks (17) + prose for the three build cases; `build` row of the kind table;
  The scheduler oracle (38) + prose for the four new checks.
- docs/SECURITY.md: R12 row + bench:call-bounds; R23 row: tests, status built.
- Summaries checked, no change needed: README.md, GETTING-STARTED.md (no scheduling claims),
  docs/kernel/README.md (R23 references point at the section; containment unchanged), docs/kernel/timer.md
  and docs/kernel/devices.md (R5 status still says billing to the owner is not attacked — see open point).

## Open points for the reviewer

- devices.md R5's status line says "billing an interrupt to its IRQ object's owner ... not attacked";
  `sched-irq-wake` now attacks it from the scheduling side. I left devices.md alone (not this package's
  page: the plan names scheduling.md); the orchestrator may want a one-line follow-up.
- `sched-latency-tcg` (host clock) shares the fixture; result in gates.
- kernel-containment uses `collect_words` (capacity 4→5, type only) and the unchanged containment steward;
  not rerun (30 min); the change cannot alter its behaviour (it reads `words[i]` by tag).

## Review round 1 (kernel-red-2), folded into 877a78c4a

- P1: `keep_smp = true` with the one-hart reason in tests/sched-deadline-preempt.toml and tests/call-bounds.toml;
  both added to the one-hart table (scheduling.md, Residual risks). Gates: both cases, both widths, three runs
  (rev1-3, all PASS); docs PASS; fmt PASS. Logs .tmp/ATK2/logs/rev*/.
- P2: receive's reading reworded: one point at 190, the caller's own queue filled by the hostile, bounded by the pump
  reading the endpoint's own lists only (worst-walk).
- P3: testbench.md: irq_billed_to's owner is the program's to name (the case names the boot's, system); the R23 scan
  proves this trace absent (line prefix, module path), not every channel.
- devices.md R5 status (tested (7), +bench:sched-irq-wake), its Residual-risks bullet, and SECURITY.md's R5 row written
  as the final merged text over ATK3's (0e650b847): mask in the model, completion order argued (ATK3's argument kept),
  billing attacked by bench:sched-irq-wake (owner system, whoever holds the handle).

## Train 34 failure (kernel-containment rv64): diagnosis and fix, folded into the amended head

- Symptom: `sched_oracle: gate destroy_call: 0 windows printed, but the program counted Some(1)`.
- Cause: a report-tag collision, not the gate's contract. Reports on a stand-in's slot 1 carry `tag | count << 8`
  in word 3; `Bench::samples` names a measure by its tag alone (`Stats::measure`). The new `Stats::DESTROY_CALL`
  was 7, which the containment gate already uses as `CT_VERDICT` (its steward's one verdict report, count 1); so
  `samples` printed `LATENCY-COUNT ... destroy_call 1` for the containment's verdict and no window followed, and the
  oracle's 'counted but not printed' arm tripped, as it should. The gate's contract ('every program that counts a
  measure must print its windows') stands and stays in the oracle; kernel-containment's program never meant to
  count it.
- Fix: `Stats::DESTROY_CALL = 10` (the tag space is Stats 1-6 and CT 4, 7-9; 10 is used by no report), with the
  shared tag space documented on both `Stats` and `CT_VERDICT`. No oracle change.
- Siblings: kernel-containment has no rv32/mttcg twin (one toml); `destroy_call` is declared by sched-latency.toml
  only; sched-latency-tcg shares the program and judges it with no target. The round-robin role's tags go to a
  launcher that never calls `samples`.
- Why it was missed: I reasoned kernel-containment's change was type-only (collect_words width) and did not run it;
  the measure-name table is shared by every program's reports, which I did not trace. Lesson noted.
- Gates after the fix (fix1, logs .tmp/ATK2/logs/fix1/): kernel-containment PASS rv64 (270 s) and rv32 (348 s);
  sched-latency PASS both widths at smp 1 and 2; the 7 new cases PASS both widths once; docs PASS; fmt PASS.
