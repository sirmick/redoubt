// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc. <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod precursor;

#[cfg(any(feature = "atsama5d27"))]
pub mod atsama5d2;

#[cfg(any(any(feature = "bao1x")))]
pub mod bao1x;

#[cfg(feature = "sbi")]
pub mod sbi;

#[cfg(any(any(feature = "bao1x")))]
pub use bao1x::rand;
#[cfg(feature = "sbi")]
pub use sbi::rand;
#[cfg(not(any(feature = "bao1x", feature = "sbi")))]
pub mod rand;

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
    #[cfg(any(feature = "precursor", feature = "renode"))]
    self::precursor::init();

    #[cfg(any(feature = "atsama5d27"))]
    self::atsama5d2::init();

    #[cfg(any(feature = "bao1x"))]
    self::bao1x::init();

    #[cfg(feature = "sbi")]
    self::sbi::init();
}
