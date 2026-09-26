# The 9P skeleton's rollback on a discarded reply

## What

When a `new_connection` reply is discarded, or delivered without its capability, the 9P skeleton
rolls back the provisional connection and its admission charge (`libs/rt/src/server/ninep.rs`,
after `finish` in the serve path). That rollback is tested through `unmint` and `keyd`'s grant, not
through the skeleton's own serve path: it runs after `finish` makes a real reply, so a host test of
the request handling cannot reach it, and a helper test would not catch the serve path dropping it.

## Why it matters

A skeleton that stopped rolling back would leave each lost reply holding one `State` unit of its
caller's share for the life of the server, since only the recipient of an id can free it
([replies and rollback](../servers/serving.md#replies-and-rollback)). A client could spend its own
admission without noticing, and a bug here passes every test.

Fixed in the servers follow-up package after the documentation rewrite. Test-only.

## Where

- [`libs/rt/src/server/ninep.rs`](../../libs/rt/src/server/ninep.rs): the serve path's handling of
  `ReplyOutcome` for `new_connection`.
- [`tests/`](../../tests): the new bench case.

## Done when

- A bench case, `ninep-newconn-discard`, has a client make `new_connection` replies undeliverable
  (its handle table full, so `accepted(1)` fails) more times than its `State` admission allows, and
  a normal `new_connection` then still succeeds.
- A mutation that drops the `forget` call fails the case.
