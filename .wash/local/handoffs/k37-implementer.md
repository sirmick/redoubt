# K37 handoff (k37-implementer, 2026-10-22, checkpoint at 70 %)

## Branch state
- wp-K37 in /home/mcloonan/redoubt/.worktrees/K37, base main b6d28a6b6. Tree CLEAN.
- ac9c125ac WIP: all code (sys, kernel, model, fake, rt, blkd/verityd/netd, logsrv, rd).
- 90b6d665c WIP: ipc-client prints "[ipc] ok: 1000 writable lend round trips, N us on rv64|rv32".
- Both WIP commits must be folded into clean logical commits before review (no fix-ups). NO docs written yet.
- Scratch (outside the tree, not on the branch): .tmp/K37/wt-main (detached main + ipc-client patch + scratch tests/k37-ipc-mttcg.toml; an instrumentation edit there was REJECTED/not applied), .tmp/K37/wt-instr (K37 HEAD~1 + ipc-client patch + debug counters in kernel hart.rs/message.rs/main.rs + scratch case). Remove both with `git worktree remove --force` when done. Env file: .tmp/K37-env.sh (source it before builds). target/prebuilt in K37 is STALE (ipc-client changed after it): rerun `make -f scripts/jobs.mk -C . prebuilt` (~9 min) before set runs.

## Traps
- Never edit the worktree while prebuilt or cases run. Never put scratch inside the worktree.
- `cargo testbench` has no `--keep`; console logs are in target/testbench/run-*/<case>-<w>-smpN.log (pruned).
- Generator fusion must check the receive passes BOTH before and after the first half (a turn checks the receive handle first; a handle the send installs is not yet there) — already done.
- rt `server::Served` trait: impls for Result<T,E>, (), typed::Finished. Handlers holding replies return `Finished` (typed::finish_held).
- Ambiguity of a1=0: can't tell which half refused; rt/logsrv/netd fall back to "make the first half alone, then the receive alone". Kept on purpose.

## Design-ruling conditions
MET (in code): two calls 0x11e/0x11f, a1-a5 receive's, a6 13-slot FirstHalf record (libs/sys record.rs FIRST_SLOTS, FirstHalf::decode(send,..)), results a0 status/a1 first-half/a2 mask, decode_result refusals (ret.rs, tests). R13 ordering: stage 1 recv rec then first rec; stage 2 receive handle then first half args; stage 3 badge NotPermitted then (send) R1; stage 4 Busy; nothing done on a1=0, current call kept. Kernel (message.rs): Then state in IPC page words W_THEN..W_THEN_REC (6 new words; THREAD_WORDS 314->320 words, compile-time assert vs CONTEXT_OFFSET passes), wake() converts results for turns; turn_result debug_assert checks the row; continue_turn in deliver's Send arm: no wake, receive half starts in taker's entry, source is kernel ref (frame+id) checked live -> Dead a1=1; endpoint listed via list_pump, drained by outermost pump (pump_listed + DRAINING flag), never nested. Receive half timeout from continuation (mark). send's endpoint dying while waiting -> fail_wait -> Turn{Dead, Nothing}, transfer back. Model: Syscall::ReplyReceive/SendReceive, Ret::Turn{first, half}, Turn state on Thread, continuation, ghost turn records + Flow::Turn check in invariants (R13), mutations TurnReplyLost, TurnSendDispositionLies (rule R13), generator fusion with lookahead, no new RNG draws; test gen::turn_fusion shows 5000 seeds identical apart from fused pairs (105 reply_receive, 35 send_receive). Directed contracts turns()/turn_traces() in model/tests/common/contracts.rs; traces.rs replays them; coverage counts failed turns as failures. Fake implements both. rt: Held, Endpoint::receive_after, Endpoint::send_then_wait (Sent), typed::finish_held/Finished, server::serve holds. netd: serving loop holds; rx thread receiver::Sink holds last frame, forward_then_wait -> Device::send_then_wait (send_receive on IRQ). blkd, verityd handlers hold. logsrv holds all its replies (rd::reply_receive/send_receive added).
OPEN: docs (ipc.md: turn section + completion rows incl. the two sentences: receive timeout from first half's completion; Dead told apart by a1; abi.md: table rows 0x11e/0x11f, records table FirstHalf, order-of-checks rows for both, unknown range 0x101-0x11f; model.md mutations list; SECURITY.md R13 row = section's status set incl. mutation:TurnReplyLost, mutation:TurnSendDispositionLies (C7); serving.md rule "a handle-free reply may be held to the loop's next receive; nothing runs between handler return and that receive"; netd.md: last frame with the wait + residual (a frame completed silently during that send waits for the next interrupt; level source fires on unmask); scheduling.md residual "A server's turn crosses harts" rewritten (coordinate with K39 via orchestrator); ipd.md probe cost + hand-offs by naming). Not converted (name as follow-up residual): 9P NineServer loop, ipd, sshd, keyd (keyd needs outcome — stays immediate by design).
OPEN: launch-idle-probes re-derivation (user ecalls worst+1 from 3 alone rounds a width, head and main; interrupt/SBI guards at 2x; case comment saying which check catches what) — needs a QUIET SLOT, ask orchestrator first. Hand-off naming per K35 method (.tmp/K35/bin tools).
OPEN: the ipc-client acceptance (see Measurements) — needs a ruling before building the case.

