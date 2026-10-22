# K35: what one keep-alive probe costs an idle 4-hart box (step 1: measured and named)

Base main 4c2c063e8 (B101's one-hart lock release included). Branch wp-K35: no commits yet.

## Method

launch-idle (4 harts, 60 s window, QEMU's `log int`) in the granted quiet slot, one job at a time,
alternating, 2 rounds a width: **base** (scratch worktree `.tmp/K35/wt-base` at 4c2c063e8,
KEEP_ALIVE 15 s, 4 probes a window) and **amp** (`.tmp/K35/wt-amp`, the same with
KEEP_ALIVE = 2 s, 30 probes a window). Per probe = (amp - base) / 26. The card's answer frames
check the probe count: distinct receive interrupts rise by exactly 26.0 on both widths (4.0 to
30.0), so it is one answer frame per probe. Raw data: `.tmp/K35/idle/s1/` (windows.txt, *.int.log,
*.lab labelled events, perprobe.txt, follows.txt). Tools: `.tmp/K35/bin/`. `label.py` names
each trap by the kernel function at its epc (nm). A user ecall is named by the program with an
`ecall` at that epc (objdump), which is unique per program.

Verdicts: base 4/4 PASS (busiest hart 3.0-3.1 rv64, 2.6-2.7 rv32). amp 4/4 FAIL (6.6-7.4/s), as
expected at fifteen times the probe rate. Host cores 0.004 base, 0.006-0.008 amp.

## Per probe, machine-wide (mean of 2 windows a width)

| event (kind @ where) | rv64 | rv32 | what it is |
| --- | --- | --- | --- |
| m_software @ acquire_ticket | 17.3 | 15.6 | a hart halted for the kernel lock, woken by the release |
| SBI send_ipi @ wake_halted | 14.4 | 13.4 | the release waking that halted hart |
| SBI set_timer @ time::rearm | 13.9 | 12.9 | this hart's timer target changed at a kernel exit |
| user ecall, ipd | 8.9 | 9.0 | the stack's own calls (unchanged by any fix) |
| user ecall, netd | 6.0 | 6.0 | rx thread ack/map/send/ack/wait_irq, plus the serving thread's reply |
| SBI send_ipi @ sched::leave (wake_idle) | 3.4 | 3.2 | a wake made work, so an idle hart is told to pick |
| s_timer (idle 2.5 / at a user return 2.7) | 5.2 | 4.2 | timer interrupts |
| m_software @ idle | 1.8 | 2.0 | the idle hart's reschedule interrupt (from wake_idle) |
| TLB shootdown: m_software + SBI shootdown + SBI serve | 1.4 x3 | 1.3 x3 | a page unmapped in a process another hart names |
| s_external (claimed 1.0 / claimed nothing 0.85) | 1.85 | 1.75 | the card's answer, taken on two harts |
| s_software @ idle | 0.4 | 0.9 | |
| **interrupts / SBI calls / user ecalls** | **28.0 / 34.6 / 14.9** | **25.9 / 32.2 / 15.0** | |

B94 read 66 / 84 on the old lock and B101 13 / 19 by a coarser difference. This measurement uses
26 probes a window, not 4, and the frame count checks it.

Every probe's chain touches all four harts: 28 of 29 probe segments in one window had events on
all four, and the median segment was 99 events.

## Named: what each hand-off, re-arm and wake follows (on its own hart, rv64; rv32 alike)

- **Lock hand-offs, 17.3** (each one m_software interrupt plus one send_ipi at the release).
  - 7.6 follow a user ecall from netd (4.2) or ipd (3.4). The chain's own threads run on
    different harts and enter the kernel back to back, and each finds the other there.
  - 2.7 follow an interrupt taken at a user return (timer 1.9, card 0.8).
  - 2.3 follow the hart's own release: it re-enters and finds the lock taken.
  - 1.7 follow another hand-off.
  - 1.4 follow an idle wake. wake_idle's reschedule interrupt is sent holding the lock, so the
    woken hart always halts once.
  - 1.4 follow a shootdown's serve.
- **Re-arms, 13.9.**
  - 5.9 at the exit after a hand-off.
  - 2.5 after an idle timer interrupt (below).
  - 2.0 after an ipd call.
  - 1.2 after an idle wake.
  - 1.0 straight after another re-arm.

  The timer's target moves at almost every exit, because each blocking call with a timeout lowers
  the hint: ipd's transmit at 100 ms, netd's send at 50 ms, ipd's receive at the poll delay.
- **Timer interrupts, 4.2-5.2.** One is the probe's own (ipd's receive times out at the
  keep-alive). The others look like stale timeout hints. `collect_due` recomputes only the
  processes whose cached `earliest_timeout` has come. A wait answered in microseconds (netd's
  50 ms send, ipd's 100 ms transmit) leaves its process's cache and the global hint early, so it
  fires later as an empty interrupt and a re-arm. A cache still stale in another process becomes
  the next hint, which explains the back-to-back runs: 1.3-1.5 extra fires a probe come in runs of
  2-3 on an idle hart, with only a re-arm between. time.rs's "at most one early interrupt" holds
  per process, not per stale wait. The guest's own arithmetic is not the cause: set rounds up,
  now rounds down.
- **Card interrupt on two harts (1.85 per frame).** Every hart's context takes the source
  (boot.md). The second hart finds nothing to claim, but it still takes the lock.
- **Shootdowns, 1.4.** The page netd's rx thread maps for each frame and transfers to ipd is
  unmapped from netd while another hart names netd: netd's serving thread, or a hart halted with
  it.

## Proposal (not built)

Bound to target, per probe on launch-idle at 4 harts, machine-wide: **at most 12 interrupts and
15 SBI calls**, from 26-28 and 32-35. User ecalls stay at 15, since no behaviour changes at the
sockets. Idle ceilings are unchanged. The stated per-probe cost goes on ipd.md beside the
keep-alive rule. Stages:

1. **Exact timeout caches (kernel, time.rs/message.rs).** A wait that ends before its timeout and
   held its process's earliest deadline recomputes that process's cache, or sets it to NEVER when
   its timed list is now empty. If it held the global hint, the hint is recomputed from the
   per-process caches: one pass over live accounts, no list walks. The design's
   "at most one early interrupt" then holds. Expected: about 3 timer interrupts, about 3-4
   re-arms and the hand-offs they cause. A case can verify it from the trace's `expired(stale)`
   records. Tier A, small.
2. **The chain's spread (kernel wake placement, or netd/ipd threading).** Removes about 10 of the
   17 hand-offs. This needs a ruling on the shape:
   - (a) a send or reply that readies a thread whose waker blocks in the same entry runs it on
     that hart, with no idle wake;
   - (b) netd's rx thread forwards its frame as the last act before `wait_irq`, so ipd runs where
     netd blocked;
   - (c) a bounded spin before the lock halt (R78 unchanged in order; risky under icount turn
     order, so not preferred).
3. **Optional:** reuse netd's rx page, a lend kept as ipd keeps its transmit page, so no unmap
   and no shootdown (about 4 events); route a source to one context at a time (about 2 events:
   design change to boot.md, Architect).

Next: on a go, build stage 1 with its test and re-measure in a quiet slot. Stage 2 waits on the
shape ruling.

## Summaries checked

None yet: no code or page changed. docs/servers/ipd.md, docs/kernel/timer.md "Hints",
scheduling.md R78 and boot.md routing are named above for the build.

# K35 stages 1-2: built, measured, named (k35-implementer-2, 2026-10-21)

Branch wp-K35 head 2d199eff5 on main 4c2c063e8, WIP commits (folded before review):
f396f2a23 kernel exact timeout caches; b8312753a netd last frame last; ad59b49a1 bench
launch-idle-probes + ipd test feature; 0b354c867 docs timer; db8ca6f0b size ceilings; 2d199eff5
docs rule (a).

## What was built

- Stage 1 (kernel/src/message.rs, time.rs; libs/ipclist insert_by): each process's timed waits are
  kept in deadline order (the blocking call's own walk); the cache is the first wait's deadline,
  lowered at listing, raised when the first leaves; a wait whose deadline was the hint recomputes
  the hint from the caches (one read per live process). expire_due sets the hint from its walk
  before ending waits. Checked builds assert cache == first deadline at every list audit, and
  hint == earliest cache at each entry's end. Oracle: `stale_waits_in` (require some) became
  `no_stale_waits_in` (require none); sched-timer-flood reads 0 stale-wait interrupts on both
  widths (main 73 rv64, 19-20 rv32).
- Stage 2 (b): servers/netd/src/receiver.rs holds the last drained frame and forwards it once the
  queue is empty and its slots are re-offered, then re-checks (MMIO only) and waits. Host test
  the_last_frame_goes_on_as_the_last_act_before_the_wait.
- Stage 2 (a): no code. sched::leave already counts unrun + beside and wakes only beyond the one
  unit this hart's kmain picks when the runner leaves for kmain; the naming confirms it (below:
  ipd's `call` readies netd's serving thread 1.0 a probe and makes 0 wakes). Rule written on
  scheduling.md "Preemption points".
- Case launch-idle-probes (quiet): launch-idle with ipd's test-only keep-alive-probes (2 s), a new
  case field recipe_features (recipe names none), [idle] judge counting answer frames
  (s_external clusters within 6 traps) and per-frame interrupts / SBI calls / user ecalls,
  min_frames. Host tests: recipe_features_reach_only_the_named_program,
  only_launch_idle_probes_builds_the_fast_keep_alive, answer_frames_are_counted_and_judged_per_frame.
  Plain ipd release byte-identical to main (same path, both widths): rv64
  58fd174182f65e69e5035027ff4e7433e6b6a97034278dc29214664471a5d434, rv32
  c42ffde7c6a4d942d3ad9e2d7556f671d25a571086c0059a4cf0542e302c2813.

Gates (pinned, exit 0): host-tests, ipd-host-tests, netd-host-tests (and the device tests directly),
docs, formatting, size-budget, unsafe-budget; boot cases both widths: sched-timer-flood, timeouts,
budget-deadline, expiry-deadline-then-timeout, worst-walk (expiry walk max 19,052 us rv64, one
run), timer-flood-at-rest; netd-build both widths; kernel checked + rv32 builds. Not run yet: the
sched-*/smp-* load run, beamlet/net classes.

## Quiet slot (.tmp/K35/idle/s2, s2n; one job at a time)

Company: SCHED5 host jobs were running during hprobe rv32 1-2, hbase rv64 2, base rv64 1, mprobe
rv64 1 (starts.txt); hprobe rv32 2 (3 SCHED5 jobs beside, 77 s) is the high outlier below.
Step 1's s1 first job (base rv64 1, beside rt-host-tests): its figures sit inside this slot's
spread, nothing looks off.

Per probe, (probes case - launch-idle) / 26, frames checked 4.0 -> 30.0:

| | interrupts | SBI calls | user ecalls | hand-offs | re-arms | timer ints (user+idle) |
| --- | --- | --- | --- | --- | --- | --- |
| head rv64 (2+2) | 27.0 | 33.1 | 14.9 | 15.9 | 13.6 | 4.0 + 1.1 |
| head rv32 (2+2) | 31.3 | 37.3 | 13.7 | 22.7 | 10.6 | 2.5 + 0.6 |
| main rv64 (1+1) | 26.6 | 33.5 | 15.0 | 17.2 | 12.8 | 2.6 + 2.2 |
| main rv32 (1+1) | 28.8 | 35.3 | 15.0 | 17.0 | 15.0 | 3.7 + 2.6 |
| main s1 rv64/rv32 | 28.0 / 25.9 | 34.6 / 32.2 | 14.9 / 15.0 | 17.3 / 15.6 | 13.9 / 12.9 | |

Per frame in the probes case (window totals, background included): head rv64 39.3 / 38.8
interrupts, 50.2 / 49.6 SBI, 32.4 user; head rv32 33.9 / 45.9 (with company), 45.4 / 55.8, 32.5 /
29.9; main rv64 35.3, 47.0, 32.5; main rv32 40.0, 51.3, 32.1.

Finding: stages 1-2 do not move the per-probe cost: head and main are within window-to-window
spread (main 26.6-28.8 interrupts, head 27.0-31.3). Idle-hart timer interrupts fell (2.2-2.6 ->
0.6-1.1) but timer interrupts from user mode and re-arms did not, and the hand-offs did not. The
case cannot be red on main: main passes any bound head meets.

## Naming (instrumented kernels, .tmp/K35/idle/s2n, per probe, 1 window a tag a width)

Idle wakes (send_ipi in sched::leave) by the entry that made them, main / head:
- netd `reply` (serving thread answers ipd's transmit call): 1.0 / 1.0
- netd `send` (rx thread forwards the frame): 0.8-0.9 / 0.4-0.7
- ipd `receive` taking a queued send (readies netd's sender): 0.5-0.8 / 1.0-1.4
- card interrupt taken on an idle hart: 0.9-1.1 / 0.9-1.1
- ipd `call` (readies netd's serving thread, ipd blocks): 0 / 0 (it readies 1.0; rule (a) already)

Lock hand-off halts (10.2-11.2 a probe here), by the entry that made the last idle wake before
them, main rv64/rv32 -> head rv64/rv32:
- call-shaped: 0 everywhere.
- send/reply/receive-shaped: netd send 2.7/2.6 -> 1.9/1.0; netd reply 1.7/1.4 -> 1.2/1.1; ipd
  receive 1.0/1.6 -> 1.9/2.8; sum 5.4/5.6 -> 5.0/4.9.
- interrupt-shaped (card interrupt on an idle hart and its wake): 4.6/4.8 -> 5.4/5.6.
(b) moved netd's send-shaped hops onto ipd's receive (the frame now waits queued for ipd more
often), net zero. (a) has nothing to remove.

## What remains and the question back

The send/reply-shaped hops (about 5 a probe) are reachable only by an entry that sends and blocks
in one: a reply-and-receive for netd's serving thread, and a send-then-wait_irq for its rx thread
(or a receive that sends first) - the ABI question the ruling reserved, with these counts. The
interrupt-shaped (about 5) follow the card's interrupt taken on two harts and the wake its idle
hart's leave sends; one-context routing (boot.md) and that wake are the remaining levers.
The derived bound therefore is not 12/15: with nothing removed, the derivation lands on the
measured 27-31 interrupts and 33-37 SBI calls a probe. Bounds, ipd.md and the case's ceilings
wait for the ruling.

# K35 landing (k35-implementer-3, 2026-10-21)

## Fold (before rebase), base 4c2c063e8

Old WIP head 2d199eff5. New wp-K35:
- c8d4c858f kernel: a wait answered before its timeout takes its timer hint with it (kernel,
  ipclist insert_by, oracle no_stale_waits_in, sched-timer-flood, timer.md, kernel-attack-gaps,
  scheduling.md Charging; size kernel 10931, ipclist 544)
- 1c97cfe82 docs: a thread readied where its waker blocks is that hart's pick (rule (a) on
  Preemption points; residual "A call within one budget crosses harts" rewritten as "A server's
  turn crosses harts": reply and receive are two entries, ~5 hand-offs a probe, a one-entry turn
  the remedy; device-interrupt residual gains its at-rest cost, routing the remedy)
- 4dad58a33 WIP bench: launch-idle-probes + ipd keep-alive-probes (size ipd 2992); ceilings,
  ipd.md and testbench.md wait for the witness.
(b) dropped: servers/netd identical to main (git diff 4c2c063e8 -- servers/netd empty), netd size
ceiling back to 1590. Docs name no plan nodes (doccheck: no process references), so K37/K38 are
stated as their remedies.

Gates pinned (taskset -c 8-23 nice -n 10), exit 0: jobs.mk set CASES="docs formatting
size-budget unsafe-budget"; set CASES="host-tests ipd-host-tests netd-host-tests".

Question sent to the orchestrator: the case can judge per answer frame only with the window's
background included (32 user ecalls a frame), not the differential 15 a probe; options A/B/C.

## Rebase and witness (2026-10-22)

Rebased on main 328bcecff (trains 96, 97): one conflict, size-budget's kernel ceiling, set to
11090 (main 11048 + 42; the case reads 11090 of 11090). Head after the fold:
607573679 kernel exact timeout caches; 544b13edb docs rule (a) + residuals; 125fcc2e2 bench
launch-idle-probes guard + ipd.md + testbench.md. servers/netd and servers/ipd's default build
unchanged from main (ipd identical 4c2c063e8..328bcecff, so the predecessor's byte-identity holds).

Witness (.tmp/K35/idle/w3, idle3.sh; witness.py from QEMU's int.log): 3 alternating rounds a
width, one job at a time, order head probes, head launch-idle, main probes, main launch-idle;
main = 328bcecff + the bench commit (wt-m), head = 96f75983b (wt-h; kernel identical to the final
head). 27 jobs PASS. Round 1's mprobe/mbase rv64 and hprobe rv32 had B103 company (prebuilt,
host-tests): rerun alone (idle/w3r), substituted; originals in idle/w3/company. Every window 30
frames (base 4).

Per frame, probes case (int / SBI / user):
| | r1 | r2 | r3 |
| --- | --- | --- | --- |
| head rv64 | 37.1 / 48.1 / 32.1 | 40.7 / 51.6 / 32.1 | 38.8 / 49.7 / 32.1 |
| main rv64 | 40.8 / 51.9 / 32.0 | 41.4 / 52.5 / 32.1 | 42.1 / 53.0 / 32.1 |
| head rv32 | 39.8 / 51.2 / 32.1 | 37.7 / 48.9 / 32.0 | 38.6 / 49.9 / 32.0 |
| main rv32 | 41.1 / 52.2 / 32.1 | 41.3 / 52.2 / 32.1 | 40.7 / 52.0 / 32.1 |

Per probe ((probes - launch-idle) / 26 frames), int / SBI / user:
| | r1 | r2 | r3 |
| --- | --- | --- | --- |
| head rv64 | 26.1 / 32.4 / 15.0 | 29.9 / 36.3 / 15.0 | 27.3 / 33.7 / 15.0 |
| main rv64 | 29.2 / 35.8 / 14.9 | 28.8 / 35.5 / 14.6 | 31.0 / 37.3 / 15.0 |
| head rv32 | 26.3 / 33.3 / 15.0 | 26.0 / 33.0 / 15.0 | 27.0 / 34.0 / 14.9 |
| main rv32 | 29.4 / 36.1 / 15.0 | 28.7 / 35.2 / 15.0 | 28.4 / 35.5 / 15.0 |

Item (4): no regression: head's lowest round is below main's highest on both widths. On rv32
head is lower than main in every round (26.0-27.0 against 28.4-29.4 interrupts a probe); on rv64
the rounds overlap (26.1-29.9 against 28.8-31.0). Per frame head reads below main in every round
on both widths (rv64 by 0.1 at the closest). Reported, not claimed as stage 1's gain.

Guard (ruling item 2 + option A + the Architect's condition): worst per frame alone across head
and main: rv64 42.1 int, 53.0 SBI -> 85, 106; rv32 41.3, 52.2 -> 83, 105. The [idle] table has one
ceiling for both widths (no per-width form), so the case carries rv64's, the larger, and its
comment gives rv32's. User ecalls: worst 32.1, spread 0.1 (<= 1) -> 33.1. min_frames 25 kept.

Gates on 125fcc2e2 (pinned 8-19), exit 0: docs (after one fix: a reflow had split an
`R12 (scheduling)` citation, C5), formatting, size-budget, host-tests; earlier on the rebased
tree: prebuilt (371/357, 0 failed), unsafe-budget, ipd-host-tests, netd-host-tests.

Summaries checked: README.md, GETTING-STARTED.md, docs/README.md, docs/plan/*, docs/kernel and
servers overviews: no claim on probe cost, stale hints or launch-idle (grep); no change.
docs/servers/netd.md: no change ((b) dropped). Updated: docs/servers/ipd.md (/net tree "What a
probe costs", status +bench:launch-idle-probes; residual "Probes scale with an account's
sockets"), docs/testbench.md ("The machine at rest" status +4, the case's paragraph),
docs/kernel/scheduling.md (rule (a); residuals "A server's turn crosses harts", "Every hart pays
for each device interrupt"), docs/kernel/timer.md, docs/todo/kernel-attack-gaps.md.
Docs name no plan nodes (doccheck forbids process references): K37/K38 appear as their remedies.

## Load run and final rebase (2026-10-22)

Load run on 125fcc2e2 (orchestrator's go after train 98; .tmp/K35/bin/load.sh, log
.tmp/K35/gates/load.log): `make -k -f scripts/jobs.mk set CASES="<57 cases>"` pinned 8-19, both
widths: every sched-* case, every case named *timer*/*timeout*/*deadline*, worst-walk,
launch-idle, launch-idle-probes, and the smoke set (userland-boot init-boot bench-net-peer
ipc-outcomes sum-clear lend-untouched-page). rc=2: one failure, every other job PASS (128 PASS
lines, smp variants counted). sched-timer-flood both widths PASS (0 finding a wait ended early),
launch-idle and launch-idle-probes both widths PASS.

The failure: deadline-placement rv64 smp=2 (host time, multi-threaded TCG), foreign timer work
in the victim's time 11 of 1000 against <= 10, under load. Log copied to .tmp/K35/fail/. Rerun
alone 3/3 PASS at 0 of 1000 (q showed nothing else running before each). A miss under load only,
not judged a finding; the change touches the timeouts' hint, not budget deadlines' placement.

Rebased onto main 7a15bc867 (train 98), clean: b50bfd392 kernel, a2d9ecccb docs, 58dcb572e bench.
Train 98's scheduling.md changes are the steward-latency rows, apart from this branch's sections.
Pinned gates on 58dcb572e, exit 0: docs, formatting, size-budget.

## Round 1 fold (2026-10-22, k35-implementer-4)

Base wp-B110 69ccdde58 (main 7a15bc867 + C14). Head **0218e48c2**, three commits, reordered so
ipd.md's figures land before the docs commit that cites them (editor P3):
- 9b2a1b376 kernel: a wait answered before its timeout takes its timer hint with it
- 4b5a0b021 bench: launch-idle-probes, a per-frame guard on what a keep-alive probe costs
- 0218e48c2 docs: a thread readied where its waker blocks is that hart's pick

### Red P1: the hint is recomputed once per call/expiry/delivery, not per ended wait
`untime` now only marks the hint stale (`time::timeout_ended(deadline)`, a flag in `Timer`).
`time::settle_timeouts(mm)` recomputes it once (one read per live process): at the end of each
outermost entry part, in `message::audit` (in every build, outside the audit's uncharged time, so
it is billed to the entry's payer), and first wherever the hint is read (`rearm`, `due`,
`expire_due`). `expire_due` still sets the hint from its walk. A fault that kills leaves the
recomputation to the next reader. timer.md (hart timer, R12 for timer work, residual "Expiry walks
the processes", rationale) and the kernel docs say this.

The cost, measured (new case `timeout-hint-exit`, hold-trace, icount shift=3, checked build, one
hart, both widths). A child with 255 threads exits; its 254 waits have deadlines rising by TID, so
each one the exit ends held the hint in turn. It exits once on an empty system, then again with all
508 other PIDs holding parked fillers with no timeout. The oracle's new `EXIT-SECTIONS` holds the
second `process_exit` section, net of audits, to the first's plus one slice (10,000 ticks).

| process_exit section, ticks | empty | every PID in use | difference |
| --- | --- | --- | --- |
| head rv64 | 170,023 | 173,425 | 3,402 (0.34 slice) |
| head rv32 | 183,404 | 188,057 | 4,653 (0.47 slice) |
| head before this fold, rv64 (recompute per wait) | 176,851 | 386,825 | 209,974: FAIL |
| head before this fold, rv32 | 193,683 | 518,465 | 324,782: FAIL |

The margin is twice the worst difference (9,306), rounded up to a slice. The exit itself takes 17 to
19 ms either way. That is its own 254 thread ends, with the same scale on an empty system. With
untimed waits it reads 137k/149k ticks, so a timed wait's end costs about 130 ticks more, a
per-wait constant. I did not investigate further. On main (7a15bc867 + C14) the case reaches no
verdict: the second child's exit notice did not come within 600 s of virtual time, on both widths.
The first (empty) exit passed. I did not investigate this either; the logs are in
.tmp/K35/r1/main/. The case is a guard on what this branch's exactness costs, not a fix case
against main.

### Red P3
- timer.md and testbench.md now say that `no_stale_waits_in` catches a cache left early. A hint
  left early while every cache is exact is caught only by the checked build's hint==earliest
  audit.
- deadline-placement rv64 smp=2, 3 rounds alternating head and main. Each round ran under the same
  load (rv64+rv32 sched-timer-flood and rv64 timeouts repeating from the head tree, 4 jobs at
  most). Head: PASS 3/3, foreign timer work 0, 0, 0 of 1000 (0, 0, 11,210 ticks). Main: PASS 3/3,
  0, 0, 0 of 1000 (0, 10,431, 5,473 ticks). The 11/1000 of the 57-case load run is not reproduced
  at this load, and head is not worse. Logs are in .tmp/K35/dp/.

### Editor P2
- History wording removed: ipd.md ("no longer leaves ... before that read" is now a present
  statement; main's figures moved to the bench commit message), scheduling.md (sched-timer-flood
  paragraph and timer-tail-billed), timer.md (both). Before-figures are in the commit messages.
- C14: rebased onto wp-B110. Counts: kernel commit boot 356, icount 188 ("181 so"); bench commit
  boot 357, host clock 169 (twice), sockets 73, and also disk 142 (C14 found launch-idle-probes
  has a disk; the orchestrator's list omitted it).

### Simplifier P3s
- `KEEP_ALIVE_US` is now one `cfg!`, still at the file's end. Plain ipd is byte-identical to main
  on both widths (sha256 e805fecc… rv64, 90eb3c97… rv32).
- `Due.stale`'s doc now says it is the witness `no_stale_waits_in` reads.
- Declined: the collect_due repair as an assert. Its `stale` is the witness a release-built case's
  trace carries. The checked build already asserts exact caches at every audit (check_lists). A
  release build that ever found a cache left early would otherwise keep a hint in the past and
  re-interrupt.

### Other changes
R12's status, its test list and SECURITY.md's R12 row name `bench:timeout-hint-exit`.
Size budget: kernel 11095 (was 11090; the stale flag and settle points), with its line in the
commit.

### Quiet slot
None run. The change cannot move launch-idle-probes' figures: at every point the hint is read
(rearm, due, expire_due), its value is the one a per-wait recompute gave. Only where in the
call the recompute's reads run changes, and at rest a call or expiry ends at most one
hint-holding wait.

### Gates (pinned taskset -c 8-19 nice -n 10; at most 4 boot jobs at once)
On 9b2a1b376 + C2/C3 as replayed (code identical to the final head; the last amend touched only
comments, same line count):
- Host gates, exit 0: unsafe-budget, size-budget, formatting, host-tests, ipd-host-tests,
  netd-host-tests.
- docs failed once on C14 disk 141→142 (fixed in the bench commit), then passed.
- Exit 0, both widths: sched-timer-flood, timeouts, budget-deadline,
  expiry-deadline-then-timeout, timeout-hint-exit.
- On the final head 0218e48c2, exit 0: docs (C14), formatting, size-budget.
- `cargo test -p testbench exit`: 3 passed (the new an_exit_is_held_to_its_empty_system_section).

### Summaries checked
- No change needed (grep for hint, stale and early interrupt): README.md, GETTING-STARTED.md,
  docs/README.md, docs/kernel/README.md (timer row unchanged and true), docs/plan/*.
- Updated: docs/kernel/timer.md, docs/kernel/scheduling.md (R12 status and list, timer-flood
  paragraph), docs/SECURITY.md (R12 row), docs/testbench.md, docs/servers/ipd.md.

### Notes
- I read every committed file's diff in full, plus the surrounding regions. I did not read all of
  message.rs or sched_oracle.rs.
- The scratch worktrees .tmp/K35/wt-* (13 of them) are the predecessors' measurements, left
  in place.

## Round 2 fold (2026-10-22, k35-implementer-4)

Head **719710c4e** on wp-B110 69ccdde58: e03ac9260 kernel, 473f82aa4 bench, 719710c4e docs. The
kernel code is unchanged since round 1. The case, timer.md, testbench.md and R12's status changed,
all in the kernel commit.

### Red P2: main's stall is the case's own, not a lost wake
I traced main (wp-B110) with block-trace and hold-trace, rv64, one hart, and the report wait cut
to 30 s with a power-off so the trace dumps. Logs are .tmp/K35/r1/diag1-console.log and
diag4-console.log; head's is hdiag1-console.log.

- **No wake is lost.** After the fill, the exiting child's budget blocked 6,347 times and was
  woken by its timeout 6,094 times (EXPIRED what=1, never 2). 253 waiters blocked. One waiter
  never ran its first `fetch_add`, so `WAITING` stuck at 253 and the child's 1 ms poll looped
  until the parent's wait ran out.
- **Each turn, the stuck waiter ran 19 µs** (median, between its return and the timer entry).
  - The poll's own `receive` section is 1.4 ms, of which the marks audit is 0.87 ms. So the
    poll's 1 ms timeout has passed before the call returns.
  - The waiter, picked next by round-robin, returns with the timer already due.
  - The timer interrupt's expiry takes 1,327 µs (median) before the slice decision. Main's
    `collect_due` reads all 253 of the child's timed waits, kept in arrival order, whenever its
    cache has come.
  - `sched::slice_over` compares now, after that expiry, with the slice's end, so the waiter's
    slice is over (6,128 of 6,128 decisions) and the poll is picked again.
- **Head escapes.** Its expiry reads only the waits due and the next one: entry to decision is
  5 µs median, at most 537 µs. The waiter keeps its slice, and the loop ran once.
- **Result.** With the poll at 10 ms, main passes the whole case on both widths:
  - rv64: 161,768 against 159,202 (+2,566).
  - rv32: 175,097 against 171,672 (+3,425).

  Main has no recomputation, so the guard passes on main, and no page claims it does.
- **Not a model item.** No wake is lost, and the model has no notion of how long an expiry takes,
  so no mutation or contract is added.
- **Open, for the orchestrator and Architect, not changed here.** A timer interrupt's own expiry
  time counts against the interrupted thread's slice. scheduling.md says a slice is the picked
  thread's user time and that kernel work between the pick and the return never uses it. It is
  silent on kernel work in an interrupt inside the slice. On head that work is bounded by the
  expiry (≤ 537 µs measured here). A sibling can still be starved if the expiry ever outgrows a
  slice.

### Red P3: fillers hold later timeouts
Each filler now waits with a 2 h timeout (later than every waiter's 1 h deadline), so the
recomputation lowers the hint for every PID (the costly branch). Head:
- rv64: 175,199 against 170,023 (+5,176, 0.52 of a slice).
- rv32: 190,183 against 183,375 (+6,808, 0.68 of a slice).

The margin follows the bench's rule: twice the worst, 13,616, rounded up to whole slices, so two
slices (20,000 ticks). The pre-fold kernel read +210k and +325k with untimed fillers. timer.md,
R12's status and the commit message state the new figures.

### Editor
- P2: testbench.md's "Checked builds" names `timeout-hint-exit` and its `EXIT-SECTIONS` check
  beside `no_stale_waits_in` (status 21, case listed).
- P3: no page claims the case passes on main.

### Gates (pinned 8-19, at most 4 boot jobs at once)
- `timeout-hint-exit` both widths, head and main together, exit 0. Head is quoted above. Main:
  - rv64: 161,768 / 159,202.
  - rv32: 175,097 / 171,672.
- Exit 0, both widths: sched-timer-flood, timeouts, budget-deadline,
  expiry-deadline-then-timeout.
- formatting failed once on the program's header (rustfmt); fixed in the kernel commit.
- On the final head 719710c4e, exit 0: timeout-hint-exit (both widths), docs (C14), formatting,
  size-budget.

## Round 2 ruling folded (2026-10-22)

Head **506617ef9**: b00bf54c6 kernel, fce4807fe bench, 506617ef9 docs. Per "K35 round 2: the
expiry's time and the interrupted slice":
- timer.md, residual "Expiry walks the processes" and R12 for timer work: an expiry reads the due
  waits plus the next. Figures: head 5 µs median and 537 µs max at 253 pending waits with every
  PID in use, against 1,327 µs median when reading every timed wait. The R12 paragraph says that
  time falls inside the interrupted thread's slice.
- scheduling.md, R12's slice paragraph, a residual sentence: a timer interrupt's expiry inside a
  slice counts against it, though billed to the payers. It is bounded by the waits it ends and the
  next of each process. The remedy is a slice that is the thread's own time. The node is not named,
  because docs carry no plan ids.
- The 10 ms poll's comment states the stall's cause with the numbers, and that at 10 ms the guard
  reaches its verdict on main and passes.
- Gates on 506617ef9, exit 0: timeout-hint-exit both widths (rv64 175,201/170,023; rv32
  190,176/183,409), docs, formatting, size-budget. Kernel code unchanged.
- K35's title and acceptance as restated in "K35 after the measurement" are the plan node's and
  the merge's. No branch file carries them; left to the orchestrator.
