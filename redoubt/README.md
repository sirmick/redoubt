# redoubt

The Redoubt-specific crates: the page-table library and the test bench. Architecture and
plans for what comes next: `planning/redoubt/` (start with its `README.md` and `STATUS.md`).

| Path              | What                                                                          |
| ----------------- | ----------------------------------------------------------------------------- |
| `paging/`         | Typed Sv32/Sv39 page tables — the one place page-table memory is touched (loader + kernel) |
| `test-programs/`  | `no_std` programs that run inside Redoubt, for example `log-server`, `rng-test`, `timer-test`, `uart-echo`, `mem-attack` |
| `testbench/`      | Host tool: builds, injects programs, boots QEMU, asserts on the console and over SSH |
| `tests/`          | Test cases for the bench, one TOML file each (`data/`: files they read; `keys/`: SSH test keys) |

The kernel is in `../kernel/`, the boot loader (both widths) in `../loader/`, and the `xous`
syscall ABI in `../xous-rs/`.

## Running tests

    cargo testbench                 # everything
    cargo testbench timer           # cases whose name contains "timer"
    cargo testbench --arch rv64     # one target
    cargo testbench --list

The exit status is non-zero if anything fails. Each boot's console log is kept in `target/testbench/`.
A case the host cannot run (no RustSBI firmware, no OpenSSH) fails, saying what is missing;
`--allow-skip` reports it as SKIP instead.

## Writing a test case

Paths in a case are relative to the workspace root.

```toml
description = "What this proves"
arch = ["rv64", "rv32"]      # targets to run on
kind = "boot"
programs = [                 # initial processes, PID 2 onwards
    "log-server",                                  # a binary of test-programs
    { package = "my-crate", bin = "my-server" },   # any workspace binary, built for the target
    { path = "prebuilt/thing.elf" },               # or a prebuilt ELF
]
smp = [1, 4]                 # one boot per hart count (default [1])
timeout_secs = 60            # default 60; fractions allowed
kernel_features = []         # extra kernel features, e.g. ["debug-print"]
expect = ['regex 1', 'regex 2']   # must each match a console line, in this order
forbid = ['regex']                # must never match; also always forbidden:
                                  # PANIC, TEST FAILED, WARNING: INSECURE
poweroff = false             # true: the guest must power off after the last expect

[[input]]                    # type on the console once a line matches `after`
after = "claimed irq 10"
send = "xyz"
```

After the last `expect` (and any sessions), the bench keeps reading the console for 50 ms, so a
forbidden line right after the last expected one still fails the case. With `poweroff = true` it
reads instead until QEMU exits, which must happen cleanly before `timeout_secs`, with no forbidden
line on the way.

Optional tables add bundle files, virtio devices and SSH sessions: see the sections below. An
unknown field or table is an error, so a misspelling cannot silently drop a check.

`kind = "build"` with `package` and `features` only checks that something compiles for the target —
coverage for a configuration the bench does not (or cannot yet) boot.

In-guest programs print through `log-server` (`test_programs::Logger`) and finish with
`<NAME> TEST PASSED` or `<NAME> TEST FAILED`. `log-server` starts every line it prints for a
client with `[pid N] `, N being the sender's PID as the kernel reports it; lines without that
prefix come from the kernel, the loader, `log-server` itself or a program that owns the UART.

## Poking at it by hand

    cargo testbench --run uart-echo                   # console on this terminal; Ctrl-A X quits
    cargo testbench --run log-server ipc-client --smp 4
    cargo testbench --run path/to/some.elf

## Other firmware

`--firmware <image>` replaces QEMU's bundled OpenSBI for a test run or for `--run`, e.g. a RustSBI
Prototyper build. rv32 has no bundled OpenSBI, so it always boots under RustSBI; run
`../scripts/fetch-rustsbi.sh` to build both firmwares (see the script's header). The bench looks
for them in a `rustsbi` checkout beside this repository's main checkout (found through git, so
worktrees work too), or where `RUSTSBI_PROTOTYPER` / `RUSTSBI_PROTOTYPER_RV32` say.

## Hostile inputs

A program entry can be a good binary that the bench corrupts before injecting it:

    programs = ["log-server", { corrupt = "rng-test", with = { segment-vaddr = "0xffffffffffd00000" } }]

`with` is `segment-vaddr`, `entry` or `truncate` (a byte count smaller than the file: a malformed
image). Cases that expect a deliberate panic set `allow_panic = true`, which drops only `PANIC` from
the always-forbidden list, and list what must not happen instead (`forbid = ['KMAIN']`).
`distinct_across_boots = ['id: (.*)']` (one capture group) boots twice and requires the captured
text to differ.

A hostile program is an ordinary `programs` entry (a test-programs binary such as
`grant-attack`, or `{ package, bin }` from any crate); how its case must judge it is below.
Hostile *data* for a program to use, such as a malformed ELF for a parent to launch, is a
`[[file]]` entry (below), which takes the same sources, corrupted ones included.

## Writing an attack case

