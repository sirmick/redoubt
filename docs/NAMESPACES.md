# Namespaces, 9P and filesystems

Target namespace design; the shared 9P library and initial server components exist, while full
system integration and the console extensions below remain pending. Owns: 9P as the user-facing
protocol, per-process namespaces, the network and
console trees, filesystem servers, the filesystem choice. Byte layouts: WIRE.md. Borrows from Plan 9,
minus its ambient parts.

## Decisions
1. **Every user-facing service speaks 9P2000**: files, network, console, process information.
   Typed messages (WIRE.md) remain for internal plumbing (the block protocol, driver to server).
2. **A namespace is data inside a process**: a table from path prefix to capability. There is no
   kernel mount table and no global VFS server.
3. **A filesystem is an unprivileged server** (`fsd`), one instance per volume.

## Capabilities are 9P connections
- **One connection = one endpoint handle**, whose badge names the attach root inside the server.
- 9P `attach` gives a root fid; every `walk` is relative to a held fid. `Tattach`'s `uname` and
  `aname` are ignored; the badge decides. A server never walks above the attach root, so a fid is a
  directory capability.
- Fids are per connection: a fid cannot be named from another connection. A copied handle is the
  **same** connection (the same badge, so the same fids), which is why a launcher never passes its
  own connection on and gets each child a fresh one (CAPABILITIES.md, one badge, one client). To
  delegate a subtree, the holder asks the server for a new connection rooted there
  (`new_connection`, below) and sends that handle.
