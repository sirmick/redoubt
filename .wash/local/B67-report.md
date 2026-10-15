# B67: SSH connect-to-prompt latency, measured, stated, reported each train

Base: main c8422f745. Head: wp-B67 850b57044 (one commit, amended after review: "from/after the
connection" became "after ssh starts" in the Login doc comment, the commit's first line, the M2
page and launch-system's comment; cargo test -p testbench 197 pass, docs PASS, fmt clean, rerun
on the amended tree). Review: kernel-red-2, Merge verdict OK with notes. Worktree
/home/mcloonan/redoubt/.worktrees/B67. Reviewer kernel-red-2 (the bench change).

Reading: docs/servers/sshd.md, docs/plan/m2-usable-shell.md and tests/launch-system.toml read
in full; tools/testbench/src/{ssh,qemu,main}.rs and docs/testbench.md (1,100–1,800 lines each)
read by every changed hunk with context plus the surrounding functions and sections, not whole.

## Headline

A login reaches its prompt about 1.9 s after `ssh` starts on either width (alice alone
1.65–2.08 s; bob beside alice 2.1–2.3 s), rv64 and rv32 within 5 % of each other. The shell's
own load, from beamlet's first read of it from the boot pack to the prompt, is the larger stage
(1.0–1.1 s); everything before it (connection, key exchange, login, carve, VM start) is 0.8 s
alone and 1.15 s beside another session. No ceiling: one head, three runs, not two trains.

## Where the numbers come from

- Train 30's session transcripts no longer exist: the bench keeps only its 8 newest
  `target/testbench/run-*` directories (tools/testbench/src/run.rs, `KEEP = 8`), and
  /home/mcloonan/redoubt/.worktrees/train-20/target/jobs/{rv64,rv32}-{launch-system,steward-ssh-two-principals}.log
  hold only the result line and the memory scan (launch-system 5.0/4.3 s, two-principals
  14.8/14.5 s: the same case times as my runs, so nothing suggests a different login). Earlier
  trains' `.tmp/train-*/` hold only bench summaries. Nothing under train-20 was changed.
- Mine: three repetitions of `make -f scripts/jobs.mk set CASES="launch-system
  steward-ssh-two-principals"` at main c8422f745 (before any edit), 2026-10-15 14:36–14:40 UTC,
  through q (launch-system `--lock net`, two-principals `--quiet`); other members' jobs were on
  the machine, so these are loaded-host figures, as a train's are. Transcripts and job logs are
  kept under /home/mcloonan/redoubt/.tmp/B67/rep{1,2,3}/ (`*.ssh.log`; the stamp line is
  `[/pattern/ matched N s after ssh started]`).

## The table (seconds after the session's `ssh` started)

