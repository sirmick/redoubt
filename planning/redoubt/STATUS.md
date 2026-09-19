# Status against the tenets

What the code does today (not the design). The kernel's default features include `debug-proc` (PLAN.md, open work). `cargo testbench` enforces the rows marked (enforced).
Update this note with each milestone.

| Tenet | Today |
| --- | --- |
| 1 Simple | Kernel about 12.9k lines (`kernel/src`; 11.8k before WP-K1's budgets, handle tables and the Redoubt call path); interrupts dispatched one at a time over the PLIC's 1024-source space; loader about 1.1k, `paging` crate about 280. Trap entry and exit are `global_asm!` in `arch/riscv/asm.rs`. Kernel globals are `KernelCell`, not `static mut` (two exceptions, each only ever addressed: the SBI console and the SMP spike's secondary stack); an `smp` feature turns `KernelCell` into a spinlock: host stress-tested, and a two-hart spike (`smp-spike`) starts a second hart through SBI HSM that runs kernel code and contends on it without losing updates. Default builds run only the boot hart. ARM, x86, the in-kernel gdb stub, Precursor, bao1x, VexRiscv, swap and the prebuilt assembly blobs are deleted. New for milestone 1 (not yet used by the kernel): `redoubt-sys`, the ABI, about 1.1k lines with one `unsafe`; `redoubt-wire`, the codecs, about 1.3k lines plus a host-only generator. `littlefs` about 2.7k lines, no `unsafe`, no dependencies (for `fsd`, not yet used). |
| 2 No ambient authority | Devices: (enforced) default deny; a process maps a device page or claims an IRQ only if the bundle's `grants` entry granted it (DEVICE-GRANTS.md, interim; the boot manifest is designed, INIT.md). Physical RAM cannot be named by address, and anonymous pages are zeroed. Server connections are still stock Xous password capabilities (128-bit IDs): no handles, badges or revocation yet. |
| 2 W^X | (enforced) `paging::Pte::leaf` cannot express a writable and executable mapping; syscalls asking for one get `InvalidArgument`; the physmap alias of kernel code is read-only; the kernel checks its own address space at boot. Known gap: user code pages have a writable alias in the kernel-only physmap. |
| 2 Verified boot | (enforced) The loader verifies an Ed25519 signature over the whole bundle and refuses to boot otherwise. One public development key. Open: M-of-N, rollback protection, verifying the loader itself. |
| 2 Fail closed | No usable RNG seed or a bad bundle signature: refuse to boot. A kernel panic powers off. |
| 2 `unsafe` budget | (enforced) Every `unsafe` in the kernel, loader and `paging` has a SAFETY justification (0 undocumented; 211 undocumented at the fork). Budgets: `paging` 12, loader 17, SBI/PLIC/timer backends 15, RISC-V arch 10, kernel core 21. Totals only fall. |
| 3 All Rust | Kernel and loader: Rust plus `global_asm!`, no C toolchain, on both widths. Firmware can be pure Rust (RustSBI Prototyper; rv32 always uses it). |
| 4 Standards | SBI, PLIC, Sv32/Sv39, device tree, ELF, ustar. The kernel argument block is our own format, specified in BOOT.md. |
| 5 Dependencies | `redoubt-sys` and `redoubt-wire`: no dependencies; host-only fuzz crates (`libfuzzer-sys`, `serde_json` as an oracle) sit outside the workspace (tenet 3). Kernel about 20 crates in its dependency tree, loader 8 direct (`fdt-rs`, `sbi-rt`, `elf`, `tar-no-std`, `crc`, `ed25519-compact`, `xous`, `paging`). None vendored or formally audited. |
| 6 Tested | (enforced) 47 cases, 80 results across both widths, OpenSBI and RustSBI; attack cases listed below. The harness can fail: each bench feature added for milestone 1 (SSH sessions over host OpenSSH, virtio disk and net, bundle data entries, a required clean power-off) has a self-check, and `must_fail` cases pass only if the bench fails for the stated reason. A missing firmware or OpenSSH fails a case rather than skipping it. Host unit test: `cargo test -p xous` round-trips every syscall `Result` variant through its register encoding. No kernel fuzzing yet; the kernel's hosted unit tests are not wired into the bench. Host tests, not yet in the bench: `cargo test -p redoubt-sys` round-trips every KERNEL-SPEC.md call, result, error and record and refuses every malformed encoding; `cargo test -p redoubt-wire -p redoubt-wire-gen` (9P, typed-message and JSON vectors, the generator's drift test). Fuzz targets: `redoubt/sys/fuzz`, `redoubt/wire/fuzz` (no findings outstanding). Attack cases take their verdict from the system: `log-server` prefixes every relayed line with the sender's PID from the kernel, and verdicts come from the kernel or loader, a victim, or `attack-checker`; `wx` and `irq-attack` prove survival only until WP-K4 and WP-K3. `littlefs` (host, `cargo test --release` in `redoubt/littlefs`): model-based operations with handles held across removes and renames, crash injection at every block write, hostile images; by hand, differential tests against the C reference (`redoubt/littlefs/diff`) and two fuzz targets. `cargo test -p redoubt-rt` (74 cases: startup blocks, `check`, admission, the 9P skeleton against hostile clients, the echo pair against a fake kernel); `rt-build` checks it compiles for both widths; fuzz targets in `redoubt/rt/fuzz`. Six cases boot a kernel and loader built with debug assertions and overflow checks (the `checked` profile), so `core`'s preconditions on every raw-pointer call are enforced on a real boot; `bench-debug-assertions` checks that the mode reaches the kernel. Not covered: a malformed argument block, since the loader is its only writer, so the block's bounds are asserted rather than tested. |
| 7 Virtio | Nothing built; the only driver is the ns16550 UART in test programs. |

## Test cases (`redoubt/tests/`)
| Case | Checks |
| --- | --- |
| `ipc` | Every message type across address spaces: scalar, blocking scalar, lend, lend_mut, move |
| `timer` | Hart timer as IRQ 0: ownership, timebase, one-shot ticks re-armed from the handler |
| `uart-irq` | External interrupt through the PLIC to a userspace handler, with injected input |
| `rng` | Kernel RNG seeded from the device tree: server IDs differ within and between boots |
| `all-together` | IPC and timer tests sharing one log server |
| `rustsbi-boot` | The same loader and bundle boot under RustSBI |
| `kernel-wx` | The kernel's code is not writable, directly or through the physmap (attack) |
| `wx` | No writable and executable mapping through the syscall interface (attack) |
| `irq-attack` | Hostile interrupt syscalls: out-of-range, unowned, doubly claimed (attack) |
| `grant-attack` | An ungranted process is denied every device page and interrupt (attack) |
| `mem-attack` | Physical RAM cannot be mapped by address; anonymous RAM is zeroed (attack) |
| `uaf-lent-page` | A frame lent out by a dying process is not reused under the borrower (attack) |
| `loader-rejects-kernel-address` | A program segment in the kernel's range is refused (attack) |
| `loader-rejects-kernel-entry` | A program entry point in the kernel's range is refused (attack) |
| `verified-boot-rejects-tamper` | A bundle changed after signing is refused (attack) |
| `smp-spike` | With the `smp` feature, a second hart started through SBI HSM contends on the spinlock big kernel lock without losing updates |
| `unsafe-budget` | The `unsafe` ratchet |
| `lend-untouched-page` | Lending, mutably lending or moving never-touched pages is served on 1 and 4 harts; a half-mapped range is refused whole; backing more than RAM is the caller's `OutOfMemory` (attack) |
| `move-borrowed-page` | A server cannot move on a page it was only lent; the lender gets it back intact (attack) |
| `return-lent-unmapped` | A lender cannot unmap or remap its own lent page; the server's return survives (attack) |
| `syscall-attack` | Hostile heap and oversized-message arguments get errors, not a panic (attack) |
| `touch-beyond-ram` | Touching reserved memory beyond RAM ends the process, not the kernel (attack) |
| `budget` | Carving and charging (R6, R7), labels (I6), a subtree destroyed and its handles swept (R10, I2, I10), 128 handles per table page, usage within limits (I5), 500 create-destroy cycles, `time_now`, `random` |
| `budget-destroy-kills` | Destroying `system` kills every process in it, the caller last (R10); the verdict is the kernel's own lines |
| `budget-mem-churn` | `system`'s usage stays exactly stable over 64 map/touch/unmap rounds and 64 lend/return cycles, on 1 and 2 harts |
| `budget-carve-attack` | Carving beyond a parent's pages, processes or weight, with the largest values and wrapping sums, is refused (attack) |
| `budget-destroy-attack` | Handles to a destroyed subtree are all `BadHandle`, before and after frames and indices are reused (attack) |
| `budget-forge-attack` | 540 forged handle indices get `BadHandle` (attack) |
| `budget-table-attack` | A table filled to `MAX_HANDLES` gets `TooLarge`; each page is charged and freed (attack) |
| `budget-syscall-attack` | Hostile arguments to every K1 call, in the spec's order of checks, and 4000 fuzzed calls: errors, never a panic (I14) (attack) |
| `loader-rejects-truncated-elf` | A program image cut short inside its program headers is refused (attack) |
| `bench-*` | The bench's own self-checks (including `bench-attack-forgery`: a client cannot forge an unprefixed or another PID's line): SSH sessions (loopback against host `sshd -i`, and to the guest's forwarded port), virtio devices, bundle data entries, console reading after the last expect, required power-off; the `must_fail` ones pass only when the bench catches the fault |
