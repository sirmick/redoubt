# SMP4: per-hart free-frame magazines, zeroed on entry

Owner's ask (2026-10-05): take slow work out of the kernel lock's critical section ("lock to
decide, unlock to do") and give each hart free-frame state it reaches without the lock. Scoped
here to the one clean form of both: a per-hart magazine whose frames are zeroed as they enter
it. Tier A, size S-M. Needs SMP1 merged: its per-hart block, its lock, `--smp 2`.

## Rules

1. **`PerHart<T>`.** Lives in SMP1's per-hart block, reached through `sscratch` (document the
   choice in memory-layout.md beside the block; the kernel loads `tp` from it for cheap access).
   Reached only through `get(&self, _: &mut IrqsOff) -> &mut T`, where `IrqsOff` is a `!Send`
   zero-sized token made once at trap entry (interrupts are off in the kernel) and never kept
   into the idle loop, which enables `SIE`. The `&mut` borrow of the token is what makes the
   returned `&mut T` unique: a shared `&IrqsOff` would hand out two live `&mut T`. No atomics.
2. **The magazine.** Fixed capacity, 16 frames, per hart, in front of `MemoryManager`'s
   free-frame bitmap. `alloc_frame` pops locally; empty, it refills a batch of 8 under the lock.
   Frees push locally; full, it flushes 8 under the lock. Refill and flush are a constant number
   of bitmap steps (R12's bound).
3. **Zeroed on entry.** A frame is zeroed (`kframe::zero`, through the physmap) as it enters the
   magazine, after the refill's lock section ends, and a freed frame is zeroed before it is
   pushed. So hand-out installs an already zeroed frame and `map_run`, `map_fixed` and
   `alloc_object_frame` zero nothing under the lock. R11 gains the invariant: a frame is zeroed
   before any translation to it exists, and between its allocation and its zeroing no hart and
   no process can name it. What keeps it: a magazine frame has no user entry (it was freed or
   never mapped), the ownership table marks it as the magazine's, and the magazine is reached
   only through its own hart's `IrqsOff`. Name those three in the page.
4. **Ownership and charging unchanged.** A magazine frame's table entry is a sentinel owner,
   `MAGAZINE_OWNER`, as `DMA_OWNER` marks the DMA pool. So the bitmap stays exactly the table's
   `None` entries and `check_free_frames` reads no magazine; `check_frame_owners` checks that
   sentinel frames are uncharged and number at most `MAX_HARTS` x 16. Budgets are charged at
   hand-out (`alloc_page`), exactly as today.
5. **`root` is held back.** `boot_budgets` keeps `MAX_HARTS` x 16 frames out of `root`'s pages,
   so a budget's free pages are still backed by frames in the bitmap: `map_fixed`'s
   `alloc_page(...).expect` and `process_map`'s rest on that (mem.rs, the comment above
   `map_fixed`'s loop). Say the number in budgets.md's boot paragraph.
6. **One hart: no magazine.** A one-hart boot allocates from the bitmap as today, zeroing at
   allocation as today, so allocation order and every existing case are unchanged
   (`scan-bounds`, the trace ring's placement, `smp-evict`'s "maps until it gets B's frame").
7. **DMA runs and page tables** are not from the magazine (contiguous runs; tables are zeroed by
   `arch/riscv/mem.rs`'s own path). List them in the report as left under the lock, with why.
8. **Lock order.** Write every order this touches beside mem.rs's note ("`ProcessTable` before
   `MemoryManager`"); the magazine takes no lock, so there should be none new.
9. **The run queue is not split.** Write a short note in docs/beyond/scheduling-extensions.md:
   per-hart run queues with stealing against R12's single stride order; what breaks (the floor,
   the pass order, the share proof), what a correct version needs, whether it is worth it at
   4-8 harts (SMP2's water-filling rule is the comparison).

## Cases (both widths, checked build)

1. **`smp-magazine`**, at 2 harts: a budget B's spinner allocates and frees pages without end on
   one hart; the other hart destroys B; a checker in a new budget maps until it has every frame
   RAM can give and watches them all zero; the audit passes. A recorded negative, a test-only
   feature that skips the zero on entry, must fail.
2. The whole bench at 1 hart: every case unchanged. The whole bench at `--smp 2`.
3. Hold-time evidence: SMP1's debug counter, before and after, for `map_anon` and `map_fixed`.

## Pages

memory.md R11 (the invariant, rule 3), memory-layout.md (the per-hart block and `PerHart`),
budgets.md (the hold-back), docs/beyond/scheduling-extensions.md (rule 9), docs/SECURITY.md's
R11 row if its Enforced-in list changes.

## Constraints

No `target_pointer_width` outside the allowed places; rv32 compiles. Every test through
`cargo testbench`. The unsafe ratchet: `PerHart::get` adds `unsafe`; give its reason.
