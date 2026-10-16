//! The approval channel's input on hostile bytes (TENETS.md 6: "fuzz what parses"): the body of
//! the fuzz target (`fuzz/fuzz_targets/approval.rs`) and of the host test that replays its kept
//! corpus (`fuzz/seeds/approval`), so a campaign's findings rerun in every
//! `cargo test -p redoubt-steward-server`. Built only for tests and fuzzing, never for the machine.

use alloc::vec::Vec;

use crate::approval::{Hash, Line, Lines, MAX_LINE, parse};

/// Never a panic; the lines are the same whether the bytes come in one read or two, cut where
/// the first byte says; no kept line is over [`MAX_LINE`]; and a command parsed is one the
/// screen's grammar writes back to the same command.
pub fn approval_one(data: &[u8]) {
    let Some((&cut, rest)) = data.split_first() else { return };
    let cut = (cut as usize).min(rest.len());
    let mut whole = Lines::new();
    let once = whole.push(rest);
    let mut split = Lines::new();
    let mut twice = split.push(&rest[..cut]);
    twice.extend(split.push(&rest[cut..]));
    assert_eq!(once, twice);
    for line in &once {
        assert!(line.len() <= MAX_LINE);
        if let Some(command) = parse(line) {
            let written: Vec<u8> = match command {
                Line::Approve { request, hash } => {
                    let hex: alloc::string::String =
                        hash.bytes().iter().map(|b| alloc::format!("{b:02x}")).collect();
                    assert!(matches!((hash, hex.len()), (Hash::Full(_), 64) | (Hash::Code(_), 8)));
                    alloc::format!("approve {request:016x} {hex}").into_bytes()
                }
                Line::Deny { request } => alloc::format!("deny {request:016x}").into_bytes(),
                Line::Pending => b"pending".to_vec(),
            };
            assert_eq!(parse(&written), Some(command));
        }
    }
}
