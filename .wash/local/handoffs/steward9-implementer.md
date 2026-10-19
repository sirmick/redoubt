STEWARD9 checkpoint. Branch wp-STEWARD9, WIP cbf4ae16e on origin/main dfbf2eeba (STEWARD11/train 76). Tier A (probe marker-end change). FULL detail, run commands, findings, and the exact pending-prose list: .tmp/STEWARD9/notes.md (read it first). Findings evidence: .tmp/STEWARD9/{sequential-ring-finding,cluster-deadline-notice-finding,cluster-wrongobject-finding}.md and rv32-dense-smp1-N15.log, rv32-14-FAIL-timerwake.log.

BRANCH STATE: the design is FINAL and the code/structure functionally DONE and committed:
- servers/steward/src/bin/steward.rs: probe ends window on the latency-done marker, LATENCY_ROUNDS const removed (CONFIRMED: reboot cascade works, dropped 0 at 1 hart). Builds clean with latency-probe.
- Three cases: tests/steward-latency.toml (N=1,2,3, both widths), tests/steward-latency-n4.toml (N=4, both widths), tests/steward-latency-n16.toml (N=16, arch=["rv64"] only). Session rounds + expect_after + arch are correct (verified). Per-round sequential leases 8/10/11/14/14s, sleep lease+1.
- docs/kernel/scheduling.md: the deadline_notice/residual/two-boot section rewritten (but still says the OLD two-case "steward-latency-dense" split -> needs the three-case update + the table rows).

TRAPS:
1. PROSE is stale (functional split done, prose not): steward-latency-n4.toml and steward-latency-n16.toml still carry the dense (N=4,16) description/comments; steward-latency.toml and scheduling.md name "steward-latency-dense". Rewrite per notes.md section "PROSE POLISH". No run depends on it, but the reviewer/docs will.
2. rv32 2-hart is BLOCKED on findings and is NOT to be gated there: rv32 2-hart N=2 timer_wake p50 31.8ms (reproducible, its own node) and (old dense) N=16 drain-rate (READ1). The Architect is ruling the MECHANICS (per-arch smp bench field vs duplicated rv64-only 2-hart toml) on thread STEWARD8-bound; smp is case-level today (main.rs:514). Do NOT finalize until that lands.
3. Runs WEDGE after a FAILING boot (qemu hangs ~7000s); kill by cwd, read the kept SCHED-TRACE-END. PASS boots exit clean.
4. No edit while a case runs; remake prebuilt after any tree edit; detached setsid for >1.5h cases.

REMAINING: (a) prose polish; (b) await the rv32-2hart gating ruling and apply it; (c) run the three cases rv64 (1,2) + rv32 (1[,2]) for clean gate logs (rv64 data already PASS in the two-case form, same numbers); (d) fill scheduling.md's table incl rv32 2-hart findings; (e) gates (notes.md list); (f) rewrite .wash/local/STEWARD9-report.md (cluster-era interim exists) with the six findings. Reviewer steward-red-5.

Context ~62%, checkpointing per the orchestrator's 65% rule.
