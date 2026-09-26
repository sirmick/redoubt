# consoled keeps the handles of a request it refuses

## What

`consoled` serves no typed protocol of its own, and answers any typed opcode outside
`ninep_common` (the `consol` opcodes included) from its own callback with a bare `malformed` reply
(`servers/consoled/src/bin/consoled.rs`, the closure passed to `serve_parking`). That reply bypasses
the serving library's `finish`, which closes the handles a request carried that its protocol did
not ask for, and `Request` closes nothing when dropped. So the handles attached to a refused request
stay open in `consoled` with no owner. The broader cause is that a server can answer a request with
the raw `reply` call while holding the runtime's owning views of it; which combinations of the two
are sound has not been audited.

## Why it matters

A client that repeats such requests fills `consoled`'s handle table and spends its memory outside
the admission the serving library keeps, until the kernel's per-receiver handle limit refuses more.
It is bounded, not unbounded, but it is a way around the rule that a client cannot grow a server's
handle table ([the serving library](../servers/serving.md#authority)).

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`servers/consoled/src/bin/consoled.rs`](../../servers/consoled/src/bin/consoled.rs): the callback
  that replies `malformed`.
- [`libs/rt/src/server/ninep.rs`](../../libs/rt/src/server/ninep.rs): where a request for the
  server's own protocol is handed over; [`libs/rt/src/ipc.rs`](../../libs/rt/src/ipc.rs):
  `Request::reply`.
- The page: [consoled](../servers/consoled.md#residual-risks).

## Done when

- A request `consoled` refuses has its handles closed, by answering through `finish` (or a library
  entry point that does) rather than the raw `reply`.
- A test sends typed requests carrying handles to `consoled`'s real serving path, repeatedly, and
  checks that its handle count does not grow.
- An audit lists every place a server combines the raw calls with the runtime's owning views, and
  each is either made sound or given a follow-up.
