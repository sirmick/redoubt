//! The buffer's rules, judged on the frames it gives: each frame is decoded by the `cells`
//! crate, the decoder the session reads frames with, so what is checked is what would be drawn.

use alloc::string::String;
use alloc::vec::Vec;

use cells::{Color, Frame, Modifiers};

use super::*;

const RED: Style = Style { fg: Color::Indexed(1), bg: Color::Reset, modifiers: Modifiers::NONE };

/// The frame's cells as `(x, y, symbol)`, after a round trip through the wire.
fn cells(frame: &Frame) -> Vec<(u16, u16, String)> {
    let frame = cells::decode(&cells::encode(frame)).expect("a frame the session reads");
    frame.cells.into_iter().map(|c| (c.x, c.y, String::from(c.symbol.as_str()))).collect()
}

fn graphemes(s: &str) -> Vec<&str> {
    // Single code points: the tests that need joined graphemes give them whole.
    s.char_indices().map(|(i, c)| &s[i..i + c.len_utf8()]).collect()
}

fn at(x: u16, y: u16, s: &str) -> (u16, u16, String) { (x, y, String::from(s)) }

#[test]
fn a_buffer_is_at_most_1024_a_side_and_65536_cells() {
    assert!(Buffer::new(1024, 64).is_ok());
    assert!(Buffer::new(256, 256).is_ok());
    assert_eq!(Buffer::new(1025, 1).err(), Some(Refusal::Size));
    assert_eq!(Buffer::new(0, 10).err(), Some(Refusal::Size));
    assert_eq!(Buffer::new(257, 256).err(), Some(Refusal::Size));
}

#[test]
fn the_first_frame_clears_and_sends_what_is_not_blank() {
    let mut b = Buffer::new(10, 3).unwrap();
    b.put(1, 1, graphemes("hi"), RED).unwrap();
    let frame = b.diff();
    assert!(frame.clear);
    assert_eq!(cells(&frame), [at(1, 1, "h"), at(2, 1, "i")]);
}

#[test]
fn a_frame_sends_only_what_changed() {
    let mut b = Buffer::new(10, 3).unwrap();
    b.put(0, 0, graphemes("abc"), Style::PLAIN).unwrap();
    b.diff();
    b.put(0, 0, graphemes("abX"), Style::PLAIN).unwrap();
    b.put(5, 2, graphemes("z"), RED).unwrap();
    let frame = b.diff();
    assert!(!frame.clear);
    assert_eq!(cells(&frame), [at(2, 0, "X"), at(5, 2, "z")]);
    assert!(b.diff().cells.is_empty(), "nothing changed since");
}

#[test]
fn a_style_change_alone_is_sent() {
    let mut b = Buffer::new(4, 1).unwrap();
    b.put(0, 0, graphemes("a"), Style::PLAIN).unwrap();
    b.diff();
    b.put(0, 0, graphemes("a"), RED).unwrap();
    let frame = b.diff();
    assert_eq!(frame.cells.len(), 1);
    assert_eq!(frame.cells[0].fg, Color::Indexed(1));
}

#[test]
fn a_resize_is_blank_and_its_frame_clears_and_sends_it_all() {
    let mut b = Buffer::new(10, 3).unwrap();
    b.put(0, 0, graphemes("abc"), Style::PLAIN).unwrap();
    b.diff();
    b.resize(4, 2).unwrap();
    b.put(0, 1, graphemes("q"), Style::PLAIN).unwrap();
    let frame = b.diff();
    assert!(frame.clear);
    assert_eq!((frame.width, frame.height), (4, 2));
    assert_eq!(cells(&frame), [at(0, 1, "q")]);
}

#[test]
fn put_clips_at_the_edge_and_returns_the_columns_written() {
    let mut b = Buffer::new(5, 1).unwrap();
    assert_eq!(b.put(3, 0, graphemes("abcdef"), Style::PLAIN).unwrap(), 2);
    assert_eq!(b.put(9, 0, graphemes("x"), Style::PLAIN).unwrap(), 0, "off the buffer: nothing");
    assert_eq!(cells(&b.diff()), [at(3, 0, "a"), at(4, 0, "b")]);
}

