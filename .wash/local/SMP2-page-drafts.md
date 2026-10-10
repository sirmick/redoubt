# SMP2 page drafts (not yet applied)

## docs/testbench.md, "Checked builds": replaces "A share is judged the same way: ... (`counted <n> of
## the calibrated 1000`)." (lines ~622-640); the CHARGED-SHARE sentences after it stay.

A share is the kernel's charges, never a count: under `icount` a count is the machine's
instructions, not a hart's time. The program prints its window, marks the budget it judges (an
empty child of the mark's weight, carved and destroyed, so the trace's lift names the parent), and
names by weight and runnable threads each budget it runs against it (`HART-SHARE <name> <start>
<end> <tolerance>[+|-] <mark>:<threads> <weight>:<threads>...`; `+` for at least, `-` for at
most). `sched_oracle` sums what the kernel charged each budget in the window, its pass's rises
times its weight as the trace states it (each hart's runner, `H`, carries its weight), less each
hart's waits for the kernel lock, which bill the waiting hart's runner though no thread of it ran;
a lift out of the cap set (`u`) is no charge. The part is the marked budget's and what was lifted
into it, the whole every budget's, and what the budget is owed is its water-filling share of the
trace's harts (`F`) among the budgets the program names, which on one hart is its weight's share.
An audit is charged to no budget, so on one hart the share is net of the audits; on several they
fall mostly on the hart that switches budgets each slice, so `sched-large-weight` keeps one hart.
Each program notes its counts beside with no verdict, and `sched-share` reports what the three
counted of the calibrated rate (`counted <n> of the calibrated 1000`). The release twins
(`sched-share-release`, `sched-large-weight-release`, `deadline-flood-billed`) have no trace and
judge their counts at one hart, in their expect lines.

Status lists (scheduler oracle and checked builds): drop `shares_are_judged_net_of_audits`; add
`host:testbench::water_filling_caps_a_budget_at_its_threads`,
`host:testbench::a_charged_share_across_harts_is_judged_by_water_filling_net_of_lock_waits`,
`host:testbench::a_hart_share_is_judged_of_every_charge_against_water_filling`,
`bench:deadline-flood-billed-traced`.

## docs/kernel/scheduling.md

- Residual "Fair kernel entry is bounded by count": add "Shares across harts are judged net of
  these waits." and the cases kept at one hart for them: `sched-budget-churn` (the shell's victim
  326 rv64, 406 rv32 at two harts), `deadline-flood-billed-traced` (407 to 884),
  `sched-wake-no-preempt` (no mid-slice nap). Lock waits per mille at two harts: exit-churn 135/145,
  share 130/195, large-weight 209, budget-churn 183/192, timer-flood 185/193, debt-lift 227/217.
- New residual (or in "Measured on QEMU"): a checked build's audits fall on the hart that switches
  budgets; `sched-large-weight` keeps one hart (rv32 572 at two).
- "Measured on QEMU, on one hart." -> "Measured on QEMU." and the sentence -> "The queue and its
  accounting are judged at 1, 2 and 4 harts; the targets are gated at 1 and 2 and recorded at 4."
- "It is attacked three ways": boot cases' list: shares "of the kernel's charges, against
  water-filling, at one hart and two"; add "a heavy budget with one thread on two harts while others
  join, and one budget spread over four harts" only when sched-capped exists.
- Responsiveness lines ~531-556: the share paragraph is about SHARE net of audits; restate as
  HART-SHARE numbers (1 and 2 harts) from .tmp/SMP2/table.py.
