# architect-10 notes

Watch lists from architect-9-notes.md stand (FSD2, K21, SMP3, SMP2).


- B7 merge check at 8bdca01ce: pages OK; commit message edit sent (drop the deps/ and environment alternatives, exact lines in the answer); OK after it, on the gate bench PASS.
- FSD2 rule-5 commit at 8a1267f12: pages and rule OK; cost finding sent (on-demand charge recursion O(n^3 L) per volume write, O(n^4 L) per tally; fix: one deepest-first pass with acc[]). OK after it.
- INIT4 merge check at d3538d345: brief lines OK; owed A (stale "does not boot" bullets, README:320) and B (netd/init statuses, testbench features line at rebase), in INIT4-merge-check.md. init-step-close.md drafted; apply on main when told.
- SMT FPGA ruling (f913630c5): shared TLB tagged by hart (priv spec), barrel strict for R12, pause in SMP1 spins; briefs SMP1-multihart/SMP1-implementer/SMP2-implementer edited. Watch at K16 merge: fpga-platform.md ASID bullet "already puts the process ID in satp" goes stale with ASID 0.
- FPGA ASID width 16 bits committed (owner decision). OWED at K16 merge: fpga-platform.md ISA-features ASID bullet -> ASID 0 interim, 16-bit PID is the tag (exact line in answer to orchestrator 29a5800f).
- K16 round 1 at 19102cb9b: OK + 2 edits (memory-layout satp "ASIDs designed in M2" -> no ASIDs/shootdown; fpga ASID bullet with 16-bit PID reason, supersedes my held line). Round 2 (c6-9) owed.
- K16 c8 ruling: A (rv64 checked + walk-trace, net of audits; walk-trace joins R23 list; page lines say "(rv64, checked build, net of its audits, N live threads across 511 processes)"). Check at round 2.
- init-step close committed on main (M1 page).
- beamlet-redoubt cut: BEAM1-5 (plan rev 311); BEAM1-implementer.md written. Rulings: one scheduler in M1; userland disk in BEAM2 (needs FSD3); BEAM5 in M1 (R59). Owner choices sent: h/1 docs strip (rec), rv32 beamlet after BEAM1 report. Briefs owed: BEAM2-5.
- FSD3 Q1 (b): range badge makes fsd a blkd user; init.md confinement lines; Q2 (a) endpoint=NAME required; fsd.md Arguments line, init.md:118 example, image manifest. Check at FSD3 merge.
- BEAM1 blocker: INIT_PAGES 2,048 + strip (budgets.md lines sent); INIT5 node cut (lend image from bundle, bound loses image term). Check at BEAM1 merge.
- INIT5 brief written (batched place, PLACE_PAGES 64, no lend; INIT_PAGES back to 1,024); needs BEAM1.
- RT2 merge check at a48e430a7: OK + 2 page edits (serve prose with other-error/keepers sentence; rewrap serving.md:421-422).
- BEAM1 thread::spawn unsafe accepted (rt 11; SAFETY + tests/thread.rs keeper; budget name/comment lines). rv32 builds (799 pages): owner rv32 question withdrawn; BEAM1 status drops "rv64 only" if both widths pass.
- BEAM1 heap flood: limits from budget (half each, heap+ets); cases beamlet-heap-flood (VM survives, exit 0) + beamlet-budget-flood (backstop, init restart); beamlet.md Limits status/sentence lines sent.
