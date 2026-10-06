# SCHED1 implementer-5: v3 kernel-envelope source preparation

Worktree `/home/mcloonan/redoubt/.worktrees/SCHED1`, branch `wp-SCHED1`. Specification:
`.wash/local/SCHED1-consistent-window-proposal.md`. No cargo, rustfmt, QEMU or Docker was run
(machine hold). Nothing has been compiled or formatted yet. Never pushed.

## Commits (on c51db00b8)

- `7a6fdab27` sched-cluster: complete the kernel-clock envelope and its old-control test
  (`tests/programs/src/sched.rs`, `tests/programs/src/bin/sched-cluster.rs`,
  `tests/sched-cluster.toml`, `tools/testbench/src/sched_oracle.rs`)
- `34b9ae44e` docs: define the cluster's kernel-clock envelope and certified audit credit
  (`docs/kernel/scheduling.md`, `docs/testbench.md`)

I read every committed file in full before committing it.

## The WIP commit c51db00b8: kept, to be folded at acceptance

Its file hashes do not match implementer-4's handoff. It is not just the go-boundary fix: it
already holds an unreviewed v3 draft. That draft has `Bench::go_cluster` (H/R/F from one
`time_now`), `cluster_spin_until`, B/L/E/P/U records, the `v3-kernel-envelope` plan, the
certified-interior `cluster_metric`, the Cmax lower witness and `cluster_old_control`. v3 keeps
the serial readiness and go protocol, so `cluster_go_boundaries` / `cluster_spinner_releases`
(the go-boundary fix) still apply and stay. My commits finish that draft. The whole branch still
has to be rebuilt into logical commits before the merge.

The WIP also changes paths outside my assignment, which I did not touch:
- `model/tests/common/contracts.rs`: 1 ms slice numbers. These belong with 7b23f528.
- `sched-carve-return.rs` and `.toml`, `sched-wake-no-preempt.rs` and `.toml`.
- `tools/testbench/src/build.rs` `take(12)`→`take(120)`. This looks like a debug aid; drop it at
  the fold unless someone wants it.
- `tests/sched-oracle-local.toml` and `tools/testbench/tests/sched_oracle_only.rs`: the focused
  host harness. Implementer-4 described it as temporary; it is needed to run the host tests
  below, so decide at the fold whether it stays.
- `Cargo.lock`, `tests/programs/Cargo.toml` and `tools/testbench/Cargo.toml`
  (`redoubt-stride` dependency).

## What 7a6fdab27 changes against the draft

Guest (`sched.rs`):
- The driver reads E, then the RTC's s, then P, then clears the RTC. If E or P fails, that is a
  branch-6 failure; the draft used `unwrap_or(0)` for E. The timer's P also fails the same way
  (the draft used `unwrap_or(0)`).
- The timer has no E and no RTC, so `early`, `arm`, `deadline` and `service` are 0 and the unit
  label is `no_rtc`. The draft printed L in the "deadline" column, which the proposal forbids.
- The containment check applies E/a/d/s only to the driver.
- Records are exactly `[_; 200]` (the draft used 256). Stack use is 16,000 B of records plus 2 KiB
  in `report_windows`, against a 32 KiB stack.
- Doc comments updated: the branches including 7, the five metadata words, and the
  units/clock of the cluster window.

Oracle (`sched_oracle.rs`):
- The metadata parser requires the stand-in's header to come before its samples. Timer
  E/a/d/s/gross must be 0 with `no_rtc`. The driver's d=a+1000δ uses checked arithmetic (overflow
  is a rejection). L and U are rebuilt with let-else overflow checks.
- Old control: `cluster_old_control_verdict` is a pure function. It requires at least one
  cluster envelope target missed AND driver lower-witness p50>b50 or p99>b99. The draft instead
  rejected unless every measure missed, which is stricter than "fail at least one target" in the
  proposal.
