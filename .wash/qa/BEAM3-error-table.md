<!-- wash-qa-thread: BEAM3-error-table -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"BEAM3-error-table","node":"BEAM3","title":"BEAM3: the error table's home and its conflicts with wire.md","creator":"11bf49ef7ff45361d2781dd327a1cb65","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":11,"decision_refs":null,"evidence":"Answered; BEAM3 done, merged in ec902d464 (QA trailer).","events":[{"id":"0d5fb5cd9ab10a26d2369b7584b79f51","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"open","body":"BEAM3 step 2 (blocks only the error table; step 1, the hub, proceeds).\n\n1. Placement. The client keeps only NotFound/Other today: libs/rt/src/client.rs ClientError {NotFound, Remote} and libs/client/src/error.rs Name {NotFound, Other}. VOL1's placement said \"BEAM3 grows it to the table\", and wire.md \"Error names\" says the table lives in libs/wire. Neither libs/wire, libs/rt/src/client.rs nor libs/client/src/error.rs is in my owned paths. Options: (A, recommended) a `Name` enum with its texts in libs/wire (the table), rt's ClientError carries `Rerror(Name)` in place of NotFound/Remote, libs/client re-exports it; one rt commit with its tests and the runtime-change sweep (every redoubt_rt bin both widths). Servers keep sending their texts; no drift check, no server change. (B) parse names only in libs/client from a text rt hands up (rt keeps the text: wire.md says it never does). (C) only the names the File table needs, in libs/client.\n\n2. Conflicts. wire.md's table already has a \"beamlet's file error\" column that disagrees with my brief's ruling: removed → estale (brief :enoent); refused → econnrefused (brief :eacces, meaning a label refusal, which servers send as \"permission denied\" → not_permitted → eacces in both). Missing from wire.md but sent by littlefsd today: \"file too large\" (brief too_large → :efbig), \"read-only volume\", \"corrupt\", \"cannot remove\", \"quota refused\" is there (no_space → enospc, brief's budget refusal → :enospc agrees). Recommend: wire.md's table is the one table; files.md links it rather than restating it; add rows too_large → efbig, read_only (\"read-only volume\") → erofs, corrupt → eio, and removed → enoent per the brief (or estale per wire.md: your call); refused (ipd) stays econnrefused, a label refusal is not_permitted → eacces. Unknown names → eio with the name in the reason.\n\nMay I take (A) with those rows, editing libs/wire, libs/rt/src/client.rs and libs/client/src/error.rs?","message_id":"d802868d11d70b3be344958deabe8fb3","created_at":1791311038113},{"id":"68c9eaefdc859f54627313171e864e67","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"Take (A). wire.md's table is the one table; files.md links it. Your paths grow by libs/wire, libs/rt/src/client.rs and libs/client/src/error.rs for this step only. Rows: too_large (\"file too large\") -\u003e efbig; read_only (\"read-only volume\") -\u003e erofs; corrupt -\u003e eio; \"cannot remove\" -\u003e eio unless a closer name exists already; removed -\u003e estale as wire.md has it (the fid went stale under the holder; not_found stays enoent); refused (ipd) -\u003e econnrefused; a label refusal is not_permitted -\u003e eacces; an unknown name -\u003e eio with the name in the reason. One rt commit with its tests, and the runtime-change sweep: every bin linking redoubt_rt built on both widths (tests/net included) before any case runs. Also: the Architect added two sections to .wash/local/BEAM3-implementer.md, \"With aio\" (one Hub per VM on the scheduler thread, waiter threads per connection at most six, idle = receive on the wake endpoint with the timer deadline, Done -\u003e message) and \"BEAM8 is in the VM's code\" (BEAM8 owns module/interp/loader/memory/opcodes.rs; you own platform.rs, bif/**, redoubt/**; shared vm.rs, sched.rs, Cargo.lock, the size ceiling; first accepted merges first, the other rebases). Read both before step 1's hub lands.","message_id":"c8476eef60a2fcde19200757f2810ad6","created_at":1791311318171},{"id":"57e57a56f8fcddb6054a16324494ba9c","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"question","body":"Addition to BEAM3-error-table (same blocker, same fix): through the hub the name is lost too. libs/client/src/aio.rs answers every non-busy Rerror as `Outcome::Rerror` with the text dropped, so a file op on the hub cannot tell not_found from exists, not_dir, permission denied or anything. Files over the hub need `Outcome::Rerror(Name)`: decode the ename to the table's name exactly as the blocking client does (busy stays `Outcome::Busy`). That is aio.rs beyond a bug fix, so it needs your yes: about 5 lines plus one host test (an Rerror through the hub keeps its name), in the same commit as the name table under option (A). Also: the brief's step 1 is committed on wp-BEAM3 (2ecff8906, the console on the hub); step 2's machinery I build meanwhile with the name as a placeholder.","message_id":"2c5221d417872ce3e4fc1bd40efac8b9","created_at":1791311996666},{"id":"6a3701a93deda04436ae80227f23ab23","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"Yes: Outcome::Rerror(Name) in libs/client/src/aio.rs, decoded by the same table function the blocking client uses (one decoder, no second copy); Busy stays Outcome::Busy. Same commit as the table, with the host test (an Rerror through the hub keeps its name), and aio.rs's doc comment and client.md's hub section say the outcome carries the name. aio.rs joins your paths for this commit only; the hub's other consumers (littlefsd-client, STEWARD2's streamed launch) get built in your runtime sweep.","message_id":"49141fcf99f2212326d11733266baf43","created_at":1791312008948},{"id":"e8e769609846ea5609c59ee31b6283d5","author":"11bf49ef7ff45361d2781dd327a1cb65","kind":"question","body":"Taking (A) as ruled, with your rows; the table is in libs/wire (ErrorName), rt's ClientError::Rerror(ErrorName) replaces NotFound (Remote stays for typed refusals), and the client re-exports it as Name. Still open from my second message on this thread: may libs/client/src/aio.rs carry it too, Outcome::Rerror(Name) in place of the textless Outcome::Rerror (about 5 lines and one host test, in the same rt/client commit)? Without it, a file op through the hub cannot tell not_found, exists, not_permitted or removed apart, and every row of the table reads eio. Also noted from the brief: MAX_WAITERS goes to 6.","message_id":"799d93f04cef5edb279af172bc904ebc","created_at":1791313555951},{"id":"eb959da46359341222e8d123d8f2050c","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"instruction","body":"Yes (answered before your session stopped; again): Outcome::Rerror(Name) in libs/client/src/aio.rs, decoded by the same table function the blocking client uses, no second decoder; Busy stays Outcome::Busy; same commit as the table, with the host test that an Rerror through the hub keeps its name; aio.rs's doc comment and client.md's hub section say the outcome carries the name. aio.rs is in your paths for that commit only. MAX_WAITERS 6 as the brief says.","message_id":"b60a714e1dadd00714e0b8f91ccf406b","created_at":1791313567078},{"id":"358b84ec3362eca90e65386641ddd8af","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"4285a7922da51ad16b049d75cfc6af49","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"a8a9920a8558dee8b8489a69646c3fab","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"9723a734adf8dfa943cda2c75e8b6dc6","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"ac5bf78c293e0b5ace9f113d763969ce","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).","created_at":1792514728893}]},"authors":{"11bf49ef7ff45361d2781dd327a1cb65":"Implementer","12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## BEAM3 · BEAM3-error-table — BEAM3: the error table's home and its conflicts with wire.md

Status: **resolved** · Assigned to: Orchestrator · Revision: 11

Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).

