# SCHED1 implementer 6: work order 3ef35750, done (2026-10-06)

## Branch

- `wp-SCHED1` head **db337fb43**, on base `4cc35d84b` (IPC3's rebased tip).
- The commits:
  1. `109047be7` testbench: prove the cluster, a waiting wake, a carve's return, a round
  2. `3672fe618` kernel: use a one millisecond scheduling slice (its content is unchanged)
  3. `08ff26e50` tests: the cluster case, its 10 ms control, and the 1 ms share cases
  4. `db337fb43` docs: the 1 ms slice and its cost, the cluster's envelope and control
- The fold was made by autosquash and checked: the tree before and after is identical.
- Nothing is pushed. The worktree is clean. The safety ref `wp-SCHED1-prefold` is kept.

## Delivered

1. **Docs** (`docs/kernel/scheduling.md`).
   - The Charging sentence, verbatim from the ruling, as its own paragraph after the billing list.
     The list's slice-end item ends in ";", so the sentence could not join it.
   - Responsiveness' arithmetic restated: 1.36 ms on rv64 and 1.45 ms on rv32 per 1 ms slice in
     the release build. Checked-build gaps are 1.26, 1.45 and 1.68 ms under 3, 9 and 17 queued
     budgets.
   - The three share cases' paragraph, with the measured figures.
   - The ties paragraph.
   - The debt-lift residual.
   - R23's list now includes `sched-debt-lift`.
   - Inheritance's status lists two new oracle tests.
2. **Fixtures** (the tests commit).
   - **large-weight, server-busy and carve-inflation** judge by ratio of counts and print useful
     work beside it. No bound moves.
   - **server-busy:** A's count is calls, not CPU, so it is in no ratio. The ratio is the server
     against the mean of users B and C, with B and C alike.
     - Residual: a loss B and C suffer together now shows only in the printed useful work.
   - **debt-lift:**
     - It now runs traced, with `post_check = "sched_oracle round"`.
     - It destroys an empty marker budget just before the sibling's `go`.
     - Its µs figure becomes a note.
   - **ties:**
     - B and A are now senders stamped with budgets Y and X.
     - One destroy each wakes a group in one kernel entry, by construction.
     - The guest checks clause 3 within each group.
     - Clause 2 is the oracle's alone, and the order is printed as a note.
3. **Oracle** (`check_round`).
   - The marker is the one `X` whose budget has no `W`, `K`, `R` or `D` record. The marked wake is
     the first `W` after its `Y`. That budget must be picked before any other budget is picked twice.
   - Tests:
     - `a_marked_wake_picked_within_one_round_passes`;
     - `a_budget_picked_twice_before_the_marked_one_fails`;
     - `a_round_check_without_its_marker_fails` (covers no marker, two markers, and never picked).
4. **The red team's round-4 P1** is folded in: `a_control_keeps_the_programs_own_gates`, in
   `tools/testbench/src/sched_oracle.rs`, in the testbench commit.

## Results: checked build, rv64 / rv32

Consoles are in `gate-fixtures/`.

| Case | Judged | Useful work |
| --- | --- | --- |
| large-weight | server 560 / 561 of all counts (want 555 ± 50) | server 411 / 396 |
| carve, depth 1 | victim 502 / 502 | 459 / 452 |
| carve, depth 4 | victim 502 / 503 | 409 / 398 |
| server-busy | server 351 / 363 per 1000 of the users' mean; B/C 500 and 499 | B 195 / 187 |
| ties | clause 3 ok in both groups. B ran first on both widths: a slice end came between the destroys | |
| debt-lift | round: woken record 6619 / 6616, picked after 1 pick of another budget, none twice | note: 63.9 ms after creation |

**debt-lift: what the gate run shows.**
- The marker's destruction runs a checked-build audit of about 26.6 ms (`U1` 1092549..1119164).
- The launcher is then preempted before its `go`.
- The spinners pass the sibling meanwhile, so the sibling is picked behind 1 budget, not 16.
- The diagnostic trace without the marker gave 16 picks and 27.8 ms. The docs say both.
- A broken lift (G's raw debt, about 6 rounds) would still fail the oracle: some other budget
  would be picked twice.

## Gates

All through the pool. All exits 0 unless noted.

| Gate | Result |
| --- | --- |
| build-rv64, build-rv32 | rc 0 |
| docs | PASS |
| formatting | PASS, after a nightly rustfmt of the fixtures. The first run failed on `sched-ties.rs`. |
| no-cruft, size-budget, unsafe-budget | PASS |
| `cargo test -p testbench --bin testbench` | 108 passed |
| the `host-tests` case, which covers the testbench crate | PASS (33.6 s) |
| five cases, both widths | PASS |

- **The host-tests run was cut short.** The make target `host-tests` matched every `*-host-tests`
  case, and the tool's one-hour limit killed the make partway, after host-tests passed.
- **The final recheck was killed** at the tool's 30-minute limit while it waited for the pool
  (0 tokens free; train-1's bench was running). The killed makes may have leaked tokens. Its parts
  were then rerun one by one with `share` and `take`.
- **Not rerun:** sched-cluster and its control. The kernel is unchanged and the fixtures do not
  touch them.
- **Not run:** the alone cases. The diff does not touch their paths.

## Summaries checked

- `docs/kernel/scheduling.md`: updated (above).
- `docs/testbench.md`: no change. It does not enumerate the oracle's per-case arguments, and its
  status list names `sched-ties`, which still exists.
- `docs/plan/m1-separation.md` lines 53 and 70: the rows name the same cases and claims. No change.
- `docs/kernel/model.md`, `docs/kernel/timer.md`: no change. Neither names these fixtures.

## Next

- The panel renews.
- Then the rebase onto main, on the orchestrator's word.
