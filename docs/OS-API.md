# OS API (draft)

A unified facade that exposes kernel calls and base-server protocols through one
Rust API layer, with thin pure-Elixir bindings above it. `USERLAND.md` sketches
the Elixir surface; this note owns the Rust side.

Status: **draft** — not frozen. It adds no kernel objects or wire formats, so it
can change without a `HISTORY.md` entry until a work package adopts it.

## Principles

1. **Rust is the primary surface.** Every native server, the loader stub, and the
   beamlet VM link `no_std` + `alloc` Rust. Elixir gets a thin wrapper in pure
   Elixir over a fixed small set of beamlet natives (`USERLAND.md`, PLAN.md).
2. **Capabilities are explicit arguments.** There is no ambient authority, no
   implicit root, no global mount table, no `std::fs` compatibility shim. A
   `File::open` needs a connection handle; a `Process::spawn` needs a budget and
   an exit endpoint.
3. **One error type.** Kernel errors (`Error` from `redoubt-sys`) and 9P/typed
   protocol errors are unified into `OsError` so application code sees one result.
4. **Blocking, not async.** The kernel has no queued sends and `redoubt-rt`'s 9P
   client is already synchronous. A future wrapper can live in a sibling crate
   later; the core facade blocks.
5. **No per-server code generation.** The wire codecs are generated (`WIRE.md`);
   the facade consumes them. Servers do not get per-crate generated bindings.

## What belongs here vs elsewhere

| Concern | Home | Why |
| --- | --- | --- |
| Syscall ABI | `redoubt-sys` | The kernel owns the encoding |
| IPC primitives, heap, startup | `redoubt-rt` | Shared by every native program |
| 9P / typed-message codecs | `redoubt-wire` | One parser, fuzzed, shared |
| **File, net, console, process, budget clients** | **`redoubt-os`** (this note) | Unified, capability-threaded API |
| Shell, supervision, `gen_tcp` shim | Elixir (`Redoubt` app) | Pure Elixir over beamlet natives |

## Crate: `libs/os` (`redoubt-os`)

Depends only on `redoubt-rt` and `redoubt-wire`. `no_std` + `alloc`, so every
native server can link it.

### Modules

| Module | Key types | What it wraps |
| --- | --- | --- |
| `fs` | `Fs`, `File`, `Dir` | 9P `Twalk`, `Topen`, `Tread`, `Twrite`, `Tclunk` |
| `net` | `Net`, `TcpStream`, `TcpListener` | 9P walks to `/net/tcp/...`, typed `connect`/`listen` |
| `console` | `Console` | 9P `/dev/cons` read/write |
| `namespace` | `Namespace` | 9P connection + path-prefix table |
| `process` | `ProcessBuilder`, `Child` | `process_create`, `process_map`, `process_start`, exit notices |
| `budget` | `BudgetClient`, `Lease` | `budget_create`, `budget_destroy`, `budget_usage`, deadline helpers |
| `label` | `LabelSet` | `labels` read, `check` helper, declassification requests |
| `time` | — | `time_now`, `sleep` |
| `error` | `OsError` | Unified error enum |

### Capability-threaded API sketch

All types take explicit handles. Nothing is global or lazy-static.

```rust
use redoubt_os::{Namespace, Fs, Net, ProcessBuilder, Budget};
use redoubt_rt::handle::Endpoint;

// A namespace holds one 9P connection + a prefix table.
let ns = Namespace::new(root_conn)?;

// Filesystem operations over that connection.
let fs = Fs::new(&ns);
let mut file = fs.open("/config.txt")?;
let n = file.read(&mut buf)?;

// Network: walk the namespace to whatever prefix maps to /net.
let net = Net::new(&ns)?;
let mut stream = net.tcp_connect("10.0.0.1:443")?;

// Process launching: every authority is on the call.
let child = ProcessBuilder::new(&budget, &exit_ep, &ns)
    .handle(keys_conn)
    .spawn("/bin/grep", &["-n", "needle"])?;
let notice = child.wait()?;
```

