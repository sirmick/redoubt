SMP2 handoff (smp2-implementer-2 -> successor). Branch wp-SMP2, worktree /home/mcloonan/redoubt/.worktrees/SMP2, off main 36d1450f9 (main is now 415c86ad6 and later: rebase before the gate). Worktree CLEAN at WIP 6b97ea734. Nothing pushed. Scratch: /home/mcloonan/redoubt/.tmp/SMP2/. Notes: .wash/local/SMP2-notes.md (built and results), .wash/local/SMP2-page-drafts.md (page text to apply). Predecessor handoff and checkpoints: .wash/local/SMP2-checkpoint*.md, .wash/local/handoffs/.

## BRANCH (oldest first)
1-6: as in the first handoff (wfi lock wait; shootdown halted wait; oracle per-hart picks + H/J/Q/F; model H harts + R12 mutations; stride cap set; kernel cap wiring + pages for R12). Real commits.
7. beb8ec133 WIP oracle water_fill + CHARGED-SHARE across harts.
8-12. ad623d74c, 640246720, 2d55097cc, 5b4fed278, 6b97ea734: WIP hart shares (see below). FOLD into family commits before review (one per family: the oracle + kernel trace records [H weight, `u` UNCAPPED, stride hook `uncapped`]; spinner shares; churn victims; deadline-flood; restatements/keep_smp; latency). `uncapped` hook belongs with commit 5 (stride) or the oracle commit; kernel H-weight and UNCAPPED with the oracle commit ("testbench, kernel: ...").

## WHAT THE WIP HOLDS
- sched_oracle.rs: Weights (H pass = runner's stride weight; G; lift child), window_records, charges() shared by CHARGED-SHARE and HART-SHARE; HART-SHARE `name start end tol[+|-] mark[:k] w[:k]...` (whole = all charged net of all lock waits; part = mark's budget + lifted, net of its harts' waits; want = water_fill normalised); `u` record restarts the count; stale_waits_in and nobody's counts per HART-SHARE; SHARE (count-based) and Bench::judged_share REMOVED; check_wake_no_preempt per hart. Tests: a_hart_share_is_judged_of_every_charge_against_water_filling (+ u lift, 1/2 harts, sides, malformed), timer-interrupt test on HART-SHARE windows, 2-hart wake proof. 49 oracle tests pass.
- Programs (counts -> notes, mark(), Bench::hart_share): share, large-weight, idle-gap, sleep-gaming, exit-churn, budget-churn (+ new bin sched-budget-churn-shell via sched::churn_against_victim), timer-flood, carve-return, carve-inflation, deadline-flood-billed; ties check "highest id last"; wake-no-preempt 4 spinners, delay a note; sched-latency head start 600 ms, smp=[1,2], N=16 server share a note only (HART-SHARE can't model the burst-runnable steward; told orchestrator).
- tomls: release twins (share-release, large-weight-release, deadline-flood-billed) keep_smp + count regexes; new deadline-flood-billed-traced (checked; keep_smp); budget-deadline keep_smp DROPPED (12/12 pass at 2 harts over seeds 1-6, both widths).
- keep_smp (with reasons): cluster, cluster-old-control, carve-return, budget-churn-shell, lift-delay, wake-no-preempt, large-weight, deadline-flood-billed-traced, kernel-containment (containment NOT acceptable per ruling; see below).

## RESULTS
1 hart both widths: every changed case passes. 2 harts both widths pass: share, idle-gap, sleep-gaming, exit-churn, timer-flood, carve-inflation, ties, budget-churn (4 variants), debt-lift, server-busy, destroy-billing, budget-deadline; sched-latency 16-seed sweep at 2 harts: 0 target misses on either width (worst N=16 timer p99 17.4 ms rv64); sched-latency at 4: rv64 PASS, rv32 failed only on the old count share (now a note: rerun). Tables: .tmp/SMP2/table.py <smp> over .tmp/SMP2/pc (partly overwritten); sweeps in .tmp/SMP2/sw/.

## RULINGS RECEIVED (orchestrator, 2026-10-08)
- Audit credit 1(a) was ruled, then I showed it wrong (deadline-flood 1 hart victim 78); final: large-weight rv32 keep_smp with residual (c) (release+trace impossible: test-only features need checked).
- Shell split into its own case, keep_smp only it (done); residual on page with 326/406, waits 848 ms of 2 s.
- carve-return keep_smp (done), page reason.
- Containment MUST judge at 2 harts: first see if J/H/Q records can be cut so the ring fits at 512 MiB; else grow sched-trace-large (256 MiB?) and memory_mib, re-pin the seed, say why, check cases sharing memory_mib.
- Page: one table of every keep_smp case, reason, class (lock waits -> SMP4 un-keeps; audit placement; scenario meaningless at N harts), and which 2-hart case covers each kept property (or say none). SMP4 list sent: shell, deadline-flood-billed-traced, wake-no-preempt.

## LEFT
1. Containment ring at 2 harts (keep_smp is on it in the WIP; the ruling says it must judge at 2). Measured (rv64, smp 2, log .tmp/SMP2/kc2.log, verdict otherwise PASS): 7.09M records needed, ring 6.29M (192 MiB), 798k dropped. Kinds: J 26% (every J is a real 0<->1 change), H 14% (none redundant), Q 7%, P/B/K/I/O/R ~7% each, U/V 5% each. Cutting is not cheap (verdict reads J/H picks, Q shares, U/V audits, I/B/O billing; merging U+V into one record saves only 324k). So ruling (b): sched-trace-large PAGES 49152 -> 65536 (256 MiB, 8.39M records), memory_mib 512 -> 576 to keep the budget tree's RAM; worst-walk also uses sched-trace-large (check it); re-pin containment's seed (now 13) with a sweep, say why in the commit, remove keep_smp.
2. Pages (drafts in SMP2-page-drafts.md): testbench.md HART-SHARE text + status lists; scheduling.md keep_smp table, residuals, Responsiveness numbers at 2 (gated) and 4 (recorded), "Measured on QEMU" retitle, R12 attack list, status lists (+sched-budget-churn-shell, deadline-flood-billed-traced); SECURITY.md R12 row tests; m2 Progress sentence. doccheck.
3. New cases from the brief: sched-capped (2, 3 harts), sched-lock-contention (2, 4); VM case -> ask to cut.
4. sched-latency-tcg at 2 harts loses its N=16 line (guest exits first): not investigated.
5. Fold WIP, rebase on main, size-budget at every commit, fmt, unsafe, gate (both widths: sched-latency, kernel-containment, smoke, sched-*, smp-*, worst-walk) + --smp 2 sweep, report (.wash/local/SMP2-report.md, member_update assignment_results).

## TRAPS (new)
- Test-only kernel features (sched-trace) need debug_assertions = true.
- --sweep needs a case that pins qemu_seed; TESTBENCH_QEMU_SEED=n works with the prebuilt testbench.
- pc.sh copies console logs from target/testbench/run-*; concurrent runs prune them: copy at once.
- New program bins must be listed in tests/programs/Cargo.toml.
- Never edit the worktree while a non-prebuilt cargo testbench (sweeps.sh) is building.
- Mark weights must be unique among empty destroyed budgets in the trace (churn children weigh 1/50, deadline-flood 0, carve-return's own carve 999).

What consumed my context: the 1- and 2-hart batches and their trace analysis (shares.py), the audit-credit detour, the latency sweeps.
