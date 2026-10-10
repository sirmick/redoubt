# CTX2 report (ctx-implementer-3, second launch)

Branch wp-CTX2, base main 24d3016b7, head b181fb8f5. Not pushed. Four commits:

1. 46c542610 sshd: a restarted sshd aborts its predecessor's sockets and attaches afresh
2. 2b768f508 consrelay: a context's console relay, kept across SSH channels
3. 4d506d29d steward: a context outlives its channel: attachments, detach, reattach and takeover in the core
4. b181fb8f5 steward, sshd: closing SSH detaches a context; a login reattaches or takes it over
   (carries the Size budget lines: libs/wire, libs/steward, model, servers/sshd,
   servers/steward, servers/consrelay, servers/init)

Local backup branches (delete after the merge): ctx2-wip-backup, ctx2-wip-backup2,
ctx2-final-tree, ctx2-pre-f5316, ctx2-pre-24d3.

## The handoff's Left list (state found at the start)

All done by the previous ctx-implementer-3 run before the fold: the trap-1 relay fix, the
steward-context-labels and sshd-restart-detaches cases (buffer bound stays a host test), the relay's
pages measured (consrelay-footprint: 22-page heap both widths, 8,360 / 7,392 bytes of first stack;
relay stack 5, heap cap 44, RELAY_PAGES 128; testbench.md row), Size budget lines. What was
missing was valid machine evidence on the rebased head; it is below.

## steward-red's findings on f72304885, folded

- **P1, takeover from a stalled channel ended the context.** `drive.rs`: a `Step::Detach` that
  fails `Timeout` counts as done (the relay lets the channel go as it takes the call; only the
  note was late); any other failure still ends it. `steward.rs`' `detach` passes `Timeout`
  through. Relay: the detach waits for its note only while the writer waits for output.
  Tests: host:redoubt-steward-server::a_takeover_from_a_stalled_channel_still_takes_it_over,
  host:redoubt-consrelay::a_detach_from_a_stalled_channel_is_answered_at_once (its stand-in
  console admits more than sshd's LIMITS so the stalled write parks instead of being refused:
  the worse case; with sshd's real limits the write was refused and the writer left).
- **P2, non-pty EOF detached.** `Chan::terminal_closed` = pty && input ended && all read; only
  that detaches. A pty channel's read after its input ends waits until the channel ends (else
  the relay's reader could see EOF before the steward's detach reached it). The relay now
  passes the attached channel's end of input to the VM as the end of a file (`ENDED`; an end
  from a channel let go changes nothing) — needed: the first gate pass showed every non-pty
  session hanging after EOF (ssh-idle, context-login...). Tests:
  host:redoubt-sshd::only_a_pty_channel_s_end_of_input_closes_the_terminal,
  host:redoubt-consrelay::the_attached_channel_s_input_end_is_the_vm_s_end_of_file,
  host:redoubt-consrelay::a_channel_s_input_end_reaches_the_vm_as_the_end_of_the_file.
  steward-session-ends' first alice login is now `pty = true`. sshd.md, sessions.md (also its
  stale "closing SSH ends it / second login refused" paragraph) updated.
- **P2, steward-context-labels could pass trivially.** Cause: sshd is blocked in the login
  call while the steward runs the batch, so the probe's cross-hand and the plain relay raced for
  the console's second admission bucket. The probe now hands over 3 s after the attach from the
  serving loop (receive timeout, as restart-probe), when sshd has served the channel's own relay.
  The plain session now expects its prompt, `plain-before` and, after the vault VM's late write,
  `plain-after`; forbid unchanged. The vault relay is refused at sshd (admission, I believe; the
  page and case say "admission or the label check").
- **P3:** stale `relay_vm` closed at the next budget creation (every batch creates budget and
  scope before its relay); model.md: attachments property row, and a residual that no property
  checks a takeover tells the old channel; consrelay.md: no cut on output without a newline,
  stated in the bound and the residuals; steward.md residual: the steward waits up to 2 s per
  hello and 1 s per attach/detach, outside R26, and the relay shares the VM's CPU.

## Also fixed

