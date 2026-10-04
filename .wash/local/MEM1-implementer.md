# MEM1: each server's stack declared in the manifest, painted, and measured by the bench

Size S+ (the bench's RAM scan is most of it; the rest is S). Needs BEAM2 (shared files: `init`'s manifest and checks, the bench's QEMU and case code,
`image/manifest.json`). Start from main after BEAM2 merges. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The owner (2026-10-03): "Per-server stack size in the manifest, plus a high-water mark measured
in the bench. Right now launch gives a fixed stack_pages (16 pages, 64 KB). Have each server
declare its stack, and have the testbench paint the stacks and report peak use per server,
failing if it's over the declared size. That tightens memory on redoubt, and on nommu those
numbers are the static stack allocations."

## Context rules (read these first)

- **Don't read whole files.** `libs/client/src/launch.rs` (the builder, `plan`, `place`);
  `servers/init/src/check.rs` only `Machine`, `counts` and the server checks you extend;
  `servers/init/src/bound.rs` only `Counts` and `bound`; `servers/init/src/bin/init.rs` only
  `STACK_PAGES` and the `Machine` it builds; in `tools/testbench/src/qemu.rs` only the QEMU
  command line and the case's end.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** No boot-log hex in reports.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/MEM1-report.md`.

## Reading list (only these)

- `docs/servers/init.md`: the manifest table, "Launching through the loader stub".
- `docs/kernel/budgets.md`: "The tree from the boot manifest" (the INIT_PAGES bullets).
- `docs/kernel/memory-layout.md`: "Launcher placement" and its residual "No guard gap".
- `docs/userland/native.md`: "The client library".
- `docs/testbench.md`: "The case file", "The size budget" (the model for the new section).

## What exists

`redoubt_client::launch`: `STACK_PAGES = 16`, `Launch::stack_pages(n)`, and `place` maps the
stack's pages zeroed, `PLACE_PAGES` at a time. `init` does not expose a stack: `init.rs` has its
own `STACK_PAGES = 16` and passes it as `Machine::stack_pages`, which `counts` gives the bound
(INIT5: one batch of the stack, at most `PLACE_PAGES`). The manifest has no stack key. The
launcher cannot read a child's stack after the move, and the kernel offers no way to.

## The settled design

