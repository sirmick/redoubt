# WAKE1 phase 2 report (wake1-implementer-2)

Branch `wp-WAKE1`, worktree `/home/mcloonan/redoubt/.worktrees/WAKE1`, on origin/main 24507bec1.
Head `5ca6ccd96`, three commits on 24507bec1 (content identical to the gated WIP head becf39697
but for the sixteen-lease bound 5 s -> 6 s and the pages' matching figures, see Gates):
- `c714d8303` kernel, testbench: the `#` and `~` block-trace records, the oracle admitting them,
  `Size budget: kernel: 10820 -> 10848` (27 test-only code lines under `block-trace`).
- `2ea86b1c6` beamlet: the forwards queue (serve.rs, system.rs, lib.rs), the three host tests,
  beamlet.md.
- `5ca6ccd96` testbench, steward, scheduling: the burst cases' bounds and lines, steward.md,
  scheduling.md, steward-latency.toml's comment.
Tier A (beamlet's hub), reviewer kernel-red-4. Design: the Architect's ruling "WAKE1 cause and fix"
(wave-2026-10-18-rulings.md) and the QA thread STEWARD8-bound's last answer; nothing reopened.

## What was built

**beamlet's serve thread never waits on the VM** (`userland/otp/redoubt/src/serve.rs`). Each
call or send it takes is packed into a `Forwarded` (thread index, the call's number or none, the
caller's badge, account and labels, words, handle numbers, a copy of the lent bytes) and pushed on
the VM's forwards queue (`Forwards`: a `beamlet_vm::sync::Lock<Queue>` shared by `Served` and its
threads through an `Arc`), then the thread is back at its `receive`. No pages are transferred to
the VM any more: the old request header (`pack`/`words_of` in the request direction) is gone; the
reply direction keeps its header.

- **Wake protocol (no lost wake-up):** the VM, in `Redoubt::idle`, says `idle = true` under the
  queue's own lock *before* its last look at the queue (`sys.collect()` drains the queue into the
  events; if any, it clears the flag and returns), then enters its blocking `io.wait` and clears
  the flag after. A thread pushing sees the flag under the same lock: either its push preceded
  the VM's look (found), or it saw `idle` (and wakes). The wake is `wake.send([WAKE,0,0,0], None,
  WAKE_US = 1 ms)` repeated only while the flag still says idle; a VM in its receive takes it in
  the sender's own kernel entry (kernel `send` pumps before it settles), a VM that woke on its own
  clears the flag and takes the queue at its poll. A running VM gets no send at all: its poll
  (`take_completed` -> `dispatch` -> `sys.collect()` -> `served.take`) finds the queue.
- **Bound:** `MAX_FORWARDS = MAX_SERVED (2) x (PARKED 32 + SENDS 32) = 128`, where `PARKED` =
  the endpoint admission's `buckets x in_flight` (8 x 4: a call is forwarded once and can only be
  queued while parked) and `SENDS = 32` is the one-way sends the steward can send a session before
  a poll (16 `lease_ended` under the latency manifest, one `server_restarted` per binding slot,
  `approval_waiting`, room over). A full queue: the thread sends a `WAKE` with `FOREVER` (the
  VM's next poll takes it and drains the queue) and retries the push: "blocks the forward as
  today", the stated residual.
- `Sys::deliver` consumes a serve thread's wake (`Served::woken`) without an event; `Sys::collect`
  drains the queue; `Sys::idle(bool)` is the flag; `lib.rs` `idle` orders flag/look/wait.
- No kernel, steward, shell or VM-cadence change. The shell's listener is main's.

**Host tests** (`userland/otp/redoubt/tests/system.rs`, fake kernel, held clock), 30 pass:
- `sixteen_sends_are_taken_at_once_and_arrive_at_one_poll`: sixteen one-way sends with the VM not
  polling all return (asserted finished after 300 ms of host time; on main the second send waits
  for the VM's take for good), then one idle takes all sixteen in order, `request` none.
- `a_send_to_a_served_endpoint_wakes_an_idle_vm`: the VM in its idle receive (10 s deadline on
  the held clock) is returned by a send 300 ms later.
- `a_full_forwards_queue_holds_the_next_send_until_the_vms_poll`: `MAX_FORWARDS + 2` sends; the
  sender is still blocked after 300 ms (the thread holds one for want of room and the next waits
  for the thread); polls take the whole queue, then the rest, in order.
The pre-existing `a_request_already_waiting_ends_the_idle_that_takes_it` and the rest still pass.

**Instrumentation kept** (test-only, `block-trace` feature, no judged build): `#` budget_create
(id, parent, pid) and `~` a wait's end (budget, wait kind, timed out, answered without blocking,
frame): kernel `budget.rs`, `message.rs` `wake`, `sched.rs` trace; the oracle's parser admits
`@~#` and judges nothing by them; documented in scheduling.md's diagnostic-features paragraph.
The `~` record was dropped and then restored: without it a block's length cannot be read, and
the after-measurement below needed it. The shell stamps are gone (notices.ex = main), the
scratch case `tests/deadline-burst-diag.toml` is gone; `analyze.py`, `chain.py`, `tab.py` and
the logs stay in `.worktrees/WAKE1/.tmp/WAKE1/` (`diag4/` before, `after-1/`, `after-2/`).