- tests/data/pipe (pipe-hostile-output's recipe, now also main's job-* cases'): consrelay
  entry and public, steward stack/heap 9/30, bootfsd heap 2,304, session 11,136, as the image.
  job-interrupt-ssh failed login (Failed) without it.
- model.md residual said 72,020 default sequences; it is 65,020.
- Rebase onto 24d3016b7: conflicts only in the variant count (now 168) and the model size
  ceiling (11,092, measured).

## Gates (exit codes)

Host, `cargo testbench --exact` / `cargo test`, on the final tree: size-budget PASS,
unsafe-budget PASS, docs PASS, formatting PASS; `cargo test -p redoubt-steward-server
-p redoubt-steward -p redoubt-sshd -p redoubt-consrelay` rc 0; builds rv64/rv32 rc 0.

Machine, `make -f scripts/jobs.mk prebuilt` (rv64 266 / rv32 252 cases, 0 failed) then
`set CASES=...` both widths (cases with no arch once, on rv64):

- On 858b0831a (= head but the pipe recipe), 70 cases: 105 rc 0, 2 rc 1 (job-interrupt-ssh both
  widths, the recipe above). The set: every steward-*, sshd-*, consrelay-* and
  bench-ssh-loopback* case; docs, size-budget, unsafe-budget, formatting, no-cruft,
  model-host-tests, steward-model-host-tests, steward-host-tests, sshd-host-tests,
  wire-host-tests, init-host-tests, elixir-oracles, beamlet-footprint, consol-size,
  boot-profile, boot-profile-unverified, image-disk, userland-boot, userland-read-only,
  userland-bad-start, init-boot, shell-commands, shell-long-output, every pipe-* case,
  job-interrupt-ssh, job-interrupt-line, job-interrupt-native, job-kill, launch-idle.
  Case list: .worktrees/CTX2/.tmp/h6-cases.txt; log .tmp/h6-set.log.
- On b181fb8f5: pipe-hostile-output, job-interrupt-ssh, job-interrupt-line,
  job-interrupt-native, job-kill: all 10 runs rc 0 (log .worktrees/CTX2/.tmp/h7-set.log).
- Not run: the whole bench (trains run it); model-mutations (main's merge adds variants beside
  R80's three; the R80 three were caught on the old base).

## Documentation checked

- Updated: docs/servers/steward.md (Contexts: stalled takeover; residual on relay waits),
  sshd.md (Ending: pty only, pty reads wait; status list), consrelay.md (protocol: when a detach
  waits; /dev/cons end of file; failure: stalled channel; the cut and its residual),
  sessions.md (closing detaches only a pty terminal; the stale paragraph), kernel/model.md
  (count 168, attachments row, untold-takeover residual, 65,020).
- Checked, no change: README.md and GETTING-STARTED.md (no claim about closing SSH);
  docs/plan/m2-usable-shell.md (contexts built; cap and idle expiry left — still true);
  SECURITY.md R80 row (its tests still exist); testbench.md rows (relay and bootfsd unchanged).
  No crate READMEs for sshd, steward or consrelay.

## Open risks

- The probe's 3 s settle assumes the channel's own relay has opened its console by then.
- The admission explanation for the labels case is inferred, not logged by sshd.
- A relay that dies alone is read from the code; no test kills one.
- The steward's `ended` after channel_closed may hold it up to 1 s (RELEASE_TIMEOUT).
- tests/data/pipe/boot.toml's header still says "for pipe-hostile-output alone"; main's job-*
  cases use it too (pre-existing, left).

## Fix round 2 (head 8373404f5, base 24d3016b7)

Commits: 46c542610, 6be503603 (consrelay), 031fda5e3 (steward core, model), 8373404f5.

- steward-red P2 (farewell skipped when the writer is busy): a detach now waits for its note
  at most 150 ms (`NOTE_WAIT_US`); the relay runs its own copy of the skeleton's loop to wake at
  that bound. Tests: host:redoubt-consrelay::a_busy_writer_s_channel_gets_its_note_before_the_detach_returns
  (fails on the old code), a_detach_from_a_stalled_channel_is_answered_at_its_bound
  (150 ms <= wait < 900 ms, no note while stalled). consrelay.md, steward.md and sessions.md say
  the bound and that a channel which has stopped reading may close untold. consrelay ceiling
  778.
- PolicyEndLeaseAdmitted alone, release, one core, clean exports: main 24d3016b7 caught by
  steward_policy seed 86 in 1.45 s; b181fb8f5 missed by steward_policy within its 500-seed cap,
  caught by steward_noninterference seed 126 in 91.8 s (the bench's job deadline is 25 s). The
  lease rule did not move; attach/detach dilute the policy family. New directed scenario
  end_lease_admitted_scenario (no RNG draws): session + agent fill the domain's four pending
  requests, the session ends the agent; P13 catches the mutation. Its job now 0.44 s.
  model.md (four directed scenarios) and the STEWARD_CAP comment updated.
- Gates: consrelay/steward-server/sshd host tests rc 0; docs, formatting, size-budget PASS.
  steward-context-login, steward-session-ends, steward-context-labels on 01c846e19 (the head
  before the model scenario, which touches only model tests and model.md): 6/6 rc 0.
  model-mutations on 8373404f5: PASS, 168 jobs, all 168 caught, case 200.6 s; slowest
  R2NoWaitCap 18.96 s, then R4OverdrawOnDelivery 8.10 s, R22MapFixedWalksFirst 2.64 s.
