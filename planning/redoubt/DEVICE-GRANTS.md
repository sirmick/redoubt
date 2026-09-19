# Device grants (closing ambient authority)

Built and enforced; **interim**. Tenet 2: "no ambient authority". Device handles in startup blocks
replace this mechanism (see "Replacement" below).

## The hole
Today any process may `MapMemory` any physical device page, or `ClaimInterrupt` any IRQ,
first-come-first-served. So a compromised driver (or any bundled process) can claim the
power-off device, another driver's registers, or steal its interrupt. That is ambient
authority: authority a process has merely by asking, not by being given.

## Model: capabilities granted at build time, enforced by the kernel
- **Default deny.** A userspace process may claim a device page or IRQ only if it was
  granted that exact resource. No grant, no access. (PID 1, the kernel, is exempt.)
- **Grants are declarative and part of the bundle.** Each process's device needs are
  listed in a manifest that ships in the boot bundle. The bundle is signed and verified
  (VERIFIED-BOOT.md), so the manifest is as trusted as the code.
- **The kernel enforces, at the two claim points**: device `MapMemory` and `ClaimInterrupt`.
  Physical RAM cannot be named at all: `MapMemory` with an explicit physical address inside
  main RAM is refused, and anonymous pages are zeroed (test `mem-attack`).

This is least privilege: even a trusted driver that is later compromised cannot reach a
device it was not granted.

## Manifest
A plain-text tar entry named `grants` in the boot bundle, one rule per line:

    # comment
    <process-name> mmio <hex-base> <hex-len>
    <process-name> irq <decimal>

`<process-name>` is the bundle file name (the same name the loader uses for PNam). A
process with no lines gets nothing. Text, not TOML: the loader is `no_std`, and a
line-oriented format needs no dependency and is trivial to audit.

## Wire format (loader -> kernel)
The loader resolves names to PIDs and emits one `Grnt` argument tag per granted process:

    data[0]        = pid
    data[1]        = n_mmio
    data[2]        = n_irq
    then n_mmio * [ base_lo, base_hi, len_lo, len_hi ]   (u32 words, 64-bit values)
    then n_irq  * [ irq ]

Same tag framing as everything else (BOOT.md). The kernel keeps no table: at each claim it
scans the `Grnt` tags (as `process_name` scans `PNam`). Claims are rare, so no new global state.

## Enforcement
- `MapMemory(phys, ..)` where `phys` is non-null and outside main RAM: allow only if some
  `Grnt` for this PID has an mmio range covering `[phys, phys+size)`. Else `AccessDenied`.
- `ClaimInterrupt(irq, ..)`: allow only if a `Grnt` for this PID lists `irq`. Else `AccessDenied`.

## Testbench
A test case declares grants; the bench writes the `grants` file into the bundle:

    [[grant]]
    program = "uart-echo"
    mmio = ["0x10000000:0x1000"]
    irq = [10]

Existing device-using programs (log-server, uart-echo, the uaf holder) get UART grants;
the timer/rng/ipc clients need none. Attack test: a process with no grant is denied both
the UART page and an IRQ, and the denial is an error, not a crash.

## Replacement (decided)
MMIO regions, IRQs and DMA authority become kernel objects reached through handles. `init`
receives all of them and places each driver's handles in its startup block, as the manifest
says; `MapMemory` of a device and `ClaimInterrupt` take a handle. Then the `grants` file, the
`Grnt` tags and the claim-time scan are deleted, and devices get delegation and revocation
from the capability mechanism (CAPABILITIES.md). `xous-names` (name lookup) is deleted too.
