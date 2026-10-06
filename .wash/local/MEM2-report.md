# MEM2 report (pre-machine)

Branch wp-MEM2 in /home/mcloonan/redoubt/.worktrees/MEM2, on MEM1's head cd73175b5. Never pushed.
No cargo command and no QEMU run (machine hold). Nothing below is compiled or tested yet.

## Commits (in order)

1. d47e3cc30 rt: cap the heap's pages and record its peak
   - libs/rt/src/heap.rs: `Heap::cap(pages)` (once, not 0, not on a fixed heap), `fix` refuses a
     capped heap; `held` page count; `pages()` refuses before `map_anon` when held + n > cap;
     `free_pages` drops held. `Record` (#[repr(C, align(8))], 8 x AtomicU32 = 4 LE u64 on both
     widths: magic, tag, cap, peak), zero until `Heap::mark(tag)`; peak raised under the lock.
     `RECORD_MAGIC` public. No new `unsafe` in src.
   - libs/rt/tests/heap.rs: capped_heap_refuses_before_the_kernel, a_fixed_heap_is_never_capped,
     the_record_is_zero_until_marked (over the fake; "no map_anon" shown by f.held unchanged).
2. e608fd7a2 rt: carry the heap cap and launch tag in startup block v2
   - libs/wire/tables/startup.md: `heap_pages: u32`, `tag: u16` appended.
   - libs/wire/src/proto/startup.rs: HAND-MATCHED to the generator's output; must be checked by
     running redoubt-wire-gen (its byte-for-byte test) after the grant.
   - libs/rt/src/startup.rs: VERSION 2, `heap_pages()`/`tag()`, builder setters; tests:
     heap_pages_and_tag_round_trip; hostile_blocks_are_refused now refuses versions 0, 1, 3 and a
     v1-shaped block (Malformed).
   - libs/rt/src/start.rs: `start` caps HEAP from the block and marks the record before main.
   - stub/src/lib.rs: VERSION 2 (version only).
   - libs/rt/fuzz/fuzz_targets/startup.rs: asserts heap_pages() is never Some(0).
3. e78aa4da2 client: `Launch::heap_pages(u32)`; block carries heap_pages and stack_tag as tag.
   libs/client/tests/launch.rs asserts both read back.
4. c23e521f1 init: `servers[i].heap_pages` (optional decimal string). check.rs refuses, naming
   `servers[i].heap_pages` (Why::Heap): 0, > u32::MAX, heap + stack >= budget pages. init.rs passes
   it; the block-fit check builds the same block (cap and tag). Test:
   a_server_heap_cap_is_optional_and_fits_its_budget_beside_its_stack.
