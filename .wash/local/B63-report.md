# B63: sched-capped's second cap at 3 harts judged on the host-clock twin (idle2-implementer, 2026-10-15)

Branch wp-B63, worktree /home/mcloonan/redoubt/.worktrees/B63, base 86f51e6fa (main after train 26),
one commit 2404fd945 (tests + docs only), tree clean. Ruling: kernel-red-2 on IDLE2-sched-capped
(event c5f9f427, four points); finding and pin in .wash/local/IDLE2-report.md 'Reopened' and
'Ruling's evidence'. Scratch: .tmp/B63/ (gate.sh, gate.out, twin-<w>-<i>.log + -smp3.log, capped-<w>.log,
gate-*.log, testbench-tests.log, msg.txt).

## The change
1. tests/sched-capped-mttcg.toml: smp = [2, 3] (keep_smp; quiet class, one at a time among quiet
   jobs as before); description names the second cap judged at three; comment quotes the ten runs.
   Nothing else: the oracle judges `50@3` shares on a 3-hart trace by itself.
2. tests/sched-capped.toml: post_check hart_shares_from=3 -> 4 (two- and three-hart shares recorded,
   four judged: spread); description; the comment says what the 3-hart icount read measures (no
   timeout in the window, three slice ends meeting at the lock in QEMU's fixed order, 6,300-6,600
   ticks a section, the last hart losing 23-45 %), with main's and IDLE2's 5-run numbers, and that
   spread at four is judged still (10/10 both trees) with the same exposure.
3. No bound moves: 283..383 is the twin's target as it was the icount case's.
4. docs/kernel/scheduling.md: "A hart waits for QEMU's turns" gains the second cap at three as an
   instance with the numbers and the twin's judging; the R12 cases paragraph (two and three harts
   recorded); the hart-count list (sched-capped-mttcg two and three). docs/testbench.md: the
   hart_shares_from note (4; two- and three-hart shares judged by the twin).
Summaries checked, no change needed: docs/README.md, docs/kernel/README.md (no case list),
SECURITY.md R12 row (names sched-capped and the twin without hart counts), README.md,
GETTING-STARTED.md.

## Gates on 2404fd945 (the tree as committed; runs made after the two structural toml edits, the
## comments and pages edited after the runs; nothing else ran on the quiet set)
Twin, five runs a width alone on the quiet cores (`q run --quiet`, cargo testbench --exact --arch
<w> sched-capped-mttcg), PASS 20/20 (smp 2 and 3). At 3 harts (60M-tick window), second-a /
second-b / second-c / second-e and the heavy budgets' lock waits of the total:
rv64: 327/328/114/114 (1.12M, 1.02M of 2.14M); 327/328/114/114 (1.14, 1.11 of 2.26M);
327/328/114/114 (1.13, 1.07 of 2.21M); 328/328/114/114 (1.06, 1.09 of 2.16M); 328/328/114/114
(1.06, 1.13 of 2.21M).
rv32: 327/328/114/114 (1.16, 1.08 of 2.24M); 327/328/114/114 (1.16, 1.10 of 2.28M); 328/328/114/114
(1.17, 1.13 of 2.36M); 327/329/114/114 (1.16, 1.02 of 2.18M); 314/315/123/123 (5.59, 5.51 of 13.9M:
a run with six times the waits beside the machine's general-core load; still 30 inside the target).
The 10-weight budgets' waits under 26k ticks each (one 25k).
sched-capped (icount) both widths, all smp: PASS 6/6; the 3-hart lines now "judged from 4 harts:
recorded" (second-a 285 / second-b 350 on rv64, 323 / 326 on rv32), spread-a-4 773 (owed 750) and
spread-b-4 judged at 4. docs PASS 5.3 s, formatting PASS 19.2 s, size-budget PASS, cargo test -p
testbench 196 pass (the oracle's hart_shares_from tests among them).
Earlier evidence (scratch trees, not this commit): main 146db8897 and 87553025a each 10/10 on the
twin at smp [2, 3], 327-329 (.tmp/IDLE2/twin3/); the icount pin 2/5 and 3/5 misses (.tmp/IDLE2/pin/).
