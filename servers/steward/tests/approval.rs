//! The approval channel's line grammar (servers/steward.md, "The powerbox and approvals"): what
//! the person types at `approve@box` and what the steward's reader makes of it.

use redoubt_steward_server::approval::{Hash, Line, Lines, MAX_LINE, parse};

const ID: &str = "8000000000000001";
const HASH: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn hash() -> [u8; 32] {
    let mut h = [0; 32];
    for (i, b) in h.iter_mut().enumerate() {
        *b = u8::from_str_radix(&HASH[2 * i..2 * i + 2], 16).unwrap();
    }
    h
}

/// The three commands, blanks and a carriage return tolerated, hex in either case; an
/// approval's hash whole or as the screen's eight-digit code.
#[test]
fn the_three_commands_parse() {
    let approve = Line::Approve { request: 0x8000000000000001, hash: Hash::Full(hash()) };
    assert_eq!(parse(format!("approve {ID} {HASH}").as_bytes()), Some(approve));
    assert_eq!(parse(format!("  approve\t{ID}  {}\r", HASH.to_uppercase()).as_bytes()), Some(approve));
    let code = Hash::Code([0x00, 0x11, 0x22, 0x33]);
    assert_eq!(
        parse(format!("approve {ID} {}", &HASH[..8]).as_bytes()),
        Some(Line::Approve { request: 0x8000000000000001, hash: code })
    );
    assert_eq!(code.bytes(), &[0x00, 0x11, 0x22, 0x33]);
    assert_eq!(Hash::Full(hash()).bytes().len(), 32);
    assert_eq!(parse(format!("deny {ID}").as_bytes()), Some(Line::Deny { request: 0x8000000000000001 }));
    assert_eq!(parse(b"pending"), Some(Line::Pending));
}

/// Anything else is no command: a short id, a hash of any other length, a stray word, an empty
/// line, a hash where none belongs.
#[test]
fn everything_else_is_no_command() {
    for line in [
        String::new(),
        format!("approve {ID}"),
        format!("approve {ID} {}", &HASH[..63]),
        format!("approve {ID} {}", &HASH[..7]),
        format!("approve {ID} {}", &HASH[..9]),
        format!("approve {} {HASH}", &ID[..15]),
        format!("approve {ID} {HASH} now"),
        format!("deny {ID} {HASH}"),
        format!("deny 0x{}", &ID[2..]),
        String::from("pending please"),
        String::from("approve"),
        format!("APPROVE {ID} {HASH}"),
    ] {
        assert_eq!(parse(line.as_bytes()), None, "{line:?}");
    }
}

/// Lines end at `\n` or `\r` however the reads cut them, empty ones are nothing; a line over
/// `MAX_LINE` is dropped to its end.
#[test]
fn reads_gather_into_lines_and_a_flood_holds_nothing() {
    let mut lines = Lines::new();
    assert!(lines.push(b"pen").is_empty());
    assert_eq!(lines.push(b"ding\nde"), vec![b"pending".to_vec()]);
    assert_eq!(lines.push(b"ny 1\r\n\n"), vec![b"deny 1".to_vec()]);
    assert_eq!(lines.push(b"pending\rdeny 2\r"), vec![b"pending".to_vec(), b"deny 2".to_vec()]);
    let flood = vec![b'a'; MAX_LINE + 1];
    assert!(lines.push(&flood).is_empty());
    assert_eq!(lines.dropped(), 1);
    assert!(lines.push(b"approve\n").is_empty(), "the tail of the dropped line goes with it");
    assert_eq!(lines.push(b"pending\n"), vec![b"pending".to_vec()]);
    assert_eq!(lines.dropped(), 0);
    let mut exact = Lines::new();
    assert_eq!(exact.push(&[&vec![b'b'; MAX_LINE][..], b"\n"].concat()), vec![vec![b'b'; MAX_LINE]]);
}

/// The fuzz campaign's kept corpus (`fuzz/seeds/approval`), rerun.
#[test]
fn the_fuzz_corpus_still_passes() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/approval");
    let mut ran = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        redoubt_steward_server::fuzz::approval_one(&std::fs::read(entry.unwrap().path()).unwrap());
        ran += 1;
    }
    assert!(ran >= 1, "the corpus is there: {ran} inputs");
}
