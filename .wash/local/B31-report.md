# B31 report: an SSH session quiet for 11 s ends as at Ctrl-D

Branch wp-B31, one commit ca99ec87d on main d0a19ac6c (K26 and
B30 in). Worktree /home/mcloonan/redoubt/.worktrees/B31. Not pushed.

## Cause

`sshd`'s per-connection driver (`servers/sshd/src/bin/sshd.rs`, `drive`) keeps its own receive
loop around the channel console's `NineServer`, but received with `FOREVER` and never called
`NineServer::expire`. sshd sets `requests_wait(FOREVER)`, so the session bound is `COLLECT_WAIT`
(10 s): the VM's parked completion call should be answered empty at its hold. It never was. The
VM's hub times its call out at hold + `COLLECT_MARGIN_US` (11 s), reads that as the server
breaking its promise, ends the console; the shell reads EOF, prints `ok`, exits; the steward
destroys the budget. Not a host-clock effect: every quiet session longer than ~11 s ends. (The
12-15 s in BEAM14's probes is the hold starting after the last traffic, not at the send.)

Probe (rv64, quiet class): the new case on unfixed sshd fails at ~10 s of bob's idle prompt
(`steward: users/bob/{} holds 0 pages, 0 processes`, `sshd: session ... ended`), exit 1; with
the fix, PASS 56.6 s.

## Fix

In `drive`: `slot.nine.expire(now)` at each turn; the receive bounded by `next_deadline()`;
`Err(Timeout)` is a turn, not the connection's end; `now` read again after the receive, so what
came is served at the time it came (as `run_around` does). +5 code lines.

## Who owns the rule

docs/servers/serving.md, "Multiplexed connections", "Who runs it": a server with its own loop
calls `expire` and bounds its `receive` by `next_deadline`. Now names `sshd`'s driver beside
`ipd`, says what goes wrong when a loop skips it, and that a `Timeout` from the bounded receive
is a turn, never the server's end (the trap sshd fell into, as asked).

## The UART console

Not affected: `consoled` runs `NineServer::run_around`, which expires and bounds its receive (so
does the beamlet fixture). The only other mux server with its own loop is `ipd`, which is
correct. So no console case.

## The case

`tests/steward-ssh-idle.toml` (rv64, rv32): alice's VM sleeps 45 s inside one evaluation with
nothing on her channel, bob sits at his prompt all that time; each then answers a marker
(`"alice-still"`, `"bob-still"`). Verdicts: the sessions' own answers over sshd's channels, sshd's
and the steward's lines, init's no-exit/no-reboot (rule F). Added to `scripts/jobs.mk`'s quiet
class (own `quiet +=` line). NB: the main checkout's jobs.mk does not have it until merge, so I
ran the gate runs explicitly with `q run --quiet`.

## Gates (main d0a19ac6c + B31; prebuilt rc=0; logs /home/mcloonan/redoubt/.tmp/B31/final/)

- steward-ssh-idle, `q run --quiet` alone, 3 per width: rv64 59.0/57.0/56.5 s, rv32
  56.3/56.6/56.2 s, all PASS rc=0.
- quiet class via jobs.mk, both widths, rc=0: steward-ssh-two-principals, steward-vault-session,
  steward-session-ends, steward-sub-budget-flood.
- steward-login-refused, steward-restart-ssh (net class), both widths: rc=0.
- userland-boot rv64, rv32: rc=0 (B30 in).
- sshd-host-tests, docs, formatting, size-budget, unsafe-budget, no-cruft: rc=0.
- ./test-shell not run: beamlet not touched.

steward-sub-budget-flood (asked about): passes on both widths with the fix, here and in an
earlier run on the K26 base, so its failure on main is very likely this idle end.

## Size budget

sshd is 1205 code lines; ceiling raised 1200 -> 1205, with the `Size budget: servers/sshd: ...`
line in the commit message.

## K26 overlap (checked)

Before K26 merged I read `git diff ac178530e wp-K26 -- servers/sshd` and trial-merged: one
unavoidable adjacent-line conflict (K26 changed the reader arm right after the `receive` line
the fix replaces), plus the sshd.md status count and the size ceiling. After K26 merged I
rebased: resolved as my receive lines + K26's `reader.took` arms, count 15; the merged loop
reread. The sshd.md sentence went into the pty bullet, which K26 does not touch. Then rebased
cleanly onto d0a19ac6c (B30).

## Pages and summaries checked

- docs/servers/serving.md "Who runs it": updated (above).
- docs/servers/sshd.md "Sessions over SSH": the case in the status list (15); the pty bullet says
  the console's parked completion call is answered at each hold, so a quiet session stays.
- docs/testbench.md, the alone-class rule: unchanged; it names "the steward's SSH session cases"
  generically and its stall-at-the-boundary reasoning still holds (now the only way a quiet
  session ends).
- docs/userland/beamlet.md (console, hub): unchanged; client side was right.
- README.md, GETTING-STARTED.md, servers/sshd (no README): no claim about idle sessions; grep of
  docs for idle/quiet-session claims found none other.

## Open risks

- A server loop that skips `expire` is not caught by any host test; only boot cases with a quiet
  console catch it. sshd's loop is not host-testable as is (machine-only module).
