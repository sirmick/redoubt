//! The screen buffer itself: a grid of cells drawn into, and the diff of what changed since the
//! last frame. Pure: no VM, so every rule is tested here; the natives in `lib.rs` only check
//! their arguments and call these.

use alloc::vec;
use alloc::vec::Vec;

use cells::{Cell as Wire, Cells, Color, Frame, MAX_SIDE, MAX_SYMBOL, Modifiers, Symbol};

use crate::draw::Encoder;

/// The most cells a buffer holds.
pub const MAX_CELLS: usize = 65_536;

/// What a cell is drawn in: two colours and attributes, as the cell protocol carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub modifiers: Modifiers,
}

impl Style {
    /// The terminal's own colours, no attributes.
    pub const PLAIN: Style = Style { fg: Color::Reset, bg: Color::Reset, modifiers: Modifiers::NONE };
}

/// What a cell shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Glyph {
    /// A grapheme of `len` bytes, kept inline.
    Text { len: u8, bytes: [u8; MAX_SYMBOL] },
    /// The right half of the wide grapheme in the cell to the left: never sent in a frame.
    Continuation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    glyph: Glyph,
    style: Style,
}

const SPACE: Glyph = Glyph::text(" ");

const BLANK: Cell = Cell { glyph: SPACE, style: Style::PLAIN };

impl Glyph {
    /// `s`, which fits (at most [`MAX_SYMBOL`] bytes).
    const fn text(s: &str) -> Glyph {
        let b = s.as_bytes();
        let mut bytes = [0; MAX_SYMBOL];
        let mut i = 0;
        while i < b.len() {
            bytes[i] = b[i];
            i += 1;
        }
        Glyph::Text { len: b.len() as u8, bytes }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Glyph::Text { len, bytes } => core::str::from_utf8(&bytes[..*len as usize]).ok(),
            Glyph::Continuation => None,
        }
    }
}

/// Why a buffer refuses a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A side of zero, or more than [`MAX_SIDE`], or more than [`MAX_CELLS`] cells in all.
    Size,
    /// Text holding a control character, the `cells` crate's rule.
    Control,
    /// A symbol that is not one grapheme of one column, or a bitmap of the wrong length.
    Shape,
}

/// The Unicode version of the width table.
pub const UNICODE: (u8, u8) = crate::wide::UNICODE;

/// The columns a grapheme takes: two if OTP calls it wide, as its `unicode_util:is_wide/1`
/// decides (a presentation selector second, or any wide code point), else one.
pub fn columns(grapheme: &str) -> usize {
    let selector = matches!(grapheme.chars().nth(1), Some('\u{FE0E}' | '\u{FE0F}'));
    if selector || grapheme.chars().any(wide) { 2 } else { 1 }
}

/// Whether `c` is in one of the table's ranges, which are sorted and apart.
fn wide(c: char) -> bool {
    let c = c as u32;
    let i = crate::wide::WIDE.partition_point(|&(_, last)| last < c);
    crate::wide::WIDE.get(i).is_some_and(|&(first, _)| first <= c)
}

/// A grid of `width` by `height` cells: `back` is drawn into, `front` is what the last frame
/// left on the screen.
pub struct Buffer {
    width: u16,
    height: u16,
    back: Vec<Cell>,
    front: Vec<Cell>,
    /// The next frame clears the screen and sends every cell that is not blank.
    clear: bool,
    /// A program's frame being read, stopped at a joined symbol ([`Buffer::read`]).
    reading: Option<Cells>,
}

impl Buffer {
    /// A blank buffer; its first frame clears the screen.
    pub fn new(width: u16, height: u16) -> Result<Buffer, Refusal> {
        let cells = check_size(width, height)?;
        Ok(Buffer {
            width,
            height,
            back: vec![BLANK; cells],
            front: vec![BLANK; cells],
            clear: true,
            reading: None,
        })
    }

    pub fn width(&self) -> u16 { self.width }

    pub fn height(&self) -> u16 { self.height }

    /// The bytes the buffer's two grids hold, and the record of positions a frame being read
    /// keeps, a bit a cell: what its holder's memory counts.
    pub fn bytes(&self) -> usize {
        2 * self.back.len() * size_of::<Cell>() + self.back.len().div_ceil(64) * 8
    }

    /// A new size, blank; the next frame clears the screen and sends it all.
    pub fn resize(&mut self, width: u16, height: u16) -> Result<(), Refusal> {
        *self = Buffer::new(width, height)?;
        Ok(())
    }

