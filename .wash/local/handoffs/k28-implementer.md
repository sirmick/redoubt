# k28-implementer handoff (K28 done, red OK; K29 designed, approved, committed; its gate is the successor's)

## Branch state
- wp-K28: head 5e7c5170c, one commit on main b60c7cc5c, worktree /home/mcloonan/redoubt/.worktrees/K28 clean. Red verdict OK (no findings, release .text byte-identical). Never pushed. Report: .wash/local/K28-report.md.
- wp-K29: from 5e7c5170c, head 535685cda = K29's ONE commit, "kernel: a destruction takes its own endpoints off the to-pump list in one walk of the list" (kernel/src/message.rs, docs/kernel/budgets.md, tests/size-budget.toml). Worktree /home/mcloonan/redoubt/.worktrees/K29 clean. Design approved by the orchestrator (answers e16b6f00, 52ee04c7) as proposed. Every committed file was read in full by me (message.rs whole, budgets.md whole). Its target/prebuilt was built from the same kernel code but before the comment/docs edits: run `make prebuilt` again before any case (the index is fingerprinted on the tree). Report: .wash/local/K29-report.md (numbers, phase splits, procedure).
- Measurement worktree /home/mcloonan/redoubt/.worktrees/K29-base: detached at bdb38430e (train-8's main, pre-K19) with UNCOMMITTED measurement edits (println stamps in kernel/src/budget.rs destroy_subtree, walk-trace in tests/kernel-containment.toml). Keep for re-measurement or `git worktree remove --force` it. Never commit from it.
- No processes of mine running (q tenants K28/K29 idle).

## What the successor does (the orchestrator's list)
Run K29's gate on 535685cda, through jobs.mk from a fresh prebuilt (`make -f /home/mcloonan/redoubt/scripts/jobs.mk -C /home/mcloonan/redoubt/.worktrees/K29 prebuilt`, then targets) or `q run --cores N --tenant K29 -- cargo testbench --exact <gate>` for the gates jobs.mk lacks (formatting, size-budget, unsafe-budget, no-cruft):
1. rv64/kernel-containment, rv32/kernel-containment: PASS with R10 p99 near 23 ms (measured on this code: 22,925 / 23,542 µs). qemu_seed = 13 is the case's only seed: one run is "seed 13 and the default".
2. rv64/worst-walk, rv32/worst-walk (whole_run = false: by name; 10-13 min a width; procedure in .wash/local/handoffs/k19-implementer-2.md: R10 there was 16-18 ms, the rv32 expiry walk ~29.3 ms is the tight margin and is unaffected by step 4).
3. The 21 K19 cases + budget-reap, both widths (52 runs; list in .wash/local/K28-report.md "Gates"): endpoint-destroy-full endpoint-destroy-open-calls budget-destroy-kills ending-pumps-once destroy-keeps-notices destroy-keeps-notices-creator process-lifecycle redoubt-dead sched-destroy-billing pid-pinning-attack handle-chain-attack handle-chain-fault process-chain-fault budget-deadline timeouts userland-boot init-boot bench-net-peer ipc-outcomes budget budget-destroy-attack budget-destroy-growth deadline-flood-billed redoubt-revoke process-attack budget-reap.
4. sched-latency, sched-latency-tcg, both widths.
5. build-rv64, build-rv32.
6. rv64/model-host-tests (redoubt-kernel has no host tests: test = false), rv64/model-mutations (fanned, ~12 min).
7. docs, formatting (both PASS on this tree already), size-budget (FAIL 9382/9377 before the raise; rerun after it), unsafe-budget, no-cruft.
Then report with assignment_results (K29 is the assignment's result), detail in .wash/local/K29-report.md. Check affected summaries: docs/kernel/budgets.md (changed: the step-4 sentence and the gate's R10 numbers; the per-phase bisect list lines ~815-827 and "pumping after each thread instead ... 24.5 and 21.2 ms" are older measurements, left as history), docs/kernel/scheduling.md:576 "R10's p99 is 26,379 µs (rv64) and 27,407 µs (rv32) on every seed" (the 16-seed sweep's number; refresh only if the sweep is run), docs/kernel/README.md#containment (the sweep), docs/testbench.md (unchanged), README.md/GETTING-STARTED.md (no claim).

## K29 in numbers (rv64 kernel-containment, 20 destructions, ~9,338 object frames, qemu_seed 13, icount)
R10 p99: pre-K19 (train-8, bdb38430e) 23,394 rv64 / 24,104 rv32; K19 + K28 fix 29,500 / 30,576 (rv32 FAILS the 30,000 bound; alone on --quiet the same); with 535685cda 22,925 / 23,542, both PASS.

Phase split of the longest destruction, stamps at destroy_subtree's phase boundaries (ms):
| phase | pre-K19 | K19+K28 | 535685cda |
| kills (step 2, incl. T/t) | 8.04 (3 pumps inside, 1.6) | 6.29 | 6.29 |
| process::budgets_dying (step 3) | 0.29 | 0.13 | 0.13 |
| message::budgets_dying (step 4) | 9.45 | 16.06 | 9.49 |
| lift_dying | 0.44 | 0.44 | 0.44 |
| destroy_marked | 5.51 | 5.45 | 5.45 |
| pump_listed (2 pumps 0.66+0.74) | — | 1.55 | 1.55 |
| whole (with ~0.5 ms of prints) | 23.9 | 30.0 | 23.4 |
The whole growth was step 4. The pumps MOVED from the kills to the end; they did not grow. Cause: K19's owner walk did `List::pumps().contains(w, frame_word(o))` (+remove) for EVERY endpoint the dying subtree owns: O(1) each but 1-2 checked kframe reads through object_phys, over thousands of endpoints: ~1.6 µs each = 6.6 ms. Dismissed: unchain's `assert!(contains && contains)` (O(1) link reads).

## The change (kernel/src/message.rs budgets_dying, in 535685cda)
The per-endpoint contains/remove in the owner walk is gone. At the END of message::budgets_dying (after the queued and taken chains are failed: the last point anything can list, since fail_wait → pump_endpoint lists while deferring), one walk of List::pumps(): `first`/`next`, `frame_of(r)`, `mm.budget(mm.endpoint(frame).owner.frame).dying` → remove. destroy_marked's free_owned_endpoints frees the frames after; pump_listed's debug assert (owner live, not dying) is unchanged and checks it. Docs: budgets.md step-4 sentence (~line 754) and the gate's R10 numbers (~line 807: p50/p99 rv32 22.9/23.5, rv64 22.1/22.9 over 20 destructions); pump_listed's doc comment follows. Model: model/src/kernel.rs:2258 only says the list drains at the end: unchanged, by the orchestrator's approval too. tests/size-budget.toml kernel ceiling 9377 → 9382, Size budget line in the message.

## Measurement procedure (how the splits were made; repeat only if a number surprises)
1. Local, never committed: tests/kernel-containment.toml `kernel_features = ["sched-trace-large", "walk-trace"]` (adds M/m walk records: 1 pump, 2 expiry, 3 reconcile) and `println!("K29 <phase> {}", crate::time::now_us());` after begin_destruction, before/after process::budgets_dying, after message::budgets_dying, lift_dying, destroy_marked, pump_listed in kernel/src/budget.rs destroy_subtree. In the pre-K19 tree the anchors differ (`budgets_dying(ss)` without top; no pump_listed: stamp after end_destruction).
2. `make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <wt> prebuilt` then `... rv64/kernel-containment` (7-12 min under load).
3. WITH walk-trace this case OVERFLOWS the 192 MiB trace ring (SCHED-TRACE-END ... dropped ~1.15M), so the bench reports "guest exited while waiting for SCHED-TRACE-END ... dropped 0" and the oracle never runs: a measurement run's VERDICT means nothing; the destructions are inside the kept records, so the split is complete. Verdicts come from runs without walk-trace/stamps.
4. Parse: /tmp/k29-phases.py <console log> (may be gone; 35 lines): collect `K29 <tag> <decimal µs>` lines and SCHED-TRACE X/Y/M/m records (`SCHED-TRACE <seq> <entry> <kind> <id> <hex µs>`), pair by time (the ring is dumped at the end of the run, so console order does not interleave), print per-phase deltas for the longest X..Y windows and the M 1/m 1 pump spans inside. Console logs: <wt>/target/testbench/run-*/kernel-containment-rv64-smp1.log, 120-250 MB: never cat.
5. Trace kinds carrying µs: X/Y (R10), U/V (audit), T/t (threads' ending), M/m (walks), I/O. The lowercase letters L e f r q w A a G g v N n are lift/reconcile records, not costs.

## Traps
- `pkill -f '<name>'` inside a chained command kills the chain itself (exit 144): the pattern matches its own shell.
- Wait with `until grep -q ... ; do sleep 15; done`; bare sleep is blocked; `rc=` greps also match "prebuilt rc=0": grep the case's own line.
- `progress` messages to the orchestrator are held until you idle; send anything it must see as a `question`.
- jobs.mk has no formatting/size-budget/unsafe-budget/no-cruft targets: `q run --cores 2 --tenant K29 -- cargo testbench --exact <gate>`.
- A `prebuilt` fails with "the tree changed while ... was built" if you commit or edit during it; make it again.
- K28's audit fix: index_process's guard reads objects.destroying (debug-only), set in begin_destruction, cleared by destroyed() after pump_listed, before the Y record; deferring unchanged. The oracle's rule (no audit inside X..Y) and K18 (audits do not move the schedule) bound any change here.
- Reviewers (red, simplifier, editor) will read 535685cda whole; the SWARM rule that an implementer reads every committed file in full is met for it.
