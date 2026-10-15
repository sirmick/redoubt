# IDLE2: the kernel at rest

Worktree /home/mcloonan/redoubt/.worktrees/IDLE2, wp-IDLE2 from main 9d680b781 (train 24).
Implementer sched4-implementer (idle2). Reviewer kernel-red-2, QA thread IDLE2-design.

## Design

Cause, from the code at today's main (to be confirmed by the counters below). `time::rearm` arms
every hart's timer for the earliest timeout and the earliest deadline (`t.threads.min(t.budgets)
.min(slice[h])`); SCHED4 (6b3ae7d3c, train 25) places an item on its payer's hart, but a payer
that runs nowhere, every blocked one-thread budget, stays every hart's. At rest nothing runs: every
timeout's payer runs nowhere, so each timeout (the system's periodic sleeps: ~0.9 timer
interrupts/s on each of the four harts, equal across harts) fires on all four harts at the same
instant. All four leave `arch::idle`'s halt and take the kernel lock in turn: the first expires
the item, the other three find nothing due and nothing runnable, and each of the three waits
halted and is woken by the releaser's IPI (`TicketLock::release` -> `wake_halted`): IDLE1's 559
m_software in `acquire_ticket` against 602 `release`s. The woken thread's own calls, on one hart,
then meet the stragglers' idle re-entries on the others: more hand-offs. Each hart that re-enters
re-arms once because the expiry moved the hint (`set_timer` in `rearm`): 501 against ~200 timer
interrupts, 2.5x. So the hand-offs and the re-arms are one defect: an item whose payer runs
nowhere is armed on every hart, and at rest that is every item.

Rule. An item whose payer runs nowhere, or whose hint names no payer, is armed on one hart: the
first hart to re-arm after it was noted or the hint moved; a hart that re-arms and sees another
hart armed at or before the item (`armed[g] <= at`, 0 = fired and re-arming included) does not
arm it. In `Hint::due_on` the payer-nowhere and `None` arms become "at, unless another hart
covers it", the covered test SCHED4 already uses for a payer's hart. Liveness is SCHED4's
argument unchanged: at every return to user mode and before every idle at least one hart is
armed at or before each kind's earliest item (a hart that relied on g is covered because g
re-arms at its own return with the state as it is then: finding nobody armed, it arms). The
noting hart re-arms first (its return or idle after the call blocked), so a timeout lands on the
hart its thread last ran on, in its own time, usually: the choice is path-dependent (whichever
hart re-armed first since the hint moved owns the item, and a hart running another budget may be
it; the interrupt is then billed to the item's payer as today), and the page says so: what the
rule bounds is the count of harts an item wakes, one, not which hart. Nothing changes at one
hart (no other hart to cover). Decided only in `rearm`, at its sites (the return to user mode, kmain's idle, an interrupt
in the idle); `note_*` record only; `due()` unchanged. The `timer-any-hart` feature stays the
negative. SCHED4's bound (10 per mille on the host-clock twins) is re-run: payer-nowhere timeouts
now land on the poller's own hart, so the counts can only fall.

Floor at rest, per timeout: one timer interrupt on one hart; one lock take (its expiry); the
woken thread runs there (no IPI: the hart is idle); two `set_timer` (the slice end at its return,
the next item at its idle); no hand-off while nothing is runnable and no item is due. Idle harts
halted in `idle()` are not in the lock's `halted` set, so a release never wakes them; `wake_idle`'s
IPI (~1/s, `sched::leave`) and device interrupts stay.

Measurement (release counters in a scratch worktree, not for commit, B57's method; after train
25's bench, `q run --cores 8`, launch-idle once a width, before = the base, after = the fix). Per
hart, printed every 30 s from the idle loop: timer interrupts in the idle that expired something /
nothing; `set_timer` by site (interrupt, idle, return); lock takes from `idle()` whose pick was
None with nothing expired (needless); IPIs sent by `release`, `wake_idle`, reclaim; ticket waits by
entry (idle re-entry, trap from user, timer in idle). Expected after: needless takes and release
IPIs ~0 at rest; timer interrupts total ~1/s on one hart, not four. The same scratch run carries
B57's drain counters (.tmp/B57/measure-drain-b57.patch) for one rv32 post-login drain: idle with
frames pending and no drainer above a few ms reopens R81 (the plan's second item).

Ceilings (launch-idle, after the measurement, both widths): `interrupts_per_hart` from 9.0 to
about twice the busiest hart measured after (expected ~1.5-2/s -> 4.0; today's 3.9 then fails
it: the natural negative); `user_ecalls` from 18 to about twice the measured 4-7/s (the
loose-by-2x note goes); `host_cores` 0.01 stays (its resolution is 1/6000 core over 60 s).

Attack angle. Today a one-thread budget that sleeps in short timeouts wakes every idle hart per
timeout and costs each a kernel section nobody pays for: a hostile budget keeps other harts'
sections busy at rest for the price of its own hart. Case `timer-flood-at-rest` (checked,
hold-trace, smp 2): A sleeps 1 ms at a time for 1 s while the other hart idles; the oracle bounds
the kernel sections on the hart that never ran A inside the window (new arg
`idle_hart_sections_max=N`, hold-trace sections by hart, from the records' hart field) at a few
(the wake_idle ones); `timer-any-hart` fails it (~1,000, one per timeout), the recorded negative,
both widths. The attacker's work is billed to it (R12 for timer work), unchanged.

Pages: timer.md "The hart timer" (the rule's payer-nowhere clause, the figure, status + the
case), "R12 for timer work" (residuals: payer nowhere is one hart's, no longer every hart's; the
twins' counts restated); scheduling.md R78 (the release's wake is unchanged; a sentence that an
idle hart never waits for the lock at rest) and the residual bullet; testbench.md "The machine at
rest" (the ceilings' basis, the loose-by-2x sentence gone) and the oracle arg; launch-idle.toml;
stride tests (placement: payer nowhere -> exactly one hart after the harts re-arm in turn).
Tier A, kernel-red-2. Commits: (1) kernel + stride + timer.md; (2) the case, the oracle arg,
testbench.md; (3) measurements, ceilings, pages. Base: SCHED4's head (6b3ae7d3c), since the rule
lives in `Hint::due_on`; wp-IDLE2 to be rebased onto it, or onto train 25's main when published.

Ruling (kernel-red-2, IDLE2-design, 2026-10-14): build as designed; bindings: (1) invariant for
the new fallbacks in timer.md + stride tests (fixpoint: payer-nowhere/None on at least one hart,
exactly one when no slice end covers; a transient test; no change at one hart shown); (2) the
wake-latency residual named; re-judge at 2 harts both widths: sched-latency wake targets,
kernel-containment, sched-timer-entry (delta), wake-no-preempt-harts + twin, SCHED4's three
host-clock twins; (3) ceilings from five windows a width after: interrupts_per_hart = 2x busiest,
user_ecalls = 2x most, host_cores unchanged; the before is a negative only if recorded failing;
drain counters never in IDLE2's commits; (4) timer-flood-at-rest's N from measurement with a
margin, timer-any-hart's failure recorded both widths, the case says which hart takes A's
timeouts and why the idle hart's sections are the measure; (5) due(), note_*, rearm's sites, the
model unchanged, confirmed. Three commits on SCHED4's head.
Base: wp-IDLE2 reset to 6b3ae7d3c (orchestrator); a replay of SCHED4 onto train 24's main
conflicts only in tests/size-budget.toml (kernel ceiling), aborted; IDLE2 moves to train 25's main
when published.

## Commit 1: 822d05a27 on 6b3ae7d3c (2026-10-14)

libs/stride/src/lib.rs `Hint::due_on`: `runs = payer.filter(|p| runners.contains(Some(p)))`;
covered = another hart armed <= at (0 included) that runs the payer when `runs` is Some, any
other hart otherwise; the payer's own hart still arms. Doc rewritten. kernel/src/time.rs: rearm's
doc (code unchanged: due(), note_*, rearm's three sites, the model untouched: binding 5).
libs/stride/src/tests.rs: `armed_at` re-arms harts in turn (the simultaneous iteration oscillates
under an exclusive rule: both arm, both drop); fixpoint test renamed
`each_item_is_armed_on_a_hart_that_runs_its_payer_or_on_one_hart` (payer-nowhere/None: covered;
`on.len() <= 1`; == 1 when no slice end and no other item is at or before it; at one hart `on ==
[0]`); new `a_payer_nowhere_item_stays_covered_across_the_covering_harts_slice_end`; the tie line
in `a_hart_covers_...` and the switch-away test's "every hart arms" step updated (covered by a
re-arming hart -> not armed; nobody covering -> armed). 30 pass (.tmp/IDLE2/c1-stride.log).
docs/kernel/timer.md: "Whose hart comes by an item" (the one-hart fallback, path dependence,
count-not-which, what it was before, the liveness argument with the slice-end case, one hart
unchanged); "What still lands elsewhere" (payer nowhere -> one hart's; the before numbers kept as
before); residual bullets ("Timer work can still land...": one hart, whichever; "Kernel work is
not interrupted": the wake-latency residual, re-judged in sched-latency and kernel-containment);
hart-timer status 10 -> 11 (renamed + new test). tests/size-budget.toml stride 953 -> 956 (Size
budget line). Checks: kernel rv64+rv32 release, checked+sched-trace, +hold-trace, +timer-any-hart
rc 0 (.tmp/IDLE2/c1-build.log; 66 pre-existing dead-code warnings as before); fmt clean;
size-budget, docs, formatting PASS (.tmp/IDLE2/c1-gate-*.log). No case run.
Review (kernel-red-2): OK to build commit 2 on, no findings.

## Machine step (commit 3): base 146db8897 (train 25), wp-IDLE2 = b167ef0d1, 044c9a955

Rebased 2026-10-14 (clean). Scratch worktrees: .tmp/IDLE2/before (146db8897 = the before: SCHED4's
kernel, every hart armed for a payer-nowhere item), .tmp/IDLE2/drain (146db8897 + B57's
measure-drain-b57.patch, applied cleanly, for the rv32 post-login drain counters). Tool
.tmp/IDLE2/tools/traps.py INT_LOG KERNEL NM: QEMU's `-d int` traps by hart and cause, and the
firmware calls / software interrupts by the kernel symbol at the epc (IDLE1's method). launch-idle
runs keep each run's int log, case log and release kernel under .tmp/IDLE2/idle/<tag>/.

BEFORE rv64 (before-rv64-1, PASS, 60.0 s window, beside B51's cases and IDLE2's prebuilt): [idle]
QEMU 0.002 host cores; interrupts/s hart 0 3.5 (m_software 2.3, s_software 0.2, s_timer 1.0),
hart 1 2.8, hart 2 2.8, hart 3 1.7; supervisor_ecall 13.5/s, user_ecall 6.1/s. By symbol over
the window: m_software in TicketLock::acquire_ticket 371 (6.2/s: the lock's hand-off wakes),
in arch::riscv::idle 54 (wake_idle), shootdown 4; s_software in idle 22; supervisor_ecall in
time::rearm 345 (5.75/s: set_timer, 1.75x the 197 s_timer interrupts), in hart::wake_halted 412
(the release's IPIs: 6.9/s), in sched::leave 44, serve/shootdown 8. As IDLE1 read it.

BEFORE rv32 (before-rv32-1, PASS): hart 0 3.4/s (m_software 2.3, s_timer 0.9), hart 1 1.8, hart 2
3.1, hart 3 1.1; acquire_ticket wakes 313 (5.2/s), rearm 319 vs 173 s_timer (1.8x), wake_halted 353.

AFTER (IDLE2's kernel, the first series; the bench's own guard failed 9 of 10 runs with "hart N
took no timer interrupt": an idle hart now takes none in a whole window, which is the rule
working; the guard was per hart because every window measured so far had every hart taking
timer interrupts, and is now "no hart took one"; the int logs of 7 of the 10 runs survived the
pruning, .tmp/IDLE2/idle/after-*; beside B51's cases): per window of 60 s, rv64 x3: acquire_ticket
wakes 92 / 103 / 95 (1.5-1.7/s, from 371), wake_halted ecalls 95 / 105 / 98 (from 412), rearm's
set_timer 194 / 197 / 194 (from 345; ~3 per timer interrupt: the noting hart's arm, the slice
end's at the thread's return, the next item's at its idle), s_timer per window 63 / 67 / 63 (from
197: one hart per timeout), wake_idle IPIs ~30 (unchanged); busiest hart 92-114 interrupts =
1.5-1.9/s (from 3.5), two harts under 20 (0.1-0.3/s); user_ecall 368 = 6.1/s. rv32 x4:
acquire_ticket 109 / 89 / 114 / 101, rearm 198 / 199 / 198 / 196, s_timer 66 / 64 / 68 / 65,
busiest hart 106 / 101 / 106 / 97 = 1.6-1.8/s (from 3.4), user_ecall 365-368 = 6.1/s.

RE-JUDGING (binding 2; .tmp/IDLE2/rejudge/; from the head's prebuilt, beside B51 and the
launch-idle series; all PASS): sched-latency 2 harts net p99 N=1/4/16, after vs main (SCHED4's
after runs): deadline_notice rv64 3,763 / 3,874 / 4,076 (3,535 / 3,428 / 3,758: +228 / +446 /
+318), rv32 4,302 / 4,390 / 5,407 (3,933 / 4,104 / 5,133: +369 / +286 / +274): the wake-latency
residual, within the program's p99 kernel section (10,167 / 11,053 ticks = 1.0-1.1 ms, SCHED4's
hold-trace run); timer_wake rv64 2,159 / 4,673 / 10,702 (2,556 / 3,456 / 9,132), rv32 5,012 /
7,268 / 21,463 (5,440 / 6,896 / 17,714); driver_wake rv64 1,895 / 2,531 / 6,133 (1,907 / 2,734 /
8,426), rv32 3,452 / 4,462 / 13,222 (3,716 / 4,446 / 15,428); decision_wake rv64 2,781 / 2,174 /
7,977 (2,633 / 1,879 / 9,008), rv32 2,645 / 3,146 / 11,606 (2,888 / 2,845 / 11,209): mixed,
N=16 timer_wake worse by 1.6 / 3.7 ms, N=1 and the driver better; every target met. 1 hart
unchanged (deadline_notice 2,248 / 2,255 / 2,315 and 2,344 / 2,423 / 2,394). kernel-containment
smp 2: deadline_notice p99 24,529 (24,001) / 27,882 (27,120); timer_wake 1,620 (2,142) / 3,869
(3,865); driver 2,958 (2,846) / 4,092 (3,529). sched-timer-entry 1-hart p99 951 / 1,450 (945 /
1,441: +6 / +9), 2-hart recorded 5,447 / 7,378 (5,445 / 7,375). sched-wake-no-preempt-harts rv64
61 mid-slice + 57 = 118 witnesses (102), rv32 0 + 74 (74); the twin rv64 54 mid-slice (69), rv32
5 mid-slice + 102 (75): AT THE FLOOR of 5, run beside B51's cases and the launch-idle series:
to be re-run alone (condition 5) before anything is concluded. SCHED4's twins: deadline-placement
0 / 0 per mille (1,270 / 9,958 ticks), flood 3, 1 / 4, 1, churn 0 / 3 (threads-exit), 0
elsewhere; timer-flood-at-rest 3 / 3.

AFTER, five windows a width with the guard fixed (.tmp/IDLE2/idle2/, all PASS, beside B51):
rv64 busiest hart 1.6 / 1.7 / 1.8 / 1.7 / 1.8 interrupts/s (harts 2-3 at 0.1-0.3), rv32 1.9 /
1.6 / 1.7 / 1.8 / 1.6; user_ecall 6.0-6.1/s on both; QEMU 0.001-0.002 host cores. Per window:
acquire_ticket wakes 106 / 112 / 105 / 106 / 109 (rv64) and 115 / 107 / 105 / 100 / 102 (rv32):
1.7-1.9/s, from 6.2 / 5.2 (3.6x and 3.0x fewer); rearm's set_timer 194-199 from 345 / 319
(3.3/s, from 5.75 / 5.3); wake_halted IPIs 104-117 from 412 / 353; wake_idle 29-34 (as before).
The hand-offs left are not at rest proper: each is a woken thread's own calls meeting another
hart's entry (its idle re-entry on a wake_idle, a device's), 1.7-1.9/s in all. Ceilings (binding
3): interrupts_per_hart 2 x 1.9 = 3.8 (from 9.0); user_ecalls 2 x 6.1 = 12.2 (from 18.0);
host_cores 0.01 unchanged. The before's busiest hart, 3.5 / 3.4, is under 3.8: the before is NOT a
recorded negative at the new ceiling and is not claimed as one (the twin-run cases are the
negative's: timer-flood-at-rest's any-hart 1,999).

Twin re-run alone on the quiet set (.tmp/IDLE2/twin/, fresh prebuilt, three runs a width, all
PASS): rv64 74 / 47 / 44 mid-slice, rv32 61 / 45 / 41; the icount case once more: rv64 1 mid-slice
+ 101 at a call (earlier runs 35 + 67 and 61 + 57: the one armed hart moves where a wake is
answered run to run), rv32 0 + 74. The 5 read beside B51's cases and the launch-idle series was
load; floors unmoved; scheduling.md's ranges restated (rv64 icount mid-slice 1-61, at a call
57-101; twin 41-74 alone with the placement, "a read at the floor is re-run alone").

sched-timer-entry five runs a width alone at the head (.tmp/IDLE2/te5/, cargo testbench, all
PASS): 1 hart p50/p99/max rv64 948-949 / 950-951 / 3,549-3,552, rv32 1,423-1,425 / 1,447-1,452 /
4,432-4,452 (from 943 / 945 / 3,540-3,548 and 1,416 / 1,441 / 4,421-4,446 at SCHED4's head: +6
ticks, the one-hart rule's `runs` filter in due_on); 2 harts recorded p99 5,447 / 7,376-7,378
(5,445 / 7,375). Bounds 2,000 / 5,000 unmoved (1.38x / 1.12x). Restated in
tests/sched-timer-entry.toml and scheduling.md 151-156.

## Commit 3: 539344966 on 044c9a955 (2026-10-14)

tests/launch-idle.toml: interrupts_per_hart 9.0 -> 3.8, user_ecalls 18.0 -> 12.2, host_cores
0.01; the comment: the ten windows' numbers, the before's, why the before is not the negative,
the host-cores resolution. tools/testbench/src/idle.rs: the measured-nothing floor is "no hart
took a timer interrupt" (was per hart; an idle hart now takes none in a window); the doc comment
and the host test (a log with no s_timer fails, a hart with none beside one with some is a
window measured). tools/testbench/src/sched_oracle.rs: commit 2's P3 (the first section with no
prior `j` reads as idle; harmless, said). docs/testbench.md: the floor's sentence; the ceilings'
basis paragraph (ten windows, the numbers, the before). docs/kernel/timer.md: the hart-timer
paragraph's at-rest numbers (before / with the rule: timer interrupts, hand-offs, re-arms, busiest
hart); the "Kernel work is not interrupted" bullet's re-judged latencies. docs/kernel/scheduling.md:
R78 (an idle hart is not in the lock's set; hand-offs at rest 5-6/s -> 1.7-1.9/s); Preemption
points' witness ranges (icount rv64 mid-slice 1-61, at a call 57-101; twin 41-74 alone with the
placement, 5 beside a bench's load, "re-run alone before believed"); the residual bullet's twin
range 41-78 / 41-75; sched-timer-entry's numbers (951 / 1,452, +6 for the rule's filter, 2-hart
5,447 / 7,378); tests/sched-timer-entry.toml likewise. Message: the at-rest numbers, the
ceilings, the floor change, the re-judged latencies, the drain count.
Gates on 539344966: fmt clean; size-budget (kernel 10808, stride 956 unchanged), docs, formatting
PASS (.tmp/IDLE2/c3-gate-*.log); `cargo test -p testbench` 197 pass (c3-testbench-tests.log).
Not in the commits: the drain counters (B57's patch in .tmp/IDLE2/drain), traps.py, the scratch
trees .tmp/IDLE2/before and /drain (remove at the merge). The whole bench: the train's.
Summaries checked: README.md, GETTING-STARTED.md ("Try the system": no rate claims), docs/README.md,
docs/kernel/README.md (timer row a pointer), SECURITY.md (R12 row: the twins and deadline-placement
listed by SCHED4; timer-flood-at-rest is the hart timer's, status in timer.md; R78 row unchanged
in substance), docs/plan/m2 (not touched: no at-rest claim).

B57's drain counters, one rv32 post-login drain (.tmp/IDLE2/drain = 146db8897 + the patch, not
for commit; .tmp/IDLE2/drainrun/launch-idle-rv32-smp4.log; launch-idle PASS): the login's drain,
36,181 frames at most, back to 0 0.37 s after the step began (QEMU 0.841 cores meanwhile):
rise->first pass 151 ms, first->last pass 3.81 s, last->zero 56 ms; 36,209 passes, zeroing
35.7 ms; stops: blocked not-all-idle 3,886, other drains 6, claims dropped 25, wakes sent 3; halts
with own frames pending 3,892, ended by timer 45 / IPI 3,739 / external 108; STRANDED (every
hart idle, frames pending, no drainer) 3 times, 193 µs in all. A second, smaller drain in the
window (4,801 frames): 4,806 passes, stranded 3 times, 104 µs. No R81 liveness defect: the idle
with pending and no drainer is a fraction of a millisecond, as B57's boot drains; the 5.8-6.2 s
the plan cites was the pre-B57 bench's host-clock wait, not a stall.

## Commit 2: timer-flood-at-rest (construction history, then the commit)

First construction (A alone sleeping 1 ms x 1000, the other hart idle; oracle arg
`rest_sections_max=<mark>:<N>` = sections on the harts that never ran A): both kernels equal
(.tmp/IDLE2/meas/last/): with the fix hart 1 (A's) 1,003 idle expiries + 1,008 call sections,
hart 0 ~120 (the launcher's); with timer-any-hart the same shape on the other hart (A ran on
hart 0 there). The idle hart never woke in either: it halted armed for what was earliest when it
last exited (nothing) and nothing re-arms a halted hart; `note_*` only record. At rest (IDLE1)
every hart does wake because each has periodic timeouts of its own, and at each of its exits it
re-arms for the global earliest, so all harts converge on the same items. Also A migrated once
(start), so "harts that never ran A" is fragile. Second construction: a bystander B sleeping
10 ms x 100 on the other hart, so that hart re-arms at each of B's returns and, armed for every
item, falls in with A's flood; measure the sections per hart on both kernels (meas2/) and choose
the measure (total sections from A's first run, or sections entered from the idle that found
nothing) with the data.
Second construction (A 1 ms x 1000, B 10 ms x 100; meas2/): both kernels alike again (rv64 total
sections 2,364 vs 2,365; idle wakes finding nothing 0 vs 5): B's hart re-arms only at B's ~200
events, and A's item is usually served before its wake lands. Third construction: two sleepers
each 1 ms x 1000, so each hart re-arms a thousand times a second, as the harts of a machine at
rest do for their own periodic sleeps (meas3/): the measure is the kernel sections a hart
entered from the idle (runner None after its `H 0`) and left with no record but a checked build's
audits (U/V): a lock take that found nothing due and nothing runnable (`idle_sections_max=
<mark>:<N>`, from the marked budget's first run on). Fix: 3 (rv64: hart 0: 3; rv32: hart 0: 1,
hart 1: 2); timer-any-hart: 1,999 on both widths (every nap of both sleepers, hart 0: both
sleepers had settled on hart 1). Under the fix the sleepers migrate between harts (picks ~500 each
per hart: a hart whose entry finds the other sleeper's timeout passed first serves it, I13); under
any-hart both settled on one hart and the other woke for nothing 1,999 times. The sections'
histograms by record kinds (awk over the console logs) are in meas2/meas3 analysis above.

Commit 2: 471852f1b on 822d05a27. tools/testbench/src/sched_oracle.rs: arg
`idle_sections_max=<mark>:<N>` (malformed refused) and `idle_sections_that_found_nothing()`:
walks the records tracking per hart whether its runner was a thread or kmain at its last `j`
and whether anything but U/V was written since; an `h` after `H 0` with nothing written counts,
from the marked budget's first `H` on; the budget never running or no section at all is an
error; the line names the count per hart. Host test
`idle_sections_that_found_nothing_are_bounded` (round-robin fixture + hart 1's `H 0`, a leaving
section, two empty sections (one with an audit), one with a `$`, one on hart 0 under a thread,
one empty before 44 ran: count 2; bound 1 fails, 2 passes; malformed args and an unknown mark
refused). tests/programs: bin timer-flood-at-rest (A marked weight 2 and B, each WakeDelay
[1000 µs, 1000, 0]; note with both least delays); Cargo.toml entry. tests/timer-flood-at-rest.toml
(checked, sched-trace + hold-trace, icount, smp [2], `idle_sections_max=2:20`; the comment says
which hart takes a sleeper's timeout and why, why the idle hart's empty sections are the measure,
the numbers, the negative, and why one sleeper shows nothing). docs/testbench.md: the arg's
sentence in the oracle section, oracle status 31 (+host test), checked-builds status 20 (+bench);
docs/kernel/timer.md hart-timer status 12 (+bench:timer-flood-at-rest).
Runs (.tmp/IDLE2/meas3/): fix rv64 3, 3, 2 / rv32 3, 2, 3 idle sections finding nothing (three
runs a width; bound 20); timer-any-hart 1,999 / 1,999 (FAIL both widths, the negative; console
logs in meas3/last/). The sleepers' least wake delay: 1,049 / 1,049 µs (rv64) and 1,415 / 1,416
(rv32) with the fix, against 1,148 / 2,277 and 1,655 / 2,953 under any-hart (contention gone).
Gates on 471852f1b: oracle 59 pass (.tmp/IDLE2/c2-oracle-test.log), test-programs both targets,
fmt clean, size-budget, docs, formatting PASS (.tmp/IDLE2/c2-gate-*.log). Binding 5 confirmed
again: due(), note_*, rearm's sites, the model untouched (commit 2 is bench + tests + docs).
Review (kernel-red-2): OK to build commit 3 on. P3 for commit 3: a hart's first section with no
prior `j` reads as entered from the idle (at_start defaults false); say so in the fn doc or
require a prior `j`. Commit 3: binding 2's re-judging at 2 harts both widths and binding 3's five
launch-idle windows a width (ceilings; the before claimed as a negative only if recorded failing).

## Commit 3 reviewed; the head is 87553025a (idle2-implementer, 2026-10-14)

Review of 539344966 (kernel-red-2, IDLE2-design event 8e5839a3): merge OK, no re-review, one P2
and the condition that the head's final launch-idle pass both widths. P2: timer.md's "Kernel work
is not interrupted" bullet gave the N = 16 timer-wake rise (1.6 / 3.7 ms, more than a section)
without its mechanism. Amended into commit 3 (539344966 -> 87553025a, message unchanged): the
bullet now says sixteen sleepers' timeouts, every payer running nowhere, are armed on one hart and
their wakes served there in turn while the other idles (before, about half each): a cost of the
one-hart rule under a burst of timeouts, not of a section; with the trigger (timer wake p99 at
N = 16 nearing its 50 ms target, or a workload with sleepers bursting on one hart: the covering
hart hands items due beyond the first to an idle hart, or an idle hart is preferred to cover).
scheduling.md's residuals gain the bullet "A burst of timeouts is served by one hart" pointing at
timer.md#residual-risks (absent before). Gates on 87553025a: docs PASS 1.6 s
(.tmp/IDLE2/c3b-gate-docs.log), formatting PASS 22.7 s (c3b-gate-formatting.log); the code is
539344966's (size-budget and `cargo test -p testbench` unchanged from c3-*).

Final launch-idle at the head (code = 87553025a's; ceilings 3.8 / 12.2 / 0.01), once a width,
general cores (q, 8 cores) beside B51's kernel-containment, walfs and host-test cases and B61's
beamlet system tests (.tmp/IDLE2/final/): rv64 PASS 66.5 s, [idle] 60.0 s: QEMU 0.001 host cores;
hart 0 1.8/s (m_software 0.8, s_timer 1.0), hart 1 1.1, hart 2 0.4, hart 3 0.1; supervisor_ecall
5.7, user_ecall 6.1. rv32 PASS 66.3 s: hart 0 1.3 (s_timer 0.0), hart 1 1.7 (s_timer 1.0), hart 2
0.1, hart 3 0.4; supervisor_ecall 5.6, user_ecall 6.1. Busiest hart 1.8 / 1.7 against 3.8,
user_ecall 6.1 against 12.2: as the ten windows. Branch frozen at 87553025a (b167ef0d1, 044c9a955,
87553025a on 146db8897), tree clean. Scratch worktrees .tmp/IDLE2/before and /drain remain for the
orchestrator to remove at the merge.

## Reopened: sched-capped at 3 harts (idle2-implementer, 2026-10-14)

Finding (orchestrator, QA IDLE2-sched-capped): rv64 sched-capped [smp=3] failed in train 26's bench and
alone at 87553025a. The clause read on the thread (late-a 271/272) is NOT the judged one: late-* carries
`50@2` and the oracle prints "judged at 2 harts only: not judged" for it at 3 harts. The missed clause
in every failing run is the second-cap scenario (`50@3`): second-a or second-b, 273-280 against
283..383 (.tmp/train-26/fail-alone-rv64-sched-capped.log line 36: second-a 273).

Pin (one at a time on the quiet set, nothing else there; prebuilt per tree; .tmp/IDLE2/pin/<tree>-<i>.log
with the oracle's summary, <tree>-<i>-smp3.log the console; run.sh): main 146db8897 (.tmp/IDLE2/pin-main)
smp=3 FAIL 2 of 5 (run 1 second-a 277, run 3 second-b 280; runs 2/4/5 PASS with second-a 340/291/287),
head 87553025a FAIL 3 of 5 (run 1 second-b 279, run 3 second-a 276, run 5 second-a 278; runs 2/4 PASS,
second-a 360/365). smp 2 and 4 PASS in all ten. Main fails alone too: IDLE2 is not the cause; the
difference 2 vs 3 of 5 is within what five runs tell. Train 25's bench (main) failed the same case
under load (.tmp/train-25/bench.log 501) and passed in its rerun.

Trace of the second window (records from the "at 3 harts" summary line; `I` = a timer interrupt,
`Q` = a wait for the kernel lock, ticks; every run of both trees alike in shape): no timeout fires in
the window (no `W`, no `$`, no `U`/`V`: all 5 budgets' records are slice ends, `I = B P R K O`,
each hart's own runner interrupted and charged); hart 2 runs one heavy one-thread budget (second-a b43
or second-b b47, by placement) for the whole window, hart 1 the other, hart 0 the three 10-weight
budgets and idles. The three harts' slice ends meet at the lock every slice in a fixed order and each
slice-end section in this checked, traced build is ~6,300-6,600 ticks (one section = hart 1's mean
wait, 630-660 µs): hart 1 waits one section at every slice end (3.0-5.6M ticks of the 19.9M-tick window),
hart 2 1.3-2.3 sections (4.6-9.0M = 23-45 % of its hart), hart 0 0.3-1.6. The share is the judged
budget's charged ticks over all charged ticks net of every wait, so the heavy budget on the hart last
in the lock order reads 276-340; under ~5.9M lost by hart 2 with hart 0 waiting little, 276-280: a
miss. Which heavy budget sits on hart 2 is the placement's, so second-a and second-b miss in turn.
Under icount the order the three simultaneous slice ends take the lock is QEMU's turn order, fixed
for the window once set (a slice end is re-armed from its own return, so three slice ends that once
meet at the lock meet there every slice): the trap scheduling.md names, "A hart waits for QEMU's
turns"; the toml already concedes it at 2 harts (judged in sched-capped-mttcg, smp=[2] only) and
the 3-hart second-cap judgment has the same exposure with no twin.

The late window (recorded, not judged at 3): main lockstep at ~2.1 ms with hart 0 rarely waiting
(I 933-954 a hart; waits 0.8-2.2M / 1.3-1.9M / 5.0-5.7M); head in 4 of 5 runs a tighter lockstep at
~2.6 ms where every hart waits every slice end (I 729-781; 5.7-6.2M / 4.0-5.5M / 7.5-9.3M), head-4 as
main. late-a's thread (b31) ran on hart 1 (main) or hart 2 (head), unmoved all window; its waits were
behind the other two harts' slice-end sections. The pattern is set at the window's start by how the
three threads come onto their harts (the phase IDLE2 moves: which hart was armed and awake at rest
before the window) and persists; it changes no judged clause at 3 harts and the 2-hart shares are the
twin's. Reported, not pursued: no fix until kernel-red-2 rules.

### Ruling's evidence (kernel-red-2, IDLE2-sched-capped event 13fe9d6f)

(a) Five alone runs a tree, rv64 sched-capped smp=3, quiet set, own prebuilt, alternating main/head
(.tmp/IDLE2/pin/): late-a / late-b / late-c shares (recorded at 3 harts, `50@2`), then each budget's
lock waits in Mticks of the window's total, then the judged second-a / second-b:
main-1 FAIL 353/282/364, waits 1.3/5.0/0.8 of 7.2; second-a 277 (miss) / second-b 333
main-2 PASS 285/356/358, 5.5/1.9/1.8 of 9.1; 340 / 364
main-3 FAIL 352/286/360, 1.7/5.1/1.3 of 8.1; 333 / 280 (miss)
main-4 PASS 283/360/355, 5.7/1.9/2.2 of 9.8; 291 / 345
main-5 PASS 354/284/361, 1.6/5.2/1.2 of 7.9; 287 / 352
head-1 FAIL 276/343/379, 8.8/6.1/4.6 of 19.5; 332 / 279 (miss)
head-2 PASS 299/347/353, 7.7/5.8/5.5 of 19.1; 360 / 305
head-3 FAIL 303/345/351, 7.5/5.7/5.5 of 18.7; 276 (miss) / 333
head-4 PASS 279/361/358, 6.0/1.9/2.1 of 9.9; 365 / 339
head-5 FAIL 263/340/396, 9.3/6.2/4.0 of 19.5; 278 (miss) / 382
(a) fails on both trees: by the ruling's decision table, the clause was already broken.

(c) The kept trace (.tmp/train-26/sched-capped-run/sched-capped-rv64-smp3.log), per window:
late (records 18749..=36355): I records 734 a hart, each interrupting the hart's own runner (h0 b35,
h1 b39, h2 b31 = late-a), all slice ends (I = B P R K O 0, the B to the interrupted budget); no E,
no W, no `$` at all in the window (no timeout fired, nothing charged to another budget); no `H 0`
(no hart idle); Q waits 734/724/734, each behind the section of the hart whose records precede it:
h0 behind h1 (734), h1 behind h2 (724), h2 behind h0 (734): a ring, one fixed order, the lock
released by the slice end that fired just before. second (64311..=90168): I h2 b43 840 (second-a),
h1 b47 840, h0 b51/b55/b59 274/276/290; no E, W or `$`; `H 0` on h0 550 times (the 10-weight
budgets' hart idles between them); Q: h1 behind h0 840, h2 behind h1 839, h0 behind h2 145. Hold
trace is not on in this case, so the holder is read as the hart of the preceding record.

(b) sched-capped-mttcg with smp [2, 3] (temp toml edit in the scratch trees .tmp/IDLE2/pin-main and
.tmp/IDLE2/pin-head only; wp-IDLE2 untouched), five runs a tree alternating, quiet class
(.tmp/IDLE2/twin3/): see below when in.
(b) in (2026-10-15, .tmp/IDLE2/twin3/<tree>-<i>.log, -smp3.log): PASS 10 of 10 at smp 2 and 3 on both
trees. At 3 harts, where the harts run at once, every run of both trees: late-a 332-333, late-b
332-334, late-c 332-334 (lock waits 66k-438k ticks a window in all, 0.1-0.7 % of the harts' time:
no hart waits a section at every slice end); second-a 327-328, second-b 328-329 (waits 2.1-2.3M in
all, ~1 % of 3 harts, split evenly between the two heavy budgets). The 60-per-mille losses are
icount's turn order alone: under mttcg the three slice ends do not queue behind one another.
Reading against the ruling's table: (a) fails on both and (b) passes near 333 on both: the clause
was already broken, IDLE2 is cleared; the fault is the case's 3-hart icount judging without a twin.
