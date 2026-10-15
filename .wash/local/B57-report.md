# B57 report (b57-implementer)

Base main db6575a3f, branch wp-B57, worktree .worktrees/B57. Head **501b527a1**:
- 27bfc28b6 kernel: the console says when a large drain of frames in flight begins and when they are all back
- 501b527a1 testbench, tests, docs: launch-idle's window opens once the frames in flight are back to 0

Tier A (kernel changes); kernel-red OK'd the design with conditions (a)-(c), all met (below).

## Delivered

- **Kernel** (kernel/src/mem.rs, kernel/Cargo.toml, +18 lines, no unsafe). The release count
  `MemoryManager.in_flight` gains a peak, `in_flight_peak`, and the named const `IN_FLIGHT_LOUD = 1024`,
  whose comment gives the reason and the rate bound.
  - `went_in_flight(n)` replaces the two `in_flight +=` sites: retire, and the pend of an object chain.
    The compare sits beside that update. The first time the count reaches 1,024 since it was last 0,
    it prints `mem: 1024 frames in flight`.
  - `give_back`, beside the existing `-= 1`: when the count is 0 and the peak was at least 1,024, it
    prints `mem: frames in flight 0, at most N` and resets the peak. The common path costs one
    compare with 0.
  - Both lines are `println!`, the kernel's CONW1 path (queued under the console's hold).
  - The test-only feature `inflight-never-none` drops the second line, for the recorded negative.
  - tests/size-budget.toml: the kernel ceiling goes 10642 → 10660, with a `Size budget:` line in the
    commit.
- **Bench** (tools/testbench/src/idle.rs, qemu.rs).
  - The window tracks drains from the kernel's bare `mem:` lines. Program lines carry consoled's
    `[con …]` prefix and do not count.
  - At the idle step's start the session is held. The window opens after:
    - the console has been silent for 50 ms, so lines already queued are read;
    - and a drain has ended since the sessions began, with none open now.
  - Bound: `DRAIN_WAIT` = 30 s. A drain still open, or none ended, fails the case with an
    `[idle] drain:` text giving the elapsed time and what is known (`over 1024` while open). The peak
    is only known from the 0 line.
  - An `[idle] drain:` line before the window's line gives:
    - the peak, the wall time from the step's start to 0, QEMU's CPU time meanwhile and its cores;
    - or that none ended in the wait;
    - and any drain that opened inside the window.
  - Five new host tests.
- **Docs.**
  - docs/kernel/memory.md "Frames in flight": what the lines are and why 1,024. It gives the rate
    bound (at most two lines per 4 MiB freed and drained), and the timing channel they reveal (someone
    freed 4 MiB or more, and when it drained: out of scope by TENETS#threat-model).
  - docs/testbench.md "The machine at rest": the wait, its bound and why, the drain line, the measured
    numbers, and 5 new host tests in the status list (6 → 11).
  - tests/launch-idle.toml: the description. Ceilings are unchanged (9.0 / 18.0 / 0.01).

## Finding: the 35,000 frames are the boot's and the login's, and on rv32 they come back ~6 s after the step

- The rise line prints during the boot (console: after `init: started erofsd:system`, before sshd
  starts). The 0 line prints only after `sshd: login alice`. So the ~35k frames include the boot's
  frees, not only the login's (B56's report attributed them to the login).
- rv64: the frames are back 0.32-0.39 s after the step began, at ~0.6 host cores.
- rv32: 5.8-6.2 s, at ~0.15 cores and 0.88-0.94 s of QEMU CPU.
- So on rv32 the old window opened ~6 s before every hart had first idled. Why rv32 takes that much
  longer after the prompt is not measured. The docs say so rather than guess.
