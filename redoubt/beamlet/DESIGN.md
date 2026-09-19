# beamlet design

A BEAM interpreter in safe Rust. It runs Erlang and Elixir code compiled by the standard
compiler, inside a microkernel OS (xous64) and, for development and testing, on POSIX.

Name: `beamlet` is a working name.

## Goals, in order
1. **Security.** Code running on the VM gets only what the embedder's `Platform` grants. The VM
   treats every `.beam` file, literal and message as hostile input.
2. **Auditability.** A reader can hold the whole VM in their head: small, commented, one obvious
   way to do each thing. The VM crate is `#![forbid(unsafe_code)]`.
3. **Simplicity.** When simplicity and speed conflict, simplicity wins.
4. **Fidelity.** Where we implement something, it behaves exactly like the real BEAM. The
   differential test suite checks this.

Non-goals: speed, a JIT, NIFs or port drivers (foreign code lives in separate OS processes, which
ports may reach when the platform grants it: see Ports), distribution
(for now), hot code upgrade (for now), and running on OTP versions other than the pinned one.

## Pinned toolchain
The VM accepts bytecode from exactly one compiler: **OTP 28**, currently **28.5.0.6**, with
**Elixir 1.20.4**. We pin to the OTP version the current stable Elixir is built for, and move when
Elixir moves.
- OTP 27 and 28 have the same live instruction set (28 only adds the no-op `debug_line`). OTP 29
  adds native records (9 opcodes and a new term type), so 29 will be a planned upgrade.
- Pinning one version lets the loader reject anything else (old atom tables, compressed
  literals, deprecated or unknown opcodes) instead of carrying compatibility code.
- `tools/genop.tab` is the compiler's opcode table from the pinned tag. `vm/src/opcodes.rs` is
  generated from it by `tools/gen-opcodes.escript`.
- Toolchains are prebuilt archives from builds.hex.pm, checked by SHA-256, unpacked under
  `../toolchains/`. `tools/env.sh` puts them on `PATH`.

Upgrade procedure: vendor the new `genop.tab`, regenerate the opcode table, re-run the opcode
census (below), implement what changed, run the differential suite.

## Structure (`vm/src`)
| File | What |
| --- | --- |
| `platform.rs` | The whole OS interface: clock, idle, console, random bytes, code loading. |
| `term/`, `atom.rs` | Terms on per-process heaps: building, reading, order, copying, GC, `~w` printing; the atom table. |
| `loader.rs`, `etf.rs`, `module.rs` | `.beam` parsing and validation; the external term format. |
| `vm.rs` | Module registry, process table, scheduler, exit signals, timers. |
| `interp.rs` | The instruction loop, one `match` arm per opcode. |
| `bif/` | Native functions, all listed in one table in `bif/mod.rs`. |
| `bits.rs`, `float.rs` | Bitstring building and reading; float formatting. |

`cli/` is the POSIX embedding: `beamlet [-pa DIR]... MODULE [FUNCTION]`.

## Terms and heaps (concurrency option B; decided 2026-09-19)
Goal: concurrency equal to Erlang on Linux (all cores, per-process GC, copied messages), in safe
Rust. Reference-counted (`Rc`) terms, the first design, can never move between threads without
`unsafe`, so terms live on per-process heaps. Stages: (1) heaps, copying and GC on one
scheduler, every test still green (done); (2) several schedulers; (3) dirty schedulers for long
natives.
- **A term is a 16-byte `Copy` value** (`term::Term`). Immediates: integers (`i64`), floats,
  atoms (a leaked, interned `&'static` name: atoms are never freed, as in BEAM), `[]`, pids,
  references. Everything else is a `Ptr` to an object: a heap *space* and an index.
- **A heap is a `Vec<Term>`** (objects are a header cell followed by their cells; a list cell is
  two cells, no header) plus an off-heap table of `Arc`s: binary bytes, bignums, resources.
  Indices, not pointers, so growing the `Vec` moves nothing and nothing needs `unsafe`.
- **Spaces:** 0 is the heap itself; others are **literal chunks**, immutable and never freed, one
  per loaded module (and per `persistent_term:put`, as BEAM keeps those in the literal area).
  Any heap may point into a literal chunk, so literals are never copied, even between processes.
  A heap holds an `Arc` snapshot of the chunk list; copying into a heap refreshes it.
  Reloading modules leaks their literals (BEAM copies them into processes on purge; hot code
  upgrade is a non-goal for now).
