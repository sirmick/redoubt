# INIT5 report: first step (before the INIT_PAGES change)

Branch wp-init5, base main c2283a825.

## Commits
- b0611ae6f rt: the fake kernel refuses a later call, and keeps a process's peak pages
  (Fake::refuse_after, Fake::held_peak; refuse = refuse_after skip 0). Granted by the orchestrator (option A).
- ebac4ebf7 client: a launch places its image and stack 64 pages at a time
  - libs/client/src/launch.rs: `pub const PLACE_PAGES: usize = 64`; `place(process, bytes, pages, dst, flags)`
    loops batches of at most 64 pages, each a fresh Buffer moved to dst + offset; the stack goes the same way.
  - servers/init/src/bound.rs: launch term = stub + tables, min(image, 64) + tables, min(stack, 64) + tables.
  - Tests: client `an_image_moves_one_batch_at_a_time` (3x64+1 pages: four moves at IMAGE_AT + k*64 pages,
    sizes 64/64/64/1, bytes read back whole, held_peak == held + 64, held unchanged);
    `a_refusal_on_the_third_batch_leaves_the_launcher_as_it_was` (refuse_after process_map 3: 4 process_maps,
    no process_start, OOM, budget handed back, peak held + 64, held unchanged, budget destroyed);
    init unit test `an_image_and_a_stack_larger_than_a_batch_count_one_batch`.

## Outside owned paths (needed by the cases)
- tests/beamlet-boot.toml: bound regex (1059|1286) -> 416.
- servers/init/tests/manifest.rs `the_image_manifest_s_bound`: ipd 147 pages -> one 64-page batch.
- tests/size-budget.toml: libs/client 623 -> 632, servers/init 1825 -> 1828 (lines in the commit).

## Bound
- beamlet-boot: before 1,059 (rv64) / 1,286 (rv32); after 416 / 416. Twice = 832 <= 1,024.
- init-boot: 447 on both widths (root holds 309).
- beamlet's image: 700-odd pages -> 11-12 batches (not printed; ceil(pages/64)).

## Page lines written
- native.md step 3: brief's text exactly.
- init.md step 2: "... read-write; placed 64 pages at a time; the stack; ..." (rewrapped); status
  list gains the two client tests (19 -> 21).
- budgets.md item: brief's text, ending "." not ";" (it is the list's last item).
- budgets.md sizing sentence: interim "416 pages on rv64 and 416 on rv32 ... 2,048 leaves 1,632 to
  spare ... do not grow with the machine, nor with the size of a program it starts"; the brief's
  final wording lands with INIT_PAGES 1,024.

## Commands (all /home/mcloonan/redoubt/.wash/local/in-dev, from the worktree)
- cargo testbench rt-host-tests / client-host-tests / init-host-tests: 0
- cargo testbench --arch rv64|rv32 init-boot: 0, 0
- cargo testbench --arch rv64|rv32 beamlet-boot: 0, 0 (after the regex change)
- cargo testbench docs / size-budget / unsafe-budget: 0 (unsafe unchanged)
- cargo +nightly fmt -p redoubt-client -p redoubt-init -p redoubt-fake-kernel (applied; clean)

## Next
INIT_PAGES 2,048 -> 1,024 in kernel/src/budget.rs, budgets.md's final sentence and "(1,024)",
init-refuses-bound grown if it now fits, stub-launch, bundle-mapped, beamlet-boot at 1,024; whole
bench on both widths only on your word.

# Second step: INIT_PAGES 1,024 (tip 929719bdd)

Commits: b0611ae6f (fake), a50685f8e (client; body now names tests/beamlet-boot.toml and
servers/init/tests/manifest.rs), 929719bdd kernel: INIT_PAGES returns to 1,024.

- kernel/src/budget.rs: INIT_PAGES 2048 -> 1024.
- budgets.md: "`INIT_PAGES` (1,024)"; sizing sentence as the brief's: "With `beamlet`, the bound is
  416 pages on rv64 and 416 on rv32 (`beamlet-boot` prints it), and 1,024 at least doubles both.
  It is a fixed count, not a share of RAM, because `init`'s needs do not grow with the machine,
  nor with the size of a program it starts, and a share of a large machine would sit idle in
  `root`." Paragraph rewrapped.
- init-refuses-bound: still refuses at 1,024; zeros entry 6 MiB -> 262144 bytes (one batch);
  expect: "would cost init 1047 pages of root and root keeps 1023 (INIT_PAGES)", both widths.

Commands (in-dev, from the worktree), all exit 0:
- cargo testbench --arch rv64|rv32: init-refuses-bound, stub-launch, bundle-mapped, beamlet-boot
  (bound 416), init-boot (bound 447, root holds 309)
- cargo testbench docs, size-budget, unsafe-budget (unchanged), host-tests (all 16 host suites
  incl. model 430 s), rt-host-tests, client-host-tests, init-host-tests
- cargo +nightly fmt --check: 0 diffs

Batches for beamlet's image: ceil(pages/64), about 11 for ~700 pages.
Not run: the whole bench on both widths (waiting on the orchestrator's word).
Risk: init-refuses-bound's margin is 24 pages (1,047 vs 1,023); init growing only widens it.

# Red round 1 notes (tip 2a60f1ad2)

Commits rebuilt: f20d584b7 fake (+ Fake::launched_in(owner, budget): each child made in a budget,
readable after the launcher closed its handle), 71fb1f996 client, 2a60f1ad2 kernel. Tree vs
929719bdd differs only in the files below.
1. a_refusal_on_the_third_batch_...: asserts the one child in the budget holds exactly the stub
   (STUB_ENTRY, 1 page) and the image's first two batches (IMAGE_AT, IMAGE_AT + 64 pages), and
   never started (start None), before the budget is destroyed.
2. Taken: an_image_moves_one_batch_at_a_time now launches with a 65-page stack: two moves,
   (STACK_TOP - 65 pages, 64 pages) and (+64 pages, 1 page), then the block at STARTUP_AT, 8 maps.
   A refusal on a stack batch is not tested separately (same loop as the image's).
3. init.md step 2: "read-write, placed 64 pages at a time; the stack;". budgets.md: "416 pages on
   both widths ..., and 1,024 at least doubles it" (interim in 71fb1f996: "...and 2,048 leaves
   1,632 to spare"); paragraph rewrapped.
Exit 0: rt-/client-/init-host-tests, docs, size-budget, unsafe-budget; nightly fmt --check clean.
QEMU cases not rerun: no code they run changed (fake, tests and pages only).
