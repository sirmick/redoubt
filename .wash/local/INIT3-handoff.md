# INIT3 handoff (init3-implementer -> init3-implementer-2)

## State

- Branch `wp-init3`, clean, no WIP: six final, logical commits on 7135c5fed (INIT2's rebased
  tip), with the pages in them. Not rebased on main yet: the orchestrator says when.
  ```
  d1cace744 init: a server that ends is restarted, and one that cannot stay up reboots the machine
  c3b12e720 testbench: a server's fault under init is reported, blamed and survived
  0801541c6 testbench: consoled under init rolls back a connection whose capability was lost
  e36058d01 testbench: a driver under init is restarted on its device, or reboots the machine
  a4f303f80 testbench: launcher-orphan, a launcher's end leaves its orphan's connections dead
  26b5140d3 consoled: a bucket holds the fids of every console init mints
  ```
- Checkpoint: passed and reported (rules 1-5 plus init-reboot).
- Built: every rule 1-8 and all six cases, each passing both widths. Every page line is written
  and committed. Detail, rule by rule, in `.wash/local/INIT3-report.md`, which is the report
  draft.
- Gates that pass: nightly `fmt --check`, doccheck, unsafe-budget, size-budget (init
  1606 -> 1826, reason line in d1cace744), init-host-tests, every `init*` case.

## What is left

1. When the orchestrator says: `git rebase main`. The INIT2 commits drop out; expect conflicts
   only if INIT2 changed lines this branch touches: init.rs, `tests/init-programs`, the init.md
   status lines, testbench.md "servers' cases". After it, rerun `cargo testbench init`,
   `launcher-orphan`, doccheck, size-budget and unsafe-budget.
2. The whole bench on both widths, only with the orchestrator's word.
3. Done: consoled's fids, ruled into INIT3 (26b5140d3). The workaround is gone.
4. The final report: member_update with a summary under 1900 bytes, pointing at INIT3-report.md.

## Do not re-learn

- **Run every command through** `/home/mcloonan/redoubt/.wash/local/in-dev`.
  - `cargo testbench FILTER` takes one substring filter, plus `--arch rv64`.
  - Formatting is `cargo +nightly fmt -p CRATE`; stable rustfmt reports false diffs in the
    kernel.
  - doccheck is `cargo run -q -p redoubt-doccheck`.
- **Boot logs** are `target/testbench/CASE-rv64-smp1.log`. Read them only with
  `grep -a "^init:\|^\[con"`.
- **Under init, an exit is a restart.** Every test program parks
  (`redoubt_init_programs::park`), or it reboots the box.
- **A labelled program cannot write the console** (consoled R69).
- **consoled's file cap:** every account-0 console init mints shares init's files cap of 4.
- **A panic with no open call is an Exit with code 101**, not Faulted. For a real fault, overrun
  the stack (orphan-launcher's `overrun`).
- **A reply whose capability has no slot** comes back to the caller as `Sys(OutOfMemory)` with
  the reply present (ipc.md R4); fill the table by minting.
- **Kernel sweep:** destroying a budget sweeps every handle that names it or is stamped with it,
  and frees the slot for reuse. init never closes or destroys by an index after the sweep: see
  `ENDED` in init.rs.
- **init-programs bins are explicit in Cargo.toml**; tests/programs lists bins in a `bin = [...]`
  array.

## What consumed my context

The rule that did not hold is "don't read whole files", and the brief could not have prevented
it. The reading list was enough for the plan, not for the design. Rule 4 (a step calling a
server waits for the new instance) deadlocks on a call queued at a dead server. Solving it took
kernel semantics the list did not name: mint stamps, R10's sweep, R4b on queued senders, and
ipc.md's OutOfMemory row. Each case also needed the serving library and harness facts above,
found by trial.

The large items, roughly:
- init.rs read whole three times (about 550 lines each): once to rewrite it, once to review it
  before committing (the rule that every committed file is read in full), and parts again.
- Writing init.rs whole (about 650 lines) once, then many edits.
- libs/client: launch.rs whole (215 lines), grants.rs whole (100), file.rs (100).
- libs/rt: server/typed.rs (190), ninep.rs ranges (about 200), start.rs ranges, client.rs ranges.
- bootfsd's binary whole; four existing test programs whole, plus stub-launch's and
  ninep-discard's.
- Docs: init.md sections (about 300 lines), budgets R10, ipc R4b, devices quarantine,
  testbench sections, SECURITY.md grep output.
- About 40 bench runs. Output was grepped to PASS/FAIL lines, but each failure needed log
  lines.
- Writing 13 new test programs and their case files.
- The commit rebuild: a staging script and five validation runs.

What would have saved context:
- **The deadlock and its fix in the brief:** "init's own handle stamped with the instance's
  budget, destroyed by the watching thread".
- **The test-program facts in the brief:** a labelled program can't write the console, the
  panic-is-an-exit rule, and the shared consoled files cap.
- **A note that init.rs gets rewritten**, so it is read whole once, deliberately.
