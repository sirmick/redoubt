# architect-9 notes

Owed edits and watch lists from architect-8-notes.md stand (FSD1, K21, K16, RT1, RT2 merge checks;
M1 edit at the init step's close; K19 with the owner).

## Done this stretch

- K16-data-region-ruling.md checked: it is the ruling. One fix: headroom about 570 KiB rv64,
  580 KiB rv32.
- INIT3 page check at d45b6e5d7: stands, with three stale status lines to fix before merge
  (netd.md:169, keyd.md:182, blkd.md:276; exact lines sent to the orchestrator).
- SMP3 brief: `SMP3-implementer.md`. Ruling: a thread that moves needs no fence.i of its own
  (one user entry per frame + W^X; any EXECUTE install shoots the process down, and a hart
  fences before running a process). The node's "on a hart a thread moves to" is dropped, and
  m2 step 4 is reworded by SMP3.
- SMP2 brief: `SMP2-implementer.md`. Ruling: R12 across harts is water-filling; the floor
  leaves out capped budgets (w*H > k*W, iterated in descending w/k over at most H-1); a budget
  leaving the cap is lifted to max(own, floor). At one hart nothing is capped. Why: with today's
  floor, a heavy one-thread budget on 2 harts holds the floor down and a late waker starves the
  others. SMP2 has a checkpoint: model scenarios (late join, second cap, uncap, spread) under
  the old floor and the new rule, sent to me before kernel work.

- K21 round 3: (ii) raw slice STANDS (corrected: -2.7 ms p99 uninstrumented; committed 135e99cdf
  with architect-8's SAFETY text). Remaining +3.2/+3.8 ms being profiled. Rule sent: (ii) stands only if no-flush via kframe
  stays >1 ms above main's p99 (22.4/22.5) on either width; else it falls (round 4's numbers). Told the orchestrator: if the
  profile blames the per-page flush, a destruction's walk of a space no hart runs needs no flush
  (one satp switch + flush if it is current); memory-layout.md residual line given.

- FSD1 merge check: OK at 5059ce847 (same tree as 9e17379a8). Owed later: files.md rename habit.

- RT1 merge check at a6a09ffc8: OK, 10 unsafe verified; one rewrap of serving.md:527-530 owed.

- K16 commit 5 gate failure (512 PIDs): pre-ruling in K16-walks-ahead.md (live-PID mask, dense
  stride queue, marked reconcile if needed; 512 stays unless a walk cannot follow live objects,
  then owner decision_request). Attribution: PID count alone fails the share (758 vs 821 at 64).
  budget_destroy 31.6 ms non-PID constant: asked for visited-vs-live counts per destroy loop
  (table-size walks are ruling 2's, converted in K16) and R10 trace time vs pre-c5 17.1/20.2.
  Answered: R10 at 64 PIDs + new values 20.8/25.8 ms (in target); the 31.6 ms is likely the
  checked build's audit after R10_END (excluded per K18/B5), to confirm in one timed run. At
  merge: R10 within 30 ms, the audit confirmed, visited-vs-live run done after item 2.

- FSD2 gaps: FSD2-gaps-ruling.md (all pairs checked; pair count + SPLIT_PAIRS allowance;
  live root's dir never removed/renamed over; minting-above reading confirmed).

- K21 round 4: slice stands (+5.8/+6.5 without); tip +0.54/+0.99 over main; no flush change, no
  memory-layout edit. Tip 0f1590e05 (+ unsafe-budget reasons() prefix-match fix).
- INIT4 net-attacks: badge counting WITHDRAWN (init's sizing gives each badge a bucket); (a):
  bucket lines dropped from net-attacks and bench-net-self-unrefused; host test proves R60's
  no-bucket; no page change.

- K21 merge check: OK at 55e1e73f7, conditional on red's free-path flush trace.

- FSD2 red BLOCK (mint frees room): rule 5 amended, charge = max(quota, held+reserve); tests
  and page lines in FSD2-gaps-ruling.md. Re-check pages after the fix round.
- FSD2 merge check: OK at 28bf55ec6 (superseded by the BLOCK fix) once `one_commit_splits_only_as_far_as_its_room` is added.

- Handed off: architect-9-handoff.md.

## Watch at merge

- FSD2: the gaps ruling's page lines; SPLIT_PAIRS derived with reasons; forged chain images.

- K21: live-space paths still flush per page; R10 vs main; commit message does not claim R10.

- SMP3: the removal-site list is complete (every place that clears or narrows VALID); the
  smp-no-shootdown negative fails all variants; the lend-within-one-process residual is gone.
- SMP2: the model's cap set is computed independently (full water-filling); no target moved
  without me.

## architect-10

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
