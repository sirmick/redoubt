2026-10-02T23:39:36Z K16 commit 1: walks by live thread coded (Account::live mask, pids(), live_tids), builds both widths; pages ipc/timer/budgets reworded; baselines on main running in scratch worktree k16-base; asked orchestrator how to measure kernel-containment (GATE1-only)
2026-10-02T23:43:24Z c1 91c893e29 committed. edf R10 rv64 11082->10468, rv32 11104->10475; sched-latency R10 p99 rv64 6366->5601, rv32 6576->5829, all PASS; gate scratch running
2026-10-03T00:07:09Z c1 checkpoint: gate before/after done (s4,s3, both widths, all PASS, R10 -1.3ms); docs PASS; reported
2026-10-03T00:18:49Z c4 277a2c060 committed (tables .bss, stack arrays; dma/stride/redoubt.rs per orchestrator). Image rv64 221302->131278, rv32 222666->138912. Whole bench running.
2026-10-02 17:21 k16-implementer-2: started b341a4ba. whole-c4 bench still running (pid 2727879, log .k16/whole-c4.log); rv32 sched-latency s3/4/5 on main running in scratch worktree .worktrees/k16-main (logs .k16/sl-main-s*.log); HEAD runs after the bench.
2026-10-02 17:25 main rv32 sched-latency done: lease end s3 47000 (dw 40424), s4 47071 (40427), s5 57816 (51362); all PASS exit 0. Waiting on whole-c4 before HEAD seeds.
2026-10-02 17:49 whole-c4 exit 1: sched-timer-flood FAIL both widths (445/446 < 450); main/c1 pass at 453-456; bisected: no decision changes, share moves +-10 with kernel entry length (nop padding). rv32 lease end: phase (HEAD 23874/34745/23800, main 47000/47071/57816). Reported b341 with question; detail .worktrees/k16/.k16/report-b341.md.
2026-10-02 17:57 rebased onto 31dfad3f0 (clean): sched-timer-flood PASS both widths; rv32 net 452/451 vs main 468/471 (commit 4 -15; stride copy recovers ~10).
2026-10-02 17:59 timer-flood seeds 4,5 on df074272d: identical to seed 3 (rv64 461/464, rv32 452/451), PASS exit 0; reported.
2026-10-02 19:36 rebased onto 81b5ea38b (K20): tip 1c440d414; timer-flood PASS rv32 net 488-490; sched-latency s3 PASS; size ceiling 7880. Reported; waiting on INIT1.
2026-10-02 23:27 a8292e92: rebased onto 5f9f9d61c (INIT1): 3cc54449f c1, 657d139c0 c4 (msg: no kernel raise now, 7856<7863; sizes re-measured). Starting commit 2.
2026-10-02 23:34 commit 2 code+docs drafted (header page, ctx in IPC page, asm one load); next: tests with rv64 context charges, build, focused cases.
- 2026-10-03T06:58:33Z impl-3: aliasing fix done (set_tid, setup_empty_process, setup_first_thread); earlier focused run died after 'thread'; rv64 sched-budget-churn shell share 577>550 once — sweeping seeds 1-6 on main and WIP
- 2026-10-03T07:01:28Z impl-3: commit 2 folded 326398dd3 (aliasing fix incl. setup_first_thread); focused set running in k16-b; sched-budget-churn rv64 fails from commit 1 on INIT1 base (shell victim 576 vs 501) — asked orchestrator; starting commit 3
- 2026-10-03T07:06:05Z impl-3: commit 2 amended 213197457 (kernel ceiling 7894, stale PROCESS_AREA row); commit 3 code+docs drafted, both widths build; static checks next
- 2026-10-03T07:18:03Z impl-3: commit 3 159de631d (u16 PIDs, satp ASID 0); docs/host-tests PASS; focused set on c3 running (.k16/foc3-summary.txt)
- 2026-10-03T07:20:38Z impl-3: per orchestrator, commit 3 split: PID half b12c58f52 committed; satp half held (branch k16-c3-full, .k16/satp-half.patch); focused set on b12c58f52 running
- 2026-10-03T07:21:01Z impl-3: hold lifted; commit 3 restored as one commit 159de631d (PID u16 + satp ASID 0); remaining focused set running (.k16/foc3b-summary.txt)
- 2026-10-03T07:34:53Z impl-3: c2/c3 bodies amended (41ad6a29d, a4d233181; fpga doc folded); starting commit 5 (values)
- 2026-10-03T07:37:54Z impl-3: c5 WIP 'values + TidMask' builds both widths; STOP per ruling 6: rv64 data+bss 464,368 of 524,288 (59,920 headroom < 64 KiB); rv32 456,096 (68,192). Asked orchestrator.
- 2026-10-03T07:50:09Z impl-4: started; reading ruling, linker scripts, loader
- 2026-10-03T07:51:51Z impl-4: link.x/link64.x 1024K + stack assert; process_ending served -> static SERVED; building
- 2026-10-03T07:52:41Z impl-4: thread-limit widened; focused run c5a (.k16/foc5a-summary.txt)
- 2026-10-03T07:54:32Z impl-4: pages: memory-layout, processes, objects, abi, budgets done; next ipc, invariants, timer, scheduling, model, README, plan, servers, userland
- 2026-10-03T07:57:08Z impl-4: fixed proc-lifecycle, redoubt-ipc/filler, budget-test ladder, budget-syscall-attack; next host tests (ipd, model, rt)
- 2026-10-03T07:59:30Z impl-4: pages done (all brief list); model bomb+coverage scaled; asked orch re rt admit / ipd sizing tests (48->192)
- 2026-10-03T08:08:19Z impl-4: c5 STOP: gate fails both widths, sched-latency N=16 decision_wake missed; WIP 2622a8070; asked orch (b5823845) to run attribution (a)/(b)
- 2026-10-03T08:14:21Z impl-4: attr (a) 64 PIDs new values: gate PASS rv64 (share 821, destroy 48.4 ms); (b) running; walkstat patch ready for (c)
- 2026-10-03T08:29:05Z impl-4: item1 live-PID set (bits.rs, PidMask, live_pids) + item2 dense stride queue done; remeasuring gate rv64 (.k16/attr-d*)
- 2026-10-03T08:31:56Z impl-4: WIP 73f4883d9 (item1), 10281b636 (item2); handoff K16-handoff-4.md; remeasure d still running
- 2026-10-03T08:32:49Z impl-4: stopped remeasure d (container killed); handoff updated with destroy-constant paragraph
- 2026-10-03T08:40:46Z impl-5: started; gate rv64 seed 13 on 10281b636 running (.k16/attr-e.log)
- 2026-10-03T08:47:53Z impl-5: gate rv64 s13 at 10281b636 PASS share 831, R10 p50/p99 20163/25440; focused set next
- 2026-10-03T08:56:07Z impl-5: step 1 done: focused set green but size/unsafe budget (owed); redoubt-ipc attempts 4x fixed (WIP 071146ee3); reporting
- 2026-10-03T09:05:18Z impl-5: step 2 run done: audit check_object_indexes p50 37.9 ms/destruction (the constant), handler sans audit ~22.6 ms; find_process 2816 visited/42 live, migrate_held_pids 512/8; table in .k16/report-items12-gate.md
- 2026-10-03T09:14:23Z impl-5: step 3 WIP 3f4630826 (process-object PID set: find_process 2816->42 visited, migrate_held_pids 512->8, holds_process->live); uninstrumented gate rerun
- 2026-10-03T10:08:12Z impl-5: model R4a mutations missed at 256 open calls (flood server 200 pages; crowd 3..8); scaled in model/src/check.rs, R4a caught; full model run next
- 2026-10-03T10:28:38Z impl-5: model-host-tests PASS 427.6 s after flood + traces sizing (WIP); sizes next, then re-roll
- 2026-10-03T10:33:41Z impl-5: re-rolled: c4f2fbb55 stride (Size budget libs/stride 329->332), a9314dc92 c5 (walks folded in; kernel 7875->7927); backup branch k16-wip-6; final focused+host run (.k16/final-summary.txt)
- 2026-10-03T11:02:42Z impl-5: tip 827dbb8b4; all focused/host/build green; reporting
- 2026-10-03T11:16:54Z impl-5: rebased onto main f8c1543f3: tip 19102cb9b; gate share 831, R10 20674/25873; budgets/docs/fmt/sched-latency/process-lifecycle/pid-reuse PASS
- 2026-10-03T11:19:45Z impl-5: c6 process-fill committed 390d9fdc1 (PASS both widths)
- 2026-10-03T11:21:17Z impl-6: started; FSD2 bench running, editing round-1 notes first
- 2026-10-03T11:23:56Z impl-6: round-1 notes folded (c1,c2,c3,c5); tip dd0840c73
- 2026-10-03T11:27:04Z impl-6: c7 thread-limit drafted (child holder in own budget); waiting for FSD2 bench to run cases
- 2026-10-03T11:29:20Z impl-6: c7 thread-limit committed ac42c55d1 (PASS both widths, docs PASS)
- 2026-10-03T11:35:09Z impl-6: c8 drafted (walk-trace feature, oracle M/m, worst-walk case rv64 4608 MiB); asked orch re release vs trace (R23); INIT4 bench running, holding runs
- 2026-10-03T14:30:11Z impl-6: ASID ruling applied: MAX_PROCESS_COUNT 511 + assert into c5 (19760c03a), process-fill 509 into c6; c8 WIP 248031a17; worst-walk run at 512 still in flight
- 2026-10-03T15:07:13Z impl-6: NOTE main rewritten: base f8c1543f3 is now fa858cbde; at the end, on the orch's word only: git rebase --onto a8cbbc7ad f8c1543f3 wp-k16
- 2026-10-03T15:24:41Z impl-6: handing off (K16-handoff-6.md); tip 248031a17 (c8 WIP); worst-walk run pid 2422434 in flight
- 2026-10-03T15:28:52Z impl-6: worst-walk numbers in .k16/report-c8.md: R10 11.7 s net -> stop-and-report sent
