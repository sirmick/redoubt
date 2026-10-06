# BEAM3: asynchronous underneath: the VM's I/O through the hub, and files over 9P

Tier A (the VM's platform on Redoubt, the file natives, the pages), size M+. Needs BEAM2, AIO1
and BEAM7, all merged: start from `main`. The old `.wash/local/BEAM3-brief.md` is BEAM7's launch
brief and is superseded by this one. What the owner sees at the end: `File.*` works in an SSH
session on its home volume, and STEWARD2's session cases take their file verdicts
(`File.write`/`read`/`ls` under `/home/<principal>`, `/vault`, `/net`, and their refusals across
principals and labels), which are owed to this package. Run everything natively on this host
under the job pool's rules.

## Context rules (read these first)

- The spec is the pages: `docs/userland/beamlet.md` "Asynchronous underneath, synchronous on
  top" (planned) and "beamlet on Redoubt" (the `files` row), `docs/userland/files.md` "Files
  over 9P" whole (its table and its one Open item, ruled below), `docs/userland/native.md`
  "Many requests at once" (the hub as AIO1 built it), `docs/servers/serving.md` "Multiplexed
  connections" and R77, `docs/userland/sessions.md` (the namespace table). Then the code by
  item: `userland/otp/vm/src/platform.rs` (`Platform::files`, the `Files` trait :150-200,
  `FileError` :251), the host `Files` in `fake-redoubt`, `libs/client/src/{file,aio,ns}.rs`'s
  public API (the signatures, not the bodies), `userland/otp/redoubt/src/lib.rs`'s platform.
  `vm.rs` and `interp.rs` by function only.
- STEWARD2's brief (`.wash/local/STEWARD2-implementer.md`) only at "steward-two-sessions" and
  "steward-vault-session" (cases 2 and 3) for the verdicts you owe; not the rest.
- Reports under 1,900 bytes, detail in `.wash/local/BEAM3-report.md`.

## Rulings that shape it

1. **The hub is the mechanism, not a thread per call.** beamlet.md's planned paragraph (two
   scheduler threads, four short, three reserved, 246 waiting) was written before AIO1. AIO1
   built the client hub (`libs/client/src/aio.rs`: owner, not thread; inline submit; one thin
   waiter per connection; completions delivered into the owner's buffers) and the servers'
   multiplexed sessions (serving.md R77) for exactly this. So: every native call on a
   connection with a multiplexed session goes through the hub, holds no VM thread, and its
   completion becomes a message to the Erlang process that asked; `Platform::idle` returns on
   a timer deadline or a completion. A thread per waiting call remains only for a server that
   offers no multiplexed session; report which servers in the image those are after AIO1 (the
   console path is the one to check first), and the page's paragraph is rewritten to what is
   built: the hub, the waiters, and the thread count that remains, with the 255-thread bound
   stated. If a server the session needs has no session, that is a finding, not a workaround.
2. **A fid is the file descriptor; the position lives in the VM** (files.md). The `Files`
   trait already has `pread`/`pwrite` beside `read`/`write`/`seek`: the Redoubt `Files` keeps
   each handle's fid, qid and position, moves data in pieces of at most `MSIZE`, and clunks on
   close. Nothing is cached across calls but the position.
3. **Errors: the Open item on files.md is ruled as its recommendation.** Redoubt errors keep
   their own atoms everywhere (`:refused`, `:not_yours`, `:not_found`, a label or budget
   refusal, by the names of native.md "An Rerror has a name"); the `File` boundary alone,
   beamlet's `prim_file` layer, maps them to POSIX atoms by one fixed table on files.md:
   `not_found` → `:enoent`, `refused` and a label refusal → `:eacces`, `exists` → `:eexist`,
   `not_dir` → `:enotdir`, `removed` → `:enoent`, `too_large` → `:efbig`, a budget refusal →
   `:enospc`, anything else → `:eio` with the Redoubt name kept in the error's reason where
   OTP allows it. The table is the page's; the Open line goes; the status line names the host
   test that pins every row.
4. **Refuse visibly; report only real fields** (files.md): `File.stat` gives `:undefined` for
   `mode`, `uid`, `gid`, `links`, `inode`, `major_device`; `chmod`, `chown`, `ln_s`, `ln`
   return `{:error, :enotsup}`; `write_stat` applies times only. The planned cases on the
   page become real: host tests, each row.
5. **The namespace resolves the path** (sessions.md): the client library's `ns` finds the
   connection for a path's prefix and the rest is walked on it; a prefix no binding holds is
   `:enoent` (bob's `/home/alice` is "no entry", not "forbidden": the steward never bound it).
   `File.ls` leaves out entries the caller may not read (the server already does).

## "With aio": how the VM's threads map onto the hub (the owner's scope word, 2026-10-06)

The owner put BEAM3 in M1's scope "with aio". The mapping, exactly:
- **One hub per VM process** (`libs/client::aio::Hub`), owned by the scheduler thread: a
  native's request is `submit`ted inline on the scheduler thread (a send the server's parked
  completion call takes at once; never a blocking wait on the scheduler), with the request's
  buffer owned by the hub until its completion. Many outstanding requests on one connection,
  on one thread.
- **One waiter thread per connection** (`spawn_waiter`): it sits in the connection's
  long-poll completion call, `deliver`s what arrives into the hub, and wakes the scheduler.
  The connections are the session's binding slots (bootfsd, the home volume, the labelled
  volume, ipd, the console, the system volume: at most six), so at most six waiter threads.
