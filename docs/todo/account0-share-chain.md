# An account-0 client can spend every bucket

## What

Admission keys account 0 by badge, and a capability a client mints for itself folds into its
parent's share only when the requester is the same client (the same badge, account and label
set). A `system`-class client of a 9P server that mints connections for itself therefore looks,
through each new badge, like a new client: the chain opens a fresh bucket per link, until the
server's bucket count is spent and every later connection is refused. The code names it an open
hole in `Minted::share`.

## Why it matters

Every driver, file server and daemon is account 0. One of them, compromised or buggy, can starve
a shared server of buckets, so the steward, `sshd` or another server cannot get a connection
([R26 (admission fairness)](../servers/serving.md#r26-admission-fairness)). `keyd` avoids it by
letting only a root badge grant; the 9P skeleton cannot, because minting a connection for a child
is how attenuation works there.

## Where

- [`libs/rt/src/server/minted.rs`](../../libs/rt/src/server/minted.rs): `Minted::share`.
- [`libs/rt/src/server/admit.rs`](../../libs/rt/src/server/admit.rs): `AdmitKey::of`, which keys
  account 0 by badge.
- The page: [serving](../servers/serving.md#residual-risks).

## Done when

A chain of connections an account-0 client mints for itself counts in one bucket (for instance,
fold while the requester's account and label set match, ignoring the badge, for minted badges),
and a host test has an account-0 client mint a chain past the bucket count while another client
still gets a connection.
