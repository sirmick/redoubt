# BEAM12 report: a child budget takes the session's labels

Branch wp-BEAM12, worktree /home/mcloonan/redoubt/.worktrees/BEAM12, from main cc51f76ad. Two
commits: 685422670 (fake kernel), 6fa630ff9 (beamlet). Tier B by the plan (no kernel change).

## The rule, from the kernel

kernel/src/budget.rs `budget_create` (lines 867-885): the spec's labels are sorted and
deduplicated; if they do not contain every one of the parent's, `LabelDenied`; if they differ from
the parent's at all and the caller's own budget is not `system`-class, `ClassDenied`. So a
`user`-class caller (every session) can only make a child with exactly its parent's labels.
docs/kernel/budgets.md "Labels on budgets" says the same. The VM's `labels()` is the kernel's
stamp on its own send, i.e. the labels of the budget it runs in, which is the named handle
`budget` it carves from (beamlet.md "Natives").

So: labels left out are the VM's own; labels given go to the kernel unchanged, which refuses fewer
(`label_denied`) and more (`class_denied`) for a session. "Narrowing" (adding labels) is allowed
only where the kernel allows it (a `system`-class caller, which no session is); "widening"
(shedding one) never. beamlet adds no check of its own: the verdict stays the kernel's.

## What changed

- libs/rt/fake/src/lib.rs: a budget carries labels (`Fake::budget` gives its owner's);
  `budget_create` is modelled with the kernel's user-class rule (record decoded, labels sorted and
  deduplicated, `LabelDenied` / `ClassDenied`), nothing charged. It panicked "does not model"
  before. Not counted by tests/unsafe-budget.toml (dev-only); its one new record read has a SAFETY
  comment like its siblings'.
- userland/otp/vm/src/platform.rs: `BudgetSpec.labels` is `Option<Vec<u64>>`, `None` = left out.
- userland/otp/vm/src/bif/system.rs: `budget_create/1` passes `None` when `labels` is left out.
- userland/otp/redoubt/src/system.rs: `None` becomes the VM's own labels (`sys.labels`).
- userland/otp/vm/tests/system.rs: the test platform's carve accepts left out or its own set
  ([7,9]); `budgets_are_carved_read_and_destroyed` asserts `labels: None`.
- userland/otp/redoubt/tests/system.rs: new `a_labelled_sessions_child_takes_its_labels_and_runs`.
- userland/shell/lib/redoubt/budget.ex: `carve/1`'s doc states the default and the refusals; it
  passes the map through and never fills in `labels`, so left out reaches the native as left out.
- docs/userland/beamlet.md (Natives: status 26 -> 27 with the new test; the `budget_create`
  bullet states the default and the kernel's refusals), docs/userland/native.md (Launching from a
  session: names the new test).

## The host test

