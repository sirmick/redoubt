# Getting started with Redoubt

Redoubt is a small, auditable RISC-V microkernel in pure Rust. Today the runtime is **beamlet**, a
safe-Rust BEAM (Erlang/Elixir) VM. The full, diagrammatic tour is **[README.html](https://sirmick.github.io/redoubt/README.html)**;
the design of record is [`docs/`](docs/README.md).

## Host requirements

**Docker** supplies the OS build/test environment: Rust with the RISC-V bare-metal targets,
QEMU for both widths, OpenSSH and graphviz. It does **not** install OTP or Elixir; beamlet's
differential and Elixir tests have the additional prerequisites in section 7.

Prefer your own environment? Install Rust + the `riscv64imac-unknown-none-elf`,
`riscv32imac-unknown-none-elf` and `riscv64gc-unknown-none-elf` targets, `qemu-system-misc`, a C
toolchain, and `graphviz` (only to regenerate `README.html`).

## 1. Enter the container

```sh
./dev.sh                 # build the image (first time), then a shell in /work
./dev.sh --rebuild       # rebuild the image after editing the Dockerfile
./dev.sh ./test          # run one command and exit
```

The container mounts only this directory as `/work` plus the agent CLIs' config dirs; nothing else
from your home. See the header of [`dev.sh`](dev.sh).

## 2. Build the firmware (once)

```sh
./scripts/build-bios.sh  # builds the vendored RustSBI Prototyper in bios/, both widths
```

Both rv32 and rv64 boot only the vendored RustSBI firmware. If `bios/target/` is already
populated for both widths, skip this.

## 3. Build the operating system

```sh
./build --arch rv64              # kernel + loader
./build --arch rv32
./build --arch rv64 --programs   # also the in-guest test programs
```

Builds are size-optimized release builds by default, which is required for the rv32 memory
layout. Pass `--debug` only when working on a configuration that fits the debug image.
`--arch` is `rv32` or `rv64`. Build, launch and image scripts default to rv64; `./test`
without `--arch` uses each case's declared architecture list, which may include both widths.

## 4. Launch it in a VM

```sh
./launch --arch rv64                       # prints the exact QEMU line, then boots; Ctrl-A X quits
./launch --arch rv64 --program log-server  # start a test program after the kernel
./launch --arch rv32 --smp 4
./launch --arch rv64 --print-only          # show the QEMU command and exit
./launch --arch rv64 --debug               # pause with a gdb stub on :1234
```

`launch` assembles and signs the boot bundle, prints the exact `qemu-system-riscv*` command line,
and wires the guest serial console to your stdin/stdout. See [`docs/DEBUGGING.md`](docs/DEBUGGING.md).

## 5. Run the tests

```sh
./test --arch rv64            # the whole bench on rv64
./test --arch rv64 timer      # cases whose name contains "timer"
./test --arch rv32 budget
./test --list
```

The bench boots real images under QEMU and asserts on the console, including attack cases whose
verdict comes from the system (the kernel, a victim, or a clean power-off), never the attacker's own
output. Console logs land in `target/testbench/`. Writing cases: [`docs/testbench.md`](docs/testbench.md).

## 6. Build an image

```sh
./mkimage                     # signed boot bundle → target/image/redoubt.bundle
```

Disk-image recipes (littlefs) live in [`image/`](image/) and are not built yet.

## 7. beamlet (the BEAM VM)

The VM currently runs on the host; its Redoubt platform and boot integration are still planned.
For differential/Elixir tests, separately install **OTP 28.5.0.6** and **Elixir 1.20.4**.
The repository and Docker image do not provide these installations. `tools/env.sh` only adds
`toolchains/otp-28.5.0.6/bin` and `toolchains/elixir-1.20.4/bin` to PATH; set
`BEAMLET_TOOLCHAINS` to another installation root with that layout, or put matching installations
on PATH yourself. Check `erl -noshell -eval 'io:format("~s~n", [erlang:system_info(otp_release)]), halt().'`
and `elixir --version` before running those suites. Pure-Rust unit tests do not require them.

```sh
cd userland/otp
. tools/env.sh                # put the pinned OTP/Elixir on PATH
cargo test                    # unit + hostile-input tests
tools/difftest                # differential tests against the real BEAM
tools/elixir-tests            # run Elixir's own suite on beamlet
```

## Regenerating the documentation page

```sh
python3 tools/gen_readme.py   # DOT → SVG → README.html (needs graphviz)
```

## Where to read next

| Want | Read |
| --- | --- |
| The tour, with diagrams | [README.html](https://sirmick.github.io/redoubt/README.html) |
| What the project believes (outranks everything) | [`docs/TENETS.md`](docs/TENETS.md) |
| Where the code stands | [`docs/STATUS.md`](docs/STATUS.md) |
| The precise kernel | [`docs/KERNEL-SPEC.md`](docs/KERNEL-SPEC.md) |
| The plan | [`docs/PLAN.md`](docs/PLAN.md), [`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md) |
| The design index + glossary | [`docs/README.md`](docs/README.md) |
