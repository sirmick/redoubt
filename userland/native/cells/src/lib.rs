//! The cell protocol: what the session draws on a person's terminal, from the screen buffer's
//! diff in beamlet or from a native program with a screen, and all either may hand it.
//!
//! A screen program never writes to the terminal. It sends frames of cells, and the session's
//! encoder, the only writer of control sequences, draws them. So this format is where a hijacked
//! screen program meets the session, and it is strict: a symbol cannot hold a control character,
//! a cell cannot fall outside its screen, and a frame that decodes re-encodes to exactly the bytes
//! it came from. The session's decoder (`Redoubt.Term.Cells`) keeps the same rules, and
//! `vectors.json` holds both to the same answers.
//!
//! A frame, every integer little-endian:
//!
//! ```text
//! u8  version      1
//! u8  flags        bit 0: clear the screen before drawing these cells; no other bit
//! u16 width        1 ..= MAX_SIDE
//! u16 height       1 ..= MAX_SIDE
//! u32 count        at most width * height
//! count cells:
//!   u16 x, u16 y   inside the screen, each position at most once in a frame
//!   u8  length     1 ..= MAX_SYMBOL
//!   length bytes   the symbol: UTF-8, with no control character (see [`Symbol`])
//!   4 bytes        foreground: kind, then three bytes (see [`Color`])
//!   4 bytes        background, the same
//!   u16 modifiers  bits of [`Modifiers::ALL`] only
//! ```

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;

/// The version this crate writes and reads.
pub const VERSION: u8 = 1;

/// The most columns or rows a screen has: the largest window an SSH session may ask for.
pub const MAX_SIDE: u16 = 1024;

/// The most bytes one cell's symbol holds: a grapheme, however many code points it joins.
pub const MAX_SYMBOL: usize = 32;

const CLEAR: u8 = 1;

/// The fewest bytes a cell takes: a position, a length, two colours and modifiers, whatever its
/// symbol (an empty one is read, then refused as a symbol).
const MIN_CELL: usize = 2 + 2 + 1 + 4 + 4 + 2;

/// What a cell shows: a non-empty UTF-8 string of at most [`MAX_SYMBOL`] bytes that holds no
/// control character. That rules out the ASCII controls (U+0000 to U+001F, ESC among them), DEL,
/// the 8-bit controls (U+0080 to U+009F, which some terminals obey as ESC and a byte), and the
/// bidirectional embedding, override and isolate controls (U+202A to U+202E, U+2066 to U+2069),
/// which reorder what is shown around them. Everything else is left to the session's width tables
/// to lay out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol(String);

impl Symbol {
    /// The symbol, if `text` may be one.
    pub fn new(text: &str) -> Result<Symbol, Error> {
        if text.is_empty() || text.len() > MAX_SYMBOL || text.chars().any(forbidden) {
            return Err(Error::Symbol);
        }
        Ok(Symbol(String::from(text)))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

/// Whether a character may never be in a symbol.
pub fn forbidden(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A cell's colour: the terminal's own, one of its 256 indexed colours (the first sixteen are the
/// named ones), or a 24-bit colour. On the wire, a kind byte and three more, unused ones zero:
/// `0 0 0 0` for the terminal's own, `1 i 0 0` for indexed `i`, `2 r g b` for 24-bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Reset,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A cell's attributes, as bits: [`BOLD`](Modifiers::BOLD) and the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(u16);

impl Modifiers {
    /// Every bit a cell may carry.
    pub const ALL: u16 = (1 << 9) - 1;
    pub const BOLD: u16 = 1;
    pub const CROSSED_OUT: u16 = 1 << 8;
    pub const DIM: u16 = 1 << 1;
    pub const HIDDEN: u16 = 1 << 7;
    pub const ITALIC: u16 = 1 << 2;
    /// No attribute.
    pub const NONE: Modifiers = Modifiers(0);
    pub const RAPID_BLINK: u16 = 1 << 5;
    pub const REVERSED: u16 = 1 << 6;
    pub const SLOW_BLINK: u16 = 1 << 4;
    pub const UNDERLINED: u16 = 1 << 3;

    /// The attributes, if `bits` holds only known ones.
    pub fn new(bits: u16) -> Result<Modifiers, Error> {
        if bits & !Self::ALL != 0 {
            return Err(Error::Modifiers);
        }
        Ok(Modifiers(bits))
    }

    pub fn bits(self) -> u16 { self.0 }
}

/// One cell of a screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub symbol: Symbol,
    pub fg: Color,
    pub bg: Color,
    pub modifiers: Modifiers,
}

/// The cells that changed on a screen of `width` by `height`, drawn after clearing it if `clear`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub clear: bool,
    pub cells: Vec<Cell>,
}

/// Why bytes are not a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes end before the frame does.
    Truncated,
    /// A version this crate does not read.
    Version,
    /// A flag bit that means nothing.
    Flags,
    /// A side of zero, or more than [`MAX_SIDE`].
    Size,
    /// More cells than the screen has.
    Count,
    /// A cell outside the screen.
    Position,
    /// A position given twice in one frame.
    Twice,
    /// A symbol that is empty, too long, not UTF-8, or holds a control character.
    Symbol,
    /// A colour of no known kind, or with a byte set that its kind leaves unused.
    Color,
    /// A modifier bit that means nothing.
    Modifiers,
    /// Bytes after the last cell.
    Trailing,
}

impl Error {
    /// The error's name, as the session's decoder and `vectors.json` spell it.
    pub fn name(self) -> &'static str {
        match self {
            Error::Truncated => "truncated",
            Error::Version => "version",
            Error::Flags => "flags",
            Error::Size => "size",
            Error::Count => "count",
            Error::Position => "position",
            Error::Twice => "twice",
            Error::Symbol => "symbol",
            Error::Color => "color",
            Error::Modifiers => "modifiers",
            Error::Trailing => "trailing",
        }
    }
}

