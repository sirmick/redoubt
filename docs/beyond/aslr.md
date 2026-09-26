# ASLR

## Idea

Randomise where a process's code, stack and anonymous memory are placed, using the kernel's random
words, so an exploit of a memory-safety bug cannot rely on fixed addresses.

## Why it is not a goal

Every address in a process is fixed today, and `map_anon` places pages deterministically
([memory layout](../kernel/memory-layout.md#residual-risks)). Programs are Rust, with `unsafe`
budgeted, so the bugs ASLR slows are rare; a bug that is exploited reaches only its own process's
handles; and fixed layouts keep the model, the traces and the bench reproducible.

## What it would need

- Placement drawn from the kernel's random words in the loader stub (segments, stack) and in
  `map_anon`'s search, with the spread using the rv64 address space.
- The model and trace replay taught to abstract over placement.
- A layout still the same on both widths wherever the ABI fixes it.

**Attack cases:** two launches of one program place it differently; a process cannot learn another
process's layout from anything the kernel returns.
