# BEAM6: the shell's VM on a diet: its idle footprint measured, then cut until the image fits 512 MiB

Tier A (the VM, the shell's start, the image), size M: about half measurement, the rest the
levers the measurement justifies. Needs BEAM2 and MEM2, both merged: start from `main`. The
owner's words: "1gb for now then we'll need to send beam on diet". Run everything natively on
this host under the job pool's rules.

## Context rules (read these first)

- Read `docs/userland/beamlet.md` "Limits inside one VM" and "beamlet on Redoubt",
  `docs/testbench.md` "The memory budget", `docs/kernel/budgets.md` "The tree from the boot
  manifest" (the RAM paragraph), `image/README.md`'s manifest bullet, and `libs/rt/src/heap.rs`'s
  header comment (28 lines: how pages are held and returned). Read the VM by function:
  `vm.rs` is 1,700 lines; `loader.rs`, `module.rs`'s `Module`, `process.rs`'s heap and
  `collect`, `memory.rs`.
- The bench's `.ram` dump is the guest's RAM: never read it; the scan prints the lines.
- Reports under 1,900 bytes, detail in `.wash/local/BEAM6-report.md`.

## Where the pages are today

At the shell's prompt the VM's budget holds 11,877 pages on rv64 and 7,554 on rv32 (BEAM2's
`pages_usage` samples), and its runtime heap's peak is 11,814 and 6,966 (the memory scan,
testbench.md's table). The heap is 99.5 % of the footprint; image, stacks and page tables are
about 60 pages. So the measurement is of the heap, and the levers are the VM's.

The disk holds 104 modules, 1.0 MiB, for the prompt's closure (113 loaded with the embedded
ones). BEAM itself holds a shell prompt in a few MiB. The suspect is the decoded code:
`Module.code` is a `Vec<Instr>` of structs with operand terms, where BEAM keeps compact
bytecode; `literals`, `strings`, `attributes` and `compile_info` ride beside it. This is a
hypothesis, named so the measurement can refute it, not a finding.

## Step 1: measure (both widths)

A breakdown the VM prints on its console, in one line per row, when started with an argument
(`report_memory`, or your name; absent = silent; the bench's case sets it, the image does
not), at the first prompt: loaded code (per `Module`: `code.len() × size_of::<Instr>()`,
literals, strings, attributes, compile_info; a total and the five largest modules), the atom
table, the shared literal table, process heaps and stacks (count of processes, sum of
`heap.len()` in bytes, the sum of what is live after a collection), ETS, off-heap binaries,
the loader's transient copies (`Lookup::Found(Vec<u8>)`: confirm it is dropped after `load`
and that no module keeps its file bytes), and the runtime heap: pages held (its `Record`
peak and `held`) against the bytes the VM accounts for; the difference is the allocator's
free-but-held pages (small-class pages are never returned; freed large blocks are unmapped).
Print `size_of::<Instr>()`, `size_of::<Term>()` on each width.

The same line on the host first (`userland/otp` host run of the shell), where iteration is
cheap and the VM's accounting is the same; the runtime heap row exists only on Redoubt. Then
a bench case `beamlet-footprint`, both widths, `memory = true`, that boots the image's
manifest with the argument added and expects the breakdown lines; its verdict on the total is
the scan's `heap beamlet PEAK of CAP pages` line (independent of the VM's own report), the
breakdown is evidence. The table goes in the report and on `beamlet.md` (below).

**Checkpoint 1,** after the table: a message to the orchestrator for the Architect, the table
and the levers ranked by pages saved per change, before any lever is coded.

## Step 2: levers, each measured before and after, kept if it pays

1. **Load lazily.** `Redoubt.Shell.start` runs `Application.ensure_all_started` and the
   commandlet registry's and `Param`'s `Code.ensure_loaded?` pull the closure in at start;
   on-demand loading already exists (`Platform::load_module` on an undefined call). Load the
   prompt's modules on first call; count what the prompt then holds. The shell keeps working
   on a corrupt volume as `userland-boot` attacks it: a module that fails to load on first
   call says so and the prompt stays.
2. **Keep no second copy.** Drop or lazily re-read `attributes` and `compile_info`
   (`module_info/1` needs the first); confirm Docs and debug chunks are already stripped by the
   packer and that the loader's file buffer dies at the end of `load`.
3. **Compact code.** Only if the table says decoded code is a third or more of the heap: shrink
   `Instr` (narrow operand encodings, indices into per-module tables, boxed rare operands) or
   keep a compact form decoded per instruction. If the table says this alone meets the target
   and it is more than a week's change, stop at checkpoint 1 and propose it as its own package.
4. **Smaller heaps, a collector that shrinks.** `MIN_HEAP_CELLS` (1,024 cells, 8 KiB a process
   on rv64) and `gc_at = max(1024, 2 × len)`: measure the idle processes' count and heaps;
   shrink a heap after a collection that leaves it mostly empty, as BEAM does after a full
   sweep. Every limit case must still pass (`beamlet-heap-flood`, `beamlet-budget-flood`: the
   process and ETS limits are a sixteenth of `budget_pages`, which this package lowers).
