# Redoubt

A small, auditable microkernel operating system in pure Rust, plus the runtimes that run on
it. It is built to stay defensible against a capable, well-resourced adversary — including one
that has read all of its source.

Redoubt is a hard fork of [Xous](https://github.com/betrusted-io/xous-core). It keeps Xous's
shape — an MMU-backed microkernel where drivers and services are unprivileged userspace
servers talking over IPC — and rebuilds it for 64-bit and 32-bit RISC-V on one clean,
width-generic code path. The `redoubt` syscall ABI keeps the heritage protocol's shape.

The userspace runtime is **beamlet**, a safe-Rust BEAM (Erlang/Elixir) VM. The architecture is
runtime-neutral, so other runtimes can live beside it; today OTP is the only one.

## Layout

| Path | What |
| --- | --- |
| `bios/` | M-mode firmware — a vendored, pinned RustSBI checkout (the Prototyper). |
| `loader/` | The S-mode boot loader, both widths: verifies the signed bundle, builds Sv32/Sv39. |
| `kernel/` | The microkernel (the TCB): memory, threads, IPC, interrupts, the timer. |
| `servers/` | Unprivileged trusted servers (`init`, `keyd`, `fsd`, …); one crate each, a thin `bin`. |
| `userland/otp/` | beamlet, the OTP/BEAM runtime (`vm`, `crypto`, `re`, `cli`, `lib`, `tests`). |
| `libs/` | Runtime-neutral crates: `sys` (ABI), `rt`, `paging`, `wire`, `signing`, `littlefs`, `flatipc`. |
| `libs/abi/` | The legacy full syscall ABI/runtime; being folded into `sys` + `rt`. |
| `image/` | Recipes and static content for the boot bundle and the disk image. |
| `tools/` | Host tools: `testbench` (build + boot + assert), opcode/table generators. |
| `tests/` | Bench cases (`*.toml`), test keys/data, and `programs/` injected into bundles. |
| `docs/` | The architecture of record — start at [its index](docs/README.md). |
| `reference/` | Third-party reference sources (AtomVM, Elixir, OTP) for differential work. |
| `toolchains/` | Pinned OTP 28 / Elixir 1.20 archives beamlet's tests need. |
| `vendor/` | Small vendored patches (e.g. `getrandom`). |

## Build, launch, test

Top-level scripts wrap the boot chain; `--arch rv32|rv64` picks the width (default rv64).

```sh
./build    --arch rv64              # compile the kernel + loader (+ --programs for test programs)
./launch   --arch rv32              # print the exact QEMU line, boot, serial on stdin/stdout
./launch   --arch rv64 --program log-server --smp 4
./launch   --arch rv64 --print-only # just show the QEMU command
./test     --arch rv64 timer        # the boot-test bench (filter, --list, ...)
./mkimage                            # signed boot bundle -> target/image/redoubt.bundle
```

`launch` attaches the guest's serial console to the terminal (`-nographic`, Ctrl-A X quits).
rv64 boots QEMU's bundled OpenSBI by default; rv32 boots the RustSBI Prototyper from `bios/`.
Build the firmware first if `bios/target/` is empty:

```sh
./scripts/build-bios.sh
```

The full suite (both widths) is also reachable directly:

```sh
cargo testbench                 # everything, rv32 + rv64
cargo testbench --arch rv64
cargo testbench rng             # cases whose name contains "rng"
cargo testbench --list
```

It boots real images under QEMU and asserts on the console, including adversarial cases
(tampered bundles, corrupted ELFs, syscall attacks). Console logs land in `target/testbench/`.

## The development environment

Everything runs inside the container; the host needs only Docker.

```sh
./dev.sh                 # build the image (first time), then a shell in /work
./dev.sh ./test          # run one command and exit
./dev.sh --rebuild       # rebuild the image after editing the Dockerfile
```

The image carries Rust (with the RISC-V targets), QEMU for both widths, OpenSSH, Node 22 and
the agent CLIs (pi, Claude Code, Codex). Nothing is required from the host but Docker.

### The sandbox boundary

`dev.sh` mounts **only this directory** (as `/work`) plus the three CLIs' config/auth dirs, so
an agent launched inside cannot read the rest of your home. The exact, auditable list is at the
top of `dev.sh`. The Docker socket is deliberately never mounted. To run an agent confined,
start `pi`, `claude` or `codex` **from inside** the `./dev.sh` shell rather than on the host.

### Caches

Cargo is redirected inside the workspace so nothing is written outside it:

- `CARGO_HOME=/work/.cargo`, `RUSTUP_HOME=/work/.rustup`

They are disposable: `rm -rf .cargo .rustup` reclaims the space. Build output lives in
`target/` (disposable), and generated boot bundles and disk images in `target/image/`.

## Heritage

Redoubt began as Xous by the betrusted.io project; the microkernel design, the syscall ABI, and
much of `libs/abi` come from there. Redoubt drops Xous's Precursor/Baochip hardware support and
its 32-bit-only, single-core, PDDB-centric assumptions, and takes the design 64-bit, SMP-ready,
and filesystem-bearing. See `docs/HISTORY.md` for what changed and why.