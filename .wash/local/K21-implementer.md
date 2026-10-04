# K21: taking or giving back a frame costs no term in RAM frames

Tier A (the kernel's frame allocator), size M, no needs. A kernel finding from QA
`GATE1-trace-ring`. Every cargo and bench command runs as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## The finding

R12 (scheduling) says: "A system call's kernel time is bounded by a constant plus a term linear
in the pages it maps or the objects it names. ... A term linear in RAM frames or kernel-object
frames is not [a constant]. Billing it to the caller does not excuse it, because every wake waits
for it." (scheduling.md, the paragraph after R12's opening).

Three production paths in `kernel/src/mem.rs` scan the RAM ownership table:
- **`alloc_frame`** is first fit from frame 0: `allocations.iter().position(Option::is_none)`.
  Every page a call backs, every page table and every object frame scans past every used frame
  below the first free one.
  - GATE1 exposed it. The test-only trace ring took 8,192 more frames at boot, below everything,
    and `sched-budget-churn`'s shell, which creates 5 budgets a cycle, ran 74 cycles where it had
    run 86.
  - Its victim's net share went from 499 to 559 (rv32). R10 times were flat, and audits were
    unbilled (`.wash/local/GATE1-ring-churn.md`).
- **`alloc_contiguous`** (`dma_alloc`) is a first-fit run search over the whole table.
- **`release_owned_frames`** (`process_create`'s rollback, process.rs) scans all of RAM, twice.

Boot-time scans (`budget.rs`'s `ram_frames_owned_by` at boot) and the checked build's audits are
not in scope: neither runs on a call path in a release build.

## The rule, and the page lines

The package writes these lines in the commit that makes them true.

scheduling.md, R12's bounded-work paragraph. After "`map_anon`'s search is linear in the
fixed-size area it searches, a constant, and never in `len` (`bench:map-anon-search-bound`).",
add:
> A RAM frame is taken from a free list and given back to it, so backing a page, a page table or
> an object costs no search of RAM, however much of it is in use (`bench:scan-bounds`).

memory.md: where the allocator is described (find the place; "A page's life" or "Backing and
zeroing"), one sentence saying the same. Also say how `dma_alloc` finds a run, under whichever
design below lands.

## The package

1. **`alloc_frame` and its frees in O(1).** A free list of frame indices.
   - **Either intrusive**: the next free index stored in the free frame's first word through the
     physmap. That costs no memory, and every frame is zeroed before any process or object sees
     it (R11; `alloc_object_frame` and backing already zero).
   - **Or a parallel `u32` array** beside `allocations`. That is one word per frame, which on
     rv64's physmap bound matters, so prefer the intrusive list unless it fights the zeroing.
   - Every path that sets an `allocations` entry to `None` must push the frame:
     - `free_frame_of`;
     - `free_object_frame`;
     - `free_contiguous`;
     - the release walks;
     - `action_inner`'s frees.

     Name them all in the report. A frame freed and not pushed is a leak the audit should
     catch: extend `check_frame_owners`, or add a checked-build audit, so that the free list and
     the table's `None` entries are exactly one set, once after a destruction, stamped as an
     audit (`sched::audit`).
2. **`release_owned_frames` without a RAM scan.**
   - A process that never ran was built by `process_create`, which knows what it allocated.
   - Release from the partial space's own tables, as an ended process is released
     (`release_all_memory_for_process`), or from a short record of what the creation took.
   - Its doc comment says the scan serves "a partially built space". Show what a partial space
     can hold, and walk that.
3. **`alloc_contiguous`.** Decide one design, and bring it back as a question if neither fits:
   - (a) a DMA pool of fixed size reserved at boot, from which runs are taken. Its search is
     linear in the pool, a fixed constant (as `map_anon`'s area is). This is probably simplest:
     `dma_alloc` is a driver's call, and the pool's size is the machine's DMA budget;
   - (b) a run allocator over the free list.

   Under (a), say what the pool's size is, why, and what a driver asking for more gets.
4. **Bench.**
   - Extend `scan-bounds`: after one budget fills 20,000 pages, time on both widths a
     one-page `map_anon`, a `budget_create` and a `process_create` that is rolled back, against
     the same with nothing filled. The bound is `scan-bounds`'s existing tolerance.
   - A recorded negative run with the first-fit scan restored behind a test-only feature
     (`alloc-first-fit`, off in every default build) must fail it on both widths.
   - `sched-budget-churn` is green on both widths with the trace ring at either size. Report the
     shares.
   - The model is unaffected (it charges runtime only). Say so.

## Owned paths

`kernel/src/mem.rs` (the allocator and its frees), `kernel/src/process.rs` (the rollback),
`kernel/src/dma.rs` (`dma_new_run`'s call only), the `scan-bounds` case and program, the feature
in kernel/Cargo.toml, scheduling.md's and memory.md's lines, and devices.md's `dma_alloc` if the
pool changes what a driver sees.

Hotspots:
- K16 (the tables) and GATE1 touch kernel files.
- `mem.rs`'s allocator is in neither today. Rebase onto whichever merges first.
- GATE1 moves the trace ring to the top of RAM through `kernel_frame`. Keep that working:
  `kernel_frame` takes from the top, the free list serves everything else.

## Gates

- The whole bench on both widths.
- The kernel's host tests.
- `cargo fmt --check`.
- The unsafe ratchet: the intrusive list goes through `kframe`, which is already counted, so it
  should need no new `unsafe`; if it does, give the reason.
- The size budget.
- doccheck.

The report says what was deleted. Report each command with its exit code.