- `..` is resolved lexically (Plan 9's rule): the path is cleaned before lookup, in the client
  library and again in every server, so it never climbs above a held root.
- 9P messages travel in lent buffers of at most `msize` (WIRE.md).
- **Every 9P endpoint also serves typed operations**: a request whose word 0 is 0 is 9P, anything
  else is a typed opcode (WIRE.md). Every 9P server serves `ninep_common`:

  <!-- wire: ninep_common -->
  | Opcode | Message | Fields | Reply |
  | --- | --- | --- | --- |
  | 2 | `new_connection` | `root: string`, `quota: u64` | `conn: handle[0] endpoint`, `id: u64` |
  | 3 | `disconnect` | `id: u64` | - |

  <!-- wire-errors: ninep_common -->
  | Code | Error |
  | --- | --- |
  | 2 | `not_yours` |
  | 3 | `refused` |

  `new_connection` mints a connection rooted at `root`, a path relative to the caller's own root
  (empty for the same root; it never climbs above it), and returns it with a random connection id.
  `quota` is the byte quota the new root gets, carved from the granter's own (0: no quota of its
  own, it shares the granter's). **Bytes are metered by the file server, not by the shared
  library**: the library calls the server's two hooks, one when a connection is granted and one
  when it is disconnected, and `fsd` decides there what a byte costs, whether the carve fits, and
  what comes back; a server that meters nothing (`bootfsd`, `consoled`, `ipd`) implements neither
  hook and the number is passed on unused. `quota` stays on the wire for all of them, so a granter
  writes the same request whoever serves it (questions 117 and 118). `disconnect` frees the connection with that id and every connection
  minted under it, and returns its quota; only the holder of the id can name it (CAPABILITIES.md,
  disconnect). `refused` (code 3) answers a `new_connection` whose root does not exist or the
  caller cannot read, or whose cap, quota or server refuses it; `malformed` (code 1) stays for a
  request that does not decode, and `not_yours` (code 2) for a `disconnect` naming an id the
  caller did not receive (question 114).
- The 9P codec parses untrusted bytes, so it is written once, shared by every server, and fuzzed.
  Independent 9P implementations give differential tests. **Every 9P server runs the conformance
  corpus** (`libs/wire/vectors/9p.txt`) against the skeleton, which checks the things that are the
  skeleton's own: no vector panics, every answer decodes and carries the request's tag, a malformed
  request is refused with an `Rerror`, an R-message sent to a server is refused, and no vector holds
  a call or mints a connection.

## Holding a call (a server that must wait)
A 9P server sometimes cannot answer yet: a console read with no input, a `/net` connect waiting for
the network. It must not answer 0 (the end of the file: a client would read a live console as closed)
and must not block. It **parks the call** instead (CONTAINMENT.md, the shared server library).

**This is also how a server delivers an unprompted event to a client** (answer 160). The IPC
primitives are caller-initiated: a `call` and a `send` both start at the client, and a `reply`
answers a call the server already took. Nothing lets a server speak to a process that is simply
reading a file. So a server that has news and a client that wants it meet by the client **calling and
waiting**: the client makes a call that means "tell me when this happens", the server parks it, and
answers it when the event occurs. The parked call *is* the push channel; there is no second
mechanism, no endpoint to hand over, and no `send` (so WIRE.md's rule holds: every milestone 1 typed
message is a `call`).

**What a park costs.** A parked call holds one of the caller's `MAX_OPEN_CALLS` and one of the
server's admission slots (its bucket and share) for as long as it waits, which is why parked calls
are capped per (account, label set), reported abandoned when the caller gives up, and may carry a
deadline (`Parked::expired`). A client that wants no wait calls the non-parking form instead (a
`size` query, not a `resize` wait).

**Only the 9P `read` and `write` paths park.** `serve_parking` hands back a request only when
`answer_in_place` returns `Answer::Waiting`, which is the `FileServer::read` -> `Read::Wait` path
or the `FileServer::write` -> `Write::Wait` path (answer 174); a
**typed** opcode goes to the server's own dispatch, whose answer is always a reply. So a parked
*typed* call — the shape `resize` below needs — is not possible yet: it needs the typed dispatch to
hand a request back the way the read path does, a small `libs/rt` extension. That gap is **question
163**, open; `resize` is specified against it.
- **The file server says so.** `FileServer::read` returns `Read::Done(usize)` (at most `out.len()`, 0
  the end of the file) or `Read::Wait`: nothing to read yet and no end. Among 9P file operations,
  `read` and `write` wait in milestone 1; typed event waits such as `resize` are also in scope
  (answer 160), pending the typed-dispatch extension (question 163).
- **A write may wait too** (answer 174). `FileServer::write` returns `Write::Done(usize)` or
  `Write::Wait`, and `serve_parking` hands a waiting write back exactly as it does a read: same
  admission, same deadline, same abandonment. Any file server may use it; in milestone 1 only
  `ipd`'s `data` files do, when a socket's send buffer is full, so a TCP writer waits instead of
  spinning.
- **The skeleton hands the call back unanswered.** `NineServer::serve_parking(request, own)` is
  `serve_with`, except that a request the file server asked to hold is returned to the server with its
  T-message untouched in its lend: nothing of it is kept in the skeleton. `answer_in_place` returns
  `Replied`, `Waiting` or `NoRoom` (not `Option<()>`), so the read path can say which happened.
  `serve`/`serve_with` answer every request, so a server that returns `Read::Wait` without serving
  through `serve_parking` gets a refusal (`Rerror`), never a caller left waiting for a reply that
  never comes.
- **A held request's handles are closed when it is handed back**, and its handle list is emptied with
  them: they were delivered into this process's table when the call was taken, so holding them across
  the park would grow the table, and serving the call again would close indices that may name
  something this process has opened since.
- **Serving it again re-reads it from the lend**, so a fid clunked while the read waited makes the
  second serving an `Rerror`, which is what the client should see.
- **The server parks it in its own buckets.** A parked call is charged through the *same* `Admission`
  the server's fids are (`NineServer::admission_mut`, `share_of`), so a client cannot hold a server's
  fid table full and its parked calls full separately; and its deadline (`Parked::expired`), its
  abandonment (`Parked::abandoned`) and its `serve` before resuming are the parked-call rules
  (CONTAINMENT.md; answers 81, 82).

## Namespaces
- Per process, built by the parent before start and handed over in the startup block (INIT.md):
  `/` -> a directory capability, `/dev/cons` -> console, `/net` -> network capability, ...
- Nothing is inherited. A child sees exactly what its parent put in its table.
- Resolution is a Rust library (in beamlet, behind its `Platform` trait; Elixir code sees resource
  terms): longest matching prefix, then 9P walks on that capability.
- A process may rearrange its own table (bind a held capability at another name). It cannot create
  authority: every entry is a capability it already holds.
- Not borrowed from Plan 9: union mounts, kernel `#` device names (ambient authority), `rfork`
  namespace flags (the table is plain data).

## The console (`/dev/cons`)
A single file: reads return input bytes, writes send output bytes. `sshd` serves one per SSH channel;
`consoled` serves the UART's. The channel's labels are its session's (CONTAINMENT.md). **A read with
nothing to read parks** (Holding a call, below): it does not return 0, which would look to a client
like a closed console, and it is not an error the client must poll. `consoled` is the first server
that must wait, which is what proves the join.

**A console server also serves typed operations** on the same endpoint — the size query, and a wait
for a change — so a TUI can lay out its screen and redraw when the window moves. Both are `call`s
like every milestone 1 typed message (WIRE.md), and neither changes the byte-stream contract: a
client that never sends them still sees a plain pipe.

<!-- wire: consol ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `size` | - | `cols: u16`, `rows: u16` |
| 17 | `resize` | - | `cols: u16`, `rows: u16` |

<!-- wire-errors: consol -->
| Code | Error |
| --- | --- |

- The opcodes start at 16 because a console server is a 9P server and `ninep_common` reserves 1-15
  (WIRE.md).
- `size` (16) **answers now**: `cols` and `rows` are the terminal's size in cells. `consoled`
  answers from its manifest argument `cols,rows` (default 80×24; INIT.md's arguments are opaque
  strings each server's note defines); `sshd` answers from the SSH pty-req (WP-S3). A server that
  serves no `consol` (an older console, a file) refuses opcode 16 as `Malformed`, and the client
  answers "unknown".
- `resize` (17) **parks until the size changes** (answer 160, Holding a call): the client calls it
  with no fields, the server holds the call, and when the window changes it replies with the new
  `cols, rows`. **Opcode 17 is part of the milestone-1 console contract**, including `consoled`'s
  indefinite UART wait below. It needs WP-R1d's typed-parking extension (question 163), so WP-B2a
  implements opcode 16 first and opcode 17 after that extension. The client re-calls `resize` after each reply to wait for the next
  change; a client that never calls it misses every change, which is why `size` exists and a TUI
  re-reads it whenever it redraws.
- **A parked `resize` is keyed to the connection it arrived on, not to the server.** The answer is the size of *that* console: a server with one console (`consoled`) resumes every parked `resize` it holds on a change, but a server with many (`sshd`, one console per SSH channel, each with its own pty size and label set) resumes **only** the calls parked on the connection whose pty changed. Resuming them all would answer one channel's waiter with another channel's geometry — a labelled session reading an unlabelled one's terminal state, the read-up direction `check` exists to stop — so the parked call is keyed by (connection, console), and WP-S3 must keep enough state to do that. **A client re-calls `resize` after each reply** to wait for the next change; a second parked `resize` on one connection is a second waiter on the same event and is pointless but harmless.
- **On a UART nothing resizes**, so `consoled` parks a `resize` call **for ever**: it is the same
  wait as a read with no input, and it is not answered until the size changes, which over UART it
  never does. The server-side deadline that CONTAINMENT.md gives a parked call does not apply here —
  `consoled` parks with `FOREVER`, because a console read waits on a person and what reclaims it is
  the caller's abandonment, not a clock. It does not refuse opcode 17: a client that waits gets an
  honest wait, and one that would rather not can call `size` instead. `sshd` is where a `resize`
  first gets an answer, from the SSH window-change request (WP-S3).
- **`resize` depends on question 163**: only the 9P `read` path can park today, so a parked *typed*
  call needs the typed dispatch to hand a request back (a small `libs/rt` extension). Until 163 is
  answered, `size` (16) is implementable and `resize` (17) is specified but not buildable. That
  extension must answer a resumed typed call **only on the connection it arrived on** — a typed
  message has no lend and no fid, so nothing is re-read on resume and the connection the call came
  in on is the only thing that says which console it is for (see the keying rule above).
- **A multi-channel server receives per channel.** An abandoned-call notice reaches only the thread
  holding the call, on the endpoint the call came in on (KERNEL-SPEC.md), and a thread blocks in one
  `receive`. So `sshd`, which parks `resize` calls from many channels, needs a serving thread per
  channel — a thread parked on channel A never sees channel B's abandonment, and that call would
  stay open holding a slot. `consoled` has one console and one endpoint, so it needs nothing special.

**Current implementation:** the console's own typed-opcode callback in `consoled` still rejects
unsupported operations; the `size`/`resize` behavior above is the accepted target, not evidence
that either handler is integrated. Question 163 remains open for typed parking. Querying the size
afresh is the Redoubt client contract (USERLAND-API.md, answer 162 and the R-T1 correction).

## The network tree (`/net`)
`ipd` serves a Plan 9 style tree:
```
/net/tcp/clone        open to get a new connection directory N
/net/tcp/N/ctl        typed connect / listen / close operations (WIRE.md)
/net/tcp/N/data       read and write the byte stream
/net/tcp/N/remote     the peer address
/net/udp/...          the same shape
```
A socket capability is a connection rooted in part of that tree. **Its scope is IP prefixes and
ports only** ("connect to 10.0.0.0/8 port 443", "listen on TCP 22"). DNS runs in the client, so a
name-scoped check could only ever see the IP the client chose. Session scopes never include the box's
own addresses, including any address that routes back to the box (CAPABILITIES.md). `ipd` is a sink
and refuses labelled callers. Elixir wraps the tree in `gen_tcp`-like modules.

### What `ipd` serves in milestone 1 (answer 174)
IPv4 and TCP only, with a static address; `/net/udp` comes later. `ipd` runs `smoltcp` 0.14.0,
vendored (`vendor/smoltcp`), with IPv4, Ethernet and TCP and nothing else: no IPv6, DHCP, DNS,
UDP, raw or ICMP sockets, and no fragment reassembly, so a fragment is dropped.

**The files.** Everything a file holds is typed (WIRE.md's encoding), never text:
- `/tcp/clone`: a read at offset 0 makes a new socket, charged to the caller (below), and returns
  its number `n: u32`. Numbers are per connection, the lowest free one; `/tcp` lists only the
  caller's connection's sockets, and a walk to another connection's number is "not found". (A
  read makes it because the 9P skeleton does not move a fid when it is opened.)
- `/tcp/N/ctl`: a write is one `net_ctl` operation (below). A read returns `state: u32` and
  `n: u32`, and **waits** while a connect is in progress or a listener has nothing accepted
  (60 s at most, then `timeout`). States: 1 connecting, 2 established, 3 closing, 4 closed,
  5 listening; for a listener that accepted, `n` is the new connection's number. A socket
  neither connected nor listening yet reads as closed. The offset is not looked at: each read is
  the state now, or the next accepted connection.
- `/tcp/N/data`: the byte stream. A read waits while there is nothing to read (0 is the peer's
  end); a write waits while the send buffer is full (`Write::Wait`, above). Either waits 30 s at
  most, then answers `timeout`, and the client asks again: they wait on the network, not on a
  person, so they are not the console's exception.
- `/tcp/N/remote`: `addr: bytes[4]` (network order) then `port: u16`.

A socket lives until `close` (its graceful end), `abort`, or the `disconnect` of its connection
(which aborts every socket of it), then lingers at most 60 s more before `ipd` resets it, plus
TCP's 10 s TIME-WAIT; it stays charged to its owner until it is gone. An established socket
without acknowledged progress for 60 s is ended.

<!-- wire: net_ctl -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `connect` | `addr: bytes`, `port: u16` | - |
| 2 | `listen` | `port: u16`, `backlog: u8` | - |
| 3 | `close` | - | - |
| 4 | `abort` | - | - |

<!-- wire-errors: net_ctl -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `in_use` |
| 4 | `too_many` |
| 5 | `state` |
| 6 | `unreachable` |
| 7 | `refused` |
| 8 | `timeout` |

A `ctl` write that fails is an `Rerror` whose text is the error's name. `connect`'s `addr` is four
bytes, network order. It is checked in this order, before the stack sees anything: the box's own
addresses (below), then the scope, then the socket's state; success means the attempt has
started, and a read of `ctl` waits for the answer. The local port is drawn at random, unique
among every live socket of every owner (TIME-WAIT included) and never a listened port.
`listen` takes `backlog` 1 to 8: `ipd` keeps that many listening sockets, each charged to the
holder, and one waiting read of `ctl` returns each accepted connection as a new number. A
half-open connection (SYN received, no answer) is given 3 s, then the listener listens again. A
port belongs to the connection that listened on it first, and to the connections minted from it
by `new_connection("")`; any other gets `in_use`, even one granted from the same root.
`unreachable` means `ipd` has no link, or the kernel gave no random word for the connection's
sequence number (there is no fallback).

**The capability.** A connection's scope is at most 8 rules, each a **connect** rule (an IPv4
prefix and a port range) or a **listen** rule (a port range). As a `bytes` field (WIRE.md,
compound values): `count: u8`, then per rule `kind: u8` (1 connect, 2 listen), `addr: bytes[4]`
(network order; zero for listen), `len: u8` (0 to 32; zero for listen), `lo: u16`, `hi: u16`. A
scope is canonical: host bits zero, `lo` ≤ `hi`.
- **Root badges** are minted by `init`; `ipd`'s arguments say what each means (below). In the
  milestone manifest the steward's badge may connect anywhere, `sshd`'s may listen on 22, and
  `netd`'s is the ingress badge, which has no `/net` at all.
- **`grant`** mints a connection whose scope is the requested one, which must be canonical and
  **no wider** than the caller's: each rule inside one of the caller's of the same kind, prefix
  within prefix (no shorter), ports within ports. So a grant never widens and never adds
  `listen`. It is how the steward gives a principal its manifest `net` scope.
- **`new_connection`** keeps the caller's scope, rooted at `""` or `"tcp"` only (a socket's
  directory is not delegated). **`disconnect`** frees a connection, everything minted under it
  and every socket of them (`ninep_common`).
