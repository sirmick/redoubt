// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel random numbers for SBI platforms: a ChaCha8 generator keyed from the `Seed`
//! kernel argument, which the loader fills from the device tree's `/chosen/rng-seed`.
//!
//! The kernel draws server IDs from this, and a server ID is what lets a process
//! connect, so a predictable seed would let any process guess its way into any server.

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

static mut RNG: Option<ChaCha8Rng> = None;

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
        None => {
            // Not acceptable outside bring-up. Shout, so it cannot go unnoticed.
            println!("WARNING: INSECURE KERNEL RNG: the loader passed no seed; server IDs are guessable");
            key[..8].copy_from_slice(&riscv::register::time::read64().to_le_bytes());
        }
    }
    unsafe { *(&raw mut RNG) = Some(ChaCha8Rng::from_seed(key)) };
}

pub fn get_u32() -> u32 {
    unsafe { (*(&raw mut RNG)).as_mut().expect("kernel rng used before init").next_u32() }
}
