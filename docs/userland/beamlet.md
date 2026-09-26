# beamlet, the Elixir VM

beamlet is a BEAM interpreter in safe Rust: it runs Erlang and Elixir code compiled by the
standard compiler, OTP 28 with Elixir 1.20. Every session and every agent on Redoubt is one
beamlet VM. The VM gets from its embedder, through one Rust trait (`Platform`), exactly the
services it is granted (a clock, a console, random bytes, code, files, programs) and nothing
else, and it treats every `.beam` file, literal and message as hostile input. beamlet is built
and runs on the host, where its tests run; running it on Redoubt, over 9P, is planned.

## Purpose

A session needs a language, a standard library and a prompt, and it needs them without C, without
a POSIX kernel underneath, and small enough to audit. The BEAM gives Elixir and Erlang, OTP's
libraries, IEx, supervision and message passing; beamlet gives the BEAM in a form Redoubt can
trust: one crate with no `unsafe`, a loader that checks everything before code runs, limits that
fail closed, and one narrow boundary where the operating system comes in. The code is in
[`userland/otp`](../../userland/otp).

## How to use it

On the host, beamlet is a command:

```text
$ cargo run -p beamlet -- -pa ebin my_module start
$ cargo run -p beamlet -- --root ./sandbox -pa ebin my_module start     # expose one directory
$ cargo run -p beamlet -- --mount /data=./data:ro --root ./sandbox ...   # add a read-only mount
$ cargo run -p beamlet -- --exec --schedulers 4 ...                      # grant programs; 4 threads
```

Without `--root`, `file` calls fail with `enotsup`; without `--exec`, opening a port to a program
fails with `eacces`. The VM's environment starts empty (`--env NAME=VALUE` adds to it), so the
host's is not visible. Tests: `cargo test` in `userland/otp` runs the unit and hostile-input
tests; the differential suites (`tools/difftest`, `tools/elixir-tests`) need the pinned OTP and
Elixir toolchains installed.

On Redoubt there is no command to run: the steward starts a session's VM when a person logs in
([sessions](sessions.md)), and a launcher starts an agent's ([agents](agents.md)). What the code
in the VM sees is ordinary Elixir: `File.read!/1`, `IO.puts/1`, `:gen_tcp.connect/3`.

## What it can and cannot do

### Loading hostile code

Status: built · partly tested: runs on the host only · tested: host:beamlet-vm::fixtures_load, host:beamlet-vm::every_truncation_is_rejected, host:beamlet-vm::wrong_formats_are_named, host:beamlet-vm::mutants_never_panic, host:beamlet-vm::empty_frames_count_against_the_stack, host:beamlet-vm::rejects_hostile_input, host:beamlet-vm::safe_mode_creates_no_atoms, host:beamlet-vm::nesting_is_bounded, host:beamlet-vm::deep_terms_are_handled_iteratively

The VM crate (`beamlet-vm`) is `#![forbid(unsafe_code)]`, and so are `beamlet-re` and
`beamlet-crypto` ([`userland/otp/vm/src/lib.rs`](../../userland/otp/vm/src/lib.rs)).
- **One compiler.** The loader accepts bytecode from OTP 28 only. It refuses deprecated or unknown
  opcodes, old atom tables and anything else another version would need compatibility code for.
- **Checked before it runs.** The loader checks every table index, label, register number and
  literal before any code runs ([`userland/otp/vm/src/loader.rs`](../../userland/otp/vm/src/loader.rs)).
  A malformation that survives loading is `bad_code` at run time: the process that ran it dies,
  uncatchably. Nothing in the VM panics on bad input. Every truncation of real `.beam` files is
  refused, and 20,000 mutated modules per run load or fail without a panic or a hang.
- **Deep terms do not recurse.** Copying, comparing, printing and collecting use work lists, never
  Rust recursion, so a million-level nested term is fine. The external term format limits
  nesting to 256 and, in safe mode, creates no atoms.

### Limits inside one VM

