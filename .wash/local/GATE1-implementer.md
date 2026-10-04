# GATE1 — The kernel containment gate: one composite boot

Package GATE1, Tier A (attack case, the bench), size M. Branch `wp-gate1`, worktree
`/home/mcloonan/redoubt/.worktrees/gate1`, five test-only commits on `main` a76994302 (already
rebased: the merge-base is main's tip). Plan node `GATE1`.

The previous implementer's handoff was lost with the previous host. Its state is in the branch's
five commit messages (`git log main..HEAD`), which are complete, and in the plan node's body:
K12 is merged, so the destruction targets the gate missed should now be met.

## Rules first

1. `.wash/SWARM.md`: *The implementer*, then *Staging, commits and handoffs*.
2. `CONTRIBUTING.md`: *Commits* and *Formatting*.

## Reading list, in this order

- `git log --format='%h %s%n%b' main..HEAD` in your worktree: what is built and what each step
  showed. That is your example of the work.
- `docs/kernel/README.md` *Containment*: the design the case must match, the verdict table, and
  *The run* (both widths, one hart, checked build, tracing kernel, virtual time, one pinned seed
  chosen by a 16-seed sweep on both widths, which the page records).
- `docs/kernel/scheduling.md` *Responsiveness*: the targets. The gate adds none and widens none.
- The owner's rule, QA `GATE1-design` (header only): a miss of R10, the deadline notice or a
  lease end is a finding against the kernel; GATE1 stops and reports, the attacker is not
  weakened. QA `GATE1-composed-r10` (header): the acceptance floor, full fill at 30/40/125 ms
  with H no slower than D. QA `GATE1-mint-lease` (header): the endpoint maker runs in `users`.
- `tests/kernel-containment.toml`, `tests/programs/src/bin/kernel-containment.rs`,
  `tests/programs/src/sched.rs` (the roles), `docs/testbench.md` only the sections on
  `sched-trace`, the scheduler oracle and the seed (grep).
- `docs/kernel/budgets.md` *Residual risks* items 3–5 (K12's design) for what changed under you.

## Owned paths

`tests/kernel-containment.toml`, `tests/programs/src/bin/kernel-containment.rs`,
`tests/programs/src/sched.rs`, `tests/programs/src/bin/sched-latency.rs`,
`docs/kernel/README.md` (*Containment*: status line, the sweep record), `docs/plan/m1-separation.md`
only if the page names the gate's progress. No kernel source: a miss is reported, not fixed.

## Deliverables, in order

1. **The full case on both widths** with every row ok and every target met (R10 ≤ 30 ms, the
   deadline notice ≤ 40 ms, the lease end ≤ 125 ms, driver and steward wakes per the page), on
   the branch as it is. Record the numbers. If a target misses: stop, write the phase numbers to
   `.wash/local/GATE1-miss.md`, and report; do not widen a target or weaken the agent.
2. **The 16-seed sweep on both widths**, and the pinned seed chosen from it, in the case file.
   Record the sweep table in `.wash/local/GATE1-sweep.md` and on the page as *The run* asks.
3. **The page**: *Containment* goes from planned to built, its status line names
   `bench:kernel-containment`, in the same commit as the case that makes it true; the docs
   checker finds nothing.
4. **The whole bench green**, rv32 compiling, the unsafe count unchanged (no kernel change), the
   size budget untouched.
5. **A clean branch**: fold the five commits into logical ones with clean messages before review.

## The environment: every build runs in the dev container

This host has no RISC-V targets, QEMU or nightly rustfmt outside the dev container. Run every
cargo, bench, fmt and doccheck command from your worktree through the wrapper:

```
/home/mcloonan/redoubt/.wash/local/in-dev <command>
```

It runs the command in the container, in your current directory, with the firmware paths set.
Host `cargo` must not be used. Plain `git` on the host, in your worktree, is fine. Never run git
in `/home/mcloonan/redoubt` itself.

Commands (through the wrapper):
- the gate: `cargo testbench kernel-containment` (both widths); one width: `--arch rv32`.
- a seed: see how `sched-latency` takes its seed (grep `seed` in `tests/sched-latency.toml` and
  `docs/testbench.md`); repeat a result about five times before trusting it, twenty only for a
  flake.
- rv32 compile: `./build --arch rv32 --programs`; formatting: `cargo +nightly fmt --all --check`;
  the docs checker: `cargo run -q -p redoubt-doccheck`; the whole bench, once before review and
  once at acceptance: `cargo testbench`.
Tail bench output to the verdict lines; never read a whole log. The gate's logs are large.

## Early checkpoint

Report (member_update, under 2000 bytes, detail in `.wash/local/GATE1-progress.md`) after
deliverable 1 on rv64 alone: the rows, the measured numbers against each target, the wall time of
one run. Aim for that within about 30 tool calls. Then set waiting and END YOUR TURN.

## Report, at the end

Per SWARM: the paths changed; the exact commands and exit codes; why each verdict is the
system's; the whole bench; the sweep and the pinned seed; open risks; the branch tip; the next
step.
