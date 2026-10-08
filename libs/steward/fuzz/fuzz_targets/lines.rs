//! The manifest lines the steward is started with, on hostile bytes: never a panic, and what it
//! reads writes back the same (TENETS.md 6: "fuzz what parses"). The body is
//! `redoubt_steward::fuzz::lines_one`, which the core's host tests also run over the kept corpus
//! (`seeds/lines`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| redoubt_steward::fuzz::lines_one(data));
