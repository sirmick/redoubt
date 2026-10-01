# The test bench

The bench is how every claim in this book is tested. It builds the real kernel, loader and
programs, packs and signs a boot bundle, boots it under QEMU, and judges what appears on the
console, over SSH and on the network, from outside the guest. It also runs host tests and source
checks. It is part of the system under tenet 6 ([the tenets](TENETS.md#6-tested-to-hell-and-back)),
held to the same standard of simplicity as the kernel: one tool, `tools/testbench`, and one file per
case in `tests/`.

## How to use it

```sh
cargo testbench                 # every case
cargo testbench timer           # cases whose name contains "timer"
cargo testbench --arch rv64     # one target
cargo testbench --list          # names and descriptions
./test                          # the same, from the repository root
```

The exit status is non-zero if anything fails. Each boot's console log, SSH transcripts, disk
images and captures are kept in `target/testbench/`. A case the host cannot run (no RustSBI
firmware, no OpenSSH) fails and says what is missing; `--allow-skip` reports it as skipped
instead. Every run boots the vendored RustSBI prototyper: `scripts/build-bios.sh` builds it for
both widths, or `RUSTSBI_PROTOTYPER` and `RUSTSBI_PROTOTYPER_RV32` name the images. There is no
fallback to QEMU's own firmware.

| Path | What |
| --- | --- |
| `tools/testbench/` | the bench: builds, injects programs, boots QEMU, judges the console, sessions and network |
| `tests/*.toml` | the cases, one per file (`tests/data/`: files they read; `tests/keys/`: SSH test keys) |
| `tests/programs/` | `no_std` programs that run inside Redoubt: the log server, victims, attackers, checkers |
| `tests/net/` | the network rig: boots the real `netd` and `ipd` through the loader stub, with clients and attackers |

## Verdicts

### What a case passes on

Status: built · tested: bench:bench-console-after-expect, bench:bench-poweroff-missing, bench:bench-qemu-early-exit, bench:rustsbi-boot, host:testbench::a_killed_bench_leaves_no_qemu, host:testbench::a_qemu_lacking_an_option_is_named

A boot case passes when every `expect` pattern matches a console line, in order, no `forbid`
pattern ever matches, and the boot ends as the case says. Three patterns are always forbidden:
`PANIC`, `TEST FAILED` and `WARNING: INSECURE`. After the last `expect`, and any sessions, the bench
keeps reading for 50 ms (1 s in a checked build), so a forbidden line right after the last expected
one still fails the case. With `poweroff = true` it reads instead until QEMU exits, and requires the
exit status the case names (0 by default; 255 for an SBI system failure). QEMU runs with
`-run-with exit-with-parent=on`, so a bench killed at its timeout, even outright, leaves no guest
running to skew the next run.

That option is QEMU 10.1's, and the bench checks for it: before a width's first boot it runs that
width's QEMU with the option and `-version`, and a QEMU that refuses it fails every boot case at
once with the version found, the version needed and QEMU's complaint (skips them, with
`--allow-skip`, as a missing firmware does). A guest that ends before it prints a line, or whose
QEMU exits with a failing status, fails with QEMU's exit status and the last lines of its stderr,
which go to the console log too; one that printed and then died fails as it did, with QEMU's exit
status.

In-guest programs print through the log server and finish with `<NAME> TEST PASSED` or
`<NAME> TEST FAILED`; attack programs end with `attempts done` instead. The log server starts every
line it prints for a client with the sender's badge, as the kernel reports it, on every path that
takes client text: `[pid N] ` for a badge below 0x100, and `[badge N] ` for any other. The loader
numbers bundle programs from 2 to at most 64 and gives each its PID as its badge, and every badge
a test program mints is 0x100 or more, so a minted badge can never print as a PID and forge a
bundle program's line (`tests/programs/src/logsrv.rs`). Lines without a prefix come from the
kernel, the loader, the log server's own fixed templates, or a program that owns the UART.

### Rule F (trusted verdicts)

Status: built · tested: bench:bench-attack-forgery, bench:bench-reporter-mismatch, bench:logsrv-badge-forgery

A verdict comes only from a party the attacker cannot impersonate. The console does not say who
wrote a line, and an attacker can print anything, including another program's `PASSED`, so an
attack case passes only on a line the attacker cannot write:

- **The kernel or the loader**, refusing: a line with no `[pid` prefix, with `KMAIN` or a later
  stage forbidden so that no program ever ran (`loader-rejects-*`), or an anchored line the kernel
  prints at boot before any program starts, which a relayed line cannot match (`kernel-wx`).
- **A victim** that owns what is attacked and still has it afterwards: the log server still hears
  UART input after every attempt (`irq-attack`); a victim inspects its pages (`mem-attack`,
  `uaf-lent-page`).
- **The log server's `DONE`.** The bundle's first program owns the console: it holds the console's
  device handles, and every other program's text reaches the UART only through it, prefixed with
  the sender's `[pid N]` or `[badge N]`. A victim, or with no victim the attacker once it is done,
  calls the checker's `done()`; the log server prints one `[server] done:` line naming the caller
  by the badge the kernel gave its handle, and powers off. A case that names a `reporter` passes
  only if exactly one line starts `[server] done:` and it names the reporter's PID; any other such
  line fails it, as does one in a case with no `reporter`.
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
above), and the case's `programs` list fixes the order of the rest, so which PID and which handles
each program gets is the case's choice, never a race.