- **`idle` is a receive on the VM's own wake endpoint** with the timer deadline; a waiter wakes
  it with one short send after a delivery. No kernel notification object is needed for this
  (AIO2's, later); say in the report if the wake costs more than one send per delivery batch.
- **Completion to mailbox:** the scheduler thread drains `completed()` after each wake and
  each slice, and each `Done` becomes a message to the Erlang process whose request it was
  (the tag → pid map is the hub owner's). A process that died meanwhile drops its completion
  and frees its buffer.
- **Threads that remain:** the schedulers (one until several harts) and the waiters. A call to
  a server with no multiplexed session takes a thread of its own for its duration, from a small
  fixed pool, with a timeout; name those servers (ruling 1) and the pool's size. The page's
  thread paragraph states this count and the 255 bound as what is built.

## BEAM8 is in the VM's code at the same time

BEAM8 owns `userland/otp/vm/src/{module,interp,loader,memory,opcodes}.rs`; BEAM3 owns
`platform.rs`, `bif/**` and `userland/otp/redoubt/**`. Both touch `vm.rs` (BEAM3: `idle`, the
completion drain, the natives' registration; BEAM8: little or nothing), `sched.rs` if the
completion dispatch lands there, `Cargo.lock`, and `tests/size-budget.toml`'s beamlet ceiling.
Order: whichever is accepted first merges first; the other rebases. Keep every `vm.rs` and
`sched.rs` edit of yours in one commit so the rebase is one hunk set; do not touch BEAM8's five
files; if a `Files` change needs `interp.rs`, stop and say so.

## What it builds

- `userland/otp/redoubt`: the Redoubt `Files` over `libs/client`'s `file` and `ns`, submitted
  through the hub; the VM's completion delivery (a completion → the asking process's mailbox;
  `idle` wakes on it); the waiter threads per connection as the hub needs them; the thread
  budget stated in one place with the 255 bound.
- `userland/otp/vm`: only what the `Files` trait and the `prim_file` natives need that is
  missing (say each addition); the POSIX mapping table lives in the `prim_file` layer, not in
  the trait.
- `libs/client/src/file.rs`: path helpers and bounded whole-file reads and writes if the
  adapter needs them (the old brief's "client followups"); keep them small and list them.
- The host: `fake-redoubt`'s `Files` keeps passing the same suite, so the host stays a strict
  subset of Redoubt.

## Cases

- Host (`beamlet-vm`, `beamlet-redoubt` on `fake-redoubt`): `File.read/write/ls/stat/rm/
  mkdir/rename` round trips; the `:undefined` fields; the four `:enotsup`; every row of the
  error table; a read past `MSIZE` in pieces; a position after `seek`; a path outside every
  binding is `:enoent`; a completion reaches the process that asked and no other.
- Machine, both widths: `beamlet-files`: a VM under `init` with a home volume
  (`littlefsd:data`) bound at `/home/alice` writes a file, reads it back, lists the directory,
  removes it, and a typed `File.read("/home/bob/x")` is `{:error, :enoent}`; the hub's
  requests counted (the serving library's stats line if present, else the case's own count)
  so the case shows no VM thread was held per read. `beamlet-console` and `userland-boot`
  unchanged in substance (the console path moved onto the hub must not change what they see).
- STEWARD2's cases 2 and 3: once both packages are on `main`, their file verdicts are
  enabled, in whichever lands second; the brief's owner of that step is the orchestrator.

## Pages (with the code)

`beamlet.md` "Asynchronous underneath" rewritten to the hub model as built (status built, the
tests); its `files` row in "beamlet on Redoubt"; `files.md` "Files over 9P" to built with the
error table and the Open line gone, its status listing the host tests and `beamlet-files`;
`native.md` "Many requests at once" gains one sentence that the VM is a hub owner;
`sessions.md`'s namespace example stays true. SECURITY.md rows only if a rule's tests change.
No dates, package IDs or review history.

## Owned paths

`userland/otp/redoubt/**`, `userland/otp/vm/src/{platform,bif/**}` for the `Files` trait and
`prim_file` natives only, `libs/client/src/file.rs` (helpers, listed), `tests/beamlet-files.toml`
and its data, the pages named. **Not yours:** the servers (`littlefsd`, `consoled`, `sshd`,
`ipd`), `libs/rt`'s serving library, the kernel, the shell's commandlets, the steward,
`libs/client/src/aio.rs` beyond a bug fix reported as such.

## The short gate

Both builds (rv64, rv32); host tests of `beamlet-vm`, `beamlet-redoubt`, `redoubt-client`;
the docs checker, `cargo fmt --check`, the size budget, the `unsafe` ratchet unchanged, the
no-cruft gate; own cases on both widths: `beamlet-files`, `beamlet-console`, `beamlet-boot`,
`beamlet-footprint` (the VM's footprint with the hub: report the change), `aio-many-reads`,
`userland-read-only`; and the smoke set (`userland-boot`, `init-boot`, `bench-net-peer`,
`ipc-outcomes`). The whole bench is the train's.

## Not here

`/net` and `gen_tcp` (BEAM5); launching programs and the natives' namespace, calls and
serving (BEAM4); screen natives (M2); file transfer (transfer.md); one thread waiting on
several endpoints (kernel work, the page's Open); littlefsd's read cost (its own note).

## Checkpoints

1. When the console path runs through the hub and `beamlet-console` is green on one width,
   before any file native: one progress line with the thread count that remains and which
   servers, if any, still need a thread per call.
2. When the host `File.*` suite is green, before the machine case: the error table as
   implemented, any `Files` trait additions, and the client helpers added.

## plan_set body for BEAM3 (needs unchanged: BEAM2, AIO1, BEAM7, all done: launchable)

Brief: .wash/local/BEAM3-implementer.md (architect-16). Tier A, size M+, start from main. The
hub (AIO1) is the mechanism: every native call on a multiplexed connection goes through it with
no VM thread held, completions become messages to the asking process, idle wakes on them; a
thread per waiting call only for a server with no multiplexed session (reported). Files: the
Redoubt `Files` over libs/client's file and ns, a fid per handle with the position in the VM,
pieces of MSIZE; files.md's error Open ruled: Redoubt atoms everywhere, one POSIX mapping table
at the File boundary; stat's undefined fields and the enotsup list pinned by host tests. Cases:
host suite on fake-redoubt; beamlet-files both widths (write/read/ls/rm under /home/alice, bob's
path :enoent). STEWARD2's cases 2 and 3 take their file verdicts once both are on main. Pages:
beamlet.md's asynchronous section rewritten to what is built, files.md to built. Not here:
/net (BEAM5), launching (BEAM4), screen, transfer.
