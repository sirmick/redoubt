//! The escape sequences that draw a buffer's changed cells: the shell's encoder for screens,
//! `Redoubt.Term.Frame.draw/1`, in Rust, byte for byte, which is kept as its reference
//! (docs/userland/shell.md, "The terminal library").
//!
//! It writes nothing but these: the style reset `CSI 0m`, and `CSI 2J` after it for a frame that
//! clears; `CSI row;col H` to place a cell, unless the cursor is already there after the cell
//! before it; SGR from a cell's numeric colours and attributes when its style differs from the
//! last one drawn; each symbol made visible as `Redoubt.Term.Text.visible/1` makes text visible;
//! and the style reset at the end. A symbol in a buffer holds no control character already (`put`
//! and `fill` refuse one); making it visible is the second guard, so nothing a cell holds is ever
//! a control sequence. After a symbol that is not one column wide, the next cell is placed again.

use alloc::vec::Vec;
use core::fmt::Write;

use cells::Color;

use crate::buffer::{Style, columns};

/// A frame being drawn: the bytes so far, where the cursor is (`(x, y)`) if that is known, and
/// the style last set.
pub(crate) struct Encoder {
    out: Vec<u8>,
    cursor: Option<(u16, u16)>,
    style: Style,
}

impl Encoder {
    /// A frame's start: the style reset, and the screen cleared if it clears.
    pub(crate) fn new(clear: bool) -> Encoder {
        let mut out = Vec::new();
        out.extend_from_slice(if clear { b"\x1b[0m\x1b[2J" } else { b"\x1b[0m" });
        Encoder { out, cursor: None, style: Style::PLAIN }
    }

    /// One cell, as `Frame`'s `cell/2` draws it.
    pub(crate) fn cell(&mut self, x: u16, y: u16, symbol: &str, style: Style) {
        if self.cursor != Some((x, y)) {
            let _ = write!(Text(&mut self.out), "\x1b[{};{}H", u32::from(y) + 1, u32::from(x) + 1);
        }
        if style != self.style {
            sgr(&mut self.out, style);
            self.style = style;
        }
        visible(&mut self.out, symbol);
        // After a wide symbol, where the cursor is is the terminal's to say: placed again.
        self.cursor = if columns(symbol) == 1 { Some((x + 1, y)) } else { None };
    }

    /// The frame's bytes, ending with the style reset.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        self.out.extend_from_slice(b"\x1b[0m");
        self.out
    }
}

/// The whole style, from a reset: the attributes, then the colours.
fn sgr(out: &mut Vec<u8>, style: Style) {
    out.extend_from_slice(b"\x1b[0");
    // The cell protocol's modifier bit i is SGR attribute i + 1, bold to crossed out.
    for i in 0..9 {
        if style.modifiers.bits() >> i & 1 == 1 {
            let _ = write!(Text(out), ";{}", i + 1);
        }
    }
    color(out, style.fg, 30, 90, 38);
    color(out, style.bg, 40, 100, 48);
    out.push(b'm');
}

fn color(out: &mut Vec<u8>, color: Color, base: u8, bright: u8, ext: u8) {
    let _ = match color {
        Color::Reset => Ok(()),
        Color::Indexed(i) if i < 8 => write!(Text(out), ";{}", base + i),
        Color::Indexed(i) if i < 16 => write!(Text(out), ";{}", bright + i - 8),
        Color::Indexed(i) => write!(Text(out), ";{ext};5;{i}"),
        Color::Rgb(r, g, b) => write!(Text(out), ";{ext};2;{r};{g};{b}"),
    };
}

/// `symbol` as `Redoubt.Term.Text.visible/1` draws text from column 0: a tab as spaces to the
/// next stop of eight code points, a C0 control in caret notation (`^[`), DEL as `^?`, any other
/// character a symbol may not hold where it stands (`cells::allowed`: a C1 control, a bidirectional
/// control, an invisible format character, a joiner outside an emoji sequence) as `<U+009B>`, and
/// every other character as itself.
fn visible(out: &mut Vec<u8>, symbol: &str) {
    let mut col = 0;
    for (c, allowed) in cells::allowed(symbol) {
        let n = c as u32;
        col += match c {
            '\t' => {
                let n = 8 - col % 8;
                out.extend(core::iter::repeat_n(b' ', n));
                n
            }
            '\u{0}'..='\u{1f}' => {
                out.extend_from_slice(&[b'^', n as u8 + 64]);
                2
            }
            '\u{7f}' => {
                out.extend_from_slice(b"^?");
                2
            }
            _ if !allowed => {
                // `<U+XXXX>`: eight columns, nine for a tag.
                let before = out.len();
                let _ = write!(Text(out), "<U+{n:04X}>");
                out.len() - before
            }
            _ => {
                let mut utf8 = [0; 4];
                out.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
                1
            }
        };
    }
}

/// Formatting into a byte buffer.
struct Text<'a>(&'a mut Vec<u8>);

impl Write for Text<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}