- The bound was first set at 10 s, then raised to 30 s on this evidence (≈5x rv32's most). The
  launch-idle windows now read 0.001-0.003 host cores, against B56's 0.005-0.008: IDLE1's level.

## Early checkpoint (2026-10-12): how the case knows frames in flight reached 0

### Why nothing already printed will do

- launch-idle boots the **release** kernel. The case has no `debug_assertions`, so
  tools/testbench/src/main.rs:889 picks `Profile::Release`.
- `reclaim::counted()` and the destruction audit are checked-build only.
- The in-flight trace records (`feature = "inflight-trace"`: PENDED / RETIRED / COMMITTED / TAKEN)
  are built only into the smp-inflight-race kernel, and the trace prints only at a reset.
- The release build prints no line about frames in flight.

So the case needs a small kernel addition, and B57 is Tier A.

### What the kernel already counts

`MemoryManager.in_flight` (kernel/src/mem.rs:96) is a release counter of the frames in flight:
- `+1` in `retire` (mem.rs:561);
- `+chain.linked` in `in_flight_linked` (mem.rs:787);
- `-1` in `give_back` (mem.rs:510), reached from `commit` and from `reclaim_wait`'s on-demand pop.

When it is 0, every frame anyone freed is zero and back in the bitmap. Every list is then empty:
staged, pending, zeroing, done and committing.

### Kernel addition (mem.rs, about 10 lines, no unsafe)

A flag plus a peak, both on MemoryManager (under the lock). There is hysteresis, so a machine at
rest prints nothing:

- **Rise:** when `in_flight` reaches `LOUD` = 1,024 frames (4 MiB) and the flag is clear, print
  `mem: 1024 frames in flight` and set the flag.
- **Fall:** when `give_back` brings `in_flight` to 0 with the flag set, print
  `mem: frames in flight 0, at most N`, where N is the peak, and clear the flag.

How often it prints:
- **At rest:** frees of 1-3 frames every few seconds never reach 1,024 (B56 follow-up's
  measurement), so the kernel prints nothing.
- **Lines expected:** at boot, at a login (about 35k frames), and after a session's step (about
  3.8k frames).

The 0 is the real count after the commit. The last done frames are given back at the next lock
take, which at rest is the next timer interrupt: up to about 1 s after the drainer's last frame.
The kernel does not change to hurry that.

Alternatives rejected:
- **Reading the lists at `take_one`'s last frame:** this is not the count. Done lists are still
  uncommitted, and the on-demand pops never reach it.
- **Reading the counter's address through QEMU's monitor (`xp`):** this ties the bench to a
  symbol's address and to the monitor's output format.
- **A host-clock settle:** this is forbidden by the node.

### Bench (tools/testbench/src/idle.rs, qemu.rs `run_sessions`)

1. **Tracking:** the console loop tracks `draining` from the two kernel lines. The rise line sets
   it and the fall line clears it.
2. **Deciding at the start edge:** at the idle step's start edge, the bench first reads the console
   lines already queued, so a rise line sent before the prompt is not missed.
3. **Holding the session:** if a drain is open, the session's `done` is held. Its quiet does not
   begin, and the window (`log int` and the CPU sample) is not opened, until the fall line comes.
4. **The bound:** 10 s from the step's start.
   - Measured drain: 0.25 s (rv64) or 0.32 s (rv32) guest time.
   - The commit at the next timer adds up to about 1 s.
   - That leaves about 3x margin for a loaded host.
   - A drain that never ends fails the case: `frames in flight did not reach 0 within 10 s of the
     idle step`.
5. **What is printed:** an `[idle] drain:` line beside the window's `[idle]` line. It gives:
   - the peak N;
   - the host wall time and QEMU's CPU time from the step's start to the fall line;
   - or "drained before the step".

   If a rise line comes inside the window, the `[idle]` line says so. A large free at rest is then
   visible, not hidden.

Ceilings are unchanged: interrupts_per_hart 9.0, user_ecalls 18.0, host_cores 0.01.

## Gates

Env per launch-env-2026-10-12. `make -f scripts/jobs.mk prebuilt` was rc 0 at each measured head.

### launch-idle alone, 4 harts, x3 a width

Command: `q run --cores 4 --quiet -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> launch-idle`.

Run at 109d21d5a. Its code is identical to head 501b527a1: only comments, docs and the commit
message changed since. Every run is rc 0, PASS.

| run | drain: peak, back to 0 after step, QEMU CPU (cores) | window: host cores, busiest hart /s, user_ecall /s |
| --- | --- | --- |
| rv64 1 | 35008, 0.32 s, 0.21 s (0.659) | 0.001, 3.0, 3.9 |
| rv64 2 | 34632, 0.35 s, 0.19 s (0.550) | 0.002, 3.3, 3.9 |
| rv64 3 | 35142, 0.37 s, 0.21 s (0.561) | 0.002, 3.5, 3.9 |
| rv32 1 | 35626, 6.03 s, 0.92 s (0.153) | 0.002, 3.4, 3.4 |
| rv32 2 | 35631, 5.80 s, 0.92 s (0.159) | 0.002, 2.9, 3.4 |
| rv32 3 | 35670, 6.21 s, 0.88 s (0.142) | 0.002, 3.1, 3.4 |