**Gifts.** A budget-attack case needs its attacker to hold budgets. The log server hands the first
program's three budgets (`root`, `system` and `users`), and never a device or a DMA device, to the
first caller of `TAKE_GIFTS`, and refuses every later caller (`tests/programs/src/bin/log-server.rs`).
The residual: a benign sibling that happens to call first takes the gifts; the attacker then fails
loudly on `Refused`, never passes silently. That is acceptable only while the log server is the
interim fixture; once the log server starts a case's programs with the budgets the case names,
the gifts go ([starting a case's programs](#starting-a-cases-programs)).

Every verdict pattern is anchored with `^` and pinned to its writer, with a comment beside it saying
why it cannot be forged. The attacker's own lines may be required as progress (so a refusal for the
wrong reason fails) and forbidden as evidence of a breach, but are never the verdict. Where the
attacker itself reports to the checker, the case shows only that the system survived, and its
description says so ("verdict: survival only").

## Cases

### The case file

Status: built · partly tested: that an unknown field or table is refused is read from the code, not attacked by a case · tested: bench:bench-console-after-expect, bench:bench-poweroff-missing

A case is one TOML file. Paths in it are relative to the workspace root, and an unknown field or
table is an error, so a misspelling cannot silently drop a check. The one exception is a `programs`
entry (and a bundle file's `from`): its forms are told apart by their keys, so an extra key in one
is ignored rather than refused (`tools/testbench/src/case.rs`).

```toml
description = "What this proves"
arch = ["rv64", "rv32"]      # targets to run on
kind = "boot"
programs = [                 # initial processes, PID 2 onwards
    "log-server",                                  # a binary of the test programs
    { package = "my-crate", bin = "my-server" },   # any workspace binary, built for the target
    { path = "prebuilt/thing.elf" },               # or a prebuilt ELF
]
smp = [1, 4]                 # one boot per hart count (default [1])
memory_mib = 32              # guest RAM (default 256)
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

A `boot` case also takes `allow_panic`, `tamper_bundle`, `sign_bare_archive` and
`distinct_across_boots` ([hostile inputs](#hostile-inputs)), `[[file]]`
([bundle files](#bundle-files)), `[disk]` and `[net]` ([devices](#devices-and-the-network)),
`[[session]]` ([SSH sessions](#ssh-sessions)), `must_fail` ([self-checks](#self-checks)), and
`poweroff_status`: the QEMU exit status a `poweroff` case requires (0 by default; 255 for an SBI
system failure, which a rejection case asks for).

With `icount`, QEMU runs the guest at a fixed instruction rate, skips idle time to the next timer
deadline and puts the RTC on the same clock (`-rtc clock=vm`). A time a case asserts is then a
count of guest instructions (at `shift=3`, 1 ms is 125,000), the same on any host however loaded.
It does not make a run repeat on its own: QEMU fills the guest's boot RNG seed from host entropy
on every boot, the kernel draws PIDs from it, and that moves every later event. `qemu_seed = N`
pins it (QEMU `-seed N`, which fills the device tree's `/chosen/rng-seed`), and with `icount` a
run then repeats exactly. The bench prints the seed before the result, and
`TESTBENCH_QEMU_SEED=M` replaces the pinned seed of every case that has one, to replay a run or
to sweep. A timing gate runs one pinned seed and states its target from a sweep of seeds
([responsiveness](kernel/scheduling.md#responsiveness)).

The kinds, and the fields each takes besides `description` and `arch`:

| Kind | What it does | Its fields |
| --- | --- | --- |
| `boot` | boots the kernel with `programs` as its first processes and judges the run | those above |
| `build` | only checks that a package compiles for each target: coverage for what the bench does not boot | `package`, `features` |
| `host-tests` | runs `cargo test` on the host for the named workspace packages, for what no boot can reach (a constant the loader and the bench share is right in the machine's eyes even when it is wrong); with `miri`, under nightly Miri. These cases are the bench's only host tests; `cargo test --workspace` is not run, though it compiles. The kernel and the test programs have no host tests (`test = false` on their targets) | `packages`, `tests` (the test files to run; default all), `miri` |
| `ssh-loopback` | runs `[[session]]`s against a host OpenSSH server with no guest, to check the session runner on its own | `authorized` (the test keys the server accepts), `[[session]]`, `timeout_secs`, `host_key` (default: the server's own), `server_log` (patterns each of which must match a line of the server's own log), `must_fail` |
| `unsafe-budget` | the ratchet on `unsafe` ([below](#the-unsafe-budget)) | `[[budget]]`: `name`, `paths`, `max_unsafe`, `max_undocumented`; `[[uncounted]]`: `path`, `reason` |
| `size-budget` | the ceiling on each trusted crate's size ([below](#the-size-budget)) | `[[crate]]`: `name`, `paths`, `max_lines` |
| `no-cruft` | the source gate ([below](#the-no-cruft-gate)) | `paths`, `[[forbidden]]` (`pattern`, `unless`, `within`), `no_allow_dead`, `one_definition`, `definition_paths`, `[[allow]]` (`path`, `rule`, `reason`) |
| `fmt` | the formatting gate ([below](#the-formatting-gate)) | `roots`, `[[skip]]` (`path`, `reason`) |

A `post_check` judges the console after the boot has passed. `sched_oracle` rebuilds the
scheduler's order from the raw events a tracing kernel prints and checks every pick against its own
reading of the rules ([scheduling](kernel/scheduling.md)); a limit such as `r10_p99_us=30000`
bounds a measured cost.

### Starting a case's programs

Status: planned · M1 (separation and containment)

Once the loader loads only the kernel and `init`
([boot](kernel/boot.md#the-loader-loads-only-the-kernel-and-init)), a boot case starts its
programs in one of two ways:

- **In `init`'s place, for the kernel's cases.** The case's first program is packed as the
  bundle's second entry. It gets `init`'s handoff: the three budgets, the Reset right, every
  device and the bundle. It is the trusted tester, as the first program is
  today. When it is the log server, it starts the case's other programs itself, through the
  loader stub and from the bundle's pages, each in a budget of its own carved from `system`. Each
  program gets the handles in the slots it has today: the boot endpoint's receive right for the
  second program, and the boot and log endpoints for the later ones. A program also gets the
  budgets the case names for it (`budgets = ["system"]`), which replaces `TAKE_GIFTS`. The
  builder tells the tester which programs to start, and with which budgets, in one more data
  entry, `programs`. It has one ASCII line per program after the first, in the case's order:
  the program's entry name, then the names of the budgets it gets (`root`, `system`, `users`),
  separated by spaces. The tester reads it from the bundle's pages, which the loader verified
  with the rest, and refuses the boot on any line it cannot parse. Each program's own budget gets
  an equal share of what `system` has free (pages and processes) and weight 1,000, a driver's,
  so no program's limits depend on another's name or order. A case that needs other sizes builds
  its own tree from the budgets it is given. The log
  server badges each program's handles with the program's place in the case, 2 on, and names
  programs by that place, because the kernel now draws every PID but the first. A case matches a
  PID in a kernel line with a pattern.
- **Under `init`, for the servers' cases.** The case boots the real `init` with a manifest of its
  own, packed as the `manifest` entry, and its test programs are `servers` entries in that
  manifest. `init` prints its own lines bare, and `consoled` starts every other program's line
  with its connection id ([consoled](servers/consoled.md#started-by-init)). A case's `reporter`
  names a manifest entry, and the bench reads that entry's connection id from `init`'s line
  announcing it. Such a case ends at its last `expect`, since no test program holds the Reset
  right.

`init` itself never takes a case's place and has no mode for the bench. The kernel's cases need
`root` and `system`, which no program under `init` may hold
([R33 (no server holds a system budget)](servers/init.md#r33-no-server-holds-a-system-budget)),
so they run a tester in `init`'s place instead.

**Open:** none.

### The scheduler oracle

Status: built · tested: bench:sched-ties, host:testbench::a_trace_that_keeps_every_clause_passes, host:testbench::each_broken_clause_is_caught, host:testbench::a_broken_trace_is_rejected, host:testbench::the_models_own_ranks_pass, host:testbench::the_models_broken_ties_are_caught, host:testbench::lifts_are_recomputed, host:testbench::weight_changes_are_recomputed, host:testbench::the_floor_and_the_passes_are_checked_on_their_own, host:testbench::destructions_are_timed_and_bounded, host:testbench::audits_are_subtracted_inside_each_window, host:testbench::an_unmatched_audit_fails

The oracle is independent of the kernel's code: it reads what the queue did (woke, requeued, left,
pass changed, picked), never why, and rebuilds the order from the events alone: the lowest pass
first; at an equal pass a budget that woke ahead of one requeued; of two that woke, the later
kernel entry's first, and within one entry the lower id; requeued ones in the order they were
requeued. It recomputes each lift and each weight change from the rule, and lets a pass fall only
at a weight change. It is itself checked against the model's ranks and against traces broken one clause at a
time. The tracing kernel is a test build only
([R23 (no test channels)](kernel/scheduling.md#r23-no-test-channels)).

## Checked builds

Status: built · tested: bench:bench-debug-assertions, bench:bench-debug-assertions-off, bench:sched-latency, host:testbench::audits_are_subtracted_inside_each_window, host:testbench::an_unmatched_audit_fails

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
they run, and a release build has none of them. So a latency target, which is measured in a checked
build because the scheduler trace needs one, excludes them. The traced kernel stamps each audit's
start and end (records `U` and `V`: which audit, and the time). The program prints each latency
sample's window, its end on `time_now` and its length (`LATENCY-SAMPLE <group> <measure> <end>
<gross>`) and how many it took of each, so a window lost on the way fails the check, and it
judges none of them. `sched_oracle` subtracts the audit time inside each window,
counting only the part of an audit that falls in it, and then applies the case's bounds
(`deadline_notice_p99_us=40000`). It reports the gross, the net and the audit time beside each
target ([responsiveness](kernel/scheduling.md#responsiveness)). An audit that never ends, ends
without beginning or runs inside a destruction fails the check. The oracle subtracts only what the
trace shows it: a kernel built with `audit-unstamped`, which leaves the audit after a destruction
unstamped, misses the containment gate's deadline notice, in a recorded negative run. The audits
themselves stay full.

Many cases use the profile: every case file with `debug_assertions = true`, most of them over
both widths (`budget`, `budget-syscall-attack`, `lend-untouched-page`, `ipc` and `smp-spike` among
them), and some, such as `all-together`, on rv64 only. The kernel
prints one line under `cfg!(debug_assertions)`: `bench-debug-assertions` expects it, and
`bench-debug-assertions-off` forbids it in an ordinary boot. To check the whole suite:

```sh
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true cargo testbench
```

That run fails `bench-debug-assertions-off`, as it should; everything else must pass.

## Hostile inputs

Status: built · tested: bench:loader-rejects-kernel-address, bench:loader-rejects-kernel-entry, bench:loader-rejects-truncated-elf, bench:verified-boot-rejects-tamper, bench:verified-boot-rejects-bare-archive, bench:rng

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
text to differ. A hostile program is an ordinary `programs` entry; hostile data for a program to
use, such as a malformed ELF for a parent to launch, is a bundle file.

## Files in the bundle

### Bundle files

Status: built · tested: bench:bench-bundle-file, bench:loader-rejects-grants

```toml
[[file]]                     # a data entry, after the programs
name = "trace"
from = { path = "tests/data/bundle-file.txt" }   # or any `programs` form, corrupted ones included
```

Entry names must differ from each other and from `kernel`. The loader refuses an entry named
`grants`, and the bench refuses a `[[grant]]` table: a program reaches a device only through the
handles it is given ([R18 (device authority)](kernel/devices.md#r18-device-authority)). Today the
loader starts every entry as a process, so it refuses a data entry, and `bench-bundle-file`
checks that refusal.

### Data entries for `init`

Status: planned · M1 (separation and containment)

Once the loader loads only the kernel and `init`, `init` receives the verified bundle, hands the
public entries to `bootfsd`, and a data entry is how a model trace reaches an in-guest replayer,
which compares results itself and prints its verdict
([boot](kernel/boot.md#the-loader-loads-only-the-kernel-and-init)). `bench-bundle-file` then
expects the entry's bytes read back in the guest. The program in `init`'s place reads them first,
from the bundle's pages. Under `init`, a program reads them through `/boot` once the manifest's
`public` list names the entry.

**Open:** none.

## Devices and the network

### Disks and network cards

Status: built · tested: bench:bench-virtio-devices, bench:bench-virtio-legacy-off, host:testbench::every_network_is_restricted, host:testbench::virtio_devices_are_modern

```toml
[disk]                       # a virtio-blk disk, zeroed, created afresh for every boot
size_kib = 4096

[net]                        # a virtio-net card on QEMU's user-mode network
forward = [22]               # guest TCP ports reachable from the host (default: none)
host_key = "ssh-ed25519 AAAA..."   # optional: the only SSH host key sessions accept
```

Devices use virtio-mmio's modern transport, which `blkd` and `netd` require. The guest reaches
nothing outside QEMU (`restrict=on`, checked for every `[net]` case): there is no outside peer, only
forwarded connections coming in, unless a case adds one deliberately. Each boot gets its own host
ports, chosen by the operating system, so benches running side by side do not collide.

### Peers, dials and the capture

Status: built · tested: bench:bench-net-peer, bench:bench-net-peer-twice, bench:bench-net-peer-count, bench:bench-net-peer-pcap-empty, host:testbench::peers_are_judged_on_records_and_capture, host:testbench::a_capture_is_read_fail_closed, host:testbench::what_the_guest_sends_is_checked, host:testbench::records_are_counted_per_peer, host:testbench::a_dial_needs_its_echo, host:testbench::prefixes_and_peers_are_checked, host:testbench::peers_get_the_wider_network_and_a_capture

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
```

- **Peers** are QEMU `guestfwd`s to a program: for each connection the guest makes, the bench's own
  binary starts as a helper on it, records the connection as a file before it echoes a byte, and
  then echoes. After the boot each peer's count must equal its `connections`. There is no host
  listener another process could take.
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

Status: built · partly tested: no guest `sshd` exists yet to log in to · tested: bench:bench-ssh-loopback, bench:bench-ssh-loopback-openssh, bench:bench-ssh-loopback-deadlock, bench:bench-ssh-loopback-forbid, bench:bench-ssh-loopback-exit, bench:bench-ssh-loopback-host-key, bench:bench-ssh-loopback-aborted-text, bench:bench-ssh-guest, host:testbench::the_reference_proxy_quotes_only_what_the_bench_chose, host:testbench::resize_needs_a_pty, host:testbench::no_other_child_inherits_a_sessions_terminal

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
OpenSSH's server in inetd mode, inside a container. Its log goes to a file beside the transcripts,
never to `ssh`'s output, so a late line of the server's cannot stand in for the session's last
output; a case's `server_log` asks what the server saw (a refused key, say), not only what the
client printed, and its `server_log_forbid` what the server must not have done (a console
started, say).

**OpenSSH's server runs in a container.** On a host that enforces SELinux, `sshd` moves every
login shell into the user's default context, and a bench running in a service's context may not
enter it, so the host's own `sshd` cannot serve the reference case. Inside a container `sshd`
finds SELinux disabled and makes no such move; the client, the session runner and the case are
unchanged, so the case is still a witness independent of Redoubt's server.
- **The image** is built from `tests/ssh-reference/Containerfile`: a Debian base pinned by
  digest and the distribution's `openssh-server` pinned by version, nothing else. The bench tags
  it with a hash of that file and builds it only when no image has that tag, so only the first
  run after a change to the file needs the network.
- **Each session** starts its own server: `ssh`'s `ProxyCommand` is `podman run -i --rm
  --network=none --pull=never` of that image running `sshd -i`, with the case's configuration,
  keys and log from a directory the bench makes for the case. Each file is mounted on its own
  with a shared SELinux label, so that concurrent sessions can all append to the one log; the
  configuration and keys are read-only, and only the log is writable. The server logs in only root, inside
  the container (root there is the bench's user outside), runs `/bin/sh` for every login and
  allows nothing else. The container has no network, so nothing but its `ssh` can reach it.
- **Rootless `podman` needs the user's own group.** A service may run the bench with another
  primary group, and `newuidmap` then refuses to map the container's users; the runner starts
  `podman` under `sg` and the user's primary group from the password database. A service also has
  no systemd user session to hold a container's cgroup, so `podman` runs with
  `--cgroup-manager=cgroupfs`; without it the image's build fails there.
- **Before the first OpenSSH loopback case** the bench logs in once and runs `exit 0`, and the
  server's log must name OpenSSH's version, so that no other server can pass for it. Without
  `podman`, or with no image and no network to build one, every OpenSSH loopback case fails with
  that reason, as on a host without OpenSSH, and `--allow-skip` skips them. The bench tells the
  network's absence from a broken recipe after a failed build: if the base image's registry or
  Debian's archive does not accept a connection, the host lacks the network; if both do, the
  recipe is at fault, and that, like any other probe failure, fails every OpenSSH loopback case.
  The residual: Debian drops a superseded package version from its archive, so once
  `openssh-server`'s pin is superseded a host without the image cannot build it, and every
  OpenSSH loopback case fails there until the pin moves; a host that has the image runs on. The
  check also runs from the bench's network, not the build's: a host whose `podman` alone is cut
  off fails the case rather than skipping it, and a host whose only way out is an HTTP(S) proxy
  that `podman` uses and the check does not finds neither source answering, so a bad pin there is
  a host lack that `--allow-skip` skips.

### Against Redoubt's sshd

Status: built · partly tested: agent forwarding is refused inside `sunset`, which no case sees, since `ssh` asks for it without a reply ([residual risks](servers/sshd.md#residual-risks)) · tested: bench:sshd-loopback-logins, bench:sshd-loopback-r67, bench:sshd-loopback-interrupt, bench:sshd-loopback-independent, bench:sshd-loopback-window-change, bench:sshd-loopback-window-change-zero, bench:sshd-loopback-env-refused, bench:bench-ssh-loopback, bench:bench-ssh-loopback-host-key, bench:sshd-host-tests, host:testbench::an_ssh_lacking_an_option_is_named

`ssh-loopback` cases run the host's OpenSSH `ssh` against Redoubt's own `sshd` on its host
platform, `redoubt-sshd-host` ([the core and its platforms](servers/sshd.md#the-core-and-its-platforms)),
which the bench builds and `ssh` starts as its `ProxyCommand`. Its host key is `loopback-host`,
and each of the case's `authorized` keys is a principal of the same name. Nothing there needs a
shell, a login context or a container.

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
  (`bench-ssh-loopback-openssh`). Its server runs in a container
  ([sessions and the loopback server](#sessions-and-the-loopback-server)).
- `ssh` gets `WarnWeakCrypto=no-pq-kex` against Redoubt's server, whose exchange is not
  post-quantum: OpenSSH's warning would otherwise be session output. The option is OpenSSH 10.1's,
  and before such a case the bench has `ssh -G` parse it: an older `ssh` fails the case with the
  version found and the version needed (skips it, with `--allow-skip`), rather than with the
  server's log that was never written.

## Self-checks

Status: built · tested: bench:bench-attack-forgery, bench:bench-console-after-expect, bench:bench-poweroff-missing, bench:bench-reporter-mismatch, bench:bench-debug-assertions, bench:bench-debug-assertions-off, bench:bench-net-peer-twice, bench:bench-net-peer-count, bench:bench-net-peer-pcap-empty, bench:bench-net-self-unrefused, bench:bench-cbo-self-unrefused, bench:bench-qemu-early-exit

The harness can fail, and each feature shows it. Cases named `bench-*` check the bench itself:
each feature has a case that passes only if the feature works and, where the bench can be
misconfigured on purpose, a case that must fail:

```toml
must_fail = '^regex$'        # passes only if the run fails with a matching reason
```

`must_fail` is judged against the run's verdict only (console, sessions, devices); a build error or
the bench's own trouble is a failure regardless. Each case writes its pattern anchored and quoting
the evidence, so it cannot pass by failing for some other reason; the bench does not enforce the
anchoring, so a reviewer checks it. An attack case can have a self-check of its own:
`bench-net-self-unrefused` runs `net-attacks`'s boot with `ipd` not told one of the box's addresses,
and must fail on the SYN the capture then shows. `bench-cbo-self-unrefused` runs
`cbo-user-fault`'s boot with a kernel that leaves `senvcfg` permissive: all four cache-block
operations must be seen running, and the run must fail on the verdict line that never comes.

## The unsafe budget

Status: built · tested: bench:unsafe-budget, host:testbench::actual_source_counts_still_enforce_the_budget, host:testbench::empty_configuration_is_not_coverage, host:testbench::every_configured_root_must_contain_rust_source, host:testbench::missing_paths_fail_regardless_of_extension, host:testbench::unreadable_source_reports_its_path, host:testbench::broken_nested_symlink_is_not_silently_skipped, host:testbench::zero_unsafe_source_is_valid_as_a_file_or_nested_directory, host:testbench::every_on_target_source_is_in_a_budget, host:testbench::a_long_safety_block_directly_above_justifies, host:testbench::a_raise_needs_its_unsafe_budget_line, bench:rt-miri, host:testbench::a_miri_case_runs_its_files_under_miri

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
under Miri (Stacked Borrows, isolation off), which checks the heap's free lists, page buffers and
lends that a native run only executes. It runs the files that finish in seconds; `ipc` and `echo`
take minutes under Miri and are run by hand. Without nightly Miri the case is missing, not passed.
With the runtime's page-buffer aliasing fix reverted, `mapping_views` fails it with the Stacked
Borrows error `not granting access to tag <wildcard> because that would remove [Unique for <…>]
which is strongly protected`.

## The size budget

Status: built · tested: bench:size-budget, host:testbench::only_code_lines_count, host:testbench::a_test_module_does_not_count, host:testbench::a_test_module_file_does_not_count, host:testbench::a_shipped_file_always_counts, host:testbench::a_form_the_case_cannot_follow_fails, host:testbench::a_raise_needs_its_reason, host:testbench::the_ratchet_reads_the_commit_that_raised, host:testbench::a_merge_is_judged_against_its_first_parent, host:testbench::merged_history_is_not_read_again, host:testbench::a_new_budget_file_needs_every_reason, host:testbench::a_deleted_budget_file_fails

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

## Vendored dependencies

### Crates as published

Status: built · partly tested: provenance against crates.io needs the network, so no bench case runs it; it is run by hand in the review of any change to `vendor/` · tested: bench:vendor-check, bench:vendor-build, host:redoubt-vendor-check::the_real_structure_passes, host:redoubt-vendor-check::a_renamed_header_fails_before_any_download, host:redoubt-vendor-check::a_missing_row_fails, host:redoubt-vendor-check::a_row_without_its_directory_and_a_stray_directory_fail, host:redoubt-vendor-check::vendored_files_are_the_published_bytes, host:redoubt-vendor-check::the_vendored_copies_are_the_ones_that_build, host:redoubt-vendor-check::the_patches_point_at_vendor, host:redoubt-vendor-check::the_readme_records_each_crate, host:redoubt-vendor-check::every_vendored_file_is_tracked

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

Status: built · tested: bench:vendor-check, host:redoubt-vendor-check::vendored_files_are_the_published_bytes, host:redoubt-vendor-check::a_patched_crate_differs_only_by_its_patch, host:redoubt-vendor-check::every_patch_is_a_vendored_crates_and_recorded

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

Status: built · partly tested: no bench case runs it yet; it is run by hand before every change to the book · tested: host:redoubt-doccheck::good_tree_is_clean, host:redoubt-doccheck::narrow_cases_fire, host:redoubt-doccheck::c1_reports_each_failure, host:redoubt-doccheck::pages_scope_keeps_only_the_listed_pages, host:redoubt-doccheck::pages_scope_keeps_a_directory

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
