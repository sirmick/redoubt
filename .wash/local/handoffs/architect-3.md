Architect handoff (architect-3, 2026-10-02). Read .wash/local/architect-handoff.md and architect-2-handoff.md first; their rulings stand except as below. Working rules as before: pages on main, stage by path, run doccheck (./dev.sh bash -c 'cd /work && cargo run -q -p redoubt-doccheck') before every commit, QA bodies <= 2000 bytes, no thread names or dates on pages. A steward-style page allows only the fixed ## headings (C9); put new sections under Interface as ###.

RULINGS AND REFS TODAY
- K16-limits (resolved). Owner's values: MAX_PROCESS_COUNT 512 with 16-bit PIDs; MAX_THREADS 255, each thread's saved registers in its own IPC page; MAX_OPEN_CALLS 256, WAIT_CAP 32, MAX_DEPTH 16, MAX_LABELS 16, MAX_START_HANDLES 128. Brief: .wash/local/K16-implementer.md, rulings 1-9.
  - Pid becomes NonZeroU16; the ASID goes out of satp (ASID 0); the loader handoff gets a PID field.
  - Walks visit live threads only (commit 1, measured against GATE1).
  - PROCESS_AREA is one page: header, a no-thread context, and the TID->IPC-frame table. The trap entry loads the context address from header slot 1; it does not index the table (my note to the orchestrator).
  - Per-PID tables go to .bss (~375 KB/width at 512). If the 512 KiB data region keeps less than 64 KiB free, the implementer stops and the Architect grows the region on memory-layout.md.
  - K16 needs GATE1, INIT1, K17. No page changes before its code.
- K13-doomed-takes-call (resolved), 94ab7073d: docs/todo/destruction-feeds-the-doomed.md, an ipc.md residual (last bullet), the SUMMARY line. Rule: a thread of a process whose budget is dying takes nothing from a pump (process_is_doomed exists). The reachable shape is the exit notice; the call shape is argued from code. Cut as K17.
- K17 brief: .wash/local/K17-implementer.md. Case destroy-keeps-notices, ordered on events (a wake never preempts), not wall clock.
- STEWARD0-design (answered), 99d01496f: steward.md "The policy core", plus gap rules in Leases, Declassification and push, Authority, Why, and a residual. Brief: .wash/local/STEWARD0-implementer.md. Notes and next steps: .wash/local/STEWARD0-notes.md. Plan: STEWARD0 (L, needs nothing), STEWARD1 (Elixir oracle + bench Elixir-on-beamlet case, M, needs STEWARD0); the steward step needs STEWARD1.

NEXT JOBS
- STEWARD0's checkpoint: review its tables before any code (checklist in the notes file).
- Write STEWARD1's brief near STEWARD0's end.

OPEN AND UNRULED
- INIT2: init's own usage check against INIT_PAGES (1,024) and the boot refusal; nothing ruled.
- From architect-2, still at INIT1's merge: devices.md stale lines, boot.md built vs planned, budgets.md "Root, system and users" vs "The tree from the boot manifest", testbench.md "Starting a case's programs" to built and the Gifts/TAKE_GIFTS paragraph out.
- B4 is done: check SUMMARY kept parked-write-clock.md (it did, as of 94ab7073d).

STALE OR FRAGILE PAGES
- scheduling.md Responsiveness sweep tables read as history.
- steward.md: the wire table libs/wire/tables/steward.md does not exist, though the page says it is included; it is the steward step's.
- ipc.md residuals: K17 removes the last bullet; the "Delivery walks every thread" bullet is K16's.
- memory-layout.md satp section ("PIDs fit every ASID width, because a PID is a byte") goes with K16.
- objects.md cost table (saved contexts row) goes with K16.

BRIEFS IN .wash/local: K16-implementer.md, K17-implementer.md, STEWARD0-implementer.md, STEWARD0-notes.md; questions K16-question.md, STEWARD0-question.md.
