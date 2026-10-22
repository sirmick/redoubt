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
