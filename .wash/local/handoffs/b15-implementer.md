B15 handoff (b15-implementer), at the checkpoint.

BRANCH STATE
- wp-B15 at 8bb4654c2. It is on main bd1b90fe6 (main's tip at the time; VOL2 included). The worktree /home/mcloonan/redoubt/.worktrees/B15 is clean and nothing is pushed.
- Three commits, in this order (as ordered by the orchestrator):
  1. 264eb168a "kernel: the containment gate's trace ring is 192 MiB". Feature sched-trace-large (implies sched-trace); trace::PAGES = if cfg!(feature="sched-trace-large") {49152} else {16384} in kernel/src/sched.rs. tests/kernel-containment.toml uses it with memory_mib = 512. The pages are kernel/README.md "The run" and testbench.md "Checked builds".
  2. ba2039706 "testbench: the containment gate judges the bystander on the kernel's charges". The oracle's CHARGED-SHARE check (sched_oracle.rs: charged_shares, expected_share, check_charged_share, 3 host tests). The program marks the bystander and sessions (empty children of weight 2 and 3). The steward stand-in opens the steady window once round 0's D lease arms: CT_STEADY report, bystander window endpoint in slots S9/B6, fails bit 64. Page row kernel/README.md:177 as ruled, plus "under both slots' leases". testbench.md CHARGED-SHARE paragraph, now including the red's residual sentence (the clause judges what the kernel charged; under-billing everything under the marks alike passes it; the expect lines and latency clauses bound that).
  3. 8bb4654c2 "testbench: a wait cut short reports the failure the guest printed". qemu.rs FAIL_LINE/after_failure plus a test; the gate's forbid quoting is fixed; bench-cbo-self-unrefused's must_fail is updated.

WHAT IS DONE
- Red review: OK with notes, all P2. Both folds are done: (1) the residual sentence, in commit 2; (2) the bench stdout saved at .wash/local/evidence-B15/large/bench-stdout-{rv64,rv32}.log, beside the consoles.
- Gate results on earlier heads (same code):
  - bcc68a776 (on 051a2f86c): kernel-containment PASS on rv64 (3,907,238 records, dropped 0) and rv32 (4,610,867, dropped 0). Charged share 833 against expected 833 on both. Smoke cases, docs, formatting, no-cruft and size-budget (9190 of 9190) all pass.
  - b9fe7f91f (on 25a99ccb3): kernel-containment rv64 PASS. Its docs/size-budget never ran (queued, then killed as stale).
- On 8bb4654c2: testbench host tests 136 passed. The rv64 kernel-containment rerun was KILLED for this checkpoint while running in QEMU. Docs and size-budget were not run.

WHAT IS NEXT (on resume, with the new command runner)
1. On 8bb4654c2: rerun kernel-containment rv64, plus docs and size-budget.
2. Save the rv64 bench stdout as .wash/local/evidence-B15/large/bench-stdout-rv64-8bb4654c2.log.
3. Report the head and exits to the orchestrator, who said they "accept and merge on it". If main moves again, rebase first. Expected conflict spot: sched_oracle.rs, where new checks are appended after check_round/check_lift_delay; keep both.
4. Update .wash/local/B15-report.md: it still names the older hashes (3b0a38079/1fb7bea85/bcc68a776). Current: 264eb168a/ba2039706/8bb4654c2.

TRAPS
- The bench prunes run directories: copy consoles out of target/testbench/run-*/ right away. The jobs log (target/jobs/<w>-kernel-containment.log) holds the oracle's lines.
- kernel-containment takes about 500 s (rv64) to 550 s (rv32) of guest time. A whole-run "all" job from another session can block docs/size-budget for a long time.
- size-budget is at exactly 9190 of 9190 kernel lines. Any added kernel line fails it.
- Folding fixes: git commit --fixup=<sha>, then GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash <base>. Never a fix-up commit on the final branch.
- In the program file, b.finish("KERNEL-CONTAINMENT") appears twice (the RTC-not-found branch too): use the last occurrence when editing by string search.
- The oracle refuses any trace with drops; the old 64 MiB ring dropped about half the run. Do not cut traces for evidence.
- Python heredocs with '' replacements: an empty match corrupts the file (it happened once; restored from git).
