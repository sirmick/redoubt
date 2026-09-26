# Getting started with Redoubt

How to build Redoubt, boot it under QEMU, run the test bench, debug the kernel and render the
book. What Redoubt is, and what each part does, is in [the book](docs/README.md).

## Prerequisites

The dev container has everything the operating system needs: Rust with the RISC-V bare-metal
targets, QEMU for both widths and OpenSSH.

```sh
./dev.sh                 # build the image (first time), then a shell in /work
./dev.sh --rebuild       # rebuild the image after editing the Dockerfile
./dev.sh ./test          # run one command and exit
```

The container mounts only this directory, as `/work`, plus the agent tools' configuration
directories; nothing else from your home ([`dev.sh`](dev.sh)).

Your own machine instead needs:
- Rust (stable, and nightly for `rustfmt`) with the targets `riscv64imac-unknown-none-elf`,
  `riscv32imac-unknown-none-elf` and `riscv64gc-unknown-none-elf`;
- `qemu-system-riscv64` and `qemu-system-riscv32`;
- OpenSSH, for the bench's SSH sessions;
- `mdbook`, `mdbook-mermaid` and `mdbook-svgbob`, to render the book
  (`cargo install mdbook mdbook-mermaid mdbook-svgbob`).

## Firmware

```sh
./scripts/build-bios.sh  # builds the vendored RustSBI prototyper in bios/, both widths
```

Both widths boot only the vendored RustSBI firmware; there is no fallback to QEMU's own. Skip
this if `bios/target/` is already built for both widths. A checkout elsewhere (a worktree) can
point at built images with `RUSTSBI_PROTOTYPER` and `RUSTSBI_PROTOTYPER_RV32`.

## Build

```sh
./build --arch rv64              # the kernel and the loader
./build --arch rv32
./build --arch rv64 --programs   # also the in-guest test programs
./mkimage                        # the signed boot bundle, in target/image/redoubt.bundle
```

Builds are size-optimised release builds, which the rv32 memory layout needs. `--debug` is for a
configuration that fits a debug image. The scripts default to rv64.

**The signing key is public.** `mkimage`, `launch` and the bench sign every bundle with a
development key derived from a fixed, public seed (`DEV_SEED` in `tools/testbench/src/build.rs`),
and the loader accepts its public half (`DEV_PUBLIC_KEY` in `loader/src/verify.rs`). Anyone can
sign a bundle the stock loader boots, so a bundle built this way is for development only. A
deployment generates its own Ed25519 key pair, keeps the secret half off the machine, signs with
it, and compiles its public half into the loader in place of `DEV_PUBLIC_KEY`
([verified boot](docs/kernel/boot.md#verified-boot)). No tool in the tree signs with another key
yet.

## Run

```sh
./launch --arch rv64                       # prints the QEMU command, then boots; Ctrl-A X quits
./launch --arch rv64 --program log-server  # start a test program after the kernel
./launch --arch rv32 --smp 4
./launch --arch rv64 --print-only          # show the QEMU command and exit
./launch --arch rv64 --debug               # pause, with a GDB stub on :1234
```

`launch` assembles and signs the boot bundle, prints the exact `qemu-system-riscv*` command, and
wires the guest's serial console to your terminal.

## Test

```sh
./test --arch rv64            # the whole bench on rv64
./test --arch rv64 timer      # cases whose name contains "timer"
./test --list                 # every case, with its description
cargo testbench               # the same bench, every case on the widths it declares
```

The bench boots real images under QEMU and judges the console from outside, including attack
cases whose verdict comes from the system, never from the attacker. Logs land in
`target/testbench/`. How cases are written and judged: [the test bench](docs/testbench.md).
Before sending a change, run the whole bench, and check that rv32 still compiles.

## Debug

The kernel has no debugger; debugging is done from the host, through QEMU's GDB stub. Release builds
carry no debug information, so ask for it while keeping the optimisation (the rv32 image size needs
it):

```sh
CARGO_PROFILE_RELEASE_DEBUG=2 ./launch --arch rv64 --program log-server --debug
# then, in another shell, with a GDB that knows RISC-V (gdb-multiarch):
gdb target/riscv64imac-unknown-none-elf/release/redoubt-kernel
(gdb) target remote :1234
(gdb) break kmain
(gdb) continue
```

Keep the environment override on `launch`: it rebuilds the image, loader included. `--debug` only
pauses QEMU and opens the stub; it does not add debug information. Check that the kernel's own
sources are in it with `readelf --debug-dump=info` (a `kernel/src/main.rs` compilation unit); a
`.debug_info` section alone may hold only dependencies' sources.

## beamlet

beamlet, the Elixir VM, runs on the host. Its differential and Elixir suites also need
OTP 28.5.0.6 and Elixir 1.20.4, which neither the repository nor the container provides:
`tools/env.sh` puts `toolchains/otp-28.5.0.6/bin` and `toolchains/elixir-1.20.4/bin` on the path,
and `BEAMLET_TOOLCHAINS` names another root with that layout. The pure-Rust unit tests need
neither.

```sh
cd userland/otp
. tools/env.sh                # the pinned OTP and Elixir on the path
cargo test                    # unit and hostile-input tests
tools/difftest                # differential tests against the real BEAM
tools/elixir-tests            # Elixir's own suite on beamlet
```

## The book

```sh
mdbook build docs             # renders docs/ into target/book
mdbook serve docs             # the same, on localhost, rebuilt on every change
cargo run -q -p redoubt-doccheck   # the docs checker: status lines, IDs, links, the register
```

The pages are plain Markdown and read on GitHub as they are.
