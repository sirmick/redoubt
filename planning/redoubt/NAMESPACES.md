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
  delegate a subtree, the holder asks the server to mint a new connection rooted there and sends
  that handle.
- `..` is resolved lexically (Plan 9's rule): the path is cleaned before lookup, in the client
  library and again in every server, so it never climbs above a held root.
- 9P messages travel in lent buffers of at most `msize` (WIRE.md).
- The 9P codec parses untrusted bytes, so it is written once, shared by every server, and fuzzed.
  Independent 9P implementations give differential tests.

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
`consoled` serves the UART's. The channel's labels are its session's (CONTAINMENT.md).

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
- **Admission** is per (account, label set), and per badge for account 0 (the shared server
  library); a badge notice frees a dead client's fids.
- **Robust to a bad disk:** crash-consistent and robust to bad metadata, and fuzzed for it. Disk
  encryption is deferred (IO-ARCHITECTURE.md, Later).
- **Crash:** clients see errors, `init` restarts it (INIT.md), copy-on-write keeps the volume
  consistent.
- **Boot:** `bootfsd` is a read-only server over the verified boot bundle, mounted at `/boot`.

### littlefs
Criteria: a published on-disk format, an independent second implementation to test against,
power-loss safety, small enough to read. (Rust is required by tenet 3, so it is not a criterion.)
- **The littlefs format (its `SPEC.md`), reimplemented in pure Rust.** Copy-on-write metadata pairs,
  power-loss safe by design, bounded memory. The C reference runs only on the host, as a test oracle:
  every image either implementation writes must read back identically in the other. Nothing C runs
  on the target. (`littlefs2` on crates.io wraps the C library: not used.)
- **Metadata** in littlefs custom attributes: what 9P `stat` needs (mtime, qid version). No owners
  or permission bits: access is by capability.
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
