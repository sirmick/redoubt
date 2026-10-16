# STEWARD3R ruling: a connection is minted in its scope; the cutoff is the kernel's, the cleanup the steward's

Thread `STEWARD3R-scope`. Question: `.wash/local/STEWARD3R-scope-question.md`.

## Ruling: shape (a), with (b)'s disconnect kept as cleanup

1. **The wire.** `ninep_common` op 2 becomes `new_connection(root, quota, scope)`, `scope` an
   optional `handle[0]` budget. This is Redoubt's own typed sideband (wire.md:3, :336-347:
   opcodes 1-15 are `ninep_common`, not 9P2000; the 9P half stays plain 9P2000, intro(5), no
   extension), and the kernel's ABI does not move (`mint` already takes a budget and only
   narrows: objects.md "mint", message.rs `is_at_or_below`). So this is the Architect's to rule,
   not the owner's; no decision_request.
2. **The server.** The serving library's `Minter::mint(badge, scope)` in `mint_at` stamps the
   connection's handle with the scope (R9); a scope the kernel refuses (not at or below the
   server's own stamp) is the kernel's error by its name in the one table, no new name. Every 9P
   server gets it for free; a server never sees a scope as anything but the handle it mints with.
3. **The cutoff is the kernel's.** Destroying the scope ends the connection's handle in every
   table at once, forwarded copies included (R10): that is R41's claim, now true. **The cleanup
   is the steward's:** the server learns nothing of the destruction (notices are abandoned-call
   and exit only), so its fids, admission `State` and quota stay until `disconnect`. The steward
   records `(server, id)` per scope and, **after** destroying the scope, disconnects each at its
   server to free what it held. Order: destroy, then disconnect; the disconnect is never the
   cutoff and the page says so.
4. **Who passes a scope.** Every `Connect` the steward issues passes the scope of the session or
   lease it belongs to, through `Kernel::connect` for `Server::Shared` as for `Server::Steward`:
   one path, sessions and leases alike (the model's P12 already reads `Connect` as taking a
   scope). The R41 departure accepted for sessions in STEWARD2 ends with this package.
5. **The client.** One function, `new_connection(root, quota, scope: Option<Handle>)`, and a
   sweep of its callers (init, the steward, sshd, tests): not a second function beside the first.
   A caller with no scope sends none, and the server mints as today.
6. **The fake kernel.** Its `Minter` records stamps, and a destroyed budget must end the handles
   stamped by it in the fake's tables as the kernel does; if the fake lacks that, it gains it in
   this package, with its own test.

## Cases