#[test]
fn put_reads_no_more_graphemes_than_the_row_has_cells() {
    let mut b = Buffer::new(3, 1).unwrap();
    let mut read = 0;
    let endless = core::iter::repeat_with(|| {
        read += 1;
        "x"
    });
    b.put(1, 0, endless, Style::PLAIN).unwrap();
    assert_eq!(read, 2);
}

#[test]
fn a_wide_grapheme_takes_two_cells_and_the_edge_cutting_one_leaves_a_space() {
    let mut b = Buffer::new(5, 1).unwrap();
    assert_eq!(b.put(0, 0, ["世", "界", "!"], Style::PLAIN).unwrap(), 5);
    assert_eq!(cells(&b.diff()), [at(0, 0, "世"), at(2, 0, "界"), at(4, 0, "!")]);
    b.put(4, 0, ["世"], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(4, 0, " ")]);
}

#[test]
fn overwriting_half_of_a_wide_grapheme_leaves_a_space_in_the_other() {
    let mut b = Buffer::new(6, 1).unwrap();
    b.put(0, 0, ["世", "界"], Style::PLAIN).unwrap();
    b.diff();
    b.put(1, 0, ["x"], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(0, 0, " "), at(1, 0, "x")]);
    b.put(2, 0, ["y"], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(2, 0, "y"), at(3, 0, " ")]);
    // A wide grapheme over the right half of another.
    b.put(0, 0, ["世"], Style::PLAIN).unwrap();
    b.diff();
    b.put(1, 0, ["界"], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(0, 0, " "), at(1, 0, "界")]);
}

#[test]
fn a_grapheme_longer_than_a_symbol_may_be_is_u_fffd() {
    let mut b = Buffer::new(3, 1).unwrap();
    let long: String = core::iter::once('e').chain(core::iter::repeat_n('\u{301}', 20)).collect();
    assert!(long.len() > cells::MAX_SYMBOL);
    b.put(0, 0, [long.as_str(), "e\u{301}"], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(0, 0, "\u{FFFD}"), at(1, 0, "e\u{301}")]);
}

#[test]
fn a_control_character_is_refused_and_nothing_of_the_call_is_written() {
    let mut b = Buffer::new(10, 1).unwrap();
    for bad in [
        "\u{1b}",
        "\u{7}",
        "\u{7f}",
        "\u{9b}",
        "\u{202e}",
        "\u{2066}",
        "a\u{0}",
        "\u{ad}",
        "\u{200b}",
        "a\u{200d}",
        "\u{2028}",
        "\u{feff}",
        "\u{e0067}",
    ] {
        assert_eq!(b.put(0, 0, ["o", "k", bad], Style::PLAIN), Err(Refusal::Control), "{bad:?}");
        assert_eq!(
            b.fill(Rect { x: 0, y: 0, w: 2, h: 1 }, bad, Style::PLAIN),
            Err(Refusal::Control),
            "{bad:?}"
        );
    }
    assert!(cells(&b.diff()).is_empty());
    // A joiner inside an emoji sequence is the grapheme's, and the grapheme is put as one symbol.
    assert_eq!(b.put(0, 0, ["👩\u{200d}💻", "x"], Style::PLAIN), Ok(3));
    let drawn = cells(&b.diff());
    assert_eq!(drawn.len(), 2);
    assert_eq!(drawn[0].2, "👩\u{200d}💻");
}

#[test]
fn fill_covers_a_rectangle_clipped_with_one_code_point_of_one_column() {
    let mut b = Buffer::new(4, 3).unwrap();
    b.fill(Rect { x: 2, y: 1, w: 5, h: 5 }, "─", RED).unwrap();
    assert_eq!(cells(&b.diff()), [at(2, 1, "─"), at(3, 1, "─"), at(2, 2, "─"), at(3, 2, "─")]);
    assert_eq!(b.fill(Rect { x: 0, y: 0, w: 1, h: 1 }, "世", RED), Err(Refusal::Shape));
    assert_eq!(b.fill(Rect { x: 0, y: 0, w: 1, h: 1 }, "ab", RED), Err(Refusal::Shape));
    assert_eq!(b.fill(Rect { x: 0, y: 0, w: 1, h: 1 }, "e\u{301}", RED), Err(Refusal::Shape));
    assert_eq!(b.fill(Rect { x: 0, y: 0, w: 1, h: 1 }, "", RED), Err(Refusal::Shape));
}