- Report labels: in the cluster group, "envelope net/gross" and "certified audit credit". Each
  sample shows envelope/certified-audit-credit/net. Driver samples show RTC a/d/s, RTC gross and
  the lower witness; timer samples show only B/L/P/U. The lower-witness percentiles are reported
  for the driver only.
- The module doc gains a cluster paragraph.
- Both consumers (`run`'s group loop and `check_cluster`) use the same `ClusterMetric`. This was
  already true in the draft and is unchanged.

## Host negatives (proposal section "Minimal host negatives"), by test

- `cluster_plan_rejects_the_old_construction` (unchanged): v2 plan, duplicate plan.
- `cluster_metadata_rejects_bad_records_and_bounds`: missing, reordered and duplicate records;
  every field wrong on its own; mixed or old units; timer RTC/E fields set; header mismatch,
  duplicate header, header after samples.
- `cluster_metadata_rejects_arming_and_containment_failures`: zero intent before its target is
  accepted, past its target or reached (delay 0) is rejected. Positive B 5 µs after its slot is
  accepted, 1 µs before is rejected. L>P; E<B; E>P; U=F accepted, U>F with P==F rejected; wrong
  d−a; s<d; overflow of d and U, and an unsigned B.
- `cluster_envelope_qualifies_where_the_old_rtc_interval_did_not`: E − RTC-gross before R, with
  the full envelope inside, is accepted. A printed window that disagrees with the envelope
  (reaching before release, or a shifted end) is rejected.
- `cluster_credit_is_the_certified_interior_only`: exact credit at both edges; zero for empty and
  sub-µs interiors and for audits outside; credit never above `audit_inside`; reversed or
  overflowing audits, credit above the envelope, credit-sum overflow and a reversed envelope are
  all rejected.
- `cluster_lower_witness_counts_the_union_of_outer_bins`: overlapping outer bins are counted
  once, clipped to the envelope.
- `the_old_control_must_fail_on_its_lower_witness`: met targets → Err; an envelope-only miss →
  Err; a lower witness above p99 or p50 → Ok; `cluster_old_control` without `cluster` → Err.
- Kept unchanged: go protocol, wait boundary (199+report, immediate, extra), fences,
  `an_unmatched_audit_fails` (invalid, nested or missing audit pairs at trace level), and the
  rank, latency and coverage tests.

All of these are written but none has been compiled or run.

## Docs (34b9ae44e)

- `docs/kernel/scheduling.md#responsiveness`: a cluster paragraph covering the window, the
  envelope, the upper-bound argument, the known-bad control and lower witness, and "no cluster
  result is recorded yet". Status +2 host tests (12).
- `docs/testbench.md#checked-builds`: the certified-interior rule for cluster envelopes only.
  Status +2 host tests (12).
- `bench:sched-cluster` is not added to any status line, because it has not passed.

Affected summaries I checked but did not edit (outside my two sections):
- `docs/kernel/scheduling.md` intro ("at most 10 ms"), Preemption points (`SLICE_US` 10,000 µs),
  and the instruction table ("one 10 ms slice") are stale against 7b23f528's 1 ms slice. Kernel
  owner / final reconciliation.
- `docs/kernel/scheduling.md` R23 lists the cases that enable `sched-trace`; `sched-cluster` (and
  `sched-wake-no-preempt`, if it traces) are missing.
- `docs/testbench.md#the-scheduler-oracle`: does not need cluster text.
- `README.md`, `GETTING-STARTED.md`: no cluster or slice claims affected by this step
  (implementer-4 checked these too).

## Needs the machine (after the grant)

1. Nightly fmt on the changed Rust files. rustfmt may reflow the let-else and the long
   `format!`s.
2. `cargo testbench sched-oracle-local`, the focused host suite including all the tests above.
   Expect compile fixes first: none of this code has been built.
3. Builds of the test programs on rv64 and rv32 (`sched-cluster`).
4. Then, only with a separate grant: one focused candidate `sched-cluster` run, seed 3, rv64.
