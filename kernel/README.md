<!--
SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
SPDX-License-Identifier: Apache-2.0
-->


# Redoubt Kernel

This contains the core kernel for Redoubt.  It requires a stage 1 loader in
order to start up, as it assumes the system is already running in
Supervisor mode.

## Building

From the repository root, `./build --arch rv64` builds the kernel and loader;
`./build --arch rv32` selects the other supported width. The Rust targets are
`riscv64imac-unknown-none-elf` and `riscv32imac-unknown-none-elf`.
For the kernel alone:

```sh
cargo build --release --target riscv64imac-unknown-none-elf -p redoubt-kernel --features qemu-virt
```

See [Getting started](../GETTING-STARTED.md) for the Docker environment and
vendored RustSBI build, and [Debugging](../docs/DEBUGGING.md) for source debug information.

## Using

`./launch --arch rv64` builds and signs the bundle, then boots it under QEMU.
`./mkimage --arch rv64` writes a kernel-only signed bundle without booting.
The [kernel specification](../docs/KERNEL-SPEC.md) describes the target interface;
[status](../docs/STATUS.md) distinguishes implemented mechanisms from planned work.

## Testing

Run `./test --arch rv64` (or `rv32`) from the repository root. See the
[test bench guide](../docs/testbench.md) for filters, cases and attack verdicts.

## Contribution Guidelines

[![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-v2.0%20adopted-ff69b4.svg)](../docs/CODE_OF_CONDUCT.md)

Please see [CONTRIBUTING](../docs/CONTRIBUTING.md) for details on
how to make a contribution.

Please note that this project is released with a
[Contributor Code of Conduct](../docs/CODE_OF_CONDUCT.md).
By participating in this project you agree to abide its terms.

## License

Copyright © 2020

This project is licensed under the [Apache License 2.0](http://opensource.org/licenses/Apache-2.0) [LICENSE](../LICENSE). For accurate information, please check individual files.
