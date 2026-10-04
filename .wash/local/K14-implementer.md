# K14 — Kernel memory follow-ups: `process_map` backs after it can refuse; one boot stack reservation

Package K14, Tier A (kernel memory code and the loader), size S. Branch `wp-k14`, worktree
`/home/mcloonan/redoubt/.worktrees/k14`, based on `main` a76994302. Plan node `K14`.

Two settled follow-ups, each a page's residual with its fix and its case already written down.
No design question is expected; if one appears, it is a blocking thread `K14-<topic>` to the
Architect, never a guess.

## Rules first

1. `.wash/SWARM.md`: *The implementer*, then *Staging, commits and handoffs*.
2. `CONTRIBUTING.md`: *Commits* and *Formatting*.

## Reading list, in this order

- `docs/todo/process-map-backs-before-refusing.md` and `docs/todo/boot-stack-reservation.md`:
  the deliverables, verbatim. The package deletes both.
- `docs/kernel/memory.md` *Failure and restart* (a refused call charges nothing) and its
  *Residual risks*; `docs/kernel/memory-layout.md` *Regions* (32 stack pages) and its *Residual
  risks*.
- Code: `kernel/src/process.rs` `process_map` (the `ensure_range_exists` call ahead of the
  per-page source, destination and charge checks); `kernel/src/mem.rs` `ensure_range_exists`;
  `kernel/src/arch/riscv/process.rs` `setup_loader_process` and `DEFAULT_STACK_SIZE`;
  `loader/src/main.rs` `USER_STACK_TOP`, `USER_STACK_PAGES`.
- The example of the work: the `map-fixed-attack` case (`tests/map-fixed-attack.toml` and its
  program) for how a refusal is judged with the caller's usage, and ATK1's merge
  (`git show --stat 5e3e7e43a`, its commits with `git log --oneline 5e3e7e43a^2 -6`) for an
  attack case landing with its page lines.

Derive the rest by grep. Do not read the book.

## Owned paths

`kernel/src/process.rs` (`process_map` only), `kernel/src/mem.rs` (`ensure_range_exists` and
what the fix needs), `kernel/src/arch/riscv/process.rs` (`setup_loader_process`,
`DEFAULT_STACK_SIZE`), `loader/src/main.rs` (the stack's reservation only), the two new cases
under `tests/` and their programs under `tests/programs/src/`, `docs/kernel/memory.md` and
`docs/kernel/memory-layout.md` (status lines and residuals), the two todo pages (deleted) and
their `docs/SUMMARY.md` lines, `docs/SECURITY.md` rows if the checker asks. `size-budget.toml`
only with the reason in the commit. Nothing in `kernel/src/message.rs` (IPC2's and K13's).

## Deliverables

1. **`process_map` refuses before it backs.** Every check that can refuse runs before the
   source is backed, or the backing is undone on refusal, so a refused call leaves the caller's
   usage unchanged. Prefer ordering the checks over undoing work; say which in the commit.
2. **Its case**, both widths: a `process_map` whose source is an untouched reservation and whose
   destination is taken, and one the child's budget cannot pay for; the caller's usage is shown
   unchanged after each refusal, judged by a party the attacker cannot impersonate (rule F).
3. **One stack reservation.** One place reserves a boot process's stack, and it is the page's 32
   pages. The second reservation goes.
4. **Its case**, both widths: in a boot process, `map_fixed` of the page just below the 32
   succeeds and of the lowest of the 32 is refused as an overlap. `map-fixed-attack` drops its
   rv32 avoidance if that was its only reason (check the comment).
5. **Pages in the same commits as the tests:** the residual items leave `memory.md` and
   `memory-layout.md`, the status lines name the new cases, the todo pages are deleted, the
   docs checker finds nothing.

Report what was deleted (a Tier A kernel package that adds lines and deletes none says why) and
the `unsafe` count before and after.

## The environment: every build runs in the dev container

This host has no RISC-V targets, QEMU or nightly rustfmt outside the dev container. Run every
cargo, bench, fmt and doccheck command from your worktree through the wrapper:

```
/home/mcloonan/redoubt/.wash/local/in-dev <command>
```

Host `cargo` must not be used. Plain `git` on the host, in your worktree, is fine. Never run git
in `/home/mcloonan/redoubt` itself; never `git stash`.

Commands (through the wrapper): your cases `cargo testbench <case>`; the related ones
`cargo testbench map-fixed`; rv32 `./build --arch rv32 --programs`; formatting
`cargo +nightly fmt --all --check`; the docs checker `cargo run -q -p redoubt-doccheck`; the
whole bench once before review and once at acceptance: `cargo testbench`. Tail output; never
read a whole log.

## Early checkpoint

Report (member_update, under 2000 bytes, detail in `.wash/local/K14-progress.md`) after
deliverables 1 and 2 pass on rv64, within about 35 tool calls. Then set waiting and END YOUR
TURN.

## Report, at the end

Per SWARM: paths changed; the exact commands and exit codes; each case and why its verdict is
the system's; `unsafe` before and after; what was deleted; rv32; the whole bench; open risks;
the branch tip; the next step. Then rebuild the branch into clean logical commits before review.
