// SPDX-License-Identifier: MIT OR Apache-2.0

//! Timer backend for platforms where the kernel has no timer of its own: a userspace
//! server drives an MMIO timer like any other device (Precursor, bao1x).

pub fn owns(_irq: usize) -> bool { false }

pub fn mask() {}

pub fn unmask() {}
