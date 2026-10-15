# SCHED4 report (k33-implementer-3, resident; worktree .worktrees/SCHED4, wp-SCHED4 from 92b84a5b8)

## Design

Rule. A timer interrupt for a timeout or a deadline is taken on the hart that runs its payer; a
hart that runs no payer of an item comes by it only when no hart runs that payer. Form, as the
Architect amended: `time.rs` keeps, per kind, the global earliest item and its payer's budget id
(`threads`/`threads_for`, `budgets`/`budgets_for`), `None` when two payers tie at one instant;
the payer of a timeout is the blocked thread's budget, of a deadline the dying budget's parent
(the bill's payer at destruction may be an ancestor with free weight; the placement reads the
parent). `lowered((hint, by), at, payer)` keeps the pair as the WIP does.

Where decided: only `rearm()`, which arms the earliest of the hart's slice end and each kind's
item unless another hart runs that item's payer (`sched::runner_ids()`, O(harts), reading
`Harts.runners[].cur`) and is armed at or before it; with `None` or a payer that runs nowhere,
every hart arms it, as today. `rearm` runs where it runs today: the return to user mode
(`sched::leave`, `pid != 1`), before `kmain` idles, and at a timer interrupt taken in the idle.
`note_timeout`/`note_budget_deadline` only record (the WIP's extra `rearm()` calls are dropped:
the entry's own return re-arms, one SBI call per exit stays); a cancel (a wait ended early, a
budget destroyed by hand) leaves the hint early, as today. No scan of the deadlines on the switch
path: `expire_due` alone recomputes both pairs, from `collect_due`'s walk (`Due::next_for`) and the
deadline list (parent id), as the WIP does.

Payer runs nowhere: every hart arms the item, so a one-thread budget's own timeout (it blocks, so it
runs nowhere) is unplaced by construction; that is the residual the timeout counts measure
(exit-churn 0.15-0.23 %). The gate is for deadlines, whose payer (the creator) runs. An idle hart
arms an item only when its payer runs nowhere, so a deadline whose creator runs waits for the
creator's hart (its section, if in one): the notice latency cost, measured below.

K33: both slice ends return through `leave`, which re-arms with the runner set as it is after the
switch: in place the runner is unchanged; the direct switch arms the pick's items on this hart,
and the budget switched from, if it now runs nowhere, is armed everywhere. `repick`'s `due()`
reads the global hints as today, so an item due that another hart is armed for still sends this
slice end through `kmain`, whose expiry serves it here: the placement is of the interrupt, not an
exclusion; a passed item is served by the first entry anywhere (I13, R10 ordering unchanged).
Hence the gate is a bound, not zero.

