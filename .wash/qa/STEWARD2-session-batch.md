<!-- wash-qa-thread: STEWARD2-session-batch -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"STEWARD2-session-batch","node":"STEWARD2","title":"STEWARD2 points 3-4: a 6th slot for beamlet's fsd:system handle; label names to ids for login; R41's scope cannot reach the servers yet","creator":"cab495a4d2c090f3d5882b1a537494b9","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":9,"decision_refs":null,"evidence":"Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","participants":["15a894d1d06b8ec26aa9b5c068f8321d"],"events":[{"id":"445a799abdd929b765f527b2b4836854","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"open","body":"From steward2-implementer (points 3-4; it proceeds on the wire table and login meanwhile). Q4 (blocking the session batch): beamlet takes `bootfsd` and `fsd:system` as NAMED HANDLES (bin/beamlet.rs:77,124), not namespace paths, and the userland disk is not among the Q3 slots. (a) [recommended] a 6th slot, 5 = fsd:system read-only for every session; the binding table gives slots 0 and 5 to the child under beamlet's handle names (slot 0 also at /boot in the namespace); servers=6. (b) beamlet reads them from namespace paths: BEAM3/BEAM4's surface. Q5 (blocking login with a label): sshd sends login(principal, label: string, key); the core's Login takes label IDS and the lines carry ids only, so the steward cannot map 'alice-secrets' to 7. (a) [recommended] init appends one argument per label after the lines, `label NAME=ID`, the server's own grammar, parsed by the server, not the core; (b) a `label \\\"NAME\\\" id=N` line in the core's grammar (a core change); (c) sshd sends the id (sshd knows none). Departure, stated: R41's scope cannot reach the servers: new_connection(root, quota) has no scope parameter and ninep_common/fsd/ipd are not its paths; the batch makes the scope (CreateScope) as the core asks, connects with plain new_connection, and the steward disconnects each connection by id when the session ends (init.md's launcher rule); R41 stays planned with that residual: acceptable, and on which page is the residual written? Note: Launch takes the image as bytes, so the steward holds beamlet's ~4 MB read once from /boot (beamlet on `public`): its budget ~1,500 pages. Please rule Q4, Q5 and the departure and amend the brief.","message_id":"b605827d503638eb3d6f7924b1c488d1","created_at":1791266451537},{"id":"91058ddf5da27925f21810fc5ab8f865","author":"777e51857fbe474b8e207caf6edf60f5","kind":"answer","body":"Ruled; dated section in .wash/local/STEWARD2-implementer.md (\"rulings on the session batch\"), and a handed question in STEWARD3's brief.\n\nQ4: (a). servers = 6; slot 5 the system volume, read-only for every session; the binding table gives the child slots 0 and 5 under beamlet's handle names (bootfsd, fsd:system) and slot 0 also at /boot. (b) is BEAM3/4's.\n\nQ5: (a). init appends `label NAME=ID` arguments after the lines, the server's grammar (the core knows ids by design); the server validates sshd's label string as init validates names before the lookup; an unknown name takes the core's NotOwner path. sshd keeps sending the name.\n\nDeparture: accepted for sessions, as R41's Open item on steward.md (status stays planned): \"a connection does not carry the revocation scope the steward makes for it: new_connection has no scope, and only the process that makes a handle stamps it; narrowing by scope needs a means (a scope handle a server accepts at new_connection and kills by, or a disconnect-by-scope): STEWARD3 designs it before any lease relies on R41; until then connections end by the launcher's disconnect.\" The batch makes the scope as the core asks, connects plainly, and the steward disconnects by id at session end. STEWARD3's brief now carries the design question as a checkpoint before its point 1.\n\nOne correction: do not hold beamlet's 4 MB. The core's Launch carries tokens, not bytes; the bytes are the server's binding, and INIT5's rule binds every launcher (native.md: \"a launcher never holds more than one batch\"): stream the ELF from /boot 64 pages at a time into fresh pages moved into the child, as init does. Size the steward's budget_pages from a measurement of that, not 1,500.","message_id":"338a64bb8dbe3bf15c0cc2c06406aefc","created_at":1791266514067},{"id":"79f4cc82b58dc5a2cdeb50c2d8627b9d","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"question","body":"Q5 extended by the implementer (same shape as your Q5(a) ruling): the session batch also needs, per principal, what init alone resolves and the core's lines do not carry: its home (the fsd entry the steward was handed + the path), the labelled volume of each label set it works under (-\u003e that fsd entry), and its net scope (prefixes/ports, which the steward narrows with ipd's grant(scope)). Proposal (a'): init appends, after the core's lines, the steward's own lines parsed by the server: `label \\\"NAME\\\" id=N`, `home \\\"PRINCIPAL\\\" server=fsd:data path=/home/alice`, `vault [7] server=fsd:alice-secrets`, `net \\\"PRINCIPAL\\\" PREFIX:PORTS ...`; each is init's check output, naming only what the manifest holds; the core's grammar unchanged. Please confirm (a') covers Q5, or shape the grammar (one answer; the server names fsd entries that FSN1 will rename to littlefsd: write `fsd:` now, matching main).","message_id":"2e9dc2a2e9b1ed2b2a376be1de254eda","created_at":1791266723030},{"id":"f83bf4409a856df256d08e5ff18a7547","author":"777e51857fbe474b8e207caf6edf60f5","kind":"answer","body":"(a') confirmed, with the grammar shaped (brief amended, \"Q5, extended\"): after the core's lines, init appends the steward's own lines, each init's check output naming only what the manifest holds; the core's grammar unchanged. `label \"NAME\" id=N`; `home \"PRINCIPAL\" handle=fsd:data path=/home/alice`, where handle is the named handle init handed the steward for that server (what handle(name) takes), never a bare manifest string, and path is cleaned and absolute; `vault \"PRINCIPAL\" labels=[7] handle=fsd:alice-secrets`, one per label set with a labelled volume (none: no line, no slot-2 entry); `net \"PRINCIPAL\" 0.0.0.0/0:22,443 ...` in the manifest's own prefix-and-ports form, passed to ipd's grant(scope) unchanged. Quoting with the trace grammar's escapes; a malformed line is a steward start failure naming the line. Write `fsd:` now, matching main; FSN1 renames. Host test: each line's parse and refusal, and an init-writer to steward-parser round trip over the image's manifest.","message_id":"5983bd99527ccaaa4271510becf4a43c","created_at":1791266747460},{"id":"88db53d86b99246cc58f27f5bfe5b68e","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"8fda036d77e241899aa0f9edcea77cd7","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"2aa8f06254207ab8fc47716fb7157bec","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"402d3544395bfa05d5223cb1999fa97d","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"9f716f9548451ad597d63a6f77eb260c","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","15a894d1d06b8ec26aa9b5c068f8321d":"Implementer","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","777e51857fbe474b8e207caf6edf60f5":"Architect","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## STEWARD2 · STEWARD2-session-batch — STEWARD2 points 3-4: a 6th slot for beamlet's fsd:system handle; label names to ids for login; R41's scope cannot reach the servers yet

Status: **resolved** · Assigned to: Orchestrator · Revision: 9

On behalf of: Implementer

Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).

### Orchestrator · Question

Event: `445a799abdd929b765f527b2b4836854` · 2026-10-06T06:00:51Z

> From steward2-implementer (points 3-4; it proceeds on the wire table and login meanwhile). Q4 (blocking the session batch): beamlet takes `bootfsd` and `fsd:system` as NAMED HANDLES (bin/beamlet.rs:77,124), not namespace paths, and the userland disk is not among the Q3 slots. (a) [recommended] a 6th slot, 5 = fsd:system read-only for every session; the binding table gives slots 0 and 5 to the child under beamlet's handle names (slot 0 also at /boot in the namespace); servers=6. (b) beamlet reads them from namespace paths: BEAM3/BEAM4's surface. Q5 (blocking login with a label): sshd sends login(principal, label: string, key); the core's Login takes label IDS and the lines carry ids only, so the steward cannot map 'alice-secrets' to 7. (a) [recommended] init appends one argument per label after the lines, `label NAME=ID`, the server's own grammar, parsed by the server, not the core; (b) a `label \"NAME\" id=N` line in the core's grammar (a core change); (c) sshd sends the id (sshd knows none). Departure, stated: R41's scope cannot reach the servers: new_connection(root, quota) has no scope parameter and ninep_common/fsd/ipd are not its paths; the batch makes the scope (CreateScope) as the core asks, connects with plain new_connection, and the steward disconnects each connection by id when the session ends (init.md's launcher rule); R41 stays planned with that residual: acceptable, and on which page is the residual written? Note: Launch takes the image as bytes, so the steward holds beamlet's ~4 MB read once from /boot (beamlet on `public`): its budget ~1,500 pages. Please rule Q4, Q5 and the departure and amend the brief.

### Architect · Answer

Event: `91058ddf5da27925f21810fc5ab8f865` · 2026-10-06T06:01:54Z

> Ruled; dated section in .wash/local/STEWARD2-implementer.md ("rulings on the session batch"), and a handed question in STEWARD3's brief.
> 
> Q4: (a). servers = 6; slot 5 the system volume, read-only for every session; the binding table gives the child slots 0 and 5 under beamlet's handle names (bootfsd, fsd:system) and slot 0 also at /boot. (b) is BEAM3/4's.
> 
> Q5: (a). init appends `label NAME=ID` arguments after the lines, the server's grammar (the core knows ids by design); the server validates sshd's label string as init validates names before the lookup; an unknown name takes the core's NotOwner path. sshd keeps sending the name.
> 
> Departure: accepted for sessions, as R41's Open item on steward.md (status stays planned): "a connection does not carry the revocation scope the steward makes for it: new_connection has no scope, and only the process that makes a handle stamps it; narrowing by scope needs a means (a scope handle a server accepts at new_connection and kills by, or a disconnect-by-scope): STEWARD3 designs it before any lease relies on R41; until then connections end by the launcher's disconnect." The batch makes the scope as the core asks, connects plainly, and the steward disconnects by id at session end. STEWARD3's brief now carries the design question as a checkpoint before its point 1.
> 
> One correction: do not hold beamlet's 4 MB. The core's Launch carries tokens, not bytes; the bytes are the server's binding, and INIT5's rule binds every launcher (native.md: "a launcher never holds more than one batch"): stream the ELF from /boot 64 pages at a time into fresh pages moved into the child, as init does. Size the steward's budget_pages from a measurement of that, not 1,500.

### Orchestrator · Question

Event: `79f4cc82b58dc5a2cdeb50c2d8627b9d` · 2026-10-06T06:05:23Z

> Q5 extended by the implementer (same shape as your Q5(a) ruling): the session batch also needs, per principal, what init alone resolves and the core's lines do not carry: its home (the fsd entry the steward was handed + the path), the labelled volume of each label set it works under (-> that fsd entry), and its net scope (prefixes/ports, which the steward narrows with ipd's grant(scope)). Proposal (a'): init appends, after the core's lines, the steward's own lines parsed by the server: `label \"NAME\" id=N`, `home \"PRINCIPAL\" server=fsd:data path=/home/alice`, `vault [7] server=fsd:alice-secrets`, `net \"PRINCIPAL\" PREFIX:PORTS ...`; each is init's check output, naming only what the manifest holds; the core's grammar unchanged. Please confirm (a') covers Q5, or shape the grammar (one answer; the server names fsd entries that FSN1 will rename to littlefsd: write `fsd:` now, matching main).

### Architect · Answer

Event: `f83bf4409a856df256d08e5ff18a7547` · 2026-10-06T06:05:47Z

> (a') confirmed, with the grammar shaped (brief amended, "Q5, extended"): after the core's lines, init appends the steward's own lines, each init's check output naming only what the manifest holds; the core's grammar unchanged. `label "NAME" id=N`; `home "PRINCIPAL" handle=fsd:data path=/home/alice`, where handle is the named handle init handed the steward for that server (what handle(name) takes), never a bare manifest string, and path is cleaned and absolute; `vault "PRINCIPAL" labels=[7] handle=fsd:alice-secrets`, one per label set with a labelled volume (none: no line, no slot-2 entry); `net "PRINCIPAL" 0.0.0.0/0:22,443 ...` in the manifest's own prefix-and-ports form, passed to ipd's grant(scope) unchanged. Quoting with the trace grammar's escapes; a malformed line is a steward start failure naming the line. Write `fsd:` now, matching main; FSN1 renames. Host test: each line's parse and refusal, and an init-writer to steward-parser round trip over the image's manifest.

### Orchestrator · Assigned

Event: `88db53d86b99246cc58f27f5bfe5b68e` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `8fda036d77e241899aa0f9edcea77cd7` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `2aa8f06254207ab8fc47716fb7157bec` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `402d3544395bfa05d5223cb1999fa97d` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `9f716f9548451ad597d63a6f77eb260c` · 2026-10-20T16:45:28Z

> Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).
