# B48 report (b48-implementer)

Worktree /home/mcloonan/redoubt/.worktrees/B48, branch wp-B48 from main 92b84a5b8.

## Design

**The leak.** `Redoubt.Pipes` starts `piped` with `launch(serve: "serve")`; the platform makes the
endpoint itself (`jobs.rs` `served()`), hands the child the receive right and keeps the send right
as a `Cap` over `Owned::new` (closed when its last copy goes, no Life: the endpoint is charged to
the VM's budget, so piped's death leaves the slot to the VM, objects.md "No call destroys an
endpoint"). `Namespace.bind("/dev/pipe", conn)` then attaches it as a 9P connection and pushes a
`Known` into `Sys.known` (`system.rs` `bind`, the `None` arm) holding a clone of that `Owned` and
the `Connection`, and binds the prefix. `Sys.known` is append-only: its entries were the VM's
startup handles, kept for the VM's life, and nothing ever removed one. So after piped's budget is
destroyed the entry pins the kernel handle (one slot of MAX_HANDLES per pipeline) and the
connection, and `/dev/pipe` stays bound to a dead server. The next pipeline's bind replaces the
prefix but not the entry.

**The owner.** A `Known` made by a bind belongs to the budget of the server it reaches: every such
entry carries that budget's `Life` as `returns` (a served send right's from `launch`'s budget; a
handle a reply brought, from the connection it came on). Startup entries carry none and stay.

**The return path.** `budget_destroy`, after the kernel's destroy succeeds and the Life is ended,
drops every `Known` whose `returns` is that Life and unbinds every prefix naming its connection
(`Namespace::unbind(&Connection)`, new in libs/client/ns.rs: by `same`). The `Owned` then has only
the Elixir term's copies; when the last is collected its `Drop` closes the send right, which is
the VM's to close. The hub's record and waiter for the connection end as today: the waiter returns
when its completion call dies with the server and gives its place up at the next `connect`
(`Io::connect`, B47). One rule, stated on the pages: a destroyed budget takes every binding of a
connection to a server it ran, so nothing of piped exists at the prompt (Pipes' moduledoc, now
true for the binding too). Not reuse-one-piped: the design is one piped per pipeline run, destroyed
when the last holder lets go (piped.md "Started by a session"); unchanged.

**Failure modes.**
- A pipeline killed mid-way: `Pipeline.own`'s `after` destroys the stages and `Pipes.release`s;
  the last holder's release destroys piped's budget, the same path. Another pipeline still holding
  piped keeps it; the entry goes when the last lets go.
- piped exits or faults on its own: `Pipes.handle_info({:exit, ...})` destroys the budget; same path.
- The session ends with pipelines open: the VM's process ends and the kernel frees its whole
  handle table (objects.md "A handle held inside dies with its holder's table"); no VM state matters.
- `Budget.destroy` fails: the Life is not ended and the entry stays, one leak; destroy failing is
  the bug then, not this path. Nothing is dropped before the kernel has agreed.
- Open files or a pending clunk on the connection hold their own `Connection` clones: they fail
  `disconnected` as today; unbinding affects only lookups to come.
- A bound handle that reached the VM through no budget it carved (`returns` None, a request's
  handle) stays known while the VM runs, as startup handles do: a residual, stated.

**Tests.** Host, `beamlet-redoubt::tests/system.rs`: a thousand times, carve, launch with serve,
play the server with a littlefsd on the served receive right (new fixture `volume_served`), bind
`/dev/pipe`, use it, destroy, stop; the namespace table and the fake kernel's handle count
(`f.held`) are where they started. Machine: `pipe-sixty-four`, pipe-eight's sibling, 64 pipelines
in one session on both widths; the session prints the table's size before and after and whether
`/dev/pipe` resolves at the prompt.