launch-system (4 harts, 1 GiB, launch's image; alice alone; one expect, the prompt):

| width | run 1 | run 2 | run 3 | case time |
| --- | --- | --- | --- | --- |
| rv64 | 1.842 | 1.864 | 2.080 | 4.7–5.2 s |
| rv32 | 1.980 | 1.650 | 1.947 | 4.3–5.0 s |

steward-ssh-two-principals (1 hart): `beamlet: Elixir.Redoubt.Shell read from the boot pack`
(the VM's first console line: VM up, loading the shell) / the prompt:

| width | session | run 1 | run 2 | run 3 |
| --- | --- | --- | --- | --- |
| rv64 | alice alone | 0.813 / 1.863 | 0.802 / 1.884 | 0.793 / 1.816 |
| rv64 | bob beside alice | 1.207 / 2.269 | 1.156 / 2.324 | 1.146 / 2.187 |
| rv32 | alice alone | 0.803 / 1.821 | 0.806 / 1.825 | 0.805 / 1.825 |
| rv32 | bob beside alice | 1.118 / 2.188 | 1.118 / 2.127 | 1.114 / 2.161 |

Stages: ssh start → VM's first line 0.79–0.81 s alone, 1.11–1.21 s beside alice's running
session (+0.35 s: the second login, carve and VM start share one hart with her idle VM); VM's
first line → prompt 1.00–1.10 s in every run, both widths, both sessions: the shell's load
dominates (~55 % alone, ~48 % beside). The console's lines (`sshd: login …`, `steward: audit …
Login`) carry no host time, so the handshake and the carve cannot be told apart within the first
stage without stamping the console log (not in scope; a follow-up if wanted). Spread: rv64
launch-system 1.84–2.08 (13 %), rv32 1.65–1.98 (20 %); two-principals within 4 %. Widths agree
within 5 %. The page's earlier figures (rv32 prompt at 3.9–4.0 s, rv64 2.1 s) are stale: rv32 has
halved since, and the paragraph is replaced.

## Delivered (paths)

- tools/testbench/src/ssh.rs: `Login` (a session's `expect`s matched before its first `send`,
  with their times) and `Sessions` (failure + logins); `run` returns `Sessions`; `drive` returns
  the login; the `Driver` records it (`login`, `typed`). The log line is unchanged.
- tools/testbench/src/qemu.rs: `run` and `run_sessions` take `out`; each login is printed through
  it, in the bench's note indent, before the result line (as `qemu seed` notes are):
  `      alice's login: /\/home\/alice \([0-9]+\)> / 1.86 s after ssh started`
  `      alice's login: /Elixir.Redoubt.Shell read from/ 0.80 s, /\([0-9]+\)> / 1.86 s after ssh started`
- tools/testbench/src/main.rs: `boot_once` passes `out` (now `let mut`); the loopback runner
  takes `.failed` (its logins are OpenSSH's on the host, not reported).
- tests/launch-system.toml: the comment with the figures, the log line and where it is printed.
- docs/servers/sshd.md "Sessions over SSH": the measurement paragraph after the figure rewritten
  with the new figures, source, stages and the no-ceiling rule (the stale 2–4 s paragraph).
- docs/plan/m2-usable-shell.md Progress: one clause on the "On Redoubt the shell runs in each
  session over SSH" sentence (the page has no polish list: asked the orchestrator, message
  882bcfa3; moved if told).
- docs/testbench.md "SSH sessions": the report prints the login's expects.

## Gates (filled at the end; see "Results" below)

- `cargo test -p testbench`: 197 passed, 0 failed (through q).
- `cargo clippy -p testbench`: fails on libs/wire (pre-existing `chunks_exact` lints under this
  clippy, 3 errors with -D warnings); 10 pre-existing warnings in testbench files none of which is
  in a changed line (budget.rs, case.rs, cruft.rs, idle.rs, memory.rs, sched_oracle.rs).
- `cargo +nightly fmt -p testbench`: applied.
- Cases with the change: see Results.

## Summaries checked

- README.md, GETTING-STARTED.md, docs/README.md: describe how to log in, no timing claim; no
  change.
- docs/userland/beamlet.md 570–577: boot-profile's console-session figures in guest time under
  icount, a different measurement; no change.
- docs/servers/steward.md 1219: a 2 s timeout for a relay's hello, not a latency; no change.
- docs/servers/sshd.md: the stale measurement paragraph (2–4 s; rv32 3.9–4.0 s) replaced; the
  "Sessions over SSH" status block names bench:steward-ssh-two-principals already.
- docs/testbench.md: the session-log sentence extended.
- docs/plan/m2-usable-shell.md: Progress updated.

## Results

Gate run with the change (2026-10-15 ~14:50 UTC, `make -f scripts/jobs.mk set CASES="launch-system
steward-ssh-two-principals bench-ssh-loopback"`, logs copied to /home/mcloonan/redoubt/.tmp/B67/gate/):

- PASS launch-system [rv64, smp=4] 5.1s — `alice's login: //home/alice \([0-9]+\)> / 2.00 s after ssh started`
- PASS launch-system [rv32, smp=4] 4.8s
- PASS steward-ssh-two-principals [rv64, smp=1] 15.1s
- PASS steward-ssh-two-principals [rv32, smp=1] 14.8s — `alice's login: /Elixir.Redoubt.Shell read
  from/ 0.83 s, /\([0-9]+\)> / 1.85 s after ssh started`; `bob's login: … 1.15 s, … 2.19 s`
- PASS bench-ssh-loopback 10.8s (the loopback path through `ssh::run`)
- docs: first run FAILED on C4 (a commit hash in docs/servers/sshd.md); the hash became the date
  (in tests/launch-system.toml too); rerun: PASS docs 1.4s, rc=0.

Not run: the whole bench, rv32 builds beyond the two cases (Tier B: the package's cases and the
docs checker). Open: the orchestrator's answer on where the M2 "polish list" sentence belongs.
