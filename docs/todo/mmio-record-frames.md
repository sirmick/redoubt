# Records at a device mapping are attacked for only some calls

## What

Every call that reads or writes a record checks each slot's frame: backed, aligned, readable
(writable for an output), RAM by its physical address, and credited to the caller. So a record
placed at a device mapping is refused. A case attacks this for a `call` body and for the
`budget_create` and `budget_usage` records. The `send`, `reply`, `receive` and `process_start`
records go through the same check but no case places them at a device mapping.

## Why it matters

Without the RAM check, the kernel would read or write a device's registers as if they were a
record, through the physmap on rv64, or stop on an assertion on rv32. Only a process holding a
device object can place a record there, so the exposure is a compromised driver.

## Where

- [`kernel/src/redoubt.rs`](../../kernel/src/redoubt.rs): `record_frames`, `read_record`.
- [`kernel/src/arch/riscv/mem.rs`](../../kernel/src/arch/riscv/mem.rs): `user_frame`.
- [`tests/ipc-outcomes.toml`](../../tests/ipc-outcomes.toml): the cases that exist.
- The page: [ABI](../kernel/abi.md#residual-risks).

## Done when

A case places a `send`, `reply`, `receive` and `process_start` record at a device mapping and
each gets `InvalidArgument` with nothing read from or written to the device.
