//! `screen-stage`: the native program the screen cases run with the shell's `screen`
//! (docs/userland/shell.md, "A native program's screen and the session's key"). It has what a
//! stage has, `/dev/stdin`, `/dev/stdout` and `/dev/stderr`, and nothing else: it reads its size
//! and keys as `cells` events on its input and draws by writing `cells` frames, each a record, on
//! its output. Its first argument says what it does:
//!
//! - `draw`: draws `screen-stage-ready` in a box, and the last key it was sent as `key:<name>`, each key on
//!   the next row below the box; redraws at each new size; exits 0 at `q`. Its words hold no space, and each
//!   key's is drawn where the last was not: the session sends only the cells that changed, so a word drawn
//!   whole on blank cells reaches the terminal whole.
//! - `flood`: sends the same whole frame, `screen-stage-flooding` on a screen of dots, as fast as its output
//!   takes it, without end, reading nothing: the session decodes every frame, and draws the first.
//! - `stderr`: writes control sequences to its standard error, then does what `draw` does.
//! - Misbehaviours, each sent once after its size, then it waits without end: `raw` (control sequences,
//!   unframed), `esc-symbol` (a frame with a symbol holding an OSC 52 clipboard write), `key-symbol` (a
//!   symbol that is the session's key, 0x1C), `big` (a frame of 200 by 100), `outside` (a cell one column
//!   past its screen), `long` (one cell of 32 `X`s), `wide-edge` (a wide symbol in the last column) and
//!   `claim` (a record claiming 4 GiB).
//!
//! It reports nothing about how they went: the session judges what it sent (docs/testbench.md,
//! rule F).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use cells::{Cell, Color, Event, Frame, Key, Modifiers, Symbol};
use redoubt_client::file::File;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(run);

/// No lend, or its namespace did not attach.
const NO_STREAM: u32 = 2;
/// Its input, output or standard error is not there or does not open: 6, 7 or 8.
const NO_STDIN: u32 = 6;
/// Its input ended, or held what is not an event.
const BAD_INPUT: u32 = 4;
/// An argument it does not know.
const BAD_ARGS: u32 = 5;

const LEND_PAGES: usize = 2;
const CHUNK: usize = 2048;

/// A clipboard write, a title and a bare ESC: each must reach the console as visible text.
const HOSTILE: &[u8] = b"\x1b]52;c;cHduZWQ=\x07\x1b]0;pwned\x07\x1bX\n";

struct Streams {
    lend: Lend,
    stdin: File,
    stdout: File,
    stderr: File,
    /// Input read but not yet a whole event.
    held: Vec<u8>,
}

impl Streams {
    fn open(ns: &Namespace, mut lend: Lend) -> Result<Streams, u32> {
        let mut stream = |i: u32, path: &str, how: u8| -> Result<File, u32> {
            let (conn, rest) = ns.lookup(path).ok_or(NO_STDIN + i)?;
            conn.open(&mut lend, rest, how).map_err(|_| NO_STDIN + i)
        };
        let stdin = stream(0, "/dev/stdin", mode::OREAD)?;
        let stdout = stream(1, "/dev/stdout", mode::OWRITE)?;
        let stderr = stream(2, "/dev/stderr", mode::OWRITE)?;
        Ok(Streams { lend, stdin, stdout, stderr, held: Vec::new() })
    }

    /// The next event, or `None` at the end of the input or on one that does not decode.
    fn event(&mut self) -> Option<Event> {
        loop {
            match cells::split(&self.held, cells::MAX_EVENT) {
                Ok(Some((body, rest))) => {
                    let event = cells::decode_event(body).ok();
                    self.held = rest.to_vec();
                    return event;
                }
                Ok(None) => {}
                Err(_) => return None,
            }
            let mut buf = vec![0; CHUNK];
            let n = self.stdin.read_at(&mut self.lend, 0, &mut buf).ok()?;
            if n == 0 {
                return None;
            }
            self.held.extend_from_slice(&buf[..n]);
        }
    }

    fn write(lend: &mut Lend, out: &File, mut bytes: &[u8]) -> Result<(), Error> {
        while !bytes.is_empty() {
            match out.write_at(lend, 0, &bytes[..bytes.len().min(CHUNK)])? {
                0 => return Err(Error::Unexpected),
                n => bytes = &bytes[n..],
            }
        }
        Ok(())
    }

    fn out(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Streams::write(&mut self.lend, &self.stdout, bytes)
    }

    fn err(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Streams::write(&mut self.lend, &self.stderr, bytes)
    }

    /// A frame, as a record.
    fn frame(&mut self, frame: &Frame) -> Result<(), Error> {
        self.out(&cells::record(&cells::encode(frame)))
    }
}

fn run(startup: &Startup) -> u32 {
    let Ok(mut lend) = Lend::new(LEND_PAGES) else { return NO_STREAM };
    let Ok(ns) = Namespace::from_startup(startup, &mut lend) else { return NO_STREAM };
    let args: Vec<&str> = startup.args().collect();
    let mut s = match Streams::open(&ns, lend) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let Some(Event::Size { width, height }) = s.event() else { return BAD_INPUT };
    let done = match args.as_slice() {
        ["draw"] => draw(&mut s, width, height),
        ["stderr"] => s.err(HOSTILE).and_then(|()| draw(&mut s, width, height)),
        ["flood"] => flood(&mut s, width, height),
        [how] => match misbehave(how, width, height) {
            Some(bytes) => s.out(&bytes).map(|()| -> u32 { redoubt_init_programs::park() }),
            None => Ok(BAD_ARGS),
        },
        _ => Ok(BAD_ARGS),
    };
    done.unwrap_or(NO_STREAM)
}

