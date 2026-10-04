# K16 item (1): 511 ASID ruling in c5, reruns (k16-implementer-7)

Tip ff3ad8464 (WIP c8 on c1..c7), base f8c1543f3. c5 is f6ff9cd8c.

## In c5
Already there from impl-6: MAX_PROCESS_COUNT = 511, ASID_BITS 9/16 with the compile-time assert
(kernel/src/arch/riscv/process.rs:82-92), memory-layout.md's `satp` paragraph in the ruling's
words, processes.md:44, and every PID count (budgets 510/511, ipc/timer 511 x 255, objects,
kernel-attack-gaps, process-fill 2..=511 / 509, pid-reuse-authority and proc-lifecycle comments).
Added by me (fixup into c5, autosquashed):
- memory-layout.md `satp`: the ruling's text had been spliced in as one 182-column line; reflowed.
- model.md:203: c5's edit left a 138-column line; reflowed.
- c5's message: the gate paragraph now gives the 511 numbers below; the slots-visited figures say
  "measured at 512 PIDs" (they were).
Left: `[false; 512]` in proc-lifecycle / pid-reuse-authority is indexed by PID 0..=511 (right).

## Reruns at 511 (cargo testbench --allow-skip <case> via in-dev, serial, no whole bench running)
- process-fill: exit 0, PASS rv64 4.0 s, rv32 4.1 s
- thread-limit: exit 0, PASS rv64 1.2 s, rv32 1.3 s
- pid-reuse-authority: exit 0, PASS rv64 4.3 s, rv32 3.8 s
- kernel-containment --arch rv64 (qemu seed 13): exit 0, PASS 219.5 s
  - share 829 of 1000 (floor 783)
  - R10 18 destructions p50/p99/max 20,740/25,253/25,253 us; no audit inside one
  - budget_destroy call to return 55,344/55,590 us (recorded)
  - driver_wake net p50/p99 8,635/11,103; timer_wake 7,760/8,817; decision_wake 7,691/7,708;
    deadline_notice net p99 28,680 (target <= 40,000); lease end 7,708 + 25,253 = 32,961 (<= 125,000)
- docs: exit 0, PASS
Logs: .k16/r511-*.log, summary .k16/r511-summary.txt.

## (2) c9: no change needed (c5: depth 15 ok, 16 TooLarge, destroys chain[1], checked).
## (4) round-1 notes: none remaining (all applied; P2-4 kept, reason in impl-6's report).
## (3) worst-walk at 511: running detached since 08:53 (.k16/r511-worst-walk.log); report follows.
