# STEWARD13: a launch does not hold the steward's loop

Node STEWARD13 (parent AGENTS). Tier A (the steward's loop and its launch). Size M. Reviewer:
steward-red-5. Needs STEWARD10 and STEWARD12 (both on main, train 79). Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "STEWARD13" and "STEWARD13 design"; the rules it builds
on: STEWARD5 (`STEWARD5-implementer.md`: the single-arbiter thread pattern, no lock across a
server call), STEWARD10 (`STEWARD10-implementer.md`: the deadline as identity witness),
STEWARD6 (`SERVER_US`), STEWARD8 (the launch bound), STEWARD12 (deadline_notice sampled at the
send). Evidence: STEWARD9's two-hart sequential run (scheduling.md "With the real steward", the
residual sentence this package replaces).

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. A run longer than 1.5 h detached (`setsid nohup`).
- Messages at most 2000 bytes; detail in a file. Read source before ruling on it; never a QA
  file's tail. Pages carry no dates or ids; a number on a page shows its arithmetic; a declared
  stack is twice the measured peak. A miss is a finding, not a tuning; no target moves.
- The core (`libs/steward`) moves only with its oracle: this package does not touch it. Not
  `libs/client` either (see "the lock" for why it need not), not the kernel.

## The symptom

A lease's deadline fires while the loop is inside `Machine::launch` (`bin/steward.rs:1025`): the
image streamed from `erofsd:system` into the child's pages and `process_start`, 1.7–6.6 s
measured (STEWARD8/9's `served start_agent`). The exit notice waits behind it, so the
`deadline_notice` target (40 ms, human control: the sponsor is told promptly) is unmet whenever a
launch is in flight. The latency cases meet it only because they sequence (the steward idle at
each deadline). STEWARD12 showed ordering the loop's work does not help when the loop is inside
the launch.

## The design: a launch thread, the loop prepares and finishes

1. **Prepare on the loop** (cheap, installing). The loop's `Launch` step does what `launch` does
   before `Launch::streamed`: resolves the held connections and the binding, creates the exit
   endpoint and, for a session, the notices endpoint and its send right. Every handle the child
   is given exists now. It then hands the thread a self-contained job: the budget handle, the
   exit endpoint, the handles and namespace by name, the args, the pages, the module, the
   `erofsd:system` connection to read from and the thread's own lend page. Nothing of
   `Machine`'s tables is touched by the thread.
2. **Launch on the thread.** One launch thread, started with the steward as the cleanup thread
   is, idling in a call on the loop's endpoint and woken by reply (STEWARD5's hand-over: the
   loop is the single arbiter of its wake, no send). It runs `Launch::streamed(...).start()`
   exactly as today, then reports (the `Job`'s process and exit, or the error) by one send on
   the loop's endpoint and re-parks. Launches are **serial, FIFO**: a second `start_agent`'s
   `Launch` step queues behind the first. A caller's wait is unchanged by that (today the second
   call is not even received until the first launch returns), so STEWARD8's bound derivation
   stands; `served start_agent` now includes a launch's wait in the queue — say so where the
   line is defined.
