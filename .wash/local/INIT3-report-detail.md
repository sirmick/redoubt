# INIT3 report: init's restarts, blame and the reboot rule

Branch `wp-init3`, six commits rebased on main 2ab6dc81c, tip 5c49e6973.

```
de85c10cd init: a server that ends is restarted, and one that cannot stay up reboots the machine
a97863598 testbench: a server's fault under init is reported, blamed and survived
900799b69 testbench: consoled under init rolls back a connection whose capability was lost
81c2f870a testbench: a driver under init is restarted on its device, or reboots the machine
b44ad41d0 testbench: a launcher's end leaves its orphan's connections dead
5c49e6973 consoled: a bucket holds the fids of every console init mints
```

Each commit was built and its cases run in its own tree (both widths), with doccheck.

## The rules, with code and tests

1. **Restart.** `servers/init/src/bin/init.rs`: `Boot::restart` prints the line, disconnects the
   dead instance's console connection, applies rules 5 and 6, then `Boot::start(i)`. The watching
   thread (`watch`) destroys the instance's budget; `start` carves a new one, mints init's own
   handle and the `handed` badges again from the kept receive rights, makes a new console
   connection, places the device copies again, and reuses the server's exit endpoint and
   watching thread. Tests: bench:init-restart, bench:init-driver-restart, bench:init-reboot.
2. **Grants.** Not through `Launch::grants`; the direct disconnect stays (see below).
3. **The boot's steps, again.** `Boot::step` and `Boot::settle`: `check_keys`, `attach_console`,
   `push_public` run on every new instance (`push_public` once the boot reached it). A failure
   after a restart reboots; one in the boot's own step refuses the boot as before. Read from the
   code: no case restarts keyd, consoled or bootfsd. That is the status's "a restarted `consoled`'s
   attach is read from the code".
4. **During the boot.** `drain()` after each later server's start, and `settle` waits on a step's
   server. A queued call to a dead server would block forever, so init's own handle at a server
   is minted stamped with that instance's budget (`mint(badge, Some(&budget))`), and the watching
   thread destroys the budget at the instance's end, so the sweep fails the call with `Dead`.
   `ENDED[i]` is counted before the destruction, so init never uses a swept slot that may have
   been reused. Test: bench:init-reboot, where all six exits fall during the boot and ipd never
   starts.
5. **The reboot rule.** `servers/init/src/restarts.rs` (`Restarts::restart`, `MOST` = 5,
   `WINDOW` = 60 s). Host tests: the_fifth_restart_goes_ahead_and_the_sixth_exit_reboots,
   a_restart_older_than_the_window_is_dropped_from_the_count,
   restarts_spread_wider_than_the_window_never_reboot,
   a_clock_that_reads_earlier_counts_the_restart_as_recent. Bench: init-reboot.
6. **Quarantined driver.** In `restart`, `device_info` on each kept copy; anything but a device
   reboots, naming the manifest's device. Bench: init-quarantine-reboot.
7. **Fault report.** `init: NAME (PID p) faulted, code c, serving nobody|account A, labels
   L1,L2|none; blamed on nobody: no steward`, then `init: restarted NAME, console N`. Labels are
   printed by manifest name. Bench: init-restart prints `labels secrets` from the kernel's notice.
8. **Restarted consoled.** `attach_console` prints the rule-8 line when `ENDED[consoled] > 0`.
   `connection()` drops init's console once consoled's end is counted. Read from the code.

## Rule 2: why the direct disconnect stays

- `Grants::connection` hides the minted id, which init prints on the started line.
- `Job` has no release without `wait`, and init's watching threads take the notices.
- A restarted consoled sweeps the parent handle that `Grants` records. A later release would go
  through a swept slot, possibly reused: the hazard grants.rs documents. init keeps
  `(id, ENDED[consoled])` and skips the disconnect if consoled has ended since.

Nothing was deleted. The direct disconnect moved from `watch` to `restart`, behind the
generation check.

## Cases (all pass on both widths)

init-reboot, init-restart, init-rollback, init-driver-restart, init-quarantine-reboot,
launcher-orphan. All existing init cases still pass; the test programs now park.

