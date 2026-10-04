# GATE1 ring assignment: the bench, and the churn regression (2026-10-02)

Tip: WIP 41ac2bee5 on wp-gate1 (the ring 16384 commit 192097eaf, then the gate's own child entry). The tree is clean; nothing is folded.

The whole bench, run alone: 278 PASS, 1 SKIP, 5 FAIL.
- handle-chain-fault on both widths, and endpoint-destroy-open-calls rv32: each passed 3/3 alone. In the bench, endpoint-destroy-open-calls hit the handle.rs panic that handle-chain-fault injects, which looks like a build race in the bench. process-chain-fault is the same family and passes.
- sched-latency rv32 (N=16 decision p50 29597 > 25000): mine. The gate's roles were reachable from the shared child(), so every sched binary was about 27 KB bigger and its spawns slower. Over seeds 1-8 the tip was one slice worse than main on every seed. Fixed: containment_child plus Bench::set_entry. rv32 p50 is now 18657 (main 18645); PASS on both widths.
- sched-budget-churn rv32 (shell share 559 net > 550): the ring.
  - tip, PAGES 8192: 499 PASS
  - tip, PAGES 16384: 559 FAIL
  - commit 1's sched.rs with the gate binary removed, 16384: 559
  - memory_mib = 288, 16384: rv32 557 FAIL, rv64 499 PASS
  Every churn variant's victim moves, by the same amount on every seed.

Ring fill at seed 13 (16384 pages): rv64 1,324,615 (63%); rv32 1,402,542 (67%). The gate's lease rows are ok.

## Diagnosis (ruled (c)): rv32, sched-budget-churn, ring 8192 vs 16384, seed 3, tip f0e5ce674
Runs: churn-8192.out/.console (PASS, shell 499) and churn-16384.out/.console (FAIL, shell 559). Script: /tmp/churn.py.

| | 8192 | 16384 |
| --- | --- | --- |
| Z (high_frame) | 8838-8846 | 17030-17038 (+8192, the ring's added frames) |
| R10 (X/Y) p50 / max | 1066 / 2979 µs | 1066 / 2980 µs (unchanged) |
| destructions in the run | 86 | 74 |
| audit kind 1 (destruction audit: check_object_indexes) | 86, 1,276,460 µs, 14.8 ms each | 74, 1,503,652 µs, 20.3 ms each |
| audit kind 2 (check_process_index) | 61, 186,513 µs, 3.1 ms each | 57, 331,154 µs, 5.8 ms each |
| trace records | 11,047 | 10,308 |

- **Audits:** both kinds scale with high_frame, and both are stamped (U/V). Across one 20.5 ms kind-1 audit, the shell's (104) pass rose about 234 µs worth (P ad1c927d59 -> ad269db08c; the timer's B charge gives about 0.72M pass per µs). So the audits are stamped and not billed, as K18's rule says.
- **The walks over 0..=high_frame outside sched::audit:**
  - check_irq_index from index_irq (device.rs:193): checked build, unstamped, but only on IRQ object changes. Churn makes none.
  - check_all_dying: only when root is destroyed.
  - destroy_quarantined_devices (message.rs:1644, called from terminate_process and kill_process, ptable.rs:784 and 801): production, but it returns at once unless dma_take_doomed().
  - None of these runs in churn.
- **What does scale, unstamped and billed:** mem.rs alloc_frame (line ~236) is `allocations.iter().position(Option::is_none)`, first-fit from frame 0. The ring's frames are taken at boot below everything (kernel_frame), so every later frame allocation scans 8192 more slots: every budget_create's object frame, every page. That is billed to the caller. The shell makes 5 budgets per cycle, so it pays the longer scans, does fewer cycles (74 against 86 destructions), and the victim's net share rises from 499 to 559. The trace volume falls with the cycles.
- **Not measured directly:** there is no stamp around alloc_frame. The reading rests on the code and on these effects: R10 is unchanged, the audits are not billed, and the cycle count falls.
