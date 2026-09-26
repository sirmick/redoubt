# An account-0 client can spend every bucket

## What

Admission keys account 0 by badge (`AdmitKey::of`), and `Minted::share` folds a capability into
its parent's share only while the requester's key stays the same. A `system`-class client of a 9P
server that mints connections for itself therefore looks, through each new badge, like a new
client: the chain opens a fresh bucket per link, until the server's bucket count is spent and every
later connection is refused. The rule,
[R26 (admission fairness)](../servers/serving.md#r26-admission-fairness), is that within account 0
every capability minted through a root badge counts in that root's share, however many links deep
and whoever holds it; the code departs from it.

## Why it matters

Every driver, file server and daemon is account 0. One of them, compromised or fed hostile input
(`sshd`, `netd`, a driver), can take every `State` bucket of a 9P server and starve every other
client, the steward and principals included (R26). It is availability
across principals, not an escape. `keyd` avoids it by letting only a root badge grant.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`libs/rt/src/server/minted.rs`](../../libs/rt/src/server/minted.rs): `Minted::share`.
- [`libs/rt/src/server/admit.rs`](../../libs/rt/src/server/admit.rs): `AdmitKey::of`, which keys
  account 0 by badge.
- The page: [serving](../servers/serving.md#residual-risks).

## Done when

- For an account-0 caller, `Minted::share` walks parent links to the root badge the chain was
  minted through; a capability used under a non-zero account is keyed by that account, as before.
- A `libs/rt` host test has an account-0 caller chain N links and flood: the chain holds one
  bucket's worth, and a second client is still admitted.
- A mutation restoring the stop-at-key-change fold fails that test.
