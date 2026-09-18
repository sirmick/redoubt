# xous64

Fork-specific code for the RV64 / SMP / filesystem work. Plan and design notes: `planning/xous64/`.

| Path              | What                                                                          |
| ----------------- | ----------------------------------------------------------------------------- |
| `../loader64/`    | Loader for SBI + device tree platforms                                        |
| `test-programs/`  | `no_std` programs that run inside Xous: `log-server`, `ipc-client`, `timer-test`, `uart-echo` |
| `testbench/`      | Host tool: builds, injects programs, boots QEMU, asserts on the console       |
| `tests/`          | Test cases for the bench, one TOML file each                                  |

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

`kind = "build"` with `package` and `features` only checks that something compiles for the target. It
covers configurations QEMU cannot boot, such as the Precursor kernel.

In-guest programs print through `log-server` (`test_programs::Logger`) and finish with
`<NAME> TEST PASSED` or `<NAME> TEST FAILED`.

## Poking at it by hand

    loader64/run-qemu.sh target/riscv64imac-unknown-none-elf/release/uart-echo     # Ctrl-A X to quit