5. **Free-but-held pages.** If the runtime heap holds many more pages than the VM accounts
   for, the cause is the VM's growth pattern (doubling `Vec`s leave large free blocks that are
   unmapped, but small-class pages never return) or fragmentation. `shrink_to_fit` after a
   load is the VM's to do; returning empty small-class pages is `libs/rt`'s and MEM2 just
   changed it: ask before touching it.
Code-page sharing between VMs waits for sessions: not here.

## Step 3: the target, and what moves with it

The targets are the owner's, from the plan node, with the image's other servers at today's
9,984 pages and BEAM1's rule (the VM's budget is twice its use at the prompt): **the image
boots at 512 MiB**, the VM at or under about 10,000 pages on rv64 (a 16 % cut); stretch,
**QEMU's default 256 MiB**, the VM at or under about 2,700 pages. Compute the exact ceiling
from `init`'s `SystemFit` refusal at 512 MiB (its `need` and `free` name the numbers) and
report which target was met. When one is:
- `image/manifest.json`: `beamlet`'s `budget.pages`, `budget_pages=` and `heap_pages` (the
  most its budget holds beside its stack, as today), the rv64 peak doubled and rounded as
  MEM2 did;
- `memory_mib` in `tests/init-boot.toml`, `userland-boot.toml`, `userland-read-only.toml`,
  `verity-flipped-tree.toml`, `verity-wrong-root.toml` (and any other case at 1024 for the
  image's sake) to the target met; at 256 the line goes (the default);
- `image/README.md`'s RAM sentence and `mkimage`'s `-m`; `budgets.md`'s paragraph "With
  `beamlet` and the userland disk's…" (the pages at the prompt, the budget, the RAM); the
  comment in `userland-boot.toml`;
- `testbench.md`'s memory table rows for `beamlet` and the paragraph after it (the prompt's
  peak, the cap rule), from six runs as MEM2's were;
- `beamlet.md`: a subsection "What the VM holds at its prompt" under "Limits inside one VM"
  with the breakdown table on both widths, its status line (built, tested: the new case and
  the host tests), one sentence per lever kept, and one **Open:** line only if a target was
  missed, naming the gap. Pages carry no dates, package IDs or review history.

**Checkpoint 2:** if the levers in hand do not reach 512 MiB, stop with the table and the
remaining gap; the owner decides the next lever.

## Owned paths