For an unlabelled session and one labelled {5, 9}, on the fake kernel with a console of that label
set: `budget_create` with labels left out succeeds; `[9]` and an explicit `[]` are each `label_denied`
(labelled only: an empty set given is not left out, as the orchestrator's design note asks);
own + `[11]` is `class_denied`; a launch into the child is made, the child destroyed, and the job's
`{exit, killed}` event comes back. Before the fix (labels left out = `[]`), run on the new fake:
FAILED, `a child: Refused("label_denied")` at the labelled iteration.

## The machine case

Not shown on the machine, and it cannot be from this branch:
- beamlet-launch and its tester (beamlet-session, which carves labelled sessions) exist only on
  wp-BEAM4-cases (.worktrees/BEAM4-cases, cb6ed7944), not on main; per the instructions it was read,
  not edited.
- Even there, a labelled session's beamlet ends at its start with code 1: `Console::open` is
  ORDWR and consoled refuses a labelled caller's write-open of the UART console (R69); that
  branch's beamlet-natives-attack expects exactly that for `labels=7`. So no labelled VM can run
  the shell's `exec` on the UART: the case needs a console of the session's label set (an SSH
  channel, sshd's; or a labelled console in the tester), which is a follow-up for after BEAM4's
  cases and STEWARD2 merge. Once a labelled session can open its console, beamlet-launch with
  `labels=N` before `vm=beamlet_launch:start` in its args file is the case.

## Gates

On the committed tree, 6fa630ff9:
- `q run --quiet -- cargo test -p beamlet-redoubt --features fake` (userland/otp): rc 0, 56 passed,
  0 failed, the new test among them.
- `q run --cores 8 -- cargo test -p beamlet-vm` (userland/otp): rc 0, 77 passed (on 71237df51;
  the amend changed only beamlet-redoubt's test).
- `q run --quiet -- cargo test -p redoubt-rt -p redoubt-client -p redoubt-fake-kernel`: rc 0, 187
  passed (on 71237df51; the fake is unchanged since).
- `make -f scripts/jobs.mk prebuilt`: rv64 218 cases, rv32 204, 0 failed.
- `make -k -f scripts/jobs.mk`: PASS rv64 and rv32 beamlet-files and beamlet-boot; PASS docs,
  formatting, size-budget, unsafe-budget, no-cruft. rc 0.
- "beamlet-vm" is no bench case; it was taken as the beamlet-vm crate's host tests above.
- Not run: the beamlet-launch machine case (see "The machine case").

## Summaries checked

- docs/userland/beamlet.md "Natives": updated (above).
- docs/userland/native.md "Launching from a session": status names the new test; prose unchanged
  (it says nothing about labels).
- docs/kernel/budgets.md: the rule's owner; unchanged (no kernel change).
- docs/plan/m1-separation.md (natives "built and tested on the host; no boot runs them"): still
  true on main; unchanged.
- docs/userland/shell.md (`labels()` row), docs/userland/sessions.md (vault sessions' labels):
  no claim about a carve's labels; unchanged.
- README.md, GETTING-STARTED.md: no claim about budget labels; unchanged.
- docs/testbench.md: says nothing of what the fake models; unchanged.
- userland/shell/lib/redoubt/process.ex: its default budget leaves labels out, so `run/3` now
  carves with the session's labels; doc needs no change.

## Red's note folded (2026-10-08)

The red (OK with notes at 6fa630ff9): `Fake::process` and `Fake::budget` kept the caller's label
order, while the kernel sorts at creation, so a fake process made with unsorted labels refused its
own correct child as `class_denied`. Folded into the fake commit (now 8552bef25; beamlet commit
17292b8b5, unchanged in content):
- `Fake::process` stores the set its labels name, sorted and without repeats (`set()`), and
  `budget_create` checks the spec's set through the same helper; a budget's labels come from a
  process or a parent, so both are sets.
- New libs/rt/tests/budgets.rs, `a_child_names_its_parents_set_in_any_order`: a parent made from
  `[9, 5, 9]` carves `[5, 9]`, `[9, 5]` and `[9, 5, 5]`, and refuses `[9]` and `[]`
  (label_denied) and `[11, 9, 5]` (class_denied). Under the old fake it would be class_denied for
  `[5, 9]`.
- libs/rt/tests/ipc.rs `call_lend_and_reply` asserted a message stamped `[3, 1]` for a process
  made with `[3, 1]`; the kernel stamps the sorted set, so it now expects `[1, 3]`.
- Host, pre-rebase: rt + client + fake + bootfsd, consoled, erofsd, ipd, keyd, littlefsd, walfsd
  tests 422 passed, 0 failed; beamlet-redoubt 56 passed. The gates rerun after the rebase onto
  main, when the orchestrator says so.

## Rebased onto main 91256b4d0 (2026-10-08)

Head dcd501e14 (8b75c65ff fake kernel, dcd501e14 beamlet). Range-diff: the fake commit is identical
(`=`); the beamlet commit differs only in two status lines that conflicted with BEAM4's cases'
merge: beamlet.md "Natives" keeps main's text ("the machine's cases run the VM under a tester in
the steward's place...") with the count 30 -> 31 for the new host test; native.md "Launching from a
session" keeps main's text and bench:beamlet-launch, with the new host test added.

Gates on dcd501e14 (all through q; scratch under /home/mcloonan/redoubt/.tmp/BEAM12):
- host: rt + client + fake + bootfsd, consoled, erofsd, ipd, keyd, littlefsd, walfsd 423 passed,
  0 failed; beamlet-redoubt 56/0; beamlet-vm 77/0.
- prebuilt: rv64 230 cases, rv32 216, 0 failed.
- PASS rv64 and rv32 beamlet-files; PASS docs, formatting, size-budget, unsafe-budget, no-cruft.
- FAIL rv64 and rv32 beamlet-launch at 1.0 s: `init: refused the boot: beamlet-session: could not
  make a fresh connection for` (servers/init/src/bin/init.rs:484, the steward slot's fresh
  connection at a server it is handed). Its rv64 siblings beamlet-natives, beamlet-serve and
  beamlet-natives-attack fail identically. The boot stops in init before any beamlet runs, and
  this branch changes neither init, the manifests, the servers nor the tester, so it is main's:
  K23's 4a240738f (init makes the steward's fresh connections) against BEAM4's cases' manifests
  (tests/data/beamlet/*.json). Deterministic (1 s, before any guest timing), so not a host-clock
  flake.

## Rebased onto main ac178530e (B27, B29, SHELL2 in)

Head f5c051e10 (b6eb91274 fake kernel, f5c051e10 beamlet). Clean rebase; range-diff `=` for both.
Gates on f5c051e10 (q; scratch /home/mcloonan/redoubt/.tmp/BEAM12): host rt + client + fake + 7
servers 423/0, beamlet-redoubt 56/0, beamlet-vm 80/0; ./test-shell rc 0, every stage passed;
prebuilt rv64 230 / rv32 216, 0 failed; PASS rv64 and rv32 beamlet-launch, beamlet-files,
beamlet-serve; PASS docs, formatting, size-budget, unsafe-budget, no-cruft. beamlet-launch's main
failure is gone with B27.
