# GATE1 — the stand-in's wake after Y, from the trace (rv64 seed 3, 2026-10-01)

Same trace as GATE1-notice-split.md (target/testbench/kernel-containment-rv64-smp1.log, run at
d806b8a43's instrumentation). No new run, no change. Correction to that file: "entry - deadline"
is µs, so the stand-in entered take_notices(D) ~32.6 s before each deadline (r0 66 s), not ms.

## Budget map
- Stand-in = **42**: the last K before each of the 9 non-D X records (its budget_destroy(H)) is 42,
  9 of 9. By creation order in kernel-containment.rs: 41 driver, 42 steward stand-in, 43 victim,
  44 sessions, 45 bystander. 2 = the program's (launcher's) budget: it is woken by every report.
  The leases are the large ids (D tops 4157, 12359, ... 69773; the next round's lease right after).

## Per D lease, Y to the stand-in's two reports (CT_SPLIT_A/B, sent after take_notices(D) returns)

| r | X, Y kernel entry | steward W after Y: pass (= floor) | picks from Y to the 2nd report's wake | steward R/D/W there |
| --- | --- | --- | --- | --- |
| 0 | 335076 (one entry) | 68ce42f70356, = sessions' pass after the lift (A 44) | 42, 2, 43, 42 | W R |
| 1 | 582956 | b20f6f55958f, same | 42, 2, 43, 42 | W R |
| 2 | 830812 | already runnable (no W); victim 43 first at a lower pass (fb48f745 < fb4903b0) | 43, 42, 42 | R |
| 3 | 1078871 | 144833c5b1600, same | 42, 2, 43, 42 | W R |
| 4 | 1326712 | 18dc03d6b6ab1, same | 42, 2, 43, 42 | W R |
| 5 | 1574579 | 1d6fd4aaceeac, same | 42, 2, 43, 42 | W R |
| 6 | 1822613 | 2204ed3d42890, same | 42, 2, 43, 42 | W R |
| 7 | 2070488 | 2698ddb147bf0, same | 42, 2, 43, 42 | W R |
| 8 | 2318k (end of trace) | 2b2cc7369ead9, same; first K after Y is 42 | (trace ends) | — |

Reading (r1 in full, records 138436-138480): Y in entry 582956; in 582957 the victim (43) and the
stand-in (42) wake at the floor; 582958 K 42 (first pick, nothing ahead); 582964 W 2 (the
launcher: split A), R 42 (requeued at the launcher's wake), K 2; 582975 K 42; 582977 W 2 (split B);
582981 D 42; 582982 W 43 (victim_control CT_END); then 583004 G 44, the next round's lease. That
order is exactly the code after take_notices(D), so both notices were received, and time_now read,
within ~8 kernel entries of Y, the stand-in the first pick after Y, with no spinner or lease
picked in between. The only other picks were the launcher (2) and the victim (43), each for one
short receive. No R of the stand-in between its two notices; its one R is at the launcher's wake.

Yet time_now at receipt is Y + 24.4 ms (first) and Y + 32.6 ms (sub-agent's), constant to the µs
in 7 of 9 leases. So c is not a scheduling wait: it is time spent inside the stand-in's own
entries between Y and its receive returning (kernel work after Y on the notice/exit path, outside
X..Y, e.g. the per-process teardown and its "[!] Terminating process" console lines), or Y's
timestamp is taken before work R10 should cover. Inference: the two W 2 are the split reports (the
order matches the code; not confirmed by payload).
