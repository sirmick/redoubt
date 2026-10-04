IPC2 handoff. Branch wp-ipc2 at 08807fb8f, rebased on main 4ace1722d, tree clean. Full detail: /home/mcloonan/redoubt/.wash/local/IPC2-progress.md (read its last two sections).

Commits:
1 1c38cdc10 model: steward numbers sessions/agents per (principal, label set). Size 10092->10093.
2 72b949486 model: lease end audited with the lease's labels, plus steward.md:348 'request's or target's labels' and the lease-end sentence. Size ->10095.
3 f33f3b14f testbench: process case staggers its two callers by 50 ms.
4 08807fb8f kernel: a receive takes the group served least recently. Holds kernel and model R2, R2OneCursor, P10 (option a), the steward take log, the bench case ipc-fair-label-sets, pages, the todo page deleted, size ->10179.

Fix round 1: every item is applied and folded into commit 4, except red F4 and editor 4, which went into commit 2. Simplifier 1: the cursor write in served() is gated under broken(R2OneCursor), keeping the mutation exactly today's cursor; a restamp of every message would be a different rule (lowest key first). Still caught at seed 18.

Timing: the host was at load 60-117 on 24 cores, so wall times are unreliable. P10 alone, user CPU: 17m35s before, 17m11s after. model-host-tests now: 497.5 s wall (446.5 s before, at lower load). Main vs tip was not measured: `git checkout main` fails because main is the shared checkout's; use 4ace1722d.

Not done: the whole bench. Run `/home/mcloonan/redoubt/.wash/local/in-dev cargo testbench --allow-skip` from the worktree, in the background; expect exactly one SKIP, bench-ssh-loopback-openssh. Then report and take the merge.

Gates run, all via in-dev:
- fmt --all --check: 0
- cargo run -q -p redoubt-doccheck: 0
- cargo testbench ipc-fair-label-sets: 0 (both widths; the new round gives [3,1,3])
- cargo testbench process: 0
- cargo testbench model-host-tests: 0
- size-budget: PASS at tip and at commits 1-2

Traps:
- The kernel's W_DUE (thread word 16) is a group's turn and W_SEQ is its arrival; both are needed. A take restamps every queued message of that group with mm.next_seq(), at most WAIT_CAP.
- The model's Endpoint::cursor is read and written only under R2OneCursor; Msg::due comes from next_msg.
- P10 harness choices: an op naming a session made by vault work is vault work; the session an op names is renamed into the without-world by the op that started it, and results are never renamed; Hold/Crash and vault CrashServing stay out; CrashServing{s} serves until it takes s's call.
- Size budget: every commit that raises it needs a 'Size budget: model: <reason>' line.
- Never git checkout main.