**The rule:** an attack case passes only on a line the attacker cannot write. The console does
not say who wrote a line, and an attacker can print anything, including another program's
`PASSED`. So the verdict comes from the system: the kernel or the loader, a victim, a checker,
or a clean power-off (TENETS.md 6; the owner's answer to QUESTIONS.md 26).

The pattern:
- **Anchor every verdict pattern** with `^`, and pin it to its writer: `^\[pid 4\] ...` for a
  program's line (PIDs follow `programs`, from 2), or no `[pid` prefix for the kernel's, the
  loader's or `log-server`'s own. A relayed line always starts with its sender's prefix, so it
  cannot match either.
- **The attacker's lines** may be required as progress (`^\[pid 3\] ... attempts done`) and
  forbidden as breach evidence (`BREACH`, `FAIL`), but never be the verdict.
- **Give the verdict to a party the attacker does not control:**
  - the kernel or the loader refusing (`loader-rejects-*`, `kernel-wx`), with `KMAIN` or a
    later stage forbidden so no program ever ran;
  - a victim that owns what is attacked and still has it afterwards (`grant-attack`: log-server
    still receives the UART input sent after every attempt; `uaf-lent-page`: the holder reads
    its page back);
  - `attack-checker`, which the attacker tells when it is done (`test_programs::checker::done()`)
    and which then says, under its own PID, that the kernel still serves, and powers off; with
    `poweroff = true` the case also needs that clean power-off.
- **Make the attacker use what it gets**, so a breach shows where the attacker cannot hide or
  fake it (a raw line on a UART it should not own, a power-off, a victim that stops hearing).

What `attack-checker` asserts is only that the system survived. Where a case can say more only
once a later package lands (process creation and exit notices, WP-K4), its case file says so.

## Files in the bundle

```toml
[[file]]                     # a data entry, after the programs; bytes injected as they are
name = "trace"
from = { path = "redoubt/model/traces/t1" }        # or any `programs` form, e.g.
                                                   # { corrupt = "rng-test", with = { truncate = 80 } }
```

Entry names must all differ (and differ from `kernel` and `grants`). This is how a model trace
reaches an in-guest replayer; the replayer compares results itself and prints a verdict line for
`expect`/`forbid`. Today's loader starts every entry but `grants` as a process, so it refuses data
entries (`bench-bundle-file`); once it loads only the kernel and `init` (WP-K4), they are data for
`init` and `bootfsd`.

## Devices

```toml
[disk]                       # a virtio-blk disk, zeroed, created afresh for every boot
size_kib = 4096              #   as target/testbench/<case>-<target>-smp<N>.img

[net]                        # a virtio-net card on QEMU's user-mode network
forward = [22]               # guest TCP ports reachable from the host (default: none)
host_key = "ssh-ed25519 AAAA..."   # optional: the only SSH host key sessions accept
```

The guest reaches nothing outside QEMU (`restrict=on`): there is no outside peer, only forwarded
connections coming in. A case that needs one must add it deliberately. Each boot gets its own
host ports, chosen by the OS, so benches running side by side do not collide; another program
could still take a port in the moment before QEMU binds it, and QEMU then fails to start, which
fails the case rather than hiding.

## SSH sessions

Sessions need `net.forward = [22]`. They start once every `expect` has matched and run
concurrently while the bench keeps watching the console (a panic still fails the case). Each
drives the host's OpenSSH `ssh` to the guest's port 22, with a test key from `tests/keys/`. Each
session's transcript is kept in `target/testbench/<case>-<target>-smp<N>-<user>.ssh.log`.

```toml
[[session]]
user = "alice+secrets"       # the login name: unique in the case; letters, digits, _ + -
key = "alice"                # optional; defaults to `user` up to any '+'
pty = false                  # optional; true asks for a terminal, as an interactive user would
                             #   (its "\r\n" line ends are read as "\n")
forbid = ['bob-secret']      # never in any line of this session's output, until ssh exits
steps = [
    { expect = 'iex\(1\)> ' },   # wait for output; matched against what earlier expects left,
                                 # so prompts without a newline work; ^ and $ match at lines;
                                 # ssh's own messages ("Permission denied ...") count as output
    { send = "File.read(\"/work/x\")\n" },
    { mark = "alice-ready" },    # tell other sessions this one got here
    { wait = "bob-ready" },      # wait for another session's mark
    { exit = 0 },                # close input, read until ssh exits, require this status
]
```

A session whose last step is not `exit` ends as `{ exit = 0 }` does: every session's exit status
is checked and all of its output passes `forbid`. A session that fails stops the others. ssh's
connection attempt gives up half a second before the case's deadline, so a failed connection
reports ssh's reason rather than a bare timeout.

Host keys: with `net.host_key` set, ssh refuses any other key. Until the box's `sshd` has a known
key (WP-S3), guest cases leave it unset and accept whatever key the guest presents.

**Test keys** live in `tests/keys/` as `NAME` and `NAME.pub` (names: lower-case letters, digits,
`-`). They are public and marked NOT-FOR-PRODUCTION: a boot manifest that lists one must never
ship. Add one with

    ssh-keygen -t ed25519 -N "" -C "NOT-FOR-PRODUCTION redoubt test key NAME" -f redoubt/tests/keys/NAME

## Self-checks: the bench can fail

Cases named `bench-*` check the bench itself (TENETS.md 6): each feature has a case that passes
only if the feature works and, where the bench can be misconfigured on purpose, a case that must
fail:

```toml
must_fail = '^regex$'        # passes only if the run fails with a matching reason
```

`must_fail` is judged against the run's verdict only (console, sessions, devices); a build error or
the bench's own trouble is reported as a failure regardless. Anchor the pattern and quote the
evidence, so a case cannot pass by failing for some other reason.

`kind = "ssh-loopback"` runs `[[session]]`s against a host OpenSSH server with no guest, to check
the session runner on its own. ssh starts the server itself for each session (`sshd -i` as its
`ProxyCommand`), so nothing listens on a port. It accepts the test keys in
`authorized = ["alice", "bob"]`, presents `loopback-host`'s key (which the sessions check; a case
may set `host_key` to expect another), and runs `/bin/sh` for every login with forwarding and
agents off. An unprivileged sshd can log in only the user running it, so every session logs in as
that user and `user` only chooses the key and names the log. The server needs
`/usr/sbin/sshd`; its configuration and log are in `target/testbench/ssh/`.
