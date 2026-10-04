# K21 early checkpoint: the finding reproduced on main (2026-10-02)

Base: wp-k21 = main 81b5ea38b, no kernel change committed. Tree clean after the runs.

Scratch instrumentation (not committed; saved as K21-scan-instrumentation.patch here): counters in
`alloc_frame` of calls, sum and max of the scan length (`index + 1`, slots `position` visits), and
the last one; reset after the trace ring's own boot allocations; printed as `K21-SCAN` before
`SCHED-TRACE-END`. The 16384 run also set sched.rs `trace::PAGES` to 16384.

Command, each run (exit codes as shown):

    TESTBENCH_QEMU_SEED=3 .wash/local/in-dev cargo testbench sched-budget-churn --arch rv32

| | ring 8192 | ring 16384 |
| --- | --- | --- |
| exit | 0 (PASS) | 1 (FAIL, shell only) |
| shell share net / gross | 499 / 292 | 559 / 306 |
| R10 destructions | 86 | 74 |
| object frames (high) | 8843 | 17035 |
| alloc_frame calls after boot | 1123 | 1045 |
| scan length mean | 8806 | 16997 |
| scan length max / last | 8844 / 8817 | 17036 / 17009 |
| trace records | 11026 | 10303 |

Matches the evidence file exactly (499 -> 559; 86 -> 74 destructions). Every post-boot
alloc_frame scans past the ring and everything below the first free frame: the scan grows by the
ring's added 8192 frames (8806 -> 16997 mean), on every call, billed to the caller.

Logs: k21-churn-8192.{log,console}, k21-churn-16384.{log,console} in this directory.
