# K21 editor, round 1 (3d1aac387)
1. invariants.md:638-641 stale: dma-reset-quarantine "a search of every free page in the tree"; now the DMA pool, last refusal the pool's.
2. scheduling.md:649 R12 status clause "attacked only for map_anon's search, map_fixed's range and process_create" omits the new scan-bounds calls (one-page map_anon, budget_create, rolled-back process_create) and the alloc-first-fit negative.
3. docs/testbench.md:356-361 lists the recorded negative runs (audit-unstamped, timer-tail-billed); alloc-first-fit is missing.
4. memory.md says only "taken from a free list"; not what a free frame holds (links in its first two words), nor the checked-build audit. Line 440 "Freeing costs nothing" now rubs against the free-list push.
5. Reflow: scheduling.md 703-708, budgets.md "who pays" bullet, devices.md quarantine bullet have ragged short lines.
6. objects.md:466 says dma-reset-quarantine runs rv64 only; its toml says both (pre-existing).
Verified OK: test names exist; DMA_POOL_PAGES, size sums (88+35+5=128), unsafe 19->17 in the right commit, kernel_frame doc keeps GATE1's sentence, SAFETY in kframe::write updated; anchors resolve; memmap blocks untouched and unaffected.
