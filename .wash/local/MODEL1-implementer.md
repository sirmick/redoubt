# MODEL1: the steward families cost 0.6 and 0.94 s a seed; three mutations are caught only after minutes

Tier A (the model: `model/src/{steward,policy,check}.rs`, its tests, its page), size M. Needs
B18 (reported: the mutations fanned out one job each in release; the steward families in their
own bounded case). Start from `main` once B18 is on it. Run everything through `q run`.

## Context rules (read these first)

- **Measure before you change anything, with the same method after.** B18's numbers are the
  baseline (`.wash/local/B18-report.md`, "Findings that shaped it"): `steward_policy` 0.6 s per
  seed and `steward_noninterference` 0.94 s per seed on one thread, release and dev the same;
  the kernel families under 1 ms per seed; `PolicyDeclassifyUnfit` caught at `steward_policy`
  seed 4,709 after 1,720 s; `R2OneCursor` and `PolicyAgentOtherSet` uncaught after 48 min when
  they searched other families first, caught by `steward_noninterference` at seeds 345 and 96.
- **Don't read whole files.** `model/src/steward.rs` (878 lines) and `policy.rs` (1,299) by
  function as the profile names them; `check.rs` only its family runner; `model/tests/common/
  {mod,contracts}.rs` for the runner and the mutation order; `model/tests/properties.rs` for
  the seed counts (`family(3, 20_000)`, `family(4, 10_000)`); `mutations.rs` for the
  "preferred family" table.
- **Don't open `.wash/qa/*.md` or other packages' reports** but B18's.
- **Reports under 1,900 bytes,** detail in `.wash/local/MODEL1-report.md`.

## Reading list (only these)

- `docs/kernel/model.md` "Property families", "Mutations", "The steward model" (the property
  table, what a seed runs: 20 to 160 policy operations, `steward_noninterference` running one
  sequence twice), "Residual risks"; `docs/servers/steward.md` "The policy core" (the crate the
  model embeds: a cost there is the shipping core's cost too).
- `.wash/local/B18-report.md`: "Findings that shaped it", "Design problems and open risks".

## The design (the page rules; this is how to build it)

1. **Profile one steward seed,** release, on the host (`perf record` on the test binary over a
   fixed seed range, or `cargo flamegraph` if present; else `std::time::Instant` spans at the
   embedder's step boundaries), and attribute the 0.6 s: the core's decision per operation, the
   embedder cloning the core's or the kernel model's state per step (33 `clone()` sites in
   `steward.rs` and `policy.rs`), the properties' checks after each operation (the table has
   thirteen; `non-interference` compares everything an unlabelled session observes), the kernel
   model's calls, allocation. Write the table before touching anything.
2. **Bring the cost down where it is accidental,** and only there: a clone per step that a
   borrow or a diff replaces; a property check that recomputes a whole view per operation when
   an incremental one is exact (say why it is exact); allocation in the hot path; a comparison in
   `noninterference` done on a snapshot per step where one at the end suffices (only if the
   property's statement allows it: the page says "after each"; if the check must stay per step,
   it stays). The core's own cost (`libs/steward`) is reported, not changed here: a slow core is
   the steward's package. Target: a seed under 0.1 s in release for both families, stated as
   measured; if the floor is the core's, report the split and stop at the floor.
3. **Seed counts from measured coverage, not habit.** Instrument the runner (a feature or an env
   var, test builds only) to record, per family, the seed at which each rule's check, each
   property and each reachable mutation was first exercised (the runner already keeps the lowest
   failing seed; this is the lowest *reaching* seed). Run each steward family to its current cap
   once with the instrument and tabulate: the seed at which the last new rule or property was
   reached, and the seed at which each mutation of the `Policy` and R2 kind is caught. Set each
   family's count at a stated multiple of the last-reach seed (recommend four times, rounded up
   to a thousand), with the table on the page; never below the seed that catches the slowest
   mutation of its kind. The kernel families keep their counts (under 1 ms a seed, they are not
   the wall) unless the table says a count is vacuous.
4. **The three late-caught mutations:** a directed scenario each (a scripted contract in
   `contracts.rs`, as the `ipc_contracts`/`sched_contracts` ones) that catches it in seconds:
   `PolicyDeclassifyUnfit` (a declassification of an item that does not fit the rule's shape),
   `R2OneCursor` (two senders' turns on one endpoint: the group cursor), `PolicyAgentOtherSet`
   (an agent reaching another label set's state). Each scenario is tried first in
   `mutations_are_caught` for its mutation (the "preferred" table gains them), so the random
   search is the backstop, not the proof. If one cannot be written because the model cannot
   distinguish the mutation from the rule (the random search catches it only by an accident of
   state), write that as a residual on the page with the seed and the reason, and the mutation
   keeps its random catch bounded by point 3's count.
5. **The page states the cost and the budget:** model.md "Property families" gains a table of
   per-family cost per seed in release (measured, both before and after), the seed count and
   the time it buys on one core; "Mutations" states the three directed scenarios and that every
   mutation is caught inside the bench's bound; the status lines gain the scenarios' tests.

### The rules it keeps

Nothing the model checks weakens: every property in the steward table and every kernel contract
stays as stated; a seed count may fall only on the measured table of point 3; a per-step check
becomes end-of-run only where the property's words allow it, and the page says so.

## The cases (host only)

`model-host-tests` (B18's bounded case) under its new counts; the directed scenarios as named
tests on the mutations list; a host test that the coverage instrument's table is reproduced
(the last-reach seeds are deterministic for a fixed generator). Acceptance from the node: the
steward case under 10 min on 4 cores; every mutation caught inside the bench's bound (the
mutation job's deadline B18 set), both measured through `q run` and reported with the commands.

## Page lines (exact text in the report)

model.md "Property families" (the cost table and the counts), "Mutations" (the scenarios, the
bound), "The steward model" (any check moved from per-step to end-of-run, with its reason),
"Residual risks" (an indistinguishable mutation, if any); `testbench.md` where the model case's
bound is stated (B18's paragraph) with the new numbers. No dates, package IDs or review
history.

## Owned paths

`model/src/{steward,policy,check}.rs`, `model/tests/**` (the runner's instrument, the
scenarios, the counts), `docs/kernel/model.md`, `docs/testbench.md`'s model paragraph, the
model's rows in `tests/size-budget.toml` if its ceiling moves. **Not yours:** `libs/steward`
(the core: report its cost, change nothing; a change there is the steward's package),
`model/src/kernel.rs` and the kernel families' semantics, `mutation.rs`'s variants (add no
mutation, retire none), the bench's fanout (B18's `jobs.mk` and `q`).

## Gates

The short gate: both builds unaffected (the model is host-only; say so); host tests of
`redoubt-model` whole, through B18's fanout, with the wall and core-time reported before and
after; the docs checker, `cargo fmt --check`, the size budget, the `unsafe` ratchet (the model
has none), the no-cruft gate; the smoke set unchanged. The whole bench is the train's.

## Not here

A change to the steward's policy core; a change to what any property states; new mutations; the
kernel families' counts unless the measured table says one is vacuous; the fanout's scheduling
(B18's and `q`'s); the model's trace format.

## Checkpoint

After point 1's profile table and point 3's coverage table, before any code change: one progress
line with both tables' headlines (where the 0.6 s goes; the last-reach seed per family; the
catching seed of each late mutation), so the counts and the targets are ruled on measurements.
