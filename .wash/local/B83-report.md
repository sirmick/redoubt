# B83: steward-pending-cap's end step — report (b83-implementer)

Worktree `.worktrees/B83`, branch `wp-B83` on main 2b0c897c9, one commit (`testbench, cases: a
session step `ended` …`); `git status --short | wc -l` = 0. Logs under `.tmp/B83/`. Reviewer
kernel-red-3 (the bench is Tier A tooling; the orchestrator widened the scope to it, option A
below).

## Delivered

- `tools/testbench/src/case.rs`: `Step::Ended(i32)` — keep the input open, read until ssh exits
  on its own, require that status; only marks may follow it, as after `exit`
  (`check_sessions`; `only_marks_follow_exit` covers both).
- `tools/testbench/src/ssh.rs`: the driver's `Ended` arm (the timeout is the case's deadline,
  the failure text names the step: `timed out waiting for ssh's own exit`, `ssh exited (status
  N) on its own, expected M`); the log gets `[ended T s after ssh started]`; a session with an
  `ended` gets no default `exit`; host test
  `ended_keeps_the_input_open_until_sshs_own_exit` (a `pty = true` session typing a line then
  `ended` against `sh`: the line is read, the status required).
- `docs/testbench.md` "SSH sessions": the step in the table, the rule (an end the box must act
  on is followed by `ended`, not `exit`: `exit` closes the input first, and a pty's close
  discards what ssh has not read; `exit` stays for sessions the bench closes), the status block
  (20, the host test named).
- `tests/steward-pending-cap.toml`: marker line, `^bye$`, the prompt, `end_session(c)` as its
  own line, `{ ended = 0 }`, the mark; the comment names the two races and the pty rule.

## Why no case-only shape works (each built and run; all logs kept)

1. Marker + end on one line (main): the marker is lost — the end destroys the shell's VM and
   its console relay with whatever they hold (train 50; the orchestrator's reruns: main 6/6
   PASS, the B81 tree 3 FAIL of 6, nothing in the guest changed).
2. Marker matched, then `end_session` sent, then an expect of its echo, then `exit`
   (`alone/`): 10/10 FAIL at the echo — the end runs before even the end line's echo is relayed
   (leases killed, `session ended`, no echo). Nothing typed after the marker can be matched.
3. Marker matched, prompt, `end_session` sent, `exit` (`alone2/`, `load/`): 14/14 FAIL on the
   approver's forbid `"mine"`: the end line never reached the box (no lease killed; sshd's
   `ended` is the detach's, then `a connection's input ended`, `Ssh(SessionEOF)`). The pty's
   master is dropped by `exit` right after `send`'s write; the slave hangs up and discards
   unread input. (`bench-ssh-loopback`'s `exit 3` precedent is a `pty = false` pipe.)
4. A second login `alice.watch` ends the first with `end_context("")`, answered after the
   destroy, and session 1 exits after that mark (`alone3/`, `load2/`): a true order, 14/14 FAIL
   `OutOfMemory` carving the session (11392 pages) from alice's unlabelled share, which holds
   her console, session 1 and the two agents' leases (33284 pages); the leases live until
   session 1 ends. (steward.md "A cap per label set" says the share holds two sessions, one SSH
   context beside the console; sessions.md:129 says two SSH contexts beside the console — the
   pages disagree; not touched, outside the package.)
   Closed too: ssh's `Connection to … closed.` line (the bench passes `LogLevel=ERROR` before
   `ssh_args`; ssh keeps the first value); `[[input]]` on alice's console session (sets no
   mark); `pty = false` (the pipe keeps the line, but the close's `channel_closed` races the
   shell's parse and eval: train 46's detach); a sleeping helper (a timer).

## The other pty sessions that send a line and then `exit`

Eleven (`shell-contexts` ×4 incl. a `detach()`, `shell-machine`, `steward-context-cap`,
`steward-context-idle`, `steward-context-labels`, `steward-ssh-resize` ×2,
`steward-three-contexts` ×2): each `exit`/`detach()` is the session's last act and no verdict
depends on the box acting on it (the `holds … 10 processes` line in shell-contexts follows an
explicit `end_context`, not an `exit`). Left as they are. The eight `pty = false` pairs
(`bench-ssh-loopback`×2, `sshd-restart-detaches`, `steward-restart-context` ×2,
`steward-session-ends` ×2) read a pipe, which keeps the line before its EOF: untouched.

## What decides whether `bye` is relayed before the end

The shell's console writes go shell → `/dev/cons` → the context's console relay → sshd → ssh
(sshd.md:216-220); `end_session` is one call to the steward, whose end batch destroys the
budget (VM and relay with it) and then sends `ended` on its own console connection, which ends
the channel. A write lands iff the relay forwarded it to sshd before the destroy: two guest
paths scheduled by QEMU on the host, so the rate swings with nothing in the guest changed, and
even the end line's own echo is lost (2). With `ended`, from the prompt after the marker to
ssh's own exit (typing the end line, the shell's parse and eval, the end batch, sshd ending the
channel) took 130 ms on rv64 and 97 and 249 ms on rv32 in the three kept transcripts
(`final-runs/`, `target/testbench/run-2294457-*`).

## Gates (all through `scripts/q` / `scripts/jobs.mk`)

- `cargo test -p testbench`: 218 passed, rc 0 (`host-tests2.log`; one pre-existing warning,
  `field harts is never read`, not in this diff).
- `steward-pending-cap` with `ended`: 5x alone per width (`gate.log`, `alone4/`) and 2x both
  widths beside a load (`cargo build --release -p testbench`, 260 crate compiles, `load3/`):
  14/14 PASS; after rustfmt, once per width: PASS (`final.log`).
- `formatting` PASS (after `cargo +nightly fmt`), `docs` PASS, `init-boot` rv64+rv32 PASS,
  `bench-ssh-loopback` PASS, `size-budget` PASS, `unsafe-budget` PASS (`others.log`).

## Summaries checked

- docs/testbench.md "SSH sessions": updated (table, rule, status).
- docs/servers/steward.md:1357 "`steward-pending-cap` ends its session before its channel
  closes rather than a second after": still true (the channel now never closes from the
  bench's side before the end); no change.
- docs/plan/m3-agents.md:61 pending-cap row, docs/SECURITY.md:147, steward.md:807/1145,
  sessions.md:325, sshd.md:300 status lines naming the case: unchanged, the case still gates
  them.
- README.md, GETTING-STARTED.md: nothing on session steps.

## Open

- The two pages' disagreement on alice's SSH context count (above).
- `[[input]]` cannot set a mark, so a console-typed action cannot order a session's steps; not
  needed here.
