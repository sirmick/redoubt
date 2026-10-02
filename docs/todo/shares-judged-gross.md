# Some scheduler shares are judged gross of audits

## What

A share counts the kernel a release build runs, so a checked build's audits inside its window
must not count ([responsiveness](../kernel/scheduling.md#responsiveness)). `sched-budget-churn`
prints each share with its window (`SHARE`), and the scheduler oracle subtracts the audit time
inside it. `sched-exit-churn`, `sched-destroy-billing` and `deadline-flood-billed` judge their
shares in the program, gross of audits. `sched-exit-churn` clears its 450 floor by about 40
thousandths; one audit-heavy exit or destroy variant would fail it on the audits alone.

## Why it matters

A gross share is judged against a kernel a release build is not, and a margin that the audits
eat makes a correct kernel fail.

## Where

`tests/sched-exit-churn.toml`, `tests/sched-destroy-billing.toml`,
`tests/deadline-flood-billed.toml` and their programs; `Bench::judged_share` in
`tests/programs/src/sched.rs`.

## Done when

- Every scheduler share is printed as a `SHARE` line and judged by the oracle, net of audits.
  The exception is one that has no audit inside its window, said so in its case.
- The residual on scheduling.md goes, and this page is deleted.
