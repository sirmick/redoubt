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
    for bad in ["\u{1b}", "\u{7}", "\u{7f}", "\u{9b}", "\u{202e}", "\u{2066}", "a\u{0}"] {
        assert_eq!(b.put(0, 0, ["o", "k", bad], Style::PLAIN), Err(Refusal::Control), "{bad:?}");
        assert_eq!(
            b.fill(Rect { x: 0, y: 0, w: 2, h: 1 }, bad, Style::PLAIN),
            Err(Refusal::Control),
            "{bad:?}"
        );
    }
    assert!(cells(&b.diff()).is_empty());
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
fn a_buffer_declares_its_two_grids() {
    let b = Buffer::new(100, 50).unwrap();
    assert_eq!(b.bytes(), 2 * 100 * 50 * size_of::<Cell>());
}
