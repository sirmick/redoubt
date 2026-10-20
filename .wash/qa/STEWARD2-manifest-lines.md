<!-- wash-qa-thread: STEWARD2-manifest-lines -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"STEWARD2-manifest-lines","node":"STEWARD2","title":"STEWARD2: per-label-set budgets vs the core's equal carve; the keyd line init cannot fill; Shared(i) binding per domain","creator":"cab495a4d2c090f3d5882b1a537494b9","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","participants":["15a894d1d06b8ec26aa9b5c068f8321d"],"events":[{"id":"5e11a51e265eab5227d9b56280d75d0e","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"open","body":"From steward2-implementer's early checkpoint (edit list in .wash/local/STEWARD2-report.md; it proceeds on the parser move and key_id meanwhile). Three design gaps:\n\nQ1 (blocking, point 1): init.md's manifest gives each label_sets[] entry its own budget (pages/processes/weight); the core's Fixed::carves splits `top` equally over the sets (less cost), and the manifest lines carry only top=. (a) init drops label_sets[].budget from the schema; init.md and steward.md say 'an equal share of the top budget' [implementer's recommendation]; (b) init keeps the field but refuses one that differs from the equal share; (c) the lines and the core carry per-set limits (a core change the brief forbids).\n\nQ2 (blocking, point 1): the `keyd [..]` line needs the ids of keys keyd holds, but init has only keyd's seeds in args (no Ed25519 to derive public keys) and already asks keyd `holds` per login/approval key, refusing the boot on a yes. (a) init writes `keyd []` and the pages say R35 at the box is init's holds check (the core's guard stays, vacuous on the box) [recommendation]; (b) init asks keyd `public_key` per class and lists their ids.\n\nQ3 (before point 4): the core emits the same `servers N` Shared(i) Connect steps for every session, but a vault session has no /net and a different /home root. Proposal: the server binds Shared(i) by a fixed order (bootfsd, home fsd, ipd, consol) and per domain (vault: labelled fsd, unlabelled home read-only, the ipd step yields no entry).\n\nNotes it found: N1 sshd is a library only (no box binary or image entry): cases 2-6 need a new sshd program, owned as 'the box platform'; size grows. N3 init's BUDGETS comment says no server is handed `users`: the steward becomes R33's named exception (a page line). (N2: the console rule is committing now.) Please rule Q1-Q3, confirm N1's ownership and N3's page line, and amend the brief.","message_id":"3495e83a438a4da0c58a32fa1c499aa1","created_at":1791252445565},{"id":"9ce9ba8b732b637372fa810643057833","author":"777e51857fbe474b8e207caf6edf60f5","kind":"answer","body":"Ruled; the brief has a dated section with page lines (.wash/local/STEWARD2-implementer.md, \"rulings from the early checkpoint\").\n\nQ1: (a). The core's equal share of top per set, less budget_cost, is the rule; init drops label_sets[].budget and its check. init.md's principals row and steward.md's sub-budget section say \"an equal share\"; per-set sizes return with M5's run-time principals if evidence asks (a sentence, not an Open).\n\nQ2: (a). init writes `keyd []`. The box's R35 is init's key-separation check plus sshd's holds refusal before any login reaches the steward; the core's guard stays (model, traces) and is stated as vacuous on the box in steward.md's residuals.\n\nQ3: a binding table in the server over a fixed slot order, `servers` = the slot count: 0 bootfsd /boot; 1 home fsd at home (every session; a vault's writes are refused by fsd's R25, not the steward); 2 the labelled fsd at /vault; 3 ipd /net; 4 the console (sshd's channel or consoled). Per domain class the table says what each slot is; a slot bound to nothing for that class (2 for unlabelled, 3 for labelled) makes no call and is produced as Connection with no namespace entry. Page line on steward.md's server bullet.\n\nN1 confirmed: the sshd box binary, manifest entry, port-22 scope and typed login are yours as the box platform; size L+ (say so). N3: the manifest's `steward` object names the server entry (`\"steward\": {\"server\": \"steward\", \"sizes\": {...}}`); init hands users to that entry alone at step 6, never via `handed` (BUDGETS stands; fix its comment); page lines: step 6, R33's status, the manifest table.","message_id":"8aed351326171f1b634329481553c40f","created_at":1791252543507},{"id":"d258e44f712c4ba440ec22e3068579c5","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"5a14cb0d8f1a9ac7aa0f6e0a14dc0b37","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"ae7352a7fd4edc21e7999b5c14b2e7de","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"f04218289b1b39d7477d9cc973ffca67","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"1685f9695d398adfc2433f5408597291","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","15a894d1d06b8ec26aa9b5c068f8321d":"Implementer","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","777e51857fbe474b8e207caf6edf60f5":"Architect","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## STEWARD2 · STEWARD2-manifest-lines — STEWARD2: per-label-set budgets vs the core's equal carve; the keyd line init cannot fill; Shared(i) binding per domain

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

On behalf of: Implementer

Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).

### Orchestrator · Question

Event: `5e11a51e265eab5227d9b56280d75d0e` · 2026-10-06T02:07:25Z

> From steward2-implementer's early checkpoint (edit list in .wash/local/STEWARD2-report.md; it proceeds on the parser move and key_id meanwhile). Three design gaps:
> 
> Q1 (blocking, point 1): init.md's manifest gives each label_sets[] entry its own budget (pages/processes/weight); the core's Fixed::carves splits `top` equally over the sets (less cost), and the manifest lines carry only top=. (a) init drops label_sets[].budget from the schema; init.md and steward.md say 'an equal share of the top budget' [implementer's recommendation]; (b) init keeps the field but refuses one that differs from the equal share; (c) the lines and the core carry per-set limits (a core change the brief forbids).
> 
> Q2 (blocking, point 1): the `keyd [..]` line needs the ids of keys keyd holds, but init has only keyd's seeds in args (no Ed25519 to derive public keys) and already asks keyd `holds` per login/approval key, refusing the boot on a yes. (a) init writes `keyd []` and the pages say R35 at the box is init's holds check (the core's guard stays, vacuous on the box) [recommendation]; (b) init asks keyd `public_key` per class and lists their ids.
> 
> Q3 (before point 4): the core emits the same `servers N` Shared(i) Connect steps for every session, but a vault session has no /net and a different /home root. Proposal: the server binds Shared(i) by a fixed order (bootfsd, home fsd, ipd, consol) and per domain (vault: labelled fsd, unlabelled home read-only, the ipd step yields no entry).
> 
> Notes it found: N1 sshd is a library only (no box binary or image entry): cases 2-6 need a new sshd program, owned as 'the box platform'; size grows. N3 init's BUDGETS comment says no server is handed `users`: the steward becomes R33's named exception (a page line). (N2: the console rule is committing now.) Please rule Q1-Q3, confirm N1's ownership and N3's page line, and amend the brief.

### Architect · Answer

Event: `9ce9ba8b732b637372fa810643057833` · 2026-10-06T02:09:03Z

> Ruled; the brief has a dated section with page lines (.wash/local/STEWARD2-implementer.md, "rulings from the early checkpoint").
> 
> Q1: (a). The core's equal share of top per set, less budget_cost, is the rule; init drops label_sets[].budget and its check. init.md's principals row and steward.md's sub-budget section say "an equal share"; per-set sizes return with M5's run-time principals if evidence asks (a sentence, not an Open).
> 
> Q2: (a). init writes `keyd []`. The box's R35 is init's key-separation check plus sshd's holds refusal before any login reaches the steward; the core's guard stays (model, traces) and is stated as vacuous on the box in steward.md's residuals.
> 
> Q3: a binding table in the server over a fixed slot order, `servers` = the slot count: 0 bootfsd /boot; 1 home fsd at home (every session; a vault's writes are refused by fsd's R25, not the steward); 2 the labelled fsd at /vault; 3 ipd /net; 4 the console (sshd's channel or consoled). Per domain class the table says what each slot is; a slot bound to nothing for that class (2 for unlabelled, 3 for labelled) makes no call and is produced as Connection with no namespace entry. Page line on steward.md's server bullet.
> 
> N1 confirmed: the sshd box binary, manifest entry, port-22 scope and typed login are yours as the box platform; size L+ (say so). N3: the manifest's `steward` object names the server entry (`"steward": {"server": "steward", "sizes": {...}}`); init hands users to that entry alone at step 6, never via `handed` (BUDGETS stands; fix its comment); page lines: step 6, R33's status, the manifest table.

### Orchestrator · Assigned

Event: `d258e44f712c4ba440ec22e3068579c5` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `5a14cb0d8f1a9ac7aa0f6e0a14dc0b37` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `ae7352a7fd4edc21e7999b5c14b2e7de` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `f04218289b1b39d7477d9cc973ffca67` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `1685f9695d398adfc2433f5408597291` · 2026-10-20T16:45:28Z

> Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).
