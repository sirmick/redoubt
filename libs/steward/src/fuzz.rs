//! One input through the manifest lines' parser (`manifest::parse_lines`), one line per `\n`: the
//! body of the fuzz target (`fuzz/fuzz_targets/lines.rs`) and of the host test that replays its
//! kept corpus (`fuzz/seeds/lines`), so a campaign's findings rerun in every
//! `cargo test -p redoubt-steward` (TENETS.md 6: "fuzz what parses"). Built only for tests and
//! fuzzing, never for the machine.

use alloc::string::String;

use crate::manifest::{lines, parse_lines};
use crate::{Policy, Store};

/// Never a panic; a manifest it reads is written back to lines that read as the same manifest,
/// and boot either refuses it or fixes it.
pub fn lines_one(data: &[u8]) {
    let Ok(text) = core::str::from_utf8(data) else { return };
    let Ok(m) = parse_lines(text.split('\n')) else { return };
    let written = lines(&m);
    let again = parse_lines(written.iter().map(String::as_str)).expect("written lines read back");
    assert_eq!(again, m);
    let _ = Store::boot(&m, Policy::SHIPPED);
}
