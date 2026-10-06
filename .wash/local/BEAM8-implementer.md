# BEAM8: the VM's code is compact: one flat operand array per module, an 8-byte instruction

Tier A (the VM's code representation: loader, interpreter, memory accounting), size M. Needs
BEAM6 (merged): start from `main`. Found by BEAM6's breakdown: decoded code was 7,180 pages on
rv64 at the shell's prompt, 82 % of what the VM accounts for, and after BEAM6's shrinks it is
still the footprint's largest part. Run everything natively on this host under the job pool's
rules.

## Context rules (read these first)

- Read `.wash/local/BEAM6-report.md` (the table, "Levers" item 3), `docs/userland/beamlet.md`
  "What the VM holds at its prompt" (the page's table you will move) and "Limits inside one
  VM"; then the code by type and function: `module.rs` (`Arg`, `Instr`, `Module`, `NATIVE_BODY`),
  `interp.rs`'s `arg(ins, i)` and every other `.args` site (count them at the start with
  `grep -n '\.args' userland/otp/vm/src/*.rs`; the report says the loader, the native-body
  patch and `on_load_entry` are the others), `loader.rs`'s instruction decode, `memory.rs`'s
  `code.instrs`/`code.operands` rows. `vm.rs` and `interp.rs` are long: by function only.
- The bench's `.ram` dump is never read; the scan prints the lines.
- Reports under 1,900 bytes, detail in `.wash/local/BEAM8-report.md`.

## The design (BEAM6's measured proposal, adopted)

Today `Instr { op: u8, args: Vec<Arg> }` is 32 bytes on rv64 with one heap `Vec` per
instruction (242,345 instructions, 521,551 operands of 24 bytes, the power-of-two classes
adding 46 % on top). The compact form:
- **One flat operand array per module**, `Module.operands: Box<[Arg]>` (or `Vec` shrunk once,
  at the end of the load), operands in instruction order.
- **`Instr` = `op: u8`, `count: u8` (or u16 if any opcode needs it: check `genop.tab`'s maximum
  arity, and say), `start: u32`**: 8 bytes. `code: Box<[Instr]>`.
- **`Arg` is 16 bytes**: `Arg::List(Vec<Arg>)` becomes a boxed or an indexed form (a `start,
  len` into the same flat array, or `Box<[Arg]>`): choose the one that keeps `Arg` at 16 bytes on
  rv64 and 16 on rv32 with `Const(Term)` inside, and pin both with `size_of` asserts in a host
  test (`Instr` 8, `Arg` 16, both widths; the rv32 figure as measured).
- **One accessor**: the interpreter keeps reading operands through `arg(ins, i)`, now
  `module.arg(ins, i)` or an `Operands<'_>` view; no second path. The loader writes the flat
  array; the native-body patch (`NATIVE_BODY`, `body_natives`) and `on_load_entry` go through
  the same view or a single `Module` method, never by poking `args` of one instruction.
- Expected: about 2,510 pages of code either width at the prompt (instructions 473 +
  operands 2,037 on rv64), saving about 4,670 on rv64 and 1,165 on rv32 against BEAM6's
  measured table. The report gives the measured before and after from `beamlet-footprint`.

## What stays the same

- The `.beam` loader's input and its validation: no format change, no new chunk, the same
  faults for the same bad inputs (`loader.rs`'s refusals and their tests).
- Eager loading, the literal table, the atom table, the process heaps: untouched (BEAM6 ruled on
  them; lazy loading is not here).
- The interpreter's semantics: `interp.rs`'s dispatch is unchanged except for how an operand is
  fetched. Every host test of `beamlet-vm` and `beamlet-redoubt` passes unchanged.

## Checkpoint (one, before the loader is rewritten)

When `Instr` and `Arg` have their new layout, the accessor is in place and the crate compiles
(the loader may still fill the flat array naively), send one progress line with: `size_of` on
both widths, the count of `.args` sites you replaced, the maximum operand count an opcode
needs, and the form you chose for `Arg::List`. Then rewrite the loader's decode.

## Measurement and the manifest

- `tests/beamlet-footprint.toml` (BEAM6's) is the measure, both widths: its table's
  `code.instrs` and `code.operands` rows keep their names and now report the flat forms (the
  operands row is the one array; a boxed `List` is counted in it, or in a row of its own, say
  which).
- The manifest's budget follows BEAM6's rule, unchanged: twice the largest heap peak across the
  memory cases (`init-boot`, `userland-boot`, `userland-read-only`, `beamlet-footprint`), six
  runs each, plus the stack, rounded up to 128; `heap_pages` the most the budget holds beside
  the stack. `image/manifest.json`, `servers/init/tests/manifest.rs`'s fit sum,
  `tests/size-budget.toml` move with it; the RAM stays 512 MiB (the next step down is out of
  reach: below). The limits cases (`beamlet-heap-flood`, `beamlet-budget-flood`) run at the new
  `budget_pages`, since the process and ETS limits are a sixteenth of it.

## Pages (with the code)

- `beamlet.md` "What the VM holds at its prompt": the table's code rows and totals replaced by
  the new measurement on both widths; one sentence on the representation (a module's
  instructions are 8-byte entries over one operand array). The subsection stays built, no Open
  line. Its Residual on 256 MiB is restated with the new arithmetic: what the prompt holds now,
  what a 256 MiB machine's share is (about 2,700 pages), and that the remaining gap is the
  code's size as decoded plus the shared literal table, so code kept in its on-disk form or
  loaded per function is what a further step would need. That is the design question this
  package leaves, stated as a residual, not a promise.
- `testbench.md`'s memory table rows for `beamlet` and the paragraph after it (the peak, the
  cap, the margins) with the new numbers; `budgets.md`'s RAM paragraph and `image/README.md`
  if a number they state moves. No dates, package IDs or review history.

## Owned paths

`userland/otp/vm/src/{module,interp,loader,memory,opcodes}.rs` and `userland/otp/vm/tests`;
`userland/otp/redoubt` only where the native-body patch or `on_load_entry` lives;
`image/manifest.json` (the `beamlet` entry), `servers/init/tests/manifest.rs` (the fit sum),
`tests/size-budget.toml` (the VM's ceiling, set per commit at the fold with the delta in the
report); the pages named. **Not yours:** `libs/rt`, the kernel, `servers/init` beyond the
test's sum, the packer and the `.beam` format, the shell, the bench's scan, BEAM3's I/O
threads, BEAM4's natives.

## The short gate

Both builds (rv64, rv32); host tests of `beamlet-vm` and `beamlet-redoubt` (whole suites);
the docs checker, `cargo fmt --check`, the size budget, the `unsafe` ratchet unchanged, the
no-cruft gate; own cases on both widths at 512 MiB: `beamlet-footprint`, `beamlet-boot`,
`beamlet-console`, `beamlet-heap-flood`, `beamlet-budget-flood`, `beamlet-lookup-host`,
`beamlet-lookup-cli-host`, `userland-read-only`, `verity-flipped-tree`, `verity-wrong-root`;
and the smoke set (`userland-boot`, `init-boot`, `bench-net-peer`, `ipc-outcomes`). The whole
bench is the train's.

## Not here

A change to the `.beam` loader's format or chunks; lazy or per-function loading; code kept in
its on-disk form (the residual's question); the literal table; the allocator; a JIT or
threaded dispatch; anything in the interpreter beyond operand fetch.