/// The frame's bytes.
pub fn encode(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + frame.cells.len() * 16);
    out.push(VERSION);
    out.push(if frame.clear { CLEAR } else { 0 });
    out.extend_from_slice(&frame.width.to_le_bytes());
    out.extend_from_slice(&frame.height.to_le_bytes());
    out.extend_from_slice(&(frame.cells.len() as u32).to_le_bytes());
    for cell in &frame.cells {
        out.extend_from_slice(&cell.x.to_le_bytes());
        out.extend_from_slice(&cell.y.to_le_bytes());
        out.push(cell.symbol.0.len() as u8);
        out.extend_from_slice(cell.symbol.0.as_bytes());
        color(&mut out, cell.fg);
        color(&mut out, cell.bg);
        out.extend_from_slice(&cell.modifiers.0.to_le_bytes());
    }
    out
}

fn color(out: &mut Vec<u8>, color: Color) {
    out.extend_from_slice(&match color {
        Color::Reset => [0, 0, 0, 0],
        Color::Indexed(i) => [1, i, 0, 0],
        Color::Rgb(r, g, b) => [2, r, g, b],
    });
}

/// The frame in `bytes`, if they are exactly one.
pub fn decode(bytes: &[u8]) -> Result<Frame, Error> {
    let mut r = Reader(bytes);
    if r.u8()? != VERSION {
        return Err(Error::Version);
    }
    let flags = r.u8()?;
    if flags & !CLEAR != 0 {
        return Err(Error::Flags);
    }
    let (width, height) = (r.u16()?, r.u16()?);
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(Error::Size);
    }
    let count = r.u32()?;
    if u64::from(count) > u64::from(width) * u64::from(height) {
        return Err(Error::Count);
    }
    // Nothing is allocated for cells the bytes cannot hold: a header alone may claim a million.
    if count as usize > r.0.len() / MIN_CELL {
        return Err(Error::Truncated);
    }
    let mut seen = BTreeSet::new();
    let mut cells = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (x, y) = (r.u16()?, r.u16()?);
        if x >= width || y >= height {
            return Err(Error::Position);
        }
        if !seen.insert((y, x)) {
            return Err(Error::Twice);
        }
        let length = usize::from(r.u8()?);
        let text = core::str::from_utf8(r.take(length)?).map_err(|_| Error::Symbol)?;
        let symbol = Symbol::new(text)?;
        let (fg, bg) = (r.color()?, r.color()?);
        let modifiers = Modifiers::new(r.u16()?)?;
        cells.push(Cell { x, y, symbol, fg, bg, modifiers });
    }
    if !r.0.is_empty() {
        return Err(Error::Trailing);
    }
    Ok(Frame { width, height, clear: flags & CLEAR != 0, cells })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(Error::Truncated);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, Error> { Ok(self.take(1)?[0]) }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn color(&mut self) -> Result<Color, Error> {
        match *self.take(4)? {
            [0, 0, 0, 0] => Ok(Color::Reset),
            [1, i, 0, 0] => Ok(Color::Indexed(i)),
            [2, r, g, b] => Ok(Color::Rgb(r, g, b)),
            _ => Err(Error::Color),
        }
    }
}

#[cfg(test)]
mod tests;
