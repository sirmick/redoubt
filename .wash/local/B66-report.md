# B66 report: the refused `ssh -p 2222` after `./launch --system`

- base: main c8422f745; branch wp-B66; worktree /home/mcloonan/redoubt/.worktrees/B66
- head: 2855ee3eb (one commit on wp-B66), tree clean
- paths: launch, tools/testbench/src/launch.rs, tools/testbench/src/main.rs,
  tools/testbench/src/qemu.rs (one `pub`), GETTING-STARTED.md
- gates: host tests 9 passed; prebuilt rv64 298 / rv32 284 cases, 0 failed; launch-system and
  launch-keep-disk PASS on rv64 and rv32; docs, formatting, unsafe-budget PASS (docs and
  formatting rerun after the last edits); `git diff --check` clean; end-to-end `./launch
  --system` five times (runs 1–5)

## Diagnosis

**The refusal came while `scripts/q run --cores 4` waited for QEMU's lease, after the ssh line was
printed and before QEMU ran.** Nothing else on the path can refuse.

Evidence, all on this host, logs under `.worktrees/B66/.tmp/B66/` (every line of launch's output
and every change of the port's answer, timestamped from the command's start; `probe.py` is the
harness):

1. *QEMU's user-mode forward never refuses.* A bare `qemu-system-riscv64 -netdev
   user,...,hostfwd=tcp:127.0.0.1:2299-:22` with no guest at all: a connect to 2299 succeeds in
   2 ms and then hangs (20 s, no reset, no data); a port nothing holds (2298) refuses in 2 ms.
   So "Connection refused" means no QEMU held 127.0.0.1:2222 at that instant, whatever the
   guest's state. Candidate "guest not yet listening" is out: that hangs, or, once the guest's
   stack is up and sshd is not, the guest closes the connection (seen in run 2, `open, then
   closed` 5 ms before `init: started sshd`).
2. *Run 1, the owner's exact command under the q daemon on an idle machine*
   (`run1-with-q.log`): the ssh line printed at 47.44 s (after the build; `launch.rs` prints
   after `pack_case`, not before the build as the body supposed), QEMU's loader spoke at
   47.54 s, the port went from refused to open within 0.3 s of the print, sshd's banner
   `SSH-2.0-Sunset-0.6.0` was read at 51.2 s. No refusal window a person can hit.
3. *Run 2, without the daemon* (`REDOUBT_Q=/nonexistent`, `run2-no-q.log`): `launch: no scheduler
   answers`; print at 7.53 s, port open 0.5 s later, banner at 8.63 s. Same.
4. *Run 3, the lease wait staged* (`run3-lease-wait.log`): two holder leases (12 + 8 cores) so
   QEMU's 4-core lease queued behind them. The ssh line printed at 63.3 s; QEMU's loader spoke
   at 98.1 s; the port **refused every connection for those 35 s**. (The build's own 8-core lease
   had also queued 22 s behind another member's `cargo test`: `q ls` at the time is in the task
   output.) q's queue is served only as cores free up; a job that cannot fit takes free cores
   after 60 s only if at least half its ask is free (`scripts/q` STARVE_S), so with a bench
   holding the machine the wait is minutes, and q's client prints nothing while it waits.
5. The owner's run itself is not on record: `/tmp/redoubt-q.sock.log` starts at the daemon's
   start, 12:28 on 10-14, and the owner's QEMU (20 hours old on 10-15) began about 11:00. Its
   command line carried the forward (orchestrator), so candidate (2) is out. Candidate (3),
   `localhost` as `::1`: `getent ahosts localhost` on this host is 127.0.0.1 only; and ssh falls
   through to the next address anyway, so it cannot refuse alone. The owner's terminal text was
   asked for through the orchestrator (not blocking).

Not a defect in the forward or the guest, so no attack case; the defect is launch's print, which
said when to log in before QEMU ran. Fixed as step 2.

## Step 2: launch proves the forward after QEMU starts

- `launch` (the script) no longer `exec`s QEMU from the argv file; its boot step is `cargo run
  -p testbench -- --run --system --arch A --boot-argv FILE [--ssh-port, --key, --keep-disk...]`
  under the same 4-core lease (the build's lease still ends before it). `--print-only` removes the
  file and stops as before.
- `launch.rs`: `--boot-argv` reads the command (`from_nul_separated`, the inverse of the writer,
  with QEMU dying with its parent as the bench's own QEMU does; `qemu::exit_with_parent` made
  `pub`) and calls `run`. `run` spawns QEMU on this terminal, then `prove_forward` connects to
  127.0.0.1:port until the guest answers: refused (QEMU not yet bound) retries at 100 ms; a hang
  (guest's stack not up; slirp accepted) is dropped after 1 s and retried; the guest closing it
  (`Answer::Closed`: nothing on 22 yet) or sshd's banner (`Answer::Banner`) is the proof; QEMU
  exiting first is the failure, with its status. Then it prints, in raw-mode `\r\n`, `port 2222
  forwards to the guest...` with the ssh line again, and waits for QEMU. The direct `cargo run
  ... --run --system` (no script) goes through `run` too. The first print now says to log in once
  that line appears, and why.
- Measured (`run4-new-launch.log`): ssh line at 5.49 s, loader at 5.64 s, `port 2222 forwards to
  the guest's sshd (SSH-2.0-Sunset-0.6.0; ...)` with the ssh line at 6.84 s. The check is sshd's
  "connection 1" on the console (then `a connection failed: Ssh(SessionEOF)`), which the print
  and GETTING-STARTED say; a dropped hanging connection leaves no console line (runs 1–3).
