# Device grants (retired)

Retired in WP-K6. Devices reach a process only as handles to kernel device objects (KERNEL-SPEC.md).
This note records what grants were and what refuses them now. Tenet 2: no ambient authority.

## What grants were
The call interface Redoubt inherited let any process map any physical device page, or claim any
IRQ, first come first served. Grants closed that as an interim measure: a plain-text `grants` entry
in the signed bundle listed, per process name, the MMIO ranges and IRQs it might claim. The loader
passed them to the kernel as `Grnt` tags, and the kernel checked each claim against them.

## What replaced them
MMIO regions (with a DMA flag) and IRQs are kernel device objects reached through handles. Mapping
a device takes its handle, and a driver waits for its interrupt with `receive` on the IRQ handle
(KERNEL-SPEC.md). Physical RAM cannot be named at all, and anonymous pages are zeroed. For now the
bundle's first program holds every device object (INTERIM, until `init` places each driver's
handles in its startup block, as the boot manifest says; INIT.md). Delegation and revocation come
from the capability mechanism (CAPABILITIES.md).

## What refuses them now
- The loader refuses a bundle entry named `grants` (`loader-rejects-grants`).
- The kernel refuses a `Grnt` boot argument, so a loader that still passed one cannot grant anything.
- The bench refuses a `[[grant]]` table in a case.
- The old claim calls are unknown numbers, `InvalidArgument` (`legacy-gone`).
