//! `stub::plan` on hostile ELF bytes: never panics, and every segment it accepts satisfies the
//! bounds/flags invariants `validate` claims (TENETS.md 6: "fuzz what parses"). The body is
//! `stub::fuzz::plan_one`, which the stub's host tests also run over the kept corpus
//! (`seeds/plan`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| stub::fuzz::plan_one(data));