#[test]
fn plot_draws_one_braille_pattern_a_cell_clipped() {
    let mut b = Buffer::new(3, 1).unwrap();
    b.plot(Rect { x: 1, y: 0, w: 3, h: 1 }, &[0x01, 0xff, 0x80], Style::PLAIN).unwrap();
    assert_eq!(cells(&b.diff()), [at(1, 0, "\u{2801}"), at(2, 0, "\u{28ff}")]);
    assert_eq!(b.plot(Rect { x: 0, y: 0, w: 2, h: 1 }, &[1], Style::PLAIN), Err(Refusal::Shape));
}

#[test]
fn widths_are_otps() {
    // The table is generated from OTP's unicode_util:is_wide/1 (tools/gen-width.escript), which
    // holds it to OTP; these are the cases its rules make.
    assert_eq!(UNICODE, (16, 0));
    for (g, w) in
        [("a", 1), ("é", 1), ("e\u{301}", 1), ("世", 2), ("가", 2), ("Ａ", 2), ("🙂", 2), ("❤\u{fe0f}", 2)]
    {
        assert_eq!(columns(g), w, "{g:?}");
    }
    // A presentation selector second makes a grapheme wide, as OTP decides.
    assert_eq!(columns("#\u{fe0e}"), 2);
}

#[test]
fn a_buffer_declares_its_two_grids_and_a_bit_a_cell_for_reading() {
    let b = Buffer::new(100, 50).unwrap();
    assert_eq!(b.bytes(), 2 * 100 * 50 * size_of::<Cell>() + (100 * 50usize).div_ceil(64) * 8);
}

// ---- a program's frames read into the buffer ----

/// Reads `bytes` whole, as the session's host does: each joined symbol it is handed is taken as
/// one grapheme (OTP's segmentation is the host's to apply) and put, and the reading goes on.
fn read_all(b: &mut Buffer, bytes: &[u8]) -> Result<Option<(u16, u16)>, Refused> {
    let mut from = 0;
    loop {
        match b.read(bytes, from)? {
            Read::Done => return Ok(None),
            Read::Size { width, height } => return Ok(Some((width, height))),
            Read::Joined { next, x, y, symbol, style } => {
                b.put(x, y, [symbol], style).unwrap();
                from = next;
            }
        }
    }
}

/// The reference: the frame's header read and its size compared, then the frame decoded whole by
/// the `cells` crate, and each cell checked and put as the session's Elixir host does it.
fn read_reference(b: &mut Buffer, bytes: &[u8]) -> Result<Option<(u16, u16)>, Refused> {
    // The host is handed the header first, and stops at a frame of another size.
    let h = cells::header(bytes).map_err(Refused::Frame)?;
    if (h.width, h.height) != (b.width(), b.height()) {
        return Ok(Some((h.width, h.height)));
    }
    let frame = cells::decode(bytes).map_err(Refused::Frame)?;
    if frame.clear {
        b.fill(Rect { x: 0, y: 0, w: frame.width, h: frame.height }, " ", Style::PLAIN).unwrap();
    }
    for c in &frame.cells {
        if columns(c.symbol.as_str()) == 2 && c.x + 1 >= b.width() {
            return Err(Refused::Cell);
        }
        let style = Style { fg: c.fg, bg: c.bg, modifiers: c.modifiers };
        b.put(c.x, c.y, [c.symbol.as_str()], style).unwrap();
    }
    Ok(None)
}

