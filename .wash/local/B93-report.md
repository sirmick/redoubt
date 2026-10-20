# B93: the footprint case's `keep` agent spins, never sleeps

Branch wp-B93, head f62382fb0 (one commit on origin/main dc75a30eb). Worktree
/home/mcloonan/redoubt/.worktrees/B93; scratch and every kept log under /home/mcloonan/redoubt/.tmp/B93.

## What changed

- `userland/shell/lib/redoubt/agents/echo.ex`: `run("keep " <> text)` was `finish(text)` then
  `Process.sleep(:infinity)`; it is now `wait(text)`, the live paths' way (the result written and
  read back, then the busy recursion `spin/0`). The module's doc comment for `keep` and the path's
  comment say why in the page's words.
- `tests/beamlet-agent-footprint.toml`: the description says the agent idles spinning, as a
  lease's agent that must stay alive does; the comment on the `agent: result written` expect says
  why (the VM's exit on nothing runnable, the bench's grace under the forbids, what a sleeping
  agent did inside it). No deadline, bound or expect changed.
- `docs/userland/agents.md`, "Leases": a new bullet states the rule the code already followed and
  the page had not stated at all (the brief's "idle-lease rule" was not on the page): an agent that
  must stay alive keeps a process runnable; a VM with nothing runnable, no timer pending and nothing
  it waits on exits `beamlet: Deadlock`, not 0, and the steward reads a lease VM's exit as its end
  (`exited CODE`); so such an agent idles by a busy recursion, never a receive with no timeout and
  no sender, `Process.sleep(:infinity)` included; the echo agent's live paths and the scripted
  agents that outlive their work do so, and one whose work is done returns, that exit its lease's
  end on purpose (`NoSink`).

No kernel, steward, hub or bench change.

## The mechanism

Why `sleep(:infinity)` with no other process lets the VM exit: `Process.sleep(:infinity)` is
`receive after :infinity`, a receive that arms no timer. The VM's scheduler, in `next()`
(userland/otp/vm/src/vm.rs:1259-1292), with the run queue empty and nothing running, nothing in
`timers`, no console reader (the agent never reads the console), no program ports, no system or io
waits (the `finish` write and read-back completed) and no pending exits, sets `stuck` and `run`
returns `RunError::Deadlock`; `redoubt::run` (userland/otp/redoubt/src/lib.rs:798-803) says
`beamlet: Deadlock` and exits `STUCK`, 1 in a default build. `init` then prints
`init: beamlet (PID n) exited, code 1`, which matches the case's forbid
`^init: .* (exited|faulted|was killed)`. So the `keep` agent's VM exited on every run, within a
few guest milliseconds of `agent: result written`.

Why only sometimes: the verdict's expects end at `agent: result written`; after the last expect the
bench reads the console under the forbids for `GRACE`, 50 ms of host time
(tools/testbench/src/qemu.rs:30, 912-936), and only then stops the guest over QMP for the scan.
The case fails exactly when `init`'s exit line reaches the bench inside that grace. The guest's path
from the agent's line to `init`'s (the VM's teardown, its console flush and connection ends, the
process exit, `init`'s notice) is longer than 50 ms of host time when the guest and the bench have
the lease's one core to themselves, so alone the stop lands first and the case passes; under load
the guest's threads and the bench's share that core with more contention and the bench's reading
of the agent's line lags the guest, so the exit line is already in the console buffer, or arrives,
inside the grace. Under `icount` with `sleep=off` the guest's own timed waits cost no host time, so
nothing in the guest slows the exit path under load; the slack is the host's. With a runnable
process the VM never reaches the stuck check: no exit, no line, no race.

## Counts, with the log paths

Every run through `make -f scripts/jobs.mk -C <worktree> <w>/beamlet-agent-footprint` from the
worktree's prebuilt, both widths at once, beside a repeating `set` of eight boot cases
(beamlet-footprint consrelay-footprint init-boot userland-boot beamlet-boot beamlet-console
beamlet-files beamlet-natives; 16 jobs a pass) as the load; the queue also held other members'
jobs. Driver: `.tmp/B93/repro.sh`; each run's bench log is `<tag>/<w>-NN.log`, a failing run's
kept console log `<tag>/<w>-NN.smp1.log`, the results in `<tag>/runs.txt`.

- **Before** (unchanged tree, prebuilt from dc75a30eb): rv64 **18/20**, rv32 **19/20**
  (`.tmp/B93/before/`). The three failures: rv64 runs 2 and 6, rv32 run 2, each
  `forbidden output /^init: .* (exited|faulted|was killed)/: init: beamlet (PID n) exited, code 1`,
  and each kept console log has `agent: result written`, then `beamlet: Deadlock`, then `init`'s
  line (`before/rv64-02.smp1.log`, `before/rv64-06.smp1.log`, `before/rv32-02.smp1.log`, lines
  50-54). The load loop ran 10 passes, 160 case runs, all rc=0. The failures fell in the first
  passes, while the queue was deepest (train-20's bench had just released the qemu lock).
- **WRITE1's original failure**, copied aside before anything reran: `.tmp/B93/prior/` (the kept
  console log shows the same three lines at 50-54; the bench log there is the passing rerun's,
  the failing verdict line having been overwritten).
- **After** (f62382fb0, prebuilt rebuilt): rv64 **20/20**, rv32 **20/20** (`.tmp/B93/after/`),
  no kept console log. The load loop ran 14 passes.
- The scan's measurement is unchanged by the spin: `heap beamlet` peaks 2104-2105 pages on rv64 and
  2012 on rv32 in every run before and after (image/README.md's 1,993 and 2,083 were
  measured earlier and stand well under `sizes.agent`'s cap; not this package's).

## Gates, with rc

- `make -f scripts/jobs.mk set CASES='formatting docs'`: rc=0 (`.tmp/B93/gate-formatting-docs.log`;
  rv64/formatting PASS 22.7 s, rv64/docs PASS 4.6 s).
- `make -f scripts/jobs.mk prebuilt` on the changed tree: rc=0 (`.tmp/B93/prebuilt-after.log`).
- `make -f scripts/jobs.mk set CASES='beamlet-agent-footprint'`: rc=0, rv64 PASS 4.4 s, rv32
  PASS 4.4 s (`.tmp/B93/gate-footprint.log`, `gate-rv64-...log`, `gate-rv32-...log`).
- The 40 after-runs above.

## Summaries checked

- README.md, GETTING-STARTED.md, docs/README.md: name M3 and the agents page, no claim about the
  echo agent or the case; no change.
- image/README.md (`sizes.agent` from the case's peak "once the agent has read its input and
  written its result"): still true, the idle's form does not enter the peak; no change.
- docs/testbench.md "Memory" (names beamlet-footprint and consrelay-footprint, not this case) and
  docs/plan/m3-agents.md (no mention): no change.
- docs/servers/steward.md "Leases" (the `exited CODE` line the new bullet cites) and
  docs/userland/beamlet.md "beamlet on Redoubt" (a lease VM's exit code as its diagnostic, cited):
  unchanged, consistent.
- docs/userland/agents.md "Leases": the bullet added; its status line unchanged (the case is a
  footprint case, named on the beamlet page's status, not a lease case).

## Finding beside the work, not this package's

In one load pass of the after-run (`.tmp/B93/after/load-1792496594.log`), `beamlet-files` failed on
both widths at once with `forbidden output /EXCEPTION/: {'EXCEPTION',error,{badmatch,{error,eio}}}`,
after `files: wrote: {error,eio}` in the VM `init` restarted after the expected exit (the replay
the case's own comment describes), 15.2 s and 15.3 s in: a write to `walfsd:data` answered `eio`
under a 16-job load. Kept console logs: `.tmp/B93/side/rv64-beamlet-files.smp1.log`,
`.tmp/B93/side/rv32-beamlet-files.smp1.log`. The case does not run the echo agent and nothing in
this package touches its path; the 13 other passes of the same set passed it. Not investigated
further.

## Commands

All through `scripts/q run` via `make -f scripts/jobs.mk`, with the env the brief names. No push.