- **Copying** (`copy`): a term moves between heaps by a sharing-preserving copy of the objects in
  the source's own space. Messages are copied into the receiver's heap on send; everything kept
  outside a process (ETS objects, monitor names, exit reasons, message timers, results) is an
  `OwnedTerm`: a small heap of its own plus its root.
- **GC:** Cheney copying collection of a process's heap at a safe point between instructions.
  The roots are everything the process holds: X and Y registers, the mailbox, the dictionary, a
  pending exit. Natives never see a collection, so they may hold terms in locals. A collection
  runs when the heap has grown by as much as the last one scanned (what survived plus the roots,
  at least 1024 cells), so collecting costs a constant share of the work however deep the stack;
  or when it references twice the off-heap bytes that survived (at least 1 MiB), as BEAM's
  virtual heap does, because a large binary costs the heap only a cell. Off-heap `Arc`s that no
  longer appear are dropped with the old heap.
- **Order-based structures** (maps, ETS keys, the dictionary) compare terms through their heaps.
- **Deep terms** stay iterative everywhere: copying, comparing, printing and GC use work lists or
  Cheney's scan, never Rust recursion. A million-level nested tuple, list or map is fine.
- **Writable binaries**: `bs_create_bin` with `private_append` appends in place when the heap's
  off-heap entry is the only reference and the binary ends at the end of its bytes, so building
  a binary by appending is linear.

### Stage 2: several schedulers (done 2026-09-19)
- **Locks without giving up `no_std`** (`sync.rs`): one `Lock<T>`. With the `std` feature it is
  a mutex and schedulers are threads; without it, a `RefCell` and one scheduler. Either way,
  taking a lock the same thread holds panics (the mutex tracks its owner) rather than
  deadlocking. Shared values are `Arc`; a resource's value is `Send + Sync` under `std` and
  keeps mutable state in a `Lock`. Loaded modules are leaked `&'static Module` (as literal chunks
  already were), so a return address is a plain copy: an `Arc` there made every call and return
  bounce one cache line between cores.
- **Messages through an inbox:** a sender never writes into another process's heap. A message
  is built on a heap of its own (BEAM's heap fragment, copied before any lock is taken) and
  queued in the receiver's table slot; the receiver appends fragments to its heap (shifting
  pointers, no graph walk) when a `receive` runs out of messages.
- **One lock for the rest** (`System`, behind `Lock`): a scheduler (`sched.rs`) runs a process's
  instructions without it, from its own copies and caches (atoms, limits, literal chunks and
  resolved calls, checked against generation counters). Natives lock it with `c.sys()` only
  where they use it: arithmetic, lists, maps and binaries never do. A native that reads and
  then acts holds one guard across both (spawn and set up, monitor, register, ETS operations,
  `process_info`), and never calls something that locks again while holding it.
- **A running process is its scheduler's:** changes other processes make to it (links,
  monitors, names, group leader, receive timeouts) queue on its slot and apply when its slice
  ends; exit signals to it wait; reading it (`process_info`) retries: the native asks to be
  called again and the caller yields.
