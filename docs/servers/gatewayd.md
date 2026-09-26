# gatewayd

`gatewayd` is the gateway between agents and the services outside the box that need credentials:
first one model provider's API, and git hosting. It holds the API keys and speaks TLS to the
service itself; an agent holds a capability to `gatewayd`, never a key. Each capability names what
its holder may ask for, and `gatewayd` checks every request against it, meters tokens and money
per principal, and logs every call. It is a label sink, cleared for nothing.

## Purpose

An agent needs a model and a git remote, and both want a secret: an API key, a token. A secret in
an agent's memory is a secret the agent can send anywhere. `gatewayd` keeps credentials out of
every session: it adds the key to a request it has checked, sends it over its own TLS connection to
the one service the capability names, and returns the answer. What an agent can do with the
service is what its capability allows, for as long as its lease lasts, and every call is on the
record.

## Interface

### Gateway capabilities

Status: planned · M4 (self-hosted development)

- A **gateway capability** is a connection to `gatewayd` whose badge names a grant by name (the
  `model` gateway, say), optionally narrowed to some of its hosts, with a **meter**: tokens and
  money the holder may spend, carved from its principal's. A `git` grant names its remotes, grants
  fetch and push separately, limits push to named ref patterns, and refuses force-push unless
  granted; the client, a Rust `git`, speaks git's smart HTTP to `gatewayd`
  (the userland pages on agents and development use these grants).
- A capability is granted by the steward, for a session or a lease, and a grant only narrows: a
  lease's capability names no more service, operations or budget than its sponsor's
  ([steward](steward.md#leases)).
- **Requests are typed**, one message per operation (a model completion, a git fetch or push), never
  raw HTTP. `gatewayd` builds the outgoing request itself from the checked fields, so a holder cannot
  reach another path, host or header of the service.
- **Every request is checked** against the capability before anything leaves the box: the service,
  the operation, the model or repository, the size, and what the meter has left; a request over the
  meter is refused ([R65 (a request only within its capability)](#r65-a-request-only-within-its-capability)).
- **Metering.** The steward owns the meters, so they survive a `gatewayd` restart, and reboots
  once the steward keeps state in M5 (persist, install, share). Before sending,
  `gatewayd` checks the principal's spend cap against the steward's figure, and refuses a request
  over it; after each response it reports the usage the provider gave.
- **Logging.** Every call is recorded with the principal chain, the capability, the operation and
  its cost, in the steward's audit log ([steward](steward.md#the-audit-log)).

```mermaid
sequenceDiagram
    participant A as agent
    participant G as gatewayd
    participant I as ipd
    participant P as model provider
    participant S as steward (audit)
    Note over A,S: planned
    A-->>G: complete(model, prompt) on its gateway capability
    G-->>S: spend cap and the principal's figure
    G-->>G: check service, model, size;<br/>under the cap
    G-->>I: TLS connection to the provider<br/>(gatewayd's own scope)
    G-->>P: HTTPS request with the API key
    P-->>G: completion and usage
    G-->>S: usage for the meter; audit record:<br/>principal chain, operation, cost
    G-->>A: the completion (no key, no headers)
```
*Figure: an agent's model call through `gatewayd`. All of it is planned.*

The attack test: a request over the spend cap is refused before anything is sent.

**Open:** the protocol table and its operations for the first provider and for git; how money is
priced from usage; the finer checks on `git` requests beyond remote, operation, refs and force
(size limits, path rules).

### Keys and TLS

Status: planned · M4 (self-hosted development)

- **API keys** are bearer secrets, not signing keys, so `keyd` cannot hold them: `gatewayd` holds
  them itself, from its arguments, and no operation returns one
  ([R66 (no credential leaves gatewayd)](#r66-no-credential-leaves-gatewayd)).
- **TLS** is `gatewayd`'s own, to each service's fixed host name, checking the service's
  certificate against roots `gatewayd` is configured with. `gatewayd` connects by name, as a person
  does ([ipd](ipd.md#name-scoped-connections)), on its own `ipd` connection whose rule names only the
  services it serves.
- **Where the keys come from.** In M4 (self-hosted development) the API keys arrive as
  `gatewayd`'s arguments in the boot manifest, which is never public, at the bundle's trust, as
  `keyd`'s seeds do.
- A git host's SSH or token credentials are held the same way.

**Open:** the TLS implementation is pure Rust with no C (rustls with a pure-Rust cryptography
provider, not one wrapping C); which provider is still to be chosen. Where API keys live across a
reboot once the steward keeps state.

### A label sink

Status: planned · M4 (self-hosted development)

`gatewayd` refuses every labelled caller, like `ipd`: whatever it forwards leaves the box. A model
that runs on the box can be a server cleared for a label, one instance per label set; `gatewayd` is
for services outside.

**Open:** none.

## Authority

Status: planned · M4 (self-hosted development)

- `gatewayd` holds its endpoint, the API keys and credentials of the services it serves, one `ipd`
  connection whose name rule names those services, and a way to append to the audit log.
- A client's authority is its capability: one grant, named operations, a meter.
- It never holds a principal's own keys; `keyd` does.

**Open:** none.

## Security properties

### R65 (a request only within its capability)

Status: planned · M4 (self-hosted development)

Every request `gatewayd` sends is built by `gatewayd` from fields it checked against the caller's
capability: the capability's service, one of its operations, within its meter. So a hijacked agent
can spend at most its meter on its named operations, and reach no other service, path or account
with the key.

**Open:** none.

### R66 (no credential leaves gatewayd)

Status: planned · M4 (self-hosted development)

No operation returns an API key or credential, and none is ever sent to anything but the service it
belongs to, over TLS to that service's name. So no session, agent or lease holds a credential it
could send elsewhere.

**Open:** none.

## Failure and restart

Status: planned · M4 (self-hosted development)

- **`gatewayd` restarts:** requests in flight fail and are asked again; the meters are the
  steward's and survive.
- **The service fails or is unreachable:** the request fails with the service's error, and nothing is
  charged but what the service reported used.

**Open:** none.

## Residual risks

- **The service sees what the agent sends.** A prompt carries whatever the agent put in it; the
  gateway limits where it goes and how much, not what it says. Labelled data never reaches it, since
  it is a sink.
- **The keys sit beside the response parser.** `gatewayd` holds every API key and parses provider
  responses (TLS, HTTP, JSON) in one process, so a response-parser bug reaches the keys. Splitting
  TLS out would not hide them, since the key travels inside the TLS plaintext. A leaked key is
  revoked at the provider.
- **Metering trusts the provider's usage figures.**
- **An answer is untrusted input.** What the model returns can try to steer the agent; the agent's
  capabilities, not the gateway, bound what that can do.

## Why

- **No credentials in agent memory.** A key in an agent is a key the agent can leak; a capability to
  a gateway is revoked with the lease.
- **Typed operations, not a proxy.** A general HTTPS proxy with a key attached lets its holder call any
  endpoint the key allows; typed operations built by the gateway allow only the named ones.
- **A sink.** A gateway is a way off the box, so it takes no labelled caller.
