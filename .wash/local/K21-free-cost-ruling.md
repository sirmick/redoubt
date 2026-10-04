# K21-free-cost: ruling (architect-8)

**Take (4), a bitmap of free frames with fixed-depth summaries, before K21 merges.** Not (a),
not (2).

Why:
- (a) passes the gate with 1.4-1.7 ms of margin, and K16 (more frames a destruction) and SMP
  (the lock) are next on R10's path. Its p50 is 6 ms above main's.
- (2) still writes every freed frame once (~0.43 µs each, so ~1.8 ms a destruction at the gate),
  and keeps kernel links inside free frames.
- (4) writes no frame at all. A free sets a bit in a dense table in kernel memory: about 4,270
  frees touch tens of words. It also removes the hazard instead of fencing it: a free frame holds
  nothing of the kernel's, so a stale mapping on another hart can reach only its next owner's data,
  never the allocator. memory.md's "first two words are the kernel's" residual goes, and so does
  the M2 line held since K21's red review.

## The design

1. **Levels.**
   - Level 0 has one bit a RAM frame: set exactly when the ownership table's entry is `None`.
     `set_owner` stays the one writer of both, as it is of the list today.
   - Each higher level has one bit per word of the level below, set exactly when that word is
     not zero. The levels go up to a single word.
2. **Depth is a constant.** The number of levels is fixed by the most RAM the kernel can map
   (`PHYSMAP_SIZE`: 128 GiB on Sv39, 2032 MiB on Sv32), so 64-ary summaries give at most a few
   levels. Compute them, put them in a named constant with the arithmetic, and assert at boot
   that RAM fits. The levels' sizes follow the actual RAM.
3. **Take** is the lowest free frame: descend from the top word by `trailing_zeros`, one word a
   level. The trace build's `kernel_frame` takes the highest, by `leading_zeros`. That keeps
   "the frames below sit where a release kernel's do".
4. **Free, and a claim by address**: set or clear the frame's bit, then go up only while a word
   changes between zero and non-zero. At most one word a level.
5. **The bitmap's own memory** (RAM/8 bytes for level 0, about 1/63 more for the summaries) is
   taken at boot, as the DMA pool is, and counted as kernel-held in `boot_budgets` (state where).
   The boot builds it in its one scan of the table.
6. **No frame is written by a free or a take.** Zeroing stays where it is (on taking).
7. **The checked build's audit**, in the stamped audit after a destruction: level 0 equals the
   table's `None` entries, every summary bit equals its word being non-zero, and the DMA pool's
   frames are `DMA_OWNER`'s. It replaces `check_free_list`.
8. **`alloc-first-fit`** stays the recorded negative for `scan-bounds`: "the first-fit scan of
   RAM the bitmap replaced".

Re-measure at seed 13, both widths: R10 p50 and p99 against main's (~18.7 / ~22.4 ms), and the
free time a destruction from the scratch instrumentation. If p99 is still more than 2 ms above
main's, report before merging.

Rebase on main first: main 082e00ccf rewrote m2-usable-shell.md's step 4. Keep main's step 4,
and drop K21's edit of it.

## Page lines

- **memory.md, the free-list paragraph** ("A frame is taken from the list ... linked both ways."):
  > A free frame is a set bit in a bitmap the kernel keeps, with a summary bit for every word of
  > it, level above level, to a fixed depth set by the most RAM the kernel can map. Taking the
  > lowest free frame reads one word a level, and giving one back sets its bit and at most one
  > word a level, so neither searches RAM, however much of it is in use (`bench:scan-bounds`). A
  > checked build proves after each destruction that the bitmap is exactly the free frames and
  > every summary is exact.
- **memory.md**: "A RAM frame is taken from a free list and given back to it, so backing a page,
  a page table or an object searches nothing" becomes "A RAM frame is taken from the free-frame
  bitmap and given back to it, so backing a page, a page table or an object searches nothing".
- **memory.md, the residual "A free frame's first two words are the kernel's"**, replaced by:
  > - **A freed frame may still be mapped on another hart.** A free writes nothing into the
  >   frame, so a stale mapping could reach only its next owner's data, never the kernel. Every
  >   path that frees a mapped frame unmaps it and flushes the TLB first, or frees an ended
  >   process's frames, which nothing runs again; on one hart that leaves no stale mapping.
  >   Several harts need the TLB shootdown before the free
  >   (M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts)).
- **memory.md**, "Freeing costs only the frame's place in the free list" becomes "Freeing costs
  only the frame's bit".
- **scheduling.md (R12)** and **testbench.md**: "taken from a free list and given back to it"
  becomes "taken from the free-frame bitmap and given back to it"; "the first-fit scan of RAM the
  free list replaced" becomes "... the bitmap replaced".
- The figure's caption is unchanged.

Not an owner choice: R12's text forbids a term in RAM frames and permits one in a fixed kernel
constant; the depth is one.
