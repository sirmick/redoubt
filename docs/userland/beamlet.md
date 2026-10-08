# beamlet, the Elixir VM

beamlet is a BEAM interpreter in safe Rust: it runs Erlang and Elixir code compiled by the
standard compiler, OTP 28 with Elixir 1.20. Every session and every agent on Redoubt is one
beamlet VM. The VM gets from its embedder, through one Rust trait (`Platform`), exactly the
services it is granted (a clock, a console, random bytes, code, files, programs) and nothing
else, and it treats every `.beam` file, literal and message as hostile input. beamlet runs on
the host and boots the shell on Redoubt with verified modules from the userland disk, and its
files are 9P files in its namespace. Native launching on Redoubt remains planned.

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

<details><summary>Status: built · partly tested: in a boot, only the process heap limit and the budget's backstop are attacked · tested (17)</summary>

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
- host:beamlet-vm::the_footprint_is_reported_at_the_first_wait_for_input
- host:beamlet-vm::held_bytes_follow_the_runtime_heap
- bench:beamlet-footprint

</details>

One VM is one trust domain, but a buggy or hostile Erlang process must not take the rest of the
VM down. Every limit fails closed: the offender ends, and nothing is lost silently
([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs)).
- **Mailbox** (`max_mailbox`, 2^20 messages): a message that would overflow a mailbox kills the
  receiver with `{system_limit, message_queue}`, untrappably. Dropping it would break protocols
  silently, like a TCP stream with a hole in it.