Status: built · partly tested: runs on the host only · tested: host:beamlet-vm::full_mailbox_kills_the_receiver, host:beamlet-vm::full_own_mailbox_kills_the_sender, host:beamlet-vm::a_roomy_mailbox_is_not_a_limit, host:beamlet-vm::the_vm_heap_limit_kills, host:beamlet-vm::a_process_can_lower_its_own_limit, host:beamlet-vm::spawn_opt_sets_a_limit, host:beamlet-vm::under_the_limit_nothing_happens, host:beamlet-vm::ets_inserts_past_the_limit_raise, host:beamlet-vm::memory_is_reported, host:beamlet-vm::jump_loops_are_preempted, host:beamlet-vm::garbage_is_collected_and_live_data_survives, host:beamlet-vm::unreferenced_binaries_are_freed

One VM is one trust domain, but a buggy or hostile Erlang process must not take the rest of the
VM down. Every limit fails closed: the offender ends, and nothing is lost silently
([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs)).
- **Mailbox** (`max_mailbox`, 2^20 messages): a message that would overflow a mailbox kills the
  receiver with `{system_limit, message_queue}`, untrappably. Dropping it would break protocols
  silently, like a TCP stream with a hole in it.
- **Process memory** (`max_heap_words`, 2^27 words, and `max_heap_size`, which a process can only
  lower): checked at the end of each slice; a process over it is collected first and killed only
  if what is live is still over.
- **ETS** (`max_ets_words`, 2^27 words for all tables together): an insert past it raises
  `system_limit`.
- **CPU:** reductions preempt every process, including a loop of plain jumps with no calls.
- **Fixed limits**, each `system_limit`: 2^20 atoms of at most 255 characters, 2^16 processes, a
  stack of 2^24 slots, bignums of 2^24 bits, binaries of 2^30 bits.

These limits are measurements, not an allocator: one native that allocates a lot at once is
caught afterwards. The hard backstop is the embedder's allocator, and on Redoubt the session
budget's page limit ([R6 (charging)](../kernel/budgets.md#r6-charging)). CPU between VMs is the
kernel's to share, by budget weight ([scheduling](../kernel/scheduling.md)).

### The `Platform` boundary

Status: built · partly tested: runs on the host only; only the host embedding exists · tested: host:beamlet-vm::programs_need_the_platform_to_grant_them, host:beamlet-vm::names_resolve_inside_the_root, host:beamlet::symlinks_cannot_leave_the_root, host:beamlet::mounts_are_separate_and_may_be_read_only, host:beamlet::files_round_trip

Everything the VM gets from outside comes through the `Platform` trait
([`userland/otp/vm/src/platform.rs`](../../userland/otp/vm/src/platform.rs)):

| Method | What it gives | Default |
| --- | --- | --- |
| `monotonic_us`, `idle` | a clock that never goes backwards; sleeping until a deadline or an event | required |
| `system_time_us` | wall-clock time | required; may answer `None` |
| `console_write`, `console_read`, `console_size` | the `user` I/O device; input never blocks | no input; size unknown |
| `random` | cryptographically secure bytes; on failure the VM raises rather than use a weaker source | required |
| `load_module`, `load_app`, `module_file` | the bytes of a `.beam` or `.app` this VM may load | no applications |
| `files` | a file system, as `prim_file` sees it | none: `file` calls fail with `enotsup` |
| `programs` | starting programs behind ports | none: `open_port` fails with `eacces` |

- **Code enters only through `load_module`.** That is where an embedder enforces signing or an
  allowlist. Changing the code path grants nothing, since the same code can load any bytes with
  `code:load_binary/3`.
- **Files are the platform's.** OTP's own `file`, `file_server` and `file_io_server` run
  unchanged over a `Files` trait whose operations are 9P's (walk and open, read, write, stat,
  clunk, create, remove, wstat). Names resolve inside the VM, relative to its own working
  directory, with `.` and `..` resolved lexically, so no name climbs above `/`. What `/` is, is the
  platform's choice, and the platform must still refuse what the VM cannot see, such as a
  symbolic link out of a mount. An open file belongs to the Erlang process that opened it and
  closes when it exits; a VM has at most 1024 open files.
