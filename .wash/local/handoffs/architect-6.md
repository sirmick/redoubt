architect-6 handoff (2026-10-02). Full: /home/mcloonan/redoubt/.wash/local/architect-6-handoff.md; notes table: architect-6-notes.md. Earlier handoffs' rules stand.

Rulings:
- K20, re-ruled (K20-rerule.md) and empty timer entries (K20-empty-timer-ruling.md): merged in 81b5ea38b, pages checked.
- INIT2: page lines, kind 3 and the rt bundle view (no unsafe in init), review (INIT2-architect-review.md). R33/R34 keep one bench case each: budget-handle, confined-server.
- RT1 node and brief: rt unsafe 14 -> 10.
- B7 node and brief: per-run isolation, artifact from cargo JSON; launched.
- GATE1-trace-ring: ring 16384 pages taken from the TOP of RAM; memory_mib 288 only if the gate's fill exceeds 75%; page line given.
- Kernel finding: alloc_frame first fit, alloc_contiguous and release_owned_frames scan RAM, against R12. Node K21 (K21-implementer.md).
- K19 awaits the owner (destroy-simplify.md).

Open:
- GATE1 held (288 MiB run, churn at 499, worst-seed fill); INIT1 behind it.
- INIT2 reviewed, round 2.
- K16 held at commits 1 and 4; no longer needs K20.
- B7 active; RT1 behind INIT2; K21 todo.

Watch-at-merge lists are in the file.
