# The resolver

The resolver is the mediated DNS server. It answers only names its caller's rule allows, from an
allowlist and a blocklist, and never sends a disallowed name upstream. [`ipd`](ipd.md) asks it when
a person's session connects by name, and filters and pins what it answers. So a principal's network
capability can name domains, not only prefixes and ports, and DNS itself is not a way out.

## Purpose

People need to reach services by name, and addresses behind a name change. A name a client
resolved for itself could stand for any address, so a person connects by name at `ipd`, which
asks the resolver. The resolver is the one place names become addresses and it applies the name
rules; `ipd` filters what it answers and pins the connection. It also stops DNS being a channel: a
client that can send arbitrary queries to an arbitrary resolver can encode data in the names it
asks for.

## Interface

### Resolving a name

Status: planned · M4 (self-hosted development)

- **Name rules.** A caller's badge names an allowlist of domain names and suffixes and a blocklist
  that subtracts from it and always wins. A suffix matches only at a label boundary: `example.com`
  covers `a.example.com` and never `evilexample.com`. The rules come from whoever granted the
  connection, and a grant only narrows them.
- **Callers.** `ipd` asks through its own resolver connection, with the rule of the connection
  that is connecting ([ipd](ipd.md#name-scoped-connections)). A session's own lookups (for
  `getaddrinfo`) come on a resolver badge carrying the same rule, minted from the same grant; their
  answers are only informational, since the session connects by name.
- **`resolve(name)`** answers the IPv4 addresses for `name` if the rules allow it, with their
  lifetime. A disallowed name is refused locally with its own error, `refused`, and never sent
  upstream, so the names queried cannot carry data out; a nonexistent allowed name is the
  upstream's NXDOMAIN ([R63 (only allowed names)](#r63-only-allowed-names)).
- **Queries go upstream from the resolver alone,** as DNS over TCP through its own `ipd`
  connection, to the upstream resolvers its arguments name. Sessions have no route to port 53, and
  no route to anything by address.
- **Names are checked strictly**: lower-cased, at most 253 bytes, labels of letters, digits and
  hyphens, no trailing tricks; a name that is not a plain host name is refused.
- **The resolver is a sink.** Like `ipd` it refuses every labelled caller, since a query leaves the
  box.

The attack test: a disallowed name never reaches upstream (the test upstream sees no query).

**Open:** the protocol table and which page includes it.

### Connections by name, pinned

Status: planned · M4 (self-hosted development)

`ipd` resolves at connect time: it checks the name against the connecting connection's rule, asks
the resolver, drops every always-forbidden address from the answer, connects to one of the rest,
and pins the connection to that address for its whole life
([ipd](ipd.md#name-scoped-connections)). A client never names an address, and an answer widens
nothing beyond the one connection it was asked for
([R64 (connections by name are pinned)](#r64-connections-by-name-are-pinned)). Name rules are for
people's sessions only; an agent holds no socket and no name rule, and reaches outside the box only
through `gatewayd` ([gatewayd](gatewayd.md)).

**Open:** none.

### Caching

Status: planned · M4 (self-hosted development)

Answers are cached per (account, label set), in practice per account since the resolver is a
sink, and never shared: a shared cache tells one principal, by how fast it answers, which names
another asked for. A cached answer keeps the lifetime the upstream gave it, capped by the
resolver's own maximum.

**Open:** the cap on an answer's lifetime and on the cache's size per (account, label set).

## Authority

Status: planned · M4 (self-hosted development)

- The resolver holds its endpoint, one `ipd` connection scoped to its upstream resolvers' addresses
  and port.
- A client's authority is its name rules: which names it may have answered.
- It holds no keys and no device.

**Open:** none.

## Security properties

### R63 (only allowed names)

Status: planned · M4 (self-hosted development)

The resolver sends upstream, and answers, only names its caller's rules allow and its blocklist does
not; a refused name causes no query. So a client cannot use DNS queries to carry data to a resolver
it chose, or to learn names outside its rules.

**Open:** none.

### R64 (connections by name are pinned)

Status: planned · M4 (self-hosted development)

A connection made by name reaches exactly one address, one the resolver answered for that name
at connect time and that is not always-forbidden, on a port its rule allows, for its whole life.

**Open:** none.

## Failure and restart

Status: planned · M4 (self-hosted development)

- **The resolver restarts:** its cache is gone; connections by name made meanwhile fail and are
  asked again. Connections already open are `ipd`'s, pinned, and stay.
- **An upstream resolver fails or lies:** the answer is an error, or another address, which `ipd`
  still filters; a lie cannot reach a forbidden address or a port the rule does not allow.

**Open:** none.

## Residual risks

- **Upstream answers are believed.** DNS over TCP is unauthenticated: an upstream or on-path
  attacker can answer an allowed name with another address, and the person's connection goes there
  on the allowed ports. Forbidden-address filtering still holds. DNS over TLS waits for a TLS
  implementation.
- **The allowed names are a channel.** A client may choose which allowed names it asks for and when;
  the resolver bounds the alphabet to its rules, not the timing.
- **A name's addresses are shared.** An allowed name hosted beside other services on one address
  reaches all of them on the allowed ports.

## Why

- **Names resolved for `ipd`, not for the client.** `ipd` checks the name the session asked for and
  the resolver answers it; the resolver alone could not stop a connection it does not carry, and
  `ipd` alone would not know what a name means. Pinning ties the two together.
- **No route to port 53 for sessions.** A client that can query any resolver can encode anything in
  the names it asks for; one resolver that answers only allowed names closes that.
- **Per-principal caches.** A shared cache is a timing channel between principals.
