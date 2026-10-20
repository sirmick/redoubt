<!-- wash-qa-thread: B82-launch-race -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"B82-launch-race","node":"B82","title":"A lease's term is counted from the request: a launch longer than the term kills the agent before it starts","creator":"7d99175e5e6690ac351ee800a1dceb77","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":4,"decision_refs":null,"evidence":"Answered; B82 done, merged in 8864d64a6 (QA trailer).","events":[{"id":"6f2da2e391ce5ac5256c50dbbf18de41","author":"7d99175e5e6690ac351ee800a1dceb77","kind":"open","body":"B82's finding (.wash/local/B82-report.md): the steward sets a lease's kernel deadline at its batch's start (effects.rs carve_lease: now + lease), and the Launch step that follows (image read from erofsd, map, start) takes ~1.25 s of guest time at one hart and ~2.46 s at two in the checked build. A lease shorter than its launch is destroyed by its own deadline between process_create and process_start: the kernel does what R10 says, the launcher's map/start answer NotPermitted, the steward reports a failed step, and the sponsor gets a lease that never ran. steward-latency's 2-s kernel-deadline leases hit this at two harts (STEWARD4 lengthens them). Design question, not blocking B82 (which pins the kernel behaviour with a case): should steward.md \"Leases\" define the term from the agent's start rather than the request? That needs a way to set or move a budget's deadline after the launch (no kernel call exists; a FOREVER budget re-carved is not it), or a launch floor below which the steward refuses a lease (as it refuses one over MAX_LEASE), or the page states that the term runs from the request and the launch counts within it. Answer when free.","message_id":"2db996ecaeebc93a86689df25e22e4bd","created_at":1792199640465},{"id":"79f6bc526021d77c66897e005a1ce76a","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"d449ccc4631b46a4ee33b15693df6aae","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"0fa408797d96969e61974168b9b5d6cc","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; B82 done, merged in 8864d64a6 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","7d99175e5e6690ac351ee800a1dceb77":"B82 implementer"}} -->

## B82 · B82-launch-race — A lease's term is counted from the request: a launch longer than the term kills the agent before it starts

Status: **resolved** · Assigned to: Orchestrator · Revision: 4

Evidence: Answered; B82 done, merged in 8864d64a6 (QA trailer).

### B82 implementer · Question

Event: `6f2da2e391ce5ac5256c50dbbf18de41` · 2026-10-17T01:14:00Z

> B82's finding (.wash/local/B82-report.md): the steward sets a lease's kernel deadline at its batch's start (effects.rs carve_lease: now + lease), and the Launch step that follows (image read from erofsd, map, start) takes ~1.25 s of guest time at one hart and ~2.46 s at two in the checked build. A lease shorter than its launch is destroyed by its own deadline between process_create and process_start: the kernel does what R10 says, the launcher's map/start answer NotPermitted, the steward reports a failed step, and the sponsor gets a lease that never ran. steward-latency's 2-s kernel-deadline leases hit this at two harts (STEWARD4 lengthens them). Design question, not blocking B82 (which pins the kernel behaviour with a case): should steward.md "Leases" define the term from the agent's start rather than the request? That needs a way to set or move a budget's deadline after the launch (no kernel call exists; a FOREVER budget re-carved is not it), or a launch floor below which the steward refuses a lease (as it refuses one over MAX_LEASE), or the page states that the term runs from the request and the launch counts within it. Answer when free.

### Orchestrator · Assigned

Event: `79f6bc526021d77c66897e005a1ce76a` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `d449ccc4631b46a4ee33b15693df6aae` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `0fa408797d96969e61974168b9b5d6cc` · 2026-10-20T16:45:28Z

> Evidence: Answered; B82 done, merged in 8864d64a6 (QA trailer).
