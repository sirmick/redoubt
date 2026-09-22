# Namespaces, 9P and filesystems

Designed, not built. Owns: 9P as the user-facing protocol, per-process namespaces, the network and
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

**Only the 9P `read` path parks today.** `serve_parking` hands back a request only when
`answer_in_place` returns `Answer::Waiting`, which is the `FileServer::read` -> `Read::Wait` path; a
**typed** opcode goes to the server's own dispatch, whose answer is always a reply. So a parked
*typed* call — the shape `resize` below needs — is not possible yet: it needs the typed dispatch to
hand a request back the way the read path does, a small `libs/rt` extension. That gap is **question
163**, open; `resize` is specified against it.
- **The file server says so.** `FileServer::read` returns `Read::Done(usize)` (at most `out.len()`, 0
  the end of the file) or `Read::Wait`: nothing to read yet and no end. Only `read` waits in milestone
  1; a `write` that must wait is a non-goal until a server needs it.
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
  `cols, rows`. It is the server's push channel, made of a `call` the client chose to make — so it
  needs no endpoint and no `send`. The client re-calls `resize` after each reply to wait for the next
  change; a client that never calls it misses every change, which is why `size` exists and a TUI
  re-reads it whenever it redraws.
- **One parked `resize` per connection is the client's to keep.** A second parked `resize` from the
  same client is not refused but is pointless: it is a second waiter on the same event, and the
  server answers both on a change. A server need keep no per-client resize state: it resumes every
  parked `resize` it holds when the size changes.
- **On a UART nothing resizes**, so `consoled` parks a `resize` call **for ever** (it is the same
  wait as a read with no input) and answers it only if its size is ever set again, which over UART it
  is not. It does not refuse opcode 17: a client that waits gets an honest wait, and one that would
  rather not can call `size` instead. `sshd` is where a `resize` first gets an answer, from the SSH
  window-change request (WP-S3).
- **`resize` depends on question 163**: only the 9P `read` path can park today, so a parked *typed*
  call needs the typed dispatch to hand a request back (a small `libs/rt` extension). Until 163 is
  answered, `size` (16) is implementable and `resize` (17) is specified but not buildable.

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
