- 2026-10-02: read SWARM sections, brief, evidence. Early checkpoint done on main 81b5ea38b: rv32 seed 3 churn shell 499 (ring 8192) -> 559 (16384); alloc_frame mean scan 8806 -> 16997. No kernel change. Detail: .worktrees/k21/.wash/local/K21-checkpoint.md. Waiting for go-ahead.
- 22:06 free list + rollback walk built (uncommitted); churn seed 3: rv32 499/rv64 506 (ring 8192), 492/490 (16384). Destruction audit 14.8->70 ms (free-list walk, stamped). dma question sent. Next: scan-bounds.
- 22:17 committed 125b5c016 free list, 81f8ecaab rollback walk, 0ca182093 scan-bounds (+negative alloc-first-fit fails map_anon both widths). Waiting on dma decision; then sched-latency, churn re-measure, ask for whole bench.
- 22:29 e49d1dbbf DMA pool committed per ruling. Tip churn 497/493 (8192), 492/502 (16384); latency PASS both widths. Asked to run whole bench. Detail .worktrees/k21/.wash/local/K21-report.md
- 23:04 whole bench on e49d1dbbf: exit 0, 282 PASS, 1 SKIP (bench-ssh-loopback-openssh), 0 FAIL. Reported.
- 23:06 rebased onto d45ef88eb -> 3d1aac387; conflict only kernel_frame (merged). fmt/doccheck/size/unsafe ok; scan-bounds, dma*, churn (493/502) pass.
- 23:13 fix round 1 amended into each commit; tip 7a9225bb8. fmt/doccheck/size/unsafe/scan-bounds/dma*/churn (493/502) pass.
- 23:58 rebased onto 5f9f9d61c -> 155a38c54; whole bench run (see tally).
Sat Oct  3 12:21:34 AM PDT 2026 instr run after (a) done
2026-10-03T00:23:33-07:00 k21-implementer-2: (c) measured, (a) amended -> tip d0af5ff50; gate PASS 28286/28583 but thin margin; reporting fallbacks. Detail .worktrees/k21/.wash/local/K21-r10-free-cost.md
2026-10-03T00:25:08-07:00 k21-implementer-2: running scan-bounds/churn/dma on d0af5ff50; then main baseline kernel-containment seed 13; bitmap estimate drafted
2026-10-03T00:36:46-07:00 k21-implementer-2: bitmap (4) rebuilt as 4 commits on 082e00ccf, tip b0fcee1f2 (detached); running gate
2026-10-03T00:49:57-07:00 k21-implementer-2: bitmap gate p99 28298/29029 (> main+2ms); reporting before merge
2026-10-03T01:22:59-07:00 k21-implementer-2: lean mark_free -> R10 within 1 ms of main; tip 5d7e55bfc; running gate+cases
