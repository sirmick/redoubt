// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(windows, unix))]
mod hosted;
#[cfg(any(windows, unix))]
pub use hosted::*;

#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod riscv;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub use crate::arch::riscv::*;

#[cfg(all(target_arch = "riscv64", not(baremetal)))]
mod riscv;
#[cfg(all(target_arch = "riscv64", not(baremetal)))]
pub use riscv::*;

