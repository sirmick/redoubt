# BEAM14 report: a labelled session's VM launches a child on the machine

Branch wp-BEAM14, worktree /home/mcloonan/redoubt/.worktrees/BEAM14, on main abd3fcd6d. One commit,
a53bbde4d. Tests, a case-scoped bundle recipe, jobs.mk's quiet list and pages; no Rust or Elixir
changed.

## Design (agreed with the orchestrator)

Over SSH, not a tester console. consoled's R69 is deliberate, and sshd's channel is the one sink
cleared for a label (sshd.md, R67); a tester serving a labelled console relayed to the UART would be
a second write-down path in bench code, with the verdict lines the tester's relay. The real steward
and sshd already carry vault sessions (steward-vault-session).

Check (a), before any case: exec over SSH. By the code it should work: Process.run mints the child's
console with NinepCommon.new_connection on the session's /dev/cons; sshd serves the channel through
NineServer::serve_parking, which answers ninep_common itself; sshd's Cons keeps the default
`minted` hook (accepts); a minted connection attaches at its own root without calling
Cons::attach. The case proved it on the machine, labelled and plain (below).

## The case: tests/steward-vault-launch.toml

- Bundle: tests/data/steward/vault-launch.boot.toml, image/boot.toml plus the `beamlet-hello`
  entry, with tests/data/steward/vault-launch.manifest.json (image/manifest.json with "public":
  ["beamlet", "beamlet-hello"]; that one line differs). The shipped image's /boot is unchanged.
- Session 1, alice+alice-secrets on a pty (session {7}):
  `exec("beamlet-hello", ["from", "the", "vault"])` shows `beamlet-hello: from the vault` on the
  channel and `{{:exited, 3}, %{pages: {256, 0}, processes: {1, 0}, weight: {1, 0}}}`;
  `{:own, carve(pages: 1, processes: 0, weight: 0) |> elem(1) |> destroy()}` -> `{:own, :ok}`;
  labels: [] -> `{:fewer, {:error, :label_denied}}`; labels: [7, 8] ->
  `{:more, {:error, :class_denied}}`. Each answer is tagged so no pattern matches the echoed input.
- Session 2, bob (plain, after the vault session ends, as the orchestrator asked): exec of
  beamlet-hello, `beamlet-hello: from bob`, `{:exited, 3}`, `pages: {256, _}`.
- Each session forbids the other's child line. expect_after: the steward's audit and
  `users/alice/{7} holds N pages, 2 processes`, sshd's `login alice+alice-secrets ... labels [7]`,
  then bob's three lines.
- Rule F: the vault child's line reaching the vault channel is sshd's label check passing a writer
  with labels equal to the channel's {7} (R25, R67); the refusals and the usage are the kernel's,
  printed by the session; the logins' labels and budgets are sshd's and the steward's lines.
- Class: quiet, by K27's rule (testbench.md "On a shared host"): a host-clock steward SSH session
  case whose VM's console is on the hub's hold and must outlive a hold boundary. Added to jobs.mk's
  quiet list beside steward-vault-session.
- timeout_secs = 160: four times the slowest pass alone (37.8 s on rv32), rounded up to 10 s.
  Measured passes: rv64 30.3, 35.7, 35.5 s; rv32 37.8, 37.3 s.
- From the rv64 session logs: the vault prompt at 2.5 s, the child's line at 3.6 s, its exit at
  5.2 s; bob's child line at 3.3 s after his ssh started.

## Gates (all through q / jobs.mk; scratch /home/mcloonan/redoubt/.tmp/BEAM14)

- prebuilt: rv64 231 cases, rv32 217, 0 failed (rebuilt after each change).
- steward-vault-launch: PASS rv64 and rv32 on the final tree (35.5 s, 37.3 s).
- beamlet-natives, beamlet-serve, beamlet-launch, beamlet-natives-attack: PASS rv64 and rv32
  (3.1/3.3, 7.4/7.7, 2.7/2.9, 4.4/5.1 s), on the tree before the last two edits, which were the case
  file's timeout and one sentence of beamlet.md, neither read by these cases.
- docs, formatting, size-budget, no-cruft: PASS on the final tree.
- No host-clock case failed, so nothing needed a rerun alone.

## Pages (summaries checked)