1. **The manifest key.** A `servers` entry may carry `stack_pages`, a decimal string (a size in
   pages, the manifest's type rule), 16 if absent (`redoubt_client::launch::STACK_PAGES`, made
   public; `init`'s own constant goes). Refused, naming `servers[i].stack_pages`: 0; above
   `MAX_STACK_PAGES` = 128 (one definition in the stub crate beside `STACK_TOP`: 512 KiB, which
   keeps 127 unmapped pages between the stack and `STARTUP_AT`, and fits the paint's index); at or
   above the server's budget pages. `Machine::stack_pages` goes; `counts` takes the largest
   declared stack, and the bound still counts one batch of it.
2. **The paint.** The launcher fills the stack's pages, instead of zeroes, with 8-byte units:
   unit `i` (0 at the stack's lowest address) is the little-endian `u64`
   `STACK_PAINT << 32 | tag << 16 | i`, `STACK_PAINT` a 32-bit constant and the format defined
   once in the stub crate, used by the launcher and the bench. `Launch::stack_tag(u16)` sets the
   tag, 0 if never called. Every launch paints, in every boot: the same pass over the pages that
   zeroing them was, and nothing else at run time. The pattern is public; check that nothing (the
   stub, the runtime's entry) assumes a zeroed stack, and report it.
3. **The tag.** `init` tags each launch with `index + 1`, `index` the server's place in
   `servers`. Frames are zeroed when next allocated, not when freed, so a restarted server's old
   paint may survive: a `memory` case is one in which no server restarts, and a unit found twice
   fails it.
4. **The measurement.** A case with `memory = true` starts QEMU with a QMP socket. After the
   case's verdict lines pass, the bench stops the guest (`stop`), saves its RAM (`pmemsave` from
   `0x8000_0000`, the case's `memory_mib`), and scans every aligned 8-byte unit for the paint. Per
   server: the lowest unit index missing from 0 up is the deepest the stack
   reached; peak = (units - that index) * 8 bytes. A unit found twice for one tag, or a server of
   the manifest with no paint at all (overrun, or ended), fails the case naming it.
5. **The verdict.** The bench prints one line per server, `stack <name> <peak bytes> of <declared
   pages> pages`, and fails a server whose declared stack is less than twice its peak, rounded up
   to a page. An overflow faults on the unmapped page below the stack before any scan can see it,
   so "over the declared size" is never observable; the margin is what is checked. Confirm from
   `map_anon`'s placement that nothing is mapped right below a declared stack, and report it.
6. **The image's numbers.** Every server in `image/manifest.json` declares `stack_pages`: twice
   the larger of its rv32 and rv64 peaks under the image's cases, rounded up. Report each server's
   peaks on both widths and the pages saved against 16.

## The cases

1. **Host, `init`:** the key parsed with its default; each refusal; the bound counts the largest
   declared stack's batch.
2. **Host, client:** extend `a_child_gets_the_stub_its_image_a_stack_and_its_block`: the stack's
   pages carry the paint with the tag and the unit indices, bottom to top.
3. **Host, testbench:** the scanner over a synthetic RAM image: the peak computed; a missing
   server, a duplicate unit and a stack under twice its peak fail.
4. **Machine:** `memory = true` on one case that boots the image's manifest and brings all its
   servers up (name it), both widths.

## Page lines (exact text in the report)

- **init.md**, the manifest table's `servers` row: "... and arguments" gains ", and its stack in
  pages (`stack_pages`, 16 if absent, at most 128)"; a bullet after "Names": "**Stacks.** A
  server's stack is the pages its first thread starts on, charged to its budget; `init` refuses 0,
  more than 128, or a stack its budget cannot hold. The bench measures each server's peak and
  holds the declared stack at twice it ([the memory budget](../testbench.md#the-memory-budget))."
  "Launching through the loader stub", step 2: "the stack" becomes "the stack, painted with a
  pattern the bench reads ([the memory budget](../testbench.md#the-memory-budget))".
- **memory-layout.md**, "Launcher placement": the stack is at most `MAX_STACK_PAGES` (128) pages
  below `STACK_TOP`, so unmapped pages always separate it from the startup block.
- **native.md**, "The client library": `stack_pages` and `stack_tag`, and that every launch paints.
- **budgets.md**: the `INIT_PAGES` stack-batch bullet unchanged; one sentence that a server's
  stack is its manifest's `stack_pages`, charged to its own budget.
- **testbench.md**, a new section "## The memory budget" after "The size budget": the `memory`
  key, the paint, the scan, the per-server line, the twice-the-peak verdict, and its residual: the
  peak is what the cases drove; a deeper path they never take is not seen, which the margin covers.
  List the new cases under its status.

## Owned paths

- `libs/client/src/launch.rs` and its tests; the stub crate's constants (`MAX_STACK_PAGES`, the
  paint format) and nothing else in `stub/`.
- `servers/init/src/{manifest.rs,check.rs,bound.rs,refusal.rs,bin/init.rs}`, `servers/init/tests/`.
- `tools/testbench/src/` (the case key, QMP, the scan, the report), the new cases.
- `image/manifest.json` (the `stack_pages` values), the page lines above.

**Not yours:** the kernel; the startup block (MEM2 changes it); the heap (MEM2). **Hotspot:**
MEM2 follows and extends the scan and the manifest checks; it starts after this merges.

## Gates

- The whole bench on both widths, alone.
- `init-host-tests`, the client's host tests, `cargo test -p testbench`.
- `cargo fmt --check`, the size and unsafe budgets, doccheck, no-cruft.

Report each command with its exit code, every server's stack peak on both widths and its declared
number, and each page line as written.
