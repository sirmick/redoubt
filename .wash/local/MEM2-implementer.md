# MEM2: each server's heap capped in the manifest, enforced by the runtime's allocator

Size S+ (the startup block's version and the sweep it forces). Needs MEM1 (the scan, the tag, the manifest checks) and ABI1 (the runtime's seam, in
`libs/rt`). This package changes the startup block, a runtime contract: every binary linking
`redoubt_rt` (servers, `tests/programs`, `tests/net`, beamlet), and the stub, which reads the
block through `redoubt-wire`, is built and tested before any whole bench. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The owner (2026-10-03): "Per-server heap cap in the manifest, enforced by rt's allocator. It's
the same idea as fsd's byte quotas, applied to RAM. On redoubt it bounds what MapAnon can take;
on nommu it becomes the server's fixed arena."

## Context rules (read these first)

- **Don't read whole files.** `libs/rt/src/heap.rs` (251 lines) whole; `libs/rt/src/startup.rs`
  only the format doc, `from_fields` and `StartupBuilder`; `libs/rt/src/start.rs` only `start`;
  `libs/wire/tables/startup.md`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** No boot-log hex in reports.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/MEM2-report.md`.

## Reading list (only these)

- `docs/servers/init.md`: the manifest table, "The startup block", R31.
- `docs/kernel/budgets.md`: the opening of "The tree from the boot manifest".
- `docs/userland/native.md`: "`redoubt-rt`, the native runtime" (the `heap` row).
- `docs/testbench.md`: "The memory budget" (MEM1's).

## What exists

The heap takes pages from `map_anon` as it needs them, or, after `fix_heap(pages)` (only `init`
calls it), from one arena mapped once; either way an exhausted source returns null. A server's
only memory bound is its kernel budget. The startup block is version 1.

## The settled design

1. **What the cap is.** A server's heap cap is the runtime's own ceiling, below its kernel
   budget: the most pages the global allocator holds at once (small classes' pages, which it
   never returns, and live large blocks). An allocation that would pass it is refused in the
   runtime, before any `map_anon`, and returns null. The kernel budget stays the hard bound on
   everything else the server holds: its image, its stack, thread stacks (`thread::spawn` maps
   them itself), `Buffer`s and lends, page tables, kernel objects. Pages are counted as mapped,
   never committed up front, so a cap costs nothing until it is used. With `fix_heap`, the arena is
   the cap; a capped heap refuses `fix` and a fixed one refuses a cap (`InvalidArgument`). On a
   backend without an MMU the same number sizes the fixed arena; nothing builds that here.
2. **The manifest key.** A `servers` entry may carry `heap_pages`, a decimal string; absent means
   no cap (the kernel budget alone, as today). Refused, naming `servers[i].heap_pages`: 0; or
   `heap_pages` plus `stack_pages` at or above the server's budget pages.
3. **The startup block, version 2,** gains two fields, 0 for none: `heap_pages` (the cap) and
   `tag` (MEM1's launch tag). `Launch` gains `heap_pages(n)` and writes `stack_tag` into the block
   too. beamlet's `budget_pages=` argument is not this package's
   (docs/todo/beamlet-budget-from-startup.md stays open). The parser
   refuses version 1. The runtime sets the cap from the block in `start`, before `main`.
4. **Past the cap.** The allocator returns null. An infallible allocation then ends in Rust's
   allocation-failure handler, which on `no_std` panics: the runtime reports the panic and exits
   with 101, and `init`'s restart rule applies, as for an exhausted budget today. A fallible one
   (`try_reserve` and the like) sees the error. Report which serving paths allocate fallibly; none
   changes here.
5. **The record.** The heap keeps one 32-byte record in the program's data: a magic word, the
   tag, the cap and the peak pages held. The runtime writes the magic and tag in `start` (the
   ELF's copy is zero, so the bundle and image copies never match) and the peak under the heap's
   lock when it rises: one comparison per page mapped. MEM1's scan finds it by magic and tag and
   prints `heap <name> <peak pages> of <cap pages>` (or `uncapped`).
6. **The verdict.** The bench fails a capped server whose cap is less than twice its peak; an
   uncapped one is reported only.
7. **The image's numbers.** Every server in `image/manifest.json` declares `heap_pages`: twice the
   larger of its rv32 and rv64 peaks under MEM1's `memory` case. beamlet's cap must also leave
   its BEAM1 limits (heap and ETS each budget / 16) reachable; report its peak beside them.

## The cases

1. **Host, runtime, over the fake:** a capped heap refuses past the cap with no `map_anon` call;
   a freed large block returns its pages to the count; cap and `fix` exclude each other; the
   record's peak.
2. **Host, startup:** version 2 round trips the two fields; version 1 is refused; R31's hostile
   blocks still refused.
3. **Host, `init`:** the key, its absence, both refusals; the block carries the cap and tag.
4. **Machine:** a program in `tests/programs` capped low allocates fallibly past its cap, prints
   the refusal, and shows with `budget_usage` that its budget was not charged for it; MEM1's
   `memory` case reports every image server's heap, both widths.

## Page lines (exact text in the report)

- **init.md**: the `servers` row gains ", and its heap cap in pages (`heap_pages`, none if
  absent)"; a bullet after "Stacks": "**Heaps.** A server's heap cap is the most pages its
  runtime's allocator holds; past it an allocation fails in the runtime before the kernel is
  asked. The budget stays the bound on everything else. `init` refuses 0, or a cap and stack the
  budget cannot hold." The startup block table: `version` 2 and the two new rows.
- **budgets.md**, after the opening of "The tree from the boot manifest": the relation in two
  sentences (the cap is the runtime's ceiling inside the budget; the budget bounds the rest).
- **native.md**: the `heap` row names the cap from the startup block; the host tests listed.
- **testbench.md**, "The memory budget": the heap record, its line and its verdict.

## Owned paths

- `libs/rt/src/{heap.rs,startup.rs,start.rs,lib.rs}`, `libs/rt/tests/`, `libs/wire/tables/startup.md`
  (and what it generates), `libs/client/src/launch.rs`, the stub's reading of the block (its
  version only).
- `servers/init/src/` (the key, the block's fields), `servers/init/tests/`.
- `tools/testbench/src/` (the scan's heap part); the new case.
- `image/manifest.json` (`heap_pages`); the page lines above.

**Not yours:** the kernel; `thread::spawn`'s stacks; `Buffer`; beamlet and its `budget_pages=`.

## Gates

- Every binary linking `redoubt_rt` built for both widths, then the whole bench on both widths, alone.
- `rt-host-tests`, `init-host-tests`, the client's and every server's host tests,
  `cargo test -p testbench`, the startup block's fuzz target built.
- `cargo fmt --check`, the size and unsafe budgets, doccheck, no-cruft.

Report each command with its exit code, every server's heap peak on both widths and its cap, and
each page line as written.