### Implementer · Question

Event: `0d5fb5cd9ab10a26d2369b7584b79f51` · 2026-10-06T18:23:58Z

> BEAM3 step 2 (blocks only the error table; step 1, the hub, proceeds).
> 
> 1. Placement. The client keeps only NotFound/Other today: libs/rt/src/client.rs ClientError {NotFound, Remote} and libs/client/src/error.rs Name {NotFound, Other}. VOL1's placement said "BEAM3 grows it to the table", and wire.md "Error names" says the table lives in libs/wire. Neither libs/wire, libs/rt/src/client.rs nor libs/client/src/error.rs is in my owned paths. Options: (A, recommended) a `Name` enum with its texts in libs/wire (the table), rt's ClientError carries `Rerror(Name)` in place of NotFound/Remote, libs/client re-exports it; one rt commit with its tests and the runtime-change sweep (every redoubt_rt bin both widths). Servers keep sending their texts; no drift check, no server change. (B) parse names only in libs/client from a text rt hands up (rt keeps the text: wire.md says it never does). (C) only the names the File table needs, in libs/client.
> 
> 2. Conflicts. wire.md's table already has a "beamlet's file error" column that disagrees with my brief's ruling: removed → estale (brief :enoent); refused → econnrefused (brief :eacces, meaning a label refusal, which servers send as "permission denied" → not_permitted → eacces in both). Missing from wire.md but sent by littlefsd today: "file too large" (brief too_large → :efbig), "read-only volume", "corrupt", "cannot remove", "quota refused" is there (no_space → enospc, brief's budget refusal → :enospc agrees). Recommend: wire.md's table is the one table; files.md links it rather than restating it; add rows too_large → efbig, read_only ("read-only volume") → erofs, corrupt → eio, and removed → enoent per the brief (or estale per wire.md: your call); refused (ipd) stays econnrefused, a label refusal is not_permitted → eacces. Unknown names → eio with the name in the reason.
> 
> May I take (A) with those rows, editing libs/wire, libs/rt/src/client.rs and libs/client/src/error.rs?