- host (libs/rt): a connection minted in scope S dies when S is destroyed (its next request
  answers `Dead`, its fids freed at the steward's disconnect), a sibling connection in scope T on
  the same server is untouched, and a `new_connection` with a scope not below the server's stamp
  is refused by name.
- host (steward): the batch at a lease's end is DestroyBudget(scope) then Disconnect per recorded
  connection, in that order; the record is empty after.
- the fuzz target `redoubt-rt/ninep_server` takes the new field (optional, absent, bad handle).
- machine: STEWARD3A's `steward-lease-ends` adds the clause that a request on the agent's
  connection after the deadline answers `Dead` before any disconnect reaches the server (the
  case's own probe), both widths.

## Page lines

steward.md R41, the section's body after "expects it refused.":

> A server narrowing a connection mints it in the scope it is handed: `new_connection(root,
> quota, scope)` carries the scope, the server's `mint` stamps the handle with it
> ([R9](../kernel/objects.md#r9-stamps)), the kernel refuses a scope not below the server's own
> stamp, and destroying the scope ends the handle in every table at once, forwarded copies
> included ([R10](../kernel/budgets.md#r10-destruction)). What the server holds for the
> connection (fids, admission, quota) it cannot see die: the steward disconnects each connection
> of a scope after destroying it, cleanup that follows the cutoff and never is it.

R41's `**Open:**` line goes to `**Open:** none.` when step 2 lands; status to built with the
cases. SECURITY.md's R41 row then.

serving.md "`ninep_common`" (and libs/wire/tables' ninep_common table): the op's third field,
"`scope`, an optional budget handle the connection is minted in; absent, the server's own stamp".

objects.md "mint": one sentence naming the user: "a 9P server mints a connection in the scope its
client hands it ([R41](../servers/steward.md#r41-narrowing-by-revocation-scope))".

agents.md "When a lease ends": replace "the launcher disconnects the agent's connections and
releases what typed servers granted it" with "the scope's destruction ends the agent's
connections at once, in every table; the steward then disconnects them at each server to free
what they held, and releases what typed servers granted".

native.md's `new_connection` sentence gains the scope argument.

## Not ruled here

Whether a server should learn of a scope's death (a notice kind) is not needed: the steward's
disconnect frees the records, and a notice would be a kernel ABI change for a cleanup. If the
records' lifetime ever matters (admission held by a dead connection until the disconnect
arrives), it is a residual on serving.md, stated by step 2 with the measured gap.

## Amendment (after the implementer's correction of fact 2)

The steward's badges at other servers are minted by `init` with the steward instance's own
budget as stamp (init.rs:436, :471-473), a child of `system`; a scope under `users` is not below
it (budget.rs:977), so shape (a) as ruled is refused at every shared server. The ruling stands
with one change, **(a1)**:

- `init` stamps the badges it hands the **steward's entry** with `users`, not the instance's
  budget; every other server's entry keeps the rule (stamped with its budget). Hard-wired to the
  steward slot, not a manifest knob. Then every connection the steward makes is stamped `users`,
  the budget every session's and lease's scope lies under, and the kernel's narrowing passes.
- What the instance-budget stamp bought ("a copy it passed on dies with it") the steward gets
  another way, and init.md says so beside the rule: at the steward's exit `init` disconnects its
  fresh connections (`release_fresh`) and reaps `users` to zero (K23), so no handle the dead
  steward made or passed on outlives its restart; the only holders of its copies are sessions and
  leases, all under `users`.
- (a2), relaxing the kernel's narrowing, is refused: it would change R9's meaning for a cleanup.
- (b) stays as the cleanup, as ruled.

Added cases: host (init): the steward's handed badges are stamped `users` and another entry's
are stamped its budget; `steward-restart` gains the clause that a request on a connection the
dead steward held at `walfsd` answers `Dead` (its disconnect by `release_fresh`), and the
restarted steward's fresh connections serve. Page lines: init.md:615-616 gains the steward's
exception with the reason above; objects.md R9 unchanged.

## Cases, settled with the implementer (second round)

1. No system clause for the cutoff. A copy of a dead steward's connection has no holder outside
   `users`, which `init` reaps right after `release_fresh`; a probe between the two steps would be
   the attacker's clock, and a disconnected connection answers `Rerror NO_CONNECTION`, not `Dead`.
   The property (a connection minted in scope S dies with S in every table; a sibling in T
   serves; the server's record is freed by the disconnect, once) is proved by the rt host test
   `a_connection_minted_in_a_scope_ends_when_the_scope_is_destroyed` over the fake kernel (R9/R10
   stamps and the narrowing check modelled, with the fake's own test), and `steward-restart`'s
   existing expectations (the thirteenth restart's session starts below every server's cap) prove
   the disconnect frees the records. The same reasoning withdraws the clause I asked STEWARD3A to
   add to `steward-lease-ends` (an agent dead with its deadline holds nothing to probe with).
2. Residual under R41: `ipd`'s typed `grant` (`How::Grant`) carries no scope on its wire, so a
   session's `ipd` connection ends by the steward's disconnect alone; every 9P connection (fsd,
   consoled, carved homes) is minted in the scope. Follow-up node STEWARD3S (a scope field on
   ipd's grant), needs STEWARD3R.
3. Implementation note accepted: the Machine keys what it holds by a counter, closes its copies
   once the launch has copied them, and keeps only sshd's console (told `ended`) and a relay's
   control connection.
