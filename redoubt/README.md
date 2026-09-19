# redoubt

The Redoubt-specific crates: the page-table library and the test bench. Architecture and
plans for what comes next: `planning/redoubt/` (start with its `README.md` and `STATUS.md`).

| Path              | What                                                                          |
| ----------------- | ----------------------------------------------------------------------------- |
| `paging/`         | Typed Sv32/Sv39 page tables — the one place page-table memory is touched (loader + kernel) |
| `test-programs/`  | `no_std` programs that run inside Redoubt, for example `log-server`, `rng-test`, `timer-test`, `uart-echo`, `mem-attack` |
| `testbench/`      | Host tool: builds, injects programs, boots QEMU, asserts on the console and over SSH |
| `tests/`          | Test cases for the bench, one TOML file each (`tests/data/`: files they read) |

The kernel is in `../kernel/`, the boot loader (both widths) in `../loader/`, and the `xous`
syscall ABI in `../xous-rs/`.

## Running tests

    cargo testbench                 # everything
    cargo testbench timer           # cases whose name contains "timer"
    cargo testbench --arch rv64     # one target
    cargo testbench --list

The exit status is non-zero if anything fails. Each boot's console log is kept in `target/testbench/`.

## Writing a test case

```toml
description = "What this proves"
arch = ["rv64", "rv32"]      # targets to run on; ones that cannot boot yet are reported as SKIP
kind = "boot"
programs = [                 # initial processes, PID 2 onwards
    "log-server",                                  # a binary of test-programs
    { package = "my-crate", bin = "my-server" },   # any workspace binary, built for the target
    { path = "prebuilt/thing.elf" },               # or a prebuilt ELF
]
smp = [1, 4]                 # one boot per hart count (default [1])
timeout_secs = 60            # default 60
kernel_features = []         # extra kernel features, e.g. ["debug-print"]
expect = ['regex 1', 'regex 2']   # must each match a console line, in this order
forbid = ['regex']                # must never match; PANIC and TEST FAILED are always forbidden

[[input]]                    # type on the console once a line matches `after`
after = "claimed irq 10"
send = "xyz"
```

Optional tables add bundle files, result comparison, virtio devices and SSH sessions: see the
sections below.

`kind = "build"` with `package` and `features` only checks that something compiles for the target —
coverage for a configuration the bench does not (or cannot yet) boot.

In-guest programs print through `log-server` (`test_programs::Logger`) and finish with
`<NAME> TEST PASSED` or `<NAME> TEST FAILED`.

## Poking at it by hand

    cargo testbench --run uart-echo                   # console on this terminal; Ctrl-A X quits
    cargo testbench --run log-server ipc-client --smp 4
    cargo testbench --run path/to/some.elf

## Other firmware

`--firmware <image>` replaces QEMU's bundled OpenSBI for a test run or for `--run`, e.g. a RustSBI
Prototyper build. rv32 has no bundled OpenSBI, so it always boots under RustSBI; run
`../scripts/fetch-rustsbi.sh` to build both firmwares (see the script's header).

## Hostile inputs

A program entry can be a good binary that the bench corrupts before injecting it:

    programs = ["log-server", { corrupt = "rng-test", with = { segment-vaddr = "0xffffffffffd00000" } }]

`with` is `segment-vaddr`, `entry` or `truncate` (a byte count: a malformed image). Cases that
expect a deliberate refusal set `default_forbid = false` and list what must not happen instead
(`forbid = ['KMAIN']`). `distinct_across_boots = ['id: (.*)']` boots twice and requires the captured
text to differ.

Attack programs need nothing special: a hostile program is an ordinary `programs` entry (a
test-programs binary such as `grant-attack`, or `{ package, bin }` from any crate) that reports
what it tried and ends with `<NAME> PASSED`/`FAILED`. Hostile *data* for a program to use, such as
a malformed ELF for a parent to launch, is a `[[file]]` entry (below), which takes the same
sources, corrupted ones included.

## Files in the bundle

```toml
[[file]]                     # a data entry, after the programs; bytes injected as they are
name = "trace"
from = { path = "redoubt/model/traces/t1" }        # or any `programs` form, e.g.
                                                   # { corrupt = "rng-test", with = { truncate = 80 } }
```

Entry names must all differ (and differ from `kernel` and `grants`). Today's loader starts every
entry but `grants` as a process, so it refuses data entries (`bench-bundle-file`); once it loads
only the kernel and `init` (WP-K4), they are data for `init` and `bootfsd`.

## Results (model-trace replay)

A replayer in the guest prints one line per result; the bench collects them and compares them with
a file, one expected result per line: same count, same text, same order. The comparison runs once
every `expect` has matched, and a mismatch names the first differing result.

```toml
[results]
pattern = '^replay: (.*)$'   # one capture group: the result
expected = "redoubt/model/traces/t1.expected"
```

The bench knows nothing of the trace format: the trace goes in as a `[[file]]`, and whoever writes
the replayer chooses what a result line says.

## Devices

```toml
[disk]                       # a virtio-blk disk, created afresh in target/testbench/ for every boot
size_kib = 4096
image = "path/to/initial-contents.img"   # optional; copied to the start of the disk

[net]                        # a virtio-net card on QEMU's user-mode network
forward = [22]               # guest TCP ports reachable from the host
```

The guest reaches nothing outside QEMU (`restrict=on`): only forwarded connections come in. Each
boot gets its own host ports, chosen by the OS, so benches running side by side do not collide.

## SSH sessions

Sessions start once every `expect` has matched and run concurrently while the bench keeps
watching the console (a panic still fails the case). They drive the host's OpenSSH `ssh` through
the forwarded port, logging in with deterministic test keys (the seed is the key's name):
`cargo testbench --ssh-key alice` prints the public key a boot manifest should list. Each
session's transcript is kept in `target/testbench/<case>-<target>-smp<N>-<session>.ssh.log`.

```toml
[[session]]
user = "alice+secrets"       # the login name
name = "vault"               # optional; defaults to `user`
key = "alice"                # optional; defaults to `user` up to any '+'
port = 22                    # optional guest port (default 22); must be in net.forward
pty = false                  # optional; true asks for a terminal, as an interactive user would
forbid = ['bob-secret']      # never in this session's output
steps = [
    { expect = 'iex\(1\)> ' },   # wait for output; matched against what earlier expects left,
                                 # so prompts without a newline work; ^ and $ match at lines
    { send = "File.read(\"/work/x\")\n" },
    { mark = "alice-ready" },    # tell other sessions this one got here
    { wait = "bob-ready" },      # wait for another session's mark
    { close = true },            # end of input
    { expect = '\[ssh exited: 0\]' },  # ssh's own messages and exit are output too
]
```

A session that fails stops the others. ssh gives up connecting when the case's time runs out, so a
failed connection reports ssh's reason.

## Self-checks: the bench can fail

Cases named `bench-*` check the bench itself (TENETS.md 6): each feature has a case that passes
only if the feature works and, where the bench can be misconfigured on purpose, a case that must
fail:

```toml
must_fail = 'regex'          # the case passes only if the bench fails it with a matching reason
```

`kind = "ssh-loopback"` runs `[[session]]`s against a host OpenSSH server (`/usr/sbin/sshd`) that
the bench starts on a free port, accepting the test keys in `authorized = ["alice", "bob"]` and
running `/bin/sh` for every login. It needs no guest, so it checks the session runner on its own.
An unprivileged sshd can log in only the user running it, so there every session logs in as that
user and `user` only chooses the key.
