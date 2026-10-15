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
//!
//! A native program with a screen and the session talk over its standard streams in **records**:
//! a `u32` length, then that many bytes ([`record`], [`split`]). On its standard output each
//! record is exactly one frame, of the size the session last gave it, and at most
//! [`max_frame`] bytes for that size. On its standard input each record is one [`Event`]:
//!
//! ```text
//! u8 kind       1: the screen's size, u16 width, u16 height, each 1 ..= MAX_SIDE
//!               2: a key, then:
//!   u8 modifiers  bits of KEY_MODIFIERS only: 1 shift, 2 alt, 4 ctrl
//!   u8 key        0 a symbol, then u8 length and a [`Symbol`]'s bytes; 1 Enter, 2 Tab,
//!                 3 Backspace, 4 Esc, 5 Up, 6 Down, 7 Left, 8 Right, 9 Home, 10 End,
//!                 11 Page Up, 12 Page Down, 13 Insert, 14 Delete; 0x80 + n Fn, n 1 ..= 24
//! ```
//!
//! The first event is the size; it comes again whenever the terminal's size changes.
//! `events.json` holds the session's encoder to [`decode_event`]'s answers.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::ops::ControlFlow;

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
/// the 8-bit controls (U+0080 to U+009F, which some terminals obey as ESC and a byte), the
/// bidirectional embedding, override and isolate controls (U+202A to U+202E, U+2066 to U+2069),
/// which reorder what is shown around them, and the invisible format characters (the soft hyphen,
/// the zero-width characters and joiners, the byte order mark, the tags of plane 14) and the line
/// and paragraph separators, with which two symbols can look the same ([`forbidden`]; the table is
/// stated once in `forbidden.txt`). The one exception is the zero-width joiner inside an emoji ZWJ
/// sequence (`👩‍💻`), which is one grapheme and draws as one ([`joined`]). Everything else is left
/// to the session's width tables to lay out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol(String);

impl Symbol {
    /// The symbol, if `text` may be one.
    pub fn new(text: &str) -> Result<Symbol, Error> { Ok(Symbol(String::from(symbol(text)?))) }

    pub fn as_str(&self) -> &str { &self.0 }
}

/// `text`, if it may be a [`Symbol`].
fn symbol(text: &str) -> Result<&str, Error> {
    if text.is_empty() || text.len() > MAX_SYMBOL || holds_forbidden(text) {
        return Err(Error::Symbol);
    }
    Ok(text)
}

/// Whether a character is never drawn as itself: a control, or an invisible format character
/// (`forbidden.txt`). A symbol may hold one only as [`joined`] allows.
pub fn forbidden(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{ad}'
            | '\u{200b}'..='\u{200d}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
            | '\u{e0000}'..='\u{e007f}'
    )
}

/// The zero-width joiner, kept in a symbol only inside an emoji ZWJ sequence.
pub const JOINER: char = '\u{200d}';

/// Whether a joiner stands inside an emoji ZWJ sequence, given the two characters before it
/// (`before`, the nearer last) and the one after: between an element, a pictograph alone or
/// followed by the emoji presentation selector or a skin tone modifier, and a pictograph.
pub fn joined(before: [Option<char>; 2], after: Option<char>) -> bool {
    let element = match before {
        [_, Some(c)] if pictographic(c) => true,
        [Some(p), Some('\u{fe0f}' | '\u{1f3fb}'..='\u{1f3ff}')] => pictographic(p),
        _ => false,
    };
    element && after.is_some_and(pictographic)
}

/// Each character of `text` in turn, with whether a symbol may hold it where it stands: every
/// character that is not [`forbidden`], and a [`JOINER`] that is [`joined`].
pub fn allowed(text: &str) -> impl Iterator<Item = (char, bool)> + '_ {
    let mut before = [None, None];
    let mut chars = text.chars().peekable();
    core::iter::from_fn(move || {
        let c = chars.next()?;
        let ok = !forbidden(c) || (c == JOINER && joined(before, chars.peek().copied()));
        before = [before[1], Some(c)];
        Some((c, ok))
    })
}

