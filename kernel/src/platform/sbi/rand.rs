// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel random numbers for SBI platforms.
//!
//! FIXME(xous64): this is NOT a secure seed. The only entropy is the `time` counter at
//! boot. The loader should pass the device tree's `/chosen/rng-seed` (or a virtio-rng
//! reading) in the kernel arguments and this should be seeded from that.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

static mut RNG: Option<ChaCha8Rng> = None;

pub fn init() {
    let seed = riscv::register::time::read64();
    unsafe { *(&raw mut RNG) = Some(ChaCha8Rng::seed_from_u64(seed)) };
}

pub fn get_u32() -> u32 {
    unsafe { (*(&raw mut RNG)).as_mut().expect("kernel rng used before init").next_u32() }
}
