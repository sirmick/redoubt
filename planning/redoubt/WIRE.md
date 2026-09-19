# Wire formats

Designed, not built. Owns: how bytes are laid out in messages and in human-written files. One
convention for every message, one for every file a person writes.

## Messages: 9P's convention
- **9P** is plain 9P2000 (no `.u` or `.L` extensions) with a fixed `msize` of 64 KiB, which is
  `MAX_LEND_PAGES` (KERNEL-SPEC.md). A 9P message travels in a lent buffer.
- **Typed messages** (everything that is not 9P: `blkd` <-> `fsd`, the steward, `keyd`, `sshd`
  <-> steward, `ipd`'s connect and listen operations) use **9P's own encoding**: little-endian
  fixed-size integers (`u8`, `u16`, `u32`, `u64`), strings as `u16` length + UTF-8, byte arrays as
  `u32` length + bytes. Handles travel in the message's handle slots and are named in the layout by
  slot index.
- **Word 0 is the opcode.** A message whose fixed fields fit in the remaining words carries them
  there; otherwise the whole encoding goes in the buffer and word 1 holds its length.
- **One layout per message type**, defined by a table in the owning server's note (opcode, then
  fields in order with their types). The Rust and Elixir codecs are generated from those tables, so
  sender and receiver cannot disagree. There is no self-describing format and no text command parser.
- **One codec**, shared by 9P and the typed messages, and fuzzed.

## Files people write: strict JSON
The boot manifest (INIT.md), package manifests (PACKAGES.md) and configuration are JSON under the
I-JSON profile (RFC 7493), enforced by one shared parser:
- UTF-8 only; no duplicate member names;
- integers beyond 2^53 (ids, accounts, labels, addresses) are written as strings;
- nesting at most 32 deep; a file at most 64 KiB;
- unknown members are errors, not ignored.