/// Whether `text` holds a character no symbol may, where it stands ([`allowed`]).
pub fn holds_forbidden(text: &str) -> bool { allowed(text).any(|(_, ok)| !ok) }

/// Whether a character is Extended_Pictographic: Unicode 16.0's ranges, as the pinned OTP has
/// them (`forbidden.txt`).
pub fn pictographic(c: char) -> bool {
    let c = c as u32;
    PICTOGRAPHIC.iter().any(|&(first, last)| first <= c && c <= last)
}

const PICTOGRAPHIC: [(u32, u32); 78] = [
    (0xa9, 0xa9),
    (0xae, 0xae),
    (0x203c, 0x203c),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21a9, 0x21aa),
    (0x231a, 0x231b),
    (0x2328, 0x2328),
    (0x2388, 0x2388),
    (0x23cf, 0x23cf),
    (0x23e9, 0x23f3),
    (0x23f8, 0x23fa),
    (0x24c2, 0x24c2),
    (0x25aa, 0x25ab),
    (0x25b6, 0x25b6),
    (0x25c0, 0x25c0),
    (0x25fb, 0x25fe),
    (0x2600, 0x2605),
    (0x2607, 0x2612),
    (0x2614, 0x2685),
    (0x2690, 0x2705),
    (0x2708, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271d, 0x271d),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2767),
    (0x2795, 0x2797),
    (0x27a1, 0x27a1),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2934, 0x2935),
    (0x2b05, 0x2b07),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x3030, 0x3030),
    (0x303d, 0x303d),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1f000, 0x1f0ff),
    (0x1f10d, 0x1f10f),
    (0x1f12f, 0x1f12f),
    (0x1f16c, 0x1f171),
    (0x1f17e, 0x1f17f),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f1ad, 0x1f1e5),
    (0x1f201, 0x1f20f),
    (0x1f21a, 0x1f21a),
    (0x1f22f, 0x1f22f),
    (0x1f232, 0x1f23a),
    (0x1f23c, 0x1f23f),
    (0x1f249, 0x1f3fa),
    (0x1f400, 0x1f53d),
    (0x1f546, 0x1f64f),
    (0x1f680, 0x1f6ff),
    (0x1f774, 0x1f77f),
    (0x1f7d5, 0x1f7ff),
    (0x1f80c, 0x1f80f),
    (0x1f848, 0x1f84f),
    (0x1f85a, 0x1f85f),
    (0x1f888, 0x1f88f),
    (0x1f8ae, 0x1f8ff),
    (0x1f90c, 0x1f93a),
    (0x1f93c, 0x1f945),
    (0x1f947, 0x1faff),
    (0x1fc00, 0x1fffd),
];

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
    /// A record longer than its bound.
    Length,
    /// An event of no known kind.
    Kind,
    /// A key of no known code.
    Key,
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
            Error::Length => "length",
            Error::Kind => "kind",
            Error::Key => "key",
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

/// The frame in `bytes`, if they are exactly one: [`reduce`] collecting the cells.
pub fn decode(bytes: &[u8]) -> Result<Frame, Error> {
    let mut frame = Frame { width: 0, height: 0, clear: false, cells: Vec::new() };
    reduce(bytes, (), |step, ()| -> ControlFlow<Infallible> {
        match step {
            Step::Frame(h) => {
                let cells = Vec::with_capacity(h.count as usize);
                frame = Frame { width: h.width, height: h.height, clear: h.clear, cells };
            }
            Step::Cell(c) => frame.cells.push(c.to_cell()),
        }
        ControlFlow::Continue(())
    })?;
    Ok(frame)
}

/// What [`reduce`] hands its function: the frame's header once it is read and checked, then each
/// cell as it is read and checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step<'a> {
    Frame(Header),
    Cell(CellRef<'a>),
}

