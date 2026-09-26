# Disk encryption

## Idea

Every block is encrypted with authenticated encryption and covered by a Merkle root, keyed through
`keyd`, in `blkd` or a server above it, so a hostile disk can neither read the data nor change it
undetected.

## Why it is not a goal

On QEMU the host is the disk, and the host is trusted
([host virtio emulation](../TENETS.md#host-virtio-emulation)); on the FPGA platform the host is the
root of trust. Encryption defends against a disk outside the trust boundary, which no planned
deployment has. Today a lying disk is a failure, never corruption, but its contents are readable by
the host ([blkd](../servers/blkd.md)).

## What it would need

- A platform whose disk is outside the trust boundary, which is what makes it worth building.
- Keys sealed in `keyd` and never released; per-volume keys, so a volume's label set also governs
  its key.
- A Merkle root per volume kept where the disk cannot roll it back, with the same protection as the
  update counter ([packages](../servers/pkg.md#system-updates)).

**Attack cases:** a changed, swapped or replayed block is refused, and the refusal is a failure,
never a crash ([R52 (a lie is a failure, never corruption)](../servers/blkd.md#r52-a-lie-is-a-failure-never-corruption));
a disk image read on another machine yields nothing.
