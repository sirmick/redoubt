# B3 — The bench says why a guest died at once, and checks its QEMU

Package B3, Tier A (the bench is trusted code), size S. Branch `wp-b3`, worktree
`/home/mcloonan/redoubt/.worktrees/b3`, based on `main` a76994302. Plan node `B3`.

## What happened

On 2026-10-01 the dev image's QEMU was 10.0.13, which does not know
`-run-with exit-with-parent=on` (QEMU 10.1 introduced it; the bench has passed it since
7c9b51f2c). Every boot case failed in 0.0 s as `guest exited while waiting for /.../` with an
empty console log, because `tools/testbench/src/qemu.rs` `run()` spawns QEMU with
`.stderr(Stdio::null())`. QEMU's one-line error was found only by running its command by hand.
The dev image is being moved to QEMU 11 (owner's Dockerfile change, not yours); this package
makes the bench say so itself next time.

## Rules first

1. `.wash/SWARM.md`: *The implementer*, then *Staging, commits and handoffs*.
2. `CONTRIBUTING.md`: *Commits* and *Formatting*.

## Reading list

- `tools/testbench/src/qemu.rs`: `run()` (the spawn, the console thread, `Line::Exited`), the
  `EXIT_WITH_PARENT` constant and the test `a_killed_bench_leaves_no_qemu`.
- `tools/testbench/src/main.rs` around the OpenSSH `-V` preflight (~line 515): the existing
  pattern for checking a host tool once and reporting what is missing (`--allow-skip`).
- `docs/testbench.md`: the paragraph on `-run-with exit-with-parent=on`, and *The harness can
  fail* / how the bench's own checks are tested (grep `known-bad`, `bench-` cases under
  `tests/bench-*.toml`, for example `tests/bench-poweroff-missing.toml`, which is your example
  of the work: a bench self-test that provokes a failure and checks the verdict's text).
- `GETTING-STARTED.md` *Prerequisites*.

## Owned paths

`tools/testbench/src/qemu.rs`, `tools/testbench/src/main.rs` (the preflight only), new
`tests/bench-*.toml` self-tests, `docs/testbench.md` (the two paragraphs), `GETTING-STARTED.md`
(the QEMU version in Prerequisites). Not the bundle builder (`build.rs`), not `Dockerfile` or
`dev.sh`.

## Deliverables

1. **A guest that exits before its first console line is reported with QEMU's error.** Capture
   QEMU's stderr (a pipe read on its own thread, bounded), and when `Line::Exited` arrives before
   any console line, or QEMU's exit status is non-zero, the verdict text carries the last stderr
   lines, and the log file gets them too. A guest that printed lines and then died keeps today's
   message plus the exit status.
2. **A preflight for the QEMU option.** Once per bench run, before the first case, the bench
   probes `qemu-system-riscv64 -run-with exit-with-parent=on` the way the test at
   `qemu.rs:396` does (or `-run-with help`), and fails at once with the version it needs
   (QEMU 10.1 or later) if the option is missing — `--allow-skip` makes it a SKIP of every boot
   case, as a missing firmware does. Both widths' binaries are checked.
3. **Self-tests**, the harness being held to the kernel's standard: a `bench-*` case that
   provokes an immediate QEMU exit (an impossible option or a missing firmware path) and checks
   the verdict names the error; and a host test for the preflight's message. Show each catches
   the deliberate break.
4. **Pages:** `docs/testbench.md` says what an early exit reports and that the bench checks its
   QEMU; `GETTING-STARTED.md` names QEMU 10.1 or later. Status lines name the new cases;
   `cargo run -q -p redoubt-doccheck` clean.

## The environment: every build runs in the dev container

Run every cargo and bench command from your worktree through
`/home/mcloonan/redoubt/.wash/local/in-dev <command>` (host `cargo` has no RISC-V targets).
The container's QEMU is 11.0.2, so to see the failure you are fixing, provoke it (deliverable
3), do not downgrade anything. Plain `git` in your worktree is fine; never in
`/home/mcloonan/redoubt` itself; never `git stash`.

Commands: the bench's own tests `cargo test -p redoubt-testbench` (check the crate name in
`tools/testbench/Cargo.toml`); your cases `cargo testbench bench-`; formatting
`cargo +nightly fmt --all --check`; the docs checker; the whole bench once before review.

## Early checkpoint

Report (under 2000 bytes, detail in `.wash/local/B3-progress.md`) after deliverable 1 with its
self-test, within about 30 tool calls. Then set waiting and END YOUR TURN.
