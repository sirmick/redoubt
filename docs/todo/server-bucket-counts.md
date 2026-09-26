# Compiled-in bucket counts

## What

`bootfsd`, `consoled` and `keyd` compile in their admission bucket counts (`LIMITS.buckets`: 16,
4 and 16), and `ipd` parses its own `buckets=N`. The rule is that every shared server takes
`buckets=N` from its manifest arguments, parsed once in the serving library, with no compiled-in
count, and that `init` refuses a boot where N is below the number of distinct (account, label set)s
the manifest routes to that server plus its system callers.

## Why it matters

A server sized for fewer buckets than the (account, label set)s it serves refuses the latecomers,
which tells them others hold state: across accounts, and between the label sets of one account,
where it is a channel out of a vault
([serving](../servers/serving.md#residual-risks)). A count fixed in code cannot follow the
manifest, so the "never binds in normal use" of
[R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) is hoped for rather than
checked.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`servers/bootfsd/src/server.rs`](../../servers/bootfsd/src/server.rs),
  [`servers/consoled/src/server.rs`](../../servers/consoled/src/server.rs),
  [`servers/keyd/src/server.rs`](../../servers/keyd/src/server.rs): `LIMITS`.
- [`servers/ipd/src/args.rs`](../../servers/ipd/src/args.rs): `ipd`'s own `buckets=` parsing, which
  moves into the library.
- [`libs/rt/src/server/admit.rs`](../../libs/rt/src/server/admit.rs): where the one parser goes.
- The page: [init](../servers/init.md#the-boot-manifest).

## Done when

- The serving library parses `buckets=N` once, for every shared server; no server has a
  compiled-in bucket count, and each still checks at start that every bucket at its cap fits its
  budget.
- `init` refuses a manifest whose N for a server is below the (account, label set)s routed to it
  plus its system callers, and an attack case shows the boot refused.