Updated: native.md "Launching from a session" (status: over SSH a steward's session launches,
vault and plain; on the UART a tester's); beamlet.md "Natives" (status 31 -> 32, and the paragraph
that said a labelled session runs nothing now says where it does: over SSH); shell.md "The shell
in a session" (status); sessions.md "Vault sessions" (status, and a sentence: a launched program
carries the session's labels and writes only to its own channel); sshd.md "Sessions over SSH"
(12 tests, and a sentence: a launched program gets its own connection to the channel's console via
new_connection, under the channel's label check) and R67's status; SECURITY.md R67 row (held to
the page); m1-separation.md progress (budgets and launching run in the steward's own sessions over
SSH).
Checked, no change: consoled.md (R69 unchanged: the UART still refuses labelled writes);
steward.md (no claim about launching from a session); testbench.md (the quiet rule names "the
steward's SSH session cases" generically); README.md, GETTING-STARTED.md (no claim about exec over
SSH).

## Open risks

- The case judges sshd's label check by its effect (the child's line shown on the vault channel).
  No case shows a child with wrong labels refused at the channel, and none can: a session can only
  make a child with its own labels (BEAM12's rule, the kernel's).
- The manifest copy can drift from image/manifest.json. It differs by one line; the init-refuses-*
  cases copy it the same way.

## The red's P1 and an M1 bug it exposed (2026-10-08)

The red (OK with notes at a53bbde4d), P1: "neither child's line reaches the other session" was not
judged, since bob logged in only after vault-done. Made the sessions overlap: bob at his prompt
before alice's exec, staying until both execs are done (uncommitted in the worktree).

Result: 6 of 6 runs alone fail (rv64 32.9/32.3/27.4 s, rv32 26.0/34.4/26.4 s), always `session bob: ... ssh exited (status 0) while waiting for
/beamlet-hello: from bob/`, his last output `(1)> ok`. The boot log: steward `users/bob/{} holds 0
pages, 0 processes`, sshd `session ... ended`. Docs and formatting pass.

Probes (rv64, quiet class, throwaway tests/zz-probe-*.toml, removed):
- bob on a pty: the same failure, so not pty-related;
- alice's exec only, bob idle about 5 s: PASS (14.1 s);
- alice sends `:timer.sleep(20_000)` with no exec: her own session ends during the sleep;
- alice exec then sleep 20 s: her session ends during the sleep;
- alice sleeping 4, 6, 8, 10, 12 then 15 s in turn: the first five survive, the 15 s one ends it.

So an SSH session with no console traffic for between 12 and 15 s ends as at Ctrl+D, whatever its
labels and with no load. The likely cause, not yet read in code: the session's hub connection to
sshd's channel console, whose completion call is held no longer than the session bound; with no
traffic the hold expires and the client reads it as the server's silence (K27's mechanism). No
existing steward SSH case stays quiet that long. Reported to the orchestrator as a question; BEAM14
waits on the answer.

Correction: I said the 6-at-once rv32 sweep failed in that round. It never ran: `--sweep` cannot be
combined with `--prebuilt`, and a sweep also needs a case that pins `qemu_seed`.

## The fix, as agreed (head 074922f8e, amended over a53bbde4d)

Alice carves first, before bob logs in. Then bob logs in and reaches his prompt, alice execs, bob
execs while her session is open, and both exit. The case's comment says the quiet stretches are
short because of B31. timeout_secs 150 = 4 x 37.4 s (this version's slowest pass alone, rv32).

- Alone, 3 per width: rv64 36.1, 35.0, 30.0 s; rv32 37.4, 28.1, 28.2 s, all PASS. On the head:
  rv64 35.2 s, rv32 37.3 s PASS.
- Six at once on rv32: six prebuilt invocations under one 8-core q lease, each in its own run
  directory (no sweep, for the reasons above): all PASS, 40.0, 42.1, 43.1, 43.3, 41.1, 43.2 s. No
  port race showed.
- Quiet stretches: the longest is alice's first carve, from her prompt to `{:own, :ok}` (the first
  evaluation loads modules): 7.5 to 10.1 s alone, 9.8 to 10.0 s under six-way load, against B31's
  12 to 15 s. Every other gap is under about 5 s. The margin is about 2 s.
- docs and formatting PASS.
