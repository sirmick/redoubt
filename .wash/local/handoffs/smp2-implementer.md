SMP2 handoff (smp2-implementer -> successor). Branch wp-SMP2, worktree /home/mcloonan/redoubt/.worktrees/SMP2, off main 36d1450f9. Worktree CLEAN. Nothing pushed. Scratch and logs: /home/mcloonan/redoubt/.tmp/SMP2/. Checkpoint docs: .wash/local/SMP2-checkpoint.md and .wash/local/SMP2-checkpoint2.md. Original design brief: .wash/local/SMP2-implementer.md (it stands, with the rulings below).

## BRANCH STATE (oldest first)
1. 18256aaf2 kernel: a hart waiting for the kernel lock halts (wfi), and release() IPIs the harts in TicketLock.halted.
   - Also: smp-lock-wait (bound 3x; 1.5x rv64, 2.5x rv32), the negative feature sched-spin-entry (130-200x), all-together, ipc and smp-boot moved to icount, keep_smp dropped from map-anon-search-bound.
   - Size kernel 9625.
2. 438872bdc kernel: shootdown acks wait halted; serve() IPIs SHOOTER (the lock holder).
   - R10 p99 at 2 harts 51.9 -> 3.0 ms (rv32 2.9); deadline notice p99 53 -> 4.3 ms. memory.md has the numbers.
   - Size kernel 9630.