An earlier 3+1 runs per width at 4a7aa8d52/fa312ab9a (same kernel, 10 s bound) read alike:
- rv64: 0.24-0.39 s, at 0.002-0.003 cores;
- rv32: 5.77-6.23 s, at 0.002-0.003 cores.

No drain opened inside any window. After `log none` the console shows a drain of ~4,640 frames,
from the session's next step.

### launch-idle under two parallel boots

Two loops of `userland-boot` ran, rv64 and rv32, each `q run --cores 4`, during the case: 3 + 4
boots, all PASS.

- rv64: rc 0, PASS. Drain 35151, 0.34 s, 0.18 s CPU. Window 0.002 cores, busiest hart 3.1.
- rv32: rc 0, PASS. Drain 35530, 5.80 s, 0.94 s CPU. Window 0.002 cores, busiest hart 3.1.

The window no longer catches the drain: host cores match the alone runs.

### Recorded negative

A scratch worktree at 109d21d5a, with launch-idle.toml given `debug_assertions = true` and
`kernel_features = ["inflight-never-none"]`. Command: `q run --cores 8 -- cargo testbench --exact
--arch rv64 launch-idle`. Result: rc 1, FAIL, 40.3 s:

    idle: the frames in flight did not come back to 0 within 30 s of the idle step: [idle] drain: no drain ended since the sessions began 30.0 s after the step began

The console had `mem: 1024 frames in flight` and no 0 line. The scratch worktree is removed.

### Gate set at head 501b527a1

Command: `make -k -f scripts/jobs.mk set CASES="formatting docs size-budget unsafe-budget host-tests
memory-host-tests userland-boot init-boot bench-net-peer ipc-outcomes sum-clear
lend-untouched-page launch-system smp-inflight-race"`. Result: rc 0, all PASS, both widths:
- the smoke set, lend-untouched-page at 1 and 4 harts;
- launch-system at 4;
- smp-inflight-race at 2.

### Other checks

