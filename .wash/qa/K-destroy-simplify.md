<!-- wash-qa-thread: K-destroy-simplify -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"K-destroy-simplify","node":"M1","title":"The destruction path: what can be made simpler, with a simplifier's hat on","creator":"4da9ce984a47023592aadafbc57addd0","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":10,"decision_refs":null,"evidence":"Architect answered (pumps at the boundary, .wash/local/destroy-simplify.md); built as K19, done, merged in f4d3b41d9.","events":[{"id":"2b52be7b5689700f9dd48aa1b6717bd7","author":"4da9ce984a47023592aadafbc57addd0","kind":"open","body":"From the owner, a design question, not a package yet. message.rs (1,758 lines) and budget.rs (1,276) are 40% of the kernel, and nearly every kernel finding of the last two weeks sat at their seam: a subtree destroyed while IPC is in flight (K9 cost follows the dying subtree; K10 endpoint destroy stops scanning threads; K12 R10 at full fill; K13 one pump per endpoint; K17 doomed takes nothing; K18 audits off the schedule; GATE1's three notice threads). Each fix was right and small, and the rules are now stated (R4b, 'doomed' on budgets.md R10 step 3, I15), but the owner asks: with a simplifier's hat on, what could make the destroy path simpler as a whole, so the next class of bugs is impossible rather than caught? Please think about it from the pages and the code, not from the fixes: (1) is there one mechanism doing two jobs (destruction deciding IPC outcomes, or IPC walking budgets) that could be one job each; (2) could destruction be a two-phase 'mark doomed, then reap' where the reap has nothing to decide, so no walk during a kill picks a receiver or a sender; (3) does R10's cost rule force complexity that a different accounting (charge the destroyer up front, say) would remove; (4) what K16's walks-by-live-thread already simplifies and what it leaves; (5) what would have to be given up (a target, a rule, a property) for each option. A short written assessment with your recommendation, on this thread or in .wash/local/destroy-simplify.md; no page edits yet. If you conclude it is already as simple as the properties allow, say so and why.","message_id":"a7a2dc441f130f1a9cc37f011ca787ca","created_at":1790987719955},{"id":"9e043a53350f6cade814a5b86a2019ae","author":"f6461b281522084916e22a757c046b4b","kind":"answer","body":"Assessment: ./.wash/local/destroy-simplify.md. No page edits.\n\nNot as simple as the properties allow. R10's nine steps are each needed by a rule, and its billing is about ten lines. The fragility comes from two structural facts:\n(a) Destruction runs IPC's live path mid-kill. Each kill's process_ending and settle_notice pump endpoints after the mark, before step 4 fails what is tied to dying budgets. So IPC carries five dying/doomed checks, each from a finding (K17's receiver_on, abandon, allowed, drop_dying_notices, budgets_dying's ordering). The pump does two jobs: deliver, and carry on after a teardown.\n(b) An endpoint has no state of its own. Its queues, receivers and notices live in thread slots, so every question about an endpoint is an all-threads walk.\nOne consequence of (a), to confirm by trace: a kill's pump can deliver a send stamped with a dying budget, which step 4 would otherwise fail with Dead. Kill order then decides the outcome.\n\nRecommend one package after K16 (Tier A, M), \"pumps at the boundary\":\n- Each operation changes state and collects the endpoints to pump; its end pumps each once (K13's rule, made general).\n- Destruction becomes mark, sever (one pass, no pumps), reap, then pump.\n- The five checks and the rescan loop go. R4b's \"doomed takes nothing\" becomes structural, and its mutation is retired as unwritable. R10 step 4 tightens to \"from the mark on\".\nNothing in the rules or targets is given up.\n\nRejected:\n- charging the destroyer up front: it needs a cost estimate and simplifies nothing;\n- deferred reaping: dying state would leak across kernel entries.\nEndpoint-owned queues would fix (b), but only decide on them if K16's worst-walk numbers demand it. K16's walks bound the cost, not the shape.","message_id":"62a25bbf4f56277b7b4a852b317c5cad","created_at":1790987856273},{"id":"8f73d916dce827166db9a7b2e3ecc2c0","author":"5e72d6c884926b33a54b579af9bb50de","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791149738695},{"id":"d2973939ef2355a8f9f006dc8fcd3ef0","author":"2dca6f931423db09b53b7b9065ea2d5d","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791169589526},{"id":"7f563f31a9041a88df7f639f1cca838d","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791228409424},{"id":"beeb78b47942296d722a20f693aad0d5","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"3106ff7f96ffa9cc5d343daff91db1ef","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"e74b05178448c7892587b21b5f57ea18","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"3bf7a320fef5305541bc8a65e34d633d","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"4cb0baa0022c04cc85e1d8b56e48e474","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Architect answered (pumps at the boundary, .wash/local/destroy-simplify.md); built as K19, done, merged in f4d3b41d9.","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","2dca6f931423db09b53b7b9065ea2d5d":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","4da9ce984a47023592aadafbc57addd0":"Orchestrator","5e72d6c884926b33a54b579af9bb50de":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator","f6461b281522084916e22a757c046b4b":"Architect"}} -->

## M1 · K-destroy-simplify — The destruction path: what can be made simpler, with a simplifier's hat on

Status: **resolved** · Assigned to: Orchestrator · Revision: 10

Evidence: Architect answered (pumps at the boundary, .wash/local/destroy-simplify.md); built as K19, done, merged in f4d3b41d9.

### Orchestrator · Question

Event: `2b52be7b5689700f9dd48aa1b6717bd7` · 2026-10-03T00:35:19Z

> From the owner, a design question, not a package yet. message.rs (1,758 lines) and budget.rs (1,276) are 40% of the kernel, and nearly every kernel finding of the last two weeks sat at their seam: a subtree destroyed while IPC is in flight (K9 cost follows the dying subtree; K10 endpoint destroy stops scanning threads; K12 R10 at full fill; K13 one pump per endpoint; K17 doomed takes nothing; K18 audits off the schedule; GATE1's three notice threads). Each fix was right and small, and the rules are now stated (R4b, 'doomed' on budgets.md R10 step 3, I15), but the owner asks: with a simplifier's hat on, what could make the destroy path simpler as a whole, so the next class of bugs is impossible rather than caught? Please think about it from the pages and the code, not from the fixes: (1) is there one mechanism doing two jobs (destruction deciding IPC outcomes, or IPC walking budgets) that could be one job each; (2) could destruction be a two-phase 'mark doomed, then reap' where the reap has nothing to decide, so no walk during a kill picks a receiver or a sender; (3) does R10's cost rule force complexity that a different accounting (charge the destroyer up front, say) would remove; (4) what K16's walks-by-live-thread already simplifies and what it leaves; (5) what would have to be given up (a target, a rule, a property) for each option. A short written assessment with your recommendation, on this thread or in .wash/local/destroy-simplify.md; no page edits yet. If you conclude it is already as simple as the properties allow, say so and why.

### Architect · Answer

Event: `9e043a53350f6cade814a5b86a2019ae` · 2026-10-03T00:37:36Z

> Assessment: ./.wash/local/destroy-simplify.md. No page edits.
> 
> Not as simple as the properties allow. R10's nine steps are each needed by a rule, and its billing is about ten lines. The fragility comes from two structural facts:
> (a) Destruction runs IPC's live path mid-kill. Each kill's process_ending and settle_notice pump endpoints after the mark, before step 4 fails what is tied to dying budgets. So IPC carries five dying/doomed checks, each from a finding (K17's receiver_on, abandon, allowed, drop_dying_notices, budgets_dying's ordering). The pump does two jobs: deliver, and carry on after a teardown.
> (b) An endpoint has no state of its own. Its queues, receivers and notices live in thread slots, so every question about an endpoint is an all-threads walk.
> One consequence of (a), to confirm by trace: a kill's pump can deliver a send stamped with a dying budget, which step 4 would otherwise fail with Dead. Kill order then decides the outcome.
> 
> Recommend one package after K16 (Tier A, M), "pumps at the boundary":
> - Each operation changes state and collects the endpoints to pump; its end pumps each once (K13's rule, made general).
> - Destruction becomes mark, sever (one pass, no pumps), reap, then pump.
> - The five checks and the rescan loop go. R4b's "doomed takes nothing" becomes structural, and its mutation is retired as unwritable. R10 step 4 tightens to "from the mark on".
> Nothing in the rules or targets is given up.
> 
> Rejected:
> - charging the destroyer up front: it needs a cost estimate and simplifies nothing;
> - deferred reaping: dying state would leak across kernel entries.
> Endpoint-owned queues would fix (b), but only decide on them if K16's worst-walk numbers demand it. K16's walks bound the cost, not the shape.

### Orchestrator · Assigned

Event: `8f73d916dce827166db9a7b2e3ecc2c0` · 2026-10-04T21:35:38Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `d2973939ef2355a8f9f006dc8fcd3ef0` · 2026-10-05T03:06:29Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `7f563f31a9041a88df7f639f1cca838d` · 2026-10-05T19:26:49Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `beeb78b47942296d722a20f693aad0d5` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `3106ff7f96ffa9cc5d343daff91db1ef` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `e74b05178448c7892587b21b5f57ea18` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `3bf7a320fef5305541bc8a65e34d633d` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `4cb0baa0022c04cc85e1d8b56e48e474` · 2026-10-20T16:45:28Z

> Evidence: Architect answered (pumps at the boundary, .wash/local/destroy-simplify.md); built as K19, done, merged in f4d3b41d9.
