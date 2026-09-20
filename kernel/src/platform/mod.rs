// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "sbi")]
pub mod sbi;
#[cfg(feature = "sbi")]
pub use sbi::rand;

/// Hosted mode has no platform; random numbers come from the arch (host) layer.
#[cfg(not(baremetal))]
pub mod rand {
    pub fn get_u32() -> u32 { crate::arch::rand::get_u32() }
}

/// Platform initialization that must not depend on the memory manager or on process
/// state. Runs first thing at boot, so that early panics can be reported.
#[cfg(not(any(unix, windows)))]
pub fn early_init() {
    #[cfg(feature = "sbi")]
    self::sbi::early_init();
}

/// Power the machine off, or reboot it (`system_reset`; KERNEL-SPEC.md). The kernel owns no
/// reset device: on every platform we support the firmware does it (SBI SRST), which is also
/// how a kernel panic ends a test run (BOOT.md).
///
/// This never returns. A firmware that refuses is a violated invariant -- the machine was
/// asked to stop and did not -- so it panics rather than carrying on with a process that
/// believes it powered the machine off (tenet 2, fail closed and loudly).
#[cfg(baremetal)]
pub fn reset(reboot: bool) -> ! {
    #[cfg(feature = "sbi")]
    self::sbi::reset(reboot);
    #[cfg(not(feature = "sbi"))]
    let _ = reboot;
    panic!("system_reset: the firmware did not stop the machine");
}

/// Platform specific initialization.
#[cfg(not(any(unix, windows)))]
pub fn init() {

    #[cfg(feature = "sbi")]
    self::sbi::init();
}
