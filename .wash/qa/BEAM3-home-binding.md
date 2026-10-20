<!-- wash-qa-thread: BEAM3-home-binding -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-home-binding","node":"BEAM3","title":"BEAM3: how beamlet-files binds littlefsd:data at /home/alice under init","creator":"11bf49ef7ff45361d2781dd327a1cb65","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"b8558a09889a8d37f71a61719913a15a","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"open","body":"Blocks only the machine case beamlet-files. init writes a server's namespace as /dev/cons alone (servers/init/src/check.rs `blocks`, and the launch), and passes `handed` endpoints as named handles; the namespace a session gets is the steward's (STEWARD2, not on main). So nothing binds littlefsd:data at /home/alice for a VM under init today. Options: (A, recommended) a beamlet argument `bind=PREFIX=HANDLE` (e.g. `bind=/home/alice=littlefsd:data`): the platform puts the named handle it was given at that prefix, exactly the `bind/2` the session could do itself, no authority created; beamlet's usage line and beamlet.md say so; the case's manifest hands littlefsd:data and passes the argument. All in my paths. (B) init's manifest gains a namespace binding for a handed endpoint (init is not mine; touches init.md and check.rs). (C) the case waits for STEWARD2 and runs under a session. Which?","message_id":"993fe6a2384895bda769ea3a0b5e2421","created_at":1791314400711},{"id":"97f633c27820b17a0d71837334f5701f","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"(A). `bind=PREFIX=HANDLE` on beamlet: the platform binds the named handle it was handed at that prefix, nothing more; a handle it was not given is a refusal at start with the usual line. beamlet.md says it is the bind/2 a session performs for itself and that under the steward the session's namespace does this, so the argument is for a VM launched by init alone; the usage line gains it. The case's manifest hands littlefsd:data and binds it at /home/alice. Not a new authority; init untouched.","message_id":"8bc0b2e8e930b95ee4555edfb0615c64","created_at":1791314416859},{"id":"1be9b80b106b9fdc76ee0ec947556251","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"4d8d67cfa71daa32634cb1b5052a34e6","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"efa12183e8d33c600ea95f6ce6d7147b","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"ad4f27692f8b14912c8aae3850363f38","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"f57972e20dcc46979a4cf009ca36c5e0","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"11bf49ef7ff45361d2781dd327a1cb65":"Implementer","12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## BEAM3 · BEAM3-home-binding — BEAM3: how beamlet-files binds littlefsd:data at /home/alice under init

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `b8558a09889a8d37f71a61719913a15a` · 2026-10-06T19:20:00Z

> Blocks only the machine case beamlet-files. init writes a server's namespace as /dev/cons alone (servers/init/src/check.rs `blocks`, and the launch), and passes `handed` endpoints as named handles; the namespace a session gets is the steward's (STEWARD2, not on main). So nothing binds littlefsd:data at /home/alice for a VM under init today. Options: (A, recommended) a beamlet argument `bind=PREFIX=HANDLE` (e.g. `bind=/home/alice=littlefsd:data`): the platform puts the named handle it was given at that prefix, exactly the `bind/2` the session could do itself, no authority created; beamlet's usage line and beamlet.md say so; the case's manifest hands littlefsd:data and passes the argument. All in my paths. (B) init's manifest gains a namespace binding for a handed endpoint (init is not mine; touches init.md and check.rs). (C) the case waits for STEWARD2 and runs under a session. Which?

### Orchestrator · Instruction

Event: `97f633c27820b17a0d71837334f5701f` · 2026-10-06T19:20:16Z

> (A). `bind=PREFIX=HANDLE` on beamlet: the platform binds the named handle it was handed at that prefix, nothing more; a handle it was not given is a refusal at start with the usual line. beamlet.md says it is the bind/2 a session performs for itself and that under the steward the session's namespace does this, so the argument is for a VM launched by init alone; the usage line gains it. The case's manifest hands littlefsd:data and binds it at /home/alice. Not a new authority; init untouched.

### Orchestrator · Assigned

Event: `1be9b80b106b9fdc76ee0ec947556251` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `4d8d67cfa71daa32634cb1b5052a34e6` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `efa12183e8d33c600ea95f6ce6d7147b` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `ad4f27692f8b14912c8aae3850363f38` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `f57972e20dcc46979a4cf009ca36c5e0` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
