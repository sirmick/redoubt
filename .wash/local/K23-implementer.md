# K23: a budget is emptied and kept, so a dead steward's restart is a logout, not a reboot

Tier A (the kernel's budget calls, the model, `init`, the steward's start, the pages), size M.
Needs STEWARD2 (merged: its `steward-restart` case and the steward's start check are what this
package changes). The owner (2026-10-06): "6 seems correct; a crash should restart it." Start
from `main` once STEWARD2 is on it. Run everything natively under the job pool's rules.

## Context rules (read these first)

- Read `docs/todo/empty-a-budget.md` whole; `docs/kernel/budgets.md` "The calls", "Class is
  trust, not order", R10 whole and the residual "a lost budget"; `docs/servers/init.md`
  "Restarts and reboots"; `docs/servers/steward.md` "Failure and restart" and its first
  residual; `docs/kernel/abi.md` for how a call is numbered and recorded; `tests/steward-
  restart.toml` and its description. Code by function: `kernel/src/budget.rs` `destroy_subtree`
  and the child links (`mark_dying`, `subtree_next`), `kernel/src/redoubt.rs`'s call dispatch,
  `model/src/kernel.rs` `destroy_budget`, `servers/init/src/bin/init.rs`'s restart path,
  the steward's start check (`users not empty`).
- Reports under 1,900 bytes, detail in `.wash/local/K23-report.md`.

## The call: `budget_reap(handle) -> remaining`

Destroys **one** child of the budget the handle names, the whole subtree under that child, as
R10 destroys a subtree (mark, kill, free objects, reach messages, lift, sweep, return, move,
free: every step as written, billed to the caller), and returns how many children the budget
still has. The budget itself keeps its limits, class, labels, account and deadline; after the
last child is gone its usage is what it was before any child was carved (I10 per child). The
caller loops until `remaining` is 0.

Why one child per call and not all: a destruction is bounded at 30 ms of kernel time (R10),
and the kernel is not preemptible; emptying `users` of twenty principals in one call would be
twenty destructions in one kernel entry, past R12's bound for a call. One per call keeps every
entry within R10's bound, and the loop runs in `init` between entries.

- **Authority:** holding the budget's handle, exactly as `budget_destroy` (R10's model: a
  handle is the authority; no class check, no caller test). A child handle cannot reap the
  parent: the attack case. `init` holds `users`' handle from boot; the steward holds one too
  (it carves under it), so a live steward could reap its own principals: that is a logout it
  could already do child by child with `budget_destroy`, nothing new.
- **If the caller runs under the child being reaped**, or its process object is charged there,
  the call never returns, as `budget_destroy`'s row says; the caller is last, as in R10 step 2.
  Which child is reaped first: the first on the budget's child list, stated; the caller cannot
  choose (it holds no child handle), and the order is not a promise.
- **Interactions, each stated on the page:** class is untouched (the budget is not recreated:
  that is the point, since `budget_create` takes no class); R4b: every call the reaped
  subtree's servers held is abandoned with `Dead` to live callers outside it; R10 step 4
  fails what was sent through handles stamped with the reaped budgets; exit notices for the
  reaped processes go where R10 step 2 sends them (below). The deadline list: a reaped child
  with a deadline leaves it (step 9).
- **ABI:** the next call number in `abi.md`'s table and `libs/sys`; the record checks as the
  other budget calls'; `every_call_round_trips` and `malformed_calls_are_refused` extended.
- **The model:** `reap_budget` beside `destroy_budget`, the same steps over one child;
  mutations that the model's checks catch, named in `model/src/mutation.rs` and on the page:
  `R10ReapDestroysParent` (the budget is destroyed too), `R10ReapKeepsCarve` (the child's
  carve is not returned), `R10ReapSkipsGrandchildren` (a grandchild survives). Each must be
  caught by `mutations_are_caught`.

## The dead steward's restart: order of operations in `init`

The steward's crash today ends in a reboot because the restarted steward finds `users` not
empty and exits (steward.md "Failure and restart"). After this package, `init`, on the
steward's exit:
1. **Settles the dead steward's own carve.** Establish from `init`'s restart path whether a
   restarted server runs in a fresh budget or its old one. The sessions' process objects are
   charged to the steward (their creator), and their exit notices are owed to the dead
   steward's exit endpoint; nobody will take them. If the budget is destroyed and recarved for
   the restart, R10 step 3 frees those objects with no notice and kills their processes first:
   the sessions end there, cleanly, before step 2 below. If `init` keeps a server's budget
   across restarts, K23 makes the steward's restart destroy and recarve it (the manifest's
   numbers are the same; the receive endpoint is `init`'s and survives), and the report says
   whether the same holds for every server or the steward alone and why.
2. **Reaps `users`** in a loop until `remaining` is 0: every principal's budget, its label-set
   sub-budgets, their sessions' and leases' budgets, endpoints, handles and stamps go, with the
   work billed to `init`. Each sshd channel and console session sees its connection fail
   (`Dead`) and ends as it does when a session's budget is destroyed today; `sshd` closes the
   channel; `consoled` releases the minted connection. Nothing of the old sessions survives.
