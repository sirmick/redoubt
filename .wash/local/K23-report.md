# K23 report

Two branches, never pushed.

| Branch | Base | Commit |
| --- | --- | --- |
| wp-K23 | main 2151b2aa4 | e97bfe82e kernel, model: budget_reap empties a budget one child at a time and keeps it |
| wp-K23-steward | wp-STEWARD2 d9627acc8 | 0bc785c7e (the same commit, cherry-picked; ceilings and mutation count for that base) |
| | | 17b1633fd init, steward: a dead steward's restart empties users first, a logout and no reboot |

When STEWARD2's rebased tip is announced, 17b1633fd moves onto it above e97bfe82e. Expect
conflicts in tests/size-budget.toml (ceilings: re-measure) and model/src/mutation.rs (`ALL`'s count).

## The call (main)

`budget_reap(h) -> remaining` is call 28 (a0 0x11c) and fails only on its handle
(`BadHandle`, `WrongObject`). The kernel's `reap_begin` reads the budget's first child. That is
the newest, since `new_budget` links at the head. It runs `mark_dying` and then `destroy_subtree`
on that child, both unchanged (K19's walk is used as is). The call returns `children()` of the
kept budget, and does not return if the caller is doomed. The work is billed to the caller as
call time.

A child's handle reaches only below that child. A budget with no children returns 0 and nothing
changes.

The model has `reap_budget` beside `destroy_budget` and picks the max-id child, the kernel's
order. Its generators draw the call: hostile index 25, and 30 % of the destroy slot. A contract
`reap_empties_and_keeps` runs in `ipc_contracts` and as
host:redoubt-model::a_reap_destroys_one_child_and_keeps_the_budget. All three mutations are
caught by that contract with seed 0:

- R10ReapDestroysParent: "one child left".
- R10ReapKeepsCarve: the R6 recount.
- R10ReapSkipsGrandchildren: the R6 recount.

bench:budget-reap is a single judge program. Its verdict comes from the kernel's own results:
the counts, the usage records, BadHandle on swept handles, the exit notice, the caller's
CallOutcome (Dead, Returned, page intact), the sender's Dead, a receive that times out, and the
checked build's audits. It covers:

- a child handle;
- a held call with a lend;
- a stamped send;
- usage back to the empty value;
- an empty budget;
- WrongObject and BadHandle.

## The restart (STEWARD2-based)

Step 1 needed nothing new. init's watching thread already destroys every dead instance's
budget, and `start` carves a fresh one, for every server. The sessions' process objects are
charged to the steward, so R10 step 3 frees them and kills their processes, with no notice owed.

`restart()` then runs `empty_users()` for the steward only. It is gated on users' usage, since
any child costs its own page in it. It loops `restarts::empty` over `Budget::reap` and says
`init: emptied users: N budgets reaped`. A refused reap reboots. Then `start`.

The steward's `users not empty` check is unchanged. The steward now says
`steward: users holds N pages, M processes` at its start.

The case's construction is the test-only steward feature `restart-probe` (orchestrator's ruling a).
A login for principal `steward-restart-probe` calls `process_exit(9)` while the steward holds
sshd's call, so init reports it `faulted, code 9`, as littlefsd-restart's is.

`steward-restart`'s verdict, in order:

1. faulted, code 9
2. emptied users (2 budgets on both widths: alice, bob)
3. restarted
4. users holds 0 pages, 0 processes, read at boot and again after the restart
5. carved
6. the console's `Redoubt shell` banner again
7. no reboot, no `users not empty`, no exit code 3

`steward-restart-reboot` is the heap-capped construction. users is emptied each time, the new
instance carves and dies the same way, and the fifth restart reboots.

Ruling (a): an sshd channel outlives its reaped session. sshd ends a channel only on the
steward's `ended`, which a dead steward never sends. Even closing ssh's input does not end it.
This is a residual on steward.md and sshd.md, and K26 is filed by the orchestrator.

Timings. The bench keeps no log of a passing run, so these come from the last failing rv64 run
of the same code, which still had alice's SSH session:

- The steward's audit stamps alice's login at 4,400,824 µs and the restarted console session's
  login at 6,761,949 µs of guest time.
- So the probe, the reap, the restart, the carve and the console launch fit in 2.36 s.
- That is an upper bound; the reap and the restart were not timed apart.
- Whole case wall time: steward-restart 4.9 s rv64, 5.4 s rv32 (icount);
  steward-restart-reboot 2.7 s and 3.4 s.

## Gates (through q / jobs.mk; exit codes)

On wp-K23 e97bfe82e (main-based):

- `make prebuilt`: rc 0 (rv64 206, rv32 192 built).
- Each of these is rc 0 on both widths:
  - budget-reap, budget, budget-destroy-kills, budget-destroy-attack
  - process-lifecycle, legacy-gone, budget-syscall-attack
  - the smoke set: userland-boot, init-boot, bench-net-peer, ipc-outcomes
- formatting 0, size-budget 0, unsafe-budget 0 (unchanged), no-cruft 0, docs 0.
- `q run --cores 4 -- cargo test -p redoubt-model --release -- --skip steward --skip mutations_are_caught`: 0.
- `REDOUBT_MODEL_MUTATIONS=R10,ExpireBudgetsFirst,BudgetDeadlineIgnored ... --test mutations`: 0 (14 caught).
- `cargo test -p redoubt-sys`: 0.
- `cargo test -p redoubt-kernel`: 0. The crate has no host tests; the kernel is tested by the bench.

On wp-K23-steward:

- `cargo test -p redoubt-init -p redoubt-steward-server`: 0, the three new init tests included.
- `cargo build -p redoubt-steward-server --features restart-probe`: 0.
- `q run --quiet -- cargo test -p redoubt-rt`: 0.
- prebuilt: rc 0.
- Each of these is rc 0 on both widths: steward-restart, steward-restart-reboot,
  steward-session-ends, init-restart, init-reboot, budget-reap, and the smoke set.
- unsafe-budget 0, no-cruft 0, formatting 0.
- docs 0 and size-budget 0 on the final head 17b1633fd. Docs failed on d9b258afa for STEWARD2's
  own sessions.md; d9627acc8 fixed it.
- Not run on the final head: the bench cases after the last rebase, which changed only STEWARD2's
  five doc and case-text files plus size-budget.toml.

Not run: the whole bench (the train's); the full mutation sweep (the train's, by instruction);
the steward model families.

## Size budget (each with its `Size budget:` line)

| Crate | Change | Ceiling | Base |
| --- | --- | --- | --- |
| kernel | +29 | 9,220 | main |
| libs/sys | +4 | 1,021 | main |
| model | +49 | 10,296 | main |
| libs/rt | +6 | 3,531 | STEWARD2 |
| servers/init | +23 | 2,394 | STEWARD2 |
| servers/steward | +14 | 1,111 | STEWARD2 |

## Summaries checked

- **Updated:**
  - budgets.md: the calls, the kernel-caller line, R10's paragraph, status, diagram and residuals;
    the dead-steward residual removed.
  - abi.md: call row, error row, unknown range.
  - kernel/README.md: the budget calls and the object table.
  - objects.md and scheduling.md: the billing line.
  - model.md: the mutation table.
  - SECURITY.md: R10's row.
  - init.md: status and the steward's order.
  - steward.md: failure, the probe paragraph, the residual added, the old one removed.
  - sshd.md: residual.
  - plan/m1-separation.md: the steward line.
  - SUMMARY.md, with docs/todo/empty-a-budget.md deleted.
- **No change needed:**
  - README.md, GETTING-STARTED.md, servers/README.md: no claim on the steward's restart.
  - beamlet.md's natives: no reap native.
  - timer.md, agents.md: they describe deadlines, not this call.
- **Found stale, not mine:** kernel/README.md's TCB size table already disagrees with main (it
  says kernel 10,403; the tree has about 13,750).

