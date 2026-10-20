<!-- wash-qa-thread: BEAM3-namespace-root -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-namespace-root","node":"BEAM3","title":"The namespace's / (and other binding ancestors) under Files: userland-boot/read-only prompt shows ?","creator":"deb44cd542dbbf90298683a07c85bb95","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"2d2c5ca051d3cd2f0367cfe0c321ae81","author":"deb44cd542dbbf90298683a07c85bb95","kind":"open","body":"userland-boot and userland-read-only fail both widths on the branch: the prompt is `? (2)\u003e 55`, the case wants `/ (2)\u003e 55`. Cause: beamlet now has Files on Redoubt; get_cwd (and set_cwd) stat the cwd `/`, and `/` is held by no binding (ns.lookup finds none), so info(\"/\") is enoent and the shell shows `?`. Before BEAM3 the platform had no Files (enotsup), so nobody checked. sessions.md / brief ruling 5 say a prefix no binding holds is enoent, but do not say what an ANCESTOR of bindings (`/`, `/home`, `/dev`) is. Options: (A, recommended) an ancestor of a binding is a directory the namespace answers itself: info = directory with no other fields, list_dir = the next name of each binding below it, mkdir/rm/open-for-write there = eacces; a path neither inside nor above a binding stays enoent (bob's /home/alice still enoent, as /home lists only what is bound). sessions.md gains one line; host tests pin it. (B) get_cwd answers the cwd without checking it, and set_cwd accepts an ancestor without a stat: smallest, but File.ls(\"/\") and cd / stay enoent. (C) start the VM's cwd at its home binding when one exists: does not fix userland-boot (no home there). A or B?","message_id":"7af631022443ac9372e82f311b245af1","created_at":1791333763392},{"id":"8dffeabe3eff3a76ee20064de7a6a7f4","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"(A). An ancestor of a binding is a directory the namespace answers itself: info is a directory with no other fields, list_dir is the next name of each binding below it, and mkdir, rm or open-for-write there is eacces; a path neither inside nor above a binding stays enoent, so bob's /home/alice is enoent because /home lists only what is bound. One line on sessions.md; host tests pin info, list_dir and the eacces for an ancestor, and the enoent for a sibling. userland-boot's prompt is then `/ (2)\u003e` as the case wants.","message_id":"3c39a1f9a70037747a260dbcf92b3e1a","created_at":1791333792546},{"id":"e63ad190b7a9db2d24ebe86d273f7c5f","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"5c99cb0cb590f4a40d8efaa60403e60a","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"96176c0f733cb1321b24582ca3f7674b","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"a7fbdd9d782645bf20f3e7c6d470dafe","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"91623831135dffb6e747400b7dfa8752","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator","deb44cd542dbbf90298683a07c85bb95":"Implementer"}} -->

## BEAM3 · BEAM3-namespace-root — The namespace's / (and other binding ancestors) under Files: userland-boot/read-only prompt shows ?

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `2d2c5ca051d3cd2f0367cfe0c321ae81` · 2026-10-07T00:42:43Z

> userland-boot and userland-read-only fail both widths on the branch: the prompt is `? (2)> 55`, the case wants `/ (2)> 55`. Cause: beamlet now has Files on Redoubt; get_cwd (and set_cwd) stat the cwd `/`, and `/` is held by no binding (ns.lookup finds none), so info("/") is enoent and the shell shows `?`. Before BEAM3 the platform had no Files (enotsup), so nobody checked. sessions.md / brief ruling 5 say a prefix no binding holds is enoent, but do not say what an ANCESTOR of bindings (`/`, `/home`, `/dev`) is. Options: (A, recommended) an ancestor of a binding is a directory the namespace answers itself: info = directory with no other fields, list_dir = the next name of each binding below it, mkdir/rm/open-for-write there = eacces; a path neither inside nor above a binding stays enoent (bob's /home/alice still enoent, as /home lists only what is bound). sessions.md gains one line; host tests pin it. (B) get_cwd answers the cwd without checking it, and set_cwd accepts an ancestor without a stat: smallest, but File.ls("/") and cd / stay enoent. (C) start the VM's cwd at its home binding when one exists: does not fix userland-boot (no home there). A or B?

### Orchestrator · Instruction

Event: `8dffeabe3eff3a76ee20064de7a6a7f4` · 2026-10-07T00:43:12Z

> (A). An ancestor of a binding is a directory the namespace answers itself: info is a directory with no other fields, list_dir is the next name of each binding below it, and mkdir, rm or open-for-write there is eacces; a path neither inside nor above a binding stays enoent, so bob's /home/alice is enoent because /home lists only what is bound. One line on sessions.md; host tests pin info, list_dir and the eacces for an ancestor, and the enoent for a sibling. userland-boot's prompt is then `/ (2)>` as the case wants.

### Orchestrator · Assigned

Event: `e63ad190b7a9db2d24ebe86d273f7c5f` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `5c99cb0cb590f4a40d8efaa60403e60a` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `96176c0f733cb1321b24582ca3f7674b` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `a7fbdd9d782645bf20f3e7c6d470dafe` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `91623831135dffb6e747400b7dfa8752` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
