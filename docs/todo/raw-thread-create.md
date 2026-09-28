# Starting a thread from safe code

## What

`redoubt_rt::handle::thread_create(entry, sp, arg)` is a safe function that starts a thread at
any address with any stack. Safe code can hand it a stack inside a page the heap or a lend still
owns, or an entry that is not a function, and the new thread then writes that memory or runs
whatever is there. `consoled` and `netd` call it, each with a stack from `map_anon` and an
`extern "C" fn(usize) -> !` entry.

## Why it matters

The runtime's claim ([native programs](../userland/native.md#redoubt-rt-the-native-runtime)) is
that safe code never holds memory it no longer has; a thread whose stack is an owner's memory
breaks it as surely as an `unmap` would, and every server is built on the runtime.

## Where

- [`libs/rt/src/handle.rs`](../../libs/rt/src/handle.rs): `thread_create`.
- [`servers/consoled/src/bin/consoled.rs`](../../servers/consoled/src/bin/consoled.rs),
  [`servers/netd/src/kernel.rs`](../../servers/netd/src/kernel.rs): the callers.
- The page: [native programs](../userland/native.md#redoubt-rt-the-native-runtime).

## Done when

- Starting a thread takes an entry that is a function (`extern "C" fn(usize) -> !`) and a stack
  the runtime owns and gives up for good (a `Buffer`, never unmapped while the thread may run),
  and the raw `thread_create` is the runtime's alone, so no new `unsafe` is needed at the callers.
- A `compile_fail` test shows the raw call is not reachable from outside.
