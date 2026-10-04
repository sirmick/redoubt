# B5: every scheduler share is judged net of the checked build's audits

Tier B (the bench), size S. Needs K18. Design: QA GATE1-notice-two-leases (K18's review).
Closes `docs/todo/shares-judged-gross.md`, which K18's trim adds.

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from your worktree.

## The rule (built by K18, on scheduling.md "Responsiveness")

Each target counts the kernel a release build runs, a share as well as a window. K18 judges
`sched-budget-churn`'s shares this way: the program prints `SHARE <name> <start> <end> <cpu> <min>
<max>` (`Bench::judged_share`), and `sched_oracle` subtracts the audit time inside the window
before it applies the bounds. The other share cases still judge their share in the program, gross
of audits. In `sched-exit-churn` the margin is about 40 thousandths over its 450 floor. One
audit-heavy exit or destroy variant would fail it on the audits alone.

## What to do, in this order

1. `sched-exit-churn`: each share goes through `judged_share`. The case gets `kernel_features =
   ["sched-trace"]` and `post_check = "sched_oracle"`, and `expect` matches the `SHARE` lines with
   their bounds, as `sched-budget-churn.toml` does. The program's own `ok` checks go, since the
   oracle judges.
2. `sched-destroy-billing`, then `deadline-flood-billed`: the same.
3. Any other case that judges a scheduler share in the program (`grep` for `share(` in
   `tests/programs`). Move each one, or say in the report why it stays gross: for example, a
   share that is no target, or a case with no audit inside its window.

No kernel change, no bound moves, no attacker weakened. A trace costs the case 32 MiB of RAM for
the budget tree. If a case no longer fits, say so; do not shrink its load.

## Tests

- Each moved case is green on both widths, with net and gross reported.
- `host:testbench::shares_are_judged_net_of_audits` already covers the oracle; add a host test
  only if a case needs something new from it.
- Whole `cargo testbench`.

## Pages

- `docs/testbench.md` "Checked builds" and `docs/kernel/scheduling.md` "Responsiveness": add the
  moved cases to their status lists. Remove the residual "Some shares are judged gross of
  audits".
- Delete `docs/todo/shares-judged-gross.md` and its SUMMARY line.

## Owned paths

The moved cases' `tests/*.toml` and their programs under `tests/programs`,
`tools/testbench/src/sched_oracle.rs` only if needed, the pages above, `docs/SUMMARY.md`.

## Report

Each case's share, net and gross, on both widths; anything left gross and why.
