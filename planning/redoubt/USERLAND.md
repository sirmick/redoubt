# Userland: the Elixir interface to the system

Designed in outline, not built. Owns: what beamlet exposes as natives, the Elixir API over them,
file I/O, pipes and standard I/O, launching programs, and the shell. The system it talks to:
NAMESPACES.md (9P, namespaces), CAPABILITIES.md (handles, minting, exit notices), CONTAINMENT.md
(labels), INIT.md (startup block), PACKAGES.md (launching). The VM itself: `redoubt/beamlet`
(DESIGN.md). Milestone 1 needs only a slice of this: console, files, launching (BUILD-PLAN.md
WP-B1, WP-B2).

## The boundary
PLAN.md fixes the principle: a small fixed set of beamlet natives, every server binding in pure
Elixir over them. That splits userland in two, and the halves are unequal:

- **What the `Platform` trait already covers.** Files, console and TCP reach Elixir through natives
  beamlet already has (`prim_file`, console, `beamlet_tcp`). WP-B1 implements them over 9P, so
  `File`, `IO`, `Path` and `gen_tcp` work unchanged: the OTP stdlib comes for free. This is why the
  9P *client* is Rust, not Elixir.
- **What has no POSIX equivalent** gets a first-class Elixir API and no compatibility shim: handles,
  namespaces, budgets, leases, labels, approvals, and serving 9P.

## Natives (proposed)
| Native | Shape |
| --- | --- |
| `ns_lookup/1`, `bind/2`, `ns/0` | the namespace table: longest matching prefix, and the rest of the path |
| `call/3` -> `ref` | submit; the reply arrives as a message to the calling process |
| `send/2` | one-way |
| `serve/1`, `reply/2` | requests arrive as messages carrying badge, account and labels |
| `budget_create/1`, `budget_destroy/1`, `budget_usage/1` | `deadline` makes it a lease |
| `labels/0` | this VM's fixed label set |
| `process_create/2`, `process_start/3` | launching programs (question 133) |

Handles are resource terms: unforgeable, collected, never serialisable. **A copy of a handle is the
same connection** (one badge, one client), so passing one to another process inside the VM is
sharing, and passing one to another VM is impossible. Delegation is always `new_connection`
(NAMESPACES.md), which is a typed call on a connection, not a native.

## Asynchronous underneath, synchronous on top
Every native call is asynchronous: the VM submits and keeps running, and the completion arrives as a
message (beamlet DESIGN.md, I/O). On Redoubt the asynchrony comes from a small pool of I/O threads,
each making one blocking kernel `call`, because the kernel has no queued sends. Above that,
`File.read/1` is ordinary synchronous Elixir: the calling Erlang process blocks in `receive`, the
scheduler does not. Concurrency is bounded by the pool and by the server's admission limits per
(account, label set); at the limit a client waits its turn.

## Mounting and namespaces
There is no mount call and no kernel mount table. A namespace is a table inside the process, built
by the parent before start (INIT.md). "Mounting" is `bind/2` of a connection already held, so it
creates no authority. Consequences the API cannot hide:
- **Nothing is inherited.** A path with no matching prefix is `:enoent`, not a permission error.
- `..` is cleaned lexically before lookup, in the client and again in the server.
- `/home/alice` and a vault volume are usually **different servers**: crossing between them is a
  different connection, which is why rename and link below are not general.

## Sharing
`new_connection(root, quota)` mints a connection rooted at a subdirectory with its own byte quota,
and returns it with a random id. That is the only delegation primitive:
- A launcher never passes its own connection to a child; it mints a fresh one and keeps the id.
- `disconnect(id)` frees that connection and everything minted under it.
- A grant that must be revocable on its own is minted into a revocation scope (CAPABILITIES.md).
- Labels decide access per volume: read needs the volume's labels ⊆ the caller's, write needs them
  equal.

## File I/O
The path is `File` -> `:file` -> `prim_file` natives -> the 9P client -> `fsd` -> littlefs ->
`blkd`. A fid is the file descriptor and a directory fid is a capability; the file position lives in
the VM, since 9P reads and writes carry explicit offsets. Everything is chunked at the 64 KiB
`msize`. Plain 9P2000 (no `.u`, no `.L`) gives `stat`/`wstat` with name, length, mtime and qid
version, and nothing else, so:

| Operation | What happens |
| --- | --- |
| `File.cp`, `cp_r` | client-side read and write loops; no server-side copy; across volumes is fine |
| `File.rename`, same directory | `wstat` with a new name: the only rename 9P2000 has |
| `File.rename`, across directories | not expressible today (question 127) |
| `File.rename`, across volumes | copy and remove, never atomic |
| `File.chmod`, `chown` | no mode or owner bits exist: access is by capability (question 128) |
| `File.stat` | mode, uid, gid and atime have no source and would be synthesised (question 128) |
| `File.ln_s`, `ln` | no symlinks in plain 9P2000; the POSIX platform resolves them, so platforms differ |
| `File.rm` of an open file | succeeds by design: an "in use" refusal would be a channel (question 129) |
| directory listing | a read of a directory fid; entries the caller cannot read are omitted |

## Pipes and standard I/O
There is no pipe object, no file-descriptor table and no inheritance, so a pipe is a 9P file that
somebody serves, and a pipeline is the shell binding names in each child's namespace before
starting it. What follows from the primitives:
- **Backpressure is free**: a 9P write is a `call`, and the server replies when there is room.
- **EOF falls out of the existing rule**: the launcher disconnects a dead child's connections when
  it receives the exit notice, and the server sees the last writer go.
- **Labels work out**: a user-level server has no label exemption, and sessions are per label set,
  so a session's shell serves its own children's pipes; piping across label sets fails, as it
  should.
- **Distinct names per stream are required** (question 130): Plan 9 sends all three streams to
  `/dev/cons` and redirects by duplicating file descriptors, which do not exist here.

Who serves a pipe is question 131. A zero-copy alternative (children `send` pages to each other
over an endpoint, with the kernel's unqueued send as flow control) is not 9P, so a program could not
read its input as a file; not taken.

## Launching a program
One mechanism, PACKAGES.md's: the launcher creates a process in a budget naming an exit endpoint,
maps the system-signed loader stub, copies the ELF in as data, writes the startup block (handles,
namespace, argv) into a read-only page, and starts the process at the stub, which parses the ELF
inside the child's own budget. The proposed Elixir shape:

```elixir
{:ok, job} =
  Redoubt.Process.start("/bin/grep", ["-n", "needle"],
    namespace: [{"/", work}, {"/dev/stdin", pipe_r}, {"/dev/stdout", pipe_w}],
    handles:   [keys: keyd],
    budget:    [pages: 4096, processes: 1, weight: 10])
```

Every authority the child gets is on that call. Notes that shape the API:
- **The launcher reads the program**: a shell that cannot read the package directory cannot run
  anything. There is no kernel path lookup.
- **Killing is budget destruction**; there is no per-process kill, so a shell that wants killable
  jobs carves a budget per job.
- **One exit notice**, once, on the endpoint named at creation. No death subscriptions.
- At most `MAX_START_HANDLES` (64) handles, namespace entries included.
- Signatures gate only authority the steward *adds*: running one's own code within one's own
  authority needs none (PACKAGES.md).
- Each launch copies the program image; there is no shared text and no demand paging (question 132).

## Labels and capabilities in Elixir
A VM's label set is fixed when its budget is created, so `Redoubt.Label.self/0` is a read-only fact.
Reading above it fails; a labelled caller reaching a sink is refused. Neither is recoverable in the
process, so both are plain errors, and declassification (a steward request, answered out of band at
`approve@box`) is the only path out.

## The shell
No shell in the Elixir world replaces bash. The precedent that matches ours is Nerves, where IEx
*is* the shell on a device with no Unix:
- **IEx** (Apache-2.0, part of Elixir, already runs on beamlet) is the REPL: line editing, history,
  completion, helpers. PLAN.md step 3 is IEx on the UART.
- **Toolshed** (Apache-2.0, `elixir-toolshed/toolshed`) is the borrow target for the command
  surface: `cat`, `ls`, `tree`, `grep`, `cmd`, `top`, `uptime`, and path autocompletion, written for
  exactly this situation. Its bodies assume POSIX and Linux `/proc`, so we take the surface and the
  conventions and implement them over this API.
- Missing from that pair, in order of need: launching programs, pipes and standard I/O (above),
  command syntax (IEx is Elixir syntax; bare-word commands are a thin layer over the evaluator), and
  job control (Erlang's `user_drv`/`group`/`edlin` give Ctrl+G, but beamlet replaces `user_drv` with
  its own `beamlet_io`).

## Prior art
Plan 9 (namespaces, `/net`, `/dev/cons`), Inferno, WASI preopens, Capsicum, Nerves and Toolshed
(IEx as the device shell).