### Error model

`OsError` is a single enum whose variants are:
- `Kernel(Error)` — a `redoubt-sys` error from a direct syscall
- `Wire(redoubt_wire::Error)` — a message that did not encode or decode
- `Protocol { origin: &'static str, status: u32 }` — a typed-message error from a server
- `Refused` — the server answered `Rerror` or a typed `not_found`/`refused`
- `Disconnected` — the endpoint or connection is gone

No `std::io::Error` mapping: that type carries categories (`NotFound`,
`PermissionDenied`) that presuppose a POSIX permission model Redoubt does not
have.

## Relationship to beamlet

beamlet's `Platform` trait (Rust side, inside `userland/otp`) consumes
`redoubt-os` instead of duplicating 9P client code:

```rust
use redoubt_os::fs::File;

impl Platform for RedoubtPlatform {
    fn prim_file_open(path: &str, modes: FileModes) -> Result<File, Error> {
        let ns = current_namespace(); // per-VM state
        Fs::new(ns).open(path)
    }
}
```

The Elixir side is pure Elixir, no NIFs:

```elixir
defmodule Redoubt.File do
  def open(path, opts \\ []) do
    # calls prim_file native, which reaches the Rust Platform code above
    :prim_file.open(path, opts)
  end
end
```

## What this is not

| Out of scope | Where it lives |
| --- | --- |
| Async / futures | A future sibling crate can wrap the blocking API |
| `std` backend | Future userland work in PLAN.md, outside this capability-explicit `no_std` facade; no libc or POSIX compatibility commitment |
| Per-server generated bindings | `redoubt-wire` owns codegen; `redoubt-os` is hand-written |
| Direct kernel syscall crate | `redoubt-sys` already covers that |
| Startup block parsing, heap, panics | `redoubt-rt` |

## Prior art

- `redox_syscall` + `redox_std`: a Rust `std` for Redox, closer to POSIX than we want.
- Fuchsia `fuchsia_component` / `fidl`: generated bindings per protocol, which we reject.
- WASI preview 1: capability-based file and net, but via a flat C-like syscall table.
- Plan 9 `libbio`, Inferno `libdraw`: library layers over 9P/Styx; closer in spirit.

## Open questions

1. Should `Namespace` own the connection or borrow it? Owned is simpler; borrowed
   avoids an extra close on drop, but lifetime noise in `no_std` is painful.
2. Should `File` buffer internally (line buffering, `BufReader`)? Or stay
   unbuffered and leave that to callers? Unbuffered keeps the crate small;
   buffering can be a wrapper.
3. How does the Elixir side discover the current namespace? The VM holds it in
   per-process state; the `Platform` trait exposes it to Rust. The Elixir module
   does not need to name it explicitly for `File` calls.
4. `ProcessBuilder::spawn` needs to find the ELF and build the startup block.
   Reuse `redoubt-rt::startup` or duplicate? Reuse, but the builder API must
   expose the same knobs.
5. `Net::new` needs to walk the namespace to find the `/net` prefix. Should that
   walk happen once at creation, or on every operation? Once is the right answer,
   but failure modes (prefix not found, server restarted) need a design answer.
6. Should `redoubt-os` define a `Sink` trait for servers that refuse labelled
   callers (`ipd`, `fsd` on vault volumes)? Or is that an application-level check?
7. Error variant granularity: `LabelDenied` is a kernel error, but a server may
   also refuse on labels. Do both map to `OsError::LabelDenied`, or does
   `OsError::Protocol` carry enough detail?

## Reading order

| Before this | After this |
| --- | --- |
| `USERLAND.md` (Elixir surface) | Work package that builds `libs/os/` |
| `NAMESPACES.md` (9P, per-process namespaces) | The beamlet `Platform` trait consuming it |
| `CAPABILITIES.md` (handles, minting, delegation) | Elixir `Redoubt` app modules |
| `KERNEL-SPEC.md` (objects, costs, syscalls) | |
| `WIRE.md` (byte layouts, typed messages) | |