- **Run queue:** one shared queue. A spawned process joins it when its parent's slice ends
  (BEAM puts it on the parent's scheduler), so `monitor(process, spawn(F))` sees it alive.
  Idle schedulers sleep on a condvar; when none runs, one blocks in `Platform::idle`.
  `system_flag(schedulers_online, N)` parks the rest. `beamlet --schedulers N` picks the count
  (default 1). Parallel `fib` on 1/2/4/8 schedulers: 339/167/87/47 ms; eight concurrent
  compiles of `lists.erl`: 29.3/18.2/10.3/6.1 s, with lock contention 1.6% of the profile at 8
  (the rest of the lost CPU is the allocator and memory traffic).
- **The platform has its own lock** (after the system lock, never before), so file and
  console I/O does not hold up other schedulers' natives.
- **Next:** finer locks (ETS, atoms, the process table) if profiles come to show contention;
  stage 3, dirty schedulers for long natives (with several schedulers a long native only
  occupies its own).

Integers are `i64` and move to `BigInt` (`num-bigint`) only on overflow, and back when they fit, so
each integer has one representation. Bignums are capped at 2^24 bits (`system_limit` beyond).

Maps are persistent AVL trees (`term/map.rs`) whose nodes are heap objects, ordered by the exact
term order. Versions share nodes, so `maps:put` on a map someone else still holds copies O(log n)
nodes, not the whole map (a plain `BTreeMap` made building a map quadratic). BEAM's iteration
order for atom keys depends on atom-table indices and is unspecified; ours is always term order.
The differential harness prints maps with `~kw` (ordered) so outputs compare, and tests must not
depend on raw iteration order.

A match context (`Term::Match`) is internal: it exists only between `bs_start_match*` and the end
of a binary match, as in BEAM.

## Processes and scheduling
One VM runs on one or more scheduler threads (see stage 2 above). Processes live in a slot table; a pid carries a serial number so a
stale pid never reaches a process that reused the slot. The scheduler is round-robin with a
budget of 2000 reductions (calls) per slice. Receive timeouts are a `BTreeSet` of deadlines; when
nothing can run, the VM calls `Platform::idle(next_deadline)`.

Exceptions unwind to the innermost handler recorded by `try`/`catch` (a handler stack, rather than
BEAM's scan of the stack for catch tags). The "raw" stack trace in x2 is `{Class, Trace}` so the
`raise` instruction can rethrow with the right class.

## Runtime services in Erlang
Some of what BEAM does in C, or in the kernel application, is here a small Erlang module in
`vm/lib/`, compiled by `tools/build-lib` and embedded in the VM (checked in, so building the VM
needs no Erlang toolchain; `tools/build-lib --check` keeps them honest):
- `beamlet_io`: the I/O protocol server behind `io:format`, `io:get_line`, `io:read`, `IO.puts`
  and `IO.gets`. Started at boot as `user` and `standard_error`; the group leader of every
  process. Output goes through `Platform::console_write`. Input requests queue and are served
  from a buffer; when it runs dry the server subscribes (`beamlet:console_subscribe/0`) and the
  VM forwards what `Platform::console_read` (non-blocking) returns as messages. While a reader
  waits, an otherwise idle VM sleeps in `Platform::idle(None)`, which returns when input
  arrives. Prompts are repeated for each line of a multi-line `get_until`, as OTP's `user` does,
  so IEx's transcript matches BEAM's.
- Logging is OTP's own `logger`, started at boot as a release does it (`beamlet_kernel`:
  `logger_server`, the kernel's logger configuration, `logger_sup`, the default handler), so
  handlers, filters and formatters (Elixir's Logger, ExUnit's `capture_log`) behave as on BEAM.
  An uncaught error or throw is reported to it as BEAM's emulator reports it. This adds about
  5 ms to boot. When the platform has no kernel `logger`, small stand-ins (`vm/lib/logger.erl`,
  `error_logger.erl`: level filtering, reports printed to `standard_error`) are loaded instead.

Other BEAM-internal modules (`init`, `erts_internal`, `code`, `net_kernel`, `persistent_term`,
`os`) are answered by natives. The environment (`os:getenv`) is the VM's own and starts empty:
the host's is not visible. There is no time zone: `localtime()` is UTC.

## Processes, messages, time
Links, monitors (including by registered name), aliases (`monitor/3`, `alias/0,1`, as used by
`gen:call`), exit signals (`exit/2`'s `kill` is untrappable; a link's is not), registered
names, the process dictionary, `send_after`/`start_timer`, and ETS (`ets.rs`: every table type,
enforced access, heirs, match specifications restricted to pure guard functions).

## Validation and failure
- The loader checks every table index, label, register number and literal before code runs. The
  interpreter then treats remaining malformations as `bad_code`: the process that ran them dies,
  uncatchably. Nothing in the VM panics on bad input.
- Resource limits, each failing with `system_limit` (or killing the process, for the stack):
  atoms 2^20, atom length 255, processes 2^16, stack 2^24 Y registers, bignums 2^24 bits,
  binaries 2^30 bits, tuples from `make_tuple` 2^24, ETF nesting 256. Memory, mailbox and ETS
  limits: see Resource limits.

## Resource limits (`vm::Limits`, `memory.rs`)
One VM is one trust domain, but a buggy or hostile process must not take the others down with
it. Every limit fails closed: the offender is ended, nothing is silently lost.
- **Mailbox** (`max_mailbox`, default 2^20): a message that would overflow a mailbox kills the
  receiver with `{system_limit, message_queue}`, untrappably. Dropping it instead would break
  protocols silently (a TCP stream with a hole in it); a process that far behind is broken.
- **Process memory** (`max_heap_words`, default 2^27 words, and BEAM's `max_heap_size` via
  `process_flag/2` or `spawn_opt/4`, which can only lower it): read off the heap
  (`memory::process`: heap cells, two words each, and the off-heap bytes it references). Checked
  at the end of each time slice; a process over a limit is collected first and killed only if
  what is live is still over, with reason `killed` (BEAM's behaviour). `kill => false` is
  accepted but does nothing (no report is logged).
  `process_info(P, memory | heap_size | max_heap_size)` and `erlang:memory/0,1` report the
  measurements (`code` is not tracked).
- **ETS** (`max_ets_words`, default 2^27 words, for all tables of the VM together): tables keep a
  running total; an insert that would pass it raises `system_limit`.
- **CPU**: within a VM, reductions preempt every process (including call-free loops). Between
  VMs, CPU share is the host scheduler's job (on Xous, the kernel's per-process scheduling); the
  embedder can also drive a VM in bounded steps (`Vm::run_bounded`).
- The limits are measurements, not an allocator: a single native that allocates a lot at once
  (e.g. `binary_to_list` of a large binary) is only caught afterwards. The hard backstop is the
  embedder's allocator; on Xous, the process's memory quota from the kernel.

## Testing
- **Unit tests** (`cargo test`): formats and parsers, with vectors taken from the real BEAM.
- **Differential tests** (`tools/difftest`): each `tests/<suite>/*.erl` exports `start/0`; it runs
  on the real BEAM (`tools/expect.escript`) and on beamlet, and the printed results must be
  identical. The real OTP `stdlib` `.beam` files are on beamlet's code path, so every test also
  exercises the loader and interpreter on OTP's own code. `<Module>.stdin` is a test's console
  input and `<Module>.args` its extra beamlet options (`--exec` for ports). A test that does
  not compile counts as a failure unless `SKIP` lists it.
- **Hostile input** (`vm/tests/hostile.rs`): every truncation of real `.beam` files is
  rejected; mutation fuzzing (20k rounds by default, 1M soaked) must never panic or hang the VM.
  It found four bugs, each now a regression test.
- **Corpora**: `tests/erlang` (ours), `tests/elixir` (Enum, String, structs, GenServer, Agent,
  Task, Supervisor, IO, exceptions), `tests/atomvm` (AtomVM's 491 modules; `SKIP` lists those
  that need AtomVM's build output, distribution or BEAM heap sizes, with reasons), and Elixir's
  own test suite (`tools/elixir-tests`; `tools/elixir-tests.skip` lists what fails by design).

## Opcode census
`tools/census.escript` lists the instructions and imports a set of `.beam` files use. On OTP 28:
126 opcodes are live (not deprecated); the Elixir standard library uses about 100 of them. The
loader refuses deprecated opcodes, and the interpreter implements all live ones except `on_load`.

## Performance
Not a goal, but measured (`perf`) so nothing is gratuitously slow. One core, wall time
including start-up; BEAM is OTP 28 with its JIT (about 0.09 s of which is its own start-up):

| Workload | beamlet | BEAM |
| --- | --- | --- |
| `fib(30)` | 0.24 s | 0.09 s |
| map/filter/sum over 1M-element list | 0.40 s | 0.13 s |
| 1M `maps:put` into a 10k-key map | 0.70 s | 0.22 s |
| `lists:sort` of 1M integers | 1.2 s | 0.25 s |
| 300k messages to another process | 0.10 s | 0.13 s |
| 1M-byte binary comprehension and match | 0.46 s | 0.17 s |

What the profile pointed at, and was fixed: natives looked up by name per call (now resolved
per import at load), allocation in term comparison, a reference-count per instruction fetch,
32-byte terms (now 16: bitstrings and atoms are thin pointers), allocation per dropped cons
cell, quadratic binary appends (`private_append` now grows a uniquely owned buffer in place),
and whole-map walks when dropping an old map version. What remains is the interpreter's own
dispatch and operand decoding; pre-decoding operands would be the next step if it matters.

## Crypto (`crypto/`)
`beamlet-crypto` implements OTP's `crypto` NIFs in pure Rust, so the real `crypto.erl` (and on
top of it `public_key`, `ssl`, `ssh`) runs unmodified. It is a separate crate the embedder opts
into (`Config::natives`), so the core VM and its trusted base stay small.
- Mechanism: when a module loads, any function with a registered native gets its body replaced
  by that native, as `erlang:load_nif/2` does. `-on_load` functions are not run. Native state
  (hash, MAC, cipher contexts) lives in resource terms, which are references to Erlang code.
- Algorithms: what modern TLS and SSH need, listed in `crypto/src/lib.rs`; `supports/1`
  reports exactly those, so `ssl`/`ssh` negotiate only what is here. Others raise `notsup`.
- Dependencies: RustCrypto (`sha1`, `sha2`, `sha3`, `md-5`, `hmac`, `aes`, `ctr`, `aes-gcm`,
  `chacha20`, `chacha20poly1305`, `poly1305`, `pbkdf2`, `p256`, `p384`, `ecdsa`, `rsa`) and
  dalek (`x25519-dalek`, `ed25519-dalek`). All pure Rust, `no_std`. Notes: `rsa` is the stable
  0.9 line, which has a known timing side channel in decryption (RUSTSEC-2023-0071; side
  channels are out of scope for now, see TENETS) and uses the previous `digest` generation,
  hence `sha1`/`sha2` 0.10 alongside 0.11. `cpufeatures` declares `libc` FFI for CPU detection
  on hosts; nothing in it is C.
- Randomness: only `Platform::random`. RSA key generation and padding draw from a ChaCha20
  keystream keyed from the platform per operation; everything else takes bytes directly.
  Failure to get randomness fails the operation.
- ECDSA signatures are deterministic (RFC 6979). Errors use the C NIFs' error shape, so
  `crypto.erl` reports them the same way.
- Tested differentially against OTP's crypto on OpenSSL (`tests/cryptotests`): every hash, MAC,
  cipher mode and padding, AEAD, and fixed-key key agreement and signature matches byte for
  byte; a hostile-argument test makes 20k calls with generated junk.

## Files (`Platform::files`, `vm/src/bif/file.rs`)
OTP's own `file`, `file_server` and `file_io_server` run unchanged; the VM implements only the
NIFs of `prim_file` and `prim_buffer`, over a `Files` trait the platform may provide.
- Names are resolved inside the VM: relative to the VM's own working directory
  (`file:set_cwd/1` changes it for this VM only), `.` and `..` resolved lexically, so no name
  climbs above `/`. What `/` is, is the platform's choice. The platform must still refuse
  escapes the VM cannot see (symbolic links).
- An open file belongs to the process that opened it; it is closed when that process exits.
  At most 1024 open files per VM (`emfile`). Times, permissions and links can be changed where
  the platform allows; ownership cannot (`enotsup`, which `write_file_info` tolerates).
- Files in memory (`file:open(Data, [ram])`) are `vm/lib/ram_file.erl`, OTP's `ram_file` API
  over a small server process instead of its C port driver.
- `file_server_2` starts at boot (about 2 ms) if the platform can load it: without a file
  system, `file:get_cwd/0` (which compilers call) still works and file operations fail with
  `enotsup`.
- POSIX: `beamlet --root DIR` exposes one directory through `cap-std`; `--mount /AT=DIR[:ro]`
  adds more, read-only if asked, and `--lib` names directories of applications for
  `code:lib_dir/1`. Symbolic links are resolved by the platform within the VM's name space
  (an absolute target starts at the VM's `/`, across mounts; 40 links is `eloop`), and cap-std
  still refuses anything that would leave a mount (unit tests try). Without `--root`, `file`
  calls fail with `enotsup`.
- The `Files` trait is synchronous and file-shaped, a first step towards the design below: its
  operations are 9P's (walk+open, read, write, stat, clunk, create, remove, wstat for rename),
  so a Xous platform implements it with a 9P client, and it can later fold into the one
  asynchronous interface without changing the Erlang side.

## Ports (`Platform::programs`, `vm/src/bif/port.rs`, `vm/lib/beamlet_port.erl`)
`open_port/2` with `{spawn, Command}` or `{spawn_executable, File}` starts a program, and so do
`os:cmd/1,2` and Elixir's `System.cmd/3` and `System.shell/2`, which are OTP's and Elixir's code.
- **A capability.** Starting programs is the platform's to grant (`Platform::programs`); without
  it `open_port/2` fails with `eacces`. A program is outside the VM's sandbox altogether (on
  POSIX, a host process with the user's rights, seeing the host's files), so the CLI grants it
  only with `--exec`. What the VM does decide: which executable (a path in its own name space,
  mapped by the platform), the program's environment (the VM's own, which starts with only
  `HOME`; `--env NAME[=VALUE]` adds to it) and its working directory. A shell command line is
  the host shell's: VM paths in it mean nothing to the host.
- **A port is a process** marked as a port in its `Pid`: to Erlang code it is a port
  (`#Port<0.N>`, `is_port/1`, ordered before pids, V4_PORT_EXT, `erlang:ports/0`), and links,
  monitors (`'DOWN'` with type `port`), exit signals and registered names are those of
  processes, with no new machinery. Its code is the embedded driver `beamlet_port`, which frames
  the program's output (stream, `{line, L}`, `{packet, N}`, `binary`, `eof`, `exit_status`) and
  handles `{Pid, {command, D}}`, `{Pid, close}` and `{Pid, {connect, New}}`. Writing
  (`port_command/2`, so data goes before a following close), closing (`port_close/1`, which ends
  the port before the caller runs again) and `port_info/1,2` are natives. Message order follows
  BEAM where Erlang and Elixir code depend on it (a last unfinished line after the exit
  status). There are no port drivers (`{spawn_driver, _}`, `port_control/3` are `badarg`).
- **The console is a port**, `{fd, 0, 1}` (and `{fd, 0, 2}` for `standard_error`), opened by the
  `user` I/O server as BEAM's is, for output. Console input still reaches `user` by
  subscription, so stdin is read only when a process asks for input.
- POSIX (`cli/src/programs.rs`): `std::process` with a thread per program reading its output
  and one writing its input (so neither blocks the VM), all feeding the one event channel that
  console input also uses, which is what `Platform::idle` waits on. Closing a port closes the
  program's input and stops reading its output; the program is not killed (as in BEAM).
- Tests: `tests/erlang/ports.erl` (against BEAM), AtomVM's port tests (which spawn `echo`), a
  VM test that ports need the grant, and Elixir's `System`, `Port` and `os:cmd` tests.

## I/O: one 9P client, asynchronous (decided 2026-09-18; files and console built as steps)
On Redoubt every user-facing service speaks 9P2000 and a process's namespace is a table of
capabilities (`planning/redoubt/NAMESPACES.md`). beamlet follows that:
- **`Platform` grows one generic I/O interface, a 9P client**, not per-service methods: attach,
  walk, open, read, write, clunk, stat on handles the embedder granted. Files are namespace walks,
  TCP is Plan 9's `/net` (`/net/tcp/clone`, `connect addr!port`, the data file), the console is
  `/dev/cons`. Framing (packet modes, active modes, line mode) stays in Erlang (`beamlet_tcp`),
  so the Rust side only moves bytes. Handles are unforgeable resource terms.
- **I/O is asynchronous.** The VM is one thread, so a blocking read would stop every process. The
  VM submits a request and continues; the completion arrives later as a message to the requesting
  Erlang process. `Platform::idle` returns on a timer deadline or a completion.
  Redoubt's kernel has no queued sends (every message occupies its sender's thread until taken),
  so on Redoubt the `Platform` gets this asynchrony from a small pool of I/O threads, each making
  one blocking call.
