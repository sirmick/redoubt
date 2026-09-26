# Device registers and DMA pages can be made executable

## What

R11 (memory) holds W^X per mapping but not per frame. `set_flags` accepts `EXECUTE` on a
device mapping and on a page `dma_alloc` returned, because the check of who owns the page lets
every non-RAM page and every held DMA frame through. So:

- a process maps one device range twice with `map_device` and makes one mapping read-execute
  while the other stays read-write: one set of registers is writable through one mapping and
  executable through the other;
- a process makes a DMA page executable while its device can still write into it.

`map_device` and `dma_alloc` themselves map read-write only, `process_map` already refuses
non-RAM and DMA frames, and lending clears the lender's own mapping, so `set_flags` is the only
hole.

The rule R11 gains: "W^X holds per frame, not only per mapping. Only RAM that a process owns is
ever executable. Device registers and DMA frames are never mapped executable: the device, or
another mapping of the same registers (in this process or a co-holder's, since a mapping
outlives its handle), can write them underneath."

## Why it matters

Only a holder of a device object can do this, so the exposure is a compromised driver (`blkd`,
the block driver; `netd`, the network driver). There, hostile device input landing in an
executable DMA buffer becomes injected code, where W^X should have left the attacker only the
driver's existing code to reuse.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/mem.rs`](../../kernel/src/mem.rs): `set_flags`, and `owned_mapping`, whose
  non-RAM and DMA-holder arms accept the page; `check_map_flags` refuses only empty flags, W+X
  and write without read.
- [`kernel/src/device.rs`](../../kernel/src/device.rs): `map_device` and `dma_alloc`, fixed at
  read-write.
- [`kernel/src/process.rs`](../../kernel/src/process.rs): `process_map`, which already refuses
  non-RAM and DMA frames.
- The page: [memory](../kernel/memory.md#r11-memory) and its residual risks; the residual on
  [devices](../kernel/devices.md#residual-risks).

## Done when

- `set_flags` refuses `EXECUTE` with `InvalidArgument` on any page that is not RAM or is a
  `dma_alloc` frame, before any page changes.
- A test pins `map_device` and `dma_alloc` at read-write.
- An attack case, `device-exec-refused`, maps a device range and asks `set_flags` for
  read-execute, and allocates a DMA run and asks the same; both are refused.
- A planted mutation that drops the check fails that case.
- R11's text on the memory page carries the per-frame sentence above, and its section status
  names the new case.