- **The box's own addresses** are refused to every scope, before the scope is looked at:
  `ipd`'s address, its network's network and broadcast addresses, `255.255.255.255`, `127/8`,
  `0/8`, `224/4`, `240/4`, and every `self=` prefix its manifest entry lists. On QEMU that is
  `10.0.2.0/24`: slirp maps every address of its network but the resolver to the host's
  loopback, where a forwarded port leads back to the guest's `sshd`, and the resolver to the
  host's. An address that routes back to the box from outside (a NAT's hairpin) is refused only
  if the manifest lists it; each `ipd`'s list must name every address of every `ipd` on the box.
  Inbound IPv4 of any protocol claiming to come from any of the box's own addresses but the
  gateway (`ipd`'s own, `127/8` and `0/8` among them) is dropped: it is spoofed or the box talking
  to itself, and answering it would mean asking ARP for one of the box's own addresses. The
  gateway is kept because QEMU's forwarded connections arrive from it. Inbound IPv4 that is not
  TCP is dropped from anyone: `ipd` serves only TCP, and answering it (ICMP "protocol
  unreachable") would reflect packets at whatever source it claims.

<!-- wire: ipd ninep -->
| Opcode | Kind | Message | Fields | Reply |
| --- | --- | --- | --- | --- |
| 16 | call | `grant` | `scope: bytes` | `conn: handle[0] endpoint`, `id: u64` |
| 17 | send | `frame` | `frame: bytes` | - |

<!-- wire-errors: ipd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `too_many` |

- `grant`: `not_permitted` for a wider, non-canonical or malformed scope, or a labelled caller;
  `too_many` when the caller's bucket holds its cap of connections.
- `frame`: one received Ethernet frame, in a one-page transfer, accepted only on the ingress
  badge (`netd`'s). It is the first `send` in any table (WIRE.md). A frame on another badge is
  dropped, and its pages unmapped.

**Labelled callers get nothing.** `ipd` refuses a caller whose budget carries any label before
it looks at 9P, `ninep_common`, `grant` or admission, so a labelled caller opens no bucket and
cannot even allocate a socket by reading `clone` (which `check` alone would let it read).

**Arguments** (INIT.md, arguments; each server defines its own), strict, all or nothing: an
argument `ipd` does not understand stops it (`BAD_ARGS`).
- `addr=A.B.C.D/LEN`, a unicast host address and its network; `gateway=A.B.C.D`, unicast and on it.
- `self=A.B.C.D/LEN`, up to 8; `ingress=BADGE`, the badge `netd` sends frames on.
- `scope=BADGE:RULE[,RULE...]`, one per root badge, up to 8; a rule is `c:A.B.C.D/LEN:LO-HI` or
  `l:LO-HI`.
- `buckets=N`, and `limits=BADGE:INFLIGHT:STATE:SOCKETS` overriding the default caps for one
  root badge. An override applies only to calls with account 0 through exactly that badge.
Badges are below 2^63 and appear once each. `ipd` refuses to start unless every bucket at its
cap fits its budget and its parked calls leave `MAX_OPEN_CALLS`' headroom, in the worst case:
`buckets` bounds how many buckets hold anything, whichever they are, so an override below the
default may be idle while a default bucket takes its slot. Each override counts as the larger of
its cap and the default, and every other bucket as the default; for parked calls that is at most
48 (QA D3-code-review-3). In the milestone manifest: six buckets, `sshd` 23 in flight and 20
sockets (a waiting accept, and a read and a write for each of 11 sessions), the steward 2 in
flight and 32 connections (the `/net` grants it has made), and four more buckets at the default 5
in flight and 8 sockets: 23 + 5 (the steward's slot, at worst a default one) + 4 × 5 = 48.

A link fault never stops `ipd`: without a working `netd` (its `info` fails, its MAC is not
unicast, or `transmit` answers `failed`) it answers `unreachable` to `connect` and `listen`,
keeps every other rule, and asks `netd` again with backoff.

## Filesystem servers
- **Holds:** one block-range handle (a partition from `blkd`). No MMIO, IRQ or DMA.
- **Serves:** 9P, one connection per client, each rooted where the granting party chose.
- **One instance per volume.** An untrusted medium gets its own server holding only that medium, so
  a parser exploit reaches that medium and nothing else.
- **Labels are per volume** (CONTAINMENT.md): each volume has one label set, from the boot manifest
  or the steward, and `fsd` checks the caller's labels against it on every request with `check`: a
  read (a qid and a `stat` included) needs the volume's labels ⊆ the caller's, a write needs them
  equal. A walk into a node the caller cannot read is refused, and a directory read lists only
  entries it can read. Its state is per volume. There are no per-file labels.
- **A remove succeeds while another connection holds a fid on the file.** An "in use" refusal would
  be a channel between connections.
- **Admission** is per (account, label set), with a fair share per badge inside it, and per badge
  for account 0 (the shared server library); a `disconnect` frees a client's fids.
- **A byte quota per attach root**, so Bob filling the `data` volume cannot make Alice's saves
  fail: each root a connection is minted at has its own quota, set by whoever granted it in
  `new_connection`'s `quota` field and carved from the granter's own. `fsd` meters it behind the
  shared library's grant and disconnect hooks; the library holds no byte counters (questions 117
  and 118).
- **Robust to a bad disk:** crash-consistent and robust to bad metadata, and fuzzed for it. Disk
  encryption is deferred (IO-ARCHITECTURE.md, Later).
- **Crash:** clients see errors, `init` restarts it (INIT.md), copy-on-write keeps the volume
  consistent.
- **Boot:** `bootfsd` is a read-only server over the verified boot bundle, mounted at `/boot`. It
  serves **only the bundle entries the boot manifest's `public` list names** (programs and module
  archives), as one flat directory, matched byte for byte; **never the manifest itself**, which
  carries `keyd`'s seeds and every principal's keys (INIT.md). A walk to any other name is "does
  not exist", the same answer as for a name the bundle never held, so `/boot` reveals nothing about
  the rest of the bundle. `init` reads the bundle and pushes the public entries' bytes to `bootfsd`
  (the `bootfs` protocol below); **`bootfsd` never sees the bundle and parses no archive.**
- **`fsd` typed operations:** `fsd` also serves typed messages on its 9P endpoint for what 9P2000
  does not express: `rename` and `copy_file` within one volume, and `get_attr`/`set_attr` for per-file
  metadata stored in littlefs custom attributes. They use the same label and quota checks as 9P.

  <!-- wire: fsd ninep -->
  | Opcode | Message | Fields | Reply |
  | --- | --- | --- | --- |
  | 16 | `rename` | `old_dir: u32`, `old_name: string`, `new_dir: u32`, `new_name: string` | - |
  | 17 | `copy_file` | `src_fid: u32`, `dst_dir: u32`, `dst_name: string` | `count: u64` |
  | 18 | `set_attr` | `fid: u32`, `attr: u8`, `value: bytes` | - |
  | 19 | `get_attr` | `fid: u32`, `attr: u8` | `value: bytes` |

  <!-- wire-errors: fsd -->
  | Code | Error |
  | --- | --- |
  | 2 | `not_found` |
  | 3 | `refused` |
  | 4 | `exists` |
  | 5 | `not_dir` |

#### Filling `/boot`: the `bootfs` protocol
`bootfsd` holds no bundle and parses no archive: **`init` reads the bundle and hands it the public
entries' bytes**, so the manifest never enters `bootfsd`'s address space at all and answer 123 holds
by construction rather than by a filter. The two operations are typed messages on `bootfsd`'s own 9P
endpoint, so their opcodes start at 16 (WIRE.md; `ninep_common` reserves 1-15):

<!-- wire: bootfs ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `add` | `name: string`, `offset: u64`, `data: bytes` | - |
| 17 | `seal` | - | - |

<!-- wire-errors: bootfs -->
| Code | Error |
| --- | --- |
| 2 | `refused` |

`add` appends `data` to the entry `name`, which must be one the argument list named and `offset`
must be exactly what has been added to it so far, so a chunk cannot be lost, repeated or reordered;
an entry larger than one message arrives as several. `seal` ends the setup: after it, `add` and
`seal` are `refused`, and only then does `/boot` answer walks at all, so no client can read an
entry that is half written. Both are `refused` from any connection `new_connection` minted, so only
the holder of the server's founding handle — `init` — can fill `/boot`, and nothing can refill it
after a client has seen it.

### littlefs
Criteria: a published on-disk format, an independent second implementation to test against,
power-loss safety, small enough to read. (Rust is required by tenet 3, so it is not a criterion.)
- **The littlefs format (its `SPEC.md`), reimplemented in pure Rust.** Copy-on-write metadata pairs,
  power-loss safe by design, bounded memory. The C reference runs only on the host, as a test oracle:
  every image either implementation writes must read back identically in the other. Nothing C runs
  on the target. (`littlefs2` on crates.io wraps the C library: not used.)
- **Metadata** in littlefs custom attributes: what 9P `stat` needs (mtime, qid version), and generic
  per-file attributes accessed through `fsd`'s typed `get_attr` and `set_attr`. No owners or
  permission bits: access is by capability.
- **Accepted limits:** large directories and files scale poorly; data is not checksummed (littlefs
  checksums metadata only, so a block device that returns wrong data undetected, beyond `blkd`'s
  contract in IO-ARCHITECTURE.md, can corrupt file contents silently). For
  milestone 1 also: no wear levelling (virtio disks do their own), no superblock expansion, and a
  file's attributes and its data are two commits, not one (WP-D2 asks for an atomic attribute commit
  if it needs one).
- **Rejected:** RedoxFS (no published spec, one implementation, format churn); ext4 as the native
  filesystem (too large).
- **Harness:** fuzzed images, crash injection at every block write, model-based tests against an
  in-memory reference, differential tests against the C reference.

## beamlet
One VM = one process = one namespace. `spawn` stays inside the VM; a new trust domain is a new VM
started by the steward (`Port` / `System.cmd` map onto that: USERLAND.md, after milestone 1).

## Prior art
Plan 9, Inferno/Styx, Fuchsia, WASI preopens, Capsicum.
