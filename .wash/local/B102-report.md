# B102 report: steward-context-login's console `ended` send, and the 1,500 s hang

Branch wp-B102, base 820d5e5ff, head 25ba031c7 (two commits). Shape and ruling: thread B102-shape;
mechanism detail in .wash/local/B102-shape.md.

## Mechanism (as read, ruled by the Architect)

- The case's step sent `consol`'s `ended` once through beamlet `send/2`, which waits
  SEND_TIMEOUT_US = 1 ms for its receiver (userland/otp/redoubt/src/system.rs:43, 451-463;
  beamlet.md states the bound). On a context the receiver is consrelay's serving thread (not
  sshd's slot, as B95 read it), which takes a send only in `receive`, between its turns and the
  calls it serves (consrelay.rs:551-575), and drops it (consrelay.rs:473). 1 ms is the kernel's
  SLICE_US (kernel/src/sched.rs:72): one slice to another budget or one host deschedule of QEMU
  times the send out. Only this case and two host tests call `:redoubt.send`.
- The hang: the `{:ended, :ok}` expect had no `within`, and nothing forbade the error, so it ran to
  1,500 s. The report then named bob.work+x, whose mark wait timed out first and aborted the
  others, while the stuck bob.work-again stopped as Aborted, which is not reported.

## Delivered

1. 217610e8c `testbench: a session stuck at an expect is the one a failure names`
   - tools/testbench/src/ssh.rs: `Stop::Waited` for a mark wait's timeout. It fails the case but
     does not call `shared.fail()`. `report` names the first session that failed of its own, and a
     mark's timeout only when none did. `run`'s concurrent drive and its verdict are factored into
     `drive_all` and `report`. `Stop::at` prefixes the step.
   - Host test `a_stuck_expect_is_named_before_a_mark_wait`: two sessions, the waiter first in the
     case's order. The waiter ends Waited, not aborted, and the failure names
     "session stuck: step 1: timed out waiting for /ok/"; with only a mark timeout, that is
     named. On the old code the waiter's Failed aborts the stuck session, so the first assert fails.
   - docs/testbench.md "Sessions and the loopback server": the sentence on how a multi-session
     failure is reported; the test is added to the status list (20 -> 21).
2. 25ba031c7 `tests: steward-context-login retries the console's ended until it is taken`
   - The step retries `send/2` for 2 s of tries (2,000 slices), with 10 ms sleeps, and prints
     `{:ended, {:ok, N}}`, N the tries. Any other answer stops it, and so does a timeout past 2 s.
     The session forbids `\{:ended, \{:error`, and a `{ within = 60 }` bounds the expect. The
     comment says why, and that this case is the only caller of `:redoubt.send`.
   - beamlet's send/2 contract and SEND_TIMEOUT_US are unchanged, as ruled.

## Gates as run (exit codes)

- `q run --cores 4 -- cargo test --manifest-path tools/testbench/Cargo.toml --bin testbench ssh::`:
  9 passed, exit 0. One dead_code warning (`Window.harts`) is on main already, not in the diff.
- `rustfmt +nightly --edition 2024 --check tools/testbench/src/ssh.rs`: clean.
- `make -f scripts/jobs.mk prebuilt`: exit 0. `set CASES="docs formatting"`: both PASS, exit 0.
- Alone, 5 rounds of `set CASES=steward-context-login` (both widths a round): rv64 5/5
  (7.2-7.5 s), rv32 5/5 (6.7-6.9 s). Tries: 1 in all 10 (.tmp/B102/alone/).
- Load run (the orchestrator's go): 10 per width, both widths at once, beside B93's 8-case set
  looping (8 passes, 128 jobs, during the run). Driver .tmp/B102/load.sh, results
  .tmp/B102/load/runs.txt. rv64 10/10 PASS (7.4-17.1 s), rv32 10/10 PASS (8.7-15.8 s).
  - Tries: 1 in each of the 17 runs whose ssh log was kept (rv64 8, rv32 9). The bench had already
    pruned the run directories of rv64 runs 5 and 8 and rv32 run 7 before the copy. Those passed,
    so their count was at most the 2 s bound, but it is not known.
  - Max 1, nowhere near half the bound. The race did not fire under this load. Train 89's was a
    whole bench, heavier than this set, and this run does not reproduce that load.
  - Not run: the full bench, rv32 builds of the kernel/servers (no target code changed: the diff
    is host bench code, a case file and a page).

## Summaries checked

- docs/testbench.md (updated, above).
- docs/userland/beamlet.md, the send/2 bound and its table row: unchanged and still true.
- docs/servers/steward.md, sshd.md, docs/userland/sessions.md: they cite the case in status lines
  but do not describe the step or the 1 ms send. No change.
- docs/servers/consrelay.md "Failure and restart": unaffected.
- README.md, GETTING-STARTED.md, tools/testbench/README.md: no claim on send/2, mark waits or how a
  failure is reported (grep). No change.

## Beside the work (not B102's)

- In the load set, beamlet-boot rv32 failed once in 8 passes: `init: rebooting: beamlet was
  restarted 5 times within 60 seconds` (.tmp/B102/load/load-1792591676.log:19). Its console was
  pruned before I could keep it. This is B95's load finding 1 symptom, now on a base with B98's
  cache=unsafe. beamlet-boot runs no ssh sessions, so neither commit is on its path.

## Tier and next

Tier A (bench). Reviewer: servers-red. The branch is clean in two logical commits, ready for review.

## Diagnostic for the Architect: beamlet-boot rv32 under load (after the report)

On 820d5e5ff (scratch worktree, since removed), `make -f scripts/jobs.mk rv32/beamlet-boot`
(--keep-failed) beside B93's 8-case set looping:
- First 20 runs: 0 failed (.tmp/B102/bb-20/). The load set passed 48/48.
- Then 250 runs: 0 failed (.tmp/B102/bb/). The load set ran 23 passes and passed 368/368.

Rate: 1 failure in 8 load passes once (2.8 s; by its time alone, not a blkd read timeout), and
0 in 270 measured runs plus 26 load-set beamlet-boot rv32 runs since. Not reproduced. Per the
Architect, it waits for the next whole bench.
