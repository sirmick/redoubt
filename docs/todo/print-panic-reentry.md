# The kernel's print can re-enter on a panic

## What

The kernel's `print!` writes through a `&mut` to the one console `Output`, a `static mut`. If
something panics inside that write (a formatting implementation, the serial driver), the panic
handler calls `println!` to report it, which takes a second `&mut` to the same `Output` while the
first is still live, and then powers the machine off. Two live mutable references to one object
are undefined behaviour in Rust, whatever the power-off that follows. The `SAFETY` comment on the
access names this as a known residual.

## Why it matters

The panic path is the kernel's last word: what it prints is how a failure is diagnosed, and
undefined behaviour there can garble or lose exactly that line. It is also an `unsafe` block whose
justification admits a case it does not cover, which the unsafe budget's rule does not allow to
stand ([the unsafe budget](../testbench.md#the-unsafe-budget)).

Fixed in the kernel follow-up package after the documentation rewrite, before the work on `init`
and the manifest.

## Where

- [`kernel/src/debug/console.rs`](../../kernel/src/debug/console.rs): `print` and `OUTPUT`.
- [`kernel/src/arch/riscv/panic.rs`](../../kernel/src/arch/riscv/panic.rs): `handle_panic`.
- The page: [the kernel](../kernel/README.md#residual-risks).

## Done when

- The panic handler never takes a reference to `Output` while a `print!` may hold one: a flag set
  around the write sends a panic during printing straight to the firmware's stateless console.
- A checked-build case panics inside a print and shows the panic line and a clean power-off.
