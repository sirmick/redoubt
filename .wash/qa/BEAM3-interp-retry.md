<!-- wash-qa-thread: BEAM3-interp-retry -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-interp-retry","node":"BEAM3","title":"BEAM3 needs 3 lines in interp.rs (BEAM8's): retry on a native body","creator":"11bf49ef7ff45361d2781dd327a1cb65","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"6dffa209a3eeaea2fbbce3fccfaa5c18","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"open","body":"Blocking beamlet-files (it fails both widths: {error,eagain} reaches Erlang). The brief says stop if a Files change needs interp.rs; it does. A file native that cannot finish yet sets the existing `p.retry` (Ctx::retry, used by port natives), and interp re-calls it next slice: but only on the external-call path (`call_mfa_with`, interp.rs ~483). OTP's prim_file calls its NIFs locally, through stub bodies the loader replaced (`crate::module::NATIVE_BODY`, interp.rs ~850), which returns the native's result without checking `p.retry`. My VM tests called `prim_file:read_file_nif` externally, so they missed it (reproduced on the host since).\n\nFix, in the NATIVE_BODY arm: if `p.retry` is set after the call, clear it, leave pc on the NATIVE_BODY instruction (x registers still hold the args) and return `Flow::Stop(Stop::Yield)`, so the body runs again when the process next runs; vm.rs already parks it until its operation ends. About 4 lines, plus a VM test whose fixture calls the NIF through a local stub (a module of its own with the stub and a body native), so the gap is caught. Options: (A, recommended) I make it in interp.rs, one hunk in its own commit, and name it to BEAM8 for their rebase; (B) BEAM8 makes it and I rebase on it; (C) avoid interp.rs: beamlet embeds its own prim_file stand-in whose Erlang wrappers call the natives externally (large, and OTP's prim_file no longer runs unchanged). Which?","message_id":"f88ace43ee3d7999c1affab24bedea4b","created_at":1791315896121},{"id":"2ea67cf745cc970af52d02eecdfb8058","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"(A). One hunk in interp.rs's NATIVE_BODY arm, its own commit, message naming the gap (a native's retry honoured on the external-call path only) and the test whose fixture calls the NIF through a local stub. Keep pc on the instruction and yield as you describe; no other interp.rs change. I am telling BEAM8 now so it rebases over the hunk; whichever of you merges second carries the trivial rebase.","message_id":"52260abfa376ce3cff43331704e3faf8","created_at":1791315907908},{"id":"58e3e1a1e0549f2fd001bae544dcc567","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"85bc5ab91a0f28e21eb71702a067ec69","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"a421a6b8a8c7a20e4974c2a610dc0e87","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"5ef93014e22f7ca92ac5afa13504ead7","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"d185b84473255835d5531daacdec1e68","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"11bf49ef7ff45361d2781dd327a1cb65":"Implementer","12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## BEAM3 · BEAM3-interp-retry — BEAM3 needs 3 lines in interp.rs (BEAM8's): retry on a native body

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `6dffa209a3eeaea2fbbce3fccfaa5c18` · 2026-10-06T19:44:56Z

> Blocking beamlet-files (it fails both widths: {error,eagain} reaches Erlang). The brief says stop if a Files change needs interp.rs; it does. A file native that cannot finish yet sets the existing `p.retry` (Ctx::retry, used by port natives), and interp re-calls it next slice: but only on the external-call path (`call_mfa_with`, interp.rs ~483). OTP's prim_file calls its NIFs locally, through stub bodies the loader replaced (`crate::module::NATIVE_BODY`, interp.rs ~850), which returns the native's result without checking `p.retry`. My VM tests called `prim_file:read_file_nif` externally, so they missed it (reproduced on the host since).
> 
> Fix, in the NATIVE_BODY arm: if `p.retry` is set after the call, clear it, leave pc on the NATIVE_BODY instruction (x registers still hold the args) and return `Flow::Stop(Stop::Yield)`, so the body runs again when the process next runs; vm.rs already parks it until its operation ends. About 4 lines, plus a VM test whose fixture calls the NIF through a local stub (a module of its own with the stub and a body native), so the gap is caught. Options: (A, recommended) I make it in interp.rs, one hunk in its own commit, and name it to BEAM8 for their rebase; (B) BEAM8 makes it and I rebase on it; (C) avoid interp.rs: beamlet embeds its own prim_file stand-in whose Erlang wrappers call the natives externally (large, and OTP's prim_file no longer runs unchanged). Which?

### Orchestrator · Instruction

Event: `2ea67cf745cc970af52d02eecdfb8058` · 2026-10-06T19:45:07Z

> (A). One hunk in interp.rs's NATIVE_BODY arm, its own commit, message naming the gap (a native's retry honoured on the external-call path only) and the test whose fixture calls the NIF through a local stub. Keep pc on the instruction and yield as you describe; no other interp.rs change. I am telling BEAM8 now so it rebases over the hunk; whichever of you merges second carries the trivial rebase.

### Orchestrator · Assigned

Event: `58e3e1a1e0549f2fd001bae544dcc567` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `85bc5ab91a0f28e21eb71702a067ec69` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `a421a6b8a8c7a20e4974c2a610dc0e87` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `5ef93014e22f7ca92ac5afa13504ead7` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `d185b84473255835d5531daacdec1e68` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
