# History

What was done, and what we learned. Current state: STATUS.md. Next: PLAN.md.

## Fork and first boot (2026-09-15 to 09-18)
- Hard fork of betrusted-io/xous-core at `c025441`.
- New S-mode loader for SBI and device-tree platforms (`global_asm!` entry, no C toolchain); the
  boot bundle became a signed ustar of ELFs, replacing Xous's `create-image` tool and its MiniELF format.
- rv64 kernel: widening 11 `target_arch = "riscv32"` gates left one compile error; every other rv32
  assumption was invisible to the type checker. Sv39 through a direct physmap (MEMORY-LAYOUT.md),
  trap entry as `global_asm!`, upstream `riscv` crate for CSRs, SBI console, PLIC backend
  (rustsbi `plic` crate), hart timer as IRQ 0 (BOOT.md), RNG keyed from `/chosen/rng-seed`.
- Milestone: boots to userspace on QEMU `virt` and passes every IPC message type.
- Test bench (`cargo testbench`): declarative TOML cases, bundles injected, console assertions,
  forbidden patterns, ELF corruption, firmware selection, `--run` and `--debug`.

## Bugs found
- **64-bit ABI:** syscall results were returned by reading the `repr(C)` `Result` enum as registers,
  which works only if every field is one word; now serialized with `to_args()`. The same class
  (memory punned as register arrays, `u32` fields in ABI types) remains to audit in `xous-rs` and
  `xous-ipc`.
- **`scause` on rv64:** interrupt causes were matched on bit 31; the flag is the top bit.
- **Upstream: `FreeInterrupt` off by one.** Any process could panic the kernel with
  `FreeInterrupt(32)`. Guarded by `irq-attack`. Worth reporting upstream.
- **Upstream: cross-process use-after-free of lent pages.** A process that died while a page was
  lent had the frame freed and reused under the borrower: `release_all_memory_for_process` passed a
  physical address where a virtual one was required. Fixed by walking the dying process's page
  table and reparenting lent frames. Guarded by `uaf-lent-page`. Worth reporting upstream.
  Remaining: a borrower returning a page to a dead lender may leak the frame (not a safety hole).
