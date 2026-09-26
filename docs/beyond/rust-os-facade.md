# A Rust OS facade

## Idea

`redoubt-os`: one blocking `no_std` and `alloc` client library over the runtime and the wire codecs,
covering namespaces and files, the network, the console, processes and budgets. Authority is an
explicit argument; there is no ambient root and no global mount table. Native servers and
beamlet's platform layer would share it instead of each speaking 9P themselves.

| Layer | What it does |
| --- | --- |
| `redoubt-sys` | call numbers, encodings and kernel errors |
| `redoubt-rt` | IPC ownership, the heap, startup and the serving library |
| `redoubt-wire` | generated codecs and protocol parsing |
| the facade | files, sockets, processes and budgets as Rust types over the three above |

## Why it is not a goal

The runtime and the client crates of M4 (self-hosted development) already give native programs
what they need, one server at a time ([native programs](../userland/native.md)). A general facade is
worth building only once several programs show the same code, and it must not hide what the lower
layers make explicit.

## What it would need

- The runtime's contract kept whole: explicit lend and reply ownership, positional partial
  handles, protocol status and kernel errors; no error conversion that drops a resource.
- One error type that tells kernel, wire, server-protocol, refusal and disconnection outcomes
  apart.
- The startup encoder reused for launching; no second copy of the ABI or a wire format.
- Decisions before adoption: whether a namespace owns or borrows its connection; whether files
  buffer; lookup caching and recovery after a server restart; the process builder's arguments;
  whether sink checks live in the facade or in each server.

**Attack cases:** no facade call succeeds where the underlying call is refused; an error path never
leaks a handle.
