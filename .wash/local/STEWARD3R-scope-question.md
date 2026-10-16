# STEWARD3R step 1: the question for the Architect (steward3r-implementer, 2026-10-15)

Thread `STEWARD3R-scope`. Base: wp-STEWARD3R at main 32a448875.

## What is there today

- `Connection::new_connection(lend, root, quota)` (libs/rt/src/client.rs:268) sends
  `ninep_common` op 2 `new_connection { root, quota }` (libs/wire/tables/ninep_common.md:7), no
  handle in the request. The server (libs/rt/src/server/ninep.rs:821 → `mint_at`:895) mints the
  reply handle through `Minter::mint(badge)` (server/minted.rs:50) = `mint_from_message(id,
  badge, None)`: stamped like the handle the request came through (R9).
- The kernel's `mint(source, badge, budget?)` already takes a budget and only narrows: the budget
  must be at or below the default stamp or `NotPermitted` (objects.md "mint"; kernel/src/message.rs
  `is_at_or_below(frame, default.frame)`). `Message::mint(badge, Some(&Budget))` exists in
  libs/rt/src/ipc.rs:323. **No kernel ABI change is needed for a scope-stamped mint.**
- Stamps on the path: `init` runs in `root` and its handles are stamped `root` (budgets.md "Root,
  system and users"); the connections it mints for the steward at each server are therefore
  stamped `root`; a lease's scope sits under `users` < `root`, so a server minting with the
  steward's scope passes the narrowing check. The steward already does exactly this for its own
  endpoint: `Step::Connect { Server::Steward }` → `kernel.mint(badge, scope)` (servers/steward/
  src/drive.rs:169-174; the real `mint` at servers/steward/src/bin/steward.rs:416). For
  `Server::Shared(slot)` the scope is **ignored**: `kernel.connect(domain, slot)` →
  `new_connection(.., root, quota)` with no scope (steward.rs:284,297).
- A connection minted in a destroyed scope dies in every table (R9/R10), but the **server never
  learns**: notices are abandoned-call and exit only (ipc.md "receive"); the minted record, its
  fids, its admission `State` and the file server's quota (`minted` hook) stay until `disconnect`
  (`connection_gone`, ninep.rs:934). For a lease, the holder's processes die with the budget
  anyway, so the scope's gain is for copies outside the lease (a handle forwarded on) and for a
  scope narrower than a budget (revoke one grant of a living session without the server's help).

## Shape (a): a scope carried at `new_connection`

- Wire table `ninep_common` op 2 gains `scope: handle[0] budget`, optional (absent = today's
  default stamp). It is Redoubt's own sideband table, not 9P2000; generated code moves with it.
  Whether that counts as "the 9P wire" (owner) is the Architect's reading.
- Serving library: `Minter::mint(badge, scope: Option<Handle>)`; `mint_at` passes the request's
  handle; the kernel refuses a scope not below the default stamp → `Rerror` (NOT_PERMITTED or a
  new text); the request handle is closed by `finish` as today. Every 9P server gets it for free.
  Fake kernel: the host `Minter` must record the stamp so a test can destroy the scope and see the
  connection's handle die while a sibling's does not.
- Client: `new_connection(lend, root, quota, scope: Option<&Budget>)` (consumer sweep: init,
  steward, sshd?, tests) or a new `new_connection_in` beside it (no sweep).
- Steward: `Step::Connect { Server::Shared }` passes the scope through `Kernel::connect`.
- Still needed: the launcher's `disconnect` at the lease's end for the server's records (agents.md
  "When a lease ends" stays true as cleanup, not as the cutoff).

## Shape (b): disconnect-by-scope the steward drives

- No wire or serving-library change. The steward records (server, id) per scope token and at the
  scope's end disconnects each (`Kernel::release` already "closes here and disconnects at its
  server"), then destroys the scope budget, which then revokes only the steward's own badges.
- Costs: the cutoff is only as fast as the steward reaches every server (admission; a flooded
  server, though `disconnect` is a `State` release, not admitted work?), and a forwarded copy
  lives until then. The scope budget binds nothing at shared servers; R41's text ("holds a
  revocation scope") is true only of the steward's own endpoint. The model's P12 holds trivially.

## My recommendation

(a) with (b)'s cleanup kept: the holder's access ends by the kernel at the scope's destruction,
the steward's reach to a server bounds only garbage. If the wire-table field is the owner's, (b)
is the fallback that needs nobody's decision.

## Page lines proposed (for the ruling to confirm or replace)

- steward.md R41 Open → "A connection narrowed to a scope is minted in it: `new_connection(root,
  quota, scope)` stamps the handle with the scope (R9), so destroying the scope ends it in every
  table; the server's record is freed by the launcher's `disconnect`." Kernel page touched:
  objects.md "mint" (unchanged rule, new user); protocol page: serving.md "`ninep_common`".
- agents.md "When a lease ends": "...the kernel ends the agent's connections with its scope; the
  launcher disconnects them at each server to free what they held".

## 2026-10-15, after the ruling: fact (2) was wrong (steward3r-implementer)

I wrote that the steward's connections to each server are stamped `root`. They are not.
`init` runs in `root` and the endpoints it creates and copies into the steward by `process_start`
keep the `root` stamp (init.rs:356, R9 "a copy keeps its stamp"): so the steward's **own receive
right** is stamped `root`, and `Server::Steward`'s `kernel.mint(badge, scope)` with a scope
under `users` passes `is_at_or_below` — that is why STEWARD2's sessions work. But the badges
`init` hands the steward at **other** servers are minted `Endpoint::mint(badge, Some(&budget))`
with `budget` the steward instance's own budget, a child of `system` (init.rs:436, :471-473; the
comment: "stamped with the instance's budget, so a copy it passed on dies with it"), and the
steward's fresh connection at that server is minted from that message, so its default stamp is
the steward's budget. `below(scope under users, steward's budget under system)` is false
(kernel/src/budget.rs:977): the kernel answers `NotPermitted`, the skeleton `refused`. Shape (a)
as ruled is refused at every shared server, for sessions and leases alike.

Ways through, none mine to choose:
- (a1) `init` hands the steward's entry its badges stamped `users` instead of the steward's
  budget (`users` is below `root`, init's default): then every connection the steward makes is
  stamped `users` and any scope under `users` narrows it. Cost: init.md "the badges `init` hands
  the instance are stamped with its budget" gains the steward's exception; the steward's handed
  badges and its connections no longer die with the steward's budget — `init` already
  disconnects its fresh connections at the steward's exit (`release_fresh`) and empties `users`
  (init.md "init empties users"), so nothing outlives the restart, but the page must say so.
- (a2) the kernel's rule relaxed (a stamp that is not below the default): the ABI's meaning
  changes; the owner's.
- (b) the disconnect-by-scope, as the fallback that needs nobody's decision.