## Measurement (seed 3, before = STEWARD12/SHELLN1/phase-1 figures at the same seed)

Per lease, the probe's `deadline_notice` = `lease_ended` send returned - that lease's own
deadline (the leases' deadlines spread by up to ~0.3 s, set at each launch's carve), gross; net
is the oracle's net of the checked build's audits. `.tmp/WAKE1/after-1/`.

| boot | first notice gross/net | last notice gross/net | send-return gaps ms | `[lease ended]` lines |
| --- | --- | --- | --- | --- |
| D=6 rv64 1 hart | 542 / 22 ms | 1,113 / 166 ms (was 1,526 / 338) | 67 68 37 44 51 (was 38-208) | 6 of 6 |
| D=6 rv64 2 harts | 795 / 52 | 1,277 / 308 (was 1,794 / 557) | 112 120 108 116 73 (was 193-376) | 6 of 6 |
| D=6 rv32 1 hart | 622 / 27 | 1,213 / 167 (was 1,674 / 342) | 86 64 42 45 48 (was 86-234) | 6 of 6 |
| D=16 rv64 1 hart | 843 / 93 | 1,826 / 448 (was 2,106 / 549) | 38-103 (was 59-183) | 16 of 16 (was 15) |
| D=16 rv64 2 harts | 845 / 103 | 2,437 / 918 (was 2,762 / 997) | 82-131 (was 86-170) | 16 of 16 |

