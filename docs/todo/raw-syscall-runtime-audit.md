# Raw memory calls beside the runtime's owning types

## What

`redoubt-rt` gives programs owning types for memory and IPC (a lend belongs to its call, the heap
owns its pages), and beside them the plain system calls as safe functions:
`redoubt_rt::handle::unmap(addr, len)` and `set_flags(addr, len, flags)` take any address, and
`redoubt-rt` re-exports the whole ABI as `abi`. A program written entirely in safe Rust can unmap
or make read-only a page its heap, a lent buffer or a mapped reply still owns, and the next safe
access through that owner faults or reads memory that has been reused. The runtime's own tests
cover its owning types used as designed; nothing audits what they promise when a program also
makes the raw calls.

## Why it matters

The runtime's claims ([native programs](../userland/native.md#redoubt-rt-the-native-runtime)),
that a program can never hold a safe object claiming memory it no longer has, hold only if the raw
calls cannot reach that memory. Safe Rust that can cause undefined behaviour is a soundness hole in
the library, and every server is built on it.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`libs/rt/src/handle.rs`](../../libs/rt/src/handle.rs): `unmap`, `set_flags`, `map_anon`.
- [`libs/rt/src/lib.rs`](../../libs/rt/src/lib.rs): `pub use redoubt_sys as abi`.
- [`libs/rt/src/ipc.rs`](../../libs/rt/src/ipc.rs), [`libs/rt/src/heap.rs`](../../libs/rt/src/heap.rs):
  the owning types.

## Done when

- Every runtime function that can invalidate memory a safe owner holds is `unsafe`, with the
  contract its caller must keep, or is replaced by a method on the owner; the re-exported ABI is
  documented as outside the runtime's guarantees.
- A host test shows safe code cannot unmap a page the heap or a lend owns.
