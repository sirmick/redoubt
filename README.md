# Redoubt

Redoubt is a small, auditable microkernel operating system written in pure Rust, built to
stay defensible against a capable, well-resourced adversary — including one that has read
all of its source.

It is a hard fork of [Xous](https://github.com/betrusted-io/xous-core). The kernel keeps
Xous's shape — an MMU-backed microkernel where drivers and services are unprivileged
userspace servers talking over capability IPC — and rebuilds it for 64-bit and 32-bit
RISC-V on one clean, width-generic code path. The `xous` syscall ABI keeps its name as the
heritage protocol the userspace runtime speaks.

## What it is

- **RV64 (Sv39) and RV32 (Sv32) from one source.** The loader, kernel and page-table crate
  are width-generic; the two ports differ only in width, and both boot QEMU `virt` through
  the same Rust firmware (RustSBI) and the same SBI/PLIC platform.
- **A microkernel.** The kernel holds only the interrupt controller, the timer, memory and
  capability IPC. Drivers and the filesystem are unprivileged servers (see the design docs).
- **Process isolation is the point.** Every process has its own address space; the kernel
  maps all of physical RAM once (the "physmap") and walks page tables in software, so it
  never switches address spaces to edit another process's tables (needed for SMP).
- **Secure by construction.** W^X is enforced by the page-table layer and re-verified at
  boot; the boot bundle is Ed25519-verified; devices are default-deny and handed to drivers
  by a signed manifest, not discovered. `unsafe` is treated as the number-one code smell and
  ratcheted down per component (`redoubt/tests/unsafe-budget.toml`); it only ever decreases.
- **All Rust, open standards.** Assembly only where it must be; RISC-V, SBI, virtio, 9P.
- **Tested to death.** `cargo testbench` boots real images under QEMU for both widths and
  asserts on the console, including adversarial cases (tampered bundles, corrupted ELFs,
  syscall attacks). The suite is green on rv32 and rv64, SMP included.

What it is deliberately **not**: the fastest, or compatible with everything.

## Layout

| Path | What |
| --- | --- |
| `kernel/` | the microkernel (the TCB) |
| `loader/` | the S-mode boot loader, both widths |
| `xous-rs/` | the `xous` syscall ABI and userspace runtime |
| `redoubt/paging/` | the typed Sv32/Sv39 page-table crate — the only code that edits PTEs |
| `redoubt/testbench/` | `cargo testbench`: build an image, boot QEMU, assert on the console |
| `redoubt/test-programs/` | `no_std` programs injected into test boot bundles |
| `redoubt/tests/` | TOML test cases and the `unsafe` budget |
| `libs/flatipc/` | zero-copy IPC |
| `planning/redoubt/` | the architecture of record — read these first |

(The userspace above the kernel is [beamlet](planning/redoubt/), a safe-Rust BEAM VM that
runs an Elixir/OTP userland.)

## Building and testing

```sh
# One-time: build the RustSBI firmware the bench boots (QEMU ships no rv32 OpenSBI).
./scripts/fetch-rustsbi.sh

# Run the whole suite on both widths.
cargo testbench

# Or one width / one case.
cargo testbench --arch rv32
cargo testbench rng
```

## Heritage

Redoubt began as Xous by the betrusted.io project; the microkernel design, the syscall ABI,
and much of `xous-rs` come from there. Redoubt drops Xous's Precursor/Baochip hardware
support and its 32-bit-only, single-core, PDDB-centric assumptions, and takes the design
64-bit, SMP-ready, and filesystem-bearing. See `planning/redoubt/` for what changed and why.
