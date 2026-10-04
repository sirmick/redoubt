# GATE1 miss, fix round 1 (RED M2), 2026-10-02

Change: slot D's deadline now ends it while slot H still lives (H is destroyed after D's
notices), so two hostile leases are live at every deadline end. Everything else from the
round is in too (B1, B2, M1, L1, L2, simplifier 1/2, editor 1).
Command: `in-dev cargo testbench kernel-containment` (seed 4, both widths). Exit 1 on both.

Every protocol row is ok on both widths, including the stricter held-call row:
72 of 72 calls checked, 0 left. 0 slot D leases were made again. Share 821 (rv32) and
830 (rv64) against the floor of 783. The oracle passes.

MISSED: the deadline notice, p99 <= 40000 µs.

| | before (H destroyed first; seed 4) | after (H live at D's deadline) |
| --- | --- | --- |
| rv64 notice net p50/p99 | 22340 / 24015 | 26155 / 57053 |
| rv32 notice net p50/p99 | 23901 / 31336 | 26607 / 48148 |
| rv64 gross p50/p99, audits in windows | 46472 / 54726, 0.49 s | 85057 / 126833, 1.16 s |
| rv32 gross p50/p99, audits in windows | 53205 / 66698, 0.50 s | 87508 / 119635, 1.19 s |

Gross samples alternate by lease: the agent's notice comes about 85 ms after the deadline and
the sub-agent's 106-127 ms after it. The p99 (18 samples, so the maximum) is a sub-agent
notice. Wall time 195 s on rv64 and 158 s on rv32.

The rest of the post-check (R10, lease end, the wakes) was not captured for this run; rerun with
stdout kept if needed. The sweep was not rerun: the pinned seed already misses on both widths.

Per the owner's rule (QA GATE1-design), GATE1 stops: no target widened, the attacker not weakened.
State: WIP commit on wp-gate1 holding the round's changes.
