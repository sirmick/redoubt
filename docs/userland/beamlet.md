# beamlet, the Elixir VM

beamlet is a BEAM interpreter in safe Rust: it runs Erlang and Elixir code compiled by the
standard compiler, OTP 28 with Elixir 1.20. Every session and every agent on Redoubt is one
beamlet VM. The VM gets from its embedder, through one Rust trait (`Platform`), exactly the
services it is granted (a clock, a console, random bytes, code, files, programs) and nothing
else, and it treats every `.beam` file, literal and message as hostile input. beamlet runs on
the host and boots the shell on Redoubt with verified modules from the userland disk. File
operations and native launching on Redoubt remain planned.

## Purpose

A session needs a language, a standard library and a prompt, and it needs them without C, without
a POSIX kernel underneath, and small enough to audit. The BEAM gives Elixir and Erlang, OTP's
libraries, the compilers, supervision and message passing; beamlet gives the BEAM in a form
Redoubt can trust: its own crates hold no `unsafe`, a loader checks everything before code runs,
limits fail closed, and one narrow boundary is where the operating system comes in. The code is in
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
fails with `eacces`. The VM's environment starts empty, with `HOME` set to `/` when there is a
`--root` (`--env NAME[=VALUE]` adds to it), so the host's is not visible. Tests: `cargo test` in
`userland/otp` runs the unit and hostile-input tests; the differential suites (`tools/difftest`,
`tools/elixir-tests`) need the pinned OTP and Elixir toolchains installed.

On Redoubt there is no command to run: the steward starts a session's VM when a person logs in
([sessions](sessions.md)), and a launcher starts an agent's ([agents](agents.md)). What the code
in the VM sees is ordinary Elixir: `File.read!/1`, `IO.puts/1`, `:gen_tcp.connect/3`.

## What it can and cannot do

### Loading hostile code

<details><summary>Status: built · partly tested: hostile loader tests run on the host; checked modules boot on Redoubt, but the refusal of other OTP versions' opcodes and atom tables is not attacked by a named test · tested (10)</summary>

- host:beamlet-vm::fixtures_load
- host:beamlet-vm::every_truncation_is_rejected
- host:beamlet-vm::wrong_formats_are_named
- host:beamlet-vm::mutants_never_panic
- host:beamlet-vm::empty_frames_count_against_the_stack
- host:beamlet-vm::rejects_hostile_input
- host:beamlet-vm::safe_mode_creates_no_atoms
- host:beamlet-vm::nesting_is_bounded
- host:beamlet-vm::deep_terms_are_handled_iteratively
- bench:userland-boot

</details>

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

<details><summary>Status: built · partly tested: in a boot, only the process heap limit and the budget's backstop are attacked · tested (14)</summary>

- host:beamlet-vm::full_mailbox_kills_the_receiver
- host:beamlet-vm::full_own_mailbox_kills_the_sender
- host:beamlet-vm::a_roomy_mailbox_is_not_a_limit
- host:beamlet-vm::the_vm_heap_limit_kills
- host:beamlet-vm::a_process_can_lower_its_own_limit
- host:beamlet-vm::spawn_opt_sets_a_limit
- host:beamlet-vm::under_the_limit_nothing_happens
- host:beamlet-vm::ets_inserts_past_the_limit_raise
- host:beamlet-vm::memory_is_reported
- host:beamlet-vm::jump_loops_are_preempted
- host:beamlet-vm::garbage_is_collected_and_live_data_survives
- host:beamlet-vm::unreferenced_binaries_are_freed
- bench:beamlet-heap-flood
- bench:beamlet-budget-flood

</details>

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
  Residual: a native is not preempted, and `crypto:mod_pow` and finite-field Diffie-Hellman run
  `modpow` on operands only the bignum limit bounds, so one call can hold its scheduler for
  minutes ([todo](../todo/beamlet-bignum-bounds.md)).
