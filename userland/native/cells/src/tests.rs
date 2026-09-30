extern crate std;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::*;

fn cell(x: u16, y: u16, symbol: &str, fg: Color, bg: Color, modifiers: u16) -> Cell {
    Cell { x, y, symbol: Symbol::new(symbol).unwrap(), fg, bg, modifiers: Modifiers::new(modifiers).unwrap() }
}

/// The frames every decoder must read, and read back to the same bytes.
fn good() -> Vec<(&'static str, Frame)> {
    vec![
        ("an empty screen, not cleared", Frame { width: 80, height: 24, clear: false, cells: vec![] }),
        (
            "a cell of each colour kind, cleared first",
            Frame {
                width: 4,
                height: 2,
                clear: true,
                cells: vec![
                    cell(0, 0, "a", Color::Reset, Color::Indexed(4), Modifiers::BOLD),
                    cell(1, 0, "界", Color::Rgb(1, 2, 3), Color::Reset, 0),
                    cell(
                        3,
                        1,
                        "e\u{301}",
                        Color::Indexed(15),
                        Color::Rgb(255, 0, 0),
                        Modifiers::ITALIC | Modifiers::UNDERLINED,
                    ),
                ],
            },
        ),
        (
            "joined code points in one symbol, every modifier",
            Frame {
                width: 2,
                height: 1,
                clear: false,
                cells: vec![cell(0, 0, "👩\u{200d}💻", Color::Reset, Color::Reset, Modifiers::ALL)],
            },
        ),
        ("the largest screen", Frame { width: MAX_SIDE, height: MAX_SIDE, clear: true, cells: vec![] }),
    ]
}

// The bytes of a frame on a 4 by 2 screen, laid out for the cases below to name by offset:
//   0 version, 1 flags, 2..4 width, 4..6 height, 6..10 count,
//   then each cell: +0..2 x, +2..4 y, +4 length, the symbol, 4 bytes fg, 4 bytes bg, 2 modifiers.
// For `one(b"a")`: x 10..12, y 12..14, length 14, symbol 15, fg 16..20, bg 20..24, modifiers 24..26.

/// The cell (1, 0) with the symbol's bytes as given, and no colour or modifier.
fn cell_bytes(symbol: &[u8]) -> Vec<u8> {
    let mut b = vec![1, 0, 0, 0, symbol.len() as u8];
    b.extend_from_slice(symbol);
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&[0, 0]);
    b
}

/// A frame of `cells` such cells, all at (1, 0).
fn frame_bytes(cells: u32, symbol: &[u8]) -> Vec<u8> {
    let mut b = vec![1, 0, 4, 0, 2, 0];
    b.extend_from_slice(&cells.to_le_bytes());
    for _ in 0..cells {
        b.extend_from_slice(&cell_bytes(symbol));
    }
    b
}

fn one(symbol: &[u8]) -> Vec<u8> { frame_bytes(1, symbol) }

/// Bytes every decoder must refuse, and why.
fn bad() -> Vec<(&'static str, Vec<u8>, Error)> {
    let good = one(b"a");
    let with = |at: usize, value: u8| {
        let mut b = good.clone();
        b[at] = value;
        b
    };
    let long = [b'x'; MAX_SYMBOL + 1];
    vec![
        ("nothing at all", vec![], Error::Truncated),
        ("only a version, and not 1", vec![2], Error::Version),
        ("a version and an unknown flag, then nothing", vec![1, 2], Error::Flags),
        (
            "a header claiming more cells than its bytes can hold",
            {
                let mut b = vec![1, 0];
                b.extend_from_slice(&MAX_SIDE.to_le_bytes());
                b.extend_from_slice(&MAX_SIDE.to_le_bytes());
                b.extend_from_slice(&(u32::from(MAX_SIDE) * u32::from(MAX_SIDE)).to_le_bytes());
                b.extend_from_slice(&good[10..]);
                b
            },
            Error::Truncated,
        ),
        ("a cell cut after a position past the edge", with(10, 4)[..14].to_vec(), Error::Truncated),
        ("a header cut short", good[..7].to_vec(), Error::Truncated),
        ("a cell cut short", good[..good.len() - 1].to_vec(), Error::Truncated),
        ("version 2", with(0, 2), Error::Version),
        ("an unknown flag", with(1, 2), Error::Flags),
        ("a width of zero", with(2, 0), Error::Size),
        (
            "a height over the largest",
            {
                let mut b = good.clone();
                b[4..6].copy_from_slice(&(MAX_SIDE + 1).to_le_bytes());
                b
            },
            Error::Size,
        ),
        (
            "more cells than the screen",
            {
                let mut b = good.clone();
                b[6..10].copy_from_slice(&9u32.to_le_bytes());
                b
            },
            Error::Count,
        ),
        ("a cell past the right edge", with(10, 4), Error::Position),
        ("a cell below the screen", with(12, 2), Error::Position),
        ("a position given twice", frame_bytes(2, b"a"), Error::Twice),
        ("an empty symbol", one(b""), Error::Symbol),
        ("a symbol that is ESC", one(b"\x1b"), Error::Symbol),
        ("a symbol holding a bell", one(b"a\x07"), Error::Symbol),
        ("a symbol that is DEL", one(b"\x7f"), Error::Symbol),
        ("a symbol that is the C1 control CSI", one("\u{9b}".as_bytes()), Error::Symbol),
        ("a symbol that overrides the text's direction", one("\u{202e}".as_bytes()), Error::Symbol),
        ("a symbol that isolates the text's direction", one("\u{2066}".as_bytes()), Error::Symbol),
        ("a symbol that is not UTF-8", one(b"\xff"), Error::Symbol),
        ("a symbol one byte too long", one(&long), Error::Symbol),
        ("a colour of no kind", with(16, 3), Error::Color),
        ("the terminal's colour with a byte set", with(17, 1), Error::Color),
        (
            "an indexed colour with a spare byte set",
            {
                let mut b = with(16, 1);
                b[18] = 1;
                b
            },
            Error::Color,
        ),
        (
            "a modifier bit that means nothing",
            {
                let mut b = good.clone();
                let n = b.len();
                b[n - 1] = 2;
                b
            },
            Error::Modifiers,
        ),
        (
            "a byte after the last cell",
            {
                let mut b = good.clone();
                b.push(0);
                b
            },
            Error::Trailing,
        ),
    ]
}

