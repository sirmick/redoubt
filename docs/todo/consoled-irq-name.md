# consoled's interrupt name

## What

`consoled` takes its interrupt as a startup-block handle named `uart:irq`. The rule is one
manifest `devices` entry per device, whose objects `init` hands together to one server as `NAME`
(the register region) and `NAME-irq` (the interrupt); `blkd` already takes `disk` and `disk-irq`.

## Why it matters

Two naming conventions for one kind of handle mean two rules for `init` to write and check, and a
manifest written to one convention silently gives the other server no interrupt
([init](../servers/init.md#the-boot-manifest)). The rule's `-irq` suffix is also what the name rule
reserves, so no device name can collide with another's interrupt.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`servers/consoled/src/bin/consoled.rs`](../../servers/consoled/src/bin/consoled.rs):
  `UART_IRQ`.
- The page: [consoled](../servers/consoled.md#started-by-init).

## Done when

- `consoled` takes `uart-irq`, and its host tests and the bench's startup blocks use that name.