- **Programs are a large grant.** A program is outside the VM altogether, so `programs` defaults
  to none. The host CLI grants it only with `--exec`.
- **The host embedding** exposes one directory with `--root` through `cap-std`, and more with
  `--mount`, read-only if asked; a symbolic link resolves within the VM's own name space, and a
  link that would leave a mount is refused.

### What runs on it

Status: built · partly tested: runs on the host only; the differential suites against the real BEAM need OTP 28 and Elixir installed and are not run by the bench · tested: host:beamlet-vm::decodes_otp_output, host:beamlet-vm::encodes_like_otp, host:beamlet-vm::printing_matches_otp, host:beamlet-vm::matches_otp, host:beamlet-vm::block_hash_handles_every_tail_length, host:beamlet-re::pcre_spellings, host:beamlet-re::braces_are_quantifiers_only_when_counted, host:beamlet-crypto::certificates_round_trip, host:beamlet-crypto::nesting_is_bounded, host:beamlet-crypto::mutants_never_panic

Where beamlet implements something, it behaves as the real BEAM does, and the differential suite
checks it: each test runs on BEAM and on beamlet and the printed results must be identical
([`userland/otp/DESIGN.md`](../../userland/otp/DESIGN.md)).
- **OTP and Elixir unchanged.** OTP's `stdlib`, `logger`, `file`, `ssl` (TLS 1.2 and 1.3) and
  `ssh` run unmodified, and so do Elixir's standard library, its compiler, OTP's Erlang compiler
  and IEx, whose transcript matches BEAM's. Every live OTP 28 opcode is implemented except
  `on_load`.
- **Processes as on BEAM.** Links, monitors, aliases, exit signals, registered names, timers and
  ETS, on one or more scheduler threads with per-process heaps and copying garbage collection.
- **Regular expressions** (`beamlet-re`) run in linear time for every pattern, so a hostile
  pattern cannot backtrack for ever. A pattern either means what it means in PCRE or fails to
  compile; backreferences and general lookaround do not compile.
- **Crypto** (`beamlet-crypto`) implements OTP's `crypto` natives in pure Rust (RustCrypto and
  dalek), so `crypto.erl`, `public_key`, `ssl` and `ssh` run on it. It takes randomness only from
  `Platform::random`, and a failure to get it fails the operation. Its `rsa` crate has a known
  timing side channel in decryption; side channels are a stated wall, not one the design closes.
- **Not supported:** NIFs and port drivers (foreign code runs as a separate program), distribution,
  hot code upgrade, and any OTP version but the pinned one.

### beamlet on Redoubt

Status: planned · M1 (separation and containment)

On Redoubt, beamlet is a native program whose `Platform` is written against the system: one 9P
client over the connections in the VM's namespace, and the kernel's calls for the rest.

| Method | On Redoubt |
| --- | --- |
| `monotonic_us`, `idle` | the kernel's `time_now` (microseconds since boot); `idle` is a `receive` with a timeout ([timer](../kernel/timer.md)) |
| `system_time_us` | `None` until wall-clock time and time sync exist, in M5 (persist, install, share) |
| `console_write`, `console_read` | writes and reads on the `/dev/cons` connection; a read with nothing to read is parked by the server, so input arrives as a completion and `Eof` means the connection ended ([consoled](../servers/consoled.md)) |
| `console_size` | a fresh `consol` `size` call on every query, never cached; a server that does not serve it refuses the call and the answer is `None` |
| `random` | the kernel's `random` call |
| `load_module`, `load_app` | reads from the boot bundle (`/boot`, served by `bootfsd`) and, from M5 (persist, install, share), the principal's profile ([packages](packages.md)); never from the session's writable namespace |
| `files` | the 9P client: walk, open, read, write, stat, clunk on the namespace's connections ([files](files.md)) |
| `programs` | launching native programs in carved budgets ([native programs](native.md)) |

