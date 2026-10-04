# K21 simplifier, round 1 (3d1aac387)
Verdict: OK with notes. All P2, none blocks.

1. keep: PREV word. Claim by address (process_map/page actions via set_owner None->Some) must unlink mid-list; singly linked would walk the list, a search of RAM again. Two words cost nothing (inside free frames).
2. keep: set_owner is the single writer; no forwarding wrapper left. Only other table write is extra_allocations (outside the list, correct). free_link/set_free_link could fold into one pair taking Option, trivial; trim optional.
3. keep: DmaPool bitmap. Reusing ownership states needs a second pseudo-pid (DMA_FREE) and every is-dma check testing two; 128 B + ~15 lines is cheaper. Alt only if the pool grows.
4. trim (optional): check_free_list is debug-only and once per destruction. The list walk checks owned-on-list and back links; the full-RAM `filter(None).count()` is the only part catching free-but-unlisted, which set_owner makes impossible by construction except via a stray direct write. Cheaper: drop the scan, keep walk + len + tail; or grep-test that allocations[..]= appears once. Keep if 70 ms is acceptable in the checked build.
5. delete/trim: layout constant is 4 lines (DMA_POOL_PAGES), not 61; right place (both widths, kernel + budget use it). Keep.
6. keep: alloc-first-fit feature is the second alloc_frame (~8 lines) for the recorded negative run; not dead code.
7. trim: boot does two scans (push_free loop, then take_dma_pool's reverse run search). Acceptable (one-off, boot); could fuse, not worth it.
8. Kernel +128 net is mostly doc comments and release_owned_frames' ram/extra index double lookup (extra_index called twice; `expect("found above")`). Trim: compute the index enum once (Option<(bool,usize)>) and drop the expect.
Deletions confirmed: three scans (alloc_contiguous, old release_owned_frames, rposition kernel_frame), free_contiguous, release_all_memory_for_process.
