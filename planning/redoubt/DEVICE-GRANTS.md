# Device grants (interim)

Built and enforced; **interim**, until device handles in startup blocks replace it (below). Tenet 2:
no ambient authority.

## The hole it closed
Stock Xous let any process `MapMemory` any physical device page, or `ClaimInterrupt` any IRQ,
first come first served: a compromised process could claim the power-off device, another driver's
registers, or its interrupt.

## Model
- **Default deny.** A userspace process may claim a device page or IRQ only if it was granted that
  exact resource. (PID 1, the kernel, is exempt.)
- **Grants ship in the signed bundle**, as a plain-text `grants` entry, one rule per line
  (`<process-name> mmio <hex-base> <hex-len>` or `<process-name> irq <decimal>`). The loader resolves
  names to PIDs and passes `Grnt` tags to the kernel (BOOT.md).
- **The kernel enforces at the two claim points:** `MapMemory` of a device address is allowed only if
  a grant covers the whole range; `ClaimInterrupt` only if a grant lists the IRQ. Otherwise
  `AccessDenied`. The kernel keeps no table: it scans the `Grnt` tags at each claim.
- **Physical RAM cannot be named at all:** `MapMemory` with an explicit physical address inside RAM
  is refused, and anonymous pages are zeroed.

Tests: `grant-attack` (ungranted process denied), `mem-attack` (RAM by address refused, pages zeroed).
Bench cases declare grants with `[[grant]]` tables; the bench writes the `grants` entry.

## Replacement (decided)
MMIO regions (with a DMA flag) and IRQs become kernel device objects reached through handles. `init`
receives all of them and places each driver's handles in its startup block, as the boot manifest
says; mapping a device and claiming an interrupt take a handle. Then the `grants` entry, the `Grnt`
tags and the claim-time scan are deleted, and devices get delegation and revocation from the
capability mechanism (CAPABILITIES.md). `xous-names` (name lookup) is deleted too.