TCP is Plan 9's `/net`, served by `ipd`: `gen_tcp` works unchanged over a backend that opens
`/net/tcp/clone` and reads and writes the data file, and framing stays in Erlang, so the Rust side
only moves bytes ([ipd](../servers/ipd.md)). The size is asked afresh because the only console
whose size changes is an SSH channel, and a cached size would answer a redraw with the size from
before the change; a caller that wants to be told of a change uses the parked `resize` call
([the shell](shell.md)).

**Open:** whether beamlet needs the timer's counter frequency beyond `time_now`'s microseconds;
if it does, it is a new field of the startup block, owned by [init](../servers/init.md), and not
a new call.

### Natives

Status: planned · M1 (separation and containment)

What has no POSIX equivalent reaches Elixir through a small fixed set of beamlet natives, and
every server binding is pure Elixir over them:

| Native | Shape |
| --- | --- |
| `ns_lookup/1`, `bind/2`, `ns/0` | the namespace table: the longest matching prefix and the rest of the path |
| `call/3` | submit a call; the reply arrives as a message to the calling Erlang process |
| `send/2` | one-way |
| `serve/1`, `reply/2` | serve an endpoint: requests arrive as messages carrying badge, account and labels |
| `budget_create/1`, `budget_destroy/1`, `budget_usage/1` | carve and end budgets; a deadline makes one a lease |
| `labels/0` | this VM's label set, fixed when its budget was made |
| `process_create/2`, `process_map`, `process_start/3` | launching native programs |

Handles are resource terms: unforgeable, collected, and never serialisable. A copy of a handle
inside the VM is the same connection (one badge, one client), so passing one to another Erlang
process is sharing it, and passing one to another VM is not possible. Delegation is always
`new_connection`, a typed call on a connection, not a native ([sessions](sessions.md)).

**Open:** two layout choices.
- Launching: the three process calls as natives, with the startup block written by Rust (the
  encoder exists in `redoubt-wire`) from a namespace and handles the Elixir caller gives, so policy
  stays in Elixir and encoding in Rust. Recommended; undecided.
- Where the Elixir modules over the natives (`Redoubt.Namespace`, `Redoubt.Process`,
  `Redoubt.Budget` and the rest) live: a Mix package versioned with the system and loaded from the
  boot bundle, with only what the VM needs at boot embedded in it (recommended), or all embedded
  in the VM.

### Asynchronous underneath, synchronous on top

Status: planned · M1 (separation and containment)

The VM is one trust domain on a few scheduler threads, so a blocking call would stop every Erlang
process on that thread. So every native call is asynchronous: the VM submits it and keeps
running, and the completion arrives later as a message to the Erlang process that asked;
`Platform::idle` returns on a timer deadline or a completion. The kernel has no queued sends (a
message occupies its sender's thread until taken: [IPC](../kernel/ipc.md)), so on Redoubt the
asynchrony comes from a small pool of I/O threads in the VM's process, each making one blocking
`call`.

Above that, Elixir is ordinary synchronous code: `File.read/1` blocks the calling Erlang process
in `receive`, not the scheduler. Concurrency is bounded by the pool and by each server's
admission limits per (account, label set); at the limit a call waits its turn
([the serving library](../servers/serving.md)).

**Open:** none.

## Why

**An interpreter, not a JIT or a port of BEAM.** BEAM is C and a JIT emits machine code, and
Redoubt has neither C nor writable-and-executable pages anywhere
([R11 (memory)](../kernel/memory.md#r11-memory)). An interpreter in safe Rust is slower (up to
about five times, measured on the host) and keeps the whole VM inside the language's memory-safety
argument. Speed is not a goal; auditability is.

**One boundary.** Every service the VM can reach is a method of `Platform`, with a default of
nothing. A reader checks what a VM can do by reading one trait and one embedder, and an embedder
that grants less gets a VM that can do less, with no code in between to trust.

**One pinned compiler.** Accepting exactly one OTP version lets the loader refuse everything else
instead of carrying compatibility code for old formats, which is where parsers go wrong.

**One 9P client, not a method per service.** Files, TCP and the console are all 9P on Redoubt, so
one generic client in Rust covers them, and the framing (packet modes, line mode) stays in Erlang
where OTP already has it. Adding a service means serving a tree, not growing the trait.
