B14 (sched-share at the 1 ms slice) is accepted and merging: head 67746b417 on main ba4aabd8b. Nothing is open on the branch. Report: .wash/local/B14-report.md.

What should outlive this member, in /home/mcloonan/redoubt/.wash/local/B14/: trace-analysis aids (Python, not in git) for a sched-trace console log. Each reads `SCHED-TRACE seq entry kind id pass` records and the program's `SHARE <name> <start> <end> ...` windows.
- charges.py LOG WEIGHT: each budget's charge in each SHARE window (pass rises x weight / STRIDE, in µs), its picks, its timer-entry B charges, and the window net of audits. The gap between the two is time charged to nobody. It found threads-exit's 7.5%.
- victim.py LOG WEIGHT BUDGET SHARE: one budget's charges by context (who was running, inside a timer entry or not). Only rough: a fold can land after the next pick.
- absorbed.py LOG WEIGHT BUDGET SHARE: what a wake's floor lift absorbs of a budget's charges made out of the queue.
- The three console logs are main fdafcf2cb's (before K24 and K25), kept as the before picture.

Traps for the next member:
- jobs.mk's prebuilt index is fingerprinted on the worktree, untracked files included. Keep scratch files outside it (/tmp), or every case reports a stale index. Rebuild the prebuilt after any edit, docs included.
- The bench now keeps only the newest run directories (about 18). Read a case's console log right after it runs.
- A case without an oracle (sched-share) prints no window. To trace one, use a scratch worktree with kernel_features = ["sched-trace"] added to the case file, and CARGO_TARGET_DIR outside the scratch.
- doccheck C5: the first citation of R12 on a page must read `R12 (scheduling)`.
