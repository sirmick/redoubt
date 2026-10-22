# K37 report (k37-implementer-2, 2026-10-22, for review)

Design: .wash/local/K37-design.md. Rulings: wave-2026-10-18-rulings.md "K37 design: two combined calls..." and "K37 acceptance: ipc-client's two-hart figure".
Base main b6d28a6b6. Branch wp-K37, clean, no WIP:
- d4b317a3c kernel: a server's turn is one entry, reply_receive and send_receive
  (sys, kernel, model, fake, rd, turn-outcomes case, ipc-outcomes turn rows, sys fuzz arm; ipc.md, abi.md, model.md, SECURITY.md, testbench.md counts; size: kernel 11386, libs/sys 1157, model 11990)
- 37fefb75e rt: a serving loop holds a handle-free reply for its next receive
  (rt ipc/server/typed, rt tests/turns.rs, blkd, verityd, netd, logsrv; serving.md, netd.md; launch-idle-probes ceilings 53/70/31.1 + comment; ipd.md probe cost; testbench.md passage; scheduling.md device-interrupt residual; size: libs/rt 4309, servers/netd 1653)
- 6ab54edb5 kernel, tests: a server's turn hands nothing across harts, counted and timed
  (checked-build hop count in hart.rs from the first delivered message, printed at power-off; ipc-turns program + case; tester op OFF; scheduling.md residual rewritten; ipc.md status; testbench.md counts; size: kernel 11412)

## New tests and why their verdicts are the system's
- turn-outcomes (boot, rv64/rv32, smp 1 and 2, checked, icount): the program is server and judge of the kernel's own results; a1=0 refusals change nothing (badged/dead/bad receive handle, unknown msg id; send_receive badged/dead; caller still waiting; transfer still the sender's); reply delivered + next call in one call; (Timeout, delivered); receive endpoint destroyed while waiting = (Dead, delivered); abandoned call = discarded; send fails by Timeout / endpoint destroyed while waiting = (Nothing, Timeout/Dead), transfer back; taken send continues to its receive; receive endpoint destroyed during the send = (Sent, Dead).
- ipc-turns (boot, rv64/rv32, smp 1 and 2, checked, icount, poweroff): the kernel's own count line "hops since the first message: idle wakes 0, lock halts 0, shootdowns 0" (ruling (A)) and the guest time under the bound (B). Red on main (scratch .tmp/K37 wt-main with the counter patch): smp2 3404362 us rv64 / 3837530 us rv32, idle wakes 1001, lock halts 3998 / 4001, shootdowns 0 (the ruling expected many of each; main has no shootdowns here: the lend's unmap finds the caller on no other hart). Head, 3 runs a width, deterministic: rv64 1h 2121397/2121349/2121349, 2h 2493709 x3; rv32 1h 2097654/2097659/2097656, 2h 2725830/2725669/2725606; 0 hops every run. Bound = worst 1h + 2 x worst excess, rounded up to 0.05 s: rv64 2.121 + 2 x 0.372 = 2.866 -> 2.90 s; rv32 2.098 + 2 x 0.628 = 3.354 -> 3.40 s.
- ipc-outcomes: turn records at MMIO refused with a1=0, device untouched.
- libs/rt/tests/turns.rs (fake kernel): held reply rides the next receive; refused turn still answers; send_then_wait taken then the interrupt; failed send gives its pages back.
- libs/sys fuzz decode: FirstHalf arm.

