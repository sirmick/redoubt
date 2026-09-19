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

Non-goals: speed, a JIT, NIFs or ports (foreign code lives in separate OS processes), distribution
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
| `term.rs`, `atom.rs` | Values, their order and their `~w` printing; the atom table. |
| `loader.rs`, `etf.rs`, `module.rs` | `.beam` parsing and validation; the external term format. |
| `vm.rs` | Module registry, process table, scheduler, exit signals, timers. |
| `interp.rs` | The instruction loop, one `match` arm per opcode. |
| `bif/` | Native functions, all listed in one table in `bif/mod.rs`. |
| `bits.rs`, `float.rs` | Bitstring building and reading; float formatting. |

`cli/` is the POSIX embedding: `beamlet [-pa DIR]... MODULE [FUNCTION]`.

## Terms
Terms are a Rust `enum`; compound terms are reference-counted (`Rc`). Erlang terms are immutable
and acyclic, so reference counting frees exactly the garbage, with no tracing collector and no
per-process heap. Costs we accept:
- Slower than BEAM's bump allocation and copying collector.
- Messages share structure between processes instead of being copied. That is invisible to Erlang
  code (terms are immutable) but it means per-process memory accounting has to count shared data;
  see Open questions.
- Long lists are dropped iteratively (`impl Drop for Cons`) so dropping cannot overflow the stack.

Integers are `i64` and move to `BigInt` (`num-bigint`) only on overflow, and back when they fit, so
each integer has one representation. Bignums are capped at 2^24 bits (`system_limit` beyond).

Maps are persistent AVL trees (`pmap.rs`, about 200 lines) ordered by the exact term order.
Versions share nodes, so `maps:put` on a map someone else still holds copies O(log n) nodes,
not the whole map (a plain `BTreeMap` made building a map quadratic: 13x slower on a 1000-key
fold). BEAM's iteration order for atom keys depends on atom-table indices and is unspecified;
ours is always term order. The differential harness prints maps with `~kw` (ordered) so
outputs compare, and tests must not depend on raw iteration order.

Nothing recurses on the Rust stack over a term's depth: dropping (`drop_flat`), comparing and
printing use explicit work lists. A million-level nested tuple, list or map is fine.

A match context (`Term::Match`) is internal: it exists only between `bs_start_match*` and the end
of a binary match, as in BEAM.

## Processes and scheduling
One VM runs on one thread. Processes live in a slot table; a pid carries a serial number so a
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
- `beamlet_io`: the I/O protocol server behind `io:format` and `IO.puts`. Started at boot as
  `user` and `standard_error`; the group leader of every process. Output goes through
  `Platform::console_write`. No input yet.
- `logger` and `error_logger`: stand-ins for the kernel's. Level filtering, and reports printed
  to `standard_error`, formatted by their own `report_cb` as OTP does. No handlers.

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
  atoms 2^20, atom length 255, processes 2^16, mailbox 2^16 messages, stack 2^20 Y registers,
  bignums 2^24 bits, binaries 2^30 bits (a per-VM `Limits` setting), tuples from `make_tuple` 2^24, ETF nesting 256.

## Testing
- **Unit tests** (`cargo test`): formats and parsers, with vectors taken from the real BEAM.
- **Differential tests** (`tools/difftest`): each `tests/<suite>/*.erl` exports `start/0`; it runs
  on the real BEAM (`tools/expect.escript`) and on beamlet, and the printed results must be
  identical. The real OTP `stdlib` `.beam` files are on beamlet's code path, so every test also
  exercises the loader and interpreter on OTP's own code.
- **Hostile input** (`vm/tests/hostile.rs`): every truncation of real `.beam` files is
  rejected; mutation fuzzing (20k rounds by default, 1M soaked) must never panic or hang the VM.
  It found four bugs, each now a regression test.
- **Corpora**: `tests/erlang` (ours), `tests/elixir` (Enum, String, structs, GenServer, Agent,
  Task, Supervisor, IO, exceptions), `tests/atomvm` (AtomVM's 491 modules; `SKIP` lists those
  that need ports, NIFs or BEAM heap sizes, with reasons).

## Opcode census
`tools/census.escript` lists the instructions and imports a set of `.beam` files use. On OTP 28:
126 opcodes are live (not deprecated); the Elixir standard library uses about 100 of them. The
loader refuses deprecated opcodes, and the interpreter implements all live ones except `on_load`.

## Performance
Not a goal, but measured so nothing is gratuitously slow. On one core: `fib(27)` 0.12 s, a
200k-element `lists:map`/`filter`/`sum` 0.13 s, a 200k-insert map fold 0.25 s, sorting 200k
integers 0.48 s. BEAM's JIT is roughly 10-30x faster.

## Open questions
- Mailbox overflow currently drops messages silently. Kill the receiver instead?
- Per-process memory limits with shared (reference-counted) terms.
- Local funs cannot be serialized (`term_to_binary`); `erlang:phash2/1,2` is missing.
- Console input for the I/O server.
- Name.
