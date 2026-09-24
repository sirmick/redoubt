# Host model recovery validation

Completed 2026-09-23.

The model was selectively recovered from reference `674831cf9` onto native lifecycle base
`e56c7a13e`. No historical branch was merged. Production kernel and runtime files are unchanged.
This evidence concerns the host oracle; no trace was replayed against the real kernel.

## Commands and results

Run in `redoubt-dev:latest`, offline, with `REDOUBT_MODEL_SEQUENCES` and
`REDOUBT_MODEL_MUTATIONS` unset. Separate target caches allow the full bench and long model run
to execute concurrently. Commands are run from the recovered workspace root.

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -p redoubt-model --release -- --nocapture` | 40 passed, 0 failed; 1 ignored test is the separate long acceptance run. Default properties: 90,020 sequences. All 100 deliberate mutations detected. |
| `cargo check --offline --locked -p redoubt-model --target riscv64gc-unknown-none-elf` | Passed. |
| `cargo check --offline --locked -p redoubt-model --target riscv32imac-unknown-none-elf` | Passed. |
| `cargo testbench` | Final optimized run: 116 PASS, 66 cases, 0 FAIL, 0 SKIP; exit 0. |
| `cargo test --offline --locked -p redoubt-model --release --test properties -- --ignored million --nocapture` | Passed, exit 0: five families of 1,000,000 plus 1,000 flood scenarios, 5,001,000 total; 2,362.13 seconds. |

Default counts are 20,000 each for kernel, lifecycle, scheduler and policy; 10,000
noninterference; 20 flood. Additional focused policy tests include 256 connection-lineage
sequences of 128 operations and 64 push snapshots. These are separate from long-run counts.
The dedicated long command reports 1 passed, 0 failed, 0 ignored and 6 filtered tests: those
six default family tests ran in the unfiltered default suite. No acceptance family was skipped.

| Long-run family | Sequences | Seconds |
| --- | ---: | ---: |
| Kernel | 1,000,000 | 200.3 |
| Budget lifecycle | 1,000,000 | 93.0 |
| Scheduler fairness | 1,000,000 | 9.7 |
| Steward policy | 1,000,000 | 778.7 |
| Steward noninterference | 1,000,000 | 1,255.7 |
| Flood | 1,000 | 24.7 |

The full bench included both-width native process and IPC regressions, server host/build cases
and the unsafe checker. Runtime remained 9, block server 4, boot filesystem and console servers
0; every configured root had 0 undocumented unsafe uses. Source/test/manifest hashes were
unchanged through final acceptance; only validation documentation was finalized afterward.

## Test execution improvements

Three policy mutations now try their detecting noninterference family first. All 100 variants,
six fallback families and original seed caps remain. In the model timer, event-free intervals
avoid repeated deadline scans while retaining every scheduler pick and charge. Pending delivery,
deadline boundaries and partial slices keep their previous handling.

The differential test compares complete model state against the original timer loop: 4,800
boundary cases across the normal model and all 100 mutations, plus 64 histories, 9,600 operations
and 3,241 tick comparisons. No histories were omitted. It covers saturation inside the fast path.

Measured default mutation runtime fell from 420.99 to 58.53 seconds; properties from 293.71 to
59.66 seconds. Concurrent load differed, so these are observed runs, not a controlled speed ratio.
Matched 1,000-sequence probes measured policy at 35.0921 versus 8.0961 seconds and noninterference
at 66.3854 versus 14.8802 seconds. No sequence counts, checks or overflow detection were removed.
The full bench's model host case fell from 517.9 to 157.0 seconds; both measurements included
the unchanged default coverage, and the latter ran alongside the long acceptance job.
The million-sequence policy family fell from 2,546.1 to 778.7 seconds. Its complete count passed
in both runs; this does not imply the interrupted baseline's other pending families completed.

## Independent implementation reviews

The retained reviewers covered actual source changes and subsequent repairs, then reviewed
both performance changes independently:

- Design/code consistency (`sv1_consistency`): resolved process-handle lifetime, scheduler-pass
  saturation and ended-process flag-validation precedence. Final verdict OK.
- Defensive correctness (`k4_defensive`): resolved forged policy approvals, premature handle
  revocation and stale ghost notice debt across PID reuse; reviewed negative controls and timer
  saturation. Final verdict OK with documented scope notes, no blockers.
- Simplification (`sv1_simplifier`): removed the duplicate push-request ID; reviewed lifecycle,
  generator and output-record repair followups. Final verdict OK, no outstanding findings.

The final generator repair also received independent actual-patch checks from
`model_final_consistency` and `model_final_defensive`. The architect reviewed specification
boundaries and documentation; recommendations for unanswered questions remain unapproved.

## Incomplete attempts and limits

Earlier interrupted runs are not acceptance: one long attempt completed three families
(3,000,000 sequences) before its process became unavailable; a superseded baseline completed four
families (4,000,000 sequences) before being stopped for the reviewed timer improvement. Their partial next-family counts
are unknown. Scratch differential fixtures that panicked or reset the checker incorrectly were
repaired; those diagnostics are not passes or omitted acceptance histories.

Question 171 remains open: late-invalid receive-output events are explicitly outside the oracle's
supported domain. Initial receive validation and settled call/reply output contracts are tested.
Questions 164–166 remain open, as do the documented 128/146 model conventions. Sparse reply-mask
witnesses and direct accounting/PID checks are not complete generated syscall traces. Real-kernel
replay, native timer and concurrency acceptance remain separate work; host checks do not close
IPC acceptance, server boot integration or native clean bundle-file readback.

## WP-K5 scheduler and time rules (2026-09-24)

The model now implements the owner-approved WP-K5 decisions (K5-plan, OWNER DECISIONS 1-7 and 9):
the preemption points and I13 reading, the floor, the four rank clauses, tick-style charging with
an exact remainder, pass inheritance with additive normalized debt measured from entry, stride
weight as free weight with the carve and `process_create` refusals, and the kernel's boot weight
split (root 1,000,000 with 1,000 free, system 250,000, users 749,000).

`Mutation::ALL` grows from 101 to 120: 19 new breaks, one per new rule (mutation.rs, R12, R7, I13
and Budget). Each is caught; `--test mutations -- --nocapture` names the property. The
`budget churn` scenario's spinning-parent and deadline-timed variants are required to catch
`R12LiftByMax`; `check::churn_variants` records that the blocking churner alone lets it survive.
The `debt lift` bound is one round (K5-debt-lift-bound), not two slices.

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -p redoubt-model --release` | Passed: lib 6, coverage 1, current contracts 16, map_fixed 10, mutations 2 (all 120 detected), policy 7, properties 6 (1 ignored), traces 4. |
