<!-- wash-qa-thread: RT1-unsafe -->
<!-- wash-qa-checkpoint-v3: {"thread":{"id":"RT1-unsafe","node":"M1","title":"RT1: the runtime's unsafe, reduced to its floor and attacked","creator":"4da9ce984a47023592aadafbc57addd0","assignee":"36d2860926662b96ec41a2154eacfa40","state":"resolved","blocking":false,"revision":10,"decision_refs":null,"evidence":"Architect wrote the node and brief; RT1 done, merged in af75ce269.","events":[{"id":"8f013e69c8b749e24c8806fc20583250","author":"4da9ce984a47023592aadafbc57addd0","kind":"open","body":"From the owner: cut a small package reducing libs/rt's unsafe (the ratchet: 11 today, 12 after INIT2 adds the bundle view) to its floor and attacking what stays; their note: mostly justified, and a process is memory-safe from other processes regardless, so this is TCB hygiene, not a hole. Please write the node (RT1, Tier A, S, needs INIT2: INIT2 adds Heap::fixed, Registers::unmap and the bundle view in start.rs, so RT1 starts from the merged runtime) and the brief at .wash/local/RT1-implementer.md. What I see from the code, for you to confirm or correct: removable - start.rs:159's transmute of the panic hook's stored address back to fn() (store a fn() in a static instead); heap.rs's free list threaded through freed blocks (two raw reads/writes; a per-page bitmap per class, or keep with a reason); ipc.rs's two buffer views folded onto one unsafe fn slice_at(addr, len) so the invariant lives once. The floor - GlobalAlloc's three (by trait), MMIO read/write_volatile (by hardware), map_anon's address-to-slice, the startup page's first read, and now the bundle view. For what stays: one keeper test per site, Miri over the heap and the buffer views (rt-miri covers the parse side only today), and the SAFETY comments stating the invariant each rests on and who guarantees it (the loader, the kernel, the owner type). Pages: native.md's runtime section and the ratchet's name line; the budget falls to the new count. Say if any of the removals costs clarity the tenets would not pay.","message_id":"631def9d2e3712d4ff4b99c36fcdbdfb","created_at":1790991705799},{"id":"bb370f781d695194f008e910de72762c","author":"48984a831826f79326d789d999769683","kind":"answer","body":"Node RT1 written (todo, M1, Tier A, S, needs INIT2). Brief with exact page lines: .wash/local/RT1-implementer.md.\n\nCorrections:\n- After INIT2 the rt ratchet is 14, not 12: the bundle view, plus Heap::fix's two run words.\n- map_anon has no unsafe: it hands back an address, and Buffer/Mapping own the views.\n- rt-miri does NOT cover the heap in the bench. heap_over_map_anon and INIT2's arena tests sit in tests/ipc.rs, which runs by hand. RT1 moves them to a fast file in rt-miri's list.\n\nRemovals (14 -\u003e target 10):\n- Panic hook: yes, but not as a fn() in a static, which needs an unsafe Sync cell or the same transmute. entry!(run, panic_hook = f) emits the program's panic handler, which calls rt's run-once logic with f. set_panic_hook, PANIC_HOOK and a registration race go; netd is the only user. If no sound form exists, the transmute stays with its reason.\n- Heap: the free list's push/pop and the arena's run/set_run fold onto one private by-value pair, words/set_words, with the invariant stated once (4 -\u003e 2). No per-page bitmap: links in freed blocks are standard, and a side table adds structure for no safety.\n- start.rs: the startup page and the bundle view fold onto one private premapped() (2 -\u003e 1). Both are pages mapped read-only before the first instruction, never unmapped.\n\nNot removed: ipc's views (an unsafe fn slice_at called twice counts 3, and a shared borrow cannot soundly give a mutable view); the MMIO read/write pair; GlobalAlloc's three.\n\nKeepers: the heap under Miri (classes, arena first fit, split, exhaustion, reuse); mapping_views, which exists; a Registers Miri test if the fake can map host memory, else the netd/blkd cases named; boot cases for premapped.\n\nNo removal costs clarity, and two gain it. Pages: native.md \"Start and end\" plus a new \"Its unsafe is few and attacked\" bullet, testbench.md's rt-miri sentence, and the toml name line.","message_id":"a71ce58388b711b2b36b466a547b17c0","created_at":1790991878962},{"id":"03b0c966dd8a0226d250b5ec6def2b75","author":"5e72d6c884926b33a54b579af9bb50de","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791149738695},{"id":"7de054f5be554d2e8767f4655204d707","author":"2dca6f931423db09b53b7b9065ea2d5d","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791169589526},{"id":"c4c39a874a8021eac8cda6f9b7ecbc48","author":"cab495a4d2c090f3d5882b1a537494b9","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791228409424},{"id":"4aecab0051799908214c319cd06d5839","author":"da644642022a19c5c33e271e726084f1","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1791575239547},{"id":"29170a30109b6e9369ae7cf835fc7bb2","author":"69971cc67de8ba82798d0a1811447b0b","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792006216142},{"id":"264d2caa20e5929f0d91019d59479659","author":"12b03266f7cf38d33506dc0a1ea74659","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792501688590},{"id":"3eb5e5888d0279a09206b2e6954c6871","author":"36d2860926662b96ec41a2154eacfa40","kind":"assign","body":"Resumed from the QA directory; the orchestrator assigns the current team.\nNext responder: Orchestrator","created_at":1792507600639},{"id":"b9031daf28846d4335663e5e1ca91840","author":"36d2860926662b96ec41a2154eacfa40","kind":"resolve","body":"Evidence: Architect wrote the node and brief; RT1 done, merged in af75ce269.","created_at":1792514728893}]},"authors":{"12b03266f7cf38d33506dc0a1ea74659":"Orchestrator","2dca6f931423db09b53b7b9065ea2d5d":"Orchestrator","36d2860926662b96ec41a2154eacfa40":"Orchestrator","48984a831826f79326d789d999769683":"Architect","4da9ce984a47023592aadafbc57addd0":"Orchestrator","5e72d6c884926b33a54b579af9bb50de":"Orchestrator","69971cc67de8ba82798d0a1811447b0b":"Orchestrator","cab495a4d2c090f3d5882b1a537494b9":"Orchestrator","da644642022a19c5c33e271e726084f1":"Orchestrator"}} -->

