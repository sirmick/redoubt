# SCHED1 implementer-5: host-only grant, 2026-10-05 (20:19 to 20:22 UTC)

Worktree `/home/mcloonan/redoubt/.worktrees/SCHED1`, branch `wp-SCHED1`. Environment:
`~/.cargo/bin` first on `PATH`, plus `RUSTSBI_PROTOTYPER`, `RUSTSBI_PROTOTYPER_RV32` and
`BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains`. No QEMU guest was run.

## Commands, in order

1. `cargo +nightly fmt --all`: exit 0. It reformatted only `tests/programs/src/sched.rs`,
   `tests/programs/src/bin/sched-cluster.rs` and `tools/testbench/src/sched_oracle.rs`. Afterwards
   `cargo +nightly fmt --all --check` exits 0.
2. `cargo testbench sched-oracle-local`: exit 1 (it compiled; 25 passed, 3 failed).
   - The failures were `cluster_metadata_rejects_arming_and_containment_failures`,
     `cluster_envelope_qualifies_where_the_old_rtc_interval_did_not` and
     `cluster_metadata_rejects_bad_records_and_bounds`.
   - The cause was a test-helper bug, not an oracle bug. `edited()` split the whole line, so index 0
     was the `CLUSTER-SAMPLE` tag and every edit landed one field early. For example, "field 4"
     (prep) edited the slot, which is 0 for attempt 0, so the line was unchanged and accepted.
   - Fix: `edited()` now writes `f[i + 1]`, numbering fields as the parser does (0 is the measure).
     I also removed an unneeded `mut` (an unused-mut warning in the wait-boundary test). No
     production code changed.
3. `cargo testbench sched-oracle-local` (after the fix): exit 0, `PASS sched-oracle-local 1.8s`.
   All 28 tests in `sched_oracle.rs` (the tests and model modules) pass. I ran it again with `-v`;
   it also exited 0.
4. `./build --arch rv64 --programs`: exit 0.
5. `./build --arch rv32 --programs`: exit 0. Both widths produce
   `target/riscv{64,32}imac-unknown-none-elf/release/sched-cluster`. The one warning is a
   pre-existing unused `mut` in `kernel-half-attack.rs`, which is not mine.
6. The kernel for the `slice-10ms` case, built as the bench builds a `kernel_features` case's
   kernel (`tools/testbench/src/main.rs` and `build.rs`: the target's `qemu-virt` plus the case's
   features, checked profile because the case sets `debug_assertions = true`):
   `cargo build --profile checked --target riscv64imac-unknown-none-elf -p redoubt-kernel --features qemu-virt,sched-trace,slice-10ms`
   exits 0, and the same for `riscv32imac-unknown-none-elf` exits 0. The warnings are the
   trace module's walk items, unused without `walk-trace`. They predate this work.

Not run: the whole `host-tests` case (testbench's crate unit tests in the bin). The filter
`host-tests` also matches 15 other cases, model-host-tests among them. The 28 oracle tests ran in
`sched-oracle-local`, which compiles the same `sched_oracle.rs`.

## The twelve status-line tests

All eight distinct names (four are listed in both sections) are in `sched_oracle.rs` and passed
in run 3:

| Test | Result |
| --- | --- |
| destructions_are_timed_and_bounded | PASS |
| audits_are_subtracted_inside_each_window | PASS |
| shares_are_judged_net_of_audits | PASS |
| an_unmatched_audit_fails | PASS |
| cluster_envelope_qualifies_where_the_old_rtc_interval_did_not | PASS |
| the_old_control_must_fail_on_its_lower_witness | PASS |
| cluster_credit_is_the_certified_interior_only | PASS |
| cluster_lower_witness_counts_the_union_of_outer_bins | PASS |

The other cluster tests also pass: `cluster_plan_rejects_the_old_construction`,
`cluster_metadata_rejects_bad_records_and_bounds`,
`cluster_metadata_rejects_arming_and_containment_failures`, go protocol, wait boundary, fences,
wake-no-preempt, carve-return, the rank/lift/reweigh/floor tests and the model tests.

## Commit added

- `95d8b7038` sched-cluster: format, and edit test records by the parser's field numbers
  (`tests/programs/src/sched.rs`, `tests/programs/src/bin/sched-cluster.rs`,
  `tools/testbench/src/sched_oracle.rs`). It folds into 7a6fdab27 and a8221db27 at the
  acceptance rebase.

`git status` is clean. No cargo, QEMU or bench process of mine is running.