- **POSIX platform:** serves the same tree from host files and host sockets, so the differential
  suite exercises `gen_tcp`, `ssl` and `ssh` over real TCP against the real BEAM.
- Mailbox overflow kills the receiver (Resource limits), so an active-mode socket that outruns its
  owner ends the owner rather than corrupting the stream.

## Applications, networking, regular expressions
- **Applications** (`vm/lib/application.erl`): a small controller. It reads `.app` files through
  `Platform::load_app`, starts dependencies in order, calls `mod` callbacks, and keeps
  environments. `kernel` and `stdlib` count as running from boot; their start callbacks never
  run. No application masters, takeover or start types. Enough for `ssl`, `ssh` and Mix-built
  Elixir applications.
- **Runtime modules**: `erlang.beam` and `erts_internal.beam` from erts load normally (their NIF
  stubs are replaced by natives), but modules that only work on BEAM's C runtime (`init`,
  `prim_*`, `erl_prim_loader`, tracing) are on a never-load list (`vm::RUNTIME_MODULES`).
- **Code path** (`vm/src/bif/code.rs`): VM state rather than OTP's `code_server`. A module is
  looked for in path order: directories added with `code:add_patha/1` (in the VM's file
  system), then the platform's modules, then directories added at the end, as BEAM's order
  (so a protocol consolidated into a prepended directory replaces Elixir's own). Changing the
  path grants nothing: the same code can already load any bytes with `code:load_binary/3`.
- **TCP** (`vm/lib/gen_tcp.erl`, `vm/lib/beamlet_tcp.erl`): `gen_tcp` is replaced by a front for one
  backend whose sockets are `{'$inet', beamlet_tcp, Pid}`, so OTP's `inet` works on them
  unchanged. The backend is a loopback network inside the VM; there is no other network unless
  the platform provides one (on Xous: a backend talking to the network server). A VM does not
  learn its host's name (`inet:gethostname/0` is `localhost`).
- **Regular expressions** (`re/`, crate `beamlet-re`): OTP's `re` over `regex-automata`. Matching
  is linear-time for every pattern, so hostile patterns cannot cause runaway backtracking.
  The rule: a pattern either means what it means in PCRE or fails to compile; it never
  silently matches something else. So where the same text means different things in the two
  dialects it is translated (a trailing `$` also matching before a final newline, `\<` and
  `\>` as literals, literal `{`, `\d`/`\s`/`\w`/`\b` ASCII-only unless `ucp`), and missing
  features are compile errors, not emulations.
  Backreferences and lookaround in general do not compile. A lookbehind at the start of a
  pattern and a lookahead at its end (with no top-level `|`) are supported by checking them
  around each match, which covers Elixir's own `(?<!\\)\|` and `^(?=.+)`; unlike PCRE, a failed
  lookahead does not make the engine try a shorter match at the same place. Compile errors use
  PCRE's messages. `re:replace/split` are OTP's code.
  For testing only, the `pcre2` feature (`cargo build -p beamlet --features pcre2`) puts real
  PCRE2 (C, vendored) behind the same API, so differential runs against BEAM measure the VM and
  not the regex dialect. The test tools (`tools/difftest`, `tools/elixir-tests`) use it by
  default via `tools/build-beamlet`; `RE_ENGINE=rust` tests the default engine, and
  `tests/<suite>/SKIP.<engine>` lists what holds with only one of them. Never in a shipped build.
- End-to-end: OTP's `ssl` (TLS 1.2/1.3) and `ssh` (daemon and client) run unmodified between
  processes of one VM over the loopback (`tests/ssltests`, `tests/nettests`), matching BEAM.

## zlib (`vm/src/bif/zlib.rs`)
OTP's `zlib.erl` runs unchanged over its stream NIFs, implemented with `miniz_oxide` (raw
deflate and the zlib wrapper; gzip headers and trailers are done here, with the CRC checked).
Output comes a chunk at a time as `zlib.erl` asks, so `safeInflate` bounds what hostile input
can make one call allocate; queued data is capped per stream. Preset dictionaries are not
supported (`not_supported`); compression strategies are accepted and ignored.

## Compiler, IEx, hashing
- Elixir's compiler runs on the VM (`Code.compile_string`, `Code.eval_string`; difftest
  `CompilerTest`), and so does OTP's Erlang compiler. The code, atom, export and literal chunks
  it produces are identical to BEAM's; chunks written with `term_to_binary(_, [compressed])`
  (debug info, docs) differ in bytes because deflate implementations differ (`miniz_oxide`
  here), and decode to the same terms. Compressed terms are decoded with the claimed size
  bounded (at most 128 MiB, and no more than the input could inflate to).
- `process_flag(error_handler, M)` is honoured: a call to a missing function becomes
  `M:undefined_function/3`, which Elixir's parallel compiler uses to wait for modules.
- IEx's read-eval-print loop runs over the console (`IEx.Server.run/1`; difftest `IexTest`,
  whose transcript matches BEAM's). `Code.fetch_docs` reads OTP's compressed docs chunks.
- `erlang:phash2/1,2` is BEAM's `make_hash2`, bit for bit (difftest `phash`), because Elixir's
  type checker and user code key things on it.

## Open questions
- Name.
