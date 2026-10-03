// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

pub mod sbi;
pub use sbi::rand;

/// Platform initialization that must not depend on the memory manager or on process
/// state. Runs first thing at boot, so that early panics can be reported.
pub fn early_init() { self::sbi::early_init(); }

/// Power the machine off, reboot it, or power it off reporting a system failure
/// (`system_reset`; kernel/devices.md). The kernel owns no
/// reset device: on every platform we support the firmware does it (SBI SRST), which is also
/// how a kernel panic ends a test run (kernel/boot.md).
///
/// This never returns. A firmware that refuses is a violated invariant -- the machine was
/// asked to stop and did not -- so it panics rather than carrying on with a process that
/// believes it powered the machine off (tenet 2, fail closed and loudly).
pub fn reset(kind: redoubt_sys::ResetKind) -> ! {
    self::sbi::reset(kind);
    panic!("system_reset: the firmware did not stop the machine");
}

/// Platform specific initialization.
pub fn init() { self::sbi::init(); }
