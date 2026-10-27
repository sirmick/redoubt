<!-- wash-qa-thread: SMP5-magazine-saving -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"SMP5-magazine-saving","node":"SMP5","title":"SMP5 design: magazines save no lock time unless the fill's zeroing leaves the lock","creator":"212d2b3d2b4a9436b65ff7c67edc3b03","assignee":"f8d338ac800f4250e2e6e4e78f6f7b15","state":"open","blocking":true,"revision":6,"decision_refs":null,"events":[{"id":"5486b578b4dc8a5a97f2d926fe42f6a2","author":"212d2b3d2b4a9436b65ff7c67edc3b03","kind":"open","body":"SMP5 design checkpoint: ./.wash/local/SMP5-design.md (no code yet, on main 58326f13c).\n\nFinding: every trap from user mode takes the one kernel lock (irq.rs:158), so every allocation already holds it. A magazine \"drawn without the lock\" saves only the bitmap walk, under 1 µs. The containment gate's per-allocation lock time is zeroing never-used frames, ~30-44 µs each. If the fill zeroes under the lock, as step 6 and the assignment say, that zeroing only moves into the fill. Predicted saving on that gate: none.\n\nQ1 (blocking, Architect): which option?\n- Option P: the plan as written, with a stated nil saving.\n- Option X (recommended): at a system call's return, after the lock is released, the hart zeroes the frames that replace what its caller just drew (at most 2, its own pending frames first, then the bitmap's), billed to that caller. They enter the magazine zero, under the lock, at its next entry. This amends R81's \"never zero at an exit to user mode\" for frames the caller took. Predicted: endpoint_create sections -300..-440 ticks each, ~2-3 s of ~225 s net lock hold (~1%). The 900 M-tick hand-off is untouched. The in-flight high-water (79.5k) should fall to the low thousands, since the refill drains pending frames under load.\n\nCommon to both:\n- Owner `MAGAZINE` (0xfffc) plus per-hart arrays, MAG_MAX 16.\n- Drawn under the lock: magazine, then the bitmap, then reclaim_wait, which now first takes from another hart's magazine. That take is the \"return\", since harts never stop (Q3).\n- Audit: MAGAZINE count against the arrays; the partition bitmap ∪ in flight ∪ magazines ∪ owned = RAM.\n- R82, with mutations R82MagazineUnzeroed and R82MagazineStillFree. No new RNG draws: refill rides Event::Commit.\n- New case smp-magazine-race (Q4) with two recorded negatives, plus a report-only per-cause hold line.\n\nQ2: option X needs one call line in arch/riscv/syscall.rs (return_registers), which is outside my owned paths.\n\nWaiting for the go.","message_id":"5de1953c30eb740f203fb4c9715078d5","created_at":1791605040648},{"id":"c368b0cb4c8e920ea443ae80ddfe90d2","author":"da644642022a19c5c33e271e726084f1","kind":"answer","body":"Orchestrator's ruling (the Architect is blocked on an owner decision; recorded in .wash/local/architect-questions.md 'SMP5' for its review): Option X, with conditions. Your finding stands and goes on the page first: under SMP1's one kernel lock every trap holds the lock, so a magazine drawn 'without the lock' saves only the bitmap walk; the lock-free allocation step 6 promised waits for the lock's split (say so on m2-usable-shell.md step 6 and memory.md: a later node, not this one). X amends R81's 'no zeroing at an exit' narrowly: after a system call releases the lock, the hart zeroes at most the frames its own caller just drew (\u003c= 2: its pending first, then the bitmap's), billed to that caller, and they enter the magazine zeroed under the lock at the next entry. That keeps the (A) ruling's reason intact (the exit cost falls on the caller who allocated, never on the next runner: state that sentence), bounded by 2 frames (~60-90 us) per return. Conditions: (1) rule id R84, not R82 (R82 and R83 are CTX3's); mutations R84MagazineUnzeroed and R84MagazineStillFree; (2) the one call line in arch/riscv/syscall.rs return_registers is allowed (name it in the commit), nothing else there; (3) the measured saving on the containment gate goes on the page as measured, even if ~1 % of lock hold, with the in-flight high-water before/after (your prediction: 79.5k -\u003e low thousands); (4) the deadline-notice p99 and R10 p99 must not move beyond their bounds and are reported; (5) the 'return' by another hart's reclaim_wait taking from a magazine is under the lock: say the audit's partition holds across it; (6) MAG_MAX 16 and the \u003c= 2 per return are constants with their reasons beside them. Go.","message_id":"dcee581c3048c9f9c9b31f3a8b2a266c","created_at":1791605070550},{"id":"f0466d007d67b8f3747f51a05bc67e29","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"cadf4bccd39a6a2f09ed9d05fb4ec970","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"e8402c096eefc6bd842d9efe40c0d73e","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"fbbb59ca969e1755fc979c21593ed977","author":"f8d338ac800f4250e2e6e4e78f6f7b15","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1793128224906}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","212d2b3d2b4a9436b65ff7c67edc3b03":"Implementer","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator","f8d338ac800f4250e2e6e4e78f6f7b15":"Orchestrator"}} -->

## SMP5 · SMP5-magazine-saving — SMP5 design: magazines save no lock time unless the fill's zeroing leaves the lock

Status: **open** · Assigned to: Orchestrator · Revision: 6

**Blocks work on its node.**

### Implementer · Question

Event: `5486b578b4dc8a5a97f2d926fe42f6a2` · 2026-10-10T04:04:00Z

> SMP5 design checkpoint: ./.wash/local/SMP5-design.md (no code yet, on main 58326f13c).
> 
> Finding: every trap from user mode takes the one kernel lock (irq.rs:158), so every allocation already holds it. A magazine "drawn without the lock" saves only the bitmap walk, under 1 µs. The containment gate's per-allocation lock time is zeroing never-used frames, ~30-44 µs each. If the fill zeroes under the lock, as step 6 and the assignment say, that zeroing only moves into the fill. Predicted saving on that gate: none.
> 
> Q1 (blocking, Architect): which option?
> - Option P: the plan as written, with a stated nil saving.
> - Option X (recommended): at a system call's return, after the lock is released, the hart zeroes the frames that replace what its caller just drew (at most 2, its own pending frames first, then the bitmap's), billed to that caller. They enter the magazine zero, under the lock, at its next entry. This amends R81's "never zero at an exit to user mode" for frames the caller took. Predicted: endpoint_create sections -300..-440 ticks each, ~2-3 s of ~225 s net lock hold (~1%). The 900 M-tick hand-off is untouched. The in-flight high-water (79.5k) should fall to the low thousands, since the refill drains pending frames under load.
> 
> Common to both:
> - Owner `MAGAZINE` (0xfffc) plus per-hart arrays, MAG_MAX 16.
> - Drawn under the lock: magazine, then the bitmap, then reclaim_wait, which now first takes from another hart's magazine. That take is the "return", since harts never stop (Q3).
> - Audit: MAGAZINE count against the arrays; the partition bitmap ∪ in flight ∪ magazines ∪ owned = RAM.
> - R82, with mutations R82MagazineUnzeroed and R82MagazineStillFree. No new RNG draws: refill rides Event::Commit.
> - New case smp-magazine-race (Q4) with two recorded negatives, plus a report-only per-cause hold line.
> 
> Q2: option X needs one call line in arch/riscv/syscall.rs (return_registers), which is outside my owned paths.
> 
> Waiting for the go.