3. **Finish on the loop.** On the report the loop does the rest of today's `launch`: closes the
   process handle and the steward's copies, `watch_exit`, `keep`s the notices send right,
   records `made`/`pids`, then runs the batch's remaining steps (check whether `Launch` is the
   last step of every batch that has it; if so there are none) and only then pushes the batch's
   `Done { object, result }` to the core (`drive.rs:147`). The core sees exactly today's events
   in today's order for that object. On an error from the thread the step fails as today
   (`StepFailed`, the core's rollback, `failed` to the caller) — including a lease whose own
   deadline fires during its launch (the kernel destroys the budget, the thread's `map` or
   `start` is refused): B82/STEWARD9's existing path, no new behaviour.
4. **The pending batch.** `Steward::run` returns a pending outcome at a `Launch` step; the loop
   holds the batch, what was produced so far and the reply, keyed by the job. While a launch is
   pending the loop serves every other object's events: deadline exits and their notices, timer
   wakes, calls, the cleanup thread's fates. **Events of the launching object itself** (its
   `end_lease`, an `Exited` for it, its `rebind`) are **held in arrival order and replayed after
   the report** — unless you show from the core's oracle (`libs/steward/src/machines.rs`, the
   result's `Option`) that the core accepts them between a batch and its `Done`; the hold is the
   default because it needs no core change and matches today's order (today they wait behind the
   launch too). The one `start_agent` whose launch is in flight is thereby the one call the
   steward holds a reply for; protocol.rs's "answers every call as it arrives and parks none"
   (R26's `ahead`) is re-stated: `end_lease` of **another** lease is still answered ahead and at
   once; the launching object's own is held for at most the launch bound. State both on the page.
5. **The lock: STEWARD10's premise re-shown.** The guard (`drive.rs:446`) rests on "no handle is
   installed in the steward's table between the `now()` read and the destroy". The thread
   installs handles: `Process::create`, one `Buffer::new` per page batch, consumed by `map`
   (`libs/client/src/launch.rs:178–300`). Rule: a `redoubt_rt::sync::Mutex` (the install lock)
   is held by the loop across its `now()`+`destroy` pair (two kernel calls, µs) and by the thread
   across `start()` **except inside the read closure**, which is the steward's own code: it drops
   the guard on entry to `read_program` and re-takes it on return. So no install interleaves
   with the pair, and the lock is never held across a server call (STEWARD5's rule). The loop's
   worst wait for the lock is one between-reads stretch (a `Buffer::new`, a fill, a `map` of
   `PLACE_PAGES` pages, or `process_start`): kernel work, measure it, state it. Defence in depth,
   stated: the thread never creates a **budget**, so the silent form (a budget at a recycled
   index) is unreachable even without the lock; the lock removes the loud `WrongObject` form.
   The page's premise sentence (steward.md:778, "no reply the steward's threads take carries a
   handle") becomes: and the launch thread installs only under the lock the destroy pair holds.
   If the closure-release cannot be made to hold (e.g. an installing call inside `place` with no
   read between), stop and raise it: a hook in `libs/client` is a design change for me, not yours.
6. **Exit notices before the watch.** The exit endpoint exists before the start; a notice pending
   on an endpoint is kept until received (ipc.md, the receive flowchart), so a child that dies
   before the loop's `watch_exit` is still reported — the same window as today, longer. Confirm
   from the kernel's page, do not assume.
7. **Cost.** One thread (stack twice the measured peak: `launch`'s stack arrays move to it), one
   lend page, one more call of the steward's in flight at `erofsd:system` (the thread's reads
   beside the loop's mints): `in_flight` in the derivation on erofsd.md/walfsd.md +1 if it counts
   the steward's calls, as STEWARD5 did; the entry by the arithmetic in the image and every
   memory-checked manifest; testbench.md's table follows.

What stays: `SERVER_US` bounds each read (a read past it fails the launch, the caller told);
the cleanup thread, the console queue and their rules; `start_agent`'s synchronous contract and
wire; the client's `launch_timeout`/`MAX_CALL_MS`.

**Residual, stated, not fixed here:** a lease batch's mints (`Connect` steps, `new_connection`
at the shared servers) still run on the loop within `SERVER_US` (1.66 s worst measured, STEWARD6),
so a deadline inside a mint still waits for it. The overlap case puts the deadline inside the
launch, the quantity this package fixes. The install lock makes moving a whole batch off the loop
possible later (a budget install under the same lock); that is a follow-up I name if the
measurement shows mints bite, not this package.

## Tier A invariants the reviewer checks

- No destroy by index can find a handle the launch thread installed: every install of the
  thread's is under the install lock, the loop's `now()`+`destroy` pair is under it, and the
  lock is dropped inside every server call (the read closure), never held across one.
- The thread touches no `Machine` table and creates no budget; all bookkeeping is the loop's.
- The core receives, per object, today's events in today's order; a `Done` is pushed once, after
  the step's bookkeeping; a held event is replayed once, in order, never lost.
- `end_lease` of another lease is answered ahead during a launch; the launching lease's own end
  is held at most the launch bound. A launch failing for any reason answers `failed` and leaves
  no half-made process (`libs/client`'s rule) and no unwatched exit endpoint.
- The loop's turn makes no call to a shared server for the launch; a server that never answers
  a read costs the loop nothing (the thread pays, within `SERVER_US`).

## Cases

- Host (`servers/steward/tests/steward.rs`, the fake kernel returning a pending launch): a
  deadline exit of another lease arriving while a launch is pending is decided and its notice
  sent before the launch's report; a timer tick the same; the launching lease's `end_lease` is
  held and served after the report (or served at once, if the core check lets you); a launch
  the thread fails (the budget gone) answers `failed` and runs the rollback; the install lock is
  not held during the read closure (the fake kernel's read hook asserts it) and is held across
  the destroy pair; a second `start_agent` during a launch queues and both are answered in
  order; STEWARD10's four guard tests unchanged.
- Bench, the acceptance: a new case (the orchestrator names it; `steward-launch-overlap` is my
  working name) on the latency manifest, both widths, smp per BENCH1 (rv64 [1, 2], rv32 [1]; a
  stated smp until BENCH1 lands): lease A started with the per-width deadline STEWARD9 derived
  (twice the worst launch, rounded up) and spinning; lease B's `start_agent` issued at A's
  deadline minus 1 s on the guest clock (the deadline is from the request, B82), so B's launch
  (≥ 1.7 s on every supported row) is in flight when A's deadline fires; a few rounds (five). The
  oracle judges A's `deadline_notice` (at the send, STEWARD12's point) against 40 ms only when
  the trace shows A's deadline inside B's `start_agent` arrival-to-answer; no overlap is a loud
  failing line, never a pass. Also recorded: timer_wake during the overlap, B's served time, the
  loop's worst wait on the install lock. **Run it first on main** so the miss is recorded (the
  notice waiting the launch's remainder), then on the branch. `steward-latency`, `-n4`, `-n16`,
  `steward-deadline-burst`, the steward set, `steward-server-restart`, `init-boot`: unchanged
  in verdict both widths.

## Pages

steward.md "Leases" (a launch runs on the launch thread; the loop serves meanwhile; the held
events; serial launches and what `served` covers), "Failure and restart" (the cleanup bullet's
premise sentence amended as in point 5), "Residual risks" (the mint residual above; the lock
wait); scheduling.md "With the real steward" (the overlap row replaces STEWARD13's residual
sentence; the status line names the case); erofsd.md/walfsd.md's `in_flight` derivation if it
moves; testbench.md's memory table if the entry moves; SECURITY.md R10/R26/R39 rows if their
tests change. Summaries checked: say which.

## Owned paths

`servers/steward/src/bin/steward.rs` (the thread, `launch` split into prepare/finish, the
wiring), `src/drive.rs` (the pending outcome, the deferred `Done`, the held events, the lock
around the destroy pair), `src/lib.rs` (the `Kernel` trait's launch outcome), `src/watchers.rs`
if the thread pool is shared, its tests; `image/manifest.json` and the memory-checked manifests'
steward entries, `tests/size-budget.toml` if moved; the new case toml; the pages above. Not
`libs/steward`, not `libs/client`, not the kernel.

## Gates

Host tests; release builds rv64 and rv32 with and without `latency-probe`; the cases above both
widths; `set CASES=formatting`, `set CASES=docs`, `size-budget`, `unsafe-budget`; the
memory-checked set if the entry moved. Report each with its exit code, the overlap case's
before/after figures with log paths, and the measured lock wait.

## Checkpoint

After the host tests of points 3–5 are green and before the bench runs, one progress note: the
pending-batch shape, what the core check (point 4) showed, the lock's measured worst hold, the
thread's measured stack. Bring to me on thread STEWARD8-bound anything that would move the design
(the core accepting events between a batch and its Done; an install the closure cannot release
around).
