# K16 handoff from k16-implementer-6 to k16-implementer-7

Worktree `.worktrees/k16`, branch `wp-k16`, tip 248031a17, base f8c1543f3 (= fa858cbde on the
rewritten main). Scratch `.k16/` (never staged). Never `cargo fmt` (stable rewrites the tree):
`rustfmt +nightly --config skip_children=true <file>`.

## Commits
Final: 23bbedefc c1 · 5b0e4711f churn · 16021d3b3 c4 · 4023a90fa c2 · 2b55a9381 c3 · 69168a158
stride · 19760c03a c5 (511 + ASID assert + pages, reworded) · 31cb7764e c6 process-fill (509) ·
571f5343f c7 thread-limit. WIP: 248031a17 "WIP c8 worst walk" (must become the real c8).

## Round-1 notes: all applied (into c1/c2/c3/c5), no fixups left. P2-4 kept, reason in the report:
allocated_threads is in the process's own header page (current process, no mm borrow);
Account.live is read for any PID; they differ between destroy_thread and thread_ended. Editor's long
`<summary>` line left (main has 119-col ones). Red 3 unchecked.

## c7: PASS both widths at 512 (not rerun at 511). process_create = header + LEVELS tables (3/4).

## 511 ASID ruling (K16-asid-bound-ruling.md): APPLIED in c5/c6/c8 sources; nothing built or
run at 511 yet. c5's message still quotes the gate at 512: replace with the final-tip gate.

## c8 (WIP): walk-trace feature (M/m spans: pump, expiry, reconcile; nested -> outer only);
oracle reports walks net of audits + audit time + pumps inside each destruction (unit test
updated, NOT rerun since); tests/worst-walk.{toml,rs}: rv64 checked, 4608 MiB, 509 holders x 255
(+init) = 129,796 threads; last 250 reports held, answered with one deadline (cheap: no pump);
then one pump (sender thread) and one destroy. R23 lists updated (scheduling.md, README.md).
NO NUMBERS YET (510 pumps did not finish in 13 min). Run in flight: pid 2422434 (from
~08:07, timeout 3600 s), log .k16/foc-worst-walk.log + target/testbench/run-1-1791040049*/.
It may be at 510 or 509 holders (built around the 511 fold): check the console line.
Still to do in c8: record R10 net, one delivery's time, destruction pumps (count/time; steps
that pump: process_ending once per served endpoint, exit notice pump_endpoint per killed
process (process.rs:742), fail_wait); residual lines in ipc.md/timer.md/budgets.md with
"(rv64, checked build, net of its audits, N live threads across 510 processes)"; ipc.md's
"Delivery walks every thread" names the R12 conflict in K16-pump-ahead.md's words.
R10 > 30 ms net => STOP, report; owner decision (K16-pump-ahead.md options).

## c9: no change needed (c5: 15 ok, 16 TooLarge, destroys chain[1], checked). Report it.

## Then: gate rv64 s13 at the tip (.k16/report-c6-c9.md), sched-latency both, process-fill,
thread-limit, pid-reuse-authority, budgets, docs, fmt, model. Rebase only on orch's word:
`git rebase --onto a8cbbc7ad f8c1543f3 wp-k16`, never plain `git rebase main`.

## Traps: child panics are silent (no UART); --fixup can't take -F: commit "amend! <subject>".

## Context went to: worst-walk's five failed runs.

## UPDATE: the run finished at 511 (509 holders): R10 11.7 s net => STOP-AND-REPORT. Numbers: .k16/report-c8.md.
