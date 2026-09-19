# Namespaces, filesystems and process launching

Status: agreed direction, 2026-09-18. Nothing here is built yet. Builds on IO-ARCHITECTURE.md
(tenet 7) and borrows from Plan 9, minus its ambient parts.

## Decisions
1. **Every user-facing service speaks 9P2000**: files, network, console, process information.
   Native typed Xous messages remain for internal plumbing (block protocol, driver to server).
2. **A namespace is data inside a process**: a table from path prefix to capability. There is no
   kernel mount table and no global VFS server.
3. **A filesystem is an unprivileged server**, one instance per volume.
4. **Process creation moves to a userspace launcher when runtime launching is first needed.** Until
   then the loader creates boot processes and the kernel's `CreateProcess` stays as it is.

## Capabilities are 9P connections
- A capability to a resource is a Xous connection to a server, attached at a root inside it.
- 9P `attach` gives a root fid; every `walk` is relative to a held fid. A server never walks above
  the attach root, so a fid is a directory capability.
- Fids are per connection and cannot be handed to another process. To delegate a subtree, the
  holder asks the server to mint a new connection rooted there and sends that connection over IPC
  (kernel prerequisite: transferable connections).
- `..` is resolved lexically (Plan 9's rule): the path is cleaned before lookup, in the client
  library and again in every server, so it never climbs above a held root.
- 9P messages travel in Xous lend buffers; no network transport is involved locally. 9P over TCP
  (to another machine) and virtio-9p (QEMU host directories, the Linux partition) come for free.
- A 9P codec is small and parses untrusted bytes, so it is written once, shared by every server,
  and fuzzed. Independent 9P implementations give differential tests.

## Namespaces
- Per process, built by the parent before start, handed over in the startup block:
  `/` -> a directory capability, `/dev/cons` -> console, `/net` -> network capability, ...
- Nothing is inherited by default. A child sees exactly what its parent put in its table.
- Resolution is a library (Rust) and beamlet's `Platform` (Elixir): longest matching prefix, then
  9P walks on that capability.
- A process may rearrange its own table (bind one held capability at another name). It cannot
  create authority: every entry is a capability it already holds.
- Not borrowed: union mounts (lookup becomes hard to reason about), kernel `#` device names
  (ambient authority), `rfork` namespace flags (the table is plain data).

## Networking as a tree (Plan 9 `/net`)
The net server serves a 9P tree: open `/net/tcp/clone`, write `connect 10.0.0.1!22` or
`announce *!22` to the control file, read and write the data file. A socket capability is a
connection rooted in part of that tree, and the server enforces its scope (which ports, which
peers). Elixir wraps the text control messages in `gen_tcp`-like modules.

## Filesystem servers
- **Holds:** one block capability (a partition range from the block server). No MMIO, IRQ or DMA.
- **Serves:** 9P, one connection per client, each rooted where the granting party chose.
- **One instance per volume.** An untrusted medium (a FAT card) gets its own server holding only
  that medium, so a parser exploit reaches that medium and nothing else. Separate trust domains can
  have separate instances on separate partitions.
- **Integrity is below it.** The block server does per-block AEAD and a Merkle root, so the
  filesystem must be crash-consistent and robust to bad metadata, but is not the security boundary
  against a hostile disk.
- **Crash:** the kernel notifies clients and the supervisor; the server restarts; copy-on-write
  keeps the volume consistent.
- **Boot:** the first filesystem is a read-only server over the verified boot bundle (the tar the
  loader authenticates), mounted at `/boot`. Programs launch from it until a disk is up.

### Which filesystem
Chosen for the tenets, not for being Rust (tenet 3 makes Rust a requirement, not a merit): a
published format, a second implementation to test against, power-loss safety, small enough to read.
- **The littlefs on-disk format (its `SPEC.md`), reimplemented in pure Rust.** Copy-on-write
  metadata pairs, power-loss safe by design, bounded memory, small. The C reference runs only on the
  host, as a test oracle: every image either implementation writes must read back identically in
  the other. Nothing C runs on the target. (`littlefs2` on crates.io wraps the C library: not used.)
- **Metadata:** littlefs custom attributes (typed tags per file, up to 1022 bytes by default) carry
  what 9P `stat` needs (mtime, qid version) and our own (content signatures, labels). Names, sizes
  and directory structure are native. No owners or permission bits: access is by capability.
- **Accepted limits:** large directories and files scale poorly; data is not checksummed by
  littlefs (the block layer's AEAD covers it); wear leveling is unneeded on virtual disks.
- **Fallback:** our own small CoW filesystem with a written spec. The filesystem is a per-volume
  9P server, so replacing it later changes nothing above it.
- **Rejected:** RedoxFS (no published spec, one implementation, format churn, its own encryption
  duplicating the block layer); ext4 as native (too large; read-only interop instead).
- **Harness:** fuzzed images, crash injection at every block write, model-based tests against an
  in-memory reference, differential tests against the C reference.
Interop servers, untrusted: `ext4-view` (read-only ext2/4, no panics on bad data), `fatfs`.

## Process launching
- **Now:** the loader creates the boot processes from the verified bundle; `CreateProcess` unchanged.
- **When runtime launching is needed:** the kernel primitive shrinks to "new address space and
  thread"; a launcher server (reached only through a launcher capability) reads the ELF from a file
  capability, checks its signature, maps segments, writes the startup block and starts it. The ELF
  parser leaves the TCB and gets a fuzz target.
- **Startup block:** the namespace table plus other granted connections (like WASI preopens,
  Fuchsia handle tables). Stdio is `/dev/cons` in the table.
- **Supervision:** the parent gets the kernel's death notification; Elixir supervisors build on it.
- **beamlet:** one VM = one process = one namespace. `spawn` stays inside the VM; a new trust domain
  is a new VM started through the launcher (`Port` / `System.cmd` map onto that).

## Prior art
Plan 9 (per-process namespaces, 9P, `/net`, lexical names), Inferno/Styx, Fuchsia (handle tables,
component namespaces), WASI preopens, Capsicum, seL4 (userspace ELF loading).
