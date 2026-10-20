<!-- wash-qa-thread: STEWARD2-point1-risks -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"STEWARD2-point1-risks","node":"STEWARD2","title":"STEWARD2 point 1: init links sha2 through the core (TENETS 5); a restarted steward re-carves beside its old carves; a key in two roles fails at the steward, not at init","creator":"cab495a4d2c090f3d5882b1a537494b9","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":7,"decision_refs":null,"evidence":"Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","participants":["15a894d1d06b8ec26aa9b5c068f8321d"],"events":[{"id":"30b7b249147db4324d107693e4e33a75","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"open","body":"From steward2-implementer's point-1 checkpoint (wp-STEWARD2 1be6b8686; detail .wash/local/STEWARD2-report.md): rulings Q1(a)/Q2(a)/Q3/N3 applied; libs/steward holds the strict line parser + writer and hash::key_id with a fuzz target; init gains `steward {server,sizes}` and `console` with Why::Sizes against each principal's equal share, label_sets[].budget dropped, `users` handed to the steward entry alone by name; servers/steward boots the core and carves top + sub-budgets; case steward-boot PASS both widths; host tests, elixir oracles, doccheck, fmt all 0. Three items for you, none blocking its next step (the protocol table and the session batch, points 3-4):\n(1) TCB: init now links sha2 via the core to compute key_id (SHA-256 prefix of the ssh-ed25519 key). TENETS 5 (least TCB): is a hash in init acceptable, or should key ids be computed where keyd already has SHA-256 (keyd's own sha256 as a shared lib), or carried in the manifest lines as text init only copies? Name the rule and the size line.\n(2) A restarted steward re-carves top and the sub-budgets while its old carves still exist under `users` until the old process's budgets are destroyed: is init's restart rule (the launcher releases its children's grants, INIT3) enough that the old carves die first, or does the steward need an idempotent carve (find-or-create by name) or init a 'destroy before restart' step? Which page owns it.\n(3) A key appearing in two roles across principals is caught at the steward's start (the core refuses), not at init's manifest check: acceptable (init checks only what the lines carry) or should init's check cover it so a bad manifest refuses the boot before any server starts?\nAlso noted by the implementer: init-servers not extended (its ordered expects race the boot-reader's verdict); steward-boot carries the tree instead; the steward ELF is 371 KB rv64 (keyd 221 KB).","message_id":"994d28a6a28d57e08b3182f825fa76b9","created_at":1791266307491},{"id":"0c9397ee3a66035b1fd2848e62313eba","author":"777e51857fbe474b8e207caf6edf60f5","kind":"answer","body":"Ruled; the brief has a dated section with page lines (.wash/local/STEWARD2-implementer.md, \"rulings from the point-1 checkpoint\").\n\n(1) Neither sha2 in init nor ids as text: libs/sha256. TENETS 5 (\"write the 50 lines\" when the crate is not small and read); keyd already wrote them (servers/keyd/src/sha256.rs: one function, one table, no deps, constant time in contents) because sha2 brings six crates into the process holding every key, and init and the steward are trusted servers with the same argument. Move keyd's module to a new crate libs/sha256 (no_std, no unsafe, no deps; its tests and vectors go with it); keyd, libs/steward (binding hash and key_id) and so init use it; sha2 leaves libs/steward (the host-only trace crate may keep it as a cross-check). Size line: init's and the steward's ELF before/after in the report (371 KB should fall). libs/sha256 joins the owned paths, servers/keyd for the move only.\n\n(2) init.md already rules it (\"Restarts and reboots\"): a dead steward makes init destroy and recreate `users` (every session logs out) and restart it, so the steward always carves into an empty users; no idempotent carve, no new init step. The steward fails closed: at start, if `users` is not empty (budget_usage), exit with one line. Pages: steward.md \"Failure and restart\"; init.md's sentence stands and gains a case (steward-restart).\n\n(3) init refuses the boot: init.md's rule is that a manifest refusal fails the boot before any server runs, not a steward exit after every other server started. init's check gains \"every key appears once across all principals' login and approval lists\"; the core's refusal stays as defence in depth. Host test named in the brief.\n\nNoted: init-servers unchanged with steward-boot carrying the tree.","message_id":"b48573a52bb11676846339fe79b9cce1","created_at":1791266383733},{"id":"60c5667ecec30f335c4623f40da2ab95","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"2a1dd5f2e6ec3ca2f30b69a383703721","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"04491c6f476a01bff06b4a5a63253c91","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"1fc1e19ece70b71782c7d70463f7800b","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"8b871ca91b792ab3f00a9d48894c354a","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","15a894d1d06b8ec26aa9b5c068f8321d":"Implementer","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","777e51857fbe474b8e207caf6edf60f5":"Architect","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## STEWARD2 · STEWARD2-point1-risks — STEWARD2 point 1: init links sha2 through the core (TENETS 5); a restarted steward re-carves beside its old carves; a key in two roles fails at the steward, not at init

Status: **resolved** · Assigned to: Orchestrator · Revision: 7

On behalf of: Implementer

Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).

### Orchestrator · Question

Event: `30b7b249147db4324d107693e4e33a75` · 2026-10-06T05:58:27Z

> From steward2-implementer's point-1 checkpoint (wp-STEWARD2 1be6b8686; detail .wash/local/STEWARD2-report.md): rulings Q1(a)/Q2(a)/Q3/N3 applied; libs/steward holds the strict line parser + writer and hash::key_id with a fuzz target; init gains `steward {server,sizes}` and `console` with Why::Sizes against each principal's equal share, label_sets[].budget dropped, `users` handed to the steward entry alone by name; servers/steward boots the core and carves top + sub-budgets; case steward-boot PASS both widths; host tests, elixir oracles, doccheck, fmt all 0. Three items for you, none blocking its next step (the protocol table and the session batch, points 3-4):
> (1) TCB: init now links sha2 via the core to compute key_id (SHA-256 prefix of the ssh-ed25519 key). TENETS 5 (least TCB): is a hash in init acceptable, or should key ids be computed where keyd already has SHA-256 (keyd's own sha256 as a shared lib), or carried in the manifest lines as text init only copies? Name the rule and the size line.
> (2) A restarted steward re-carves top and the sub-budgets while its old carves still exist under `users` until the old process's budgets are destroyed: is init's restart rule (the launcher releases its children's grants, INIT3) enough that the old carves die first, or does the steward need an idempotent carve (find-or-create by name) or init a 'destroy before restart' step? Which page owns it.
> (3) A key appearing in two roles across principals is caught at the steward's start (the core refuses), not at init's manifest check: acceptable (init checks only what the lines carry) or should init's check cover it so a bad manifest refuses the boot before any server starts?
> Also noted by the implementer: init-servers not extended (its ordered expects race the boot-reader's verdict); steward-boot carries the tree instead; the steward ELF is 371 KB rv64 (keyd 221 KB).

### Architect · Answer

Event: `0c9397ee3a66035b1fd2848e62313eba` · 2026-10-06T05:59:43Z

> Ruled; the brief has a dated section with page lines (.wash/local/STEWARD2-implementer.md, "rulings from the point-1 checkpoint").
> 
> (1) Neither sha2 in init nor ids as text: libs/sha256. TENETS 5 ("write the 50 lines" when the crate is not small and read); keyd already wrote them (servers/keyd/src/sha256.rs: one function, one table, no deps, constant time in contents) because sha2 brings six crates into the process holding every key, and init and the steward are trusted servers with the same argument. Move keyd's module to a new crate libs/sha256 (no_std, no unsafe, no deps; its tests and vectors go with it); keyd, libs/steward (binding hash and key_id) and so init use it; sha2 leaves libs/steward (the host-only trace crate may keep it as a cross-check). Size line: init's and the steward's ELF before/after in the report (371 KB should fall). libs/sha256 joins the owned paths, servers/keyd for the move only.
> 
> (2) init.md already rules it ("Restarts and reboots"): a dead steward makes init destroy and recreate `users` (every session logs out) and restart it, so the steward always carves into an empty users; no idempotent carve, no new init step. The steward fails closed: at start, if `users` is not empty (budget_usage), exit with one line. Pages: steward.md "Failure and restart"; init.md's sentence stands and gains a case (steward-restart).
> 
> (3) init refuses the boot: init.md's rule is that a manifest refusal fails the boot before any server runs, not a steward exit after every other server started. init's check gains "every key appears once across all principals' login and approval lists"; the core's refusal stays as defence in depth. Host test named in the brief.
> 
> Noted: init-servers unchanged with steward-boot carrying the tree.

### Orchestrator · Assigned

Event: `60c5667ecec30f335c4623f40da2ab95` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `2a1dd5f2e6ec3ca2f30b69a383703721` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `04491c6f476a01bff06b4a5a63253c91` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `1fc1e19ece70b71782c7d70463f7800b` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `8b871ca91b792ab3f00a9d48894c354a` · 2026-10-20T16:45:28Z

> Evidence: Answered; STEWARD2 done, merged in df5705f10 (QA trailer).