## Measurements (detail .wash/local/K37-report.md)
1000 lend round trips, guest time, icount (ipc config), rv64/rv32: main 1h 2.166/2.194 s, 2h 3.403/3.836 s (+57/+75 %); K37 1h 2.121/2.097, 2h 2.492/2.723 (+17.5/+30 %). Data .tmp/K37/rt/summary.txt. mttcg host time: main 2h +34..45 %, K37 +2..11 % (noise). .tmp/K37/mt/summary.txt. Instrumented K37 at 2 harts: 0 idle wakes, 0 lock halts, 0 shootdowns across the 1000 turns (both widths) => remaining icount gap is QEMU vCPU turn mechanics. So "within 1 % under icount" is not reachable; question for the Architect (unsent): judge (A) kernel events per round trip = 0 (test-only counter feature), (B) absolute guest-time bound per width at 2 harts (deterministic under icount; main is ~37 % over), or (C) mttcg twin. Lean (B) + artefact residual. Main's instrumented counts not taken.

## Gates run (all pass, host, via q run)
- cargo test -p redoubt-sys: pass.
- cargo test -p redoubt-model --release (all but mutations_are_caught): pass; mutations for Turn*,Ipc*,ReceiveKeepsCurrent,ReceiveWithBadgedHandle,R5NoUnmaskOnReceive,ReceiveDropsOpenCalls: all caught. Full mutation sweep NOT run (long; bench model-mutations case).
- cargo test -p redoubt-netd: pass (new test the_last_frame_of_a_drain_goes_on_with_the_wait; silent-during-drain test adjusted to three frames).
- builds: rv64/rv32 release kernel, rv64 --programs, prebuilt (before ipc-client edit): ok. NOT built: checked kernel explicitly (ipc case is checked and passed).
- Boot: set CASES="ipc redoubt-ipc" both widths: PASS.
- NOT run: rt/fake host tests (cargo test -p redoubt-rt -p redoubt-fake-kernel — built only), blkd/verityd tests, client tests, docs, formatting, size/unsafe budget, ipc-outcomes, timeouts, net classes, sched-latency, sched-irq-wake, sched-wake-no-preempt(-harts), launch-idle-probes, idle ceilings, model-mutations, rv32 programs build, whole bench.

## Next steps
1. Send the ipc-client acceptance question (above) to orchestrator; ask for a quiet slot for launch-idle-probes.
2. Run rt/fake/blkd/verityd/client host tests; kernel host-tests; formatting/docs gates.
3. Add a kernel-side boot case for turns (attack-ish: R13 outcomes incl. Dead a1=0 vs a1=1 on endpoint destroyed, continuation without wake) — e.g. extend ipc-outcomes or a new turn-outcomes case with a test program using rd::reply_receive/send_receive.
4. Write docs; fold WIP into logical commits; full gates; report per launch-env.
