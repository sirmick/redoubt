// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel random numbers for SBI platforms: a ChaCha8 generator keyed from the `Seed`
//! kernel argument, which the loader fills from the device tree's `/chosen/rng-seed`.
//!
//! The kernel draws server IDs from this, and a server ID is what lets a process
//! connect, so a predictable seed would let any process guess its way into any server.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

use crate::cell::KernelCell;

static RNG: KernelCell<Option<ChaCha8Rng>> = KernelCell::new(None);

pub fn init() {
    let mut key = [0u8; 32];
    match crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Seed")) {
        Some(arg) => {
            let seed = arg.data.iter().flat_map(|word| word.to_le_bytes());
            // Fold the whole seed into the key, in case it is longer than 32 bytes.
            for (i, byte) in seed.enumerate() {
                key[i % 32] ^= byte;
            }
            println!("Kernel RNG seeded from the loader ({} bytes)", arg.data.len() * 4);
        }
        // Fail closed: there is no acceptable fallback. A clock is not entropy.
        None => panic!("the loader passed no RNG seed; refusing to run with guessable server IDs"),
    }
    RNG.with(|rng| *rng = Some(ChaCha8Rng::from_seed(key)));
}

pub fn get_u32() -> u32 {
    RNG.with(|rng| rng.as_mut().expect("kernel rng used before init").next_u32())
}

/// Fill `bytes` from the kernel's CSPRNG (the `random` call).
pub fn fill(bytes: &mut [u8]) {
    RNG.with(|rng| rng.as_mut().expect("kernel rng used before init").fill_bytes(bytes))
}