- **Fixed limits**, each `system_limit`: 2^20 atoms of at most 255 characters, 2^16 processes, a
  stack of 2^24 slots, bignums of 2^24 bits, binaries of 2^30 bits.

These limits are measurements, not an allocator: one native that allocates a lot at once is caught
afterwards. The hard backstop is the embedder's allocator, and on Redoubt the session budget's page
limit ([R6 (charging)](../kernel/budgets.md#r6-charging)). On Redoubt the platform lowers
`max_heap_words` and `max_ets_words` to a sixteenth of the VM's budget each, which it takes from its
required argument `budget_pages=N`, the budget's pages
([todo](../todo/beamlet-budget-from-startup.md)). A flooding process peaks at about four times its
heap limit, the old heap, the collector's copy and its growth, so the budget must be at least twice
what the VM uses with no Erlang process running; then one flooding process, or the tables, meets its
limit while the VM still has pages. Several flooding at once, or a native's single large allocation,
reach the backstop instead, which ends the VM, and `init` restarts it. It is a server like any other
under `init`'s restart rule: a VM that cannot stay up (a start module that fails every time, a
manifest without `budget_pages`) is restarted until the limit, and then the machine reboots
([init](../servers/init.md#restarts-and-reboots)). CPU between VMs is the kernel's to share, by
budget weight ([scheduling](../kernel/scheduling.md)).

### The `Platform` boundary

<details><summary>Status: built · partly tested: file and program grants run on the host only; Redoubt's verified lookup is tested below · tested (9)</summary>

- host:beamlet-vm::programs_need_the_platform_to_grant_them
- host:beamlet-vm::the_bundle_wins_over_a_front_directory
- host:beamlet-vm::a_name_the_bundle_lacks_is_found_on_the_path
- host:beamlet-vm::a_refused_system_module_never_touches_the_code_path
- host:beamlet-vm::app_spec_uses_one_source_attempt_and_keeps_its_erlang_result
- host:beamlet-vm::names_resolve_inside_the_root
- host:beamlet::symlinks_cannot_leave_the_root
- host:beamlet::mounts_are_separate_and_may_be_read_only
- host:beamlet::files_round_trip

</details>

Everything the VM gets from outside comes through the `Platform` trait
([`userland/otp/vm/src/platform.rs`](../../userland/otp/vm/src/platform.rs)):

| Method | What it gives | Default |
| --- | --- | --- |
| `monotonic_us`, `idle` | a clock that never goes backwards; sleeping until a deadline or an event | required |
| `system_time_us` | wall-clock time | required; may answer `None` |
| `console_write`, `console_read`, `console_size` | the `user` I/O device; input never blocks | no input; size unknown |
| `random` | random bytes from a cryptographic source; on failure the VM raises rather than use a weaker source | required |
| `load_module`, `load_app`, `module_file` | a system `.beam` or `.app` lookup answers found bytes, an absent name or a refused object; `module_file` names a loaded module | applications absent |
| `files` | a file system, as `prim_file` sees it | none: `file` calls fail with `enotsup` |
| `programs` | starting programs behind ports | none: `open_port` fails with `eacces` |

- **`load_module` is a lookup, not a gate.** It asks the platform for a module before examining
  the code path. `Found` uses those bytes, so no directory shadows a system module
  ([packages](packages.md#profiles-and-upgrades)); `Absent`
  searches the code path's directories in order, including those added with `code:add_patha/1`;
  `Refused` stops without touching the code path
  ([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs), `locate_module`). On Redoubt
  a name missing from `system.index` is absent, while an indexed object that fails verification
  is refused ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)). `load_app`
  carries the same three outcomes: `beamlet:app_spec/1` makes one platform attempt and returns
  the bytes on `Found`, or `error` on `Absent` and `Refused`. Residual:
  the bundle's protocols are not consolidated when it is built, so a protocol consolidated in a
  session's own directory is not used and protocol dispatch stays the slower, unconsolidated kind;
  behaviour is the same. Code in the VM can also load any bytes it holds with
  `code:load_binary/3` ([`userland/otp/vm/src/bif/info.rs`](../../userland/otp/vm/src/bif/info.rs)),
  through the same loader checks. So what confines loaded code is not how it arrived but what the VM
  holds: every module, however loaded, reaches only what the `Platform` grants. Loading one's own
  bytecode acts within one's own authority.
- **Files are the platform's.** OTP's own `file`, `file_server` and `file_io_server` run
  unchanged over a `Files` trait, path-based and POSIX-shaped: `open`, `read`, `pread`, `pwrite`,
  `seek`, `info`, `list_dir`, `rename`, `delete` and the rest; a 9P client implementing it is
  Redoubt's, below. Names resolve inside the VM, relative to its own working
  directory, with `.` and `..` resolved lexically, so no name climbs above `/`. What `/` is, is the
  platform's choice, and the platform must still refuse what the VM cannot see, such as a
  symbolic link out of a mount. An open file belongs to the Erlang process that opened it and
  closes when it exits; a VM has at most 1024 open files.
- **Programs are a large grant.** A program is outside the VM altogether, so `programs` defaults
  to none. The host CLI grants it only with `--exec`, and the programs it then starts are host
  processes with the user's own rights, not sandboxed.
- **The host embedding** exposes one directory with `--root` through `cap-std`, and more with
  `--mount`, read-only if asked; a symbolic link resolves within the VM's own name space, and a
  link that would leave a mount is refused.

### What runs on it

<details><summary>Status: built · partly tested: beamlet boots the shell on Redoubt; the differential suites against the real BEAM need OTP 28 and Elixir installed and are not run by the bench, and linear-time matching, crypto's refusal without randomness, the cofactored Ed25519 check and the bound on a zlib stream are not attacked by a named test · tested (15)</summary>

- host:beamlet-vm::decodes_otp_output
- host:beamlet-vm::encodes_like_otp
- host:beamlet-vm::printing_matches_otp
- host:beamlet-vm::matches_otp
- host:beamlet-vm::block_hash_handles_every_tail_length
- host:beamlet-re::pcre_spellings
- host:beamlet-re::braces_are_quantifiers_only_when_counted
- host:beamlet-crypto::certificates_round_trip
- host:beamlet-crypto::nesting_is_bounded
- host:beamlet-crypto::mutants_never_panic
- host:beamlet-crypto::ed25519_is_rfc_8032
- host:beamlet-crypto::the_all_zero_seed_is_refused_not_a_panic
- host:beamlet-crypto::x25519_refuses_a_low_order_point
- host:beamlet-vm::compressed_terms_round_trip
- bench:userland-boot

</details>

Where beamlet implements something, it behaves as the real BEAM does, and the differential suite
checks it: each test runs on BEAM and on beamlet and the printed results must be identical.
- **OTP and Elixir unchanged.** OTP's `stdlib`, `logger`, `file`, `ssl` (TLS 1.2 and 1.3) and
  `ssh` run unmodified, and so do Elixir's standard library, its compiler, OTP's Erlang compiler
  and IEx, whose transcript matches BEAM's. Every live OTP 28 opcode is implemented except
  `on_load`.
- **Processes as on BEAM.** Links, monitors, aliases, exit signals, registered names, timers and
  ETS, on one or more scheduler threads with per-process heaps and copying garbage collection.
- **Regular expressions** (`beamlet-re`) run in linear time for every pattern, so a hostile
  pattern cannot backtrack for ever. A pattern either means what it means in PCRE or fails to
  compile; backreferences and general lookaround do not compile.
- **Crypto** (`beamlet-crypto`) implements OTP's `crypto` natives in pure Rust, so `crypto.erl`,
  `public_key`, `ssl` and `ssh` run on it. It takes randomness only from `Platform::random`, and a
  failure to get it fails the operation. Its `rsa` crate has a known timing side channel in
  decryption; side channels are a stated wall, not one the design closes.
- **One X25519 and Ed25519 for the whole box.** They are `ed25519-compact`'s, the crate the loader,
  `keyd` and `sshd` use, built from the same vendored bytes, and the RustCrypto primitives beamlet
  shares with the servers are built from them too
  ([vendored dependencies](../testbench.md#vendored-dependencies)). Where it departs from OpenSSL,
  and so from the real BEAM: an all-zero Ed25519 seed is refused as a bad key, where the crate
  would panic; and a signature is checked cofactored, as the loader's and `sshd`'s are, so one
  whose R differs by a point of small order verifies. That forges nothing without the key, but
  signatures are malleable, and code that takes a signature as an identifier must not.
- **Compression** is OTP's `zlib`, whose natives run on `miniz_oxide`: deflate and inflate in raw,
  zlib and gzip formats, so `:zlib`, `:zip` and compressed external terms work unchanged. Each
  stream bounds what it holds queued, so a hostile archive cannot make one call allocate without
  limit.
- **Not supported:** NIFs and port drivers (foreign code runs as a separate program), distribution,
  hot code upgrade, and any OTP version but the pinned one.

### The console on a host

Status: planned · M2 (usable shell)

On a host, beamlet's command line puts the terminal in raw mode for as long as the VM runs and
restores it on every exit, a panic included. `console_size` is the terminal's size, and a change
of size reaches the shell as the message `{:console_resize, cols, rows}`, as the console's parked
`resize` delivers it on Redoubt ([the shell](shell.md#the-terminal-library)). Console input goes
to one Erlang process, the shell's driver, which takes it with `beamlet:console_subscribe/0`; a
second subscription is refused, so no code run at the prompt can take the keyboard, or the
interrupt key with it, from the driver.

**Open:** none.

### The console, the clock and randomness

<details><summary>Status: built · partly tested: its tests run on the host, on the fake kernel, against a console server that keeps `consoled`'s protocol with a host terminal for its device; it runs in a boot in bench:beamlet-boot and bench:beamlet-console · tested (7)</summary>

- host:beamlet-redoubt::writes_reach_the_screen
- host:beamlet-redoubt::typing_reaches_the_vm_then_its_end
- host:beamlet-redoubt::a_read_waits_for_typing_without_holding_the_vm
- host:beamlet-redoubt::a_console_without_consol_has_no_size
- host:beamlet-redoubt::idling_with_a_deadline_returns_by_it
- host:beamlet-redoubt::after_the_console_ends_idling_still_waits_for_its_deadline
- host:beamlet-redoubt::there_is_no_wall_clock

</details>

The first part of beamlet's platform on Redoubt, `beamlet-redoubt`
([`userland/otp/redoubt`](../../userland/otp/redoubt/src/lib.rs)), serves the console, the clock
and randomness over the client library and the runtime's calls. `/dev/cons` is opened once. The
VM's thread writes to it and asks its size; a reader thread of its own, with its own lend, reads
it and hands what it read to the VM's thread as messages, so a read that waits never holds the
VM's thread. After the console's end, `idle` still sleeps until its deadline. There is no wall
clock, so `system_time_us` is `None`. `./shell --fake` runs the shell on it.
- **Writes are still the VM's own calls.** A console that stops answering a write or a size query
  stops the VM, until those calls move to the I/O threads
  ([asynchronous underneath](#asynchronous-underneath-synchronous-on-top)).
- **Randomness is the kernel's.** On the fake kernel it is seeded from the host for a person's
  run, and fixed for a test's, so a test repeats. On the machine it is the kernel's own.

### beamlet on Redoubt

Status: built · partly tested: files, programs, `/net` and the system natives are not built · tested: bench:beamlet-boot, bench:beamlet-console, bench:beamlet-heap-flood, bench:beamlet-budget-flood, bench:userland-boot, bench:userland-bad-start, bench:userland-read-only, host:beamlet-redoubt::the_index_is_sorted_one_line_per_module_and_a_malformed_line_is_refused_whole, host:beamlet-redoubt::a_module_loads_only_if_its_object_hashes_to_its_entry, host:beamlet-redoubt::an_application_resource_is_checked_as_a_module_is, host:beamlet-redoubt::verified_module_lookup_propagates_found_absent_and_refused, host:beamlet-redoubt::verified_application_lookup_propagates_found_absent_and_refused

On Redoubt, beamlet is a native program. Its built `Platform` adapter uses the client library
([native programs](native.md#the-client-library)) for the console and verified code lookup,
and the kernel's calls for the clock and randomness. The file, network and program adapters
remain planned.

| Method | On Redoubt |
| --- | --- |
| `monotonic_us`, `idle` | the kernel's `time_now` (microseconds since boot); `idle` is a `receive` with a timeout ([timer](../kernel/timer.md)) |
| `system_time_us` | `None` until wall-clock time and time sync exist, in M5 (persist, install, share); the VM then counts system time from the Unix epoch at boot, so the logger and anything else that stamps a time works and a date says 1970. A check that a date has begun (a certificate's `notBefore`) then fails, a check only that one has not passed (a token's expiry) passes, and times from two boots cannot be ordered |
| `console_write`, `console_read` | the client library's `console`: writes and reads on the `/dev/cons` connection; a read with nothing to read is parked by the server, so input arrives as a completion and `Eof` means the connection ended ([consoled](../servers/consoled.md)) |
| `console_size` | a fresh `consol` `size` call on every query, never cached; a server that does not serve it refuses the call and the answer is `None` |
| `random` | the kernel's `random` call |
| `load_module`, `load_app` | looks the requested file (`Elixir.Enum.beam`, `elixir.app`) up in `system.index`, read from `/boot` (served by `bootfsd`) and parsed strictly at start. A missing name is `Absent` without an object read. For an indexed name, it reads `/<sha256 hex>` whole from the userland disk's `fsd` (`fsd:system`): matching bytes are `Found`; a missing, short, long or hash-mismatched object is `Refused`, with one console diagnostic and no other source tried ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)). From M5 (persist, install, share), the principal's profile joins the lookup ([packages](packages.md)), never the session's writable namespace. This decides which module a name finds, not what code may run |
| `files` | the client library's `file`: walk, open, read, write, stat, clunk on the namespace's connections ([files](files.md)) |
| `programs` | the client library's `launch`: native programs in carved budgets ([native programs](native.md)) |

TCP is Plan 9's `/net`, served by `ipd`: `gen_tcp` works unchanged over a backend that opens
`/net/tcp/clone` and reads and writes the data file, and framing stays in Erlang, so the Rust side
only moves bytes ([ipd](../servers/ipd.md)). The size is asked afresh because the only console
whose size changes is an SSH channel, and a cached size would answer a redraw with the size from
before the change; a caller that wants to be told of a change uses the parked `resize` call
([the shell](shell.md)).

On the machine, beamlet is the program `beamlet`, started like any other with a console, a
budget, a connection to `bootfsd` and one to the userland disk's `fsd`, each a named handle
(`bootfsd`, `fsd:system`). It runs one scheduler thread until several harts
([several harts](../plan/m2-usable-shell.md#several-harts)), and starts its threads with the
runtime's `thread::spawn`.

If the module it is told to start cannot load, it says why on its console and waits without
exiting: a tampered disk must not become a restart loop that reboots the machine.

A confined boot runs no labelled beamlet: beamlet reads `system.index` through `bootfsd`, one
instance a labelled domain may not share with the unlabelled ones
([R34 (confined placement)](../servers/init.md#r34-confined-placement)).

The timer's counter frequency is not needed: `time_now`'s microseconds serve the clock and
`idle`'s deadlines (bench:beamlet-console).

### Natives

Status: planned · M1 (separation and containment)

A native is one of three kinds, and no other:
- **OTP's own**, reimplemented in Rust where BEAM has C (`crypto`, `re`, `zlib`, the file and
  buffer primitives): the API is OTP's, so the differential test holds each to the real BEAM
  ([what runs on it](#what-runs-on-it)).
- **The system's:** what has no POSIX equivalent, the small fixed set below.
- **A primitive the interpreter is too slow for**, and no more than the primitive: the screen
  buffer ([screen natives](#screen-natives)).

Anything else is Elixir. Every native's own code holds no `unsafe`; it bounds the work one call
does, since a native is not preempted by reductions; it reaches no panic from any argument; and it
is attacked with hostile arguments, fuzzed where it parses. OTP's own natives parse what OTP's do
(certificates, compressed streams, patterns), held to BEAM by the differential test; a native of
the other two kinds never parses a large untrusted format, which goes to a native program in a
budget of its own instead ([native programs](native.md)).

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
| `launch/1` | launching a native program: the image, budget, namespace, handles and arguments come from the Elixir caller, and the client library's `launch` makes the calls and writes the startup block |

Handles are resource terms: unforgeable, collected, and never serialisable. A copy of a handle
inside the VM is the same connection (one badge, one client), so passing one to another Erlang
process is sharing it. It cannot reach another VM in a message: the term format writes a resource
as a plain reference, with no state behind it, so a decoded copy grants nothing, and a handle
crosses between processes only in a kernel call that names it. Delegation is
always `new_connection`, a typed call on a connection, not a native ([sessions](sessions.md)).

The namespace and launching natives are the client library's `ns` and `launch`, so policy stays
in Elixir and encoding in Rust. The file server's operations that name fids (`Redoubt.File`'s
`rename`, `copy_file` and attributes) go through the library's `fsd` beside the file natives,
since the fids are the platform's, not Elixir's ([native programs](native.md#the-client-library)).

The Elixir modules over them are of two layers. A server's typed calls are generated from its
wire table, one function per message ([wire](../servers/wire.md#generated-clients)), so the
binding cannot drift from the server. Above the natives and the generated calls, a thin
hand-written module gives what is idiomatic and adds no authority: `Redoubt.Namespace`,
`Redoubt.Budget` and `Redoubt.Process` over the natives, `Redoubt.Keys` over `keyd`'s calls.

The Elixir modules live on the userland disk, one object per module, bound to the bundle by
`system.index` ([R75](../kernel/boot.md#r75-verified-userland)); only what the VM needs before it
can read the disk is embedded in it: its own console server, code and kernel modules
(`beamlet_io`, `beamlet_code`, `beamlet_kernel`, `beamlet_port`, `beamlet_tcp`) and its
stand-ins for `application`, `gen_tcp` and `ram_file`
([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs), `EMBEDDED`).

**Open:** none.

### Screen natives

Status: planned · M2 (usable shell)

The screen buffer is `beamlet-screen`, a crate of the VM's with no `unsafe` and no dependency but
the VM and the `cells` crate. Its natives, in the Erlang module `redoubt_screen`, are primitives
over cells; widgets, layout and focus are Elixir ([the shell](shell.md#full-screen-programs)).

| Native | What it does |
| --- | --- |
| `new(W, H)` | a buffer: a resource, every cell blank, owned by the calling Erlang process |
| `resize(B, W, H)` | a new size, blank; the next `diff` clears the screen and sends it all |
| `put(B, X, Y, Text, Style)` | writes `Text` along the row from `(X, Y)`, one grapheme a cell and a wide one two, clipped at the edge (a wide grapheme cut by it becomes a space, and a grapheme longer than a cell's symbol may be, U+FFFD); returns the columns written |
| `fill(B, Rect, Symbol, Style)` | one symbol and style over a rectangle, clipped |
| `plot(B, Rect, Bits, Style)` | a bitmap of 2×4 dots a cell, drawn in Braille (U+2800 to U+28FF) |
| `width(Text)` | the columns `Text`, of at most 64 KiB, takes, by the table `put` uses |
| `diff(B)` | the cells changed since the last `diff`, as a `cells` frame, which then becomes what is shown |

- **A control character is refused, not drawn.** `put` and `fill` raise `badarg` on one (the
  ASCII and 8-bit controls, DEL, and the bidirectional embedding, override and isolate controls:
  the `cells` crate's own rule), so a caller that forgot to make text visible fails loudly, and
  nothing reaches the encoder that the cell protocol would not carry.
- **Bounded.** A buffer is at most 1024 cells on a side and 65,536 cells in all; `put` reads no
  more of its text than the row has cells, and `width/1` takes at most 64 KiB; no native does
  more than one pass over a buffer. So every call has a ceiling on its time and its allocation,
  and it is charged in reductions by the cells and bytes it touched.
- **Counted.** A process holds at most four buffers, and each counts toward its owner's heap
  limit, so a loop of `new/2` is ended by the limit that ends any runaway allocation.
- **One writer.** A buffer answers only the Erlang process that made it; any other gets `badarg`.
  It sits behind a lock only because a resource may move between schedulers.
- **One width table**, generated from one pinned Unicode version and held to vectors: `width/1`
  is `put`'s own, and the terminal library measures with it, so what is measured is what is drawn.
- **The diff speaks the cell protocol**, so a screen drawn in the session and a native program's
  frames reach the encoder by one decoder ([the shell](shell.md#the-terminal-library)).

**Open:** none.

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

A call that waits holds its thread for as long as it waits, and a process has at most 255 threads
([processes](../kernel/processes.md)). Most calls are short, but some wait on a person or a peer
for as long as nothing happens: the console read, the parked `resize`, a read on a TCP
connection's data file, a `serve` loop, a job's exit notice. A read on `/dev/cons` and a read on a
file are the same 9P message, so what makes a call a waiting one is where it goes, never its
message: the console, a `/net` connection's data file, a served endpoint and an exit notice wait;
every other call is short.

So the threads are split. Two run the schedulers. Four take short calls, which never queue behind
a waiting one, and each short call has a timeout, so a hung server costs its caller an error, not
the session its short calls. Three are reserved for the session's own waiting calls (the console
read, the parked `resize`, and the loop that serves the session's pipes), so no user code can take
them. The rest, 246, take user code's waiting calls, one each. A waiting call past that is
refused (`system_limit`), not queued, so a session with too many open sockets learns it at once.
A job's exit notice takes its thread when the job is launched, before `process_create`, and a
launch with none free is refused, so no job ever runs with nobody waiting to reap it. Each
server's admission limits are separate: a call a server has not admitted yet waits its turn there
([the serving library](../servers/serving.md)).

**Open:** whether one thread should one day wait on several endpoints at once, which is kernel
work ([IPC](../kernel/ipc.md)).

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

**Natives are few, and of three kinds.** A native is code every Erlang process in the session can
reach, the ones handling hostile data among them, and it is not preempted. So what may be one is
closed: OTP's own natives, which the differential test holds to BEAM; the system's, a fixed set
with Elixir above it; and a primitive an interpreter cannot do fast enough, kept to the primitive.
Nothing else is.

**Its dependencies are userland's.** beamlet's own crates hold no `unsafe`; the crates it links
(RustCrypto, `regex-automata`, `miniz_oxide`, `num-bigint`) do, for speed and for the platform.
That is the latitude an application has and privileged code does not
([the tenets](../TENETS.md#5-dependencies-are-part-of-the-trusted-computing-base)): the VM is
per-principal code, and a bug in it reaches that principal's own capabilities, which the kernel
contains ([trust tiers](../servers/README.md#trust-tiers)). What is shared with the servers is
built from the same vendored bytes, and the rest is vendored too, one version of each
([vendored dependencies](../testbench.md#vendored-dependencies)); only what builds or tests on
the host comes from crates.io, pinned by the lockfile.

**One 9P client, not a method per service.** Files, TCP and the console are all 9P on Redoubt, so
one generic client in Rust covers them, and the framing (packet modes, line mode) stays in Erlang
where OTP already has it. Adding a service means serving a tree, not growing the trait.
