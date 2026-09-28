# The raw system call beside the runtime

## What

`redoubt_rt::abi` is `redoubt-sys` itself, and on the machine its `syscall` is a safe function
that makes any system call: `unmap` and `process_map` included, on any address. Safe code in a
program built on the runtime can unmap a page the heap, a `Buffer` or a lend still owns, which the
runtime's own `unmap` is private to prevent. The test rig's DMA probe (`tests/net/src/rig.rs`)
uses it to free a page nothing in the runtime owns; the bench's test programs and the loader stub
call `redoubt-sys` directly.

## Why it matters

The runtime's claim ([native programs](../userland/native.md#redoubt-rt-the-native-runtime)) is
that safe code never holds memory it no longer has. While the raw call is one re-export away, the
claim holds only for programs that choose not to make it, and nothing checks that choice.

## Where

- [`libs/rt/src/lib.rs`](../../libs/rt/src/lib.rs): `pub use redoubt_sys as abi`.
- [`libs/sys/src/ecall.rs`](../../libs/sys/src/ecall.rs): `syscall`.
- [`tests/net/src/rig.rs`](../../tests/net/src/rig.rs): the DMA probe's page.
- The page: [native programs](../userland/native.md#redoubt-rt-the-native-runtime).

## Done when

- A program built on the runtime cannot make a raw system call from safe code: the runtime
  re-exports the ABI's types without `syscall`, or makes the raw call `unsafe` with its contract
  stated, and a `compile_fail` test shows the safe route gone.
- The DMA probe's page is freed through an owner (a `dma_alloc` that returns one), not a raw call.
