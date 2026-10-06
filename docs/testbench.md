# The test bench

The bench is how every claim in this book is tested. It builds the real kernel, loader and
programs, packs and signs a boot bundle, boots it under QEMU, and judges what appears on the
console, over SSH and on the network, from outside the guest. It also runs host tests and source
checks. It is part of the system under tenet 6 ([the tenets](TENETS.md#6-tested-to-hell-and-back)),
held to the same standard of simplicity as the kernel: one tool, `tools/testbench`, and one file per
case in `tests/`.

## How to use it

```sh
cargo testbench                 # every case but those run only by name
cargo testbench timer           # cases whose name contains "timer"
cargo testbench --arch rv64     # one target
cargo testbench --list          # names and descriptions, those run only by name marked
cargo testbench sched-latency --sweep 1..20 --jobs 4   # one case, a seed sweep, 4 boots at a time
./test                          # the same, from the repository root
```

The exit status is non-zero if anything fails. Each run keeps its boots' console logs, SSH
transcripts, disk images and captures in a directory of its own,
`target/testbench/run-<pid>-<time>/`, and `target/testbench/last` names the latest. Runs may
overlap in one worktree: each packs the kernel cargo reports for its own features, never the
shared `target/` path another run may have rebuilt. A case the host cannot run (no RustSBI
firmware, no OpenSSH) fails and says what is missing; `--allow-skip` reports it as skipped
instead. Every run boots the vendored RustSBI prototyper: `scripts/build-bios.sh` builds it for
both widths, or `RUSTSBI_PROTOTYPER` and `RUSTSBI_PROTOTYPER_RV32` name the images. There is no
fallback to QEMU's own firmware.

| Path | What |
| --- | --- |
| `tools/testbench/` | the bench: builds, injects programs, boots QEMU, judges the console, sessions and network |
| `tests/*.toml` | the cases, one per file (`tests/data/`: files they read; `tests/keys/`: SSH test keys) |
| `tests/programs/` | `no_std` programs that run inside Redoubt: the log server, victims, attackers, checkers |
| `tests/net/` | the network clients and the judge that the net cases start under `init`, and the host test that checks each net case's manifest against its case file |

## Verdicts

### What a case passes on

<details><summary>Status: built · tested (6)</summary>

- bench:bench-console-after-expect
- bench:bench-poweroff-missing
- bench:bench-qemu-early-exit
- bench:rustsbi-boot
- host:testbench::a_killed_bench_leaves_no_qemu
- host:testbench::a_qemu_lacking_an_option_is_named

</details>

A boot case passes when every `expect` pattern matches a console line, in order, no `forbid`
pattern ever matches, and the boot ends as the case says. Three patterns are always forbidden:
`PANIC`, `TEST FAILED` and `WARNING: INSECURE`. After the last `expect`, and any sessions, the bench
keeps reading for 50 ms (1 s in a checked build), so a forbidden line right after the last expected
one still fails the case. With `poweroff = true` it reads instead until QEMU exits, and requires the
exit status the case names (0 by default; 255 for an SBI system failure). The bench starts QEMU
with Linux's parent-death signal asked for between fork and exec, so a bench killed at its
timeout, even outright, takes its QEMU with it and leaves no guest running to skew the next run.

Before a width's first boot the bench runs that width's QEMU with `-version`, and a QEMU that
cannot run fails every boot case at once with its complaint (skips them, with `--allow-skip`, as a
missing firmware does). A guest that ends before it prints a line, or whose QEMU exits with a
failing status, fails with QEMU's exit status and the last lines of its stderr,
which go to the console log too; one that printed and then died fails as it did, with QEMU's exit
status.

In-guest programs print through the log server and finish with `<NAME> TEST PASSED` or
`<NAME> TEST FAILED`; attack programs end with `attempts done` instead. The log server starts every
line it prints for a client with the sender's badge, as the kernel reports it, on every path that
takes client text: `[pid N] ` for a badge below 0x100, and `[badge N] ` for any other. The
tester numbers a case's programs by their place, 2 on (it is 2), at most 16, and gives each its
place as its badge, and every badge a test program mints is 0x100 or more, so a minted badge can
never print as a place and forge a case program's line (`tests/programs/src/logsrv.rs`). Lines
without a prefix come from the kernel, the loader, the log server's own fixed templates, or a
program that owns the UART.

### Rule F (trusted verdicts)

Status: built · tested: bench:bench-attack-forgery, bench:bench-reporter-mismatch, bench:logsrv-badge-forgery, bench:bench-init-reporter-forged, bench:init-console-forgery

A verdict comes only from a party the attacker cannot impersonate. The console does not say who
wrote a line, and an attacker can print anything, including another program's `PASSED`, so an
attack case passes only on a line the attacker cannot write:

- **The kernel or the loader**, refusing: a line with no `[pid` prefix, with `KMAIN` or a later
  stage forbidden so that no program ever ran (`loader-rejects-*`), or an anchored line the kernel
  prints at boot before any program starts, which a relayed line cannot match (`kernel-wx`).
- **A victim** that owns what is attacked and still has it afterwards: the log server still hears
  UART input after every attempt (`irq-attack`); a victim inspects its pages (`mem-attack`,
  `uaf-lent-page`).
- **The log server's `DONE`.** The program in `init`'s place owns the console: it holds the
  console's device handles, and every other program's text reaches the UART only through it,
  prefixed with the sender's `[pid N]` or `[badge N]`. A victim, or with no victim the attacker
  once it is done, calls the checker's `done()`; the log server prints one `[server] done:` line
  naming the caller by the badge the kernel delivers with the call, which the tester chose, and
  powers off. A case that names a `reporter` passes only if exactly one line starts
  `[server] done:` and it names the reporter's place; any other such line fails it, as does one
  in a case with no `reporter`.
- **`consoled`'s prefix, under `init`.** In a servers' case the real `init` holds the UART until
  `consoled` starts, and `consoled` starts every line written through a child's connection with
  that connection's `[con N]`, so only `init`'s lines are bare. The bench learns the reporter's N
  from `init`'s bare announcement and takes a `TEST PASSED` only under it
  ([the servers' cases under `init`](#the-servers-cases-under-init)).
- **A sole first program judging the kernel.** In `map-fixed-attack`, `write-only-attack` and
  `process-attack` one program runs, alone: it holds the console and the reset, makes every
  refused call itself, and prints its own unprefixed `ok:` lines. It is trusted because nothing
  else runs that could impersonate it, and because what it attacks is the kernel, not another
  party in the case: each `ok:` line reports a kernel result it read back (the error, the budget's
  usage, the mapping left in place), and the case also needs a clean power-off, which a fault in
  the program prevents. The residual: a kernel bug that corrupted this program's own memory could
  make it misreport, and no second party would see it.

**Program order.** A program that attacks another program, or the fixture, is never the first. In a case with several programs, the
first is the trusted tester (the log server, unless the case is one of the sole-program cases
above), and the case's `programs` list fixes the order of the rest, so which place and which
handles each program gets is the case's choice, never a race.

Every verdict pattern is anchored with `^` and pinned to its writer, with a comment beside it saying
why it cannot be forged. The attacker's own lines may be required as progress (so a refusal for the
wrong reason fails) and forbidden as evidence of a breach, but are never the verdict. Where the
attacker itself reports to the checker, the case shows only that the system survived, and its
description says so ("verdict: survival only").

## Cases

### The case file

Status: built · partly tested: that an unknown field or table is refused is read from the code, not attacked by a case · tested: bench:bench-console-after-expect, bench:bench-poweroff-missing, host:testbench::a_case_out_of_the_whole_run_runs_only_by_name, host:testbench::host_tests_route_only_to_a_requested_workspace_and_forward_features, host:testbench::an_oracles_tools_must_be_on_the_path, host:testbench::sweep_seeds_are_ranges_or_lists, host:testbench::a_sweep_is_refused_before_anything_builds, host:testbench::sweep_boots_have_their_own_files, host:testbench::the_join_prints_in_seed_order_and_counts_failures

A case is one TOML file. Paths in it are relative to the workspace root, and an unknown field or
table is an error, so a misspelling cannot silently drop a check. The one exception is a `programs`
entry (and a bundle file's `from`): its forms are told apart by their keys, so an extra key in one
is ignored rather than refused (`tools/testbench/src/case.rs`).

```toml
description = "What this proves"
arch = ["rv64", "rv32"]      # targets to run on
whole_run = true             # false: run only when the filter is the case's name
kind = "boot"
programs = [                 # the first in init's place, the rest started by it
    "log-server",                                  # a binary of the test programs
    { package = "my-crate", bin = "my-server" },   # any workspace binary, built for the target
    { package = "my-crate", bin = "my-probe", features = ["probe"] },  # built with these features, in a target directory of its own
    { package = "beamlet-redoubt", bin = "beamlet", workspace = "userland/otp" },  # a binary of a workspace of its own, built there
    { path = "prebuilt/thing.elf" },               # or a prebuilt ELF
    { erlang = "path/to/module.erl" },             # an Erlang module, compiled by the pinned erlc (a bundle file's)
    { otp = "io" },                                # an OTP module's .beam from the pinned toolchain (a bundle file's)
    { zeros = 6291456 },                           # that many zero bytes, an entry only its length matters for (a bundle file's)
]
smp = [1, 4]                 # one boot per hart count (default [1])
memory_mib = 32              # guest RAM (default 256)
memory = false               # true: stop an init guest and measure its servers' painted stacks
timeout_secs = 60            # default 60; fractions allowed
icount = "shift=3,sleep=off" # virtual time: 2^3 ns per guest instruction, the RTC on it too
qemu_seed = 1                # pin the guest's randomness (QEMU -seed)
kernel_features = []         # extra kernel features
debug_assertions = false     # true: a checked build of the kernel and the loader
expect = ['regex 1', 'regex 2']   # each must match a console line, in this order
forbid = ['regex']                # must never match
poweroff = false             # true: the guest must power off after the last expect
reporter = "checker"         # the program whose done() is the verdict (rule F)
post_check = "sched_oracle"  # a host-side check of the console after a passing boot

[[input]]                    # typed on the console once a line matches `after`
after = "claimed irq 10"
send = "xyz"
```

A `boot` case also takes `allow_panic`, `tamper_bundle`, `sign_bare_archive`,
`distinct_across_boots` and `distinct` ([hostile inputs](#hostile-inputs)), `[[file]]`
([bundle files](#bundle-files)), `[disk]` and `[net]` ([devices](#devices-and-the-network)),
`[[session]]` ([SSH sessions](#ssh-sessions)), `must_fail` ([self-checks](#self-checks)), and
`poweroff_status`: the QEMU exit status a `poweroff` case requires (0 by default; 255 for an SBI
system failure, which a rejection case asks for).

With `icount`, QEMU runs the guest at a fixed instruction rate, skips idle time to the next timer
deadline and puts the RTC on the same clock (`-rtc clock=vm`). A time a case asserts is then a
count of guest instructions (at `shift=3`, 1 ms is 125,000), the same on any host however loaded.
It does not make a run repeat on its own: QEMU fills the guest's boot RNG seed from host entropy on
every boot, the kernel draws PIDs from it, and that moves every later event. `qemu_seed = N` pins
it (QEMU `-seed N`, which fills the device tree's `/chosen/rng-seed`), and with `icount` a run then
repeats exactly. The bench prints the seed before the result, and `TESTBENCH_QEMU_SEED=M` replaces
the pinned seed of every case that has one, to replay a run or to sweep. A case with a disk keeps
`sleep` on (`icount = "shift=3"`): with `sleep=off` an idle guest's clock jumps to the next
deadline, which while `blkd` waits for the host's virtio completion is its own 10 s request
timeout, so the read fails and the volume is poisoned; with `sleep` on, idle time is the host's, so
such a run repeats closely, not exactly; a time a case is held to comes with a sweep's spread.
`--sweep SEEDS` runs one case once per seed (`1..20`, or `3,5,9`), from one build, and prints each
seed's result in seed order and a summary line; each boot keeps its files in `seed-<N>-<arch>/`
inside the run's directory. `--jobs J` boots up to J seeds at once. It is never the default; what a
result under it is worth is the shared-host rule below. A timing gate runs one pinned seed and
states its target from a sweep of seeds ([responsiveness](kernel/scheduling.md#responsiveness)).

**On a shared host.** `cargo testbench` runs its cases one after another; what may run beside
the invocation, another invocation in its own run directory, a seed under `--jobs`, a build, is
decided by the clock each case measures with. A boot case with `icount` measures in guest time:
the host's load does not move a guest time, so its pass, and a failure the guest itself
reports, are verdicts whatever ran beside it (a pinned seed adds only that the run repeats
exactly). Its one exposure to the host's clock is `timeout_secs`, the bench's deadline for the
boot: a case that only ran out of that deadline beside other work has no verdict, and is rerun
alone. A boot case without `icount` (most of them) keeps the host's clock in the guest, so load
lengthens every wait it makes: its pass is a verdict unless what it expects is a timeout or a
bound on a time, and a failure beside other work has no verdict until it fails alone. A `[net]`
table by itself changes neither class: an empty one gives the guest a card that reaches
nothing, and binds no host port. A table with a `forward`, a `poke`, a peer or a dial puts host
sockets beside the boot, in wall time: its dials retry until the case's deadline, so a pass
stands and a failure is rerun alone, as for any host-clock wait; and because the bench picks a
forwarded host port by binding and releasing it before QEMU takes it, two such boots at once
may race for one port, so the rule asks that two such boots not run at once. An `ssh-loopback`
case is the same kind of case: its `sshd` runs in inetd mode through `ssh`'s `ProxyCommand`
and binds no port, its guest's stdio is a virtio-serial port, and the kind's probe (one
loopback session with a 30 s connect timeout) runs once before any case and decides whether the
host can serve the kind, never a case's verdict. So a loopback case that asserts no time (13 of
the 14) passes as a verdict, and a failure that is only its deadline is rerun alone. A case
that measures with the host's clock is a verdict only alone: the rule asks that no other
invocation and no build run beside it. Those are a `host-tests` case whose crates' tests assert
a wall-clock bound (`redoubt-rt`, `redoubt-client`, `redoubt-keyd` and `redoubt-consoled` do;
`redoubt-ipd`, `redoubt-model` and `testbench` only read the clock), which no tolerance would
make load-proof; and a case whose expectation is a timeout (`bench-ssh-guest`, and
`bench-ssh-loopback-deadlock`, whose `must_fail` is the mark it never gets). A
`host-tests` case whose crates assert no bound measures nothing with the host's clock; it may
run beside other work, and the rule asks that its test threads be bounded then
(`RUST_TEST_THREADS`), so that it cannot oversubscribe the host by itself. The kinds that boot
nothing (`build`, `fmt`, `no-cruft`, the size and `unsafe` budgets, the docs checker, the Elixir
oracles) have no clock, and are verdicts anywhere.

A case with `whole_run = false` is left out of a run with no filter and out of one whose filter
is only part of its name; it runs when the filter is its whole name, and `--list` marks it "by
name only". It has one reason: a measurement too long to repeat at every train.

The kinds, and the fields each takes besides `description`, `arch` and `whole_run`:

| Kind | What it does | Its fields |
| --- | --- | --- |
| `boot` | boots the kernel with `programs` as its first processes and judges the run | those above |
| `build` | only checks that a package compiles for each target: coverage for what the bench does not boot | `package`, `features` |
| `host-tests` | runs `cargo test` on the host for the named workspace packages, for what no boot can reach (a constant the loader and the bench share is right in the machine's eyes even when it is wrong); with `miri`, under nightly Miri. These cases are the bench's only host tests; `cargo test --workspace` is not run, though it compiles. The kernel and the test programs have no host tests (`test = false` on their targets) | `packages`, `tests` (the test files to run; default all), `miri`, `workspace`, `features`, `tools` |
| `ssh-loopback` | runs `[[session]]`s against a host OpenSSH server with no guest, to check the session runner on its own | `authorized` (the test keys the server accepts), `[[session]]`, `timeout_secs`, `host_key` (default: the server's own), `server_log` (patterns each of which must match a line of the server's own log), `must_fail` |
| `unsafe-budget` | the ratchet on `unsafe` ([below](#the-unsafe-budget)) | `[[budget]]`: `name`, `paths`, `max_unsafe`, `max_undocumented`; `[[uncounted]]`: `path`, `reason` |
| `size-budget` | the ceiling on each trusted crate's size ([below](#the-size-budget)) | `[[crate]]`: `name`, `paths`, `max_lines` |
| `no-cruft` | the source gate ([below](#the-no-cruft-gate)) | `paths`, `[[forbidden]]` (`pattern`, `unless`, `within`), `no_allow_dead`, `one_definition`, `definition_paths`, `[[allow]]` (`path`, `rule`, `reason`) |
| `fmt` | the formatting gate ([below](#the-formatting-gate)) | `roots`, `[[skip]]` (`path`, `reason`) |
| `elixir` | runs scripts that check an Elixir oracle on beamlet against the Rust it shadows, each of which must exit 0, on the pinned toolchain ([below](#elixir-oracles)) | `otp`, `elixir`, `scripts` (each a path and its arguments), `must_fail` |

For `host-tests`, `workspace` defaults to the repository root and otherwise names a relative
directory below it with a `Cargo.toml`; an absolute path, a path outside the repository or a
missing workspace fails before Cargo runs. `features` defaults to none and passes the named
features to Cargo. For example, `beamlet-lookup-host` runs the VM and Redoubt tests from
`userland/otp` with `beamlet-redoubt/fake`; `beamlet-lookup-cli-host` runs separately without that
feature, so the two cases do not combine their Cargo features. `tools` names host programs the
tests run as an oracle (`erofs-oracle`'s `mkfs.erofs`, `fsck.erofs` and `dump.erofs`): each must be
an executable on the path, or the case fails naming the first missing, and skips it with
`--allow-skip`, as for any other host lack.

A `post_check` judges the console after the boot has passed. `sched_oracle` rebuilds the
scheduler's order from the raw events a tracing kernel prints and checks every pick against its own
reading of the rules ([scheduling](kernel/scheduling.md)); a limit such as `r10_p99_us=30000`
bounds a measured cost. Two arguments judge a sibling's first run: `round`, that it comes before
any other budget is picked twice, and `lift-delay`, that it comes within a round of what the lift
on its parent predicts. A `walk-trace` kernel's walks are bounded by their longest, net of the
audits inside them (`pump_max_us`, `expiry_max_us`, `reconcile_max_us`), judged before
`r10_p99_us`, so `worst-walk`, whose destruction bound must fail, still holds its walks.

### Starting a case's programs

<details><summary>Status: built · tested (7)</summary>

- bench:rng
- bench:budget
- bench:budget-carve-attack
- bench:budget-destroy-kills
- bench:programs-unknown-budget-attack
- bench:programs-unknown-program-attack
- bench:bundle-mapped

</details>

The loader loads only the kernel and `init`
([boot](kernel/boot.md#the-loader-loads-only-the-kernel-and-init)), so a kernel case's first
program is packed as the bundle's second entry and runs in `init`'s place. It gets `init`'s
handoff: the three budgets, the Reset right, every device and the bundle. It is the trusted
tester. When it is the log server, it starts the case's other programs itself, through the
loader stub and from the bundle's pages, each in a budget of its own carved from `system`. Each
program gets the same slots:
- slot 1, the boot endpoint: its receive right for the first program started, a send badged
  with its place for each later one;
- slot 2, the log endpoint, badged with its place;
- slot 3, its own budget;
- slot 4 on, the budgets the case names for it (`budgets = ["system"]`), in the line's order.

Its own budget's handle gives a program nothing it lacks: the pages are already its own to
spend, a carve from it comes out of its own share ([R7 (carving)](kernel/budgets.md#r7-carving)),
and destroying it ends only the program and what it started
([R10 (destruction)](kernel/budgets.md#r10-destruction)), which the tester sees as a `killed`
notice. It lets a program read its own charges and run measured work in a budget it carves. The
budget is `system`-class, as its parent is, so a program may add labels below it; a kernel case
has no server those labels could reach.

The builder tells the tester which programs to start, and with which budgets, in one more data
entry, `programs`. It has one ASCII line per program after the first, in the case's order: the
program's entry name, then the names of the budgets it gets (`root`, `system`, `users`),
separated by spaces. The tester reads it from the bundle's pages, which the loader verified with
the rest, and reads every line before it starts anything. It refuses the boot, and powers off,
on a line that is not printable ASCII, names an entry the bundle lacks or a budget other than the
three, or names a budget twice, and on more programs than `system` has processes. Each
program's own budget gets an equal share of what `system` has free (pages and processes) and
weight 1,000, a driver's, so no program's limits depend on another's name or order. A case that
needs other sizes builds its own tree from the budgets it is given.

The log server badges each program's handles with the program's place in the case, 2 on, and
names programs by that place, because the kernel draws every PID but the first. A case matches a
PID in a kernel line with a pattern. Each program gets an exit endpoint of its own, so the tester
tells their notices apart ([processes](kernel/processes.md)) and prints each under the program's
name and place, with the kernel's PID beside it.

A program the tester starts holds no device, and every page it has is backed, its stack
included: only the process the loader starts has a stack reservation
([memory](kernel/memory.md#backing-and-zeroing)). So a case that needs a device, or a page never
touched, runs its program alone in `init`'s place, where its own budget is `root`.

`init` itself never takes a case's place and has no mode for the bench. The kernel's cases need
`root` and `system`, which no program under `init` may hold
([R33 (no server holds a system budget)](servers/init.md#r33-no-server-holds-a-system-budget)),
so they run a tester in `init`'s place instead.

### The servers' cases under `init`

<details><summary>Status: built · tested (5)</summary>

- bench:init-servers
- bench:init-console-forgery
- bench:bench-init-reporter-forged
- bench:init-boot
- bench:littlefsd-reboot

</details>

A servers' case boots the real `init` with a manifest of its own, packed as the `manifest`
entry, and its test programs are `servers` entries in that manifest. They get no budget handle,
their own included, as no server does
([R33 (no server holds a system budget)](servers/init.md#r33-no-server-holds-a-system-budget)).
`init` prints its own lines bare, and `consoled` starts every other program's line with its
connection id, `[con N] ` with N in 16 lowercase hex digits
([consoled](servers/consoled.md#started-by-init)). A case's `reporter` names a manifest entry, and
the bench reads that entry's connection id only from `init`'s bare line announcing it,
`init: started NAME, console N`; a second such line in one boot fails the case. A restarted
server is announced as `init: restarted NAME, console N`, and the bench never reads a reporter's
id from that line, so a reporter that restarts cannot pass its case. A case that judges a reboot
expects `init`'s reboot line and then the next boot's first line, the loader's. The machine
resets in the same QEMU run, with the same disk, and from that line on the bench reads the
reporter's id afresh from the next boot's announcement, so a case may go on to its verdict in the
next boot; still only one `TEST PASSED` line, in the whole run, may pass it. The case passes only if
exactly one line says `TEST PASSED` and it starts with that `[con N] `; any other such line fails
it, wherever it came from. Such a case ends at its last `expect`, which waits for the verdict,
since no test program holds the Reset right. The test programs are `redoubt-init-programs`
(`tests/init-programs`): `boot-reader` reads a public entry through the root badge at `bootfsd`
its entry is handed, `con-forger` prints its arguments as lines, and the rest are the servers,
clients and drivers of `init`'s restart cases. Since `init` restarts a program that exits, each
parks when it is done, unless its exit is the point of its case.

### The scheduler oracle

<details><summary>Status: built · tested (13)</summary>

- bench:sched-ties
- host:testbench::a_trace_that_keeps_every_clause_passes
- host:testbench::each_broken_clause_is_caught
- host:testbench::a_broken_trace_is_rejected
- host:testbench::the_models_own_ranks_pass
- host:testbench::the_models_broken_ties_are_caught
- host:testbench::lifts_are_recomputed
- host:testbench::weight_changes_are_recomputed
- host:testbench::the_floor_and_the_passes_are_checked_on_their_own
- host:testbench::destructions_are_timed_and_bounded
- host:testbench::audits_are_subtracted_inside_each_window
- host:testbench::shares_are_judged_net_of_audits
- host:testbench::an_unmatched_audit_fails

</details>

The oracle is independent of the kernel's code: it reads what the queue did (woke, requeued, left,
pass changed, picked), never why, and rebuilds the order from the events alone: the lowest pass
first; at an equal pass a budget that woke ahead of one requeued; of two that woke, the later
kernel entry's first, and within one entry the lower id; requeued ones in the order they were
requeued. It recomputes each lift and each weight change from the rule, and lets a pass fall only
at a weight change. It is itself checked against the model's ranks and against traces broken one clause at a
time. The tracing kernel is a test build only
([R23 (no test channels)](kernel/scheduling.md#r23-no-test-channels)).

### Elixir oracles

<details><summary>Status: built · tested (3)</summary>

- bench:elixir-oracles
- bench:bench-elixir-oracles-broken-guard
- host:testbench::another_version_is_refused

</details>

An `elixir` case runs its `scripts` in order from the workspace root, each judged by its exit
status: `elixir-oracles` runs the steward core's reference over its traces
([the trace encoding](servers/steward.md#the-trace-encoding)) and the wire codec's vectors. The
scripts source `userland/otp/tools/oracle.sh`, which puts the pinned toolchain on the path and
runs a module on beamlet. Before any script the case checks the toolchain: the `erl` that
`userland/otp/tools/env.sh` puts on the path must be of the OTP release `otp` (its
`releases/<major>/OTP_VERSION`), and `elixir --version` must say `elixir`. A missing or other
toolchain fails the case even with `--allow-skip`, and no `must_fail` waits for it: an oracle that
does not run catches nothing; the dev image carries both, and `scripts/setup.sh --with-beam`
builds both on your own machine.

## Checked builds

<details><summary>Status: built · tested (14)</summary>

- bench:bench-debug-assertions
- bench:bench-debug-assertions-off
- bench:boot-profile
- bench:boot-profile-unverified
- bench:sched-latency
- bench:sched-budget-churn
- bench:sched-exit-churn
- bench:sched-timer-flood
- bench:sched-carve-return
- host:testbench::audits_are_subtracted_inside_each_window
- host:testbench::shares_are_judged_net_of_audits
- host:testbench::an_unmatched_audit_fails
- host:testbench::cluster_credit_is_the_certified_interior_only
- host:testbench::cluster_lower_witness_counts_the_union_of_outer_bins

</details>

`debug_assertions = true` builds the kernel and the loader (the trusted base, not the programs) with
the workspace's `checked` profile: `release` with debug assertions and overflow checks on. It is the
same kernel checked harder, not a special build. Three things that are silent in a release build
then panic, and every case forbids `PANIC`:
- `core`'s preconditions on raw-pointer calls (`slice::from_raw_parts`, `ptr::read`,
  `copy_nonoverlapping`): a misaligned or null pointer, which is undefined behaviour and silent in a
  release build;
- the kernel's own `debug_assert!`s: every error a call returns must be in that call's row of the
  error table ([the ABI](kernel/abi.md)), which `budget-syscall-attack` exercises over every call
  and thousands of hostile values;
- arithmetic overflow, wherever the kernel or the loader does not use a checked or wrapping
  operation on purpose.

A checked build also runs the kernel's audits, full scans that check the indexes, frame owners and
handle chains after a destruction and when a process object is freed. They hold the hart while
they run, and a release build has none of them. So a latency target or a share, which is measured
in a checked build because the scheduler trace needs one, excludes them: an audit neither fills a
window nor moves the schedule. The scheduler charges an audit's time to no budget and moves the
running slice's end past it, so the thread that ran it is picked and preempted as in a release
build; a kernel built with `audit-billed`, which keeps the old charge, misses the containment
gate's deadline notice, in a recorded negative run. The traced kernel stamps each audit's start
and end (records `U` and `V`: which audit, and the time). The program prints each latency
sample's window, its end on `time_now` and its length (`LATENCY-SAMPLE <group> <measure> <end>
<gross>`) and how many it took of each, so a window lost on the way fails the check, and it
judges none of them. `sched_oracle` subtracts the audit time inside each window,
counting only the part of an audit that falls in it, and then applies the case's bounds
(`deadline_notice_p99_us=40000`). It reports the gross, the net and the audit time beside each
target ([responsiveness](kernel/scheduling.md#responsiveness)). A share is judged the same way:
the program prints its window, the CPU its count stands for and its bounds in thousandths
(`SHARE <name> <start> <end> <cpu> <min> <max>`), and `sched_oracle` judges it of the window net of
the audit time inside it (`sched-budget-churn`'s victim, whose attacker destroys a budget each
slice; `sched-exit-churn`'s, whose attacker's processes start and end; `sched-timer-flood`'s,
beside deadlines; `sched-carve-return`'s, from the return of a carve). An audit runs beside the
work a share counts, never inside it, so the credit stops at the window less the share's CPU and
a net share is never past the whole (`sched-budget-churn`'s deadline victim, which counted
1,960,657 µs of a 2 s window holding 49,077 µs of audits, read 1004 before the cap). The residual:
the program's calibrated CPU count runs at least 0.3% (rv64) and 0.65% (rv32) over the work it
measures, a bias in every share's CPU that the cap now hides behind its `credited` note; the
calibration is the likely source, and it is a follow-up. A case that judges a share
in its program has no audit inside its window. An audit that never ends, ends without beginning
or runs inside a destruction fails the check. The oracle subtracts only what the trace shows it: a
kernel built with `audit-unstamped`, which leaves the audit after a destruction unstamped, misses
the containment gate's deadline notice, in a recorded negative run. The audits themselves stay
full. The cluster's envelopes ([responsiveness](kernel/scheduling.md#responsiveness)) are credited
more strictly: a stamp is a floored microsecond, so an audit stamped u and v ran from somewhere in
`[u, u + 1)` to somewhere in `[v, v + 1)`, and only its **certified interior** `[u + 1, v)` (empty
unless v > u + 1) is subtracted, where it meets the envelope. The uncertain edge bins stay counted
as elapsed time, so the credit is never more than the audit time inside, and an audit outside an
envelope lowers nothing. The credit is summed with checked arithmetic and may not exceed the
envelope. The other cases keep the whole-stamp subtraction above. The same trace records each
timer interrupt from user mode with every charge inside it, and `sched_oracle` checks that the
budget it interrupted pays only for its own items or its slice's end
([charging](kernel/scheduling.md#charging)); a kernel built with `timer-tail-billed`, which keeps
the old billing, fails that check in a recorded negative run. A kernel built with
`alloc-first-fit`, which takes each frame by the first-fit scan of RAM the bitmap replaced,
fails `scan-bounds` on both widths in a recorded negative run
([R12 (scheduling)](kernel/scheduling.md#r12-scheduling)).

Many cases use the profile: every case file with `debug_assertions = true`, most of them over
both widths (`budget`, `budget-syscall-attack`, `lend-untouched-page`, `ipc` and `smp-boot` among
them), and some, such as `all-together`, on rv64 only. The kernel
prints one line under `cfg!(debug_assertions)`: `bench-debug-assertions` expects it, and
`bench-debug-assertions-off` forbids it in an ordinary boot. To check the whole suite:

```sh
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true cargo testbench
```

That run fails `bench-debug-assertions-off`, as it should; everything else must pass.

`boot-stats` is a diagnostic feature like `sched-trace` and `walk-trace`, but of programs, not the
kernel: a Cargo feature of `init`, `blkd`, `verityd`, `littlefsd`, `erofsd` and `beamlet-redoubt`, off by
default and never in the image's build. Without it the release binaries of `init`, `blkd`,
`verityd`, `littlefsd`, `erofsd` and `beamlet` have the section sizes they had before it, on rv64 and rv32
(measured with `llvm-size` against the commit before it, both built from the same path: `beamlet`'s
sizes move with its build path). With it, `init`'s lines and beamlet's line for its first object
carry `[t=N]`, `time_now` in µs; `blkd`, `littlefsd` and `erofsd` say their counts at each power
of two of their requests from 2^12 and `verityd` from 2^7, and `littlefsd` and `erofsd` once more, exactly, on a
walk of
`Elixir.BootStats.beam`, a name no volume holds; and at the VM's first console read beamlet says
`beamlet: first console read [t=N]` and what its lookups cost: their count and the guest time spent
in them ([beamlet on Redoubt](userland/beamlet.md#beamlet-on-redoubt)). The feature needs no
checked build. `boot-profile` and `boot-profile-unverified` boot the image's programs with it under `icount` and a pinned seed: measurements, which bound only the boot's time
to its prompt ([the boot-time target](userland/beamlet.md#beamlet-on-redoubt)).

## Hostile inputs

<details><summary>Status: built · tested (6)</summary>

- bench:loader-rejects-kernel-address
- bench:loader-rejects-kernel-entry
- bench:loader-rejects-truncated-elf
- bench:verified-boot-rejects-tamper
- bench:verified-boot-rejects-bare-archive
- bench:rng

</details>

A program entry can be a good binary the bench corrupts before injecting it:

```toml
programs = ["log-server", { corrupt = "rng-test", with = { segment-vaddr = "0xffffffffffd00000" } }]
```

`with` is `segment-vaddr`, `entry` or `truncate` (a byte count smaller than the file).
`tamper_bundle = true` flips one bundle byte after signing, and `sign_bare_archive = true` signs the
archive without its domain and length; the loader must refuse both
([R15 (verified boot)](kernel/boot.md#r15-verified-boot)). A case that expects a deliberate panic
sets `allow_panic = true`, which drops only `PANIC` from the always-forbidden list, and forbids what
must not happen instead. `distinct_across_boots = ['id: (.*)']` boots twice and requires the captured
text to differ; `distinct = ['console (.*)']` requires, within one boot, two or more lines it
matches and no capture twice, such as a restarted program's new console. A hostile program is an
ordinary `programs` entry; hostile data for a program to use, such as a malformed ELF for a parent
to launch, is a bundle file.

## Files in the bundle

### Bundle files

Status: built · tested: bench:bench-bundle-file, bench:programs-unknown-budget-attack, bench:littlefsd-reboot, host:testbench::a_file_s_servers_merge_into_its_manifest_by_name

```toml
[[file]]                     # a data entry, after the programs
name = "trace"
from = { path = "tests/data/bundle-file.txt" }   # or any `programs` form, corrupted ones included

[[file]]                     # a manifest, with entries merged into its `servers`
name = "manifest"
from = { path = "tests/data/littlefsd/boot.json" }
servers = [{ name = "client", args = ["reboot", "littlefsd:data"] }]

[[file]]                     # a manifest pinning the userland disk's root, from the run's pack
name = "manifest"
from = { path = "image/manifest.json" }
verity = "image/userland.toml"
```

A file read from a path may be a manifest with `servers` entries merged in by name: each replaces
the members it gives in the entry of its name, or is added after the rest if none has it, so cases
that differ in one entry share one manifest, and a case can boot the image's own manifest with a
client added.

Entry names must differ from each other and from `kernel`. A file named `programs` replaces the
listing the builder writes, so an attack case can hand the tester a hostile one. The bench
refuses a `[[grant]]` table: a program reaches a device only through the handles it is given
([R18 (device authority)](kernel/devices.md#r18-device-authority)). The loader starts only the
kernel and the program in `init`'s place, so a data entry reaches the guest signed and is never
run. `bench-bundle-file` reads its entry back: the program in `init`'s place finds it in the
bundle's pages and compares it, byte for byte, with the file it was built with.

### Data entries for `init`

<details><summary>Status: built · partly tested: no model trace is replayed under `init` yet · tested (1)</summary>

- bench:init-servers

</details>

`init` receives the verified bundle and hands the public entries to `bootfsd`, and a data entry
is how a model trace reaches an in-guest replayer, which compares results itself and prints its
verdict ([boot](kernel/boot.md#what-init-does-with-the-bundle)). The program in `init`'s place
already reads a data entry from the bundle's pages ([bundle files](#bundle-files)); under `init`,
a program reads it again through `/boot` once the manifest's `public` list names the entry.

## Devices and the network

### Disks and network cards

<details><summary>Status: built · tested (22)</summary>

- bench:bench-virtio-devices
- bench:bench-virtio-legacy-off
- bench:erofs-corrupt
- bench:erofs-read-only
- bench:image-disk
- bench:init-boot
- bench:net-tcp
- bench:userland-boot
- bench:userland-read-only
- bench:verity-flipped-tree
- bench:verity-wrong-root
- host:testbench::a_recipe_can_generate_a_directory_of_files
- host:testbench::a_recipe_packs_a_table_and_a_volume_per_partition
- host:testbench::a_stage_is_walked_parents_first_in_name_order
- host:testbench::devices_sit_on_fixed_slots
- host:testbench::every_network_is_restricted
- host:testbench::virtio_devices_are_modern
- host:testbench::the_userland_disk_sits_on_its_slot_read_only
- host:testbench::the_userland_pack_is_deterministic_and_stages_each_object_by_name
- host:testbench::a_verified_partition_is_its_volume_then_its_tree
- host:testbench::an_erofs_partition_is_its_stage_and_each_damage_is_corrupt_where_it_is
- host:testbench::the_manifest_pins_the_packs_root

</details>

```toml
[disk]                       # a virtio-blk disk, zeroed, created afresh for every boot
size_kib = 4096
partitions = 1               # optional: a GPT of this many equal partitions, by blkd's builder
# or, instead of both: a disk recipe packed for every boot as ./mkimage packs it
# recipe = "image/disk.toml"
# stage = "tests/data/littlefsd/stage"   # optional: what every partition holds instead of its stage

[userland]                   # the userland disk, attached read-only, packed once per run
recipe = "image/userland.toml"
# flip = "Elixir.Enum.beam"  # optional: one bit of this file's data flipped on the disk
# flip_tree = true           # optional: one bit flipped in the first level-1 tree block
# wrong_root = true          # optional: the manifest pins a root one digit off the pack's

[net]                        # a virtio-net card on QEMU's user-mode network
forward = [22]               # guest TCP ports reachable from the host (default: none)
host_key = "ssh-ed25519 AAAA..."   # optional: the only SSH host key sessions accept
```

A disk `recipe` (`image/disk.toml`) is packed by the code `./mkimage` runs (`testbench
--pack-disk`): a GPT of equal partitions by `blkd`'s builder, then each partition as its `fs`
says, holding its stage's tree: `littlefs`, a writable volume written through `littlefsd`'s own
code, or `erofs`, a read-only volume written by `libs/erofs`'s writer
([erofsd](servers/erofsd.md#the-packer)), so a case boots the disk the image ships.

A recipe's littlefs or erofs partition may also generate files, for a case that needs many and not their
contents: `generated = { files = 600, read = "f000" }` makes `f000` to `f599` in the volume's root
(as many digits as the last needs), all empty except `read`, which holds its own name and a
newline. They sit beside the stage's tree, if there is one, and a name in both is refused.

A littlefs or erofs partition may be verified, `verity = true`: it holds the largest volume that
fits beside its hash tree, then the tree ([verityd](servers/verityd.md#the-tree)), and the pack
says its root and data blocks. An erofs volume is followed in its range by zeros, which the tree
covers too.

For `erofs-corrupt`, an erofs partition may be damaged after its pack, through the parser
`erofsd` uses to find the place: `damage = { what = "magic" }` flips a bit of the superblock's
magic; `{ what = "block-past-count", path = "tail.txt" }` starts that file's blocks at the
volume's block count; `{ what = "compressed", path = "motd" }` lays that file out compressed;
`{ what = "name-offset", path = "lib" }` starts the last name of that directory's first block past
the block's end.

The userland disk (`image/userland.toml`) is packed by the same code: first its objects are
staged, each module of the applications the recipe names, compiled by the pinned toolchain and
stripped, as a plain file under its own name, then its one partition is packed as a verified
volume of those files. The bench stages and packs it once per run: a bundle file read from a path
with `verity = "image/userland.toml"` is a manifest, and the builder writes the pack's root and
data blocks into the `verity` of the `volumes` entry of the partition's name, so the signed
manifest pins the disk; `[userland]` attaches the same pack. A case's damage goes on its own copy
of the disk after the pack, never on the root: `flip` flips one bit of a file's data where the
volume holds it, and `flip_tree` one bit of the first level-1 tree block. `wrong_root` changes the
manifest's root by one digit instead, never the disk
([R76 (verified volumes)](servers/verityd.md#r76-verified-volumes)). Nothing is generated beside the objects.

Devices use virtio-mmio's modern transport, which `blkd` and `netd` require. Each sits on a fixed
virtio-mmio slot, the one `image/manifest.json` names: the card at `0x10007000` with interrupt 7,
the data disk at `0x10008000` with interrupt 8, and the userland disk at `0x10006000` with
interrupt 6, whether or not the case has the others. So a case's manifest names its devices as the
image's does. The userland disk is attached read-only (`readonly=on`): QEMU refuses every write
to it, so nothing on the box can change it. The guest reaches nothing outside QEMU
(`restrict=on`, checked for every `[net]` case): there is no outside peer, only forwarded
connections coming in, unless a case adds one deliberately. Nor is it offered IPv6 (`ipv6=off`),
which it does not speak: slirp would otherwise send it router advertisements. Each boot gets its
own host ports, chosen by the operating system, so benches running side by side do not collide.

### Peers, dials and the capture

<details><summary>Status: built · tested (13)</summary>

- bench:bench-net-peer
- bench:bench-net-peer-twice
- bench:bench-net-peer-count
- bench:bench-net-peer-pcap-empty
- bench:netd-restart
- host:testbench::a_poke_gets_a_udp_forward
- host:testbench::peers_are_judged_on_records_and_capture
- host:testbench::a_capture_is_read_fail_closed
- host:testbench::what_the_guest_sends_is_checked
- host:testbench::records_are_counted_per_peer
- host:testbench::a_dial_needs_its_echo
- host:testbench::prefixes_and_peers_are_checked
- host:testbench::peers_get_the_wider_network_and_a_capture

</details>

A network case can give the guest hosts to reach, and judge what it sent, all from outside the
guest, after the boot has passed:

```toml
[net]
forward = [8000]
self_forbidden = ["10.0.2.0/24", "127.0.0.0/8"]   # the guest must never send a SYN to these

[[net.peer]]                 # a host the guest may connect to, and exactly how often
addr = "10.0.9.100:7"
connections = 1

[[net.dial]]                 # the bench connects in through a forwarded port and needs the echo
port = 8000
send = "hello\n"
expect = "hello\n"

[net.poke]                   # one UDP datagram into the guest, sent when a console line matches
port = 47000
payload = "redoubt netd restart probe"
after = '^\[con [0-9a-f]{16}\] judge knows netd.s instance: poke it$'
```

- **Peers** are QEMU `guestfwd`s to a program: for each connection the guest makes, the bench's own
  binary starts as a helper on it, records the connection as a file before it echoes a byte, and
  then echoes. After the boot each peer's count must equal its `connections`. There is no host
  listener another process could take.
- **The poke** is one UDP datagram, sent once, through a host port QEMU forwards to its guest port,
  when a console line first matches its `after`: a trigger from outside that nothing resends, as
  TCP would. `netd-restart` faults `netd` with it, through a test-only feature.
- **The network.** A case with peers widens slirp's network to a /16 in which its host, resolver
  and the guest's address stay where they were; the peers sit outside the /24 the guest is
  configured for, reached through its gateway. `restrict=on` still holds.
- **The capture.** A case with peers records every frame on the guest's card, before slirp, and
  reads it fail closed: missing, empty, cut or malformed fails the case (`net.truncate_capture`
  cuts it on purpose, for the self-checks). The guest may send only ARP requests for the gateway,
  ARP replies (to any address) and IPv4 TCP, never a fragment and never a SYN to a `self_forbidden`
  prefix. Each peer's count must also match the distinct SYNs to it in the capture, and a case with
  peers needs one that expects a connection, whose SYN shows the capture was live.

The guest's own claims about the network are never trusted.

## SSH sessions

### Sessions and the loopback server

<details><summary>Status: built · partly tested: no guest `sshd` exists yet to log in to · tested (16)</summary>

- bench:bench-ssh-loopback
- bench:bench-ssh-loopback-openssh
- bench:bench-ssh-loopback-deadlock
- bench:bench-ssh-loopback-forbid
- bench:bench-ssh-loopback-exit
- bench:bench-ssh-loopback-host-key
- bench:bench-ssh-loopback-aborted-text
- bench:bench-ssh-guest
- host:testbench::the_reference_proxy_quotes_only_what_the_bench_chose
- host:testbench::the_cpio_writer_writes_newc
- host:testbench::the_guest_recipe_parses_and_hashes
- host:testbench::a_bad_package_is_broken_and_no_network_is_the_hosts
- host:testbench::the_keeper_finds_what_names_the_case
- host:testbench::the_keeper_waits_to_the_deadline
- host:testbench::resize_needs_a_pty
- host:testbench::no_other_child_inherits_a_sessions_terminal

</details>

Sessions need `net.forward = [22]`. They start once every `expect` has matched and run concurrently
while the bench keeps watching the console. Each drives the host's OpenSSH `ssh`, an implementation
independent of the box's, to the guest's port 22 with a test key.

```toml
[[session]]
user = "alice+secrets"       # the login name, unique in the case
key = "alice"                # optional; defaults to `user` up to any '+'
pty = false                  # true asks for a terminal
forbid = ['bob-secret']      # never in this session's output, until ssh exits
steps = [
    { expect = '\(1\)> ' },     # wait for the prompt
    { send = "File.read(\"/work/x\")\n" },
    { mark = "alice-ready" },    # tell other sessions this one got here
    { wait = "bob-ready" },      # wait for another session's mark
    { resize = [132, 43] },      # with `pty = true`: resize ssh's terminal
    { exit = 0 },                # close input, read until ssh exits, require this status
]
ssh_args = ["-W", "host:9"]  # optional: more ssh arguments, before the host
command = "echo hi"          # optional: a command (with `-s`, a subsystem) in place of a shell
```

Every session's exit status is checked and all of its output passes `forbid`; a session that fails
stops the others. A `pty = true` session's `ssh` reads its input from a pseudo-terminal the bench
opens at 80x24, so that `ssh` reports a size change as OpenSSH does for a user; its output stays on
pipes. `resize` sets that terminal's size and signals `ssh` (`SIGWINCH`), which then sends a
`window-change` if the size changed. The terminal is the client's input and never a verdict. With `net.host_key` set, `ssh` refuses any other host key. Test keys live in
`tests/keys/`; they are public and marked not for production, and a boot manifest that lists one
must never ship. The `ssh-loopback` kind runs sessions against a server that `ssh` starts itself
for each session as its `ProxyCommand`, so nothing listens on a port: Redoubt's `sshd` on its host
platform ([against Redoubt's sshd](#against-redoubts-sshd)), or with `server = "openssh"`
OpenSSH's server in inetd mode, in a QEMU guest. Its log goes to a file beside the transcripts,
never to `ssh`'s output, so a late line of the server's cannot stand in for the session's last
output; a case's `server_log` asks what the server saw (a refused key, say), not only what the
client printed, and its `server_log_forbid` what the server must not have done (a console
started, say).

**OpenSSH's server runs in a QEMU guest.** On a host that enforces SELinux, `sshd` moves every
login shell into the user's default context, and a bench running in a service's context may not
enter it, so the host's own `sshd` cannot serve the reference case. A container needs a runtime
and user namespaces that the bench's host may refuse. A guest under the bench's own
`qemu-system-riscv64` needs neither; the client, the session runner and the case are unchanged,
so the case is still a witness independent of Redoubt's server. The guest is not Redoubt, and it
boots QEMU's own OpenSBI: the rule that only RustSBI boots is Redoubt's.
- **The image** is a Linux kernel and an initramfs built from `tests/ssh-reference/guest.toml`:
  Debian's riscv64 kernel, `openssh-server`, the libraries it links and `busybox-static`, each
  pinned by version and sha256 in one timestamp of `snapshot.debian.org`, which keeps every
  version it has published; with them the guest's `/init`, its logins (root and `sshd`'s
  privilege-separation user), the kernel module it loads (`virtio_mmio`) and what it leaves out,
  each with its reason. The bench fetches each package with `curl`, checks its sha256, unpacks it
  with `dpkg-deb`, and writes the initramfs's `newc` archive itself. It keeps the image in
  `target/ssh-reference/` under the first 16 hex digits of the recipe's sha256, and builds it only
  when that is missing, so only the first run after a change to the recipe needs the network.
- **Each session** boots its own guest: `ssh`'s `ProxyCommand` is QEMU with no network and no host
  filesystem. The case's configuration and keys reach the guest in its initramfs: the bench writes,
  for each case, the image's archive followed by an archive of the case's files. The session's stdio
  is a virtio-serial port; the guest's `/init` runs `sshd -i` on it, reading and writing one open of
  the port, and powers off when `sshd` is done. Every `sshd` process opens its log anew and a port
  takes one open at a time, so they share a FIFO that one reader carries, a line per write, to a
  second port, which QEMU appends to the case's log: a line of up to 4 KiB, the most a FIFO takes in
  one write, reaches the log whole among other sessions' lines. The guest's console goes to
  `guest.log` beside it, for people reading a failed run, and is never a verdict; a failed boot
  powers off rather than reboots. The server logs in only root, inside the guest, runs `/bin/sh` for
  every login and allows nothing else.
- **ssh ends each guest.** Once the session ends, `ssh` exits without waiting for the server and
  hangs up on its proxy. What `sshd` logs after that is lost; the lines the cases ask for come
  earlier. `ssh` has no parent-death signal, so a bench killed outright leaves it running until
  its session ends, and its guest with it. **The keeper:** once a case's sessions have all exited,
  any `qemu-system-riscv64` whose command line names the case's directory (the run's own) has
  until the case's deadline, and at least five seconds, to go, as a loaded host may slow a
  guest's shutdown with nothing wrong; one still running then is killed, and the case fails,
  naming it, unless a session has already failed it.
- **Before the first OpenSSH loopback case** the bench logs in once and runs `exit 0`, and the
  server's log must name OpenSSH's version, so that no other server can pass for it. Its `ssh` waits
  at most 30 seconds for the server's banner, so a guest that never boots fails the probe rather
  than hanging the bench. Without a `qemu-system-riscv64` the bench can use, without `curl`,
  `dpkg-deb` or `xz` to build the image, or with no image and `snapshot.debian.org` not answering,
  every OpenSSH loopback case fails with that reason, as on a host without OpenSSH, and
  `--allow-skip` skips them. A package whose sha256 is not the recipe's, a fetch that fails while
  the snapshot answers, a build that fails and a probe that fails all fail every OpenSSH loopback
  case. The check connects directly: on a host whose only way out is an HTTP(S) proxy that `curl`
  uses, a fetch that fails there finds the snapshot silent, and is a host lack that `--allow-skip`
  skips.

### Against Redoubt's sshd

<details><summary>Status: built · partly tested: agent forwarding is refused inside `sunset`, which no case sees, since `ssh` asks for it without a reply ([residual risks](servers/sshd.md#residual-risks)) · tested (11)</summary>

- bench:sshd-loopback-logins
- bench:sshd-loopback-r67
- bench:sshd-loopback-interrupt
- bench:sshd-loopback-independent
- bench:sshd-loopback-window-change
- bench:sshd-loopback-window-change-zero
- bench:sshd-loopback-env-refused
- bench:bench-ssh-loopback
- bench:bench-ssh-loopback-host-key
- bench:sshd-host-tests
- host:testbench::an_ssh_lacking_an_option_is_named

</details>

`ssh-loopback` cases run the host's OpenSSH `ssh` against Redoubt's own `sshd` on its host
platform, `redoubt-sshd-host` ([the core and its platforms](servers/sshd.md#the-core-and-its-platforms)),
which the bench builds and `ssh` starts as its `ProxyCommand`. Its host key is `loopback-host`,
and each of the case's `authorized` keys is a principal of the same name. Nothing there needs a
shell, a login context or a guest.

- **The self-checks run on it,** their steps written for its scripted console, and their
  `server_log` patterns for its log.
- **Cases, verdicts from `ssh`'s exit status and the server's log:** a login key the server's
  `keyd` holds is refused and never reaches the login table, and so is an unknown principal;
  `alice+secrets` gets the labels `{alice-secrets}`; on a labelled channel a shell without a
  pty, `exec`, a subsystem, and remote and local port forwarding are refused, and no console
  starts ([R67 (a channel keeps its labels)](servers/sshd.md#r67-a-channel-keeps-its-labels)),
  and the log names each refusal, the shell's as `shell-without-pty`;
  a break reaches the console as its interrupt; one connection's end leaves another's session
  running; a host key other than the case's is refused. A resized terminal's window change
  reaches the console as its new size, and one to no size (0x0) is refused and never reaches it.
  `env` is refused, and the log names only the request's kind, never the client's name or value.
  `ssh` wants no reply to either refusal, so the server's log is the whole evidence: each case
  requires the refusal's line and forbids its opposite.
- **One reference case stays on OpenSSH's own `sshd`,** with `server = "openssh"`: concurrent
  sessions, marks, exit statuses, a pty and a refused key. It is the session runner's independent
  witness, so a bug the runner shares with Redoubt's server cannot pass every self-check
  (`bench-ssh-loopback-openssh`). Its server runs in a QEMU guest
  ([sessions and the loopback server](#sessions-and-the-loopback-server)).
- `ssh` gets `WarnWeakCrypto=no-pq-kex` against Redoubt's server, whose exchange is not
  post-quantum: OpenSSH's warning would otherwise be session output. OpenSSH 10.1 introduced the
  warning and the option together, so the bench has `ssh -G` parse the option once per run, with
  no configuration file as the sessions run it, and passes it only to an `ssh` that takes it: an
  older one has no warning to quiet, and none to filter from its output (OpenSSH 9.6 prints
  nothing over such an exchange). When it drops the option, the run's output says why.

## Self-checks

<details><summary>Status: built · tested (14)</summary>

- bench:bench-attack-forgery
- bench:bench-console-after-expect
- bench:bench-poweroff-missing
- bench:bench-reporter-mismatch
- bench:bench-init-reporter-forged
- bench:bench-debug-assertions
- bench:bench-debug-assertions-off
- bench:bench-net-peer-twice
- bench:bench-net-peer-count
- bench:bench-net-peer-pcap-empty
- bench:bench-net-self-unrefused
- bench:bench-cbo-self-unrefused
- bench:bench-qemu-early-exit
- bench:bench-elixir-oracles-broken-guard

</details>

The harness can fail, and each feature shows it. Cases named `bench-*` check the bench itself:
each feature has a case that passes only if the feature works and, where the bench can be
misconfigured on purpose, a case that must fail:

```toml
must_fail = '^regex$'        # passes only if the run fails with a matching reason
```

`must_fail` is judged against the run's verdict only (console, sessions, devices); a build error or
the bench's own trouble is a failure regardless, and so is an `elixir` case's toolchain. Each case
writes its pattern anchored and quoting the evidence, so it cannot pass by failing for some other
reason; the bench does not enforce the anchoring, so a reviewer checks it. An attack case can have
a self-check of its own: `bench-net-self-unrefused` runs `net-attacks`'s boot with `ipd` not told
one of the box's addresses, and must fail on the SYN the capture then shows.
`bench-cbo-self-unrefused` runs `cbo-user-fault`'s boot with a kernel that leaves `senvcfg`
permissive: all four cache-block operations must be seen running, and the run must fail on the
verdict line that never comes.
`bench-elixir-oracles-broken-guard` runs the steward core's reference with its `not_locked` guard
held always, and must fail on the first event where its output leaves the core's
([the trace encoding](servers/steward.md#the-trace-encoding)).

## The unsafe budget

<details><summary>Status: built · tested (13)</summary>

- bench:unsafe-budget
- host:testbench::actual_source_counts_still_enforce_the_budget
- host:testbench::empty_configuration_is_not_coverage
- host:testbench::every_configured_root_must_contain_rust_source
- host:testbench::missing_paths_fail_regardless_of_extension
- host:testbench::unreadable_source_reports_its_path
- host:testbench::broken_nested_symlink_is_not_silently_skipped
- host:testbench::zero_unsafe_source_is_valid_as_a_file_or_nested_directory
- host:testbench::every_on_target_source_is_in_a_budget
- host:testbench::a_long_safety_block_directly_above_justifies
- host:testbench::a_raise_needs_its_unsafe_budget_line
- bench:rt-miri
- host:testbench::a_miri_case_runs_its_files_under_miri

</details>

`unsafe-budget.toml` lists every source directory of the trusted computing base that runs on the
target, each with the most uses of `unsafe` it may hold and the most that may lack a justification
(zero everywhere): a `// SAFETY:` comment above an `unsafe` block, and a `# Safety` section in the
doc comment of an `unsafe fn` or `unsafe impl`, in the comment block directly above or within a
few lines. The case counts both and fails if either is over.
Budgets only go down. Raising either limit, dropping a budget or narrowing its paths needs a line
`Unsafe budget: <name>: <reason>` in the commit that does it, which the case reads the way the
size budget reads its own ([below](#the-size-budget)), with the same history check.

A configured path with no Rust source in it fails. So does coverage left out: every workspace
member that can be built for the target (its crate root is `no_std`) must have each of its Rust
sources in some budget, unless the file lists it under `[[uncounted]]` with a reason (the test
programs, the model, host tools, vendored code), and an `uncounted` entry that names no such
member fails too. Vendored third-party crates are outside the ratchet
([below](#vendored-dependencies)). So are crates outside the workspace: the gate reads only the
workspace's members, and `userland/otp` is its own workspace whose `no_std` crates (`re`, `crypto`,
`vm`) no budget counts. They are not in the trusted computing base; if one ever joins it, it joins
the workspace and the gate sees it.

The ratchet counts `unsafe`; it does not check it. `rt-miri` runs the native runtime's host tests
under Miri (Stacked Borrows, isolation off), which checks the heap's free lists, page buffers,
lends and device registers that a native run only executes. It runs the files that finish in
seconds, among them `registers` over the fake kernel's real device memory, and the heap's own,
`heap`, in under a minute (fewer random rounds under Miri); `ipc` and `echo` take minutes under
Miri and are run by hand. Without nightly Miri the case is missing, not passed.
With the runtime's page-buffer aliasing fix reverted, `mapping_views` fails it with the Stacked
Borrows error `not granting access to tag <wildcard> because that would remove [Unique for <…>]
which is strongly protected`.

## The size budget

<details><summary>Status: built · tested (12)</summary>

- bench:size-budget
- host:testbench::only_code_lines_count
- host:testbench::a_test_module_does_not_count
- host:testbench::a_test_module_file_does_not_count
- host:testbench::a_shipped_file_always_counts
- host:testbench::a_form_the_case_cannot_follow_fails
- host:testbench::a_raise_needs_its_reason
- host:testbench::the_ratchet_reads_the_commit_that_raised
- host:testbench::a_merge_is_judged_against_its_first_parent
- host:testbench::merged_history_is_not_read_again
- host:testbench::a_new_budget_file_needs_every_reason
- host:testbench::a_deleted_budget_file_fails

</details>

The size of the trusted computing base is budgeted, not observed
([the tenets](TENETS.md)). `size-budget.toml` lists each trusted crate (the kernel, the loader,
the stub, the libraries they and the servers link, the model and the servers) with a ceiling in
lines of code: every line of every `.rs` file under its paths that is not blank, a `//` comment
(doc comments included) or inside a `/* */` comment. Tests are left out, so writing them costs a
crate nothing: a `#[cfg(test)]` item counts for nothing, and neither does a file only test modules
reach (`mod tests;`, the modules it declares in turn, and those declared inside an inline test
module). A file any other module declaration reaches counts, whatever else names it; a module is
found by its name, its enclosing inline modules and its `#[path]` or `cfg_attr` path, and one whose
file is not under the crate's paths fails the case. So does a form the case does not follow, which
could reach a file past a test's `#[path]` to it: a `mod` whose name is not a plain identifier
(`r#name`, a macro's `$name`) and the word `include`, however it is invoked. The case fails when
a crate is over its ceiling. The ceilings started at each crate's size when the case landed and only
fall: the case reads the commits that changed its file on the branch it runs on, merges included,
and where one raised a ceiling over the file in its first parent, dropped a crate (a rename drops
the old name) or narrowed a crate's paths (fewer lines counted under the same ceiling), requires a
line `Size budget: <crate>: <reason>` for each such crate, in its message or, for a merge, in a
commit it brings in. A commit whose parent lacks the file is judged against the last version
before it; where there is none, as for a file the main branch lacks (new or renamed), every entry
in it needs its line. A commit that deletes the file fails the case, so deleting it and adding it
back raised cannot escape. A raise not yet committed fails, and so does a path with no Rust source in
it; uncommitted changes do not stop the committed ones being judged.

A branch is judged on its own commits: those since its merge-base with the main branch
(`redoubt`, or `origin/HEAD` in a clone that has no such branch), so a package is judged before it
merges and merged history is never read again. On the main branch itself only a raise not yet
committed is judged. With neither branch to measure from, as in a shallow clone that lacks the
merge-base, the case fails rather than passing unchecked.

The residual: a macro defined outside the budgeted paths, a dependency's `macro_rules!` or a
proc macro, can emit `mod x;` or `include!` at a call site, and the case reads only the call. No
trusted crate's dependencies are known to, and adding one is a `Cargo.toml` change reviewed as
part of the trusted computing base
([tenet 5](TENETS.md#5-dependencies-are-part-of-the-trusted-computing-base)).

## The memory budget

Status: built · tested: bench:init-boot, bench:userland-boot, bench:userland-read-only, bench:beamlet-footprint, bench:init-refuses-stack, bench:memory-host-tests

A boot case under `init` can set `memory = true`. After its console verdict the bench stops
QEMU over QMP and dumps the guest's physical RAM beside the case's log, as
`<case>-<arch>-smp<N>.ram`. The runtime writes each server's heap record before `main`, so the
record is the witness that a server has started: a dump missing a declared server's record lets
the guest run on and is taken again, until every record is present or the case's deadline
passes, when the case fails. A server's peak is therefore the most pages its heap had held when
the last declared server had started, or later, never earlier than the stop line. A wait is
printed first, as `memory: dumped twice, waited S s for NAMES`. The launcher has painted each
server's first-thread stack with a tag for that server and an index for every eight-byte unit.
The bench scans the RAM for those tags at their encoded offsets within physical pages. That
ignores paint words copied into ordinary stack slots, while refusing a missing server, a
duplicate unit or an out-of-range index at an encoded offset. A stack page's units are counted
from the one physical page that holds the most of them, so a stack word copied into a buffer or a
message is not a duplicate. The lowest missing unit marks the stack's deepest touched point; two
pages holding equally many of a stack page's units are refused for that unit's stack page or one
below it, and ignored above it, where either leaves the peak where it is. The bench prints
`stack NAME PEAK of PAGES pages` for each server and fails when the declared pages are less than
twice the measured peak rounded up to a page. The dump, the size of the guest's RAM, is deleted
once scanned; a scan that fails on a duplicate or out-of-range unit, or cannot read the dump,
keeps it as the evidence. The QMP socket is always removed. It is in the temporary directory,
under a name another user can predict, and QEMU creates it under the bench's umask, so it is
private to the bench's user only under umask 077 or on a single-user host
([a private directory for the QMP socket](todo/qmp-socket-private-dir.md)). The manifest's
`stack_pages` defaults to 16 and cannot exceed 128
([the boot manifest](servers/init.md#the-boot-manifest)).

Each server's runtime also keeps a heap record, 32 bytes in its data: a magic word, the server's
launch tag, its heap cap and the most pages its heap has held at once
([the native runtime](userland/native.md#redoubt-rt-the-native-runtime)). The runtime writes the
magic word and the tag before `main`, so the program's file and the launcher's copies of it hold
none. The same scan finds each server's record by its magic word and tag and prints
`heap NAME PEAK of CAP pages`, or `heap NAME PEAK pages uncapped`. It fails a missing or
duplicated record, a record whose cap is not the manifest's `heap_pages`, and a capped server
whose cap is less than twice its peak; an uncapped server is reported only.

The standard image is scanned after `init-boot`, `userland-boot`, `userland-read-only` and
`beamlet-footprint` on both widths. Its server declarations use twice the largest peak across six
such runs: each stack rounded up to pages, from the runs that sized the stacks, before the heap
caps existed; and each heap cap in pages, from six later runs with the stacks as declared. The
stack columns below are the first runs' and the heap columns the later runs'. `beamlet`'s heap peak
moves by a page between runs, so its cap is instead the most its budget holds beside its stack,
20,846 pages: at least twice its largest peak across six runs of each memory case on each width,
`beamlet-footprint` included, 72 pages over twice it. Its budget, 20,864 pages, is that cap and its
stack rounded up to 128, not to 1,024, because at 512 MiB it must lie between 20,792 pages (a cap
of twice the peak) and 20,990 (a case that adds a 256-page client still fits on rv32); the image
then has 383 pages to spare on rv32, a client case 126
([budgets](kernel/budgets.md#the-tree-from-the-boot-manifest)). The read-only case also scans its
additional client from the merged manifest. `erofsd:system`'s row and
`verity:system`'s heap, which holds 4 checked data blocks, are from the six runs with the userland
volume on EROFS.

| Image server | Largest stack peak (bytes) | Declared stack (pages) | Largest heap peak (pages) | Heap cap (pages) |
| --- | ---: | ---: | ---: | ---: |
| `keyd` | 6,248 | 4 | 4 | 8 |
| `consoled` | 9,112 | 5 | 9 | 18 |
| `bootfsd` | 7,304 | 4 | 28 | 56 |
| `blkd` | 4,504 | 3 | 17 | 34 |
| `netd` | 4,280 | 3 | 2 | 4 |
| `ipd` | 8,040 | 4 | 4 | 8 |
| `littlefsd:data` | 7,176 | 4 | 9 | 18 |
| `blkd:system` | 4,504 | 3 | 17 | 34 |
| `verity:system` | 7,864 | 4 | 50 | 100 |
| `erofsd:system` | 9,704 | 5 | 12 | 24 |
| `beamlet` | 33,240 | 17 | 10,387 | 20,846 |

The read-only client's largest stack peak is 6,616 bytes and its heap's 31 pages; its case uses
the 16-page stack default and no cap. `beamlet`'s heap peak is the shell after the commands
`userland-read-only` types (6,048 pages on rv32), above what its prompt holds
([beamlet](userland/beamlet.md#what-the-vm-holds-at-its-prompt)); its cap leaves its process heap
and ETS limits, a sixteenth of its budget each (1,304 pages), reachable: a flooding process, about
four times its limit, still fits under the cap ([beamlet](userland/beamlet.md#limits-inside-one-vm)).

This is a measurement of the paths the case drove. Other requests or deeper call paths may
need more stack or heap, and any guest, including another server, can forge the public paint
pattern and the heap record. The result is evidence for the image's declarations, not a proof
against hostile guest code.

## Vendored dependencies

### Crates as published

<details><summary>Status: built · partly tested: provenance against crates.io needs the network, so no bench case runs it; it is run by hand in the review of any change to `vendor/` · tested (11)</summary>

- bench:vendor-check
- bench:vendor-build
- host:redoubt-vendor-check::the_real_structure_passes
- host:redoubt-vendor-check::a_renamed_header_fails_before_any_download
- host:redoubt-vendor-check::a_missing_row_fails
- host:redoubt-vendor-check::a_row_without_its_directory_and_a_stray_directory_fail
- host:redoubt-vendor-check::vendored_files_are_the_published_bytes
- host:redoubt-vendor-check::the_vendored_copies_are_the_ones_that_build
- host:redoubt-vendor-check::the_patches_point_at_vendor
- host:redoubt-vendor-check::the_readme_records_each_crate
- host:redoubt-vendor-check::every_vendored_file_is_tracked

</details>

`vendor/` holds third-party crates exactly as crates.io published them, each used through a
`[patch.crates-io]` path, and `vendor/README.md` records each one's version, license and published
checksum. Git must track every vendored file: a crate's own `.gitignore` applies inside `vendor/`,
and many name `Cargo.lock`. Two checks guard them, and they prove different things:

- **Integrity since vendoring** (`vendor-check`, every bench run): every file is byte for byte what
  `vendor/SHA256SUMS` records, nothing is added or removed, and the build takes the crates from
  `vendor/`, never from a registry copy. The checksums were taken from the tree itself, so this
  proves the crates have not changed since they were vendored, not that they are what crates.io
  published. `vendor-build` checks they build `no_std` for both widths.
- **Provenance** (`tools/vendor-check/provenance.sh`, in review): downloads each published crate,
  checks its SHA-256 against the live crates.io index and against `vendor/README.md`, and compares
  it with the vendored copy. It fails closed: before any download it checks that the README's
  table and `vendor/` name the same crates, in the same number, and it exits 0 only if every crate
  it checked matches on all three counts and it checked as many crates as `vendor/` holds.

The residuals: provenance is only as current as the last review that ran it, and the vendored
crates' `unsafe` is checked by recorded Miri runs, not by a bench case
([ipd under Miri](servers/ipd.md#under-miri), [sshd under Miri](servers/sshd.md#under-miri)).

### Patched crates

<details><summary>Status: built · tested (4)</summary>

- bench:vendor-check
- host:redoubt-vendor-check::vendored_files_are_the_published_bytes
- host:redoubt-vendor-check::a_patched_crate_differs_only_by_its_patch
- host:redoubt-vendor-check::every_patch_is_a_vendored_crates_and_recorded

</details>

A crate we must change is still vendored from its published bytes, with exactly one patch file,
`vendor/patches/<crate>.patch`, holding every change. `vendor/<crate>/` is the published crate
with that patch applied, since that is what Cargo builds, and nothing else differs.

- **Integrity:** `vendor/SHA256SUMS` records the published bytes. `vendor-check` copies the
  crate, reverses the patch on the copy, and requires the result to match the sums byte for byte,
  so the patch file is the whole difference.
- **Provenance:** `provenance.sh` downloads the published crate, applies the patch, and compares
  the result with `vendor/<crate>/`.
- **The README** records each patched crate's patch: what it changes and why, as its own section.
- A new release of the crate means taking the published bytes again and redoing the patch on
  them; a patch the crate's author has taken goes away with the release that carries it.

`sunset` is patched ([sshd](servers/sshd.md#the-core-and-its-platforms)), and what its patch does
is tested through its public API (`tools/vendor-check/tests/sunset_patch.rs`). `ascii` is patched
too: rustc 1.98 refuses to build it from a path as published, since Cargo caps a registry crate's
lints, not a vendored one's.

## The no-cruft gate

Status: built · tested: bench:no-cruft, host:testbench::a_rule_scoped_where_nothing_is_searched_fails

`no-cruft.toml` reads the sources and boots nothing. It fails on:
- a name of an interface the tree has dropped (its `forbidden` patterns);
- the native runtime handing out the raw call: `libs/rt/src` re-exporting `redoubt-sys` whole
  (alias, glob, `self` or `extern crate`), or a `pub` item or one-line re-export there naming
  `syscall` (`forbidden` patterns held `within` that path; a `within` that is missing or outside
  the case's `paths` fails the case);
- `allow(dead_code)` or `allow(unused...)` in the kernel, the loader, the layout and paging crates
  or the test programs;
- a Cargo feature that no `cfg(feature)` reads;
- a second literal definition of `PAGE_SIZE` or `USER_AREA_END`, or any `const PAGE` alias; the
  model is searched too, and its own `PAGE_SIZE` in `model/src/spec.rs` is an `[[allow]]`, because
  the model is the independent oracle and depends on no implementation crate.

Its `[[allow]]` entries (path, rule, reason) are the only exemptions, and an entry that no longer
covers anything fails the case too.

## The formatting gate

Status: built · partly tested: that a skip with no reason, or one that names no workspace, fails is read from the code, not run by a self-check · tested: bench:formatting, host:testbench::only_tracked_workspaces_are_reported, host:testbench::diff_headers_of_both_rustfmt_forms_name_the_file

`formatting.toml` holds the tree to [the formatting rule](../CONTRIBUTING.md#formatting): in each
cargo workspace root it names (the main workspace, each host-only fuzz or oracle crate that is its
own workspace, and `userland/otp`), it runs `cargo +nightly fmt --all --check` and fails on every
file that is not formatted with the repository's `rustfmt.toml`. It also fails on a tracked `Cargo.toml`
with a `[workspace]` table that is neither a root nor a `[[skip]]`, so a new crate outside the main
workspace cannot drift unchecked, and on a skip with no reason or no workspace. Two are skipped:
`bios/`, the upstream RustSBI firmware with its own configuration, and the docs checker's fixture
crate, whose layout is the checker's input. The generated wire codecs (`libs/wire/src/proto/`) are
in `rustfmt.toml`'s `ignore`: the generator is their one definition, and its own test holds them to
its output. Without nightly rustfmt the case fails, unless
`--allow-skip`. The case file is not called `rustfmt.toml`: rustfmt would read it as the
configuration for everything under `tests/`.

## The docs checker

### What it checks

<details><summary>Status: built · partly tested: no bench case runs it yet; it is run by hand before every change to the book · tested (5)</summary>

- host:redoubt-doccheck::good_tree_is_clean
- host:redoubt-doccheck::narrow_cases_fire
- host:redoubt-doccheck::c1_reports_each_failure
- host:redoubt-doccheck::pages_scope_keeps_only_the_listed_pages
- host:redoubt-doccheck::pages_scope_keeps_a_directory

</details>

`redoubt-doccheck` (`tools/doccheck`) holds this book to its own rules:
`cargo run -q -p redoubt-doccheck` prints each finding as `path:line: C<n>: message`;
`--pages <path>...` keeps only the findings on those pages, and `--code` adds C11, the check of
code comments and case descriptions for the documentation switch-over. It checks that
every section has one well-formed status line and every test it names exists; that milestones carry
their names; that no page carries process references; that every rule ID is defined once and cited
by its short name; that every relative link resolves; that the [security register](SECURITY.md)
agrees with the pages; that no binary sits under `docs/`; that pages keep their templates; that
every wire table is included once; and that every page is in the table of contents. Each rule has a
small bad tree it must fire on and a good one it must not. `mdbook build docs` renders the book.

### The docs checker in the bench

Status: built · tested: bench:docs

A `host-tests` case runs the checker's tests, among them one that checks this whole book and one
that builds it with `mdbook` and fails on any warning; the checker also reads code comments and
case descriptions, so they cite pages and rule IDs that exist.
