<!-- wash-qa-thread: BEAM3-footprint-headroom -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-footprint-headroom","node":"BEAM3","title":"beamlet-footprint headroom: consoled and beamlet need more declared in image/manifest.json","creator":"deb44cd542dbbf90298683a07c85bb95","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"a736065e7690846afaf8e6c3ee2b4a50","author":"deb44cd542dbbf90298683a07c85bb95","kind":"open","body":"beamlet-footprint now runs to its end (the waiter fix holds the console) and fails only its 2x-peak headroom check, from the console on the hub (consoled serves a multiplexed session): rv64 consoled stack peak 10376 B -\u003e needs 6 pages, declared 5; consoled heap peak 10 pages -\u003e needs 20, capped 18; beamlet main stack peak 36616 B -\u003e needs 18, declared 17. rv32: consoled heap peak 11 -\u003e needs 22, capped 18 (stack ok). image/manifest.json is not in my owned paths. Options: (A, recommended) raise in image/manifest.json in the console-on-hub commit: consoled stack_pages 5-\u003e6, heap_pages 18-\u003e22; beamlet stack_pages 17-\u003e18 (+6 pages of root's bound, boot holds 349 of 523); (B) reduce consoled's mux cost instead (servers are not mine: a follow-up). OK to do A?","message_id":"644225e2dcb33adc87510629706325c8","created_at":1791333763453},{"id":"94cd125e85ba5312cf242d3fe9059634","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"(A), in the console-on-hub commit, with the rule's arithmetic in the commit message and the table rows on testbench.md: consoled stack 5 -\u003e 6 pages (peak 10,376), consoled heap 18 -\u003e 22 (peak 11 pages on rv32), beamlet stack 17 -\u003e 18 (peak 36,616); the manifest and tests/data copies init's test holds equal. Also one sentence in your report on why a multiplexed session costs consoled 5 more heap pages (what it holds per session), so (B) can be filed with a number if it is worth it.","message_id":"58389af6a4d694050ff1b2ee94807ab9","created_at":1791333797704},{"id":"2328ce673ccb8d43dc1dae7ac18d01bd","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"3d857c40ab92324df34df7157aa5273d","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"598b801470eb1fb7f4b7bf6110995cef","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"607131e739d395f0cd2d9ad27d8bd7c8","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"5189d59d82dcc974d775302ee0cbac11","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator","deb44cd542dbbf90298683a07c85bb95":"Implementer"}} -->

## BEAM3 · BEAM3-footprint-headroom — beamlet-footprint headroom: consoled and beamlet need more declared in image/manifest.json

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `a736065e7690846afaf8e6c3ee2b4a50` · 2026-10-07T00:42:43Z

> beamlet-footprint now runs to its end (the waiter fix holds the console) and fails only its 2x-peak headroom check, from the console on the hub (consoled serves a multiplexed session): rv64 consoled stack peak 10376 B -> needs 6 pages, declared 5; consoled heap peak 10 pages -> needs 20, capped 18; beamlet main stack peak 36616 B -> needs 18, declared 17. rv32: consoled heap peak 11 -> needs 22, capped 18 (stack ok). image/manifest.json is not in my owned paths. Options: (A, recommended) raise in image/manifest.json in the console-on-hub commit: consoled stack_pages 5->6, heap_pages 18->22; beamlet stack_pages 17->18 (+6 pages of root's bound, boot holds 349 of 523); (B) reduce consoled's mux cost instead (servers are not mine: a follow-up). OK to do A?

### Orchestrator · Instruction

Event: `94cd125e85ba5312cf242d3fe9059634` · 2026-10-07T00:43:17Z

> (A), in the console-on-hub commit, with the rule's arithmetic in the commit message and the table rows on testbench.md: consoled stack 5 -> 6 pages (peak 10,376), consoled heap 18 -> 22 (peak 11 pages on rv32), beamlet stack 17 -> 18 (peak 36,616); the manifest and tests/data copies init's test holds equal. Also one sentence in your report on why a multiplexed session costs consoled 5 more heap pages (what it holds per session), so (B) can be filed with a number if it is worth it.

### Orchestrator · Assigned

Event: `2328ce673ccb8d43dc1dae7ac18d01bd` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `3d857c40ab92324df34df7157aa5273d` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `598b801470eb1fb7f4b7bf6110995cef` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `607131e739d395f0cd2d9ad27d8bd7c8` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `5189d59d82dcc974d775302ee0cbac11` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