Negative. Where the deadline falls while the creator makes calls, its own entries serve it (the
"91 of 96 on the creator" runs) and `timer-any-hart` lands on the victim only by phase (5 of 96,
~1 %): not a negative. Construction `deadline-placement`: creator C (one thread) and spinner V, equal
weight, two harts; C creates its round of deadline budgets then spins in user mode making no call
until the window ends. Any-hart: both timers fire at each deadline, icount's turn decides, a
substantial share lands on V (~0.19 % of V's pool per landing; 8+ landings fail 1 %). B1: only C's
hart is armed, C is in user mode, every destruction lands there. Case `deadline-placement`
(checked, `sched-trace`, smp 2) gates the count; the recorded negative is the same with
`kernel_features += timer-any-hart` (the WIP's feature). deadline-flood-billed-traced and
sched-exit-churn also gate the count at 2 harts (their any-hart outcome is timing-bound, reported).

Oracle. New arg `foreign_timer_in=<share>:<per_mille>`: inside the share's window, timer
interrupts from user mode that interrupted the judged budget (the mark's, or one lifted into it)
and charged a budget outside its marks; their ticks over the judged budget's charged ticks (the
share's `its`) at most per_mille. Judged at 2+ harts, recorded at 1 (at one hart every interrupt
is on the victim's hart). Report-only count unchanged. Gate: 10 per mille in the three cases.

Latency. `sched-latency` and `kernel-containment` at 2 harts, both widths, before (92b84a5b8) and
after: deadline_notice net p99 at N = 1, 4, 16 (baseline 4.6/5.1/4.6 ms rv64, 6.2/6.1/5.2 ms rv32);
after <= before + one kernel section, the longest the same run's `hold-trace` twin or
kernel-containment's `kernel sections` line records. Pages: timer.md "The hart timer" (the rule,
payers in the figure's three times), "R12 (scheduling) for timer work" (the residual becomes the
rule; residuals: ties, payers running nowhere, an entry's expiry on another hart, device
interrupts stay IRQ1's); scheduling.md residual risks' pointer; testbench.md the arg. Model: no
rule changes (the model has no per-hart timer; its bills are unchanged), so no mutation; the
kernel feature `timer-any-hart` is the negative. Commits: (1) kernel + pages, (2) oracle arg +
host tests + the three tomls + `deadline-placement` program and case, (3) measurements into docs.

Ruling (kernel-red-2, QA SCHED4-design, 2026-10-14): build as designed; bindings: (1) liveness
invariant stated in timer.md and host-tested on the pure decision (fallbacks part of the rule; a
tie is two different payers); (2) due() keeps the global hints; testbench.md says foreign_timer_in
counts timer interrupts from user mode only, and the oracle reports (not gates) foreign expiry
ticks reached on the judged budget's hart through kmain/its own calls; (3) gate 10 per mille at
2+ harts; timer-any-hart must fail deadline-placement on both widths, quoted; (4) latency after <=
before + the run's longest section, both widths, N = 1/4/16, in timer.md; (5) K33's cases
re-judged (sched-timer-entry p99 delta; wake-no-preempt-harts and twin, condition-5 style; floors
unmoved without a ruling; sched-latency, kernel-containment at 2 harts); (6) three commits, each
reviewed on the thread; no model rule change.

## Commit 1: b2f930729 on 92b84a5b8 (2026-10-14)

kernel/src/time.rs: `Timer.threads`/`budgets` are `redoubt_stride::Hint` (at, payer); note_* take
the payer and only record (no rearm added); `rearm` decides with `Hint::due_on(h, runner_ids(),
armed)` per kind, `timer-any-hart` falling back to the plain `at`; `due()` reads `.at`;
`expire_due` recomputes both hints with payers (the walk's `Due::next` is a Hint; the deadline
fold uses the parent's id). message.rs `settle`/`collect_due` pass `budget_of(pid)`'s id;
budget.rs passes the parent's; sched.rs `runner_ids()`; Cargo.toml feature `timer-any-hart`.
libs/stride: `Hint` (`NONE`, `lowered`, `due_on`) + three host tests (lowering and ties; liveness
and placement over 20,000 random runner/item sets iterated to the harts' re-arm fixpoint; a hart
covers an item only when armed at or before it, `armed 0` = fired and re-arming counts).
Pages: timer.md hart-timer status (tested 5 -> 8: the three host tests), the rule paragraph, the
figure's hints and decision box, "R12 for timer work" rewritten as the rule + what still lands
elsewhere (with TBD-NEG-*, TBD-GATES, TBD-LAT-* for commits 2-3), a residual-risks bullet and the
"kernel work is not interrupted" bullet's new cost; scheduling.md's residual bullet points at it.
Checks: `cargo test -p redoubt-stride` 28 pass (.tmp/SCHED4/c1-stride.log); kernel builds rv64 and
rv32: release, checked+sched-trace, checked+sched-trace+timer-any-hart all rc 0
(.tmp/SCHED4/c1-build.log); rustfmt clean; size-budget PASS (kernel 10757 -> 10772, stride 929 ->
953, two budget lines), docs PASS, formatting PASS.