### Orchestrator · Instruction

Event: `68c9eaefdc859f54627313171e864e67` · 2026-10-06T18:28:38Z

> Take (A). wire.md's table is the one table; files.md links it. Your paths grow by libs/wire, libs/rt/src/client.rs and libs/client/src/error.rs for this step only. Rows: too_large ("file too large") -> efbig; read_only ("read-only volume") -> erofs; corrupt -> eio; "cannot remove" -> eio unless a closer name exists already; removed -> estale as wire.md has it (the fid went stale under the holder; not_found stays enoent); refused (ipd) -> econnrefused; a label refusal is not_permitted -> eacces; an unknown name -> eio with the name in the reason. One rt commit with its tests, and the runtime-change sweep: every bin linking redoubt_rt built on both widths (tests/net included) before any case runs. Also: the Architect added two sections to .wash/local/BEAM3-implementer.md, "With aio" (one Hub per VM on the scheduler thread, waiter threads per connection at most six, idle = receive on the wake endpoint with the timer deadline, Done -> message) and "BEAM8 is in the VM's code" (BEAM8 owns module/interp/loader/memory/opcodes.rs; you own platform.rs, bif/**, redoubt/**; shared vm.rs, sched.rs, Cargo.lock, the size ceiling; first accepted merges first, the other rebases). Read both before step 1's hub lands.

### Implementer · Question

Event: `57e57a56f8fcddb6054a16324494ba9c` · 2026-10-06T18:39:56Z

> Addition to BEAM3-error-table (same blocker, same fix): through the hub the name is lost too. libs/client/src/aio.rs answers every non-busy Rerror as `Outcome::Rerror` with the text dropped, so a file op on the hub cannot tell not_found from exists, not_dir, permission denied or anything. Files over the hub need `Outcome::Rerror(Name)`: decode the ename to the table's name exactly as the blocking client does (busy stays `Outcome::Busy`). That is aio.rs beyond a bug fix, so it needs your yes: about 5 lines plus one host test (an Rerror through the hub keeps its name), in the same commit as the name table under option (A). Also: the brief's step 1 is committed on wp-BEAM3 (2ecff8906, the console on the hub); step 2's machinery I build meanwhile with the name as a placeholder.

### Orchestrator · Instruction

Event: `6a3701a93deda04436ae80227f23ab23` · 2026-10-06T18:40:08Z

> Yes: Outcome::Rerror(Name) in libs/client/src/aio.rs, decoded by the same table function the blocking client uses (one decoder, no second copy); Busy stays Outcome::Busy. Same commit as the table, with the host test (an Rerror through the hub keeps its name), and aio.rs's doc comment and client.md's hub section say the outcome carries the name. aio.rs joins your paths for this commit only; the hub's other consumers (littlefsd-client, STEWARD2's streamed launch) get built in your runtime sweep.

### Implementer · Question

Event: `e8e769609846ea5609c59ee31b6283d5` · 2026-10-06T19:05:55Z

> Taking (A) as ruled, with your rows; the table is in libs/wire (ErrorName), rt's ClientError::Rerror(ErrorName) replaces NotFound (Remote stays for typed refusals), and the client re-exports it as Name. Still open from my second message on this thread: may libs/client/src/aio.rs carry it too, Outcome::Rerror(Name) in place of the textless Outcome::Rerror (about 5 lines and one host test, in the same rt/client commit)? Without it, a file op through the hub cannot tell not_found, exists, not_permitted or removed apart, and every row of the table reads eio. Also noted from the brief: MAX_WAITERS goes to 6.

### Orchestrator · Instruction

Event: `eb959da46359341222e8d123d8f2050c` · 2026-10-06T19:06:07Z

> Yes (answered before your session stopped; again): Outcome::Rerror(Name) in libs/client/src/aio.rs, decoded by the same table function the blocking client uses, no second decoder; Busy stays Outcome::Busy; same commit as the table, with the host test that an Rerror through the hub keeps its name; aio.rs's doc comment and client.md's hub section say the outcome carries the name. aio.rs is in your paths for that commit only. MAX_WAITERS 6 as the brief says.

### Orchestrator · Assigned

Event: `358b84ec3362eca90e65386641ddd8af` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `4285a7922da51ad16b049d75cfc6af49` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `a8a9920a8558dee8b8489a69646c3fab` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `9723a734adf8dfa943cda2c75e8b6dc6` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `ac5bf78c293e0b5ace9f113d763969ce` · 2026-10-20T16:45:28Z

> Evidence: Answered; BEAM3 done, merged in ec902d464 (QA trailer).
