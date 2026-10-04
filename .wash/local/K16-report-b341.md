# K16 report for assignment b341a4ba: commit 4 bench, rv32 lease-end rerun

All commands via `/home/mcloonan/redoubt/.wash/local/in-dev`. Scratch worktrees (detached, never
committed): `.worktrees/k16-main`, `.worktrees/k16-b`, both clean at 277a2c060 now.

## Commit 4 (277a2c060): whole bench is NOT green

`cargo testbench --allow-skip` (log `.k16/whole-c4.log`): exit 1. PASS 277 (rv64 134, rv32 109,
34 host/width-free), SKIP 1 (bench-ssh-loopback-openssh: podman not installed, the expected one),
FAIL 2:

- `sched-timer-flood [rv64]`: "30 sleepers and 64 staggered budget deadlines: the victim got 445 of 1000"
- `sched-timer-flood [rv32]`: "30 sleepers, 1 us apart: the victim got 446 of 1000"

The bar is `vs + 50 >= 500`, i.e. 450. The same shares at seeds 3, 4 and 5 (the seed does not move this case).

Victim share (first case / second case, thousandths), seed 3:

| build | rv64 | rv32 |
| --- | --- | --- |
| main ca5a6437b | 463 / 453 | 462 / 456 |
| commit 1 91c893e29 | 463 / 453 | 462 / 455-456 |
| commit 4 (HEAD) | 456 / **445** | **446** / (not reached) |
| HEAD, stride reconcile reverted to the copy | 462 / 452 | 456 / 450 |
| HEAD, stride + mem.rs reverted | 462 / 451 | 458 / 450 |
| HEAD, stride + budget.rs + dma.rs reverted | 462 / 452 | 456 / 450 |
| HEAD, runnable list back on the stack | 457 / 449 | 453 / 443 |
| HEAD, in-place loop rewritten (skip None first) | 455 / 445 | 446 / - |
| HEAD + 400 nops at the end of sched::reconcile | 456 / 448 | 449 / - |
| HEAD + 800 nops at the end of sched::reconcile | 462 / 451 | 453 / 443 |

Reading: no hunk of commit 4 changes a decision (ids, seq, msg ids hand out the same values;
earliest_timeout of an empty account is never read, `account()` filters budget None; stride
reconcile walks the same slots in the same order). The in-place reconcile executes FEWER
instructions than the old one (rv32 disassembly: the old one does two 1,536-byte memcpys of the
slots and the same 64-slot loop; `.k16/dis-k16*.txt`). Padding the kernel's reconcile with nops
moves the victim's share by about ±10 in either direction, not monotonically. Under icount shift=3 (8 ns per
instruction) and 1 µs sleepers, the case's share depends on how long a kernel entry takes. Main sat 3-6
above the bar on the second case; commit 4's shorter entry lands below it. The victim's
calibrated rate also rose (rv64 24232 -> 24471, rv32 8558 -> 8709 iterations/ms), which alone
takes ~4 (rv64) and ~8 (rv32) off its share; its raw count fell 0.8% (rv64) and 1.8% (rv32).

Not a fix I can make inside commit 4 without improvising: see the question in the report.

## Commit 4 sizes (release kernel, bytes; unchanged from the handoff, `.k16/sizes-c4.txt`)

| width | image before -> after | .data | .bss |
| --- | --- | --- | --- |
| rv64 | 221,302 -> 131,278 | 89,592 -> 32 | 2,096 -> 92,680 |
| rv32 | 222,666 -> 138,912 | 83,916 -> 32 | 2,056 -> 86,972 |

Stack: sched::reconcile's frame on the rv32 checked build goes from over 2.7 KiB (two 1,536-byte
copies of the slots) to 192 B;
`Sched::runnable`'s list is in SCHED; process_start keeps one MAX_START_HANDLES array beside the
slots; read_slots keeps no frames array. Size ceilings: kernel 7752 -> 7769, libs/stride 328 -> 329,
`Size budget:` reasons in the commit message.

## rv32 sched-latency lease end p99, seeds 3/4/5 (target <= 125000; decision wake p99 <= 95000)

| build | seed 3 | seed 4 | seed 5 |
| --- | --- | --- | --- |
| main ca5a6437b | 47,000 (dw 40,424) | 47,071 (dw 40,427) | 57,816 (dw 51,362) |
| commit 1 (handoff) | 57,193 (dw 51,364) | - | - |
| HEAD 277a2c060 | 23,874 (dw 18,052) | 34,745 (dw 28,866) | 23,800 (dw 18,051) |

It is phase: the N=16 net decision wake p99 lands only on ~18.05 / 28.87 / 40.42 / 51.36 ms (steps
of ~10.9 ms), and main itself reaches 51,362 at seed 5. R10's share moves smoothly (6.5 -> 5.8 ms).
All runs PASS, exit 0. Logs `.k16/sl-main-s*.log`, `.k16/sl-head-s*.log`.

## After the rebase onto main 31dfad3f0 (orchestrator's 19ef/8789 instruction)

wp-k16 rebased clean: c90fd545f (commit 1), df074272d (commit 4). `cargo testbench
sched-timer-flood`, net share (post-check) / gross, thousandths; floor 450.

| build | rv64 sleepers | rv64 +deadlines | rv32 sleepers | rv32 +deadlines | exit |
| --- | --- | --- | --- | --- | --- |
| main 31dfad3f0 | 469/469 | 472/464 | 468/468 | 471/468 | 0 |
| commit 1 c90fd545f | 469/469 | 467/460 | 468/468 | 466/463 | 0 |
| commit 4 df074272d (branch) | 461/461 | 464/456 | 452/452 | 451/447 | 0 |
| commit 4, stride reconcile reverted to the copy | 465/465 | 466/458 | 463/463 | 461/457 | 0 |

Logs: .k16/tf-main31.log, tf-c1r.log, tf-rebased.log, tf-r-nostride.log.

## After the rebase onto main 81b5ea38b (K20 merged)

Tip 1c440d414 (commit 4), c1471de17 (commit 1). Conflicts resolved:
- message.rs next_timeout: K20's `stale`/`found` and `Timeouts` return kept; iteration is commit 1's
  (`pids()`, `mm.live_tids(pid)`).
- sched.rs leave(): K20's block kept whole (payer, timer-tail-billed, billing reopened for the
  payer); its `s.reconcile(mm, runnable)` becomes commit 4's free `reconcile(&mut s.cpu, mm, runnable)`,
  in K20's order (after the billing lines), `next.is_some()` returned.
- tests/size-budget.toml kernel: main 7852 (at ceiling); commit 1 7847; commit 4 7880 -> ceiling 7880.
  Commit 4's message corrected: "5 lines lighter, the kernel is 28 over its old ceiling" (was 16/17,
  which measured headroom under the old ceiling that K20 has since used).
fmt clean; release builds rv64/rv32 OK; size-budget PASS.

sched-timer-flood (net/gross): rv64 sleepers 491/491, +deadlines 494/486, cancelled-waits 491/491;
rv32 488/488, 490/482, 490/490. exit 0. Log .k16/tf-k20.log.
sched-latency seed 3: rv64 R10 p99 5620, lease end 34058 (dw 28438); rv32 R10 p99 5825, lease end 56455
(dw 50630), all targets met; sched-latency-tcg PASS both. exit 0. Log .k16/sl-k20.log.
