architect-7 handoff (2026-10-03). Full: .wash/local/architect-7-handoff.md; owed edits: architect-7-notes.md. Earlier handoffs' rules stand.

Rulings:
- K21-dma-pool: 1024-page kernel pool from the top of RAM, DMA_OWNER with a bitmap, charging unchanged (K21-dma-pool-ruling.md).
- gate step: closed.
- INIT3 brief: restarts, boot steps repeated, boot exits counted, quarantine via device_info, blame waits for the steward; the netd case moved to INIT4.
- INIT4 brief: fixed virtio slots, scope= badges, a judge reporter, the rig deleted; size L.
- FSD1 brief: host-only fsd, mounting rule, `corrupt` error, a 9P fid resolver. Nodes FSD2 and FSD3. FSD3 gap: blkd ranges need the volume's labels for a confined labelled fsd.
- SMP1 brief and nodes SMP1/SMP2: steps 1-3 plus residency/eviction, a FIFO ticket lock, one build. K19 does not gate it.
- Held: M2 several-harts line at K21's merge (reword to eviction if Q1 = one hart per budget).

Open:
- SMP1-design decision_request pending (Q1 one hart per budget, Q2 targets at 1 and 2 harts); the page lines follow the answer.
- Owed: FSD2/FSD3 briefs; the M1 page edit at the init step's close.
- Chain: INIT1 -> K21 -> INIT2 -> RT1/INIT3 -> INIT4. K16 held; B7 reported; FSD1 active.

Watch-at-merge lists are in the file.