- **The steward's send is taken at once whatever the VM does.** The listener's receive stamps
  (the stamped build, after-1) lag the send returns by 30-80 ms for the first notice and then by
  the listener's own print pace (each `recv` 250-370 ms after the previous, the inline print
  through the relay), while the sends keep returning 37-131 ms apart: the sends are not gated by
  the VM's Erlang side. On the block trace (after-2, rv64 one hart, `~` records): the serve
  thread's one wake to the idle VM at the first notice is answered in the sender's own entry
  (+403 ms); in the whole burst window **no send of the VM's process on its wake endpoint
  (frame 31380) blocks for the notices** (the 6 blocked sends there, 10-135 ms, are the hub
  waiters' hand-overs of the shell's console-write completions, each ending at a poll that found
  a message: +965, 1089, 1340, 1649, 2003, 2339 ms, the print's completions); before, each
  forward blocked 107 ms median there. No `~` on that frame timed out: the 1 ms wake step never
  expired. **No drop at NOTICE_US** in five boots (before: one of six once in five, one of sixteen).
- **The remaining serial term is the steward's own work per exit**, not the hub: the gaps between
  send returns (37-68 ms rv64/1, 42-86 rv32/1, 73-131 at two harts) match the trace's occupancy
  per gap (the steward's budget 86 runs 18 ms of a 41 ms gap, the checked kernel's audits 58-61 %).
  STEWARD12's open note predicted it ("with fast sends the per-exit console lines, ~30 ms each,
  would be the next serial cost"). Not changed (no steward change in scope); stated on the pages
  as the measured cost; the probe's own lines are part of it (a bench condition).
- **"All N taken at one poll, prints follow" on main's unsplit listener:** the host test shows the
  mechanism (16 at one poll); on the box the listener prints inline, so its receive stamps come
  at the print's pace (250-370 ms each) and the last `[lease ended]` line is drawn 2.3-2.6 s (six)
  and 6.1-6.6 s (sixteen) after the deadline. The hub is no longer the gate; the shell's print
  is the user-visible term. SHELLN1 stays closed as ruled; the pages say the print is the shell's.
- **An idle VM is unchanged:** `File.read` from the idle shell 0.99-1.33 s (before 0.96-1.31),
  beside a spinning process 1.37-1.56 (before 1.37-1.53), after the burst 0.99-1.24.
- **Derived bounds:** worst per-lease gross over the two after-fix runs of each gated boot (after-1
  and gate-2): six 1.277 s -> 3 s; sixteen 2.511 s (gate-2, two harts) -> 6 s (after-1's 2.437
  alone gave 5 s; the second run moved it); asserted per `LATENCY-SAMPLE` line by digits
  (`[0-9]{1,6}|[12][0-9]{6}` and `|[1-5][0-9]{6}`), plus six / sixteen `[lease alice/agent-N
  ended]` session expects before the burst's result. Gate-2 (gated widths and hart counts): six
  first 0.55/0.78/0.61 s, worst 1.19/1.27/1.24 s (rv64 1, rv64 2, rv32 1), 6 lines each;
  sixteen first 0.59/0.91 s, worst 1.85/2.51 s, 16 lines each.

## Pages (the documentation check)

- `docs/userland/beamlet.md`: `serve(Endpoint)` bullet (the queue); new `#### The forwards
  queue` under "Asynchronous underneath, synchronous on top" (the protocol, the bound and its
  arithmetic, the full-queue residual, the history and the figures, the bench condition, the
  waiters' hand-over residual, the shell's print); that section's status 9 -> 12 with the three
  host tests.
- `docs/servers/steward.md`: "Leases" bullet (taken at once, queued, none missed while the serve
  thread lives); "Failure and restart" burst bullet rewritten with the measured cause, fix and
  figures; "Residual risks" bullet rewritten (steward's pace, shell's print, the two loss paths).
- `docs/kernel/scheduling.md`: the burst passage under "With the real steward" (figures, the
  history, the shell's print); the diagnostic-features paragraph documents `~` and `#`.
- `tests/steward-latency.toml` comment "Why one at a time": figures and cause.
- Checked, unchanged: `GETTING-STARTED.md:134` and `docs/userland/agents.md:86` (the notice exists
  and is printed, no latency claim); `docs/testbench.md:810` (`deadline_notice_p99_us` is the
  latency cases' bound); `docs/SECURITY.md` (no rule's tests changed); README.md,
  `docs/plan/m3-agents.md` (no claim on notice latency). No dates or package ids added.

## Gates (every command through `scripts/jobs.mk` under q; logs `.worktrees/WAKE1/.tmp/WAKE1/gate-2/`)

- gate-2 on becf39697 (the head's code; the final commits differ in text and the -16 bound only,
  see below): `make -f scripts/jobs.mk -k -j8 set CASES="formatting docs size-budget unsafe-budget
  <18 beamlet-*> <40 steward-*> steward-deadline-burst steward-deadline-burst-16 steward-latency
  steward-latency-n4 steward-latency-n16"` (65 cases): **128 PASS, 125 targets rc=0, one FAIL:
  `size-budget`**, which read the WIP commit that raised the kernel's ceiling (10820 -> 10848, the
  27 code lines of the `~` and `#` records) and found no `Size budget: kernel:` line; the rebuilt
  commit carries it and the case is rerun on the final head (below). formatting rc=0 (after a
  `cargo +nightly fmt` of serve.rs and system.rs the first launch caught), docs rc=0, unsafe-budget
  rc=0; the beamlet set and the steward set both widths all rc=0; steward-deadline-burst rv64 1+2
  harts and rv32 1 hart PASS with the new bound and the six lines; steward-deadline-burst-16 rv64
  1+2 harts PASS (worst 2.51 s against the then 5 s); the three latency cases PASS as gated
  (`deadline_notice` p99 15.9-32.4 ms against 40 ms; steward-latency rv64 2 harts 700 s).
- Host: `cargo test -p beamlet-redoubt --features fake --test system`: 30 passed (three new).
  `cargo check -p beamlet-redoubt --target riscv64gc-unknown-none-elf`: clean (one unused import
  removed). `cargo check --release -p beamlet-redoubt` for riscv64gc and riscv32imac: rc=0
  (`gate-2/release-checks.log`).
- `./test-shell` (through `q run --cores 4`): native ok, on_beamlet ok, on_beam ok, entry_point
  ok, terminal ok; **formatting FAILED** on `userland/shell/lib/redoubt/agents/echo.ex` (`mix
  format --check-formatted`: its `@loads` list), a file this branch does not touch and main's own
  (849e751b3): a finding for the orchestrator, not fixed here (no shell change in scope);
  **on_fake_kernel FAILED** once: in `cargo test -q -p beamlet-redoubt --features fake` the
  console test binary's running tests (`idling_with_a_deadline_returns_by_it`,
  `after_the_console_ends_idling_still_waits_for_its_deadline`,
  `an_end_of_input_already_waiting_ends_the_idle_that_takes_it`,
  `a_write_answered_busy_goes_again_after_the_retry_interval`; the console tests serialize on a
  `turn()` mutex, so three of the four were waiting for it) ran past 60 s and the fake called the
  binary stuck ("a wait only a change can end saw none in the fake kernel for 60s", the held
  clock at 10.14 s). **It is main's own flake, not this branch's:** main's console binary, built
  and run in the shared checkout on 24507bec1 (`q run --cores 4 -- cargo test -q -p
  beamlet-redoubt --features fake --test console`), hung the same way in 1 of 10 runs
  (`.tmp/WAKE1/main-console-rep2.log`: the same message, the held clock at 10.16 s,
  `a_vm_without_a_console_runs_its_module_to_exit` among the four), while the branch's binary
  passed 15 of 15 runs alone and hung in 2 of 7 whole-command runs (`host-fake-stage-rep5.log`
  and test-shell). The hang is the console tests' held-clock fixture under parallel test threads
  (whichever test holds the turn waits on a tick or a change that never comes); a finding for the
  orchestrator (the fixture or the console tests, a beamlet host-test node), not fixed here. Three
  of my whole-command runs also failed tcp.rs for want of `BEAMLET_TOOLCHAINS` in that shell
  (`host-fake-stage-rep1..3.log`), an environment slip, passing with it. The bench's
  `beamlet-lookup-host` (the same packages' tests) passed in gate-2.
- gate-3 on the final head 5ca6ccd96 (`.tmp/WAKE1/gate-3/`): `set CASES="size-budget docs
  formatting unsafe-budget steward-deadline-burst-16"`: all rc=0 (size-budget with the
  `Size budget: kernel:` line; docs with the 6 s text; steward-deadline-burst-16 rv64 one and two
  harts PASS under the 6 s bound, worst 1.83 / 2.51 s, 16 lines each). The code is identical to
  gate-2's head (only the -16 toml's bound and the pages' text changed after it), so gate-2's
  beamlet and steward sets, burst-6, and latency results stand for this head.
- Repetitions for the console-hang finding (background, results in `.tmp/WAKE1/*fake-stage-rep*`,
  `main-*`): at the time of this report the branch's whole command passed 3 of 3 further runs and
  main's 4 of 4; the console binary alone hung on main once in ten.
- Not run: the whole bench (the train's).

## Findings beyond the node

1. The burst's remaining serial cost is the steward's loop per exit (37-131 ms, console lines
   and the probe's lines included), ~N x that after the kernel's N destructions: a steward node
   if the owner wants the burst told faster; at the image's 3-5 leases it is 0.1-0.6 s.
2. The sponsor's visible term is the shell's inline print, 250-370 ms a line through the relay
   (the last of sixteen drawn 6.1-6.6 s after the deadline): the SHELLN1 split would now move the
   listener's receive but not the drawn lines; a cheaper print path is a shell/relay question.
3. The hub waiters' wake-up sends to a busy VM still wait for its turn (10-135 ms on the trace,
   one send a batch): the same shape as the forward for completions, stated as a residual on
   beamlet.md, not changed (the ruling named the serve thread's forward).
4. The leases' deadlines in the burst spread by up to ~0.3 s (set at each launch's carve, not at
   the shell's request); the per-lease gross is what the probe prints and the cases bound.
