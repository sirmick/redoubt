//! The approval channel's input on hostile bytes: never a panic, the same lines however the
//! reads cut them, and a parsed command written back reads as itself (TENETS.md 6: "fuzz what
//! parses"). The body is `redoubt_steward_server::fuzz::approval_one`, which the server's host
//! tests also run over the kept corpus (`seeds/approval`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| redoubt_steward_server::fuzz::approval_one(data));
