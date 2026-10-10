SMP2 handoff (smp2-implementer-2 -> successor). Full text: /home/mcloonan/redoubt/.wash/local/SMP2-handoff2.md (read it first). With it: .wash/local/SMP2-notes.md (built and results) and .wash/local/SMP2-page-drafts.md (page text to apply). The first handoff's TRAPS still apply: size-budget lines and their chain, fixup and autosquash, nightly fmt, doccheck, KernelCell, never /tmp.

Branch wp-SMP2, worktree /home/mcloonan/redoubt/.worktrees/SMP2, off main 36d1450f9. Rebase on main before the gate. The worktree is CLEAN at WIP 6b97ea734; nothing is pushed. Commits 1-6 are real. Seven onward (beb8ec133, ad623d74c, 640246720, 2d55097cc, 5b4fed278, 6b97ea734) are WIP, to fold into one commit per family.

Done:
- HART-SHARE in the oracle: whole = all charged net of all lock waits; part = the marked budget, net of its harts' waits; want = water-filling, normalised; `+` and `-` sides.
- Kernel trace: H carries the runner's weight; `u` UNCAPPED records the uncap lift (stride hook `uncapped`), so the oracle doesn't read it as a charge.
- SHARE (count-based) removed. wake-no-preempt's proof is read per hart.
- Programs converted: share, large-weight, idle-gap, sleep-gaming, exit-churn, budget-churn plus a new shell case, timer-flood, carve-return, carve-inflation, deadline-flood plus a traced twin. Release twins keep their count regexes.
- ties and wake-no-preempt restated; latency head start 600 ms, smp=[1,2]; budget-deadline's keep_smp dropped (12/12 pass at 2 harts).
- keep_smp, each with its reason and per ruling: cluster x2, carve-return, budget-churn-shell, lift-delay, wake-no-preempt, large-weight, deadline-flood-billed-traced. Containment has it too, but the ruling says containment must judge at 2 harts.

Results: at 1 hart every changed case passes on both widths. At 2 harts the cases that remain unkept pass. The sched-latency 16-seed sweep at 2 harts has 0 target misses on either width.

Left:
1. Containment ring: 7.09M records needed against 6.29M; no record class is cheap to cut. Grow sched-trace-large to 256 MiB and memory_mib to 576, check worst-walk, re-pin the seed, drop keep_smp.
2. Pages: the keep_smp table with each case's class and its 2-hart coverage, as the orchestrator asked.
3. sched-capped and sched-lock-contention; ask to cut the VM case.
4. sched-latency-tcg at 2 harts.
5. Fold, rebase, gate, --smp 2 sweep, report.
