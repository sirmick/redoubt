# BEAM3 handoff (checkpoint: bench being made fast, B18/B19)

## Branch state (wp-BEAM3, worktree /home/mcloonan/redoubt/.worktrees/BEAM3, tree clean, base main 992d447ce)
Commits, oldest first:
1. ebfbdfbbd beamlet: the console on the hub, a waiter per connection, no reader thread
2. 1b5af4227 beamlet: a native body honours its native's retry (interp.rs ONE hunk, own commit, ruled on BEAM3-interp-retry)
3. 8eb3d6f20 wire, rt, client: an Rerror keeps its name by one table (BEAM3-error-table option A)
4. 366f32932 beamlet: a file operation the platform finishes later parks only the process that asked
5. 72fc42f4d beamlet: files over 9P on Redoubt, through the hub — the old WIP 185863450 (beamlet-files on PACK1's shape: erofsd:system, endpoint=, fs=erofs, erofsd in programs/forbid) is FOLDED in, and so is a rustfmt fix to bin/beamlet.rs's usage doc line (left over-long by the rebase)
6. c9eae5ae5 WIP: fix B (BEAM3-waiter-session). Before review, give it a clean message, e.g. "client, beamlet: a caller busy past the session bound keeps its sessions"; it stays its own commit (aio.rs is outside the owned paths, as the ruling sanctions)
Folding without -i: if the commit to fold is directly under HEAD: `git reset --soft <target> && git commit --amend --no-edit`.

## Fix B as built (libs/client/src/aio.rs waiter())
- HAND_OVER_US = COLLECT_WAIT/4 (2.5 s): a wake-up send not taken in that time is held; the answers are copied into pages of their own length (compact()), so the 16-page completion buffer is reused; the waiter then calls with hold 0 (keeps the session, takes what is ready) and tries the oldest again.
- MAX_HELD = 4: with 4 held, the send waits FOREVER (no more reads; the server may end the session at its bound). Stated in aio.rs's module doc and native.md.
- After a failed/ended completion everything held is sent FOREVER, then the waiter returns.
- Tests: libs/client/tests/aio.rs a_caller_busy_past_the_session_bound_keeps_its_session (12 s; held answer is 1 page; a new read after is answered, not Ended) — NOT YET RUN (killed at the checkpoint); userland/otp/redoubt/tests/console.rs a_vm_busy_past_the_session_bound_keeps_its_console (parked read + pending write + 12 s sleep) — PASSES with the fix, FAILS without it (console.rs:141 Eof panic; checked by swapping in HEAD's aio.rs).
- Pages: native.md "Many requests at once": new bullet "A busy caller keeps its sessions", status 11→12 with the new test; beamlet.md async section: sentence on a scheduler away past the session bound + new test (8→9).

## Measurement (asked by the orchestrator)
Uncommitted probe on rv64/boot-profile (icount guest time, verified volume): from the platform's start to the first console read (the prompt) the VM thread never entered its endpoint: 7.93 s (7 927 716 us; unverified run the same). Lookups: 95, all from the pack, 106 ms in total, the longest one 23.5 ms. So the long stretch is the VM's own start work (interpreting), not lookups; under icount it is under the 10 s bound, but a slower/shared run can pass it, which is what failed beamlet-footprint/userland-read-only before. With fix B, at most a write answer and a read answer are held then (writes are one at a time), well under MAX_HELD. Probe reverted (never committed).

## Evidence since resume
- Host: `q run --cores 4 -- cargo test -p beamlet-vm -p beamlet-redoubt --features beamlet-redoubt/fake` (in userland/otp): exit 0, every suite green (console 10 incl. the new test, files 12, vm 37, io_wait 4, ...).
- fmt: `cargo +nightly fmt --check` clean for redoubt-client and the userland/otp workspace.
- Sweep: all 12 `*-build` cases both widths via jobs.mk: 24/24 PASS (log /tmp/beam3-sweep.log). The client API change is two new pub consts only.
- rv64/boot-profile and boot-profile-unverified PASS (with the probe in).

## Next
1. Run the client test: `q run --quiet -- cargo test -p redoubt-client --test aio` (and the whole redoubt-client suite for the gate). Then reword commit 6.
2. Machine, both widths: beamlet-files (on PACK1's shape, never run yet), beamlet-footprint and userland-read-only (failed before fix B), beamlet-console, beamlet-boot, aio-many-reads; smoke userland-boot, init-boot, bench-net-peer, ipc-outcomes. Report beamlet-footprint's footprint change (waiters' 8-page stacks + completion buffers per connection; held hand-overs only while the VM is busy).
3. Short gate: both builds, host tests (beamlet-vm, beamlet-redoubt, redoubt-client), docs, fmt --check, size-budget (libs/client grew with fix B: check its ceiling), unsafe, no-cruft.
4. Report .wash/local/BEAM3-report.md (summary ≤1900 bytes) + `result` with head, commits, every gate exit; include the measurement above, the rebase conflict list (from the earlier progress), departures: Twstat refused → write_stat times and truncate-at-position enotsup; littlefsd stores no mtime (0); rename and console size are typed calls on the VM thread; Console::file accessor + file.rs helpers outside the original owned paths; aio.rs fix B (ruled).

## Traps
- Follow the resume note for how cases run after B18/B19 (q / jobs.mk rules may change).
- aio-many-reads-two fails both widths pre-existing (fails the same on train-3's worktree); report as such.
- Fake kernel does not model thread_exit: a waiter that returns panics on the host (seen only in a probe).
- beamlet-files' Erlang must print with ~s only (no epp/io_lib_pretty on the small disk); its big file is 70000 bytes from a 250-byte pattern (a 100000-int list exceeds the per-process heap).
- rustfmt is `cargo +nightly fmt`; vendored crates spam warnings: grep our paths.
- vm.rs is shared with BEAM8 (my hunks: next, finish, send_owned, terminate, poll_files, test Home); interp.rs only commit 2's hunk.
- Host CLI `beamlet` prints nothing without a tty: probe Erlang with a throwaway beamlet-redoubt test, never committed.
- Case logs: target/testbench/run-*/<case>-<arch>-smpN.log.
