# K21 simplifier, round 3 (HEAD 55e1e73f7; the bitmap commit is f688e1530)
Verdict: OK with notes. All P2.

Deleted correctly: FreeList, NEXT/PREV, push_free/unlink_free/free_link, kframe free-frame use, check_free_list's walk + count scan. grep: no remnants in kernel/docs/tests/tools. No profiling instrumentation left (the only println in the diff is unrelated, init's).

1. keep: LEVELS from PHYSMAP_SIZE. Fixed depth is what makes take O(1) under R12; upper levels at small RAM are 1-2 words. A depth derived from RAM would be a runtime loop bound.
2. keep (note): level 0 repeats the table's None entries, by design: the table stays the truth, set_owner the sole writer, and summary() is one derivation shared by boot build and audit. Alternative (no level 0: a per-64-frame "has a None" summary, take scans <=64 table entries) deletes ~1 bit/frame and the level-0 equality but makes take/claim scan the table; bounded, yet slower and the audit no better. Not worth it.
3. trim: set_word has one caller (boot init loop). Write self.free.bits[..] there, or have summary-fill write through bits directly; deletes the pair's half. word() has 3 callers, keep.
4. trim (optional): find_free(highest) exists for kernel_frame, a sched-trace-only caller. Either #[cfg] the highest arm or take the lowest and accept the placement change; today it is 3 lines in the release build.
5. trim (optional): boot makes three passes (bitmap-run search, summary build, take_dma_pool's run search). Each is one-off; fusing buys little. Keep.
6. keep: mark_free is one path (early return when a word's zero-ness is unchanged); no second route to a free frame exists: the boot fill of the bitmap's own frames is the only table write outside set_owner and is documented and precedes the bitmap.
7. keep: check_free_frames vs existing audits: the old allocation audit checks owners against objects; this checks a derived index against the table, the same kind as check_object_indexes' other audits and runs inside it. Not a duplicate.
8. release_owned_frames: round 1's double-lookup trim done.
Size ceiling: kernel 7943->7983 (+40) for the bitmap (levels, sized, summary, audit, unsafe slice). mem.rs is 1263 lines vs 1058 on main (+205). A raise is avoidable only partly: items 3+4 save ~6-8 lines; the 40 is mostly real (comments + summary/sized). I would accept the raise but apply 3 and 4 first.
