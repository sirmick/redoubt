# Namespaces, 9P and filesystems

Designed, not built. Owns: 9P as the user-facing protocol, per-process namespaces, the network tree,
filesystem servers, the filesystem choice, process launching. Borrows from Plan 9, minus its
ambient parts.

## Decisions
1. **Every user-facing service speaks 9P2000**: files, network, console, process information.
   Native typed Xous messages remain for internal plumbing (the block protocol, driver to server).
2. **A namespace is data inside a process**: a table from path prefix to capability. There is no
   kernel mount table and no global VFS server.
3. **A filesystem is an unprivileged server** (`fsd`), one instance per volume.
4. **Control messages are typed and binary.** No server parses text commands; operations that Plan 9
   expresses as text written to a `ctl` file are typed 9P writes decoded by one shared, fuzzed codec.

## Capabilities are 9P connections
- A capability to a resource is a connection to a server, attached at a root inside it.
- 9P `attach` gives a root fid; every `walk` is relative to a held fid. A server never walks above
  the attach root, so a fid is a directory capability.
- Fids are per connection and cannot be handed to another process. To delegate a subtree, the
  holder asks the server to mint a new connection rooted there and sends that handle over IPC.
- `..` is resolved lexically (Plan 9's rule): the path is cleaned before lookup, in the client
  library and again in every server, so it never climbs above a held root.
- 9P messages travel in Xous lend buffers; no network transport is involved locally.
- The 9P codec parses untrusted bytes, so it is written once, shared by every server, and fuzzed.
  Independent 9P implementations give differential tests.

## Namespaces
- Per process, built by the parent before start, handed over in the startup block (INIT.md):
  `/` -> a directory capability, `/dev/cons` -> console, `/net` -> network capability, ...
- Nothing is inherited. A child sees exactly what its parent put in its table.
- Resolution is a library (Rust) and beamlet's `Platform` (Elixir): longest matching prefix, then
  9P walks on that capability.
- A process may rearrange its own table (bind a held capability at another name). It cannot create
  authority: every entry is a capability it already holds.
- Not borrowed from Plan 9: union mounts (lookup becomes hard to reason about), kernel `#` device
  names (ambient authority), `rfork` namespace flags (the table is plain data).

## The network tree (`/net`)
`ipd` serves a Plan 9 style tree: a client clones a connection directory, issues typed connect or
listen operations, then reads and writes the data file. A socket capability is a connection rooted in
part of that tree. **Its scope is IP prefixes and ports only** ("connect to 10.0.0.0/8 port 443",
"listen on TCP 22"). Names are not part of a capability: DNS runs in the client, so a name-scoped
check could only ever see the IP the client chose. Elixir wraps the tree in `gen_tcp`-like modules.

## Filesystem servers
- **Holds:** one block capability (a partition range from `blockd`). No MMIO, IRQ or DMA.
- **Serves:** 9P, one connection per client, each rooted where the granting party chose.
- **One instance per volume.** An untrusted medium gets its own server holding only that medium, so
  a parser exploit reaches that medium and nothing else.
- **Integrity is below it.** `blockd` does per-block authenticated encryption (AEAD) and a Merkle
  root, so the filesystem must be crash-consistent and robust to bad metadata, but is not the
  security boundary against a hostile disk.
- **Labels:** `fsd` stores the writer's labels on data and checks readers' labels (CONTAINMENT.md).
- **Crash:** clients see errors, `init` restarts it (INIT.md), copy-on-write keeps the volume
  consistent.
- **Boot:** the first filesystem is `bootfsd`, a read-only server over the verified boot bundle,
  mounted at `/boot`. Programs launch from it until a disk is up.

### littlefs
Criteria: a published on-disk format, an independent second implementation to test against,
power-loss safety, small enough to read. (Rust is required by tenet 3, so it is not a criterion.)
- **The littlefs format (its `SPEC.md`), reimplemented in pure Rust.** Copy-on-write metadata pairs,
  power-loss safe by design, bounded memory. The C reference runs only on the host, as a test oracle:
  every image either implementation writes must read back identically in the other. Nothing C runs
  on the target. (`littlefs2` on crates.io wraps the C library: not used.)
- **Metadata** in littlefs custom attributes (typed tags per file): what 9P `stat` needs (mtime, qid
  version), labels, content signatures. No owners or permission bits: access is by capability.
- **Accepted limits:** large directories and files scale poorly; data is not checksummed by littlefs
  (`blockd`'s AEAD covers it).
- **Rejected:** RedoxFS (no published spec, one implementation, format churn, its own encryption
  duplicating `blockd`); ext4 as the native filesystem (too large).
- **Harness:** fuzzed images, crash injection at every block write, model-based tests against an
  in-memory reference, differential tests against the C reference.

## Process launching
- **Now:** the loader creates the boot processes from the verified bundle; `CreateProcess` is
  unchanged.
- **When runtime launching is needed:** the kernel primitive shrinks to "new address space and
  thread"; the steward's launcher library reads the ELF from a file capability, checks its signer
  against the trust list (PACKAGES.md), maps segments, writes the startup block and starts it.
- **Supervision:** OS processes are restarted only by `init` (INIT.md); a parent is told when a
  child dies, and OTP supervisors act only inside a VM.
- **beamlet:** one VM = one process = one namespace. `spawn` stays inside the VM; a new trust domain
  is a new VM started by the steward (`Port` / `System.cmd` map onto that).

## Prior art
Plan 9 (per-process namespaces, 9P, `/net`, lexical names), Inferno/Styx, Fuchsia (handle tables,
component namespaces), WASI preopens, Capsicum, seL4 (userspace ELF loading).