- `ssh_line(port, identity)` is the one source of the printed line; the host test
  `the_ssh_line_names_the_forwarded_port` holds it to the host port `virtio_devices` puts in
  `hostfwd` for the same port. `the_forward_is_proved_by_the_guest_s_answer` covers banner,
  close and QEMU-exits-first against a local listener; the argv test round-trips and rejects a
  cut-short file.

## Tests and gates

- `scripts/q run --cores 8 -- cargo test --quiet -p testbench launch`: 9 passed (3 new), exit 0.
- `rustfmt +nightly --config skip_children=true` on the three Rust files; `git diff --check` clean.
- `make -f scripts/jobs.mk prebuilt` then `make -j4 -f scripts/jobs.mk set CASES="launch-system
  launch-keep-disk docs formatting unsafe-budget" WIDTHS=rv64,rv32`: exit 0; prebuilt rv64 298
  cases / rv32 284, 0 failed; PASS launch-system rv64 5.5 s, rv32 5.1 s; PASS launch-keep-disk
  rv64 7.0 s, rv32 7.0 s; PASS docs 5.8 s, formatting 23.1 s, unsafe-budget (0 undocumented).
  After the lease-wait line and its page sentence: `set CASES="docs formatting"` PASS again.
- Final end-to-end (`run5-final.log`): ssh line 2.36 s, `launch: QEMU starts under a lease of 4
  cores from scripts/q, behind the jobs already running; the ssh port is refused until it does`
  2.37 s, loader 2.51 s, `port 2222 forwards to the guest's sshd (...)` with the ssh line 3.65 s.
- Not run: the whole bench, the size budget (no size-bearing binary changed: the bench only).
- `unsafe`: none added (the one `unsafe` in `exit_with_parent` is unchanged, only `pub` now).

## Summaries checked

- GETTING-STARTED.md "Try the system": changed (the two prints, when to log in, the lease wait,
  sshd's connection 1); the `--system` line in the command list.
- README.md: no mention of the ssh line or launch's prints; no change.
- docs/testbench.md:233, 1034, 1304 (`launch = true`, `--keep-disk`, idle cases boot launch's
  command line): still true; the case path (`launch::command`) is unchanged; no change.
- docs/plan/m2-usable-shell.md and docs/servers/sshd.md: no claim about launch's port or prints
  (`grep -n 'launch --system\|ssh line\|ssh -p'`); no change.
- tests/launch-system.toml's description: unchanged, still what launch boots.

## The orchestrator's two branches of launch:99

- (a) a daemon answering, QEMU's lease waiting behind a bench: run 3, the 35 s refused window;
  now the script prints the lease line before it, and the bench prints the forward's proof after.
- (b) no daemon answering: run 2. `launch` prints `launch: no scheduler answers (scripts/q
  daemon); running without a lease` before the build and runs both steps unleased, at once; it
  neither waits nor exits after the ssh line; the port opened 0.5 s after the line. If the owner's
  launch ran this branch, its refusal needs a wait this branch does not have, so (a) under the
  old daemon is the consistent reading.

## Open

- The owner's terminal text (asked via the orchestrator, not waited on) would confirm which
  branch; the mechanism is reproduced regardless.
- q's client itself is silent while a lease waits (`scripts/q`, not this package's file); the
  script's line before the lease covers launch.