- Host tests: `cargo test -p testbench --bin testbench idle`, 10 pass. Also inside host-tests.
- Clippy on testbench: no warnings in the changed code (the existing ones are elsewhere).
- Console patterns: every string pattern in tests/*.toml (expect, forbid, fail and the sessions')
  was matched against both new lines with a script (.tmp/B57/match.py). None matches.

## Pages and summaries checked

- **Updated:**
  - docs/kernel/memory.md "Frames in flight": a new paragraph on the lines, 1,024, the rate bound
    and the timing channel.
  - docs/testbench.md "The machine at rest": the wait, its bound, the drain line, the measured
    numbers, and the status list (11).
  - tests/launch-idle.toml: the description.
- **Checked, no change:**
  - README.md, docs/README.md: no idle or in-flight claims.
  - GETTING-STARTED.md "Try the system" (lines 110-125): it says the terminal is the serial
    console and names only `sshd: listening`, so the new kernel lines contradict nothing.
  - docs/plan/m2-usable-shell.md lines 169-170 and 240 (R81 step): still true, since zeroing and
    commit are unchanged.
  - docs/kernel/README.md: the TCB unsafe cells are unchanged (no unsafe added). Its line counts
    are declared snapshots (line 379), and the size budget is held separately.
  - docs/kernel/devices.md "The console's one writer": the new lines are ordinary kernel lines on
    that path, so no change.
  - docs/kernel/timer.md and scheduling.md: nothing on drain lines.

### kernel-containment at 2 harts (timing check)

New lines could land inside destructions, so this case was run for its timing. Command: `q run
--cores 2 -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> --smp 2
kernel-containment`, at head 501b527a1.

| | result | `mem:` lines | gross notice p50 / p99 | gross driver wake p99 |
| --- | --- | --- | --- | --- |
| rv64 | rc 0, PASS, 370.4 s | 1 rise, no 0 line before the end | 181 / 266 ms | 5.6 ms |
| rv32 | rc 0, PASS, 524.5 s | 1 rise, no 0 line before the end | 159 / 244 ms | 4.2 ms |

The gross numbers include audits; the net post-check (R10 within 40 ms) passed. So a destruction
pays for at most one ~30-byte line per run.

## Open risks

- **Kernel line cost.** One SBI call per byte. The rise line can land inside the call that retires
  the 1,024th frame, and the 0 line inside a lock take's commit. Per the rate bound, that is at most
  two lines per 4 MiB freed and drained. Containment shows one.
- **rv32's ~6 s wait after the prompt.** It is unexplained (not measured here). It is the dominant
  part of the wait and sets the 30 s bound. A candidate for IDLE2.
- **Lines before the sessions begin are not tracked.** A drain that opened at boot and never
  closed is caught only by the "none ended" rule. The negative shows that rule fires.

## kernel-red P2: the rv32 drain measured (no kernel edit)

**Method.** `inflight-trace` needs a checked build, and its trace prints only at a reset, which
`launch` never does. So the measurement uses temporary release-build counters in a scratch
worktree (.tmp/B57/meas at 501b527a1; patch .tmp/B57/measure-drain-b57.patch, not for commit).
They are printed after the kernel's `mem: frames in flight 0` line.

Counters, from the rise line to the 0 line:
- drain passes (`take_one` returning a frame) and the guest-time gaps between consecutive passes,
  bucketed;
- `take_one` with its own frames pending but not taken: not all idle, or another hart drains;
- claims dropped because not all idle;
- wakes sent by `take_one` (hand-on and last-idler);
- halts with own frames pending, and what ended each halt (`sip` after the `wfi`: timer, IPI or
  external);
- **stranded time:** with every hart idle, frames pending and DRAINER 0, measured from a halt to
  the next pass.

Release kernel, launch-idle, 4 harts, `q run --cores 8 -- cargo testbench --exact`.

**Run A** (first patch, before the stranded and woke counters). rv32 and rv64 PASS. rv32 back to 0
0.40 s after the step (peak 29,074).
- rv32: rise to first pass 133 ms, first to last pass 3.79 s. Gaps:
  - <50 µs: 34,768, totalling 0.235 s;
  - 0.5-5 ms: 895, totalling 1.13 s;
  - 5-50 ms: 31, totalling 0.36 s;
  - ≥50 ms: 8, totalling 2.06 s.

  Stops: blocked not-all-idle 5,927; claims dropped 832; wakes sent 3; halts with own pending
  5,935.
- rv64 alike: span 3.65 s, gaps 924 / 25 / 8, wakes 259.

**Runs B** (all counters), rv32 x3 and rv64 x2. The drain shown is the boot's, ending in the
console session's prompt. Every run then failed at the SSH login: the host's loopback TCP is down,
see below.

| | passes | span first→last | gaps 0.5-5 ms | 5-50 ms | ≥50 ms | stranded | halts with own pending ended by timer / IPI / ext |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rv32 #1 | 18,648 | 1.80 s | 467, 0.56 s | 13, 0.16 s | 5, 0.94 s | 3 times, 0.26 ms | 6 / 2,091 / 1,200 |
| rv32 #2 | 18,648 | 1.71 s | 498, 0.53 s | 12, 0.11 s | 5, 0.94 s | 3, 0.44 ms | 5 / 1,935 / 1,416 |
| rv32 #3 | 18,648 | 1.81 s | 499, 0.57 s | 12, 0.12 s | 5, 0.99 s | 3, 0.40 ms | 6 / 1,948 / 961 |
| rv64 #1 | 18,357 | 2.08 s | 513, 0.61 s | 12, 0.16 s | 5, 1.19 s | 148, 32 ms | 5 / 1,936 / 1,110 |
| rv64 #2 | 18,357 | 1.96 s | 495, 0.66 s | 13, 0.17 s | 5, 1.02 s | 123, 42 ms | 6 / 2,028 / 977 |

Zeroing itself: 21-23 ms for 18,648 frames on rv32, 15 ms on rv64, about 1 µs a frame. Back-to-back
passes cost ~7 µs each, the lock round trip.

**Reading.**
- **No stranding.** Time with every hart idle, frames pending and nobody draining is 0.3-0.4 ms per
  drain on rv32 and 32-42 ms on rv64 (the hand-on latency: about 0.3 ms each, 123-148 hand-ons).
- **A halt with frames pending ends because work arrives.** It ends on an IPI (lock hand-off or
  wake, ~2,000) or a device interrupt (~1,000-1,400). Only 5-6 per drain end on the timer, which is
  what a stranded hart would wait for.
- **The gaps are work-sized.** They fall at 0.5-5 ms and in a few ≥50 ms periods. The drain is
  stopped by not-all-idle (3,000-6,000 times), with its claim dropped 450-850 times, and it resumes
  when the work ends: the last idler finds its own frames pending, which is why wakes are few.
- **Verdict:** no R81 liveness defect. The drain runs only while every hart idles, and the boot and
  login keep some hart busy for most of the drain's span.
- **Still open:** the post-login rv32 case (the prebuilt runs' 5.8-6.2 s) could not be measured
  with SSH down. Run A's passing rv32 had its drain over before 0.40 s after the step.

**Host problem.** Since some time after this report's gate set passed (2026-10-12), TCP connects to 127.0.0.1 time out on this host,
inside and outside the sandbox. A Python listen() then connect() to its own port times out after
5 s. Every SSH or launch case fails at its first login ("ssh: connect to host 127.0.0.1 port N:
Connection timed out"). The uninstrumented head fails the same way.

## kernel-red P3 (done in the worktree, not yet committed)

- The toml names the 50 ms console silence a host-clock settle, for the queued lines only.
- On a timeout, the `[idle] drain:` text names each unmet condition: the console never silent, a
  drain still open, or no drain ended.
- One test is extended for the silence case. Host tests: 10 pass.

## Head cf7012933 (P3 and the bound's reason amended into the tests/docs commit)

- 27bfc28b6 kernel (unchanged).
- cf7012933 testbench, tests, docs: P3 (the 50 ms silence named a host-clock settle; a timeout
  names each unmet condition) and the bound's reason, with the measured counts, in
  tests/launch-idle.toml.
- Gates after the amend:
  - `make -f scripts/jobs.mk set CASES="docs formatting"`: rc 0, both PASS.
  - idle host tests: 10 pass.
- Nothing needing 127.0.0.1 was run after the loopback failure, per the orchestrator. The
  post-login rv32 re-measure waits for loopback; the scratch tree .tmp/B57/meas keeps the counters
  applied.

## Round 2 (2026-10-14, b57-implementer-2): kernel-red's BLOCK at cf7012933

Base main db6575a3f. Head **cd40d1eb2** (both commits amended, no fix-up commits):
- 807ce2b1a kernel: the console says when a large drain of frames in flight begins and when they are all back
- cd40d1eb2 testbench, tests, docs: launch-idle's window opens once the frames in flight are back to 0

### Delta

- **P1.** The inside count leaves the `[idle] drain:` line, which is written once at the window's
  start, when it is 0 by construction. `Window::judge` now appends `; N drain(s) opened inside the
  window` to the window's own `[idle]` line, written after `end()`, so a rise inside the window is
  reported where it can be seen. The host test
  `only_the_kernel_s_lines_open_a_drain_and_one_inside_is_named` now opens a window (start set,
  end unset), sees a rise, closes it, sees another rise (not counted), and asserts the judged
  summary ends with the count, the drain line does not carry it, and a window with none ends with
  its exceptions. docs/testbench.md "The machine at rest" says the window's own line names it;
  the tests/docs commit message says the same. tools/testbench/src/idle.rs, docs/testbench.md.
- **P3.** `IN_FLIGHT_LOUD` is no longer `pub` (kernel/src/mem.rs; read only there). Amended into
  the kernel commit; the `Size budget:` line is unchanged (the ceiling and the kernel's size are).
- **P2** (follow-up, not this package): see Residual below.
- Both commit messages gain a second `Co-Authored-By` for this round's model.

### Gates (env per launch-env-2026-10-12; every command through scripts/q)

- `q run --cores 4 -- cargo test -p testbench --bin testbench idle`: 10 passed, rc 0
  (.tmp/B57/r2-host-idle.log).
- `make -f scripts/jobs.mk prebuilt && make -k -j4 -f scripts/jobs.mk set CASES="formatting docs
  size-budget launch-idle"` at cd40d1eb2: rc 0 (.tmp/B57/r2-gates.log).
  - prebuilt rc 0; size-budget PASS; docs PASS 1.6 s; formatting PASS 22.8 s.
  - launch-idle rv64 PASS 66.5 s: drain back to 0 **0.36 s** after the step (peak 34,252), QEMU
    0.22 s CPU (0.605 cores); window 0.002 cores, busiest hart 3.2/s.
  - launch-idle rv32 PASS 73.2 s: drain back to 0 **6.33 s** after the step (peak 35,643), QEMU
    0.91 s CPU (0.144 cores); window 0.002 cores, busiest hart 3.0/s.
  - No drain opened inside either window (no `inside` text on the `[idle]` line).

### rv32 post-login re-measure (the one Round 1 left pending)

Scratch tree .tmp/B57/meas at 501b527a1 with measure-drain-b57.patch applied (kernel identical to
head's but for the counters and `pub`). `q run --cores 4 --lock net -- cargo testbench --exact
--arch rv32 launch-idle`: PASS, 66.5 s, rc 0 (.tmp/B57/meas3-rv32.out; console in
.tmp/B57/meas/target/testbench/run-2272992-1792007058696834762/launch-idle-rv32-smp4.log).

The login's drain, rise to 0 (peak 34,750): rise → first pass 152 ms, first → last pass 3.61 s,
last → 0 line 165 ms; 35,718 passes, zeroing 40.6 ms (1.1 µs a frame); 40,787 `take_one` calls.
- Gaps: <50 µs 35,320 (0.25 s); <500 µs 9; 0.5-5 ms 320 (0.59 s); 5-50 ms 60 (0.56 s); ≥50 ms 8
  (2.22 s).
- Stops: blocked not-all-idle 5,063; blocked other-drains 6; claims dropped 386; wakes sent 3;
  halts with own pending 5,069.
- Stranded (every hart idle, frames pending, nobody draining): 3 times, 331 µs in all. Halts with
  own frames pending ended by timer 51, IPI 3,840, external 1,177, none 1.
- The session's next step after the window: a drain of 4,433, 0.40 s first → last, stranded 3
  times, 156 µs.

Reading, as Round 1's: no stranding; the drain's span is work-sized gaps while the boot's and the
login's harts are busy, resumed as they idle; of 5,069 halts with frames pending, 51 ended on the
timer.

**Unexplained, recorded for IDLE2:** in this instrumented run the rv32 drain ended 0.38 s after
the step, as Run A's did (0.40 s), while every prebuilt run at head (this round's 6.33 s, Round
1's 5.8-6.2 s) ends it about 6 s after the step. The instrumented tree is built and booted by
`cargo testbench` in the run, the gate's by the prebuilt bench; which of that, or the counters'
own cost, moves the boot and the login relative to each other is not measured here. The bound
(30 s) and the docs' numbers (six prebuilt runs alone) stand; 6.33 s is within the "about five
times rv32's most" the bound gives.

### Residual (kernel-red P2, verbatim; a follow-up, not this package)

> P2: the window's 'none ended since the sessions began' rule holds only because the boot's
> excursion stays open past the login (rise before sshd, fall after login). The login's own share
> of the ~35k is unmeasured; a boot that drains before `sshd: listening` with a login freeing
> <4 MiB fails the case at 30 s with a misleading text. Follow-up: feed Window the console from
> boot, or measure the split.

### Pages and summaries checked this round

- docs/testbench.md "The machine at rest": updated (where the inside count is named). Status
  list unchanged (11; the test keeps its name).
- docs/kernel/memory.md "Frames in flight": no `IN_FLIGHT_LOUD` or inside-count claim; no change.
- tests/launch-idle.toml: its comment names the drain line and the bound only; no change.
- README.md, GETTING-STARTED.md, docs/README.md, docs/kernel/README.md: no claim touched by this
  round (checked in Round 1, unchanged since).

## Round 3: the split line (2026-10-14, b57-implementer-3; read, nothing run, no code changed)

Head still cd40d1eb2 on db6575a3f. Thread B57-split-line.

### The mechanism

- **consoled writes a line in one hold, prefix and body together.** `Console::write`
  (servers/consoled/src/server.rs:304) takes `HOLD_CHUNK` = 256 bytes at a time and, inside
  `registers.console_held(..)`, calls `Lines::write` (:118), which puts the `[con ID] ` prefix
  (only when `open` is `None`) and then the body through `uart.put_all`. So inside one 9P write no
  kernel line can come between prefix and body: the orchestrator's option (a) is already the code.
- **The hold covers a write, not a line.** Between two 9P writes nobody holds the console.
  `Lines` remembers an unfinished line (`open = Some{badge, prefix: 23}`) across writes, and the
  next write by the same badge continues it without a prefix.
- **The kernel's line goes out at once when nobody holds.** `println!` → `debug::console::print`
  (kernel/src/debug/console.rs) → `Hold::line` (libs/conhold) returns `Line::Free` with `holder == 0`
  and the bytes go to the SBI console immediately; the kernel neither knows nor ends consoled's open
  line. `give_back` prints under the kernel lock (`MemoryManager`), from `commit`, which runs at
  every lock take (mem.rs:493); the fall line fires at the first lock take after the drain's last
  frame, which at rest is the next trap of any kind.
- **The session leaves a line open across writes.** The shell's result is drawn as the iodata
  `[to_origin, erase_below, text, ..]` (userland/shell/lib/redoubt/term.ex:325,530-532) and the
  console port writes its leading `"\r"` as a 9P write of its own: every result line in a passing
  log is `[con ID] ^M:timed` (target/testbench/last/beamlet-reduction-rate-rv64-smp1.log:62). In the
  failing run (target/testbench/run-2638331-1792021269057172991/...rv64-smp1.log:63-64, `cat -v`):
  `[con ID] ^Mmem: frames in flight 0, at most 20654` then `:timed` on a bare line. So: write 1 =
  prefix + `\r` (held, released); the loop's 30 s of sleeps idled the hart and zeroed the login's
  frames, and the first lock takes after it are this write's own traps, so the fall line is printed
  there, with nobody holding (or queued and flushed by that release): straight after the `\r`;
  write 2 = `:timed\r\n`, which `Lines` continues bare because its `open` says the prefix is out.
  Deterministic because the fall always lands at the first traps after an idle, and the result's
  first trap is the `\r`.
- **Not new to B57.** Any kernel line with nobody holding (a kill line, ptable.rs:189) lands in a
  session's open line the same way, and the shell's prompt is open the whole time it waits for
  input. B57's fall line is the first kernel line that fires reliably while a line is open.
  consoled.md "Started by `init`" promises "a line a program writes cannot come out bare": the code
  departs from it whenever the kernel writes between two writes of an open line. devices.md "The
  console's one writer" states the hold's scope as the write, so the kernel's side keeps its rule.

### Proposed fix (CONW2, a separate Tier A package B57 needs)

One bit each way on `console_hold`, no change to the kernel's "never waits for user mode":
- **Release says whether the writer's line is open** (`hold` = 3, release-open). The kernel keeps
  `line_open` in `Hold`; a free kernel line while it is set goes out after `\r\n` (as `died` does),
  and `Hold` notes `kernel_wrote`.
- **Take answers whether the kernel wrote since** (a `Return::Held { wrote }` or a second success
  code). consoled then sets `lines.open = None`, so its next bytes get the prefix again.
  Bytes on the console become `[con ID] \r` / `\r\n` / `mem: ..` / `[con ID] :timed`.
- Touches redoubt-sys (wire + round-trip tests), kernel redoubt.rs/console.rs, conhold (+ host
  tests), rt handle.rs `console_held`, consoled `Lines` (+ host tests), the model's call, devices.md
  and consoled.md. B57's own reader (idle.rs counts only bare `mem:` lines) also relies on the
  kernel's lines being bare; today launch-idle is safe only because its session is held silent.
- **Case** `console-kernel-line-mid-line`: a program on a minted console writes `half` with no
  newline, ends a child (kill line, ptable.rs:189), then writes `rest\n`. Expected, anchored `^`:
  `\[con ID\] half$`, `^\[!\] Terminating process with PID`, `^\[con ID\] rest$`; forbid `^rest$`
  and `^\[con ID\] half\[!\]`. The bytes are the kernel's and consoled's (rule F). Host tests: conhold
  ends an open line before a free line; `Lines` re-prefixes after a take that says the kernel wrote.

### Rejected

- consoled holding across an open line (no ABI change): the prompt is open while waiting for input,
  so kill lines would wait for a human and spill at 4 KiB inside the prompt; against devices.md.
- `mem:` lines deferred to the next release: at rest nothing releases; launch-idle never opens.
- The shell writing its `\r` with its text (Tier B, shell branch): unsplits these three cases only,
  not the rule, and is another package's files. Not a B57 change.
- B57's code: nothing in it is wrong for this; no B57 edit proposed unless ruled otherwise.

### Ruling (kernel-red-2, thread B57-split-line, event 36d485a3669c01cc916f5ce26df9c9b1)

Mechanism confirmed; the one-bit-each-way change is binding for CONW2 (Tier A, separate package)
under five conditions in the ruling: state in `Output` under `PRINTER`; a free line with the line
open writes `\r\n` first, held lines and the 4 KiB bound unchanged; consoled only re-prefixes after
a Take that says the kernel wrote; proof = the mid-line case at 2 harts on both widths, the three
beamlet cases green on CONW2's head, conhold and redoubt-sys host tests, the model's rule; pages
devices.md and consoled.md. B57 needs CONW2, B57's code and docs unchanged, its three cases rerun on
CONW2's head before it rejoins a train.

## Round 4 (2026-10-15, b57-implementer-3): rebased onto CONW2

First onto CONW2 a40d75bd9 (`git rebase --onto a40d75bd9 db6575a3f wp-B57`, head 24b0d7dfe,
ceiling 10676; idle host tests 10, size-budget, docs, fmt green; launch-idle rv64 PASS 66.5 s,
rv32 PASS 66.4 s), then, CONW2 having moved to today's main 92b84a5b8 (its head d9123b273,
kernel ceiling 10763 = the gate's count there), `git rebase --onto d9123b273 a40d75bd9 wp-B57`:
the two commits reapplied unchanged but for tests/size-budget.toml's kernel ceiling in the kernel
commit, resolved to the sum **10781** (10763 + this package's 18; the commit's
`Size budget: kernel:` line stays true). Head **26e47f1d3** (72061b949 kernel, 26e47f1d3
testbench/tests/docs) on CONW2 d9123b273 on main 92b84a5b8.

Gates at 26e47f1d3 (through q):
- `cargo test -p testbench idle`: 10 passed (the window's host tests).
- `cargo testbench --exact --arch rv64 size-budget`: PASS (kernel 10781).
- `make -f scripts/jobs.mk docs`: PASS. `rustfmt --check` on the two commits' .rs files: clean.
- `make -f scripts/jobs.mk prebuilt` then `set CASES=launch-idle`: see below.
- The three beamlet cases that failed train 22 were run on the identical tree (CONW2 a40d75bd9 +
  these two commits cherry-picked, same ceiling 10676) in `$REDOUBT_TMP/CONW2/b57`, alone through
  q, and not rerun here: beamlet-reduction-rate rv64 PASS 43.9 s, rv32 PASS 43.0 s
  (b57/target/testbench/run-2947021-1792025857332465568, run-2947022-1792025857335197626);
  beamlet-schedulers-mttcg rv64 PASS 21.1 s, rv32 PASS 20.3 s; beamlet-schedulers-one-hart-mttcg
  rv64 PASS 22.6 s, rv32 PASS 22.2 s; launch-idle there rv64 PASS 66.4 s, rv32 PASS 66.3 s (all
  rc=0; the run directories under b57/target/testbench/). The fall line now stands alone:
  `[con 6e787df40e9b7ce5] ` / `mem: frames in flight 0, at most 24439` /
  `[con 6e787df40e9b7ce5] :timed` (rv64; rv32 24701).
- launch-idle at 26e47f1d3 (prebuilt rc=0): rv64 PASS 66.4 s rc=0, rv32 PASS 66.3 s rc=0.
  Drain lines, bare as before: rv64 `mem: frames in flight 0, at most 35591` then
  `[idle] drain: back to 0 frames in flight 0.32 s after the step began`, window
  `[idle] 60.0 s: QEMU 0.003 host cores`; rv32 `at most 36185`, `0.38 s`, `0.003 host cores`.

Both heads for train 24: CONW2 d9123b273 on main 92b84a5b8, then B57 26e47f1d3 on it.
