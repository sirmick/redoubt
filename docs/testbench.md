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

Status: built · tested: bench:bench-console-after-expect, bench:bench-poweroff-missing, bench:rustsbi-boot

A boot case passes when every `expect` pattern matches a console line, in order, no `forbid`
pattern ever matches, and the boot ends as the case says. Three patterns are always forbidden:
`PANIC`, `TEST FAILED` and `WARNING: INSECURE`. After the last `expect`, and any sessions, the bench
keeps reading for 50 ms (1 s in a checked build), so a forbidden line right after the last expected
one still fails the case. With `poweroff = true` it reads instead until QEMU exits, and requires the
exit status the case names (0 by default; 255 for an SBI system failure).

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
interim fixture; once `init` starts a case's programs, the gifts go
([M1 (separation and containment)](plan/m1-separation.md#remaining-work)).

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
icount = "shift=3,sleep=off" # run in QEMU's virtual time, so timing cases are deterministic
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

The kinds, and the fields each takes besides `description` and `arch`:

| Kind | What it does | Its fields |
| --- | --- | --- |
| `boot` | boots the kernel with `programs` as its first processes and judges the run | those above |
| `build` | only checks that a package compiles for each target: coverage for what the bench does not boot | `package`, `features` |
| `host-tests` | runs `cargo test` on the host for the named workspace packages, for what no boot can reach (a constant the loader and the bench share is right in the machine's eyes even when it is wrong) | `packages` |
| `ssh-loopback` | runs `[[session]]`s against a host OpenSSH server with no guest, to check the session runner on its own | `authorized` (the test keys the server accepts), `[[session]]`, `timeout_secs`, `host_key` (default: the server's own), `must_fail` |
| `unsafe-budget` | the ratchet on `unsafe` ([below](#the-unsafe-budget)) | `[[budget]]`: `name`, `paths`, `max_unsafe`, `max_undocumented` |
| `no-cruft` | the source gate ([below](#the-no-cruft-gate)) | `paths`, `[[forbidden]]` (`pattern`, `unless`), `no_allow_dead`, `one_definition`, `definition_paths`, `[[allow]]` (`path`, `rule`, `reason`) |

A `post_check` judges the console after the boot has passed. `sched_oracle` rebuilds the
scheduler's order from the raw events a tracing kernel prints and checks every pick against its own
reading of the rules ([scheduling](kernel/scheduling.md)); a limit such as `r10_p99_us=30000`
bounds a measured cost.

### The scheduler oracle

Status: built · tested: bench:sched-ties, host:testbench::a_trace_that_keeps_every_clause_passes, host:testbench::each_broken_clause_is_caught, host:testbench::a_broken_trace_is_rejected, host:testbench::the_models_own_ranks_pass, host:testbench::the_models_broken_ties_are_caught, host:testbench::lifts_are_recomputed, host:testbench::the_floor_and_the_passes_are_checked_on_their_own, host:testbench::destructions_are_timed_and_bounded

The oracle is independent of the kernel's code: it reads what the queue did (woke, requeued, left,
pass changed, picked), never why, and rebuilds the order from the events alone: the lowest pass
first; at an equal pass a budget that woke ahead of one requeued; of two that woke, the later
kernel entry's first, and within one entry the lower id; requeued ones in the order they were
requeued. It is itself checked against the model's ranks and against traces broken one clause at a
time. The tracing kernel is a test build only
([R23 (no test channels)](kernel/scheduling.md#r23-no-test-channels)).

## Checked builds

Status: built · tested: bench:bench-debug-assertions, bench:bench-debug-assertions-off

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
expects the entry's bytes read back in the guest.

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

Status: built · partly tested: the loopback self-checks fail on the development host, where the loopback server cannot start a login shell ([todo](todo/ssh-loopback-host.md)); no guest `sshd` exists yet to log in to · tested: bench:bench-ssh-loopback-deadlock, bench:bench-ssh-guest

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
    { expect = 'iex\(1\)> ' },   # wait for output
    { send = "File.read(\"/work/x\")\n" },
    { mark = "alice-ready" },    # tell other sessions this one got here
    { wait = "bob-ready" },      # wait for another session's mark
    { exit = 0 },                # close input, read until ssh exits, require this status
]
```

Every session's exit status is checked and all of its output passes `forbid`; a session that fails
stops the others. With `net.host_key` set, `ssh` refuses any other host key. Test keys live in
`tests/keys/`; they are public and marked not for production, and a boot manifest that lists one
must never ship. The `ssh-loopback` kind runs sessions against a host OpenSSH server that `ssh`
starts itself for each session, in inetd mode, so nothing listens on a port.

## Self-checks

Status: built · tested: bench:bench-attack-forgery, bench:bench-console-after-expect, bench:bench-poweroff-missing, bench:bench-reporter-mismatch, bench:bench-debug-assertions, bench:bench-debug-assertions-off, bench:bench-net-peer-twice, bench:bench-net-peer-count, bench:bench-net-peer-pcap-empty, bench:d3-net-self-unrefused

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
`d3-net-self-unrefused` runs `d3-net-attacks`'s boot with `ipd` not told one of the box's addresses,
and must fail on the SYN the capture then shows.

## The unsafe budget

Status: built · tested: bench:unsafe-budget, host:testbench::actual_source_counts_still_enforce_the_budget, host:testbench::empty_configuration_is_not_coverage, host:testbench::every_configured_root_must_contain_rust_source, host:testbench::missing_paths_fail_regardless_of_extension, host:testbench::unreadable_source_reports_its_path, host:testbench::broken_nested_symlink_is_not_silently_skipped, host:testbench::zero_unsafe_source_is_valid_as_a_file_or_nested_directory

`unsafe-budget.toml` lists every source directory of the trusted computing base that runs on the
target, each with the most uses of `unsafe` it may hold and the most that may lack a justification
(zero everywhere): a `// SAFETY:` comment above an `unsafe` block, and a `# Safety` section in the
doc comment of an `unsafe fn` or `unsafe impl`. The case counts both and fails if either is over.
Budgets only go down; raising one needs a stated reason in the change that does it.

A configured path with no Rust source in it fails, but a source directory left out of every budget
is not counted at all, and the ratchet cannot prove that every on-target source is configured. The
loader stub (`stub/src`), which runs on the target and uses `unsafe`, is in no budget today
([todo](todo/stub-unsafe-budget.md)). Vendored third-party crates are outside the ratchet
([below](#vendored-dependencies)).

## Vendored dependencies

Status: built · partly tested: provenance against crates.io needs the network, so no bench case runs it; it is run by hand in the review of any change to `vendor/` · tested: bench:vendor-check, bench:vendor-build, host:redoubt-vendor-check::the_real_structure_passes, host:redoubt-vendor-check::a_renamed_header_fails_before_any_download, host:redoubt-vendor-check::a_missing_row_fails, host:redoubt-vendor-check::a_row_without_its_directory_and_a_stray_directory_fail, host:redoubt-vendor-check::vendored_files_are_the_published_bytes, host:redoubt-vendor-check::the_vendored_copies_are_the_ones_that_build, host:redoubt-vendor-check::the_patches_point_at_vendor, host:redoubt-vendor-check::the_readme_records_each_crate

`vendor/` holds third-party crates exactly as crates.io published them, each used through a
`[patch.crates-io]` path, and `vendor/README.md` records each one's version, license and published
checksum. Two checks guard them, and they prove different things:

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
crates' `unsafe` has never run under Miri ([todo](todo/miri-vendored-unsafe.md)).

## The no-cruft gate

Status: built · tested: bench:no-cruft

`no-cruft.toml` reads the sources and boots nothing. It fails on:
- a name of an interface the tree has dropped (its `forbidden` patterns);
- `allow(dead_code)` or `allow(unused...)` in the kernel, the loader, the layout and paging crates
  or the test programs;
- a Cargo feature that no `cfg(feature)` reads;
- a second literal definition of `PAGE_SIZE` or `USER_AREA_END`, or any `const PAGE` alias; the
  model is searched too, and its own `PAGE_SIZE` in `model/src/spec.rs` is an `[[allow]]`, because
  the model is the independent oracle and depends on no implementation crate.

Its `[[allow]]` entries (path, rule, reason) are the only exemptions, and an entry that no longer
covers anything fails the case too.

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

Status: planned · M1 (separation and containment)

A `host-tests` case runs the checker's tests, among them one that checks this whole book and one
that builds it with `mdbook` and fails on any warning; the checker then also reads code comments and
case descriptions, so they cite pages and rule IDs that exist.

**Open:** none.