/// The vectors the session's decoder is held to (userland/native/cells/vectors.json), as
/// `(name, bytes, error)`: one a line, `"name"`, `"hex"`, then `"frame"` or `"error"`.
fn vectors() -> Vec<(String, Vec<u8>, Option<String>)> {
    let field = |line: &str, key: &str| {
        let start = line.find(&alloc::format!("\"{key}\": \""))? + key.len() + 5;
        Some(String::from(&line[start..start + line[start..].find('"')?]))
    };
    include_str!("../../../../native/cells/vectors.json")
        .lines()
        .filter_map(|line| {
            let hex = field(line, "hex")?;
            let bytes =
                (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            Some((field(line, "name").unwrap(), bytes, field(line, "error")))
        })
        .collect()
}

#[test]
fn every_vector_is_answered_as_the_decoder_answers_it() {
    let vectors = vectors();
    assert_eq!(vectors.len(), 51);
    let mut read = 0;
    for (name, bytes, error) in vectors {
        // A buffer of the frame's size, when it gives one a buffer may be; else any: a header
        // the decoder refuses is refused before its size is compared.
        let size = (bytes.len() >= 6)
            .then(|| (u16::from_le_bytes([bytes[2], bytes[3]]), u16::from_le_bytes([bytes[4], bytes[5]])));
        let mut b =
            size.and_then(|(w, h)| Buffer::new(w, h).ok()).unwrap_or_else(|| Buffer::new(80, 24).unwrap());
        let answer = read_all(&mut b, &bytes);
        match error {
            Some(e) => assert_eq!(
                answer.map_err(|r| match r {
                    Refused::Frame(f) => f.name(),
                    _ => "other",
                }),
                Err(e.as_str()),
                "{name}"
            ),
            // The largest screen is more cells than a buffer holds: its frame is another size.
            None if Buffer::new(size.unwrap().0, size.unwrap().1).is_err() => {
                assert_eq!(answer, Ok(size), "{name}")
            }
            None => {
                assert_eq!(answer, Ok(None), "{name}");
                let mut reference = Buffer::new(b.width(), b.height()).unwrap();
                read_reference(&mut reference, &bytes).unwrap();
                assert_eq!(b.diff(), reference.diff(), "{name}");
                read += 1;
            }
        }
    }
    assert_eq!(read, 3, "the good frames a buffer can hold");
}

/// A whole 80 by 24 frame: plain symbols, wide ones and joined ones, each row filled as far as
/// they fit, in every colour kind.
fn whole() -> Vec<u8> {
    let symbols = ["a", "界", "e\u{301}", "❤\u{fe0f}", "─", "Z"];
    let mut cells = Vec::new();
    for y in 0..24u16 {
        let mut x = 0;
        for (i, s) in symbols.iter().cycle().enumerate() {
            let w = columns(s) as u16;
            if x + w > 80 {
                break;
            }
            let fg = [Color::Reset, Color::Indexed(i as u8), Color::Rgb(1, 2, i as u8)][i % 3];
            let modifiers = Modifiers::new((i % 512) as u16).unwrap();
            cells.push(cells::Cell {
                x,
                y,
                symbol: cells::Symbol::new(s).unwrap(),
                fg,
                bg: Color::Reset,
                modifiers,
            });
            x += w;
        }
    }
    cells::encode(&Frame { width: 80, height: 24, clear: true, cells })
}

#[test]
fn a_frame_read_in_puts_what_the_reference_puts_whatever_its_bytes() {
    // A whole frame and seeded mutations of it: each read into a buffer already drawn on, the
    // same answer as the reference's, and for a frame read whole the same buffer.
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let seed = whole();
    let mut answers = alloc::collections::BTreeMap::new();
    // Besides the mutations: the frame itself, a byte after it, and a wide symbol at the edge.
    let edge = cells::Cell {
        x: 79,
        y: 3,
        symbol: cells::Symbol::new("界").unwrap(),
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };
    let targeted = [
        seed.clone(),
        [&seed[..], &[0]].concat(),
        cells::encode(&Frame { width: 80, height: 24, clear: false, cells: alloc::vec![edge] }),
    ];
    let mutations = (0..4_000).map(|_| {
        let mut bytes = seed.clone();
        for _ in 0..(next() % 4 + 1) {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        bytes.truncate(bytes.len() - (next() % 3) as usize);
        bytes
    });
    for (i, bytes) in targeted.into_iter().chain(mutations).enumerate() {
        let (mut b, mut reference) = (Buffer::new(80, 24).unwrap(), Buffer::new(80, 24).unwrap());
        for b in [&mut b, &mut reference] {
            b.put(0, 0, graphemes("before"), RED).unwrap();
            b.diff();
        }
        let answer = read_all(&mut b, &bytes);
        assert_eq!(answer, read_reference(&mut reference, &bytes), "input {i}");
        // A refused frame is never drawn; this reference, decoding whole first, puts none of it.
        if answer == Ok(None) {
            assert_eq!(b.diff(), reference.diff(), "input {i}");
        }
        *answers.entry(alloc::format!("{answer:?}")).or_insert(0) += 1;
    }
    // The mutations reach the refusals that matter, not only the first field.
    for reason in
        ["Ok(None)", "Cell", "Twice", "Position", "Symbol", "Color", "Modifiers", "Trailing", "Truncated"]
    {
        assert!(answers.keys().any(|k| k.contains(reason)), "{reason} never answered: {answers:?}");
    }
}

#[test]
fn a_frame_of_another_size_touches_nothing() {
    let mut b = Buffer::new(10, 3).unwrap();
    b.put(0, 0, graphemes("keep"), RED).unwrap();
    b.diff();
    let other = cells::encode(&Frame { width: 11, height: 3, clear: true, cells: Vec::new() });
    assert_eq!(b.read(&other, 0), Ok(Read::Size { width: 11, height: 3 }));
    assert!(b.diff().cells.is_empty());
}

#[test]
fn a_joined_symbol_stops_the_reading_and_it_goes_on_only_from_where_it_stopped() {
    let mut b = Buffer::new(10, 1).unwrap();
    let cell = |x, s| cells::Cell {
        x,
        y: 0,
        symbol: cells::Symbol::new(s).unwrap(),
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };
    let bytes = cells::encode(&Frame {
        width: 10,
        height: 1,
        clear: false,
        cells: alloc::vec![cell(0, "a"), cell(1, "e\u{301}"), cell(2, "b")],
    });
    let Ok(Read::Joined { next, x: 1, y: 0, symbol: "e\u{301}", .. }) = b.read(&bytes, 0) else {
        panic!("not stopped at the joined symbol")
    };
    assert_eq!(b.read(&bytes, next + 1), Err(Refused::Resume));
    // A refused resume ends that reading: the frame starts again from its header or not at all.
    assert_eq!(b.read(&bytes, next), Err(Refused::Resume));
    let Ok(Read::Joined { next, .. }) = b.read(&bytes, 0) else { panic!() };
    assert_eq!(b.read(&bytes, next), Ok(Read::Done));
    assert_eq!(cells(&b.diff()), [at(0, 0, "a"), at(2, 0, "b")], "the joined symbol is the caller's to put");
    assert_eq!(b.read(&bytes, next), Err(Refused::Resume), "a frame read whole is not gone on with");
}

#[test]
fn a_refused_frame_keeps_the_cells_before_the_refusal_and_a_wide_one_at_the_edge_is_refused() {
    let mut b = Buffer::new(4, 1).unwrap();
    let cell = |x, s| cells::Cell {
        x,
        y: 0,
        symbol: cells::Symbol::new(s).unwrap(),
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };
    let bytes = cells::encode(&Frame {
        width: 4,
        height: 1,
        clear: false,
        cells: alloc::vec![cell(0, "a"), cell(3, "界")],
    });
    assert_eq!(b.read(&bytes, 0), Err(Refused::Cell));
    // The session draws none of it: it ends the screen without sending this diff.
    assert_eq!(cells(&b.diff()), [at(0, 0, "a")]);
}

#[test]
fn a_clearing_frame_blanks_the_buffer_first() {
    let mut b = Buffer::new(4, 1).unwrap();
    b.put(0, 0, graphemes("abcd"), RED).unwrap();
    b.diff();
    let cell = cells::Cell {
        x: 1,
        y: 0,
        symbol: cells::Symbol::new("z").unwrap(),
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };
    let bytes = cells::encode(&Frame { width: 4, height: 1, clear: true, cells: alloc::vec![cell] });
    assert_eq!(b.read(&bytes, 0), Ok(Read::Done));
    assert_eq!(cells(&b.diff()), [at(0, 0, " "), at(1, 0, "z"), at(2, 0, " "), at(3, 0, " ")]);
}

#[test]
fn the_positions_given_are_a_frames_own_and_a_resumed_frame_neither_puts_again_nor_forgets_them() {
    let mut b = Buffer::new(4, 1).unwrap();
    let cell = |x, s| cells::Cell {
        x,
        y: 0,
        symbol: cells::Symbol::new(s).unwrap(),
        fg: Color::Reset,
        bg: Color::Reset,
        modifiers: Modifiers::NONE,
    };
    let frame = |cells| cells::encode(&Frame { width: 4, height: 1, clear: false, cells });
    // Each frame may give a position once: the next frame gives it again.
    let a = frame(alloc::vec![cell(0, "a")]);
    assert_eq!(b.read(&a, 0), Ok(Read::Done));
    assert_eq!(b.read(&frame(alloc::vec![cell(0, "b")]), 0), Ok(Read::Done));
    // Stopped at a joined symbol: what the caller puts meanwhile over a cell already read stays,
    // since going on reads only the cells after it; and a position given before the stop is still
    // given.
    let joined = frame(alloc::vec![cell(0, "c"), cell(1, "e\u{301}"), cell(2, "d")]);
    let Ok(Read::Joined { next, .. }) = b.read(&joined, 0) else { panic!() };
    b.put(0, 0, ["q"], Style::PLAIN).unwrap();
    assert_eq!(b.read(&joined, next), Ok(Read::Done));
    assert_eq!(cells(&b.diff()), [at(0, 0, "q"), at(2, 0, "d")]);
    let again = frame(alloc::vec![cell(1, "e\u{301}"), cell(1, "z")]);
    let Ok(Read::Joined { next, .. }) = b.read(&again, 0) else { panic!() };
    assert_eq!(b.read(&again, next), Err(Refused::Frame(cells::Error::Twice)));
}

// ---- the buffer drawn as escape sequences ----

/// The frames of `draw.json` (written by the shell's `Redoubt.Term.Frame.draw/1`), each as its
/// size, whether it clears, its cells as `(x, y, symbol, style)`, read without the cell
/// protocol's checks, and the bytes drawn.
#[allow(clippy::type_complexity)]
fn draw_vectors() -> Vec<(String, u16, u16, bool, Vec<(u16, u16, String, Style)>, Vec<u8>)> {
    let field = |line: &str, key: &str| {
        let start = line.find(&alloc::format!("\"{key}\": \""))? + key.len() + 5;
        Some(String::from(&line[start..start + line[start..].find('"')?]))
    };
    let unhex = |hex: String| -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    };
    let color = |b: &[u8]| match b[0] {
        0 => Color::Reset,
        1 => Color::Indexed(b[1]),
        _ => Color::Rgb(b[1], b[2], b[3]),
    };
    include_str!("../../draw.json")
        .lines()
        .filter_map(|line| {
            let frame = unhex(field(line, "frame")?);
            let le = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
            let (width, height, clear) = (le(2), le(4), frame[1] == 1);
            let mut cells = Vec::new();
            let mut at = 10;
            while at < frame.len() {
                let n = usize::from(frame[at + 4]);
                let symbol = core::str::from_utf8(&frame[at + 5..at + 5 + n]).unwrap();
                let rest = &frame[at + 5 + n..];
                let modifiers = Modifiers::new(u16::from_le_bytes([rest[8], rest[9]])).unwrap();
                let style = Style { fg: color(&rest[..4]), bg: color(&rest[4..8]), modifiers };
                cells.push((le(at), le(at + 2), String::from(symbol), style));
                at += 15 + n;
            }
            Some((
                field(line, "name").unwrap(),
                width,
                height,
                clear,
                cells,
                unhex(field(line, "drawn").unwrap()),
            ))
        })
        .collect()
}

#[test]
fn a_buffer_is_drawn_as_the_shells_encoder_draws_its_frame_control_characters_made_visible() {
    let vectors = draw_vectors();
    assert_eq!(vectors.len(), 6);
    for (name, width, height, clear, cells, drawn) in vectors {
        let mut b = Buffer::new(width, height).unwrap();
        if !clear {
            b.draw();
        }
        // Forced in: the buffer's own calls refuse a symbol holding a control character.
        for (x, y, symbol, style) in &cells {
            let at = usize::from(*y) * usize::from(width) + usize::from(*x);
            b.back[at] = Cell { glyph: Glyph::text(symbol), style: *style };
        }
        let out = b.draw();
        assert!(
            out == drawn,
            "{name}:\n  drawn {:?}\n  wants {:?}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&drawn)
        );
        // Drawn, it is shown: the next frame has nothing.
        assert_eq!(b.draw(), b"\x1b[0m\x1b[0m", "{name}");
    }
}

#[test]
fn no_byte_drawn_from_a_buffer_starts_a_sequence_but_the_encoders_own() {
    // Every symbol that holds a control character, forced in: what is drawn holds ESC only as the
    // start of CSI H, CSI J or SGR, and no C1 control.
    for (name, width, height, _, cells, _) in draw_vectors() {
        let mut b = Buffer::new(width, height).unwrap();
        for (x, y, symbol, style) in &cells {
            b.back[usize::from(*y) * usize::from(width) + usize::from(*x)] =
                Cell { glyph: Glyph::text(symbol), style: *style };
        }
        let out = String::from_utf8(b.draw()).unwrap();
        assert!(
            !out.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
            "{name}"
        );
        for (i, _) in out.match_indices('\x1b') {
            let seq = &out[i + 1..];
            let end = seq.find(|c: char| c.is_ascii_alphabetic()).expect("a final byte");
            assert!(
                seq.starts_with('[') && seq[1..end].bytes().all(|b| b.is_ascii_digit() || b == b';'),
                "{name}: {seq:?}"
            );
            assert!(matches!(&seq[end..end + 1], "H" | "J" | "m"), "{name}: {seq:?}");
        }
        assert!(!out.bytes().any(|b| b < 0x20 && b != 0x1b || b == 0x7f), "{name}");
    }
}

#[test]
fn a_draw_beside_a_writer_on_another_thread_sees_whole_calls_and_loses_no_change() {
    extern crate std;
    use std::sync::{Arc, Mutex};

    // The natives hold a buffer behind its resource's lock, as this mutex: a writer puts whole
    // rows of one letter while a drawer on another thread draws as fast as it can.
    let b = Arc::new(Mutex::new(Buffer::new(80, 24).unwrap()));
    let writer = {
        let b = Arc::clone(&b);
        std::thread::spawn(move || {
            for n in 0..3_000u32 {
                let letter = ["A", "B", "C"][(n / 24 % 3) as usize];
                let row: Vec<&str> = core::iter::repeat_n(letter, 80).collect();
                b.lock().unwrap().put(0, (n % 24) as u16, row, Style::PLAIN).unwrap();
            }
        })
    };
    let mut draws = Vec::new();
    while !writer.is_finished() {
        draws.push(b.lock().unwrap().draw());
    }
    writer.join().unwrap();
    draws.push(b.lock().unwrap().draw());
    assert!(draws.len() > 1);

    // Each draw replayed on a model of the terminal: a row it touches is one letter across (no
    // put is seen half done), and the model ends as the buffer is (no change is lost).
    let mut model = [[b' '; 80]; 24];
    for out in &draws {
        let (mut x, mut y, mut i) = (0, 0, 0);
        let mut letters: [Option<u8>; 24] = [None; 24];
        while i < out.len() {
            if out[i] == 0x1b {
                let end = i + out[i..].iter().position(|c| c.is_ascii_alphabetic()).unwrap();
                let params = core::str::from_utf8(&out[i + 2..end]).unwrap();
                match out[end] {
                    b'H' => {
                        let (row, col) = params.split_once(';').unwrap();
                        (y, x) = (row.parse::<usize>().unwrap() - 1, col.parse::<usize>().unwrap() - 1);
                    }
                    b'J' => model = [[b' '; 80]; 24],
                    _ => {}
                }
                i = end + 1;
            } else {
                assert!(letters[y].is_none_or(|l| l == out[i]), "a row drawn half put");
                letters[y] = Some(out[i]);
                model[y][x] = out[i];
                (x, i) = (x + 1, i + 1);
            }
        }
    }
    let b = b.lock().unwrap();
    for (y, row) in model.iter().enumerate() {
        let shown: Vec<u8> =
            (0..80).map(|x| b.back[y * 80 + x].glyph.as_str().unwrap().as_bytes()[0]).collect();
        assert_eq!(&shown[..], &row[..], "row {y}");
    }
}