3. 0fbcbfc9a testbench, kernel: the oracle judges each hart's pick.
   - Trace records: H (a hart's runner), J (waiting threads), Q (a lock wait, start and end in TICKS), F (ticks since boot and harts, before C).
   - Summary: `lock waits N of 1000`. 'S' is passed over (SMP3's latent panic).
   - Size kernel 9658.
4. 8b4b70c66 model: H harts (Scheduler.set_harts, on, pick_on, run_on, slice_end_on, preempt_on), cap_set by water-filling to a fixed point, the capped floor, the uncap lift, the all-capped floor rising to the max.
   - Six mutations: R12CappedHoldsFloor, R12CapOnce, R12AllCappedHoldsFloor, R12UncapBanksCredit, R12OneRunnerPerBudget, R12SpreadChargesOnce.
   - scheduler_fairness seed%11==10 runs five scenarios through check.rs smp_scenario: late join, second cap, uncap, spread at 2 and 4, idle harts. A contract covers the one-runner rule.
   - All R12 mutations are caught (REDOUBT_MODEL_MUTATIONS=R12).
   - Size model 10735.
5. 587b153ee stride: Queue caches weights, waiting and running per slot; cap() scans the top H-1 by w/k; the all-capped max; the lift.
   - None of it is kept at one hart: set_waiting, ran_by and the push weight are skipped at H=1, and Harts::set_harts(n, waiting, weight) recounts.
   - reconcile_counted. The differential runs Harts at 1, 2 and 4 harts, 3000 seeds each, and catches the six mutations. Unit tests for caps, the lift and the recount when a hart comes online.
   - Size libs/stride 837, model 10736.
6. 487531848 kernel: sched::hart_online() from hart_main; set_waiting at settle (only when online>1); audit_caps inside audit_marks.
   - scheduling.md: R12 stated across harts (with the icount-rate correction), the capped paragraph in "The current minimum and ties", the status lists; SECURITY.md R12 row.
   - Size kernel 9719.
7. beb8ec133 WIP oracle shares across harts (to fold into a real commit):
   - water_fill(); CHARGED-SHARE marks `w:k`; judge_across_harts at H>1 (from F) over the mark roots, net of lock waits (Q on harts whose H runner is under a root), stating every want;
   - tests water_filling_caps_a_budget_at_its_threads and a_charged_share_across_harts_is_judged_by_water_filling_net_of_lock_waits. 49 oracle tests pass.
   - NOT yet on testbench.md's oracle status or the page text.

Verified on commit 6's tree:
- At 1 hart, both widths pass: the smoke set, smp-*, all-together, ipc, sched-latency, worst-walk (rv32 reconcile 8521 µs against 8900; it failed before the H=1 skips), kernel-containment, every oracle case, unsafe, no-cruft and docs.
- At 2 harts: sched-latency PASS on both widths; sched-ties PASS rv64; sched-ties rv32 FAILS its PROGRAM check (clause 3: one group's wakers ran lowest id first), which needs restating across harts.
- Every commit's size-budget passes.

## RULINGS (the orchestrator's; mark the all-capped one as taken while the Architect is paused, it is in .wash/local/architect-questions.md)
- The wfi lock wait on every platform, plain wfi: a bounded spin was measured and rejected. The page has +8%/+11% without icount.
- The shootdown-ack halted wait: its own commit with its numbers (done).
- Q4: lock waits are NOT unbilled; shares across harts are judged NET of lock waits (Q records), as of audits. The residual is written in scheduling.md "Fair kernel entry is bounded by count" with exit-churn 292/1000 and deadline-flood about 1000 ms of 2 s; the orchestrator files an M2 step-5 node (SMP4). Add "Shares across harts are judged net of these waits" to that residual when the share judging lands.
- The all-capped rule: when every queued budget is capped, the floor rises to the highest queued pass (uncontested time banks for no one). Mutation R12AllCappedHoldsFloor, caught by idle harts.
- Share verdicts move to the oracle, from the kernel's charges. Conditions:
  - its own commit, or one per family, after the kernel commits;
  - at 1 hart every share case gives the same verdict before and after, shown as a before/after table;
  - the oracle states every water-filling want;
  - deadline-flood-billed keeps its release build with keep_smp (and the reason) and gains a traced twin judged across harts.
- keep_smp on sched-cluster, sched-share-release and sched-large-weight-release, each with its reason. It stays on bench-poweroff-missing.
- budget-deadline is SMP2's: restate its lateness bound for H with the cause measured (2 of 6 failed at 2 harts, lateness 45-51 ms against about 31-36 ms). If the residual is lock wait, say so on the page and keep keep_smp until SMP4.
- Widen sched-latency's lease head start (300 ms in sched.rs steward(); at 2 harts rv64 N=16 got only 19 of 50 deadline samples) and say why on the case.
- Lock waits of 292/1000 (exit-churn) go in the report and beside the SMP4 note.

## WHAT'S LEFT
1. HART-SHARE in sched_oracle.rs. CHARGED-SHARE refuses lifts and reweighs inside the window, so the churn victims can't use it.
   - Planned format: `HART-SHARE name start end tol[+] mark:k w:k ...`.
   - Judged: the first mark's budget's own charges, net of its harts' lock waits. Whole: every budget the kernel charged in the window, net of all lock waits. Share = judged / whole.
   - Want: water_fill over the judged (weight from the trace, k) and the declared competitors (w:k) at H (F), normalised by the sum of wants. At 1 hart that is w/W, today's want.
   - `+` means one-sided (at least).
   - I was about to factor check_charged_share's accounting loop (lines ~2036-2119) into a helper both use. Keep CHARGED-SHARE's output strings unchanged: tests match them exactly.
2. Programs by family, each with sched-trace kernel_features plus post_check sched_oracle in its toml. Turn counts into b.note, add marks (move kernel-containment's mark() into tests/programs/src/sched.rs as pub), and print HART-SHARE lines:
   - spinner shares: sched-share (3 lines), sched-large-weight (the server's; the users' evenness count check is H-invariant, keep it), sched-idle-gap, sched-sleep-gaming (victim, one-sided);
   - churn victims: exit-churn, budget-churn, timer-flood, carve-return (they use judged_share SHARE lines now);
   - deadline-flood-billed: keep_smp plus a traced twin;
   - sched-server-busy needs no change (equal-weight ratios; it passes at 2 harts);
   - sched-destroy-billing passed at 2 harts.
   For each: a before/after table at 1 hart, then a run at 2 harts.
3. Other restatements:
   - sched-ties' program clause-3 check across harts;
   - sched-wake-no-preempt: start 4 spinners;
   - sched-debt-lift and sched-lift-delay: run at 2 and see whether picks counted across harts hold;
   - sched-cluster: keep_smp;
   - budget-deadline: restate, or keep_smp with the residual;
   - sched-latency: the head start; set smp=[1,2] on the gated cases (the owner gates the targets at 1 and 2, records them at 4); a 16-seed sweep at 2; numbers at 4;
   - sched-latency-tcg at 2 harts loses its N=16 "server got" line (not investigated).
4. The brief's new cases:
   - sched-capped: the scenarios on QEMU at 2 and 3 harts;
   - sched-lock-contention: the FIFO wait bound at 2 and 4 harts;
   - the VM case: ask the orchestrator to cut it if beamlet-redoubt isn't done.
   Also: the oracle's own cap-set and floor replay. It currently only checks wakes against a lower floor; cap checking needs weights in the trace.
5. Pages:
   - testbench.md: the oracle status (new tests) and the HART-SHARE description;
   - scheduling.md: "Measured on QEMU" retitled per the brief; the R12 "attacked three ways" list; Responsiveness with the 2-hart (gated) and 4-hart (recorded) numbers; the lock-wait residual sentence;
   - model.md (done);
   - m2 Progress: "R12 holds across harts, judged by the oracle and the model at 1, 2 and 4 harts; targets gated at 2".
6. The --smp 2 sweep on both widths (one q lease per width, separate jobs), the gate, the report (.wash/local/SMP2-report.md, ≤2000-byte member_update), and member_update assignment_results.

## TRAPS
- target/prebuilt goes stale on ANY edit, so never edit while a gate or a non-prebuilt cargo testbench run is building. Once QEMU has booted it is safe.
- After any rebase, run size-budget at EVERY commit: a ceiling raise needs a `Size budget: <crate>: a to b, reason` line in the commit that raises it, and the ceilings chain from commit to commit. A raise left uncommitted reads "raised ... and not committed", which is expected until the commit.
- Fold fixes with `git commit -m "fixup! <subject>"` and `GIT_SEQUENCE_EDITOR=true git rebase -q -i --autosquash 36d1450f9`. Reword with GIT_SEQUENCE_EDITOR="sed -i -E 's/^pick (sha)/edit \1/'" plus `git commit --amend -F file`.
- Formatting: `cargo +nightly fmt -p <crate>` or `rustfmt +nightly --edition 2024 --config skip_children=true <file>` (the model is edition 2021: use cargo +nightly fmt -p redoubt-model). Check with `cargo +nightly fmt --check`; stable fmt differs on untouched files.
- doccheck: run `cargo test -q -p redoubt-doccheck` for the exact finding. The first citation of a rule must read `R78 (fair kernel entry)` and so on. SECURITY.md rows must list the status lines' tests.
- Under icount, counts are machine instructions (a halted hart gives its rate away); judge from charges. QEMU 10.2 runs pause and wrs.nto as no-ops.
- An uncontended acquire returns held=false. Any new hart must have sie.SSIE before its first lock wait (init_hart runs before acquire in hart_main).
- The kernel's KernelCell asserts the lock is held: never read now_ticks() before the lock (use raw riscv time::read64).
- Logs are pruned: copy what you need into .tmp/SMP2. Never use /tmp.

## COMMANDS
export PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper; unset MAKEFLAGS; q=/home/mcloonan/redoubt/scripts/q
- one case: $q run --cores 8 --tenant SMP2 -- cargo testbench [--smp 2] --exact <case>
- prebuilt and a gate: make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <wt> prebuilt; make -k -f .../jobs.mk -C <wt> rv64/<c> rv32/<c> ... (a gate script is in .tmp/SMP2/gate3.sh)
- a sweep: target/prebuilt/testbench --prebuilt target/prebuilt --arch rv64 --smp 2 --exact <c> (.tmp/SMP2/sweep.sh)
- stride: $q run --cores 8 -- cargo test -q --release -p redoubt-stride
- model: cargo test -q -p redoubt-model --release --test properties --test current_contracts; REDOUBT_MODEL_MUTATIONS=R12 ... --test mutations -- --nocapture
- oracle: cargo test -q -p testbench sched_oracle

What consumed my context: the per-case gate and sweep runs, the repeated rebases and size-budget fixes, and reading the oracle and model.
