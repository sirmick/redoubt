# Linux on reserved cores

## Idea

On hardware too messy to write drivers for (a board with no IOMMU and no hypervisor extension),
Redoubt takes some cores and Linux the rest. Linux runs the hardware and serves virtio devices to
Redoubt through one shared window of memory; Redoubt runs its ordinary virtio drivers
([tenet 7](../TENETS.md#7-devices-speak-virtio)).

## Why it is not a goal

Every milestone runs on QEMU, where the host serves virtio. On such a board **Linux is trusted**: a
compromised Linux, or its boot chain, which also supplies the device tree and the random seed,
owns the machine, and Redoubt does not defend against it. That is a weaker configuration than any
milestone claims.

## What it would need

- **A partition the firmware enforces:** RustSBI domains assigning harts, RAM and MMIO to a Linux
  domain and a Redoubt domain, with PMP keeping each domain's harts out of the other's memory and
  SBI calls kept within a domain. RustSBI's domain support is unchecked today, so this waits until
  it exists and is attacked on QEMU `virt` ([boot](../kernel/boot.md#residual-risks)).
- **One shared window** for the virtio rings and buffers, with Linux running the device side in a
  small userspace backend.
- **Doorbells** across domains: polling first, then a hardware mailbox or a small SBI extension.
- The platform stating plainly that Linux is in the trusted computing base.

**Attack cases:** a Redoubt hart cannot read Linux's memory or the reverse outside the window; a
device answer through the window is treated as a hostile device's, as on QEMU.
