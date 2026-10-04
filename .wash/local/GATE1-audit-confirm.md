# GATE1 — the checked build's audits account for c (rv64 seed 3, 2026-10-01)

Local, uncommitted, reverted: commented out `#[cfg(debug_assertions)] MemoryManager::with(|mm|
mm.check_object_indexes())` after Y in destroy_subtree (kernel/src/budget.rs:1259-1260) and
`#[cfg(debug_assertions)] self.check_process_index()` in index_process (kernel/src/process.rs:
225-226). (A first attempt commented only the calls and failed to build; no run.) Case file as on
the branch, 'target missed' forbid in place. Afterwards `git checkout` of both files; `git status`
clean; tip d806b8a43 (WIP split instrumentation) on 21eb80f6c.

`in-dev cargo testbench kernel-containment --arch rv64` → **exit 0, PASS, 197.2 s**.

| Measure (µs) | audits on | audits off |
| --- | --- | --- |
| deadline notice p50 / p99 | 53675 / 66857 | **21567 / 23182** (target 40000: met) |
| a (deadline→X) p50/p99 | 270 / 341 | 270 / 323 |
| b (X→Y) p50/p99 | 20806 / 20952 | 20808 / 20954 |
| c A (Y→agent notice) p50/p99 | 24422 / 37663 | **343 / 355** |
| c B (Y→sub-agent notice) | 32599 / 46164 | **1958 / 1971** |
| R10 post-check p50/p99/max | 23228 / 23636 | 20954 / 23637 / 23637, 17488 frames max |
| lease end | 32052 | 8507 + 23637 = 32144 (125000) |
| driver wake | 8719 / 10608 | 8656 / 10772 (max 28748) |
| timer wake | 7966 / 9566 | 7966 / 9565 |
| decision wake | 8415 / 8416 | 8415 / 8507 |
| budget_destroy call→return | 110510 | 31174 / 31179 |

All 10 ok rows, oracle and post-check pass; victims responsive: met; D re-made 0.
