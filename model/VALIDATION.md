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

### Review fixups (K5-code-review-1)

- **Owner decision 5, at least one unit per deschedule.** Both the model (`sched.rs`, `MIN_CHARGE`)
  and `redoubt-stride` (`Cpu::switch`) now apply it. A new mutation, `R12NoMinimumCharge`, is caught
  by a focused `sched_contracts` case: ten zero-length runs must each move the pass. That brings
  `Mutation::ALL` to 121. A destruction is not a deschedule, so a budget destroyed on the CPU is
  charged what it ran and nothing more, in the kernel and the model alike.
- **(k) shell.** The shell now starts each command from inside its own slice, holding the lead that
  run gave it, after one to three back-to-back commands that end before they run.
  `R12LiftCountsEntryWait` is now caught on 3000 of 3000 shell seeds (residue 9 of 30,000); before,
  it was caught on none.
- **(b) wakes never preempt.** Rank runs now stop part-way through a slice about half the time, so
  a later wake finds a thread running. `R12PreemptOnWake` is caught on 2959 of 3000 rank seeds;
  before, on none.
- **(d) gaming.** At weights above STRIDE, the gamer's every turn is a burst shorter than
  `w / STRIDE`. `R12DropRemainder` is caught on 1376 of 3000 gaming seeds; before, on about 1 in
  135.
- No unmutated scenario fails on any residue of 30,000 seeds.

The differential (`libs/stride/tests/differential.rs`) now drives `redoubt_stride::Cpu`, the
wiring the kernel's `sched.rs` calls, rather than a copy written for the harness. It destroys the
running budget with its threads (nothing deschedules it first), and whole subtrees bottom-up in
one step. It still agrees over 3000 seeds; a model with any of 18 scheduling rules broken
disagrees. Bugs planted in `Cpu` are each caught at seed 0: the minimum charge dropped; the
running budget destroyed without its runtime charged; a carve returned without folding first.

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -p redoubt-model --release` | Passed: lib 6, coverage 1, current contracts 16, map_fixed 10, mutations 2 (all 121 detected), policy 7, properties 6 (1 ignored), traces 4. |
| `cargo test -p redoubt-stride --release` | Passed: 10 unit tests; the differential over 3000 seeds; 18 broken models all disagree. |

### Destruction order (K5-code-review-4 D1, the orchestrator's ruling)

The top of a destruction returns its carve to its parent first (`Scheduler::return_carve`, called
at the start of the kernel model's R10), before any of the destruction's work; the budgets below
it return theirs at their own bottom-up step. The kernel does the same at `mark_dying`, and the
differential's leaf and subtree destructions return the top's carve first on both sides; it
agrees over 3000 seeds. Default suites pass again, with all 121 mutations detected.

## WP-K5b DMA device reset and frame quarantine (2026-09-25)

The model implements answer 173 as the K5b plan rules it. `dma_alloc` frames are held until
their process ends (OD2), and they cannot be lent, transferred or moved by `process_map`. At
death the process's reset set S (OD3) is reset, and its frames are pooled only if every device in
S confirmed in that same call. Otherwise all of them are quarantined (P1-1), and so is each device
that failed to confirm. A quarantined device is refused to `map_device` and `dma_alloc` (OD6).
A quarantined frame's charge moves to the destroyed top's parent after the carve returns (OD5, N1).
`Boot::default` appends a DMA device whose first reset fails, so the handles init creates
start at 10, not 9; the example trace was re-recorded for that and nothing else.

The new invariant, I-DMA, arms each DMA frame against every device in its holder's S. A device is
disarmed only when its object shows it genuinely reset, and no free frame may still be armed.
`Mutation::ALL` grows from 121 to 126 with five K5b breaks, each caught by `kernel_sequence`
(`--test mutations -- --nocapture`). The setup now sometimes hands a child a DMA device, so the
co-holder and reuse paths come up. A scripted trace (`dma_quarantine_trace`) makes the OD6 break
visible to trace replay, and `tests/dma_contracts.rs` scripts OD2, pooling, P1-1 and the
parent-at-its-limit case.

| Command | Result |
| --- | --- |
| `cargo test -p redoubt-model --release` | Passed: lib 6, coverage 1, current contracts 16, map_fixed 10, mutations 2 (all 126 detected), policy 7, properties 6 (1 ignored); traces 4 and dma contracts 4 after the trace fixes. |
