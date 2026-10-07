# MODEL1 report (in progress)

Branch wp-MODEL1, worktree /home/mcloonan/redoubt/.worktrees/MODEL1, rebased onto main ba4aabd8b
(K25 included). Two commits; see "Final" at the end.

## Point 1: where a steward seed's time goes (measured)

Method: perf is refused (perf_event_paranoid 4) and there is no valgrind, so (a) stacks sampled
with gdb as the parent (SIGINT every 100 ms; release build with line tables,
`CARGO_PROFILE_RELEASE_DEBUG=1`, separate target dir), 300 samples of `steward_policy` and 150 of
`steward_noninterference`, one model thread each; (b) a scratch test (not committed) timing
`Run::apply` per op kind over seeds 0..50, release, one thread, through `q run --cores 2`.

Baseline, release, one thread, seeds 0..100 through `q run --cores 1`:
`steward_policy` 43.4 s (0.43 s/seed), `steward_noninterference` 73.3 s (0.73 s/seed).

Split of `steward_policy` seeds 0..50 (0.431 s/seed):

| Op | Ops | Share of time | Per op |
| --- | --- | --- | --- |
| Tick of 1-5 minutes | 182 | 99.5 % | 117.8 ms |
| Tick of 1-400 ms | 203 | 0.1 % | 77 us |
| every other op (17 kinds: core decision, embedder batches, clones, all 13 property checks, kernel checker) | 4,348 | 0.4 % | 12-28 us |

Simulated time: 12.9 minutes per seed. Stacks: 100 % of samples under `Steward::tick` ->
`Kernel::step(Op::Tick)` -> `Kernel::tick`; inside it `Scheduler::reconcile` 50 %, `raise_floor`
45 %, `pick` 36 %, `slice_end`/`deschedule` 18 %, `next_event` 16 %; leaves are BTreeMap
iteration. `steward_noninterference`: the same (100 % under `Kernel::tick`, reconcile 45 %,
raise_floor 43 %, pick 35 %).

Why: `Kernel::tick` (kernel.rs:2346) charges time slice by slice (SLICE = 1 ms) whenever more
than one budget is runnable; its batched path applies only with one runnable budget. In the
steward world init (root), the steward and server (system) and every session and lease process
(its own budget) each keep a runnable thread, so a 3-minute tick is ~180,000 slices at ~0.65 us
each. Nothing a steward property observes depends on that CPU simulation: `Counters` carry pages,
processes and weight only, no CPU time; deadlines and timeouts fire at instants regardless.

So: the core's own cost is negligible (well under 1 % of a seed); the embedder's clones and the
property checks are under 0.4 % together; the cost is the kernel model's tick, called by the
steward embedder. `kernel.rs` and `sched.rs` are outside MODEL1's owned paths.

## Point 2: the batched tick (orchestrator's ruling (a))

`Kernel::tick` (kernel.rs) gains a second batch: with several budgets queued, a fresh slice,
nothing to pump, nothing due and the queue steady (`Scheduler::steady`: queued exactly when
runnable), the whole slices up to the next event run on `Scheduler::run_queue_slices` (sched.rs):
the same picks (lowest (tier, pass, tie, id), next thread after the cursor), charges (pending plus
the slice, MIN_CHARGE, the weight or 1, the remainder), requeues (back/front/LIFO ties) and floor,
on a copy of the queued entries only. Proof: `event_free_tick_matches_slice_reference` extended
with three kinds of four queued budgets of unequal weights and several threads (alone, up to a
timeout, up to a budget deadline) and a 60,000,123 us tick, over all 151 mutations and None
(13,832 boundary cases + 64 histories; after the rebase over K25, all 153: 14,014 cases); a guard asserts the unmutated cases are steady with four
budgets queued (not vacuous); a deliberately wrong batch (requeue counter not advanced) fails it.

Seed cost after, same method (release, one thread, seeds 0..100 through `q run --cores 1`):

| Family | Before | After |
| --- | --- | --- |
| `steward_policy` | 0.43 s | 0.011 s |
| `steward_noninterference` | 0.73 s | 0.025 s |

Split after (seeds 0..200): 9 ms a policy seed; minute ticks 2.1 ms each (82 %), the rest ~1.6 ms.

## Point 3: coverage (model/tests/steward_reach.rs, release, 4 threads)

Instrument: a `Policy` table whose 21 guards (held / refused), 47 effects and 2 filters record
what ran, plus `Run::reached` property instances (P1-P14 tags). Unmutated runs to the current
counts: steward_policy 20,000 seeds in 96 s, steward_noninterference 10,000 in 81 s (4 threads).

| Family | Items reached | Last new item (seed) | Never reached within the count |
| --- | --- | --- | --- |
| steward_policy | 113 | `effect audit_start_failed` (655); before it P6 declassification/copy out (121) | effects audit_copy_failed, audit_push_failed, pass_failure; filter audit_visible (not called by this family) |
| steward_noninterference | 116 | `effect audit_start_failed` (1,295); before it `rule item_fits refuses` (223) | effects audit_copy_failed, audit_push_failed, pass_failure |

