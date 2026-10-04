# B4 — Three cases order on events, not on the host's clock

Package B4, Tier B-sized (tests and `libs/rt`'s own test only), size S; the red team reviews at
low because two of the cases are attack cases. Branch `wp-b4`, worktree
`/home/mcloonan/redoubt/.worktrees/b4`, based on `main` f72375f15. Plan node `B4`.

## Rules first

1. `.wash/SWARM.md`: *The implementer*, then *Staging, commits and handoffs*.
2. `CONTRIBUTING.md`: *Commits* and *Formatting*.

## Reading list

- `docs/todo/wall-clock-flakes.md`: the three cases, what each orders with a fixed host-clock
  wait, and the done-when. It is the whole specification; the package deletes it.
- The three cases and their programs: `tests/uaf-lent-page.toml` and its program (the grabber's
  100 ms after SYNC; the holder answers SYNC before it holds the lend, so today the case can pass
  without testing reuse at all — fix that ordering first, it is a test bug, not only a flake);
  `tests/touch-beyond-ram.toml` and its program (the survivor's 300 ms against the attacker's
  refusal, with the expect lines in order); `libs/rt`'s `parked.rs` test (LONGEST 2 s, C's 100 ms,
  D's deadline, on the fake kernel's host clock).
- `docs/testbench.md` rule F (trusted verdicts) and the pages each case's status line sits on
  (grep the case name in `docs/`), so the verdict's source does not move to the attacker when
  the ordering changes.
- One example of the work: a case in `tests/` that already orders on an exit or abandoned
  notice rather than a sleep (grep `notice` in `tests/programs/src/bin/`).

Derive the rest by grep. Do not read the book.

## Owned paths

The three case files and their programs under `tests/` and `tests/programs/src/`, the test in
`libs/rt` (the test only, not the runtime), `docs/todo/wall-clock-flakes.md` (deleted) and its
`docs/SUMMARY.md` line, and the status lines that name the cases if their text must change. No
kernel, loader or runtime code: if a case cannot be ordered on an event the system provides, that
is a question to the orchestrator, not a new mechanism.

## Deliverables

1. `uaf-lent-page`: the holder answers SYNC only once it holds the lend; the grabber proceeds on
   that answer, not on time. Show the case fails on a kernel that reuses the frame (it must have
   been able to before; if it could not, say so: that is the finding).
2. `touch-beyond-ram`: the survivor proceeds on the attacker's refusal (its exit or abandoned
   notice, or a line the judging program sees), not on 300 ms.
3. `parked.rs`: the test drives the fake kernel's clock, or orders on the runtime's events, with
   no wall-clock constant that a loaded host can miss.
4. Each passes 20 runs in a row while another bench runs beside it (five other packages are
   running on this host; that is your loaded host), both widths for the bench cases.
5. The todo page and its SUMMARY line go; status lines and the docs checker clean.

## The environment: every build runs in the dev container

Run every cargo, bench, fmt and doccheck command from your worktree through
`/home/mcloonan/redoubt/.wash/local/in-dev <command>` (host `cargo` has no RISC-V targets).
Plain `git` in your worktree is fine; never in `/home/mcloonan/redoubt` itself; never `git stash`.

Commands: `cargo testbench <case>`; a loop: `for i in $(seq 20); do cargo testbench <case> || break; done`
inside one in-dev call, tailing to the verdict lines; `libs/rt`'s test as the bench runs it
(grep `rt-host-tests` in `tests/`); formatting `cargo +nightly fmt --all --check`; the docs
checker `cargo run -q -p redoubt-doccheck`; the whole bench once at the end:
`cargo testbench --allow-skip` (exactly one SKIP, bench-ssh-loopback-openssh).

## Early checkpoint

Report (under 2000 bytes, detail in `.wash/local/B4-progress.md`) after deliverable 1 with its
20-run loop on rv64, within about 30 tool calls. Then set waiting and END YOUR TURN.
