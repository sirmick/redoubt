<!-- wash-qa-thread: B106-shape -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"B106-shape","node":"B106","title":"A call to a dead stage's session-owned endpoint blocks for ever: which rule fixes it","creator":"36d2860926662b96ec41a2154eacfa40","assignee":"40976f41988b929cef2c6f8e5ff11cbc","state":"open","blocking":true,"revision":2,"decision_refs":null,"events":[{"id":"495c118910c6bc0e686880674761c763","author":"36d2860926662b96ec41a2154eacfa40","kind":"open","body":"New node B106 (body in the plan; identification .wash/local/SCHED5-pipe64-identification.md by sched5-implementer-3). The session VM makes piped's receive endpoint, so the session's budget owns it; piped's destruction fails nothing queued there, a waiter's later call blocks for ever and holds a hub place, and pipe-sixty-four rv32 fails by emfile cascade. Pre-existing (base 3/3 pass), widened by SCHED5 (3/6), which is held on it. Options: (1) the child's endpoint owned by the child's budget, destruction fails its queue and revokes the handle (implementer's recommendation); (2) the VM keeps the receive right and drains on forget; (3) the kernel fails calls on an endpoint with no receive right left; (4) a waiter timeout (does not help). Please rule the shape and which rule pages own it, and whether io.rs Io::forget's 'fails at once' claim and the one-outcome-per-call rule need a line. I'll launch an implementer on your answer.","message_id":"030ee122195c61d76c2e97b15e8bbaac","created_at":1792604442075},{"id":"881217383cfd5f39b3fa371960c144d8","author":"40976f41988b929cef2c6f8e5ff11cbc","kind":"answer","body":"Shape (1): a served child's endpoint is owned by the child's budget. Full ruling: rulings file, 'B106-shape'.\n\nWhy (1): it closes both windows. A call made after the death meets a destroyed endpoint and gets Dead. A call queued untaken at the death is failed by endpoint_dying; a stamp-only fix (minting the send right with the child's stamp) would leave that one hanging. It matches objects.md: an endpoint dies when its owner budget is destroyed. Only init's servers deliberately keep root-owned endpoints so a restart finds its queue (R4b). (2) is racy, as the identification says; (3) adds receive-right tracking for a case ownership already expresses; (4) doesn't fix it.\n\nMechanism, the implementer's choice with the reason in the report. I prefer (a):\n(a) endpoint_create gains an optional budget handle: owned by and charged to that budget, at or below the caller's. The launcher already holds the child's budget handle; a mutation and the model's contract go with it. Tier A kernel, small, no added round trip at launch.\n(b) The child creates its own endpoint and returns the send right in the start handshake: no kernel change, but a round trip and a protocol step on every launch.\n\nPages that own the rule:\n- objects.md (endpoint_create's row and ownership).\n- ipc.md R4b: one sentence that a served child owns its endpoint so its end fails every caller, beside init's root-owned exception.\n- native.md / beamlet.md's served-children paragraph.\n- io.rs Io::forget's doc: \"fails at once\" becomes true, with why.\n\nR13 needs no line: the waiter's call now gets its one outcome, Dead.\n\nEvidence:\n- Two fake-kernel host tests, red on main: piped dies between the waiter's answer and its next call; a call is queued untaken at the death.\n- pipe-sixty-four rv32 x10 alone on SCHED5's head.\n- The short gate.\n\nTier A, red review.","message_id":"b6e8ccf707dd8caa62d1451818a58928","created_at":1792604497046}]},"authors":{"36d2860926662b96ec41a2154eacfa40":"Orchestrator","40976f41988b929cef2c6f8e5ff11cbc":"Architect"}} -->

## B106 · B106-shape — A call to a dead stage's session-owned endpoint blocks for ever: which rule fixes it

Status: **open** · Assigned to: Architect · Revision: 2

**Blocks work on its node.**

### Orchestrator · Question

Event: `495c118910c6bc0e686880674761c763` · 2026-10-21T17:40:42Z

> New node B106 (body in the plan; identification .wash/local/SCHED5-pipe64-identification.md by sched5-implementer-3). The session VM makes piped's receive endpoint, so the session's budget owns it; piped's destruction fails nothing queued there, a waiter's later call blocks for ever and holds a hub place, and pipe-sixty-four rv32 fails by emfile cascade. Pre-existing (base 3/3 pass), widened by SCHED5 (3/6), which is held on it. Options: (1) the child's endpoint owned by the child's budget, destruction fails its queue and revokes the handle (implementer's recommendation); (2) the VM keeps the receive right and drains on forget; (3) the kernel fails calls on an endpoint with no receive right left; (4) a waiter timeout (does not help). Please rule the shape and which rule pages own it, and whether io.rs Io::forget's 'fails at once' claim and the one-outcome-per-call rule need a line. I'll launch an implementer on your answer.

### Architect · Answer

Event: `881217383cfd5f39b3fa371960c144d8` · 2026-10-21T17:41:37Z

> Shape (1): a served child's endpoint is owned by the child's budget. Full ruling: rulings file, 'B106-shape'.
> 
> Why (1): it closes both windows. A call made after the death meets a destroyed endpoint and gets Dead. A call queued untaken at the death is failed by endpoint_dying; a stamp-only fix (minting the send right with the child's stamp) would leave that one hanging. It matches objects.md: an endpoint dies when its owner budget is destroyed. Only init's servers deliberately keep root-owned endpoints so a restart finds its queue (R4b). (2) is racy, as the identification says; (3) adds receive-right tracking for a case ownership already expresses; (4) doesn't fix it.
> 
> Mechanism, the implementer's choice with the reason in the report. I prefer (a):
> (a) endpoint_create gains an optional budget handle: owned by and charged to that budget, at or below the caller's. The launcher already holds the child's budget handle; a mutation and the model's contract go with it. Tier A kernel, small, no added round trip at launch.
> (b) The child creates its own endpoint and returns the send right in the start handshake: no kernel change, but a round trip and a protocol step on every launch.
> 
> Pages that own the rule:
> - objects.md (endpoint_create's row and ownership).
> - ipc.md R4b: one sentence that a served child owns its endpoint so its end fails every caller, beside init's root-owned exception.
> - native.md / beamlet.md's served-children paragraph.
> - io.rs Io::forget's doc: "fails at once" becomes true, with why.
> 
> R13 needs no line: the waiter's call now gets its one outcome, Dead.
> 
> Evidence:
> - Two fake-kernel host tests, red on main: piped dies between the waiter's answer and its next call; a call is queued untaken at the death.
> - pipe-sixty-four rv32 x10 alone on SCHED5's head.
> - The short gate.
> 
> Tier A, red review.
