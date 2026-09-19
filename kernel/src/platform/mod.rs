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

/// Platform specific initialization.
#[cfg(not(any(unix, windows)))]
pub fn init() {

    #[cfg(feature = "sbi")]
    self::sbi::init();
}