Catch seeds, random search (baseline binary, `mutations_are_caught` order, Policy + R2OneCursor):
every Policy variant caught by seed 224 in its preferred family (most at 0-34); late ones, each
family searched to its count:

| Mutation | steward_policy | steward_noninterference |
| --- | --- | --- |
| PolicyDeclassifyUnfit | 4,709 | 6,333 |
| R2OneCursor | not caught (20,000) | 345 |
| PolicyAgentOtherSet | not caught (20,000) | 96 |
| PolicyEndLeaseAdmitted | 224 | (not searched) |
| PolicyDeclassifyLive | 153 | (not searched) |

`item_fits` first refuses at seed 72 (policy), but its mutation is caught only at 4,709: refusing
needs an unfit item submitted; catching needs it approved and copied out too.

## Point 4: directed scenarios (model/tests/common/contracts.rs)

`declassify_unfit` (both an over-long and an unprintable item; P6), `one_cursor` (P10, take order),
`agent_other_set` (P10, unlabelled audit view): each holds unmutated and catches its mutation in
under 10 ops; `steward_contracts` runs before the families in `mutations_are_caught`; named tests
`declassify_unfit_scenario`, `one_cursor_scenario`, `agent_other_set_scenario` in mutations.rs.

## Final (head 8a5c43910 on main ef2ab5a94, three commits)

- 10b719a2a model: a tick charges the whole slices of several queued budgets at once (kernel.rs tick,
  sched.rs `steady`/`run_queue_slices`, equality test extended, model.md "What it abstracts"; size
  10,325 -> 10,403)
- 016ade38e model: directed scenarios catch the steward's three late breaks before the random search
  (contracts.rs `steward_scenario` + three scenarios, policy.rs P10 op source, mutations.rs order and
  three named tests, model-mutations.toml late entry and late_cores gone, deadline 25; model.md
  Mutations/Scripted contracts; ipc.md and SECURITY.md R2 list one_cursor_scenario; size -> 10,422)
- 8a5c43910 model: the steward families' counts follow what their seeds reach (Policy table hook in
  steward.rs, Run::reached tags, steward_reach.rs instrument + the_reach_table_is_reproduced, counts
  5,000/7,000, steward-model-host-tests 4 cores, whole_run=false gone, deadline 94; model.md cost
  table, counts rule, residual; testbench.md rows; size -> 10,509)

Cost per seed, release, one thread (q run --cores 1, seeds 0..100):

| Family | Before | After | Count | Time on one core |
| --- | --- | --- | --- | --- |
| steward_policy | 0.43 s | 0.011 s | 20,000 -> 5,000 | ~1 min |
| steward_noninterference | 0.73 s | 0.025 s | 10,000 -> 7,000 | ~3 min |

Gates at head, all exit 0:
- `q run --cores 4 -- cargo test -p redoubt-model --release --lib`: 8 passed; tick differential 14,014
  boundary cases, 64 histories, 2,770 history tick comparisons, all 153 mutations.
- `make -f scripts/jobs.mk prebuilt` 0; rv64/docs 0, rv64/formatting 0, rv64/size-budget 0,
  rv64/no-cruft 0, rv64/unsafe-budget 0 (model has no unsafe).
- rv64/model-host-tests 0 (116 s wall incl. build).
- rv64/steward-model-host-tests 0 (95 s wall incl. build; jobs: noninterference 43.4 s, policy 12.3 s,
  reproduction 7.7 s, 4 cores each). Acceptance: under 10 min on 4 cores.
- rv64/model-mutations 0: 153 jobs, 46 s wall, none late; longest R2NoWaitCap 13.2 s (deadline 25 s).
- Not run: rv32/both builds and the smoke set: only the host-only model crate, its tests, case files
  and pages changed; no kernel, sys, rt or server crate.

Documentation check: model.md (What the model is/abstracts, Property families, Scripted contracts,
Mutations, Residual risks), testbench.md (heavy host cases table, whole_run sentence, late_cores),
ipc.md R2 status, SECURITY.md R2 row updated. Checked, no change needed: README.md, GETTING-STARTED.md,
docs/plan/m1-separation.md progress (claims the model and its mutations, no counts or cost),
GLOSSARY.md model/mutation, model/README.md, servers/steward.md (policy core status, R37/R42 planned
sections, model-*.trace seeds: events unchanged since the batch is exact).

Open risks / notes:
- Counts rule: 4x last reach alone gives 3,000/6,000; 5,000/7,000 come from the floor at the random
  catch of PolicyDeclassifyUnfit (4,709/6,333); the page states both.
- Never reached by either family: audit_copy_failed, audit_push_failed, pass_failure; audit_visible
  not called by steward_policy (residual on the page, no generator change).
- kernel.rs (4,100 lines) read where changed (tick, at_instant, next_event, settle, the test module),
  not whole, per the context rules; sched.rs, policy.rs, steward.rs, contracts.rs read in full.
- The steward case runs in dev, where the core is unoptimised: 0.04 s/seed noninterference there.
