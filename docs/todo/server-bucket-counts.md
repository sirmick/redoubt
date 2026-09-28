# init does not check bucket counts

## What

Every shared server takes `buckets=N` from its startup block, parsed once in the serving library,
and does not start without it. The rule also has `init` refuse a boot where N is below the number
of distinct (account, label set)s the manifest routes to that server plus its system callers, and
`init` does not exist yet, so nothing checks N against the manifest.

## Why it matters

A server sized for fewer buckets than the (account, label set)s it serves refuses the latecomers,
which tells them others hold state: across accounts, and between the label sets of one account,
where it is a channel out of a vault
([serving](../servers/serving.md#residual-risks)). Until `init` checks N, the "never binds in
normal use" of [R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) rests on
whoever writes the manifest.

Fixed with `init` and the boot manifest in
[M1 (separation and containment)](../plan/m1-separation.md#remaining-work).

## Where

- The page: [init](../servers/init.md#the-boot-manifest).
- [`libs/rt/src/server/admit.rs`](../../libs/rt/src/server/admit.rs): `buckets`, the parser
  whose N `init` checks.

## Done when

- `init` refuses a manifest whose N for a server is below the (account, label set)s routed to it
  plus its system callers, and an attack case shows the boot refused.