#[test]
fn good_frames_decode_and_encode_back_to_their_bytes() {
    for (name, frame) in good() {
        let bytes = encode(&frame);
        assert_eq!(decode(&bytes).as_ref(), Ok(&frame), "{name}");
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes, "{name}");
    }
}

#[test]
fn bad_bytes_are_refused_for_their_reason() {
    for (name, bytes, error) in bad() {
        assert_eq!(decode(&bytes), Err(error), "{name}");
    }
}

#[test]
fn no_control_character_is_ever_a_symbol() {
    for c in (0..0x20).chain(0x7f..0xa0).chain(0x202a..0x202f).chain(0x2066..0x206a) {
        let c = char::from_u32(c).unwrap();
        assert_eq!(Symbol::new(&c.to_string()), Err(Error::Symbol), "U+{:04X}", c as u32);
        assert_eq!(Symbol::new(&format!("a{c}b")), Err(Error::Symbol), "U+{:04X} inside", c as u32);
    }
    for ok in ["a", " ", "~", "é", "e\u{301}", "界", "🙂", "👩\u{200d}💻", "\u{a0}", "\u{2028}"] {
        assert!(Symbol::new(ok).is_ok(), "{ok:?}");
    }
}

#[test]
fn every_cut_of_a_frame_is_refused() {
    for (_, frame) in good() {
        let bytes = encode(&frame);
        for n in 0..bytes.len() {
            assert!(decode(&bytes[..n]).is_err(), "cut at {n} of {}", bytes.len());
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic_and_what_decodes_encodes_back() {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let seed = encode(&good()[1].1);
    for _ in 0..50_000 {
        let mut bytes = seed.clone();
        for _ in 0..(next() % 4 + 1) {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        bytes.truncate(bytes.len() - (next() % 3) as usize);
        if let Ok(frame) = decode(&bytes) {
            assert_eq!(encode(&frame), bytes);
        }
    }
}

/// `vectors.json`: every case above, for the session's decoder to be held to the same answers.
/// `CELLS_VECTORS=write cargo test` writes it; this test fails when it is not current.
#[test]
fn vectors_are_current() {
    let mut cases = Vec::new();
    for (name, frame) in good() {
        cases.push(format!(
            "  {{\"name\": \"{name}\", \"hex\": \"{}\", \"frame\": {}}}",
            hex(&encode(&frame)),
            frame_json(&frame)
        ));
    }
    for (name, bytes, error) in bad() {
        cases.push(format!(
            "  {{\"name\": \"{name}\", \"hex\": \"{}\", \"error\": \"{}\"}}",
            hex(&bytes),
            error.name()
        ));
    }
    let json = format!("[\n{}\n]\n", cases.join(",\n"));
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors.json");
    if std::env::var("CELLS_VECTORS").as_deref() == Ok("write") {
        std::fs::write(path, &json).unwrap();
    }
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(committed == json, "vectors.json is not current: run CELLS_VECTORS=write cargo test -p cells");
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn frame_json(frame: &Frame) -> String {
    let cells: Vec<String> = frame
        .cells
        .iter()
        .map(|c| {
            format!(
                "{{\"x\": {}, \"y\": {}, \"symbol\": \"{}\", \"fg\": {}, \"bg\": {}, \"modifiers\": {}}}",
                c.x,
                c.y,
                c.symbol.as_str(),
                color_json(c.fg),
                color_json(c.bg),
                c.modifiers.bits()
            )
        })
        .collect();
    format!(
        "{{\"width\": {}, \"height\": {}, \"clear\": {}, \"cells\": [{}]}}",
        frame.width,
        frame.height,
        frame.clear,
        cells.join(", ")
    )
}

fn color_json(color: Color) -> String {
    match color {
        Color::Reset => "[\"reset\"]".to_string(),
        Color::Indexed(i) => format!("[\"indexed\", {i}]"),
        Color::Rgb(r, g, b) => format!("[\"rgb\", {r}, {g}, {b}]"),
    }
}
