# B95 report: beamlet-files' eio under load

Base 3e6f29443. Head 09e7125f6 on wp-B95 (worktree .worktrees/B95). Two commits:
0b16e9b1a (rt) and 09e7125f6 (serving residual).

**Round 4 (train 89):**
- size-budget: libs/rt counted 4118 lines against its 4110 ceiling, all from this branch.
  tests/size-budget.toml raises it to 4118 in the rt commit, which carries the line `Size
  budget: libs/rt: a session's deadline read afresh as its answer goes out, and a pending
  request's mark that the file server held it, so only a held request is judged against its
  deadline, 4110 to 4118.` Range-diff: .wash/local/B95-range-diff-r3.txt.
  - `set CASES=size-budget` on 09e7125f6: rc=0 (before the raise, rc=2: "libs/rt: 4118 lines,
    ceiling is 4110").
  - The same case on a trial merge of 3e6f29443 + wp-B94 (f43fbebb2) + wp-B95: rc=0, "libs/rt:
    4118 of 4118 lines". The trial worktree is removed.
- rv32 steward-context-login (train 89) is read below under "steward-context-login"; no code
  changed for it.

**Round 3 (servers-red BLOCK P1 and P3 on 3a227b286; range-diff .wash/local/B95-range-diff-r2.txt):**
- P1: `pump` sets the session's deadline from `time_now().unwrap_or(now)` read as the answer goes
  out, whatever the fill asked, so a session answered after another's slow fill in one `expire`
  or `wake` pass is not given a deadline already past.
- New held-clock test `a_session_answered_after_a_slow_fill_in_the_same_pass_has_its_whole_bound`:
  two sessions due at one turn; A's fill takes 2×bound, then B is answered empty; another event
  then gives the server a turn, and B's session must still be the one its answer went to.
  On 3a227b286 it fails: left `[0, 0, 1, 0]` (B's call opened a fresh session), "B's session
  ended at its own answer" (.tmp/B95/p1-fails-on-3a227b286.txt). It passes on the fix.
  mux_held.rs now runs its two tests one at a time on the binary's held clock.
- P3: serving.md "The end" says "late by the work before it in that turn, never early". The
  Multiplexed connections status lists the new test (17).
- Gates on 8e8dd63b1, all rc=0 (logs .tmp/B95/host-r3/):
  - `set CASES='rt-host-tests rt-miri formatting docs'`: PASS.
  - `prebuilt`: rv64 366 and rv32 352 cases, 0 failed. This is the consumer sweep.
  - `set CASES='client-host-tests ipd-host-tests sshd-host-tests consrelay-host-tests'`: PASS
    (the mux's consumers on the fake kernel; consumers.log).
  - `set CASES='beamlet-files aio-many-reads'`: all four PASS (beamlet-files 1.8/1.7 s,
    aio-many-reads 0.8/0.7 s; boots.log).
- Scratch cleaned after these: the .tmp/B95/inject and .tmp/B95/fixinject worktrees and the
  B95-diag ref are removed. The two diagnostic commits are kept as .tmp/B95/diag.patch and
  .tmp/B95/fixinject.patch; every log cited here is in .tmp/B95/ outside them.
- The two commits were rebuilt from the final tree (rt = final less the residual paragraph,
  then the residual), and the rt message was reworded for the fresh read.

Earlier heads: 3a227b286 (round 2), 2d173e842 (round 1).

**Round 2 (the Architect's answers to review P2, P3-1, P3-2 on B95-cause):**
- P2: a request not yet tried is served in its turn whatever the clock says; only a request the
  file server held (`Answer::Waiting`, the new `Pending::tried`) is judged against its deadline,
  at the time it is reached. `expire`'s `late` and `next_deadline` count only tried requests.
  mux_held now asserts the read behind the 2×bound write is answered with its data.
- P3-1: re-reading the clock in `expire` is not one line: `expire` takes synthetic times from
  the fuzz target and ipd's and sshd's tests. So it is one sentence on serving.md "The end"
  ("judged at the loop's turn ... late by at most one fill, never early").
- P3-2: the residual now says the successor's calls collect what the predecessor left pending,
  and the predecessor's InFlight, Requests and Pages stay held until the session ends.
- Gates after round 2, on 3a227b286, all rc=0 (logs .tmp/B95/host-r2/):
  - mux_held: passes.
  - `set CASES='rt-host-tests client-host-tests ipd-host-tests sshd-host-tests'`: PASS.
  - `set CASES='rt-miri formatting docs'`: PASS.
  - `prebuilt`: rv64 366 and rv32 352 cases, 0 failed. This is the consumer sweep: every bin
    linking redoubt_rt, both widths.
  - `set CASES='beamlet-files aio-many-reads'`: all four PASS (beamlet-files 2.4/2.3 s).

  Range-diff: .wash/local/B95-range-diff-r1.txt. The round-1 gates below were run on the
  round-1 head.
- A stale `.git/worktrees/B95/index.lock` from 14:01, with no git process alive, was removed
  before the fold.

Round 1 commits (since folded and reworded as cf35cb9e3 and 3a227b286):

- 1c275faa2 `rt: a multiplexed session's bound runs from its answer, and a request's deadline is
  judged when it is answered`: libs/rt/src/server/ninep_mux.rs, libs/rt/tests/mux_held.rs (new),
  docs/servers/serving.md (Multiplexed connections: status line 15→16 with the new test; the
  "Served only into" bullet says a request's deadline is judged when an answer reaches it; "The
  end" says the bound runs from the answer).
- 2d173e842 `serving: a restarted entry on the same badge meets its predecessor's multiplexed
  session`: the residual the Architect accepted on B95-cause (wording verbatim), on serving.md
  "The end", worded so the follow-up node can delete it whole.

Tier A (libs/rt). Ruling: .wash/local/wave-2026-10-18-rulings.md "B95-cause" and its corrected
premise. Identification: .wash/local/B95-identification.md.

## Cause (question 1)

Not walfsd's write path, not blkd: no `volume poisoned` line. IPD2's steward-three-contexts
failure is B51's poisoning, a different path. The cause is the 9P mux's session bound:
- `pump` set the deadline from a `now` read before `fill` served the pending requests
  synchronously.
- A fill of 10 s or more (walfsd's bound) left the session past its deadline at its own answer,
  and the next `expire` ended it.
- The waiter's next completion call opened a fresh session, and the hub reads `OPENED` as the
  connection's end. The VM's volume was then eio for its life: B93's first run already printed
  `cleaned: []` and `1 waiters`.
- The fresh session outlived the VM. init's restart uses the same badge, so the replay's
  `Hub::connect` landed on that session and got `Unexpected`, then eio.

## The fix

`pump` passes `fill` a clock; `fill` moves `now` after each request the file server is asked,
and `pump` sets the session's deadline from the time the last one was answered. A held request
is judged against its deadline at the time it is reached; an untried one is always served
(round 2). The syscall-free paths (`collect_into`, the fuzz target) keep
one fixed time. Cost: one `time_now` per served request.

## Reproductions (logs under /home/mcloonan/redoubt/.tmp/B95/)

| Run | Tree | Result |
| --- | --- | --- |
| alone | diagnostic | both widths pass, 1.7 s; 136 requests, 2 waiters (alone-*) |
| injected 11 s fill | diagnostic | both widths fail exactly as B93 (inject-*.log): fill 11000 ms, session ended, fresh session, `cleaned: []`, `1 waiters`, replay connect `Unexpected` ×8, `wrote: {error,eio}` |
| natural load: beamlet-files ×10 per width beside B93's 8-case set (load/) | diagnostic | rv64 10/10, rv32 9/10 |

In the natural run, 31 boots of beamlet-files kept their consoles (measured runs and the load
set's). Five had one fill of 5134, 5142, 6401, 6891 and 9819 ms, each answering a single 24-byte
reply. None ended a session; every first run ended with 136 requests and 2 waiters. So the gap
under load is a single serve approaching and, at B93's heavier load, passing the bound: (a),
closed by the fix. No sign of (b), a live waiter failing to call again within the bound, so I
raise no follow-up for it.

## Other findings under load (not B95's; for the orchestrator)

1. **walfsd fails to mount under load.** rv32 run 2: `walfsd: the volume does not mount, and is
   served as corrupt` at start. Every beamlet then exits 1, and init reboots: "beamlet was
   restarted 5 times within 60 seconds". Console at load/rv32-02.kept.smp1.log and
   load/consoles/run-567976-*. It looks like B51/IPD2's load-induced blkd fault striking during
   mount, where walfsd prints no reason. Not B95's mechanism.
2. **userland-boot rv32 fails its heap check under load.** In the load set: `bootfsd: heap needs
   2308 pages for twice its 1154-page peak, capped at 2304`, 6 times (load/load-*.log). Alone on
   the fixed tree it passes (9.2 s). bootfsd's peak sits at the cap's half. This ran on the
   diagnostic tree, but the diagnostic adds no heap use.

## Gates as run (exit codes)

- `cargo test -p redoubt-rt --test mux_held` (q, 4 cores): passes on the fix. With the fix
  reverted it fails at its timeout assertion; with that assertion removed it fails at "the
  session ended before its bound" (left `[0,0,1,0]`). Each assertion catches the bug on its own.
- Host cases, `make -f scripts/jobs.mk set CASES=...`, five groups of at most four, all PASS,
  rc=0: rt, client, fileserver, blkd, host-tests, init, ipd, littlefsd, erofsd, netd, piped,
  consrelay, sshd, steward, net, verity host tests, and rt-miri. These ran on b0c545082, which
  is 1c275faa2's code before the docs and the rustfmt fold.
- `set CASES='formatting docs'`: rc=0 on head. The first run failed formatting on mux_held.rs;
  I fixed it and folded it in.
- `make -f scripts/jobs.mk prebuilt` on 1c275faa2: rc=0, rv64 366 and rv32 352 cases, 0 failed.
  This is every bin on both widths, rv32 included.
- `set CASES='beamlet-files aio-many-reads'` on the fix: rc=0, all four PASS (beamlet-files
  3.0/2.2 s).
- Not run: the full bench (Tier A's merge gate is the train's); the unsafe count (no unsafe
  added); an injected slow serve on top of the fix at boot (ready in .tmp/B95/fixinject, commit
  86fd9b9ea, but its cold build would load the machine during B94's exclusive run; the held-clock
  host test covers the same path).

## Question 2 (the case ending at its exit)

Ruled (a): unchanged. The replay in the grace stays the case's condition and a forbidden line
there stays the verdict. testbench.md is unchanged.

## Summaries checked

- docs/servers/serving.md: updated as above. R77 and docs/SECURITY.md are unchanged: R77's claim
  is what a client may hold, and the new test asserts the session's life and timeouts.
- docs/userland/native.md:624-632 (a hub's waiter and `COLLECT_WAIT`), docs/userland/beamlet.md:1362,
  docs/testbench.md:369: these state what a client must do to keep its session. That is still true,
  and the server is now more lenient. No change.
- libs/rt/src/server/ninep_mux.rs module doc ("for the session bound since the last one
  returned"): matches the code now. No change.
- README.md, GETTING-STARTED.md, milestone progress: this is a behaviour fix inside a built
  feature, with no status claim. No change.

## Risks and notes

- A long fill for one session still delays every other session's call in walfsd's queue. A
  client whose call waited in the kernel queue behind another's 10 s fill could find its own
  session expired before the server took that call. That is not observed here: beamlet is
  walfsd:data's only client. Same family as (b), worth a line on the follow-up node if one is
  made.
- Stranded-session residual: the follow-up node (Architect's INIT6) closes it.
- Scratch: .tmp/B95/inject (diagnostic at 0958edaf7, also the local ref `B95-diag`) and
  .tmp/B95/fixinject. To be removed once the reviewer has read the logs. The diagnostic is not on
  wp-B95.
- Nothing pushed.

## steward-context-login, rv32, train 89 (read only; reruns are the orchestrator's)

The step that failed is the reattached VM's `:redoubt.send` of `consol`'s `ended` to its
`/dev/cons`, which answered `{:error, :timeout}` (.tmp/train-89/*bob.work-again.ssh.log:6), so
`{:ended, :ok}` never came and the case ran to 1,500 s.

- **What the timeout is.** A `send` is one-way: the mux never answers it. beamlet's `send/2`
  native gives the receiver `SEND_TIMEOUT_US` = 1 ms to take it (userland/otp/redoubt/src/
  system.rs:43, 451-463). `timeout` is that kernel send not taken within 1 ms. A session ending,
  or a request judged against its deadline, has no path to the sender's result.
- **Who should have taken it.** Over SSH the VM's `/dev/cons` is sshd's slot endpoint, not
  consoled. sshd's slot driver (servers/sshd/src/bin/sshd.rs:676-830) is in `receive` only between
  turns. Each turn runs the SSH core, writes its output to ipd (retrying with `sleep(us)` on
  backpressure), and runs `slot.turn` (the console's `wake`). On one rv32 hart under a whole
  bench, without icount, a turn longer than 1 ms needs only the host to deschedule QEMU once.
- **What this branch changed on that path.** One `time_now` per request the file server is
  asked in a fill, and one per answered `pump`: microseconds per turn. sshd sets
  `requests_wait(FOREVER)`, so its requests' deadlines are `FOREVER`. The `tried` filter in
  `next_deadline` and `expire` therefore changes nothing there: a `FOREVER` deadline was already
  dropped, and its session deadlines now only come later, never earlier. I find no way for the
  change to keep sshd's driver out of `receive` longer than it was.
- **Reading.** A pre-existing 1 ms rendezvous race between the VM's send and sshd's driver,
  exposed by the train's load. No earlier occurrence in the kept logs (grep of .tmp for
  `{:ended, {:error, :timeout}}`). If the reruns alone reproduce it, the place to look is the
  case's reliance on a 1 ms send, or sshd's turn length, not the mux.
