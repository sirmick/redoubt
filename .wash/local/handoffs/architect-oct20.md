# Architect handoff (architect-oct20 → successor), 2026-10-21 checkpoint, ~70% context

## Branch state / traps (read first)
- I hold no branch and no worktree. I edit only `.wash/local/` files (append-only to the rulings file, briefs) and add plan nodes that have not started. Nothing is lost if this process ends.
- TRAPS:
  1. Messages, QA bodies and plan node bodies are capped at 2000 bytes, titles at 200. I overran four times this session; trim before sending.
  2. plan_set is atomic. A call that also edits a started node (e.g. B106) is refused whole, and new nodes in the same call are not created. Put new nodes in their own call. Started nodes are the orchestrator's to edit; ask.
  3. Don't report a node added until plan_set succeeded. I once told the orchestrator K36 existed before the retry succeeded.
  4. An implementer's key may be inactive when you reply ("not an active workspace member"). Route through the orchestrator on the thread, asking it to relay.
  5. plan.toml is rewritten by agentd. Read node bodies with grep; never plan_get (about 115 KB).
  6. Check that a node is still open before folding work into it. I folded into B102 after it had merged and had to re-route the stale counts to B104.
- Rulings file: `.wash/local/wave-2026-10-18-rulings.md`. All of this session's sections are appended at its end, from "Owner's answers 2026-10-20 → WEIGHT1, IPD2" onward. Append; never rewrite.

## Kernel chain, in order
K36 → B106 → SCHED5 (held on B106) → K34 (raised priority).
- B103 needs SCHED5.
- K35 (per-packet IPIs) and B101 are independent, but K35 starts from B101's reported per-probe cost.
- INIT6 is independent (todo).

## Open threads and what each waits on
- **K36-survey** (no thread yet). k36-implementer's survey comes to me before any code. Decide from it whether every sweep needs dead entries or only endpoint and process sweeps, which holders must close dead handles, and the steward's identity guards (keep or simplify). Ruling: rulings file, "B106 review (sched5-red P1-1)".
- **B106-shape**. Ruled shape (1): a served child's endpoint is owned by the child's budget, preferring (a) endpoint_create with an optional budget handle. Then the red's P1-1 (a swept index is reused, so a call gets BadHandle) became K36. B106 needs K36 (the orchestrator set this). P2-1 (split R9EndpointInAnyBudget) folds into B106; P3-1 closes with K36. Sections "B106-shape" and "B106 review (sched5-red P1-1)".
- **SCHED5-cause**. Long thread. Rulings, all under "SCHED5:" sections:
  - per-call cap MAX_RANGE_PAGES = 16 (derived from one slice);
  - rt heap large-block placement: one filler chunk, one-pass termination;
  - map_anon's search is a named exception (worst 114 ms) and K34 removes it;
  - map-anon-search-bound re-derived from the densest buildable shape (45 ms);
  - the hammer must cover the window (coverage check); the contention family plus irq-boot-hart-only record until K34;
  - rv32 gated at [1,2]; the deadline-notice cell recorded and given to B103;
  - device dma_alloc and map_device stated as an exception (drivers' open() only);
  - process_map from untouched reservations stated as an overrun (loader's first process only).
  SCHED5 is "reported", held on B106 (pipe-sixty-four rv32).
- **B101-cause**. Ruled (i), wake only the next ticket's hart. (ii), wake_idle after release, was withdrawn because it failed timer-flood-at-rest (668/666 against 20). R78 keeps immediate wakes, with the price stated. Waiting: (i) verified alone, plus launch-idle in 12 alternating alone rounds, testbench.md stating the measured range and the ceiling's origin honestly (1.6-1.9 does not reproduce; 2.1-2.5), and B94's per-probe cost reported for K35. Sections "B101-cause" and "B101-cause, correction".
- **B98-cause**. Done: bench disks and launch cases run cache=unsafe, ./launch keeps writeback, blkd's "lie is permanent" stands. launch-idle's load sensitivity was added to B101's body.
- **B105**. Node only, todo, Tier B: beamlet panics on one process's 3 MiB allocation; the rule is a pre-allocation check, killing only the process. Not launched.
- **beamlet-boot rv32 restart loop**. 1 failure in 8 load passes once, then 0 in about 300 runs since. A disk timeout is excluded (2.8 s against blkd's 10 s). It waits for the next whole bench (consoles kept). If it reproduces: replay means a Tier B case fix; a fast exit 1 means a node.
- steward-home-quota eio: explained by B95 and B98; the verification showed no client-side node.

## Nodes I added this session
WEIGHT1, IPD2, INIT6 (re-scoped to "Tversion ends the mux session"), B98, B99, B101, B103, B105, K34, K35, K36. Briefs: `.wash/local/WEIGHT1-implementer.md`, `IPD2-implementer.md`, `INIT6-implementer.md`. B104's body gained the testbench.md boot-case recount (351 boot cases, 187 under icount).

## Pending drafts
None owed, beyond reading K36's survey when it arrives.

## Principles applied (consistent across rulings)
- No call a process can make holds the kernel longer than about a slice. Exceptions are stated only for trusted callers who can't be hostile (drivers, the loader's first process).
- Targets and ceilings never move for a measurement. A host-clock case's miss beside load is no verdict; misses alone are.
- A negative control that passes proves nothing: change the load until the pair separates, or state why it can't.
- Bench-only host artefacts (fsync) belong to the bench, not the box's rules.
- Prefer the book's own rule (launcher-owned resources, ownership) over a fence that must prove a negative.