## M1 · RT1-unsafe — RT1: the runtime's unsafe, reduced to its floor and attacked

Status: **resolved** · Assigned to: Orchestrator · Revision: 10

Evidence: Architect wrote the node and brief; RT1 done, merged in af75ce269.

### Orchestrator · Question

Event: `8f013e69c8b749e24c8806fc20583250` · 2026-10-03T01:41:45Z

> From the owner: cut a small package reducing libs/rt's unsafe (the ratchet: 11 today, 12 after INIT2 adds the bundle view) to its floor and attacking what stays; their note: mostly justified, and a process is memory-safe from other processes regardless, so this is TCB hygiene, not a hole. Please write the node (RT1, Tier A, S, needs INIT2: INIT2 adds Heap::fixed, Registers::unmap and the bundle view in start.rs, so RT1 starts from the merged runtime) and the brief at .wash/local/RT1-implementer.md. What I see from the code, for you to confirm or correct: removable - start.rs:159's transmute of the panic hook's stored address back to fn() (store a fn() in a static instead); heap.rs's free list threaded through freed blocks (two raw reads/writes; a per-page bitmap per class, or keep with a reason); ipc.rs's two buffer views folded onto one unsafe fn slice_at(addr, len) so the invariant lives once. The floor - GlobalAlloc's three (by trait), MMIO read/write_volatile (by hardware), map_anon's address-to-slice, the startup page's first read, and now the bundle view. For what stays: one keeper test per site, Miri over the heap and the buffer views (rt-miri covers the parse side only today), and the SAFETY comments stating the invariant each rests on and who guarantees it (the loader, the kernel, the owner type). Pages: native.md's runtime section and the ratchet's name line; the budget falls to the new count. Say if any of the removals costs clarity the tenets would not pay.

### Architect · Answer

Event: `bb370f781d695194f008e910de72762c` · 2026-10-03T01:44:38Z

> Node RT1 written (todo, M1, Tier A, S, needs INIT2). Brief with exact page lines: .wash/local/RT1-implementer.md.
> 
> Corrections:
> - After INIT2 the rt ratchet is 14, not 12: the bundle view, plus Heap::fix's two run words.
> - map_anon has no unsafe: it hands back an address, and Buffer/Mapping own the views.
> - rt-miri does NOT cover the heap in the bench. heap_over_map_anon and INIT2's arena tests sit in tests/ipc.rs, which runs by hand. RT1 moves them to a fast file in rt-miri's list.
> 
> Removals (14 -> target 10):
> - Panic hook: yes, but not as a fn() in a static, which needs an unsafe Sync cell or the same transmute. entry!(run, panic_hook = f) emits the program's panic handler, which calls rt's run-once logic with f. set_panic_hook, PANIC_HOOK and a registration race go; netd is the only user. If no sound form exists, the transmute stays with its reason.
> - Heap: the free list's push/pop and the arena's run/set_run fold onto one private by-value pair, words/set_words, with the invariant stated once (4 -> 2). No per-page bitmap: links in freed blocks are standard, and a side table adds structure for no safety.
> - start.rs: the startup page and the bundle view fold onto one private premapped() (2 -> 1). Both are pages mapped read-only before the first instruction, never unmapped.
> 
> Not removed: ipc's views (an unsafe fn slice_at called twice counts 3, and a shared borrow cannot soundly give a mutable view); the MMIO read/write pair; GlobalAlloc's three.
> 
> Keepers: the heap under Miri (classes, arena first fit, split, exhaustion, reuse); mapping_views, which exists; a Registers Miri test if the fake can map host memory, else the netd/blkd cases named; boot cases for premapped.
> 
> No removal costs clarity, and two gain it. Pages: native.md "Start and end" plus a new "Its unsafe is few and attacked" bullet, testbench.md's rt-miri sentence, and the toml name line.

### Orchestrator · Assigned

Event: `03b0c966dd8a0226d250b5ec6def2b75` · 2026-10-04T21:35:38Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `7de054f5be554d2e8767f4655204d707` · 2026-10-05T03:06:29Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `c4c39a874a8021eac8cda6f9b7ecbc48` · 2026-10-05T19:26:49Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `4aecab0051799908214c319cd06d5839` · 2026-10-09T19:47:19Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `29170a30109b6e9369ae7cf835fc7bb2` · 2026-10-14T19:30:16Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `264d2caa20e5929f0d91019d59479659` · 2026-10-20T13:08:08Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Assigned

Event: `3eb5e5888d0279a09206b2e6954c6871` · 2026-10-20T14:46:40Z

> Resumed from the QA directory; the orchestrator assigns the current team.
> Next responder: Orchestrator

### Orchestrator · Resolved

Event: `b9031daf28846d4335663e5e1ca91840` · 2026-10-20T16:45:28Z

> Evidence: Architect wrote the node and brief; RT1 done, merged in af75ce269.
