# The web stack

## Idea

Redoubt serves HTTP. TCP, TLS and HTTP each end in an isolated, trusted server, split by
privilege: `ipd` holds the sockets, `tlsd` the keys and the TLS state, `httpd` the parsing.
Applications get parsed requests, never bytes off the wire, and inbound access is only through
SSH forwarding ([network policy](../TENETS.md#network-policy)).

## Why it is not a goal

Inbound services other than SSH are a non-goal. Every milestone's network work is outbound: people
connect by name, and agents reach a model and `git` through `gatewayd`, which does its own TLS.
A web stack adds two parsers of hostile input and a key-holding server for no milestone's need.

## What it would need

- **`tlsd`**: TLS with the keys in `keyd`, used and never released; one TLS state per connection;
  nothing of one connection visible to another. Whether `gatewayd`'s TLS moves into it is open
  ([the servers](../servers/README.md#the-path-for-people-and-agents)).
- **`httpd`**: an HTTP parser in its own budget, handing applications typed requests; a crash in it
  blames the connection it was serving.
- **Applications as ordinary principals' programs,** each reached through a capability `httpd`
  holds, with no socket and no key.
- **Inbound only through SSH forwarding**, so the box still accepts no connection but SSH.

**Attack cases:** a malformed request crashes nothing but its own connection's work; one client's
TLS state is never visible to another; an application never receives a socket or a key; nothing
listens except `sshd`.
