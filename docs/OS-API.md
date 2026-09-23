# Rust client facade proposal

**Draft; no `libs/os` implementation or adopted API.** Build the facade from integrated native
server and beamlet callers. [USERLAND](USERLAND.md) owns the language boundary;
[KERNEL-SPEC](KERNEL-SPEC.md) and each server's protocol own behavior.

The proposed `redoubt-os` is a blocking `no_std` + `alloc` client layer over `redoubt-rt` and
`redoubt-wire`: namespace/file, network, console, process and budget operations. Authority is
an explicit argument; there is no ambient root or global mount table. Sharing client code avoids
duplicating 9P logic in native servers and beamlet's Redoubt Platform.

| Existing layer | Responsibility |
| --- | --- |
| `redoubt-sys` | Syscall numbers, encodings and kernel errors |
| `redoubt-rt` | IPC ownership, heap, startup and shared server machinery |
| `redoubt-wire` | Generated codecs and protocol parsing |
| beamlet natives + pure Elixir | Language boundary and higher-level policy |

The facade must preserve explicit lend/reply ownership, positional partial handles, protocol
status and kernel errors. A proposed unified error type would distinguish kernel, wire,
server-protocol, refusal and disconnected outcomes. Error conversion must not discard resources.
Reuse the startup encoder for launching; do not duplicate ABI or wire formats.

## Decisions before adoption

- Whether a namespace owns or borrows its connection, and how callers discover per-VM namespaces.
- Whether files buffer or leave buffering to wrappers.
- Namespace lookup caching and recovery after a server restart.
- Process-builder arguments needed to expose all startup/authority choices.
- Whether sink checks belong in the facade or each server.
- Error granularity for kernel versus protocol label refusals.

Async wrappers and the future Rust `std` backend in [PLAN](PLAN.md) are outside this proposal.
There is no libc/POSIX compatibility commitment here. Retain generated wire codecs; derive
facade methods and any Elixir wrappers from actual callers when the work package is adopted.