`userland/otp/vm/src/{loader,module,process,memory,vm}.rs` and `userland/otp/vm/tests`;
`userland/otp/redoubt` (the argument, the report line); `userland/shell` (the start and the
registry's loading); `image/manifest.json` (the `beamlet` entry), `image/README.md`,
`image/mkimage` (the `-m`); `tests/beamlet-footprint.toml` and the `memory_mib` lines named;
the pages named. **Not yours:** `libs/rt` (ask), the kernel, `servers/init`, the packer,
`tools/testbench` (the scan is MEM2's; if a line must change, ask), the VM's `Platform` I/O
(BEAM3's), `sched.rs`'s reductions.

## The short gate

Both builds (rv64, rv32); host tests of `beamlet-vm`, `beamlet-redoubt` and the shell's own
(`mix test` where the bench runs it); the docs checker, `cargo fmt --check`, the size budget,
the `unsafe` ratchet unchanged, the no-cruft gate; own cases on both widths:
`beamlet-footprint`, `beamlet-boot`, `beamlet-console`, `beamlet-heap-flood`,
`beamlet-budget-flood`, `userland-read-only`, `verity-flipped-tree`, `verity-wrong-root` (all
at the new `memory_mib`), and the smoke set (`userland-boot` and `init-boot` carry the memory
scan at the new RAM, `bench-net-peer`, `ipc-outcomes`). The whole bench is the train's.

## Not here

Sharing code pages between VMs (sessions); a new allocator in the VM or in `libs/rt`; the
bignum and ETS limits; the I/O threads (BEAM3); the natives (BEAM4); a compact instruction
encoding beyond what step 1 justifies (it may become its own package at checkpoint 1).

## Checkpoint 1 ruling (Architect, on `.wash/local/BEAM6-report.md`)

The hypothesis held: decoded code is 82 % of what is accounted. Rulings:

1. **Levers now: 1 and 2** (shrink `code` after decoding; shrink each literal chunk before
   `Literals::add`). `term/mod.rs` and `atom.rs` join the owned paths for those lines and the
   read-only getters already made. Lever 6 is skipped as reported; lever 5 (lazy loading) is
   not done: the report's reason stands (the five largest modules return on the first command),
   written as one sentence on the page.
2. **Lever 3 (compact code) is its own package,** not here, although it is under a week: it
   rewrites the interpreter's operand access, which wants its own review and its own cases, and
   BEAM6 reaches the owner's visible target without it. Its node text is below; the report
   states the shape and the estimate (flat per-module operand array, 8-byte `Instr`, 16-byte
   `Arg`, ~2,510 pages of code either width) so the package starts from a measured design.
3. **The unaccounted pages owe an accounting line, not a lever.** With the runtime's getters
   (granted) the breakdown prints held-now and the record's peak; the gap splits into
   held-now less accounted (free-but-held small-class pages, B-tree nodes, the platform's
   buffers) and peak less held-now (the transient of the largest load: file, decode, the
   literal heap's doubling). Both numbers go in the table; neither becomes a lever in this
   package unless free-but-held is above 500 pages after levers 1 and 2, in which case stop
   and say so. B-tree nodes stay uncounted; the page says the breakdown is the VM's own
   count and the scan is the independent one.
4. **The 256 MiB stretch stays on the node as unreached, with the arithmetic:** even with
   compact code the prompt is about 5,450 pages on rv64 against a ceiling of about 2,700,
   because the shared literal table (about 700 pages after lever 2), the module tables and
   the compact code together are above it. Reaching it needs code kept in its on-disk form or
   loaded per function: a design question, not a lever; it is the compact-code package's
   Open item, and this page's one **Open:** line.
5. **The manifest's numbers in step 3** follow MEM2's rule, not the prompt alone: the budget is
   twice the largest scan peak across the memory cases (`init-boot`, `userland-boot`,
   `userland-read-only`, `beamlet-footprint`, six runs), since a shell that has run commands
   holds more than its prompt. Compute the 512 MiB ceiling from `SystemFit` and report the
   margin.
6. **The page.** On `beamlet.md`, under "Limits inside one VM", a subsection **"What the VM
   holds at its prompt"**: a status line (built · tested: `bench:beamlet-footprint`, the two
   host tests); one paragraph on how it is measured (the VM's own count of what it holds,
   printed under `report_memory`, which the image never passes; the bench's scan of the
   runtime heap's record as the independent total; B-tree nodes uncounted); the table in pages
   on both widths with rows at the page's level: decoded code (instructions, operands),
   literals (per module, the shared table), module tables, atoms, processes (heaps, the rest),
   ETS and binaries, accounted, runtime heap held and peak, not accounted; one sentence naming
   the five largest modules; one sentence per lever kept and one for why loading stays eager;
   the budget rule's link to budgets.md; the one **Open:** line of point 4. Bytes and counts
   stay in the report. No dates, package IDs or review history.

### Plan node for compact code (new, under M2, needs BEAM6; state todo)

Title: "The VM's code is compact: one flat operand array per module, an 8-byte instruction".
Body: Found by BEAM6's breakdown: decoded code is 7,180 pages on rv64 at the shell's prompt
(242,345 instructions of 32 bytes, 521,551 operands of 24 in one Vec per instruction, 82 % of
what the VM accounts for). Design measured by BEAM6: a flat per-module operand array, `Instr`
= op, count, start (8 bytes), `Arg::List` boxed so `Arg` is 16 bytes; the interpreter reads
operands through its one accessor (`interp.rs` `arg(ins, i)`); the loader, the native-body
patch and `on_load_entry` are the other users. About 2,510 pages of code either width, saving
about 4,670 on rv64 and 1,165 on rv32. Tier A, size M. Done when `beamlet-footprint`'s table
shows it on both widths, every limits and boot case passes, and the manifest's budget follows.
Open: QEMU's default 256 MiB (about 2,700 pages) is still out of reach with it; code kept in
its on-disk form or loaded per function is the question that package would answer.

## plan_set body for BEAM6 (state todo, needs BEAM2, MEM2: launchable)

Brief: .wash/local/BEAM6-implementer.md (architect-16). Tier A (the VM, the shell's start,
the image), size M, needs BEAM2 and MEM2 (merged): start from main. Today the VM's budget
holds 11,877 pages (rv64) / 7,554 (rv32) at the prompt, its runtime heap 11,814 / 6,966: the
heap is the footprint. Step 1 measures a breakdown the VM prints under an argument (code per
module with size_of::<Instr>, atoms, literals, process heaps live and held, ETS, binaries,
loader copies, runtime pages held vs accounted), on the host then in a case beamlet-footprint
with the memory scan as the verdict; checkpoint 1 with the table and ranked levers. Step 2:
lazy loading in the shell's start, no second copy of a module's chunks, compact code only if
it is a third of the heap (else proposed as its own package), smaller heaps and a shrinking
collector, free-but-held pages (libs/rt by permission). Step 3: the owner's targets, the
image at 512 MiB (VM about 10,000 pages rv64) or 256 MiB (about 2,700); manifest, memory_mib
lines, README, budgets.md, testbench.md's table and beamlet.md's new breakdown subsection move
with the target met; checkpoint 2 if missed. Short gate plus the beamlet and verity cases at
the new RAM. Code-page sharing between VMs is for sessions, not here.