### Orchestrator · Answer

Event: `c368b0cb4c8e920ea443ae80ddfe90d2` · 2026-10-10T04:04:30Z

> Orchestrator's ruling (the Architect is blocked on an owner decision; recorded in .wash/local/architect-questions.md 'SMP5' for its review): Option X, with conditions. Your finding stands and goes on the page first: under SMP1's one kernel lock every trap holds the lock, so a magazine drawn 'without the lock' saves only the bitmap walk; the lock-free allocation step 6 promised waits for the lock's split (say so on m2-usable-shell.md step 6 and memory.md: a later node, not this one). X amends R81's 'no zeroing at an exit' narrowly: after a system call releases the lock, the hart zeroes at most the frames its own caller just drew (<= 2: its pending first, then the bitmap's), billed to that caller, and they enter the magazine zeroed under the lock at the next entry. That keeps the (A) ruling's reason intact (the exit cost falls on the caller who allocated, never on the next runner: state that sentence), bounded by 2 frames (~60-90 us) per return. Conditions: (1) rule id R84, not R82 (R82 and R83 are CTX3's); mutations R84MagazineUnzeroed and R84MagazineStillFree; (2) the one call line in arch/riscv/syscall.rs return_registers is allowed (name it in the commit), nothing else there; (3) the measured saving on the containment gate goes on the page as measured, even if ~1 % of lock hold, with the in-flight high-water before/after (your prediction: 79.5k -> low thousands); (4) the deadline-notice p99 and R10 p99 must not move beyond their bounds and are reported; (5) the 'return' by another hart's reclaim_wait taking from a magazine is under the lock: say the audit's partition holds across it; (6) MAG_MAX 16 and the <= 2 per return are constants with their reasons beside them. Go.

### Orchestrator · Assigned

Event: `f0466d007d67b8f3747f51a05bc67e29` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `cadf4bccd39a6a2f09ed9d05fb4ec970` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `e8402c096eefc6bd842d9efe40c0d73e` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `fbbb59ca969e1755fc979c21593ed977` · 2026-10-27T19:10:24Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator
