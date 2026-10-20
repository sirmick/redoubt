# KILL1 report (phase 2, the kernel arm): no kernel defect; the kill is a lease's own deadline under the bench's virtual clock

Branch `wp-KILL1` in `.worktrees/KILL1`, on origin/main ddafcad7d. Head: see the final line.
Cause and evidence: `.wash/local/KILL1-cause.md` (the probe's lines, four boots, QEMU's source).
Logs: `.tmp/KILL1/` (`probe{1,2,3,4}/`, `gates-run{1,2,3}/`, `model-siblings-*.log`).

## Delivered

1. **The kernel model's sibling contract** (`model/tests/destroy_contracts.rs`,
   `model/tests/common/contracts.rs::sibling_destroy_spares_neighbours`): sixteen children of
   one parent, a process each reporting to init's endpoint, each ending as a lease ending by
   exit does, in the gate's three orders plus newest-first and interleaved; after every end each
   other child's budget counts its process and the process lives. Passes; teeth shown with the
   model's destroy made to reach the parent's other children: fails at the first end
   (`model-siblings-teeth.log`), the mutation reverted. Listed on model.md "Scripted contracts".
2. **The identification** (the probe, removed per the ruling): every kill is R10 at a
   **deadline**, of the victim's own budget, its counted chain exactly its own VM's process, the
   parent's child list intact. The guest's `time` counter skipped from ~575 s to the oldest live
   lease's deadline (1912083242 µs for 1912083218) the first time the guest idled with no timer
   armed nearer; the case on the host's clock passes in 78 s with all sixteen `exited 0`.
3. **The case** `tests/lease-code-load.toml`: on the host's clock (`icount` removed, the reason
   in its comment), `whole_run` back on, its waits sized from the measurement below.
4. **The pages:** testbench.md "The case file": the `sleep on` sentence replaced by the measured
   behaviour (an idle guest's clock skips to its next armed deadline, sleep on or off; with sleep
   on measured once the guest idled with nothing nearer than a lease's deadline) and the rule for
   authors (a case under the virtual clock whose guest can idle with a live lease it means to
   outlive runs on the host's clock or expects the lease's end). scheduling.md "With the real
   steward" (sixteen of sixteen on the host's clock, the clock finding named), erofsd.md
   "Serving" (the same), steward.md "Residual risks" (the bullet rewritten as the bench-clock
   residual with R39 kept and the kernel's walk shown bounded), model.md (the contract listed and
   described). SECURITY.md unchanged (R10/R39 rows name no changed test).

## Not changed, and why

- `kernel/`: nothing. `destroy_subtree`, `subtree_next`, the counted chains and the
  process-budget match were shown sound by the probe; no fix, so no unsafe or size movement.
- `servers/steward/`: nothing (phase 1 excluded its arms; the probe confirms).
- The probe (feature `kill-probe`, the scratch case) lives only on the local branch
  `kill1-probe` (0237ceb2a, never pushed) and is deleted with this report; KILL1-cause.md keeps
  how it was done.
- testbench.md's case counts ("145 of the 226", "68 of them", "38 cases", "(8)") were stale
  before this package (347 boot cases today, 186 under `icount`) and are not recounted here.

## Summaries checked

README.md, GETTING-STARTED.md, docs/README.md, docs/testbench.md (the clock paragraph changed;
nothing else names the case), docs/kernel/budgets.md (R10: no claim moves), docs/kernel/model.md
(changed), docs/kernel/scheduling.md (changed), docs/servers/steward.md (changed),
docs/servers/erofsd.md (changed), docs/servers/verityd.md and walfsd.md (name the case's
measurements only; no claim about the kill), docs/SECURITY.md (unchanged).

## Gates (every one through `make -f scripts/jobs.mk set CASES=...`, exit codes in the logs)

- `lease-code-load`, host clock, both widths at one and two harts: sixteen boots, sixteen PASS
  (`gates-run1..3`: 72.6 to 85.1 s each; `gates-run4`, after the waits were resized to the
  measurement: 70.9 to 80.5 s). The twelve first measured the resize; run 4 confirms it.
- `model-host-tests`: PASS, 174.8 s (`gates-run1`), the new contract in it.
- `docs`: PASS (`gates-run1`, `gates-run4`, `gates-run5` on the final tree).
- `formatting`: FAIL in run 1 on one over-long line of phase 1's steward test, formatted with
  `cargo +nightly fmt --all`; PASS in `gates-run4` and `gates-run5` (the final tree).
- Not run, with the reason: `size-budget` and `unsafe-budget` (nothing of the kernel or any
  binary changed: a test file, the model, a case file and pages); the steward set and the
  latency cases (no steward or kernel change); `kernel-host-tests` (no such case in `tests/`;
  the model's tests are `model-host-tests`); release checks (no binary changed).
- Model contract teeth: `model-siblings-teeth.log` (fails under a destroy that reaches the
  parent's other children; mutation reverted, not committed).

## Files read

Every delivered hunk was read in full (`git diff ddafcad7d..HEAD`). Of the files touched,
`servers/steward/tests/steward.rs` (phase 1's test, ~2,600 lines) and
`model/tests/common/contracts.rs` (~1,000 lines) were read in their changed regions and the
helpers they call, not whole; the pages in the sections changed and every passage naming the
case or the finding (`grep` over docs/ for the case name, "fifteen of sixteen", "killed").

## Commits (wp-KILL1, base ddafcad7d)

1. `steward:` the fake kernel's handle table modelled whole; sixteen leases ending by exit
   destroy each their own budget (phase 1's test, kept: teeth shown in
   `.wash/local/KILL1-identification.md`).
2. `model:` the sibling-destroy contract, model.md listing it.
3. `testbench:` lease-code-load on the host's clock, whole run, waits from the measurement;
   testbench.md, scheduling.md, erofsd.md, steward.md corrected.

Head: 9aadcd7c3. Nothing pushed. Reviewer: kernel-red-4 (the kernel test file is the model's).

## Open

- testbench.md's case counts in "Which cases run in guest time" are stale from before this
  package (347 boot cases today, 186 under `icount`); left as found.
- Why the bench's QEMU skips an idle guest's clock with `sleep` on is not identified (the
  measured behaviour is on the page without a mechanism); the cause file keeps the two source
  paths read.
