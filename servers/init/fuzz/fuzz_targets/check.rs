//! The boot manifest's reading and checks on hostile bytes: never a panic, and a plan only within
//! what the machine holds (TENETS.md 6: "fuzz what parses"). The body is
//! `redoubt_init::fuzz::check_one`, which `init`'s host tests also run over the kept corpus
//! (`seeds/check`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| redoubt_init::fuzz::check_one(data));
