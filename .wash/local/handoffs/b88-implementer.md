B88 handoff. Branch wp-B88 HEAD 3d33d9d13 (WIP verityd fix) on origin/main 31d9c1f31. block-trace instrument already its own commit 77538cdc2. Full detail+evidence: /home/mcloonan/redoubt/.worktrees/B88/.tmp/B88/findings.md (read it first). Trace reader: .tmp/B88/trace.py. env: `source .tmp/B88/env.sh`.

CAUSE (trace-backed, accepted): the rv32 two-hart wedge is the verified system-volume READ PATH (erofsd->verityd->blkd) throughput under two concurrent VM code-loads. NOT the steward (refuted), NOT a kernel lost-wake (refuted). Evidence (block-trace @ records, run target dirs in .tmp/B88/keep-*): budget ids steward 86 (ep 852), erofsd:system 72 (ep 851), verityd 69 (ep 850), shell VM 110, spinner VMs weight 50. Steward's outgoing calls block <=1.43s max, its only long block is recv on its OWN ep 852 (never stuck in a mint). N=3 wedge = two spinners (same Echo module tree) saturating the path.

MEASURED (steward-stall-measure diagnostic, seed 3, rv32 2-hart; AgentStarted, 99=no wedge; hit=held/req, blkd reads @ served=1024):
 no fix: 36 / 18.7% / 2704
 cache27 only: 36 / 24.4% / 2515  (cold-dominated; why cache alone fails)
 cache27 + read-ahead: 68 (rounds N=1,N=2 PASS; N=3 wedges) / 24.4% / 784
 cache92 + read-ahead: 68 (unchanged) / 41% / 624  -> more cache cuts blkd but does NOT move the wedge => N=3 blocker is compute, not I/O
 two-reader host test: small(4) held0 checked320 blkd229; big(27+ra) held180 checked140 blkd33.

FIX (per orchestrator steer, last two messages): cache sized from the manifest heap, read-ahead of a cold run. servers/verityd/src/: volume.rs (Cache now runtime-sized; Caps::from_heap = data=heap/2 - HEAP_RESERVE_BLOCKS(56), floor DATA_CACHE(4); Volume::load reads a consecutive missing run in ONE blkd call bounded by READAHEAD_BLOCKS=MAX_SECTORS/SECTORS_PER_BLOCK and the cache cap; peek; Cache::holds; ra buffer=one blkd read), server.rs (read() gathers run-by-run via load+peek), bin/verityd.rs (Caps::from_heap(startup.heap_pages())), tests.rs (two-reader test + Caps). Each read-ahead block is hash-checked in Volume::load (self.tree.check) BEFORE keep/serve (verity rule holds). READAHEAD_BLOCKS derived from MAX_SECTORS (no new magic constant). 16 verityd tests pass; both widths compile.

CACHE SIZE — DECIDE FROM THE RUNNING TEST: B88-d4-s3 (running now) tests data=4 (new formula at heap 100) + read-ahead. Check `grep -c AgentStarted $(readlink -f target/testbench/last)/steward-stall-measure-rv32-smp2.log` (and .tmp/B88/d4-s3.log). If 68 -> data=4 is the smallest that gives the win; if <68 -> use data=27. The orchestrator referenced 27 (==92). Pick the smallest that gives AgentStarted 68; say so in the report.

MEMORY GATE (key constraint): init-boot measures verity heap peak and requires heap_cap >= 2*peak. Peak with data=27 = 79 -> needs cap 166. Original peak 50 = cap/2 at cap100 (no headroom); the read-ahead buffer alone forces heap growth even at data=4 (peak ~56 -> cap ~116). So you MUST grow verity heap_pages. Non-data heap K~52 (tree32+scratch8+ra8+top1+misc3).

DONE: block-trace (committed); verityd cache+read-ahead (WIP 3d33d9d13); two-reader host test; cause+evidence (findings.md).
NOT DONE (next steps):
1. Read d4 result -> pick cache size.
2. Set verity heap_pages in image/manifest.json + every memory-checked manifest (init-boot/userland-boot/*-footprint/steward-session-ends/steward-ssh-two-principals use image or own manifests; all had heap 100) + latency.manifest.json, to 2*peak(+margin) for the chosen data (116 or 166). budget stays 256 (no growth). Verify: `scripts/q run --cores 4 -- cargo testbench --exact --arch rv64 init-boot` passes.
3. Docs: docs/servers/verityd.md one paragraph (cache from manifest pages + read-ahead of a cold run + the arithmetic) and RESOLVE its "Read-ahead is not done" residual; docs/testbench.md memory-budget table verity peak 50->measured, cap 100->chosen.
4. size-budget.toml code delta (sum rule; budget 256 unchanged).
5. N=3 pinning number: spinner load 1 vs 2 concurrent, rv32 vs rv64 (one diag run each, or console/trace). State N=3 MISSED, cause compute-not-I/O; 5s start_agent target does NOT move.
6. Rebase onto origin/main (re-rebase after STEWARD3V lands ~this hour) and run 3V's host test `every_manifest_entry_holds_its_buckets_at_their_caps` (it may touch verity manifest entries).
7. Fold WIP into TWO clean commits: (a) cache-from-manifest, (b) read-ahead.
8. Gates: steward-latency rv32 2-hart with fix, KEPT (expect N=1,2 pass, N=3 miss); fmt `make -f scripts/jobs.mk set CASES=formatting`; docs `set CASES=docs`; unsafe-budget; size-budget; verityd cases. One steward-latency run at a time.
9. Report .wash/local/B88-report.md: cause+trace lines, fix, cases, measured rows, residuals: N=3 missed=compute-not-I/O (with the number); the steward's mints at shared servers wait within FOREVER (bounded disconnects, unbounded mints; max observed 1.43s) -> its OWN node; session-vs-lease scheduler weight -> the Architect (not this node).
Reviewer kernel-red-3.

TRAPS: local diag tomls tests/steward-stall-diag.toml + tests/steward-stall-measure.toml are UNTRACKED, NEVER commit them (they stay in the worktree for re-runs). No worktree edits while a case runs (tree changed). .tmp is git-ignored scratch, never stage it. Caps::from_heap formula is data=heap/2-56; adjust manifest heap to yield the chosen data.