- **init-restart.** A labelled program cannot write the console (consoled R69), so the fault is
  served for a labelled `restart-faulter` while restartee holds the unlabelled reporter's WAIT.
  Both calls get `Dead`. Since 5c49e6973 the client opens its console first, as any server
  does; the workaround is gone.
- **launcher-orphan** (approved as built):
  - C's connection is minted through L's connection, by L, not through the tester's.
  - The tester launches C in a budget of its own, which L's end does not touch, so C is alive
    to be refused.
  - C's refused attach comes after the tester's `Job::wait` has released L's grants.
  - Verdict: the server's own count of the connections it minted, 2 before and 0 after. C's
    refusal is a trace.
  - L faults by overrunning its stack, with no `unsafe`: a panic with no call held is only an
    exit.
  - wire.md's line stays.
- **First line of a boot:** `loader: Redoubt rv64 loader, boot hart 0` (rv32 likewise).

## Page lines (as written)

- init.md "Restarts and reboots": status built, partly tested (steward's blame; restarted
  consoled's attach read from the code), tested (8). Driver sentence, two new bullets and the
  caption as in the brief. `**Open:** none.` removed (doccheck C1: not in a built section).
- init.md "Residual risks": the two bullets, verbatim.
- init.md "Fresh connections per child": built, tested (3), adding init-restart. "Authority":
  drops "the copies kept for a restart are not exercised" and "restarts are not built", adds
  init-restart and init-driver-restart.
- wire.md: `Status: built · tested: bench:launcher-orphan`, `**Open:** none.` removed. No text
  differs from the code. init itself releases by id, not through `Grants`, which the text
  allows.
- serving.md: the brief's partly-tested clause, plus init-rollback and init-restart.
- devices.md: the clause goes from "Which process gets which device" (tested 10); system_reset
  is tested (5), adding init-reboot and init-quarantine-reboot.
- testbench.md: the sentence verbatim, plus one on the new test programs parking.
- SECURITY.md: no row links these sections; unchanged.

## Gates

- `cargo +nightly fmt --check`: 0. doccheck: 0. unsafe-budget: PASS. init has none and keeps
  none; the tester's one `unsafe` (`Bundle::at`) is in tests/programs, which the budget does not
  count.
- size-budget: PASS. servers/init 1606 -> 1819 lines, with the reason line in de85c10cd.
- init-host-tests: PASS.
- The whole bench on both widths has not been run: waiting for the word.

## Design problems found, and open risks

- **consoled's file cap: fixed here, as ruled** (5c49e6973).
  - `CONSOLE_FIDS = 2`: the namespace's attach and the opened `cons`. `files` is
    `CONSOLE_FIDS * MAX_THREADS`.
  - `BUDGET` is 2 MiB: a bucket at its caps costs 154 880 bytes at `MAX_THREADS` 31 (13 fit)
    and 326 912 at 255 (6 fit).
  - consoled gets 1024 pages in image/manifest.json and in every tests/data/init manifest.
    init's system-fit host test sums the new figure.
  - consoled.md's Admission bullet is rewritten as ruled.
  - New host test `every_console_init_mints_attaches_and_opens_in_its_one_bucket`, listed under
    "Started by `init`".
  - Size budget: servers/consoled 341 -> 343, with the reason line.
  - Checked: r4-host-tests, consoled-build on both widths, every init case, doccheck, fmt, both
    budgets.
- **The bound after restarts in the boot.** The check of root's usage against the bound after
  the boot does not count restarts during the boot. No case restarts a server and then finishes
  the boot (init-reboot reboots first).
- **A failed carve or launch on a restart** powers off with a system-failure status, as in the
  boot, rather than rebooting.

## Simplifier round (folded into de85c10cd)

- Taken: settle's todo stack is gone. No case reaches a second server's restart during a
  step's wait, so that server's step now runs at once through a recursive `settle`. The commit
  body says so.
- Taken: `Restarts` holds `[Option<u64>; MOST]` and one cursor, so `len` is gone.
- Taken: init's ceiling is 1819, its measured size, with the same reason line.
- Left: the blame labels. The brief's rule 7 fault line names them, and init-restart judges
  `labels secrets`.
- Left: the device-name lookup by placement. Placements carrying the manifest name would
  change `Plan` and check.rs and their tests to save about 5 lines in `restart`.