/// The decoder, a cell at a time, so that a large frame is never held whole: `f` is given the
/// header, then each cell, and continues with an accumulator or stops with a value. Gives
/// `Ok(Continue(acc))` when the bytes are exactly one frame, `Ok(Break(value))` when `f` stopped,
/// or the error at the first field refused, as [`decode`] would refuse it: by then `f` has seen
/// the cells before it, and a caller draws nothing until it has `Continue`. The session's
/// `Redoubt.Term.Cells.reduce/3` is the same decoder in Elixir.
pub fn reduce<'a, A, B>(
    bytes: &'a [u8],
    acc: A,
    mut f: impl FnMut(Step<'a>, A) -> ControlFlow<B, A>,
) -> Result<ControlFlow<B, A>, Error> {
    let h = header(bytes)?;
    let mut acc = match f(Step::Frame(h), acc) {
        ControlFlow::Continue(acc) => acc,
        stop => return Ok(stop),
    };
    let mut cells = Cells::new(&h);
    while let Some(cell) = cells.next(bytes)? {
        acc = match f(Step::Cell(cell), acc) {
            ControlFlow::Continue(acc) => acc,
            stop => return Ok(stop),
        };
    }
    Ok(ControlFlow::Continue(acc))
}

/// A frame's header, read and checked: its size, whether it clears the screen, and how many cells
/// follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub width: u16,
    pub height: u16,
    pub clear: bool,
    pub count: u32,
}

/// The header of the frame in `bytes`, each field checked as it is read, and a count the bytes
/// after it cannot hold refused: a header alone may claim a million cells, and nothing is ever
/// allocated for them.
pub fn header(bytes: &[u8]) -> Result<Header, Error> {
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
    if count as usize > r.0.len() / MIN_CELL {
        return Err(Error::Truncated);
    }
    Ok(Header { width, height, clear: flags & CLEAR != 0, count })
}

/// One cell as the decoder reads it, its symbol still in the frame's bytes: checked as a
/// [`Symbol`] is, so it holds no control character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRef<'a> {
    pub x: u16,
    pub y: u16,
    pub symbol: &'a str,
    pub fg: Color,
    pub bg: Color,
    pub modifiers: Modifiers,
}

impl CellRef<'_> {
    /// The cell, its symbol copied out.
    pub fn to_cell(&self) -> Cell {
        let (x, y, fg, bg, modifiers) = (self.x, self.y, self.fg, self.bg, self.modifiers);
        Cell { x, y, symbol: Symbol(String::from(self.symbol)), fg, bg, modifiers }
    }
}

/// The cells after a frame's header, read one at a time from its bytes. It holds where the next
/// cell starts, how many are left and which positions were given (a bit a cell of the screen,
/// made at the first cell), and no borrow of the bytes: a caller may stop after any cell and go on
/// later with the same bytes.
pub struct Cells {
    width: u16,
    height: u16,
    at: usize,
    left: u32,
    seen: Vec<u64>,
}

impl Cells {
    /// The cells of the frame whose header is `h`.
    pub fn new(h: &Header) -> Cells {
        Cells { width: h.width, height: h.height, at: HEADER, left: h.count, seen: Vec::new() }
    }

    /// Where the next cell starts in the frame's bytes.
    pub fn at(&self) -> usize { self.at }

    /// The next cell of `bytes`, the frame's whole bytes, each field checked as it is read;
    /// `None` after the last, once nothing follows it.
    pub fn next<'a>(&mut self, bytes: &'a [u8]) -> Result<Option<CellRef<'a>>, Error> {
        let mut r = Reader(bytes.get(self.at..).ok_or(Error::Truncated)?);
        if self.left == 0 {
            return if r.0.is_empty() { Ok(None) } else { Err(Error::Trailing) };
        }
        let (x, y) = (r.u16()?, r.u16()?);
        if x >= self.width || y >= self.height {
            return Err(Error::Position);
        }
        let i = usize::from(y) * usize::from(self.width) + usize::from(x);
        if self.seen.is_empty() {
            self.seen = vec![0; (usize::from(self.width) * usize::from(self.height)).div_ceil(64)];
        }
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        if self.seen[word] & bit != 0 {
            return Err(Error::Twice);
        }
        self.seen[word] |= bit;
        let length = usize::from(r.u8()?);
        let symbol = symbol(core::str::from_utf8(r.take(length)?).map_err(|_| Error::Symbol)?)?;
        let (fg, bg) = (r.color()?, r.color()?);
        let modifiers = Modifiers::new(r.u16()?)?;
        self.at = bytes.len() - r.0.len();
        self.left -= 1;
        Ok(Some(CellRef { x, y, symbol, fg, bg, modifiers }))
    }
}

