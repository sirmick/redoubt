# IPC2 — Fair waiting serves the least recently served group

Package IPC2, Tier A (kernel IPC and the model), size M. Branch `wp-ipc2`, worktree
`/home/mcloonan/redoubt/.worktrees/ipc2`, based on `main` a76994302. Plan node `IPC2`.

The earlier brief and the MOD1 steward-family patch that were to be handed to this package were
lost with the previous host; nothing is inherited. Everything below is derived from the pages
and the thread, which are complete.

## Rules first

1. `.wash/SWARM.md`: *The implementer*, then *Staging, commits and handoffs*.
2. `CONTRIBUTING.md`: *Commits* and *Formatting*.

## Reading list, in this order

- `docs/kernel/ipc.md`: R2 (fair waiting) as now written, and the first bullet of *Residual
  risks* ("The kernel still turns by one cursor per endpoint"). The rule is settled; this
  package makes the code match it.
- `docs/todo/r2-least-recently-served.md`: the deliverables, verbatim. The package deletes it.
- `.wash/qa/MOD1-r2-cursor-blame.md`: the Architect's ruling (decision ref 9f0b47f38): why a
  single cursor breaks R37, and the shape of the fix. Read the *Answer* only.
- `docs/kernel/invariants.md` I11 (fair turns): its bound stays k receives; its status line and
  *Kept in* must name the new code and tests.
- `docs/servers/steward.md` R37 (vault non-interference): the rule only.
- Kernel: `kernel/src/message.rs` `next_sender` (~line 992) and `deliver` (~1024), and the
  endpoint frame's `cursor` word. Nothing else in this file is yours: K13 will change
  `process_ending`, `thread_ending` and `pump` next, so keep this diff to the turn order.
- Model: `model/src/kernel.rs` the endpoint's `cursor` (~483), the take at ~1538–1560 and ~1720,
  `Ghost::took`; `model/src/mutation.rs` the R2 mutations (~32–36) and how
  `R2FifoAcrossAccounts` is wired at kernel.rs ~1550 (your example of a mutation);
  `model/src/check.rs` `flood` (~363); `model/src/policy.rs` `steward_noninterference` (~702);
  `model/tests/properties.rs`.
- One example of the work: the K10 merge (`git show --stat aec5ce054`, parent commits with
  `git log --oneline aec5ce054^2 -5`): a kernel change, its model twin, the mutation, the page
  status lines and the bench case landing together.

Derive the rest by grep. Do not read the book.

## Owned paths

`kernel/src/message.rs` (turn order only), `model/src/{kernel,mutation,policy,check}.rs`,
`model/tests/`, the new bench case under `tests/` and its program under `tests/programs/src/`,
`docs/kernel/ipc.md` (R2's status line; the residual goes), `docs/kernel/invariants.md` (I11),
`docs/todo/r2-least-recently-served.md` (deleted), and `docs/SECURITY.md` only if the register
cites the residual. `size-budget.toml` only if the kernel grows, with the reason in the commit.

## Deliverables

1. **Kernel.** `receive` takes the oldest message of the group served least recently. A waiting
   group's turn is due from its last take, or its oldest message's arrival if not served since,
   whichever is later, read from one kernel counter no process can read. Ties go to the lower
   group key. A group with nothing queued keeps no state: no per-label-set table on the
   endpoint. The per-endpoint `cursor` goes.
2. **Model.** The same rule in `model/src/kernel.rs`; `Ghost::took` still checks I11's bound
   within k receives; `flood` and `kernel_sequences` pass.
3. **`steward_noninterference`** extended to vault approvals and denials as vault work, vault
   session ends and crashes in both worlds (MOD1's extension, moved here by the thread), passing.
4. **A mutation** that keeps one cursor across label sets (today's behaviour) and fails the
   steward family. Name it in the R2 style of `model/src/mutation.rs`.
5. **A bench case** on both widths whose verdict comes from the system (rule F): one label set's
   service order at a shared endpoint is unchanged by what another label set sends.
6. **Pages in the same commits as the tests:** R2's status line names the new tests and
   mutation; the residual bullet goes; I11's *Kept in* names the new code; the todo page is
   deleted; `cargo run -q -p redoubt-doccheck` finds nothing.

A design gap (for example where the counter lives if the pages leave it open, or an I11 bound
the new rule cannot keep) is a blocking question on a thread `IPC2-<topic>` to the Architect,
with the page, the rule and the options. Never improvise a rule.

## The environment: every build runs in the dev container

This host has no RISC-V targets, QEMU or nightly rustfmt outside the dev container. Run every
cargo, bench, fmt, miri and doccheck command from your worktree through the wrapper:

```
/home/mcloonan/redoubt/.wash/local/in-dev <command>
```

It runs the command in the container, in your current directory, with the firmware paths set.
Host `cargo` (a distro build with only x86_64) must not be used. Plain `git` on the host, in your
worktree, is fine. Never run git in `/home/mcloonan/redoubt` itself.

Commands (through the wrapper):
- model host tests: `cargo testbench --list | grep -i model` to find the case names, then run them;
  focused: `cargo test -p redoubt-model --test properties flood`.
- the IPC case and yours: `cargo testbench redoubt-ipc`, `cargo testbench <your-case>`.
- rv32 compile: `./build --arch rv32 --programs`.
- formatting: `cargo +nightly fmt --all --check`.
- the whole bench, before the review and before acceptance only: `cargo testbench`.
- the docs checker: `cargo run -q -p redoubt-doccheck`.
Tail bench output to the verdict lines; never read a whole log.

## Early checkpoint

Report (member_update, under 2000 bytes, detail in `.wash/local/IPC2-progress.md`) once
deliverables 1 and 2 compile on both widths and `flood` and `kernel_sequences` pass, before
starting 3–5. Aim for that within about 40 tool calls. Then set waiting and END YOUR TURN.

## Report, at the end

Per SWARM: the paths changed; the exact commands and exit codes; each new case and why its
verdict is the system's; the `unsafe` count before and after; what was deleted; the rv32 build;
the whole bench; open risks; the branch tip; the next step. Then rebuild the branch into clean
logical commits before the merge.