3. **Starts the steward,** which finds `users` empty and carves afresh; the console principal's
   session (`console "PRINCIPAL"` in the manifest) is started again by the new steward as at
   boot: the owner's "a crash should restart it". The steward's `users not empty` check stays
   exactly as it is: it is now the guard that step 2 ran, and a steward that still finds `users`
   held exits as before (the five-restart reboot remains the backstop, for a kernel bug).

## The case: `steward-restart`'s new verdict

The same construction (a steward that dies after carving); the new expectations: `init`'s
restart line; `init: emptied users: N budgets reaped` (one line, a count); no `users not empty`
line, no `NOT_STARTED`, no reboot line; the steward's start line a second time; the console
session's prompt back on the console (the shell's first prompt after the restart); `sshd`'s
channel, if the case opens one, closed with the session's end; the kernel's usage of `users`
back to zero before the restart (the steward's own line at its start, as `steward-session-ends`
reads it). Attack cases, `budget` or a new `budget-reap`: a child handle cannot reap its parent
(`WrongObject` or the handle's own error, say which); a reap with a call held open by a server
in the reaped subtree fails the outside caller with `Dead` and its lend comes back; a reap with
a message in flight stamped with the reaped budget arrives nowhere; the budget's usage after the
last reap equals its usage before the first carve; a budget with no children returns 0 and
changes nothing. Both widths.

## Pages (with the code)

- `budgets.md` "The calls": a `budget_reap` row; R10: a paragraph after the nine steps ("A
  budget is emptied one child at a time by `budget_reap`, each child destroyed as above and the
  budget kept with its limits, class, labels, account and deadline; the call returns how many
  children remain, so each kernel entry is one destruction"), its status gains the cases and
  the three mutations; the residual "a lost budget" rewritten to what a launcher can now do.
- `init.md` "Restarts and reboots": the steward's restart empties `users` first, the order
  above, and whether the server's carve is recarved; `steward.md` "Failure and restart": the
  first bullet becomes "If it dies, `init` reaps `users`, which logs every session out and ends
  every lease, and restarts the steward; the console session starts again"; its first residual
  goes; `abi.md`'s call table; `SECURITY.md`'s R10 row gains the cases;
  `docs/todo/empty-a-budget.md` deleted, with `SUMMARY.md` and `todo/README.md`. No dates,
  package IDs or review history.

## Owned paths

`kernel/src/{budget,redoubt}.rs` and `libs/sys` (the call), `model/src/{kernel,mutation}.rs`
and tests, `servers/init/src/bin/init.rs` (the restart order) and its host tests,
`servers/steward` only for a start-line change the case needs, `tests/steward-restart.toml`,
`tests/budget-reap.toml` (or `budget.toml`'s additions) and programs, the pages. **Not yours:**
R10's steps themselves (K19's to-pump list and chains are in them: use `destroy_subtree` as it
is), the scheduler, `sshd`, `consoled`, the steward's session logic.

## The short gate

Both builds; host tests of `redoubt-kernel`, `redoubt-model` (whole), `redoubt-sys`,
`redoubt-init`, `redoubt-steward-server`; the docs checker, `cargo fmt --check`, the size
budget (the kernel grows by one call; ceiling at the fold, delta reported), the `unsafe`
ratchet unchanged, the no-cruft gate; own cases on both widths: `steward-restart`,
`steward-session-ends`, `budget-reap`, `budget`, `budget-destroy-kills`,
`budget-destroy-attack`, `init-restart`, `init-reboot`, `process-lifecycle`; and the smoke set.
The whole bench is the train's.

## Not here

Reaping several children per call; a deferred or incremental destruction; recreating `users`
(its class forbids it, and the call exists so that it need not be); the steward's own crash
causes; a kernel notification of the reap to anyone (the connections' `Dead` is the signal).

## Checkpoint

After the kernel call and its model pass the host tests, before `init`'s change: one progress
line with the call's number, the first-child rule, and what `init`'s restart does with a
server's budget today (step 1's finding).

## plan_set body for K23 (parent M1, needs STEWARD2, state todo)

Owner (2026-10-06): "a crash should restart it": a dead steward's restart must be a logout, not
a reboot. Brief: .wash/local/K23-implementer.md (architect-16). Tier A, size M. A kernel call
budget_reap(handle) -> remaining destroys one child subtree of the budget as R10 does and keeps
the budget (limits, class, labels, account, deadline), returning the children left, so each
kernel entry is one destruction within R10's bound; authority is the handle, as
budget_destroy's; a child handle cannot reap its parent; the model gets reap_budget and three
mutations. init, on the steward's exit: settles the dead steward's carve (sessions' process
objects and notices go with it), reaps users in a loop, restarts the steward, which finds users
empty and starts the console session again; its users-not-empty check stays as the guard.
steward-restart's verdict becomes: emptied, restarted, prompt back, no reboot. Pages:
budgets.md (the call, R10's paragraph, the lost-budget residual), init.md, steward.md, abi.md,
SECURITY.md; docs/todo/empty-a-budget.md deleted.