- **Process memory** (`max_heap_words`, 2^27 words, and `max_heap_size`, which a process can only
  lower): checked at the end of each slice; a process over it is collected first and killed only
  if what is live is still over. A resource whose native declares its size, a
  [screen buffer](#screen-natives), counts that size as its holder's own memory, toward
  `max_heap_size` as heap words do. Only a heap counts it, not a queued message or an ETS table
  holding it: the screen buffer's limit of four a process and the mailbox's limit bound those,
  and any other sized resource needs a bound of its own.
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
limit ([R6 (charging)](../kernel/budgets.md#r6-charging)). The image also caps `beamlet`'s heap
([init](../servers/init.md#the-boot-manifest)), but at the budget's edge, where the budget binds
first: that cap is there for the bench's measurement. On Redoubt the platform lowers
`max_heap_words` and `max_ets_words` to a sixteenth of the VM's budget each, which it takes from its
required argument `budget_pages=N`, the budget's pages
([todo](../todo/beamlet-budget-from-startup.md)). Each counts the VM's own 8-byte words, as the VM
counts a process (two for a 16-byte term), so a limit is the same bytes on rv32 as on rv64. A flooding process peaks at about four times its
heap limit, the old heap, the collector's copy and its growth, so the budget must be at least twice
what the VM uses with no Erlang process running; then one flooding process, or the tables, meets its
limit while the VM still has pages. Several flooding at once, or a native's single large allocation,
reach the backstop instead, which ends the VM, and `init` restarts it. It is a server like any other
under `init`'s restart rule: a VM that cannot stay up (a start module that fails every time, a
manifest without `budget_pages`) is restarted until the limit, and then the machine reboots
([init](../servers/init.md#restarts-and-reboots)). CPU between VMs is the kernel's to share, by
budget weight ([scheduling](../kernel/scheduling.md)).

#### What the VM holds at its prompt

Started with the argument `report_memory`, which the image never passes, the VM prints what it
holds the first time it waits for console input with nothing else to run, for the shell at its
prompt, one console line a row ([`memory.rs`](../../userland/otp/vm/src/memory.rs)). It is the
VM's own count: each allocation as the runtime's heap holds it (a power-of-two block up to 2 KiB,
whole pages above), every waiting process collected first; B-tree nodes are not counted.
`beamlet` adds its runtime heap's pages held at that moment and its record's peak, and the bench's
scan of that record ([the memory budget](../testbench.md#the-memory-budget)) is the independent
total, read once the case has typed one command. In pages, each row rounded to whole pages on its
own:

| What | rv64 | rv32 |
| --- | ---: | ---: |
| Decoded code: instructions | 523 | 523 |
| Decoded code: operands | 2,118 | 2,118 |
| Literals: each module's | 36 | 36 |
| Literals: the shared table | 684 | 681 |
| Module tables | 227 | 198 |
| Atoms | 94 | 66 |
| Processes (23): heaps, collected | 56 | 56 |
| Processes: the rest | 121 | 116 |
| ETS and binaries | 1 | 1 |
| Accounted | 3,864 | 3,799 |
| Runtime heap at the prompt: held, peak | 4,086, 4,482 | 3,916, 4,319 |
| Not accounted (held less accounted) | 222 | 117 |
| The scan's peak, after one command | 5,430 | 5,251 |

A module's instructions are 8-byte entries over one array of its operands, 16 bytes each on either
width, a list operand's items in the same array, so decoded code is the same size on both widths
and still about two thirds of the count. The five largest modules are `unicode_util`, `erl_parse`,
`Elixir.Enum`, `Elixir.Kernel` and `string`. What is not accounted is free small blocks, B-tree
nodes and the platform's buffers; the peak above what is held is the boot pack, which is held
until the prompt, and a load's transient, and the scan's peak is higher than the prompt's because
the first command loads more modules.

- **Loaded code keeps no spare room.** The loader shrinks a module's instructions and its operand
  array to their counts once decoded, where the decoding's doubling left up to twice as many.
- **A literal chunk keeps no spare room.** A module's constants are shrunk to their count before
  they join the shared table (about 350 pages on either width).
- **Loading stays eager, but for the shell's commands.** The shell's start loads what its prompt
  needs. The largest modules are the ones evaluating any line needs, so loading them on first call
  would move their pages to the first command, not save them. A session calls few of its
  commands, so a command's module is loaded when the command is first called
  ([the shell](shell.md#commands)), and the boot pack holds none of them.

The image budgets the VM twice the largest peak the scan finds across its memory cases, in
`beamlet-footprint`, the one case that scans a shell's VM now that the steward starts the others'
shells, and with that budget, 11,904 pages, the single VM boots in 512 MiB
([budgets](../kernel/budgets.md#the-tree-from-the-boot-manifest)). The budget was set from a peak of
5,904 pages on rv64, when every module of the shell's was loaded at its start, the commands'
among them; with the commands loaded when called the peak is 5,430, and twice it would be 10,880
(residual: the budget is not lowered yet). The line editor under the shell's driver is loaded at
the prompt: OTP's `group`, `edlin`, `edlin_key`, `group_history`, `prim_tty`, `shell`,
`gen_statem`, `sys` and `kernel`, with `Redoubt.Term` and the driver. The shell's protocols are not
consolidated, but nothing at the prompt, nor a plain line, dispatches a protocol on a struct, so
Elixir's `Protocol` and `Enumerable` load only for a line that does.

Residual: QEMU's default 256 MiB is out of reach. Its `system` budget leaves the VM about 2,700
pages beside the other servers, and the VM holds more than that at its prompt (the table above),
most of it decoded code and the shared literal table. Fitting needs code kept in its on-disk form or
loaded a function at a time.

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
| `console_write`, `console_read`, `console_size`, `prompt_drawn` | the `user` I/O device; input never blocks; the shell's driver says when its first prompt is drawn | no input; size unknown; nothing |
| `console_listening` | told whether a process reads the console; with none, input held is no reason for `idle` to return | holds nothing |
| `random` | random bytes from a cryptographic source; on failure the VM raises rather than use a weaker source | required |
| `load_module`, `load_app`, `module_file` | a system `.beam` or `.app` lookup answers found bytes, an absent name or a refused object; `module_file` names a loaded module | applications absent |
| `files` | a file system, as `prim_file` sees it | none: `file` calls fail with `enotsup` |
| `programs` | starting programs behind ports | none: `open_port` fails with `eacces` |
| `system` | the system's own calls: the namespace, calls and serving, budgets, labels, launching ([natives](#natives)) | none: each `redoubt` native answers `{error, not_supported}` |

- **`load_module` is a lookup, not a gate.** It asks the platform for a module before examining
  the code path. `Found` uses those bytes, so no directory shadows a system module
  ([packages](packages.md#profiles-and-upgrades)); `Absent`
  searches the code path's directories in order, including those added with `code:add_patha/1`;
  `Refused` stops without touching the code path
  ([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs), `locate_module`). On Redoubt
  a name the userland volume's `erofsd` answers `not_found` to is absent, while any other refusal at
  the open or on the read (`corrupt`, a short or long file, a device error) is refused, with one
  console line naming the file and the error's name
  ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)). `load_app`
  carries the same three outcomes: `beamlet:app_spec/1` makes one platform attempt and returns
  the bytes on `Found`, or `error` on `Absent` and `Refused`. Residual:
  the bundle's protocols are not consolidated when it is built, so a protocol consolidated in a
  session's own directory is not used and protocol dispatch stays the slower, unconsolidated kind;
  behaviour is the same. Consolidating them when the bundle is built is not done, at the cost of
  that speed and of Elixir's `Protocol` module once a line dispatches on a struct: a `defimpl`
  typed at the prompt for a protocol already consolidated would be ignored. Code in the VM can also load any bytes it holds with
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

<details><summary>Status: built · partly tested: beamlet boots the shell on Redoubt; the differential suites against the real BEAM need OTP 28 and Elixir installed and are not run by the bench, and linear-time matching, crypto's refusal without randomness, the cofactored Ed25519 check and the bound on a zlib stream are not attacked by a named test · tested (16)</summary>

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
- bench:userland-read-only

</details>

Where beamlet implements something, it behaves as the real BEAM does, and the differential suite
checks it: each test runs on BEAM and on beamlet and the printed results must be identical.
- **OTP and Elixir unchanged.** OTP's `stdlib`, `logger`, `file`, `ssl` (TLS 1.2 and 1.3) and
  `ssh` run unmodified, and so do Elixir's standard library, its compiler, OTP's Erlang compiler
  and IEx, whose transcript matches BEAM's. Every live OTP 28 opcode is implemented except
  `on_load`.
- **erts's Erlang where it is code.** Of BEAM's preloaded modules, those whose Erlang is
  useful load from erts like any other: `erlang`, `erts_internal`, `persistent_term`, `atomics`
  and `counters`, their NIF stubs answered by natives, and `prim_eval`, whose `receive` is BEAM
  assembly: it runs a `receive` that `erl_eval` evaluates, such as one typed at the shell's
  prompt. The rest stand on BEAM's C runtime (the boot process, ports, sockets, tracing) and
  never load; a call to one is `undef` unless a native answers.
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

<details><summary>Status: built · partly tested: host only; a change of the terminal's size is not delivered · tested (8)</summary>

- host:beamlet::the_terminal_is_raw_while_the_vm_reads_it_and_restored_at_a_normal_end
- host:beamlet::the_terminal_is_restored_when_the_run_ends_in_an_exception
- host:beamlet::the_terminal_is_restored_when_the_vm_halts
- host:beamlet::the_terminal_is_restored_when_a_signal_ends_beamlet
- host:beamlet::a_panic_restores_the_terminal
- host:beamlet-vm::a_second_console_subscription_is_refused_and_the_first_reader_keeps_the_console
- host:beamlet-vm::the_console_is_free_once_its_reader_has_exited
- host:beamlet-vm::the_console_size_is_the_platforms_answer_asked_at_each_call

</details>

On a host, beamlet's command line puts the terminal in raw mode from the moment a process of the
VM first reads the console, so that every byte typed reaches the VM as it is (Ctrl+C included),
and restores it on every exit: a result, an exception, a halt, a signal, a panic included
([`userland/otp/cli/src/tty.rs`](../../userland/otp/cli/src/tty.rs)). Output processing stays
on, so a line written past the shell's encoder still lands where a line does on a host; the
encoder ends its lines with CR LF itself, as it must on Redoubt. `console_size` is the
terminal's size, read afresh at each call. A change of size is not delivered; the shell reads the
size at each prompt
([the shell](shell.md#paste-scrolling-a-plainer-terminal-and-the-consoles-size)).
Console input goes to one Erlang process, the shell's driver, which takes it with
`beamlet:console_subscribe/0`; a second subscription is refused, so no code run at the prompt
can take the keyboard, or the interrupt key with it, from the driver.

### The console, the clock and randomness

<details><summary>Status: built · partly tested: its tests run on the host, on the fake kernel, against a console server that keeps `consoled`'s protocol with a host terminal for its device; it runs in a boot in bench:beamlet-boot and bench:beamlet-console · tested (13)</summary>

- host:beamlet-redoubt::writes_reach_the_screen
- host:beamlet-redoubt::a_long_write_reaches_the_screen_whole_and_in_order
- host:beamlet-redoubt::a_write_answered_busy_goes_again_after_the_retry_interval
- host:beamlet-redoubt::the_console_is_one_hub_connection_with_one_waiter
- host:beamlet-redoubt::typing_reaches_the_vm_then_its_end
- host:beamlet-redoubt::a_read_waits_for_typing_without_holding_the_vm
- host:beamlet-redoubt::an_end_of_input_already_waiting_ends_the_idle_that_takes_it
- host:beamlet-redoubt::a_console_without_consol_has_no_size
- host:beamlet-redoubt::idling_with_a_deadline_returns_by_it
- host:beamlet-redoubt::after_the_console_ends_idling_still_waits_for_its_deadline
- host:beamlet-redoubt::there_is_no_wall_clock
- host:beamlet-redoubt::input_nobody_reads_holds_no_idle_and_waits_for_the_next_reader
- host:beamlet-vm::the_platform_hears_the_console_reader_come_and_go

</details>

The first part of beamlet's platform on Redoubt, `beamlet-redoubt`
([`userland/otp/redoubt`](../../userland/otp/redoubt/src/lib.rs)), serves the console, the clock
and randomness over the client library and the runtime's calls. `/dev/cons` is opened once, and
its reads and writes go through the VM's hub
([asynchronous underneath](#asynchronous-underneath-synchronous-on-top)): one read is out at a
time, held by the server until there is typing, and one write, a page at most, with what the VM
wrote after it waiting in order behind it, since the console's share is a page a badge. So
neither a read that waits nor a slow console holds the VM's thread, until 64 KiB wait unwritten,
when a write waits in place as one to a slow terminal does. When the VM ends, what it wrote
reaches the console before its last line, waiting at most 2 s. After the console's end, `idle`
still sleeps until its deadline. When the process reading the console exits, the VM says so
(`console_listening`) and no read goes out until another process reads it: typing waits at the
console, what came in before is held for the next reader, and `idle` does not return for it, so a
VM with nobody at the console sleeps rather than spins. There is no wall clock, so
`system_time_us` is `None`.
`./shell --fake` runs the shell on it.
- **The size is still the VM's own call.** It is a typed call, which no hub carries, so a console
  that stops answering a size query stops the VM.
- **Randomness is the kernel's.** On the fake kernel it is seeded from the host for a person's
  run, and fixed for a test's, so a test repeats. On the machine it is the kernel's own.

### beamlet on Redoubt

Status: built · partly tested: programs and `/net` are not built · tested: bench:beamlet-natives, bench:beamlet-boot, bench:beamlet-console, bench:beamlet-files, bench:boot-profile, bench:boot-profile-unverified, bench:pack-outside-module, bench:pack-bad-truncated, bench:pack-bad-wrong-length, bench:pack-bad-wrong-name, bench:beamlet-heap-flood, bench:beamlet-budget-flood, bench:userland-boot, bench:userland-bad-start, bench:userland-read-only, bench:verity-flipped-tree, bench:verity-wrong-root, host:beamlet-redoubt::a_module_is_its_file_and_a_failed_read_is_refused, host:beamlet-redoubt::not_found_at_the_open_is_absent_and_every_other_error_is_refused_by_name, host:beamlet-redoubt::verified_module_lookup_propagates_found_absent_and_refused, host:beamlet-redoubt::verified_application_lookup_propagates_found_absent_and_refused, host:beamlet-redoubt::a_packed_module_comes_from_the_pack_and_any_other_from_the_volume, host:beamlet-redoubt::a_pack_with_a_bad_entry_is_refused_whole, host:testbench::the_boot_pack_is_deterministic_sorted_and_only_of_the_objects

On Redoubt, beamlet is a native program. Its built `Platform` adapter uses the client library
([native programs](native.md#the-client-library)) for the console, files, verified code lookup
and the system natives, and the kernel's calls for the clock and randomness. The network and
program adapters remain planned.

| Method | On Redoubt |
| --- | --- |
| `monotonic_us`, `idle` | the kernel's `time_now` (microseconds since boot); `idle` is a `receive` with a timeout ([timer](../kernel/timer.md)) |
| `system_time_us` | `None` until wall-clock time and time sync exist, in M6 (persist, install, share); the VM then counts system time from the Unix epoch at boot, so the logger and anything else that stamps a time works and a date says 1970. A check that a date has begun (a certificate's `notBefore`) then fails, a check only that one has not passed (a token's expiry) passes, and times from two boots cannot be ordered |
| `console_write`, `console_read` | the client library's `console`: writes and reads on the `/dev/cons` connection; a read with nothing to read is parked by the server, so input arrives as a completion and `Eof` means the connection ended ([consoled](../servers/consoled.md)) |
| `console_size` | a fresh `consol` `size` call on every query, never cached; a server that does not serve it refuses the call and the answer is `None` |
| `prompt_drawn` | nothing, but in a `boot-stats` build the line `beamlet: first prompt drawn [t=N]`, the end of the span the boot profile times |
| `random` | the kernel's `random` call |
| `load_module`, `load_app` | takes the requested file from the boot pack, if the pack holds it (below); otherwise reads the requested file (`Elixir.Enum.beam`, `elixir.app`) whole from the root of the verified userland volume, through its `erofsd` (`erofsd:system`), which reads it through its `verityd`; a reader of the volume trusts that `erofsd` and `verityd` ([R76 (verified volumes)](../servers/verityd.md#r76-verified-volumes)) in place of checking each object itself. A name `erofsd` answers `not_found` to at the open is `Absent`; any other refusal at the open or on the read is `Refused`, with one console diagnostic naming the file and the error's name and no other source tried; the bytes read whole are `Found` ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)). From M6 (persist, install, share), the principal's profile joins the lookup ([packages](packages.md)), never the session's writable namespace. This decides which module a name finds, not what code may run |
| `files` | 9P on the namespace's connections through the VM's hub: walk, open, create, read, write, stat, remove and clunk, and `littlefsd`'s `rename` ([files](files.md#files-over-9p)) |
| `programs` | the client library's `launch`: native programs in carved budgets ([native programs](native.md)); planned, as ports. Launching is the `launch/1` native's |
| `system` | the namespace's files and named handles, typed calls on a pool of threads, served endpoints on a thread each, budgets from the named handle `budget`, the kernel's label stamp, and `launch` ([natives](#natives)) |

TCP is Plan 9's `/net`, served by `ipd`: `gen_tcp` works unchanged over a backend that opens
`/net/tcp/clone` and reads and writes the data file, and framing stays in Erlang, so the Rust side
only moves bytes ([ipd](../servers/ipd.md)). The size is asked afresh because the only console
whose size changes is an SSH channel, and a cached size would answer a redraw with the size from
before the change; a caller that wants to be told of a change uses the parked `resize` call
([the shell](shell.md)).

On the machine, beamlet is the program `beamlet`, started like any other with a console, a
budget, and a connection to the userland disk's `erofsd`, a named handle its argument
`endpoint=` names (`erofsd:system`). It runs one scheduler thread until several harts
([several harts](../plan/m2-usable-shell.md#several-harts)), and its waiter threads are the
runtime's `thread::spawn`.
- **`bind=PREFIX=HANDLE`** puts a named handle beamlet was handed at a prefix of its namespace
  (`bind=/home/alice=walfsd:data`): the `bind/2` a session performs for itself
  ([namespaces](sessions.md#namespaces)), for a VM `init` launches alone, whose namespace holds
  only `/dev/cons`. In the steward's sessions the session's namespace does this. It creates no authority:
  the handle was handed already, and a bind naming a handle it was not handed, or a prefix that is
  not a clean absolute path, is refused before any server is asked anything, with one line on its
  console.
- **`report_io`** has the platform say, when the VM ends, how many requests went through the hub
  and how many threads it ran (`beamlet: io: N requests through the hub; threads: 1 scheduler, W
  waiters`); bench:beamlet-files reads it.

In a `boot-stats` build ([checked builds](../testbench.md#checked-builds)) beamlet says `beamlet:
first console read [t=N]` at the VM's first console read, with `time_now` in µs, and `beamlet:
first prompt drawn [t=N]` when the shell's driver has drawn its first prompt
(`beamlet:prompt_drawn/0`): the driver takes the console before it draws its banner and prompt,
so the second line is the boot's time to its prompt, where a user sees the box ready, and the span
the target is on; both lines are drawn beside the prompt. `boot-profile` and
`boot-profile-unverified` measure it.
Measured in that build under `icount` (`shift=3`, sleep on) with seed 1, in guest time
(bench:boot-profile, bench:boot-profile-unverified):

| system volume | rv64 verified | rv64 unverified | rv32 verified | rv32 unverified |
| --- | ---: | ---: | ---: | ---: |
| littlefs (`littlefsd`, retired for this volume) | 1,016.7 s | 534.7 s | 1,044.2 s | 558.0 s |
| EROFS (`erofsd`) | 16.0 s | 12.5 s | 15.7 s | 12.1 s |
| EROFS, with the boot pack | 12.4 s | 9.9 s | 12.0 s | 9.6 s |
| EROFS, with the boot pack, the shell the steward's console session, to the driver's first read | 15.4 s | 13.2 s | 17.6 s | 15.3 s |
| the same, 16-page lends for the public entry's push and the session's stream, to the first prompt drawn | 14.7 s | 12.2 s | 16.3 s | 13.7 s |
| the same, the shell's commands loaded when first called, a boot pack of 78 entries | 13.1 s | 10.9 s | 14.7 s | 12.4 s |

On littlefs 99 % of the boot was in the VM's 96 lookups: `littlefsd` found each file's name in the
volume's root directory again two or three times for every 9P operation, 77,710 block reads of 673
distinct blocks ([littlefsd](../servers/littlefsd.md#residual-risks)). On EROFS the same 96 loads
make the same 652 9P operations, and `erofsd` reads the volume 641 times (112 inodes, 243
directory blocks, 286 runs of a file's blocks) in 642 calls to `blkd` of 3.35 MB in whole sectors;
the loads take 7.1 s verified and 3.7 s unverified, and the VM's own work and its console about
6.5 s either way.
With the boot pack (below) the 96 lookups become one read of the pack, 115 9P reads of 16 KiB:
the boot makes 208 9P operations, and `erofsd` reads the volume 208 times (16 inodes, 35 directory
blocks, 157 runs of a file's blocks) in 209 calls to `blkd` of 2.46 MB. (That build counted each
call `erofsd` made to `blkd`, and the whole sectors it carried; `erofsd`'s `boot-stats` now
counts each read it asks of its range, whatever calls the range client splits it into, and the
bytes it asks for.) Reading the pack takes 3.6 s verified and 1.2 s unverified, and the VM's work
after it, decoding the modules as they are called, 8.0 s; the verified prompt is the same across
seeds 1 to 5. On littlefs the prompt with the boot pack is at 152 s verified, since `littlefsd`
finds the file again for each read.

Since the steward starts the shell as the console principal's session
([the steward](../servers/steward.md#authentication-and-sessions)), the verified first prompt on
rv64 comes at 14.7 s: `init` has started its servers by 0.6 s, and pushes beamlet's 4 MB public
entry to `bootfsd` until 1.9 s, when it starts the steward; from 1.9 to 5.8 s the steward carves
the session's budget, streams its image from `bootfsd` into the new process in the client
library's launch batches of 64 pages ([native programs](native.md#the-client-library)), and the VM
reads its boot pack; from 5.8 to 11.8 s the shell's application starts, until its driver takes
the console; from 11.8 to 14.7 s the shell starts under the driver, to its banner and first
prompt. Unverified the same points are at 1.8, 3.6, 9.5 and 12.2 s. On rv32 the steward starts
at 2.3 s, the boot pack is read by 6.6 s, the driver reads at 13.2 s and the prompt comes at
16.3 s; unverified at 2.2, 4.4, 10.9 and 13.7 s. The push and the stream are calls of the same
cost whatever they carry, about 2.5 ms each in this build: before `init` and the steward lent 16
pages a call (one page of a public entry per `add`, a 9P read of 8 KiB), they were 1,024 and about
512 calls, and the steward started at 3.1 s and the shell's driver read at 13.7 s on rv64 (4.0
and 15.7 s on rv32). The `add` carries 32 KiB, a power of two, so `bootfsd`'s entry doubles onto
4 MiB and its heap's peak stays half its cap. Since the shell's commands are loaded when first
called, its start loads fewer modules: the driver reads at 11.3 s and the first prompt comes at
13.1 s on rv64 (10.9 s unverified), and at 12.7 and 14.7 s on rv32 (12.4 s unverified).

**The boot-time target:** in this build, the prompt within 20 s of guest time, verified and
unverified, on both widths: the slowest measured prompt plus a tenth, rounded up to 5 s.
`boot-profile` and `boot-profile-unverified` fail past it, and run in every whole run of the bench.

Before its VM starts, beamlet reads the volume's boot pack, `boot.pack`, whole: one open, one `stat`
for its length, one allocation of that length charged to the VM, and reads of 16 KiB in order, the
lend a module's file is read through ([`pack.rs`](../../userland/otp/redoubt/src/pack.rs)). The
image's builder makes the boot pack from the modules and resources the shell loads to its prompt,
which `image/userland.toml` names, their bytes copied from the files beside it: a magic and a
version word, the entry count, an index of each entry's name, offset and length sorted by name, then
the entries' bytes back to back. The same files and names give the same pack, byte for byte. beamlet
checks the boot pack whole before using any of it: the magic and the version, each name a module's
or a resource's file name in strictly ascending order, the entries' bytes back to back from the
index's end to the file's, and each module's first atom, its own name, the one its entry names. A
lookup takes a name the boot pack holds from it, a copy of its bytes that the loader decodes and
drops as it decodes a file's, and asks the volume for any other name, as above, so the order is the
boot pack, then the volume's file, then the code path. Once every entry has been taken, at the
prompt, the boot pack's allocation goes; a module it holds that is never called keeps it, at most
the pack's size. A boot pack the volume does not hold is said once on the console and the lookups go
to the files; one that cannot be read, or does not check, is said once, naming the boot pack and
why, and beamlet waits as below. The boot pack is per VM and read-only: no sharing of its pages
between VMs, no byte cache in the steward, no decoded snapshot, no pack per profile. It is a file of
the verified volume ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)), never
written at run time.

If the volume does not attach, because `erofsd` serves it as corrupt, the boot pack cannot be read
or does not check, or the module it is told to start cannot load, it says why on its console and
waits without exiting: a tampered disk must not become a restart loop that reboots the machine.

Each label set that runs beamlet reads its own attachment of the userland image through its own
`blkd`, `verityd` and `erofsd`, all carrying that set
([R34 (confined placement)](../servers/init.md#r34-confined-placement)).

The timer's counter frequency is not needed: `time_now`'s microseconds serve the clock and
`idle`'s deadlines (bench:beamlet-console).

### Natives

<details><summary>Status: built · partly tested: the machine's cases run the VM under a tester in the steward's place, but for budgets and launching, which also run in the steward's own sessions over SSH, a vault session's among them · tested (32)</summary>

- bench:steward-vault-launch
- bench:beamlet-natives
- bench:beamlet-serve
- bench:beamlet-launch
- bench:beamlet-natives-attack
- host:beamlet-vm::a_lookup_gives_the_prefixs_connection_and_the_rest_and_refuses_by_name
- host:beamlet-vm::a_bind_names_a_connection_and_refuses_anything_else
- host:beamlet-vm::the_table_lists_path_name_and_handle
- host:beamlet-vm::a_calls_reply_arrives_as_a_message_with_its_handles_as_resources
- host:beamlet-vm::a_send_is_one_way
- host:beamlet-vm::budgets_are_carved_read_and_destroyed
- host:beamlet-vm::labels_are_fixed
- host:beamlet-vm::a_launch_takes_everything_from_its_caller_and_its_end_arrives_as_a_message
- host:beamlet-vm::requests_arrive_with_badge_account_and_labels_and_are_answered
- host:beamlet-vm::a_decoded_handle_grants_nothing
- host:beamlet-vm::arguments_of_the_wrong_type_are_badarg_and_reach_no_platform_call
- host:beamlet-vm::lists_past_their_caps_are_refused
- host:beamlet-vm::a_handle_of_the_wrong_kind_is_refused_by_name
- host:beamlet-vm::a_dropped_handle_is_closed_when_its_holder_is_collected
- host:beamlet-vm::without_system_calls_every_native_is_not_supported
- host:beamlet-redoubt::the_labels_are_the_kernels_stamp_on_the_vms_own_send
- host:beamlet-redoubt::a_lookup_gives_the_longest_prefix_and_the_rest_and_refuses_by_name
- host:beamlet-redoubt::a_bind_is_the_files_namespace_and_one_connection
- host:beamlet-redoubt::a_budget_is_no_connection_and_no_endpoint
- host:beamlet-redoubt::a_bind_to_a_server_that_never_answers_is_refused_within_its_bound
- host:beamlet-redoubt::a_typed_call_goes_out_on_a_pool_thread_and_its_reply_is_an_event
- host:beamlet-redoubt::requests_arrive_with_badge_account_and_labels_and_an_answer_reaches_the_caller
- host:beamlet-redoubt::a_request_never_answered_is_answered_by_the_serve_thread_at_its_deadline
- host:beamlet-redoubt::a_request_already_waiting_ends_the_idle_that_takes_it
- host:beamlet-redoubt::an_endpoint_served_stays_open_when_its_term_is_dropped
- host:beamlet-redoubt::a_launch_takes_what_it_is_given_and_its_end_is_an_event
- host:beamlet-redoubt::a_labelled_sessions_child_takes_its_labels_and_runs

</details>

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

They are the module `redoubt`'s, over the platform's `System`
([`userland/otp/vm/src/platform.rs`](../../userland/otp/vm/src/platform.rs); the argument checks in
[`bif/system.rs`](../../userland/otp/vm/src/bif/system.rs), the calls in
[`userland/otp/redoubt/src/system.rs`](../../userland/otp/redoubt/src/system.rs)). A platform
with no `System`, the host CLI's, answers each `{error, not_supported}`; the host's
`fake-redoubt` runs Redoubt's, on the fake kernel. Each native, as Erlang sees it:
- **`ns_lookup(Path)`** is `{ok, Handle, Rest}`: the connection the longest matching prefix of a
  clean absolute path names and the rest below it, or, for a name with no `/`, the named handle the
  VM was started with (`keyd`, `budget`) and `<<>>`. A path that is not clean (`..`, `//`) is
  `bad_name`, one nothing is bound at `not_found`. **`bind(Prefix, Connection)`** puts a connection
  at a clean absolute prefix of the files' own namespace: a path under an existing prefix shadows
  it for longer matches only. A handle that is not a 9P connection is attached once, then and
  there, each of the attach's two calls waiting at most `ATTACH_US` on the VM's thread; one whose
  server does not answer or does not speak 9P, or a budget, is `not_a_connection`.
  **`ns()`** is `[{Path, Name | nil, Handle}]`, the namespace's entries in binding order, then the
  named handles.
- **`call(Handle, {Words, Buffer, Handles}, TimeoutMs)`** is `{ok, Ref}`, and the reply arrives as
  `{reply, Ref, {ok, {Words, Buffer, Handles}} | {error, Name}}`. A buffer is a binary for a message
  that lends one and `nil` for an inline one; the reply's buffer is the bytes its word 1 says at
  the front of the lend. No hub carries a typed call, and no scheduler waits for one: a pool of
  `CALL_THREADS` threads makes them, `MAX_QUEUED` more wait, and past that a call is `busy`; each
  is bounded by its timeout, at most `MAX_CALL_MS`. The caller waits in `receive`, as for any
  message. **`send(Handle, {Words, Buffer, Handles})`** is one way, waiting at most
  `SEND_TIMEOUT_US` for its receiver.
  The natives encode nothing of a protocol: the generated clients do
  ([wire](../servers/wire.md#generated-clients)).
- **`serve(Endpoint)`** serves a receive right the VM holds, on a thread of its own, which keeps
  the endpoint open and serves it for the VM's life (at most `MAX_SERVED` endpoints; one served
  already, by the same handle, is `already_served`; a second handle to the same receive right is
  not told apart): each call is admitted by the serving library's admission, per (account,
  label set) with a share per badge
  ([R26 (admission fairness)](../servers/serving.md#r26-admission-fairness)), and parked with the
  library's deadline, `REQUEST_WAIT_US`
  ([R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting)); it reaches the
  serving process as `{request, Request, Badge, Account, Labels, {Words, Buffer, Handles}}`, `Request`
  `nil` for a one-way send. **`reply(Request, {Words, Buffer, Handles})`** answers it once. A request
  the Erlang side never answers is ended by that thread at its deadline and its admission given
  back: the VM holds nothing for it. These are calls, not 9P, so the multiplexed connections' rule
  ([R77 (multiplexed requests)](../servers/serving.md#r77-multiplexed-requests)) is not this one's.
- **`budget_create(#{pages, processes, weight, labels, account, deadline})`** carves a child from
  the VM's own budget, the named handle `budget` (`no_budget` without one); `labels`, `account`
  and `deadline` may be left out, and a deadline, in the clock's microseconds, makes it a lease
  ([deadlines](../kernel/budgets.md#deadlines)). Labels left out are the VM's own, the set
  `labels()` reads: a session is `user`-class, and the kernel gives a `user`-class caller's child
  exactly its parent's labels, so in a labelled session that default is what lets it carve, and
  launch, at all. Labels given go to the kernel as they are: fewer than the session's are
  `label_denied`, more `class_denied` ([labels on budgets](../kernel/budgets.md#labels-on-budgets)). **`budget_destroy(Budget)`** destroys it and
  everything in it; **`budget_usage(Budget)`** is `{ok, #{pages => {Limit, Used}, processes =>
  {Limit, Used}, weight => {Limit, Carved}}}`. A handle of another kind is `wrong_object` before
  the kernel is asked.
- **`labels()`** is the VM's label set, read at its start off the first call thread's first
  wake-up, which the kernel stamps with the sender's labels as it stamps every message: the
  kernel's word, not a launcher's, and fixed.
- **`launch(#{image, budget, namespace, handles, args, stack_pages, heap_pages})`** is `{ok, Job}`,
  and the job's end arrives as `{exit, Job, Cause, Code}` (`exited`, `faulted` or `killed`):
  the client library's `launch` with the image the caller read, a budget it carved, its namespace
  entries and named handles (at most `MAX_START_HANDLES` together), and arguments; the platform
  adds the loader stub, which it carries as `init` does, and the job's own exit endpoint
  ([native programs](native.md#launching-from-a-session)). At most `MAX_JOBS` run at once, each
  watched by a thread that waits for its exit notice; a job no thread takes to watch is killed,
  its budget destroyed, since nothing would hear its end.

Every refusal is a Redoubt name: a term of the wrong type is `badarg`, as for any native, and a
well-formed request the platform refuses is `{error, Name}`, the kernel's error by its name in the
ABI (`wrong_object`, `label_denied`), a 9P error by the one table's
([wire](../servers/wire.md#error-names)), or the platform's own (`not_found`, `not_a_connection`,
`no_budget`, `busy`). What waits on the VM's thread is bounded and named: a `send/2`, at most
`SEND_TIMEOUT_US`; a bind of a handle not yet a connection, its attach's two calls at most
`ATTACH_US` each; a hand-off to a thread of the platform's, at most `HAND_US`; and the kernel's
own calls for budgets and launching, which answer at once. Each native bounds its work: no list is
walked past its cap, and no binary is copied past its own:

| Bound | Value | Where |
| --- | --- | --- |
| `MAX_MESSAGE_HANDLES`, `MAX_LABELS` | 4 handles a message, 16 labels a spec, and a message's 4 words | [`bif/system.rs`](../../userland/otp/vm/src/bif/system.rs) |
| `MAX_START_HANDLES`, `MAX_ARGS` | 128 namespace entries and named handles together, 128 arguments | [`bif/system.rs`](../../userland/otp/vm/src/bif/system.rs) |
| `MAX_BUFFER`, `MAX_IMAGE`, `MAX_NAME` | 64 KiB a message's buffer, 8 MiB an image, 4 KiB a path, name or argument | [`bif/system.rs`](../../userland/otp/vm/src/bif/system.rs) |
| `MAX_CALL_MS` | 5 s, the longest a call waits | [`bif/system.rs`](../../userland/otp/vm/src/bif/system.rs) |
| `CALL_THREADS`, `MAX_QUEUED` | 2 calls out at once, 64 waiting | [`pool.rs`](../../userland/otp/redoubt/src/pool.rs) |
| `SEND_TIMEOUT_US`, `ATTACH_US`, `HAND_US` | 1 ms a send waits for its receiver, 1 s each of a bind's attach calls, 1 s a thread of the platform's to take its work (one that does not has ended, and is set aside) | [`system.rs`](../../userland/otp/redoubt/src/system.rs) |
| `MAX_SERVED`, `REQUEST_WAIT_US` | 2 endpoints served, 5 s a request waits for its answer | [`serve.rs`](../../userland/otp/redoubt/src/serve.rs) |
| `MAX_JOBS` | 4 jobs running at once | [`jobs.rs`](../../userland/otp/redoubt/src/jobs.rs) |

Handles are resource terms: unforgeable, collected, and never serialisable. A copy of a handle
inside the VM is the same connection (one badge, one client), so passing one to another Erlang
process is sharing it. A handle no process holds is closed at the next collection; a budget's is
closed and the budget lives on, since destruction is a call. It cannot reach another VM in a
message: the term format writes a resource as a plain reference, with no state behind it, so a
decoded copy grants nothing, and a handle crosses between processes only in a kernel call that
names it. Delegation is always `new_connection`, a typed call on a connection, not a native
([sessions](sessions.md)). Tested: `a_decoded_handle_grants_nothing`,
`a_dropped_handle_is_closed_when_its_holder_is_collected`,
`arguments_of_the_wrong_type_are_badarg_and_reach_no_platform_call`,
`lists_past_their_caps_are_refused` and `a_handle_of_the_wrong_kind_is_refused_by_name` on the host,
and `beamlet-natives-attack` on the machine, where a VM offers every native a handle another VM
wrote out, which it reads back as a plain reference, and asks the kernel for a child budget that
adds a label (`class_denied`: a `user`-class caller may not); a VM in a labelled session runs
nothing there, since the console carries no labels and its write-open of `/dev/cons` is refused
([consoled](../servers/consoled.md)). Over SSH a vault session's channel carries its labels, and
there `steward-vault-launch` carves with labels left out, launches a program the vault's channel
shows, and has fewer and more labels refused, in the steward's own session
([sshd](../servers/sshd.md#sessions-over-ssh)). The cases' tester carves each session as the steward does
([steward](../servers/steward.md#fixed-sub-budgets-per-label-set)): the principal's account, a
sub-budget per label set, the session from it; what it hands beyond a session's own is said in
each case.

Residuals:
- A request the Erlang side never answers is ended at its deadline with `malformed` (status 1):
  no status every protocol shares means a timeout, so its caller cannot tell the two apart.
- A typed call whose caller exits is not cancelled: it holds its pool thread until its reply or
  its timeout, and the reply, with any handles it brought, is dropped and closed.

The namespace and launching natives are the client library's `ns` and `launch`, so policy stays
in Elixir and encoding in Rust. The file server's operations that name fids (`Redoubt.File`'s
`rename`, `copy_file` and attributes) go through the library's `littlefsd` beside the file natives,
since the fids are the platform's, not Elixir's ([native programs](native.md#the-client-library)).

The Elixir modules over them are of two layers. A server's typed calls are generated from its
wire table, one function per message ([wire](../servers/wire.md#generated-clients)), so the
binding cannot drift from the server. Above the natives and the generated calls, a thin
hand-written module gives what is idiomatic and adds no authority: `Redoubt.Namespace`,
`Redoubt.Budget` and `Redoubt.Process` over the natives, `Redoubt.Keys` over `keyd`'s calls
([`userland/shell/lib/redoubt`](../../userland/shell/lib/redoubt)).

The Elixir modules live on the userland disk, one file per module, on a volume whose root the
signed manifest pins ([R75](../kernel/boot.md#r75-verified-userland)); only what the VM needs
before it can read the disk is embedded in it: its own console server, code and kernel modules
(`beamlet_io`, `beamlet_code`, `beamlet_kernel`, `beamlet_port`, `beamlet_tcp`) and its
stand-ins for `application`, `gen_tcp` and `ram_file`
([`userland/otp/vm/src/vm.rs`](../../userland/otp/vm/src/vm.rs), `EMBEDDED`). The `application`
stand-in runs no kernel application; when the first application with a callback module starts,
it starts the kernel's servers that other code calls, as BEAM starts them at boot:
`erl_signal_server`, `global_name_server`, and `kernel_safe_sup`, which OTP's `group` waits for
before it serves a line. With no C library to ask a character's width, `prim_tty:wcwidth/1`
answers `{error, enotsup}` and OTP measures with its own table, which agrees with BEAM's libc on
ASCII and wide East Asian characters and parts from it on a combining mark: libc gives it no
column, the table one.

### Screen natives

<details><summary>Status: built · partly tested: host only; no boot draws a screen · tested (16)</summary>

- host:beamlet-screen::a_buffer_is_at_most_1024_a_side_and_65536_cells
- host:beamlet-screen::the_first_frame_clears_and_sends_what_is_not_blank
- host:beamlet-screen::a_frame_sends_only_what_changed
- host:beamlet-screen::a_resize_is_blank_and_its_frame_clears_and_sends_it_all
- host:beamlet-screen::put_reads_no_more_graphemes_than_the_row_has_cells
- host:beamlet-screen::a_wide_grapheme_takes_two_cells_and_the_edge_cutting_one_leaves_a_space
- host:beamlet-screen::overwriting_half_of_a_wide_grapheme_leaves_a_space_in_the_other
- host:beamlet-screen::a_control_character_is_refused_and_nothing_of_the_call_is_written
- host:beamlet-screen::widths_are_otps
- host:beamlet-screen::a_control_character_is_badarg_and_nothing_is_drawn
- host:beamlet-screen::only_the_process_that_made_a_buffer_draws_into_it
- host:beamlet-screen::a_process_holds_at_most_four_buffers
- host:beamlet-screen::a_buffer_nothing_holds_is_given_back
- host:beamlet-screen::a_large_buffer_ends_its_owner_at_its_heap_limit
- host:beamlet-screen::a_buffer_resized_past_the_heap_limit_ends_its_owner
- host:beamlet-vm::sized_resources_past_a_processs_own_heap_limit_end_it

</details>

The screen buffer is `beamlet-screen`
([`userland/otp/screen`](../../userland/otp/screen/src/lib.rs)), a crate of the VM's with no
`unsafe` and no dependency but the VM and the `cells` crate. Its natives, in the Erlang module
`redoubt_screen`, are primitives over cells; widgets, layout and focus are Elixir
([the shell](shell.md#full-screen-programs)).

| Native | What it does |
| --- | --- |
| `new(W, H)` | a buffer: a resource, every cell blank, owned by the calling Erlang process |
| `resize(B, W, H)` | a new size, blank; the next `diff` clears the screen and sends it all |
| `put(B, X, Y, Graphemes, Style)` | writes `Graphemes`, a list of binaries, along the row from `(X, Y)`, one a cell and a wide one two, clipped at the edge (a wide grapheme cut by it becomes a space, and a grapheme longer than a cell's symbol may be, U+FFFD); returns the columns written |
| `fill(B, Rect, Symbol, Style)` | one symbol, one code point of one column, and one style over a rectangle, clipped |
| `plot(B, Rect, Dots, Style)` | a bitmap, one byte a cell holding its 2×4 dots in Braille's own order, drawn as U+2800 plus the byte |
| `diff(B)` | the cells changed since the last `diff`, as a `cells` frame, which then becomes what is shown |

A `Style` is `{Fg, Bg, Modifiers}`, each colour `reset`, `{indexed, I}` or `{rgb, R, G, B}` and the
modifiers the cell protocol's bits; a `Rect` is `{X, Y, W, H}`.
- **A control character is refused, not drawn.** `put` and `fill` raise `badarg` on one (the
  ASCII and 8-bit controls, DEL, and the bidirectional embedding, override and isolate controls:
  the `cells` crate's own rule) and write nothing of that call, so a caller that forgot to make
  text visible fails loudly, and nothing reaches the encoder that the cell protocol would not
  carry.
- **Bounded.** A buffer is at most 1024 cells on a side and 65,536 cells in all (larger is
  `system_limit`); `put` reads no more of its graphemes than the row has cells; no native does
  more than one pass over a buffer. So every call has a ceiling on its time and its allocation,
  and it is charged in reductions by the cells it touched.
- **Counted.** A process holds at most four buffers (a fifth is `system_limit`; one nothing holds
  any more is given back), counted in its process dictionary, and each buffer declares the bytes
  of its two grids, which count as its holder's own memory, toward its own heap limit
  ([limits](#limits-inside-one-vm)), from `new` and from `resize`. So a loop of `new/2` is ended
  by the limit that ends any runaway allocation. Code that erases the dictionary's count starts a
  new one; the heap limit still holds.
- **One writer.** A buffer answers only the Erlang process that made it; any other gets `badarg`.
  It sits behind a lock only because a resource may move between schedulers.
- **One segmentation and one width table, OTP's.** Text arrives split into graphemes by OTP's own
  segmentation (`String.graphemes/1`), and a grapheme's columns come from a table generated from
  OTP's `unicode_util:is_wide/1` (`tools/gen-width.escript`, which fails the build's check when the
  table is not current): wide if a presentation selector follows its first code point or any of its
  code points is wide. The shell measures with the same `is_wide/1`, so what is measured is what
  the buffer lays out, at the pinned OTP's Unicode version. The buffer does not segment text itself,
  so `fill` takes a single code point, and a caller that hands `put` two characters as one
  grapheme draws them in one cell, misplacing what follows, and nothing more.
- **The diff speaks the cell protocol**, so a screen drawn in the session and a native program's
  frames reach the encoder by one decoder
  ([the shell](shell.md#the-terminal-library)).

### Asynchronous underneath, synchronous on top

<details><summary>Status: built · tested (9)</summary>

- host:beamlet-redoubt::the_console_is_one_hub_connection_with_one_waiter
- host:beamlet-redoubt::a_read_waits_for_typing_without_holding_the_vm
- host:beamlet-redoubt::a_vm_busy_past_the_session_bound_keeps_its_console
- host:beamlet-redoubt::two_askers_wait_at_once_and_each_gets_its_own_answer
- host:beamlet-vm::a_completion_reaches_the_process_that_asked_and_no_other
- host:beamlet-vm::a_message_does_not_end_a_wait_for_a_file
- host:beamlet-vm::a_process_killed_while_it_waits_has_its_operation_dropped
- bench:beamlet-console
- bench:beamlet-files

</details>

The VM is one trust domain on a few scheduler threads, so a blocking call would stop every Erlang
process on that thread. So the VM's I/O is asynchronous underneath: a request is submitted and
the VM keeps running, its answer comes back later to whoever asked, and `Platform::idle` returns
on a timer deadline or an answer. The kernel has no queued sends (a message occupies its sender's
thread until taken: [IPC](../kernel/ipc.md)), so on Redoubt the asynchrony is the client
library's hub ([native programs](native.md#many-requests-at-once)) over the servers' multiplexed
connections ([the serving library](../servers/serving.md#multiplexed-connections)),
in `beamlet-redoubt` ([`userland/otp/redoubt/src/io.rs`](../../userland/otp/redoubt/src/io.rs)):
- **One hub per VM**, owned by the platform and run by whichever scheduler holds it. A request is
  sent from the scheduler's own thread, waiting at most 1 ms for its server to take it, and its
  buffer is the hub's until its answer.
- **One waiter thread per connection**, started the first time the VM uses the connection, blocked
  in its completion call; it hands each batch of answers to the VM's own endpoint with one send.
- **`idle` is a `receive`** on that endpoint until the next timer deadline, and the platform hands
  each answer to whoever asked: the console's to its reader, a file's to the Erlang process whose
  call it was. An answer, request, input or the input's end that arrived before the VM idled
  returns the idle at once: the `receive` is entered only with nothing to hand over, so nothing
  taken waits for the next wake-up to be looked at.

Above that, Elixir is ordinary synchronous code: `File.read/1` blocks the calling Erlang process,
not the scheduler. OTP's `prim_file` runs unchanged: a file native whose operation the platform
has begun answers it later (`Files` names the asking process), the process waits, and only that
operation's end (or an exit signal) wakes it, not a message; the native is then called again and
takes the result. Several requests of one operation (a walk, an open, the reads up to a count) go
out as each answer comes. A process that dies while it waits has its operation dropped with what
it holds, and an operation that opened a file closes it. The VM's own code loading reads files
waiting in place. Concurrency is bounded by each server's shares of requests and pages per
connection ([R77 (multiplexed requests)](../servers/serving.md#r77-multiplexed-requests)); over
them, a request is answered `busy` and asked again `RETRY_US` (10 ms) later, a console write and
a file operation's request alike; the answers are counted, and said with the I/O report.

**Threads.** A process has at most 255 threads ([processes](../kernel/processes.md)). The VM's are
its schedulers (one until several harts) and its waiters, one per connection it uses, at most 6:
a session's bindings (`bootfsd`, the home volume, a labelled volume, `ipd`, the console, the system
volume), a bind being one of them again. No call holds a thread while it waits: a console read,
a file read and a read on a `/net` connection's data file are each one request on the hub, so
what a person or a peer takes to answer costs nothing but the request. Every 9P server in the
image serves multiplexed sessions, so no call needs a thread of its own. What the scheduler's
thread still waits for itself: typed calls, which no hub carries (the console's `size` and
`littlefsd`'s `rename`), and the system volume's module lookups, since loading code is
synchronous in the VM; a server that stops answering one of them stops the VM. A scheduler away
from its endpoint that long, past a server's session bound, keeps its sessions: each waiter holds
what arrives meanwhile and keeps calling ([a busy caller](native.md#many-requests-at-once)). The
serving natives and a job's exit notice are [launching's](#natives) (planned).

Residual: each connection costs its waiter thread, since a thread waits on one endpoint at a time;
one thread waiting on several at once would be kernel work ([IPC](../kernel/ipc.md)).

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