/// The cells of `text` along row `y` from column `x`, one character a cell, clipped.
fn cells(width: u16, x: u16, y: u16, text: &str, fg: Color) -> Vec<Cell> {
    text.chars()
        .zip(x..width)
        .filter_map(|(c, x)| {
            let symbol = Symbol::new(c.encode_utf8(&mut [0; 4])).ok()?;
            Some(Cell { x, y, symbol, fg, bg: Color::Reset, modifiers: Modifiers::NONE })
        })
        .collect()
}

fn text(width: u16, height: u16, x: u16, y: u16, text: &str) -> Frame {
    let cells = if y < height { cells(width, x, y, text, Color::Indexed(2)) } else { Vec::new() };
    Frame { width, height, clear: true, cells }
}

/// A box around `screen-stage-ready`, and under it the last key sent, on a row of its own.
fn screen(width: u16, height: u16, key: &str, n: u16) -> Frame {
    let mut frame = text(width, height, 2, 1, "screen-stage-ready");
    if height > 3 {
        let edge = String::from("+") + &"-".repeat(20) + "+";
        frame.cells.extend(cells(width, 0, 0, &edge, Color::Reset));
        frame.cells.extend(cells(width, 0, 2, &edge, Color::Reset));
        frame.cells.extend(cells(width, 0, 1, "|", Color::Reset));
        frame.cells.extend(cells(width, 21, 1, "|", Color::Reset));
        let row = 3 + n % (height - 3);
        frame.cells.extend(cells(width, 1, row, &format!("key:{key}"), Color::Rgb(255, 200, 0)));
    }
    frame
}

fn draw(s: &mut Streams, mut width: u16, mut height: u16) -> Result<u32, Error> {
    let mut key = String::from("none");
    let mut n = 0u16;
    loop {
        s.frame(&screen(width, height, &key, n))?;
        match s.event() {
            Some(Event::Size { width: w, height: h }) => (width, height) = (w, h),
            Some(Event::Key { key: Key::Symbol(symbol), .. }) if symbol.as_str() == "q" => return Ok(0),
            Some(Event::Key { key: k, modifiers }) => {
                key = name(&k, modifiers);
                n = n.wrapping_add(1);
            }
            None => return Ok(BAD_INPUT),
        }
    }
}

fn name(key: &Key, modifiers: u8) -> String {
    let ctrl = if modifiers & cells::CTRL != 0 { "ctrl+" } else { "" };
    match key {
        Key::Symbol(symbol) => format!("{ctrl}{}", symbol.as_str()),
        other => format!("{ctrl}{other:?}"),
    }
}

fn flood(s: &mut Streams, width: u16, height: u16) -> Result<u32, Error> {
    let mut frame = text(width, height, 1, 1, "screen-stage-flooding");
    for y in (0..height).filter(|&y| y != 1) {
        frame.cells.extend(cells(width, 0, y, &".".repeat(usize::from(width)), Color::Reset));
    }
    let record = cells::record(&cells::encode(&frame));
    loop {
        s.out(&record)?;
    }
}

/// One cell's bytes, written by hand: the crate's encoder would not write these symbols.
fn raw_cell(x: u16, y: u16, symbol: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&x.to_le_bytes());
    b.extend_from_slice(&y.to_le_bytes());
    b.push(symbol.len() as u8);
    b.extend_from_slice(symbol);
    b.extend_from_slice(&[0; 10]);
    b
}

fn raw_frame(width: u16, height: u16, cells: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![cells::VERSION, 1];
    b.extend_from_slice(&width.to_le_bytes());
    b.extend_from_slice(&height.to_le_bytes());
    b.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    cells.iter().for_each(|c| b.extend_from_slice(c));
    cells::record(&b)
}

/// What a misbehaviour sends, after its size.
fn misbehave(how: &str, width: u16, height: u16) -> Option<Vec<u8>> {
    let edge = width - 1;
    Some(match how {
        "raw" => [HOSTILE, b"XXXXXXXXXXXXXXXX\n"].concat(),
        "esc-symbol" => raw_frame(width, height, &[raw_cell(0, 0, b"\x1b]52;c;cHduZWQ=\x07")]),
        "key-symbol" => raw_frame(width, height, &[raw_cell(0, 0, b"\x1c")]),
        "big" => raw_frame(200, 100, &[raw_cell(150, 50, b"X")]),
        "outside" => raw_frame(width, height, &[raw_cell(width, 0, b"X")]),
        "long" => raw_frame(width, height, &[raw_cell(edge, 0, &[b'X'; 32])]),
        "wide-edge" => raw_frame(width, height, &[raw_cell(edge, 0, "界".as_bytes())]),
        "claim" => u32::MAX.to_le_bytes().to_vec(),
        _ => return None,
    })
}