## Risks and notes

- `budget_reap`'s count walks the child list: O(children) per call. A holder emptying n children
  pays O(n²), billed to itself.
- init's gate on usage counts a users holding only moved quarantine or held PIDs as occupied. The
  line then says 1 reaped when none was.
- The probe case opens no SSH session across the restart (ruling a); K26 brings alice's channel
  back.

## Rebase and red round

wp-K23 was rebased onto main 850bddcc3 with no conflicts, giving 85cd200eb. From a fresh
prebuilt on that commit (rv64 216 and rv32 202 cases built), each of these exited 0:

- `budget-reap` on rv64 and rv32
- size-budget, formatting, docs
- the R10 mutations: 14 caught

The red team's verdict was OK with notes: three P2 findings, all folded into the same commit.

1. budgets.md: "the call takes no child handle, so the caller cannot choose" replaces the false
   "the caller holds no handle to it".
2. abi.md: the `budget_destroy` and `budget_reap` rows add "or its process object is charged
   there".
3. These prose lines are rewrapped: kernel/README.md:33, budgets.md:108 and its billing
   paragraph, scheduling.md's billing paragraph.

The fixes change pages only. docs exited 0 on the final head, wp-K23 2f840b08b.

## Steward commit re-aimed onto STEWARD2 10bda633a

