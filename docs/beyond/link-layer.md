# The link layer

## Idea

Two servers below `ipd`:
- **`linkd`**, one per network card: 802.1Q tagging, each VLAN presented as its own interface
  capability; a small layer-2 and layer-3 allowlist and a rate limiter before any stack parses a
  byte; egress priority queues keyed by the capability the traffic came from.
- **`routerd`**, holding several interface capabilities and forwarding between them: longest-prefix
  match, TTL, ARP, stateful filtering and NAT for forwarded traffic, with its data plane in Rust.

## Why it is not a goal

Every milestone's network is one card, one `netd` and one `ipd` per network or trust domain
([netd](../servers/netd.md), [ipd](../servers/ipd.md)). Nothing in the hard goals needs the box to
route, to split a card into VLANs, or to shape traffic.

## What it would need

- `linkd` between `netd` and `ipd`, holding no socket and parsing no more than the headers it
  filters.
- `routerd` as a system server whose control plane is Rust when the router is shared between
  principals; an Elixir control plane only for a network one principal owns.
- The same rules as `ipd`: default deny, and the box's own addresses never reachable from inside.

**Attack cases:** a frame tagged for another VLAN never reaches its interface's holder; a flood from
one capability does not delay another's queue; a forwarded packet never reaches the box's own
services.