## Gates (exit codes; host work via q)
- cargo test -p redoubt-rt -p redoubt-sys -p stub -p redoubt-fake-kernel -p redoubt-client -p redoubt-blkd -p redoubt-verity -p redoubt-verityd -p redoubt-netd -p redoubt-ipd -p redoubt-fileserver -p redoubt-piped -p redoubt-consrelay -p redoubt-init -p redoubt-bootfsd -p redoubt-consoled -p redoubt-keyd: 0. libs/sys/fuzz cargo check --bins: 0.
- make prebuilt: 0 (several times; last at 6ab54edb5).
- jobs.mk set, both widths, all rc=0 (before the hop commit, .tmp/K37/sets/summary.txt run 1): redoubt-ipc ipc-outcomes; redoubt-ipc-attack legacy-gone; timeouts receive-bad-record; net-tcp net-attacks; net-pinned netd-restart; sched-latency sched-irq-wake; sched-wake-no-preempt sched-wake-no-preempt-harts; budget-syscall-attack syscall-attack; process-attack ipc-fair-label-sets; blkd-fairness verity-signed; verity-bad-signature verity-rollback; netd-host-tests ipd-host-tests; verity-host-tests blkd-host-tests; net-host-tests rt-host-tests; client-host-tests init-host-tests; elixir-oracles model-host-tests; ipc turn-outcomes.
- After the hop commit (run 2): docs formatting 0; turn-outcomes ipc 0; smp-shootdown sched-lock-contention 0; smp-boot ipc-outcomes 0; ipc-turns PASS x4; size-budget unsafe-budget 0 (after the raises).
- NOT run: whole bench, model-mutations (bench case; the Turn*/Ipc*/receive mutations were checked caught by the predecessor), launch-idle / launch-idle-probes (quiet slot), rv32 programs build outside prebuilt (prebuilt builds both widths' cases).

## Measurements (ipc, ruling's figures)
icount (ipc case config): main 1h 2.166/2.194 s, 2h 3.403/3.836 s; K37 1h 2.121/2.097, 2h 2.492/2.723 (.tmp/K37/rt/summary.txt). mttcg host time, ipc pair, 3 runs a width: main 2h over 1h +34..+54 %, K37 +2..+11 % (.tmp/K37/mt/summary.txt).

## Quiet slot (.tmp/K37/idle, slot.sh; 32 boots one at a time, 28 with no other q job, 4 beside one main rv64 steward-latency-n16 on general cores 0-1; all PASS)
launch-idle-probes per frame (30 frames every window), head / main (scratch, same commit base):
- interrupts: head rv64 19.7, 25.6, 26.0; rv32 25.3, 26.4, 25.5 | main rv64 34.6, 38.9, 38.9; rv32 39.0, 39.6, 39.2
- SBI calls: head 28.8, 34.1, 34.5; 34.2, 34.9, 34.0 | main 45.9, 50.0, 49.8; 50.4, 50.8, 50.4
- user ecalls: head 30.0, 30.1, 30.1; 30.1, 30.1, 30.0 | main 32.0, 32.1, 31.9; 32.0, 32.1, 32.0
Ceilings (one for both widths): user ecalls worst+1 = 31.1 (red on main: 31.9-32.1, the witness); interrupts 2 x 26.4 -> 53, SBI 2 x 34.9 -> 70 (guards; main 34.6-39.6 / 45.9-50.8 pass them). Confirm runs at the committed head: rv64 25.9/34.5/30.1 PASS, rv32 26.2/34.7/30.0 PASS.
Per probe (probe window less the round's launch-idle window, / 30): head interrupts 10.8-12.6, SBI 14.4-16.6, user ecalls 10.8-11.2; main 23.2-24.6, 29.0-30.4, 12.8-13.0. (ipd.md had said 26-30 / 32-36 / 15 from K35's windows; restated from today's.)
Naming (marker kernels, one window a tag and width, K35's names.py extended for the two turns, / 30): idle wakes per probe main 3.2 (netd send 0.9-1.0, netd reply 0.9, card interrupt on an idle hart 0.8-0.9, ipd receive 0.6) -> head about 0 (-0.3, 0.0); lock-wait halts main 8.6-9.6 -> head 4.0-4.1, all after a wake made by ipd's or consrelay's (9P skeleton) two-call reply; none after the card's interrupt in these windows.
K39: launch-idle's figures could move only through re-arms: K39 changes when a slice's end is computed (pause/resume), not which entries a frame makes, so user ecalls (the witness) cannot move; interrupts and SBI calls could move by a re-arm per interrupt return if K39 arms differently, which the 2x guards absorb. Worth one confirm run of both idle cases after the rebase.

## ipc-turns red on main
Measured in a scratch tree at main with the counter, ipc-turns and OFF applied: smp2 rv64 3404362 us, rv32 3837530 us (over 2.90 / 3.40 s); idle wakes 1001, lock halts 3998 (rv64) / 4001 (rv32), shootdowns 0 (as measured: the lend's unmap finds the caller on no other hart). smp1 2165698 / 2193487 us, 0/0/0. Those console logs were not kept (the scratch tree was removed before copying them); the figures were read from them at the time.

## Open
- After K39 publishes: rebase onto main, fold the preemption-points sentence (two-entry turns -> turns are one entry) into K39's rewritten text; rerun docs, ipc-turns, turn-outcomes, launch-idle(-probes) once a width.
- NOT run: whole bench, model-mutations bench case (mutations TurnReplyLost/TurnSendDispositionLies and the IPC/receive ones were checked caught by the predecessor with cargo test).
- Not converted (stated on serving.md and scheduling.md residuals): the 9P skeleton's loop (consrelay, fileservers), ipd, sshd; keyd by design.

## Summaries checked
- README.md, GETTING-STARTED.md, docs/README.md: no IPC call list or turn claim; no change.
- kernel/README.md, model/README.md: one-line crate descriptions; no change.
- docs/kernel/ipc.md, abi.md, model.md, scheduling.md (residual), SECURITY.md (R13 row = section set), servers/serving.md, servers/netd.md, testbench.md counts: updated.
- docs/testbench.md launch-idle-probes passage, docs/servers/ipd.md "What a probe costs" and "Probes scale" residual, scheduling.md device-interrupt residual: updated from the slot.


# Round 1 fold and rebase (2026-10-22)

Head 38134865d on main f977dd514 (train 101: K39, B109, B112, B111). Commits: fb0cd5804 kernel (turns), 63eb8621f rt (held replies, probe ceilings, ipd.md), 38134865d kernel+tests (hop count, ipc-turns).

Folded: rt receive_after one fallback path (simplifier P2); kernel CheckedReply struct (P3); netd/logsrv reply-side fallback not changed (netd's loop already uses rt's; logsrv is raw rd with no rt); ipc.md: a receive handle closed or revoked during the send does not end the turn (red P3); history words out of scheduling.md, ipd.md, testbench.md (editor P2); testbench.md: the power-off hop count line and the tester's OFF (editor P3); serving.md residual wording. Rebase: mutation count 185 (model.md, Mutation::ALL), K39's R12 row kept, testbench.md counts per C14 (191 of 362 under icount, 184 at shift=3,sleep=off), preemption-points sentence folded into K39's rule (a) paragraph, ceilings summed: kernel 11452, model 11997, libs/sys 1157, libs/rt 4305, servers/netd 1653 (each equal to its count at the head).

model-mutations (set CASES=model-mutations, at 84c583767 on train 100's main): 185 PASS, 0 FAIL; slowest R2NoWaitCap 20.8 s (wall 21.4) of the 25 s deadline; TurnReplyLost and TurnSendDispositionLies 0.0 s. Fusion: R2NoWaitCap alone on one core, three runs each: head 15.26/14.89/15.02 s, main 14.15/14.20/14.21 s: +0.8 s (6 %). The case comment's 18.7 s restated (15.0 alone, 20.8 in the case); the 25 s deadline is not widened (a fifth of margin now, not a third: a ruling's to change).

Quiet slot 2 (.tmp/K37/idle2, slot2.sh + slot2b.sh): 16 boots one at a time on the quiet cores, all PASS; general cores carried other jobs (my gate sets, other members' shell tests) during most of them. hprobe rv64 round 1's console log was pruned by the concurrent gate runs, so rv64 took a fourth round (rounds 2-4 used).
Per frame, head: rv64 ints 22.4/25.9/24.9, SBI 31.3/34.5/33.6, ecalls 30.1/30.1/29.9; rv32 20.1/25.7/21.1, 29.4/34.4/30.1, 30.1/30.1/30.0. Main (train 101) one window a width: 34.6/46.1/32.1 rv64, 39.8/51.0/32.1 rv32 (B112's main rounds: 33.9-38.8 / 45.2-50.3 / 31.1-32.1).
Ceilings, most + spread of the wider width, rounded up: interrupts 25.7+5.6 -> 32, SBI 34.4+5.0 -> 40, user ecalls 30.1+0.2 = 30.3. Note: the Architect's ruling for the ecall bound was worst+1 (31.1) when the rounds spread by under 1; B112's main reads as low as 31.1, which worst+1 would pass, so worst+spread (as instructed for all three) is what keeps it red on main. All three are red on main.
Per probe (probe less base / 30): rv64 12.8/16.4/11.2, 11.8/15.6/11.2, 14.6/17.8/11.2; rv32 10.0/13.8/11.2, 15.6/18.8/11.2, 12.0/15.4/11.2 -> ipd.md: 10 to 16 interrupts, 14 to 19 SBI calls, 11 user ecalls.
Not run after the last fold: launch-idle-probes with the new ceilings (needs a prebuild and the quiet set; every measured window is inside them), whole bench.

Gates at 5748208b7 (train 101 rebase, before the slot's text): formatting, unsafe-budget, ipc-turns, turn-outcomes, ipc-outcomes, ipc, rt-host-tests, netd-host-tests, verity-host-tests, blkd-host-tests, sched-latency, sched-wake-no-preempt-harts: all rc=0; docs and size-budget rc=0. docs rc=0 again at the final text.

# Round 2 fold (2026-10-22)

Head 206d3422d on main f977dd514: d46925b68 kernel (turns), 1bb6c6b78 rt, 7e12c59ff kernel+tests (hops, ipc-turns), 206d3422d testbench: launch-idle-probes judges a fixed span of 30 answer frames.
- Red P2: idle judge `frames` key (exact span: first frame's first trap to the first trap of frame N+1; window without it fails; per-frame ceiling needs it), replaces min_frames; case idle step 64 s; frame rule: a new frame needs a user ecall since the last s_external (fixed a double count in a main window, 29.9 -> its true 29 frames). Slot logs over a 29-frame span (.tmp/K37/bin/span.py logic): head ints 20.2-26.1, SBI 29.6-34.8, ecalls 29.9-30.0; main 34.8-40.2, 46.2-51.3, 31.8-32.0. Ceilings 32/40/30.1. Confirm (quiet set, .tmp/K37/confirm): rv64 32 frames, over 30: 20.0/29.4/29.8 PASS; rv32 24.2/33.0/29.8 PASS.
- Red note: model-mutations comment states a fifth of margin. Editor P3s done.
- Gates: docs 0, formatting 0, host-tests 0, launch-idle-probes rv64/rv32 0.
