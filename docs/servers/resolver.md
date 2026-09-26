# The resolver

The resolver is the mediated DNS server. A session asks it for a name; it answers only names its
caller's capability allows, from an allowlist and a blocklist, and records each answer against the
caller's connection so that [`ipd`](ipd.md) lets that connection reach exactly the addresses it was
told, for as long as the answer lives. So a principal's network capability can name domains, not
only prefixes and ports, and an agent cannot use DNS itself as a way out.

## Purpose

`ipd` scopes sockets by IP prefix and port, because it sees only the address a client chose: DNS
runs in the client, so a name check in `ipd` alone would check nothing. People need to reach
services by name, and addresses behind a name change. The resolver closes the gap: it is the one
place names become addresses, it applies the name rules, and it tells `ipd` what it answered. It
also stops DNS being a channel: a client that can send arbitrary queries to an arbitrary resolver
can encode data in the names it asks for.

## Interface

### Resolving a name

Status: planned · M4 (self-hosted development)

- A client holds a connection to the resolver whose badge names its **name rules**: an allowlist of
  domain names and suffixes (`example.org`, `*.example.org`), and a blocklist that wins over it. The
  rules come from whoever granted the connection, and a grant only narrows them, like an `ipd`
  scope.
- **`resolve(name)`** answers the IPv4 addresses for `name` if the rules allow it, with their
  lifetime; a name the rules do not allow is refused before any query leaves the box
  ([R63 (only allowed names)](#r63-only-allowed-names)).
- **Queries go upstream from the resolver alone,** through its own `ipd` connection, to the
  upstream resolvers its arguments name. Sessions hold no route to port 53 anywhere: their `ipd`
  scopes do not include one.
- **Names are checked strictly**: lower-cased, at most 253 bytes, labels of letters, digits and
  hyphens, no trailing tricks; a name that is not a plain host name is refused.
- **The resolver is a sink.** Like `ipd` it refuses every labelled caller, since a query leaves the
  box.

**Open:** the transport to the upstream resolvers (DNS over TCP through `ipd`, or over UDP once
`ipd` serves it, or DNS over TLS through [`gatewayd`](gatewayd.md)); whether a refused name is
answered differently from a name that does not exist; the protocol table and which page includes
it.

### Connections by name, pinned

Status: planned · M4 (self-hosted development)

Each answer is recorded against the caller's `ipd` connection: for the answer's lifetime, that
connection may connect to exactly the answered addresses on the ports its name rule allows, and to
nothing else by name. A client cannot connect to an address it chose itself and claim a name for
it, and an answer given to one connection widens no other
([R64 (connections by name are pinned)](#r64-connections-by-name-are-pinned)). An agent's scope
stays prefixes and ports; name rules are for people's sessions and for leases whose approval named
them.

**Open:** how `ipd` learns an answer (the resolver granting a narrowed rule per answer at `ipd`, or
`ipd` consulting a table the resolver keeps); what happens to an open connection when its answer
expires (the recommendation: it stays, and no new connection uses the address).

### Caching

Status: planned · M4 (self-hosted development)

Answers are cached per (account, label set), never shared across principals: a shared cache tells
one principal, by how fast it answers, which names another asked for. A cached answer keeps the
lifetime the upstream gave it, capped by the resolver's own maximum.

**Open:** the cap on an answer's lifetime and on the cache's size per (account, label set).

## Authority

Status: planned · M4 (self-hosted development)

- The resolver holds its endpoint, one `ipd` connection scoped to its upstream resolvers' addresses
  and port, and whatever it needs to tell `ipd` about answers.
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

A connection that reaches an address by name reaches only addresses the resolver answered that same
connection, within the answer's lifetime and the name rule's ports.

**Open:** none.

## Failure and restart

Status: planned · M4 (self-hosted development)

- **The resolver restarts:** its cache and its recorded answers are gone; connections by name need a
  fresh `resolve`. Connections already open are `ipd`'s and stay.
- **An upstream resolver fails or lies:** the answer is an error, or wrong addresses within the name
  rule's ports; a lie cannot widen what a connection may reach beyond the name rules.

**Open:** none.

## Residual risks

- **Upstream answers are believed.** Without DNSSEC, an upstream or on-path attacker can answer an
  allowed name with any address, which the connection may then reach on the allowed ports.
- **The allowed names are a channel.** A client may choose which allowed names it asks for and when;
  the resolver bounds the alphabet to its rules, not the timing.
- **A name's addresses are shared.** An allowed name hosted beside other services on one address
  reaches all of them on the allowed ports.

## Why

- **Names at the resolver, addresses at `ipd`.** `ipd` cannot check a name it never sees; the resolver
  cannot stop a connection it does not carry. Pinning ties the two together.
- **No route to port 53 for sessions.** A client that can query any resolver can encode anything in
  the names it asks for; one resolver that answers only allowed names closes that.
- **Per-principal caches.** A shared cache is a timing channel between principals.