5. 469dd90e4 testbench: memory.rs `Server { name, stack_pages, heap_pages }` (was `Stack`),
   `servers()` (was `stacks()`); the scan keeps a 4-unit window, so a record across a chunk end is
   read; lines `heap NAME PEAK of CAP pages` / `heap NAME PEAK pages uncapped`; failures: missing
   record, cap != manifest, cap < 2*peak (saturating); duplicate record is an error, as a duplicate
   paint unit is. qemu.rs: measure_stacks -> measure_memory, messages say "memory measurement".
   Cargo.toml adds redoubt-rt; Cargo.lock hand-edited (one line in testbench's deps).
   New tests: scanner_reports_each_heap_and_holds_a_cap_to_twice_its_peak,
   scanner_refuses_a_missing_or_duplicated_heap_record; old ones now carry records.
6. 0b03eb4fd tests: tests/heap-cap.toml + tests/programs heap-cap (tester in init's place) and
   heap-capped (child on the runtime, cap 16, budget 512). Child takes 1-page blocks fallibly
   until refused, calls tester (AT_CAP), then try_reserve 64 pages, calls (REFUSED). Tester reads
   the child's budget with budget_usage before and after; passes only if refused, usage equal, and
   the budget had room for the 64.
7. 0e81f66a6 docs (page lines below); tests/memory-host-tests.toml description.

Not yet written (needs the machine): commit 8, image/manifest.json heap_pages = 2 x max(rv32,
rv64) peak per server under the memory case, the testbench.md table's heap column, beamlet's
peak vs its BEAM1 limits.

## Page lines as written

- init.md servers row: "..., and its stack in pages (`stack_pages`, 16 if absent, at most 128),
  and its heap cap in pages (`heap_pages`, none if absent)".
- init.md bullet after Stacks: "**Heaps.** A server's heap cap is the most pages its runtime's
  allocator holds; past it an allocation fails in the runtime before the kernel is asked. The
  budget stays the bound on everything else. `init` refuses 0, or a cap and stack the budget cannot
  hold."
- init.md startup block table: `version` "2; a block of any other version is refused";
  `heap_pages` "the child's heap cap in pages ([Heaps](#the-boot-manifest)), 0 for none"; `tag`
  "the launch tag the bench measures the child's stack and heap by, 0 for none: `init` gives each
  server its place in the manifest, from 1". "Using it" adds: the runtime "caps its heap at
  `heap_pages`, and marks its heap record with `tag`, all before `main`". Status +2 entries
  (heap_pages_and_tag_round_trip, bench:heap-cap), 15.
- budgets.md after the opening: "A server's heap cap (`heap_pages`, init) is its runtime's own
  ceiling inside its budget: past it the runtime refuses an allocation before the kernel is asked,
  and the cap reserves nothing until it is used. The budget stays the hard bound on everything else
  the server holds: its image, its stacks, its buffers and lends, its page tables and its kernel
  objects." Status +1 (init host test), 10.
- native.md heap row: "...; capped at the startup block's `heap_pages` (init), past which it
  refuses before `map_anon` is asked; and a record of its cap and peak for the bench (the memory
  budget)". Status +3 rt heap tests, 21.
- testbench.md, The memory budget: a paragraph on the record (32 bytes: magic, tag, cap, peak;
  magic and tag written before main), the two line forms, and the verdict (missing/duplicated
  record, cap not the manifest's, capped cap < 2 x peak fail; uncapped reported only).

## Affected summaries checked

- README.md, GETTING-STARTED.md: no heap/startup-block claims; no change.
- docs/plan/m1-separation.md: no heap or block-version claim; no change.
- docs/servers/README.md, docs/kernel/README.md, image/README.md: no heap claim; image/README.md
  will need a word only if commit 8 changes the RAM line; checked again then.
- docs/kernel/memory-layout.md: no heap-cap claim; no change.
- docs/userland/beamlet.md "Limits inside one VM": says the hard backstop is the embedder's
  allocator and the budget; once beamlet is capped (commit 8) the backstop is the cap first, so
  that sentence changes with commit 8.
- docs/todo/beamlet-budget-from-startup.md: stays open (not this package's); unchanged.

## Point 4: serving paths that allocate fallibly (none changed)

try_reserve / try_with_capacity in: libs/rt/src/server/{admit,minted,ninep,ninep_mux,parked}.rs,
libs/rt/src/startup.rs, servers/blkd/src/{args,gpt,lib}.rs, servers/bootfsd/src/server.rs,
servers/consoled/src/server.rs, servers/fsd/src/{quota,server,volume}.rs,
servers/ipd/src/{fs,link,scope}.rs, servers/keyd/src/keys.rs. Every other allocation is
infallible: past the cap it reaches Rust's default no_std alloc-error handler, which panics, so
the runtime reports and exits 101 and init's restart rule applies. To confirm on the machine.

## Traps / still to do after the grant

- Run redoubt-wire-gen and its test: the generated startup.rs was matched by hand.
- Size budget: libs/rt, libs/client, servers/init, stub grew; if a ceiling is passed the commits
  need `Size budget: <crate>: <reason>` lines (rebuild of the commits, no fix-up commits).
- servers/init/tests/manifest.rs clones `m.servers[3]` (blkd) with budget(17) in two tests; once
  the image gives blkd a heap_pages, those clones need `heap_pages: None` (goes in commit 8).
- cargo fmt not run; lines kept under 110 by hand.
- Doc pages were read in the brief's named sections, not whole (the brief's context rule); the
  rest of each committed source file was read in full. Cargo.lock (generated) was not read whole.
- Duplicate heap record = scan error: a restarted server leaving a stale record in freed RAM would
  trip it, as a stale paint unit would MEM1's stack scan.

# After the machine grant (2026-10-05)

Commands (exit codes): cargo +nightly fmt --all 0 (two fmt-only changes folded into commit 1);
cargo test -p redoubt-wire-gen 101 then 0 (the Rust codec matched; the generator's Elixir codec
libs/wire/elixir/proto/startup.ex was missing and was taken as generated, no users); target build
of rt, client, stub, every server, test-programs, init-programs, fsd-programs, net-tests,
net-client, both triples: 0, 0; beamlet-redoubt (userland/otp) both triples: 0, 0 (vendor warnings
only; one unused const in heap-cap fixed). `./build --programs` covers only kernel, loader and
test-programs, so the sweep was run with explicit -p lists. Host tests: rt 147 (one borrow error
in my v1 test fixed), init, client 36, stub 23, wire 39, wire-gen 17, bootfsd 15, consoled 16, keyd
40, blkd 49, netd 29, ipd 51, fsd 61, testbench 91: all 0. Fuzz target `startup`: cargo-fuzz not
installed; `cargo build --bin startup` in libs/rt/fuzz 0. size-budget 0 after raises (rt 3511, wire
3077, client 993, init 1969, each with its line), unsafe-budget 0, docs 0, no-cruft 0,
memory-host-tests 0.

Semantic changes beyond the brief:
- MEM1 bug: the QMP socket path in the run dir passed 108 bytes, so every memory case failed to
  start QEMU here; it now lives in the temp dir (commit 457d8133f, with a_qmp_socket_path_binds).
- Stacks measured deeper on this branch: fsd:system 12,712 B -> 7 pages (was 6), beamlet 33,256 B
  -> 17 (was 16); image bound 507 -> 508 on the machine.
- beamlet's heap cap is 24,558 (the budget's room beside its stack), not 2 x peak: its peak moved
  11,813 -> 11,814 between runs and 23,626 failed the verdict by one page.

Cases on the final tree: heap-cap rv64 PASS, rv32 PASS; init-boot rv64/rv32 PASS; userland-boot
rv64 PASS (final), rv32 PASS; userland-read-only rv64 PASS (final), rv32 PASS (rv32 runs used
beamlet cap 23,626; the only change after is beamlet's larger cap).

Heap peaks rv64/rv32 (pages) and caps: keyd 4/4 cap 8; consoled 9/9 18; bootfsd 28/28 56; blkd
17/17 34; netd 2/2 4; ipd 4/4 8; fsd:data 9/8 18; blkd:system 17/17 34; fsd:system 16/15 32;
beamlet 11,814/6,966 cap 24,558; read-only client 31/31 uncapped.

# Final state (fix round 1, rebased onto main 27e74ec3b), 2026-10-06

Head a359e2cdc on main 27e74ec3b (MEM1 merged). Commits:
bc50b4357 rt heap cap+record; 00b3b514a startup block v2; 9f8109114 client Launch::heap_pages;
51fed8c22 init heap_pages; 42bc99847 testbench heap scan; af44b2b40 heap-cap case; c2bbe58e0 docs;
a359e2cdc image caps. range-diff against eb23aa4aa: 1-6 and 8 identical; 7 (docs) differs only by
tests/memory-host-tests.toml's description, merged with MEM1's on main (one conflict). Outside
docs and that file the tree equals eb23aa4aa's (main's .wash files aside), so its gates carry.

Findings (review round 1):
- red 1 applied: Record is #[repr(C, align(32))] with a const assert (32 bytes, divides a page),
  so it never crosses a page; the reason is in its doc comment.
- red 2 + simplifier 3 applied: heap_pages is parsed as u32 in manifest.rs (one SchemaError,
  test with 4294967295 / 4294967296); check.rs's u32 bound, the two `as u32` casts and their
  comments are gone; Why::Heap's text stays about 0 and the budget.
- red 3 applied as docs: beamlet.md says the budget is the backstop; the image's cap sits at the
  budget's edge, where the budget binds first, and is there for the measurement. Cap unchanged.
- red 4 applied: heap-capped ends with an infallible allocation past its cap; heap-cap expects its
  exit notice to carry exit::PANIC (101) ("ended the child, code 101" line). No init restart line:
  the tester is in init's place. native.md's heap row names this.
- red 5: noted.
- simplifier 1 applied (Heap's cap field gone; the cap is the record's CAP word); 2 applied
  (cap()+mark() -> one Heap::start(cap, tag), one lock; tests use start); 4 applied (one line);
  6 declined: no existing fixture launches a capped child and reads its budget around the refusal.
- editor 1: stack column left as MEM1's (mine, 12,712/33,256, fit the same pages); heap column is
  my measurement. 2 applied (beamlet.md reflowed). 3 applied: rv32 ran the final caps. 4: this.

Gates on eb23aa4aa (pool): fmt --check 0; ./build --programs rv64 0 rv32 0; sweep (rt, client, stub,
servers, test/init/fsd programs, net, net-client) rv64 0 rv32 0; beamlet rv64 0 rv32 0; host tests
rt 147, init 62, client 36, stub 23, wire 39, wire-gen 17, testbench 97: all 0; fuzz startup build 0;
size-budget 1 (init 1970 > 1969) then 0 after ceilings set in their commits (init 1970, rt 3504);
unsafe 0; docs 0; no-cruft 0; memory-host-tests 0. On a359e2cdc: docs 0.
Cases on eb23aa4aa: heap-cap, init-boot, userland-boot, userland-read-only PASS on rv64 and rv32.

Heap peak rv64/rv32 -> cap (pages): keyd 4/4 -> 8; consoled 9/9 -> 18; bootfsd 28/28 -> 56;
blkd 17/17 -> 34; netd 2/2 -> 4; ipd 4/4 -> 8; fsd:data 9/8 -> 18; blkd:system 17/17 -> 34;
fsd:system 16/15 -> 32; beamlet 11,814/6,966 -> 24,558 (budget 24,576 with a 17-page stack);
read-only client 31/31 uncapped.
