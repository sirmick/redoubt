K13 handoff (parked). Branch wp-k13, clean, tip 59566c175 on base main 4d42be45f. No kernel change has been made. Detail: /home/mcloonan/redoubt/.wash/local/K13-progress.md

THE CASE (59566c175, committed alone): tests/ending-pumps-once.toml, tests/programs/src/bin/ending-pumps-once.rs, plus the tests/programs/Cargo.toml bin entry. Checked build, rv64 and rv32.
Setup:
- The judge, the first program, makes E and spawns P (800-page budget) with E's receive right.
- P tid 1 receives on E until it has taken MAX_OPEN_CALLS (64) messages. The judge fills it from one thread with calls on a 2 ms timeout: each is taken, abandoned, and stays open, so R4a holds.
- The judge's thread S receives on E. P tid 2 (C) calls E, and S takes and holds that call.
- P tid 3 (R) receives on E.
- The judge's thread Q calls E. It is queued: R4a blocks R, and no judge thread is receiving.
- The judge destroys P's budget.
It then checks three things: Q is still waiting; a new judge thread takes Q's call on E and Q gets its reply; S's receives until a 50 ms timeout show exactly one Abandoned(C's id) and nothing else.
Failing line on main, both widths (exit 1): '[ending-pumps-once] FAIL: Q is still waiting after the kill (got 2)', where 2 means Err(Dead). The two setup checks before it pass. The bench stops at the first FAIL, so the later checks are only reached once the fix is in.

PUMP MAP:
- Inside process_ending -> thread_ending, the only pump is thread_ending's pump of the served endpoint (message.rs:1491), and it runs after each thread.
- unwind does not pump: give_buffer_back and abandon take &ProcessTable.
- finish_served does not pump: return_lend and free_abandoned_lend take &ProcessTable, close_call takes no ss, and wake only readies the thread and sets its result.
- On the kill path but outside process_ending: kill_process/terminate_process -> destroy_quarantined_devices, and process::settle_notice -> pump_endpoint on the exit endpoint, which is not E here.
- fail_wait's pump (message.rs:554) is not reached.

THE FIX I WOULD MAKE (message.rs, process_ending and thread_ending only):
- One private helper does the thread's teardown: unwind, W_WAIT := None, then drop_open_call and finish_served for each open call. It returns the served endpoint.
- thread_ending calls the helper and pumps at once, so a lone thread's exit is unchanged.
- process_ending calls the helper for every thread and gathers the endpoints into [Option<EndpointRef>; MAX_THREADS], each once, with no heap. After the last thread it pumps each once.

THEN:
1. Run the brief's cases on both widths: ending-pumps-once, redoubt-dead, process-lifecycle, the abandon/notice hits from --list, endpoint-destroy-full, budget-destroy-growth, sched-latency.
2. Remeasure at the full fill with test-only phase records like K15's U/V (sched.rs trace::audit), named on the page.
3. Update the pages: ipc.md R4b status line and residual bullet, budgets.md numbers, SECURITY.md R4b row, SUMMARY.md, and delete the todo page.
4. Run the gates.
Run every command through /home/mcloonan/redoubt/.wash/local/in-dev.