The head is wp-K23-steward 21ff396ba, one commit on STEWARD2's 10bda633a. The cherry-picked
kernel commit is dropped: budget_reap is on main beneath STEWARD2.

The conflicts were resolved like this:

- STEWARD2's new residual on budgets.md and steward.md ("init does not call budget_reap") is
  removed, since this commit makes init call it.
- docs/todo/empty-a-budget.md is deleted again, with its SUMMARY.md line.
- size ceilings: libs/rt 3,534 and servers/init 2,395 (STEWARD2's plus mine), measured by the
  size gate.
- steward-restart-reboot's program list is synced to STEWARD2's (walfsd:data).

From a fresh prebuilt (rv64 224, rv32 210 cases built), each of these exited 0 on both widths:

- steward-restart, steward-restart-reboot, steward-session-ends, init-restart, init-reboot
- init-boot, bench-net-peer, ipc-outcomes
- userland-boot on rv64

docs, size-budget and formatting also exited 0.

Not mine: rv32 userland-boot fails, and fails again run alone on the quiet cores (q --quiet).
After `NoSuch.call()` it times out at 450 s waiting for STEWARD2's expected
`UndefinedFunctionError ... NoSuch.call/0`. The case's own description says a call to NoSuch
"stays silent". That is beamlet's loading path. The boot, the steward's lines and the prompt
before it are as expected, and the commit touches nothing there.

## Final: the steward commit on STEWARD2's final tip

The head is wp-K23-steward 8865a7a05, one commit on wp-STEWARD2 c3567f923 (main cc51f76ad).

The red's P1 is fixed. init hands the steward a fresh connection at each manifest server, made
through the badge, and disconnects it at the steward's exit, so every session connection under
it goes with it. Stamping could not do this: a server tracks no exits.

The probe is now a timed exit. Every restart-probe instance exits 14 s after its console session
starts. SSH probes are not used, because sshd keeps a slot for each login the dying steward
never answered, and has four slots.

The red's P2 is folded: steward-restart makes 13 restarts, past ipd's cap of 12, and steward.md
says littlefsd:alice-secrets is covered by the shared code only.

littlefsd:alice-secrets's stack is 5 pages (peak 8,536 B). The control run, with init handing
badges, refuses the console session from the 8th restart on.

The rebase took STEWARD2's restructured serve loop: the probe exit sits after `after`. It also
took main's milestone names (SUMMARY.md: Beyond M6). The todo page is deleted again, and no
"recreates users" text remains.

Size ceilings: libs/rt 3,542, servers/steward 1,193, servers/init 2,413.

From a fresh prebuilt, each of these exited 0 on both widths:

- steward-restart, steward-restart-reboot, steward-session-ends, steward-ssh-two-principals
- init-restart, init-reboot, init-boot, budget-reap
- the smoke set: userland-boot (rv32 included), bench-net-peer, ipc-outcomes

size-budget, formatting, unsafe-budget, no-cruft and docs also exited 0.
