# Device grants (closing ambient authority)

Status: designing, 2026-09-18. Tenet 2: "no ambient authority — a process can touch only
what it was explicitly given: memory it mapped, connections it holds, devices it was granted."

## The hole
Today any process may `MapMemory` any physical device page, or `ClaimInterrupt` any IRQ,
first-come-first-served. So a compromised driver (or any bundled process) can claim the
power-off device, another driver's registers, or steal its interrupt. That is ambient
authority: authority a process has merely by asking, not by being given.

## Model: capabilities granted at build time, enforced by the kernel
- **Default deny.** A userspace process may claim a device page or IRQ only if it was
  granted that exact resource. No grant, no access. (PID 1, the kernel, is exempt.)
- **Grants are declarative and part of the bundle.** Each process's device needs are
  listed in a manifest that ships in the boot bundle. The bundle is the trust root (it
  will be signed; see verified boot), so the manifest is as trusted as the code.
- **The kernel enforces, at the two claim points**: device `MapMemory` and `ClaimInterrupt`.
  Main-RAM mappings (heap, stacks, IPC pages) are unaffected — those are not devices.

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

Same tag framing as everything else (see BOOT.md). The kernel does not store these; it
scans the `Grnt` tags on demand at a claim, exactly as `process_name` scans `PNam`. Claims
are not hot, and this needs no new global or init ordering.

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

## Not covered here
- Revocation and dynamic grants (a spawner handing a device to a child at runtime): later,
  once there is runtime process creation.
- The manifest's integrity depends on verified boot, which is still open.