/// The most bytes a frame of a `width` by `height` screen can take: every cell given, each with
/// the longest symbol. A record on a program's standard output claiming more is refused before
/// any of it is held.
pub fn max_frame(width: u16, height: u16) -> usize {
    HEADER + usize::from(width) * usize::from(height) * (MIN_CELL + MAX_SYMBOL)
}

/// A frame's header: version, flags, width, height and count.
const HEADER: usize = 1 + 1 + 2 + 2 + 4;

/// The most bytes an event's record takes: its length, then a key with the longest symbol.
pub const MAX_EVENT: usize = 4 + 1 + 1 + 1 + 1 + MAX_SYMBOL;

/// `body` as a record: its length, then its bytes.
pub fn record(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

/// The first record in `bytes`, and what follows it; `None` while it has not all arrived; refused
/// [`Error::Length`] as soon as its length says more than `max`.
pub fn split(bytes: &[u8], max: usize) -> Result<Option<(&[u8], &[u8])>, Error> {
    let Some(head) = bytes.get(..4) else { return Ok(None) };
    let length = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if length > max {
        return Err(Error::Length);
    }
    Ok(bytes[4..].get(..length).map(|body| (body, &bytes[4 + length..])))
}

/// What the session sends a native program with a screen, on its standard input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The screen's size: every frame the program sends is this size from now.
    Size { width: u16, height: u16 },
    /// A key, with bits of [`KEY_MODIFIERS`].
    Key { key: Key, modifiers: u8 },
}

/// Every modifier bit a key may carry.
pub const KEY_MODIFIERS: u8 = SHIFT | ALT | CTRL;
pub const SHIFT: u8 = 1;
pub const ALT: u8 = 2;
pub const CTRL: u8 = 4;

/// A key: a symbol typed, or a named key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Symbol(Symbol),
    Enter,
    Tab,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// A function key, 1 to 24.
    F(u8),
}

const SIZE: u8 = 1;
const KEY: u8 = 2;
const F: u8 = 0x80;
const NAMED: [Key; 14] = [
    Key::Enter,
    Key::Tab,
    Key::Backspace,
    Key::Esc,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Insert,
    Key::Delete,
];

/// The event's bytes, a record's body.
pub fn encode_event(event: &Event) -> Vec<u8> {
    let mut out = Vec::new();
    match event {
        Event::Size { width, height } => {
            out.push(SIZE);
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
        }
        Event::Key { key, modifiers } => {
            out.extend_from_slice(&[KEY, *modifiers]);
            match key {
                Key::Symbol(symbol) => {
                    out.extend_from_slice(&[0, symbol.0.len() as u8]);
                    out.extend_from_slice(symbol.0.as_bytes());
                }
                Key::F(n) => out.push(F + n),
                named => out.push(NAMED.iter().position(|k| k == named).map_or(0, |i| i as u8 + 1)),
            }
        }
    }
    out
}

/// The event in `bytes`, a record's body, if they are exactly one.
pub fn decode_event(bytes: &[u8]) -> Result<Event, Error> {
    let mut r = Reader(bytes);
    let event = match r.u8()? {
        SIZE => {
            let (width, height) = (r.u16()?, r.u16()?);
            if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
                return Err(Error::Size);
            }
            Event::Size { width, height }
        }
        KEY => {
            let modifiers = r.u8()?;
            if modifiers & !KEY_MODIFIERS != 0 {
                return Err(Error::Modifiers);
            }
            let key = match r.u8()? {
                0 => {
                    let length = usize::from(r.u8()?);
                    Key::Symbol(Symbol::new(
                        core::str::from_utf8(r.take(length)?).map_err(|_| Error::Symbol)?,
                    )?)
                }
                code @ 1..=14 => NAMED[usize::from(code) - 1].clone(),
                code @ 0x81..=0x98 => Key::F(code - F),
                _ => return Err(Error::Key),
            };
            Event::Key { key, modifiers }
        }
        _ => return Err(Error::Kind),
    };
    if !r.0.is_empty() {
        return Err(Error::Trailing);
    }
    Ok(event)
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
