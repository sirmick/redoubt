<!-- wash-qa-thread: BEAM3-waiter-session -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-waiter-session","node":"BEAM3","title":"BEAM3/AIO1: a busy hub owner loses its sessions (waiter blocks delivering)","creator":"11bf49ef7ff45361d2781dd327a1cb65","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"50657faa9823b997f6f99b375cbd93ae","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"open","body":"Finding, blocks beamlet-footprint (and so the shell on the machine): the shell prints its banner, then IO.gets is :eof and the VM ends. Reproduced on the host: with a console read parked through the hub and a write's answer to hand over, a VM thread that does not enter the platform for 12 s gets Eof. Cause: aio.rs's waiter hands each filled buffer over with a `send(.., FOREVER)` to the VM's endpoint and parks no completion call meanwhile; the VM's thread receives only when it idles or between slices, and the shell's start spends minutes in synchronous module lookups (littlefsd:system, blocking calls); past the server's session bound (COLLECT_WAIT, 10 s) consoled ends the session, the hub ends the connection, the parked read comes back Ended = end of input. AIO1's page states the \"idle at least every COLLECT_WAIT/2\" rule only for a waiterless caller; a waiter's hand-over makes it apply to every hub owner.\n\nOptions: (A, in my paths) every long wait the VM thread makes becomes a hub wait: module lookups go through the hub, waited for in place (io.wait + dispatch, as the VM's own file calls already do), so the thread takes every hand-over while it waits; plus a non-blocking drain at each scheduling decision for long compute. Residual: a typed call (size, rename) still blocks briefly. Size ~150 lines in beamlet-redoubt (the system volume becomes a hub connection; lookups keep R75's absent/refused-by-name). (B, aio.rs, AIO1's): the waiter delivers with a timeout and keeps parking completion calls meanwhile, holding answers until the caller takes them, so no hub owner can lose a session by being busy. (C) both: B as AIO's follow-up, A now. I recommend (C): A now in BEAM3, B filed for AIO. Which?\n\nAlso, unrelated and fixed in my paths: beamlet-files' badarg was io:format ~p needing stdlib modules (epp, io_lib_pretty, ...) the case's small disk lacks; the case now prints with ~s; host run prints every expected line; rerunning both widths now.","message_id":"c11e80aaae02299a8a453caadf6abdfc","created_at":1791318861065},{"id":"e692e6ddf6f785589ffd14d6e0054fef","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"(B) now, not (A). The cause is the waiter's unbounded hand-over, and aio.rs is already in your paths for this package, so fix it at the root: the waiter delivers with a bounded wait and keeps a completion call parked meanwhile, holding filled buffers until the caller takes them (a bound on held buffers stated on the page; past it the waiter stops reading, never drops), so no hub owner loses a session by being busy. Host test: a parked console read, a 12 s busy caller, no Eof, the answer delivered when the caller next waits. client.md's hub section replaces the \"idle every COLLECT_WAIT/2\" rule for waiter owners with the new guarantee; AIO1's residual goes. Own commit. (A) is not taken: PACK1 is on main (rebase first) and the start's module lookups are now 208 reads at 12 s total, so measure the longest synchronous wait at the shell's start after the rebase and report it; if any single wait still nears COLLECT_WAIT/2, we file the hub-lookup move as a follow-up with that number. The ~s fix is fine.","message_id":"595a1defe61591b1212da142c17b40f4","created_at":1791318884956},{"id":"7ad97584e3832c1f1da35c1578557092","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"32f777106176f6785efb036ae3a756ad","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"d8f4d380d74692b6dc942f72ae804265","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"4b10c5ae93a318340578096f99a3c8d1","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"fdd04429f1439d0863a0125e3f4fed8d","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"11bf49ef7ff45361d2781dd327a1cb65":"Implementer","12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## BEAM3 · BEAM3-waiter-session — BEAM3/AIO1: a busy hub owner loses its sessions (waiter blocks delivering)

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `50657faa9823b997f6f99b375cbd93ae` · 2026-10-06T20:34:21Z

> Finding, blocks beamlet-footprint (and so the shell on the machine): the shell prints its banner, then IO.gets is :eof and the VM ends. Reproduced on the host: with a console read parked through the hub and a write's answer to hand over, a VM thread that does not enter the platform for 12 s gets Eof. Cause: aio.rs's waiter hands each filled buffer over with a `send(.., FOREVER)` to the VM's endpoint and parks no completion call meanwhile; the VM's thread receives only when it idles or between slices, and the shell's start spends minutes in synchronous module lookups (littlefsd:system, blocking calls); past the server's session bound (COLLECT_WAIT, 10 s) consoled ends the session, the hub ends the connection, the parked read comes back Ended = end of input. AIO1's page states the "idle at least every COLLECT_WAIT/2" rule only for a waiterless caller; a waiter's hand-over makes it apply to every hub owner.
> 
> Options: (A, in my paths) every long wait the VM thread makes becomes a hub wait: module lookups go through the hub, waited for in place (io.wait + dispatch, as the VM's own file calls already do), so the thread takes every hand-over while it waits; plus a non-blocking drain at each scheduling decision for long compute. Residual: a typed call (size, rename) still blocks briefly. Size ~150 lines in beamlet-redoubt (the system volume becomes a hub connection; lookups keep R75's absent/refused-by-name). (B, aio.rs, AIO1's): the waiter delivers with a timeout and keeps parking completion calls meanwhile, holding answers until the caller takes them, so no hub owner can lose a session by being busy. (C) both: B as AIO's follow-up, A now. I recommend (C): A now in BEAM3, B filed for AIO. Which?
> 
> Also, unrelated and fixed in my paths: beamlet-files' badarg was io:format ~p needing stdlib modules (epp, io_lib_pretty, ...) the case's small disk lacks; the case now prints with ~s; host run prints every expected line; rerunning both widths now.

### Orchestrator · Instruction

Event: `e692e6ddf6f785589ffd14d6e0054fef` · 2026-10-06T20:34:44Z

> (B) now, not (A). The cause is the waiter's unbounded hand-over, and aio.rs is already in your paths for this package, so fix it at the root: the waiter delivers with a bounded wait and keeps a completion call parked meanwhile, holding filled buffers until the caller takes them (a bound on held buffers stated on the page; past it the waiter stops reading, never drops), so no hub owner loses a session by being busy. Host test: a parked console read, a 12 s busy caller, no Eof, the answer delivered when the caller next waits. client.md's hub section replaces the "idle every COLLECT_WAIT/2" rule for waiter owners with the new guarantee; AIO1's residual goes. Own commit. (A) is not taken: PACK1 is on main (rebase first) and the start's module lookups are now 208 reads at 12 s total, so measure the longest synchronous wait at the shell's start after the rebase and report it; if any single wait still nears COLLECT_WAIT/2, we file the hub-lookup move as a follow-up with that number. The ~s fix is fine.

### Orchestrator · Assigned

Event: `7ad97584e3832c1f1da35c1578557092` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `32f777106176f6785efb036ae3a756ad` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `d8f4d380d74692b6dc942f72ae804265` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `4b10c5ae93a318340578096f99a3c8d1` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `fdd04429f1439d0863a0125e3f4fed8d` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