    /// Writes `graphemes` along row `y` from column `x`, one a cell and a wide one two, clipped at
    /// the edge: a wide grapheme the edge cuts becomes a space, and a grapheme longer than a
    /// symbol may be becomes U+FFFD. Takes no more graphemes than the row has cells from `x`, and
    /// writes nothing if any of those holds a control character. Returns the columns written.
    pub fn put<'a>(
        &mut self,
        x: u16,
        y: u16,
        graphemes: impl IntoIterator<Item = &'a str>,
        style: Style,
    ) -> Result<u16, Refusal> {
        if x >= self.width || y >= self.height {
            return Ok(0);
        }
        let room = usize::from(self.width - x);
        let taken: Vec<&str> = graphemes.into_iter().take(room).collect();
        if taken.iter().any(|g| cells::holds_forbidden(g)) {
            return Err(Refusal::Control);
        }
        let (mut col, y) = (usize::from(x), usize::from(y));
        for g in taken {
            if g.is_empty() {
                continue;
            }
            let w = columns(g);
            if col + w > usize::from(self.width) {
                // A wide grapheme cut by the edge.
                self.set(col, y, SPACE, style);
                col += 1;
                break;
            }
            let glyph = if g.len() > MAX_SYMBOL { Glyph::text("\u{FFFD}") } else { Glyph::text(g) };
            if w == 2 {
                self.set_wide(col, y, glyph, style);
            } else {
                self.set(col, y, glyph, style);
            }
            col += w;
            if col >= usize::from(self.width) {
                break;
            }
        }
        Ok((col - usize::from(x)) as u16)
    }

    /// One symbol and one style over a rectangle, clipped. The symbol is one code point of one
    /// column (a space, a line, a block, a Braille pattern): the buffer does not segment text, so
    /// it takes no more than it can measure.
    pub fn fill(&mut self, rect: Rect, symbol: &str, style: Style) -> Result<(), Refusal> {
        if cells::holds_forbidden(symbol) {
            return Err(Refusal::Control);
        }
        if symbol.chars().count() != 1 || columns(symbol) != 1 {
            return Err(Refusal::Shape);
        }
        let glyph = Glyph::text(symbol);
        for (x, y) in self.clip(rect) {
            self.set(x, y, glyph, style);
        }
        Ok(())
    }

    /// A bitmap over a rectangle: `dots` holds one byte a cell, row by row, each the cell's eight
    /// dots in Braille's own bit order (dot 1 is bit 0, dot 8 bit 7), drawn as U+2800 plus the
    /// byte; clipped.
    pub fn plot(&mut self, rect: Rect, dots: &[u8], style: Style) -> Result<(), Refusal> {
        if dots.len() != usize::from(rect.w) * usize::from(rect.h) {
            return Err(Refusal::Shape);
        }
        for (x, y) in self.clip(rect) {
            let i = (y - usize::from(rect.y)) * usize::from(rect.w) + (x - usize::from(rect.x));
            let c = char::from_u32(0x2800 + u32::from(dots[i])).expect("a Braille pattern");
            let mut utf8 = [0; 4];
            self.set(x, y, Glyph::text(c.encode_utf8(&mut utf8)), style);
        }
        Ok(())
    }

    /// Reads the frame in `bytes`, a native program's, into the buffer, a cell at a time as
    /// `cells::reduce` reads it, from byte `from`: 0 for its header, or where the last call
    /// stopped. The frame must be the buffer's size, else [`Read::Size`] and nothing is touched;
    /// one that clears blanks the buffer first. Each cell, once the decoder has checked it, must
    /// be one cell's worth as the session lays it out, and is then put as [`put`](Buffer::put)
    /// puts one grapheme. A symbol of more than one code point stops the reading at
    /// [`Read::Joined`]: whether it is one grapheme is OTP's segmentation to say, so the caller
    /// says it, puts it, and goes on from `next`. A refused frame keeps the cells put before the
    /// one refused; the caller draws none of it.
    ///
    /// Nothing is allocated but the decoder's record of the positions given, a bit a cell of
    /// the buffer, whatever the frame claims.
    pub fn read<'a>(&mut self, bytes: &'a [u8], from: usize) -> Result<Read<'a>, Refused> {
        let mut cells = if from == 0 {
            self.reading = None;
            let h = cells::header(bytes).map_err(Refused::Frame)?;
            if (h.width, h.height) != (self.width, self.height) {
                return Ok(Read::Size { width: h.width, height: h.height });
            }
            if h.clear {
                self.back.fill(BLANK);
            }
            Cells::new(&h)
        } else {
            match self.reading.take() {
                Some(cells) if cells.at() == from => cells,
                _ => return Err(Refused::Resume),
            }
        };
        while let Some(c) = cells.next(bytes).map_err(Refused::Frame)? {
            let style = Style { fg: c.fg, bg: c.bg, modifiers: c.modifiers };
            if c.symbol.chars().nth(1).is_some() {
                let next = cells.at();
                self.reading = Some(cells);
                return Ok(Read::Joined { next, x: c.x, y: c.y, symbol: c.symbol, style });
            }
            if columns(c.symbol) == 2 && c.x + 1 >= self.width {
                return Err(Refused::Cell);
            }
            self.put(c.x, c.y, [c.symbol], style).expect("a decoded symbol holds no control character");
        }
        Ok(Read::Done)
    }

    /// The cells changed since the last frame, as a frame, which then becomes what is shown.
    pub fn diff(&mut self) -> Frame {
        let cells = self
            .changes()
            .map(|(x, y, text, style)| Wire {
                x,
                y,
                symbol: Symbol::new(text).expect("a buffer holds no control character"),
                fg: style.fg,
                bg: style.bg,
                modifiers: style.modifiers,
            })
            .collect();
        let frame = Frame { width: self.width, height: self.height, clear: self.clear, cells };
        self.shown();
        frame
    }

    /// The cells changed since the last frame, as the escape sequences that draw them
    /// (`draw.rs`): the bytes `Redoubt.Term.Frame.draw/1` writes for [`diff`](Buffer::diff)'s
    /// frame. They then become what is shown.
    pub fn draw(&mut self) -> Vec<u8> {
        let mut encoder = Encoder::new(self.clear);
        for (x, y, text, style) in self.changes() {
            encoder.cell(x, y, text, style);
        }
        self.shown();
        encoder.finish()
    }

    /// Each cell to send, in order, as `(x, y, symbol, style)`: when the screen is to be cleared,
    /// every one that is not blank, else every one that changed; never the right half of a wide
    /// grapheme.
    fn changes(&self) -> impl Iterator<Item = (u16, u16, &str, Style)> {
        let w = usize::from(self.width);
        self.back.iter().zip(&self.front).enumerate().filter_map(move |(i, (back, front))| {
            let changed = if self.clear { *back != BLANK } else { back != front };
            let text = back.glyph.as_str().filter(|_| changed)?;
            Some(((i % w) as u16, (i / w) as u16, text, back.style))
        })
    }

    /// What was drawn into the buffer is now what the screen shows.
    fn shown(&mut self) {
        self.front.copy_from_slice(&self.back);
        self.clear = false;
    }

    /// The positions of `rect` inside the buffer.
    fn clip(&self, rect: Rect) -> impl Iterator<Item = (usize, usize)> + use<> {
        let x0 = usize::from(rect.x).min(usize::from(self.width));
        let y0 = usize::from(rect.y).min(usize::from(self.height));
        let x1 = (usize::from(rect.x) + usize::from(rect.w)).min(usize::from(self.width));
        let y1 = (usize::from(rect.y) + usize::from(rect.h)).min(usize::from(self.height));
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
    }

    /// Writes one cell of one column. A wide grapheme it overwrites half of leaves a space in its
    /// other half.
    fn set(&mut self, x: usize, y: usize, glyph: Glyph, style: Style) {
        let at = y * usize::from(self.width) + x;
        self.orphan(at);
        self.back[at] = Cell { glyph, style };
    }

    /// Writes a wide grapheme in two cells, `x` and the one to its right, likewise.
    fn set_wide(&mut self, x: usize, y: usize, glyph: Glyph, style: Style) {
        let at = y * usize::from(self.width) + x;
        self.orphan(at);
        self.orphan(at + 1);
        self.back[at] = Cell { glyph, style };
        self.back[at + 1] = Cell { glyph: Glyph::Continuation, style };
    }

    /// Cell `at` is about to be overwritten: if it is half of a wide grapheme, the other half
    /// becomes a space.
    fn orphan(&mut self, at: usize) {
        let w = usize::from(self.width);
        if self.back[at].glyph == Glyph::Continuation {
            self.back[at - 1].glyph = SPACE;
        } else if (at + 1) % w != 0 && self.back[at + 1].glyph == Glyph::Continuation {
            self.back[at + 1].glyph = SPACE;
        }
    }
}

/// How far [`Buffer::read`] got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read<'a> {
    /// The frame was read whole and put.
    Done,
    /// The frame is of another size: nothing was read past its header, nothing put.
    Size { width: u16, height: u16 },
    /// The cell at `x`, `y` has a symbol of more than one code point, checked by the decoder and
    /// not yet put; the next cell starts at byte `next`.
    Joined { next: usize, x: u16, y: u16, symbol: &'a str, style: Style },
}

/// Why [`Buffer::read`] refused a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The decoder refused it.
    Frame(cells::Error),
    /// A cell is not one cell's worth: a wide symbol in the last column.
    Cell,
    /// `from` is not where the last reading stopped.
    Resume,
}

/// A rectangle: its top left corner and its size, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// The cells a buffer of `width` by `height` holds, if it may be that size.
fn check_size(width: u16, height: u16) -> Result<usize, Refusal> {
    let cells = usize::from(width) * usize::from(height);
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) || cells > MAX_CELLS {
        return Err(Refusal::Size);
    }
    Ok(cells)
}

#[cfg(test)]
mod tests;