- **`MapMemory` on physical RAM (found by review round 1's red team):** an explicit physical
  address inside RAM skipped the device-grant check and was not zeroed, so any process could map a
  free frame and read a dead process's memory. Fixed in `c36c220`; guarded by `mem-attack`.
- **`xous-rs` `Result` marshalling:** `Unimplemented` encoded to another variant's opcode, and
  `UnknownResult` decoded shifted by one slot; both invisible to the compiler. Fixed in `8c21f32`,
  with a round-trip unit test over every variant.
- **Device tree parser:** the `fdt` 0.1.5 crate mis-parsed RustSBI's valid, re-serialized tree and
  panicked. Moving the loader to `fdt-rs` made it boot under RustSBI; RustSBI had been blamed first.

## Hardening pass (2026-09-18)
Typed page tables (the `paging` crate) with W^X by construction and a boot-time self-check;
`KernelCell` for globals; RNG and verified boot fail closed; default-deny device grants; the `unsafe`
ratchet (211 undocumented uses at the fork, 0 now); ARM, x86 and the in-kernel gdb stub deleted.

## rv32 re-homed (2026-09-18)
rv32 and rv64 now share one loader, one SBI/PLIC/timer platform and one physmap design, differing
only in width; both boot on QEMU under RustSBI (QEMU ships no rv32 OpenSBI). Precursor, bao1x,
VexRiscv, the Sv32 window scheme, swap and the prebuilt assembly blobs were deleted.
Width bugs found at first rv32 boot, all "assumed rv64":
- `PHYSMAP_BASE + phys` is right only when the physmap starts at physical 0; use `physmap_virt()`.
- The loader writes 64-bit `XArg` and `MREx` fields on both widths; parsing them through `usize`
  overflowed a `<< 32` on rv32.
- The PLIC sits at its own rv32 root entries, so the loader must pre-create shared intermediate
  tables before user address spaces copy the kernel roots.
- Reading the 64-bit `time` CSR on rv32 needs `rdtimeh`/`rdtime` with a wrap-retry loop.

## Interrupts (2026-09-18)
The interrupt path carried pending IRQs as a `usize` bitmask capped at 32 entries, though the
controller only ever reports one at a time. `cf101f8` dispatches one interrupt and sizes the handler
table to the PLIC's 1024 sources. The hart timer was delivered as IRQ 0 with `PlatformSpecific`
deadline calls (BOOT.md), pending the kernel-owned design.

## beamlet (sibling repository)
In parallel, beamlet reached: OTP's `crypto`, `public_key` and `ssl` running unmodified on Rust
natives; Elixir's compiler and its own test suite; console input and IEx; mailbox overflow killing the
receiver.

## Design v1 (tag `redoubt-design-v1`)
Tenets, then capabilities and principals (agents as first-class principals), containment (labels),
resources (budgets, scheduling), init and the steward, 9P namespaces, packages, I/O architecture.
Decisions made along the way: no dynamic linking; the BEAM is not in the TCB; littlefs over RedoxFS;
9P for everything user-facing; revocation by budget rather than a derivation tree.

## Review round 1 (tag `redoubt-design-v2`)
Red team, simplifier and editor. Thirteen decisions:
1. Revocation by budgets only; stamps inherited from the requesting capability; leases in the kernel.
2. No rights bits on handles.
3. The kernel owns the hart timer; receive takes a timeout.
4. No overcommit; a 1 ms user clock (dropped in round 2).
5. Device authority becomes handles; `xous-names` deleted; RAM never nameable; pages zeroed.
6. Labels static per budget; reads above them fail; dynamic taint deferred.
7. Approvals only out of band, like 2FA; made a tenet.
8. Two scheduling classes and runtime-charged stride; donation and CPU quotas deferred.
9. Native user code stays; trust lists and profiles become steward state.
10. launcherd into the steward; auditd deferred; one restart rule; `keyd` stays separate.
11. Admission per budget; crash quarantine (replaced in round 2).
12. Browser GUI, link layer and router, Linux partition to Later; Linux there is TCB.
13. Corrections: IP-prefix sockets, no persisted authority, M-of-N, per-item declassification,
    closed options.

## Review round 2 (tag `redoubt-design-v3`)
The same three roles, attacking the v2 fixes. Fourteen decisions:
1. Calls between user budgets need equal labels, sends may go up; system servers enforce no read up
   and no write down from the caller's label set.
2. Labels per volume, not per file; declassification copies an item.
3. More labels than the parent only via the steward; budget usage and exit messages obey labels.
4. Normal and vault sessions; a terminal is cleared for its authenticated owner's labels only.
5. Mint takes a budget handle (the stamp or a descendant); 64-bit never-reused budget ids.
6. Approval rendering stripped and steward-named; requests bound by id and hash; `approve-hs` with a
   FIDO key.
7. Budgets are charged; admission per principal; crash blame after 3 consecutive crashes means logout.
8. The loader loads only the kernel and `init`; every process maps its own ELF via the loader stub.
9. IPC is `call` and `send` with lend or transfer, zero-copy, no kernel queue; transfers need receiver
   opt-in.
10. Budgets: pages, processes, weight, class, labels, deadline; one flat stride queue.
11. Shared package store deferred (per-principal packages, A/B system updates); projects as
    principals; `blkd` merged, disk encryption deferred.
12. The 1 ms clock dropped: assume the attacker has a perfect clock.
13. North star adds Alice's leased agent and a scripted hostile agent; the shell is IEx.
14. Exit messages and kill-by-budget; FPGA host and bitstream assumptions; steward state on a system
    volume; small deletions and editorial fixes.
