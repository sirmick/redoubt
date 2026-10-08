//! `beamlet-screen`: the screen buffer's natives, module `redoubt_screen`
//! (docs/userland/beamlet.md, "Screen natives").
//!
//! Primitives over cells, never widgets: a buffer is a grid of cells a screen program draws into,
//! and `diff/1` hands the session the cells that changed as a `cells` frame, which the shell's
//! encoder draws. Widgets, layout and focus are Elixir (`Redoubt.Screen`).
//!
//! - **A control character is refused, not drawn:** `put/5` and `fill/4` raise `badarg` on one (the `cells`
//!   crate's own rule), so a caller that forgot to make text visible fails loudly.
//! - **Bounded:** 1024 cells a side and 65,536 in all; `put/5` reads no more graphemes than the row has
//!   cells; no call makes more than one pass over a buffer; each is charged in reductions by the cells it
//!   touched.
//! - **Counted:** a process holds at most four buffers, and each declares its grids' bytes, which count as
//!   its holder's own memory, toward its heap limit.
//! - **One writer:** a buffer answers only the process that made it; any other gets `badarg`.
//!
//! Text comes in as graphemes, split by OTP's own segmentation (`String.graphemes/1`), and is
//! measured by a table generated from OTP's `unicode_util:is_wide/1` (`wide.rs`): what the shell
//! measures is what the buffer lays out, at one Unicode version.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use beamlet_vm::bif::{Ctx, NativeSpec};
use beamlet_vm::sync::Lock;
use beamlet_vm::term::Pid;
use beamlet_vm::{Exception, Term};
use cells::{Color, Modifiers};

mod buffer;
mod wide;

pub use buffer::{Buffer, MAX_CELLS, Rect, Refusal, Style, UNICODE, columns};

type R = Result<Term, Exception>;

/// The most buffers one process holds at once.
pub const MAX_BUFFERS: usize = 4;

/// The process dictionary's key for the count of a process's buffers.
const COUNT: &str = "$redoubt_screen_buffers";

pub static NATIVES: &[NativeSpec] = &[
    ("redoubt_screen", "new", 2, new),
    ("redoubt_screen", "resize", 3, resize),
    ("redoubt_screen", "put", 5, put),
    ("redoubt_screen", "fill", 4, fill),
    ("redoubt_screen", "plot", 4, plot),
    ("redoubt_screen", "diff", 1, diff),
];

/// A buffer as a resource: its owner, the grid, and its share of the owner's count of buffers,
/// given back when the last term holding it goes.
struct Grid {
    owner: Pid,
    buffer: Lock<Buffer>,
    count: Arc<AtomicUsize>,
}

impl Drop for Grid {
    fn drop(&mut self) { self.count.fetch_sub(1, Ordering::Relaxed); }
}

/// The process's count of its buffers, kept in its dictionary as a resource. Code that erases
/// the entry starts a new count; the heap limit still bounds what its buffers hold.
fn count(c: &mut Ctx) -> Arc<AtomicUsize> {
    let key = c.atom(COUNT);
    if let Some(held) = c.p.dictionary.get(&c.p.heap, key).and_then(|t| c.resource::<Arc<AtomicUsize>>(t)) {
        return Arc::clone(&held);
    }
    let fresh = Arc::new(AtomicUsize::new(0));
    let t = c.new_resource(Arc::clone(&fresh));
    let heap = &c.p.heap;
    c.p.dictionary.put(heap, key, t);
    fresh
}

/// The buffer `t` names, if the caller made it.
fn buffer(c: &Ctx, t: Term) -> Result<beamlet_vm::bif::Held<Grid>, Exception> {
    let held = c.resource::<Grid>(t).ok_or_else(|| c.badarg())?;
    if held.owner != c.p.pid {
        return Err(c.badarg());
    }
    Ok(held)
}

/// Charge the caller for `cells` cells touched: one reduction for each sixteen, and one for the
/// call.
fn charge(c: &mut Ctx, cells: usize) { c.p.budget = c.p.budget.saturating_sub(1 + cells / 16).max(1); }

/// A coordinate or a length: a non-negative integer that fits in 16 bits.
fn u16_of(c: &Ctx, t: Term) -> Result<u16, Exception> {
    t.as_usize().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| c.badarg())
}

fn side(c: &Ctx, t: Term) -> Result<u16, Exception> {
    u16_of(c, t).and_then(|n| if n >= 1 { Ok(n) } else { Err(c.badarg()) })
}

fn refused(c: &Ctx, r: Refusal) -> Exception {
    match r {
        Refusal::Size => c.system_limit(),
        Refusal::Control | Refusal::Shape => c.badarg(),
    }
}

/// `{Fg, Bg, Modifiers}`, the colours as `reset`, `{indexed, I}` or `{rgb, R, G, B}`.
fn style(c: &Ctx, t: Term) -> Result<Style, Exception> {
    let e = c.tuple_elems(t).filter(|e| e.len() == 3).ok_or_else(|| c.badarg())?;
    let modifiers = e[2]
        .as_usize()
        .and_then(|n| u16::try_from(n).ok())
        .and_then(|n| Modifiers::new(n).ok())
        .ok_or_else(|| c.badarg())?;
    Ok(Style { fg: color(c, e[0])?, bg: color(c, e[1])?, modifiers })
}

fn color(c: &Ctx, t: Term) -> Result<Color, Exception> {
    let byte = |t: &Term| t.as_usize().and_then(|n| u8::try_from(n).ok());
    if let Term::Atom(a) = t {
        if a.as_str() == "reset" {
            return Ok(Color::Reset);
        }
    }
    let e = c.tuple_elems(t).ok_or_else(|| c.badarg())?;
    match e.as_slice() {
        [Term::Atom(a), i] if a.as_str() == "indexed" => byte(i).map(Color::Indexed),
        [Term::Atom(a), r, g, b] if a.as_str() == "rgb" => match (byte(r), byte(g), byte(b)) {
            (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r, g, b)),
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| c.badarg())
}

/// `{X, Y, W, H}`.
fn rect(c: &Ctx, t: Term) -> Result<Rect, Exception> {
    let e = c.tuple_elems(t).filter(|e| e.len() == 4).ok_or_else(|| c.badarg())?;
    Ok(Rect { x: u16_of(c, e[0])?, y: u16_of(c, e[1])?, w: u16_of(c, e[2])?, h: u16_of(c, e[3])? })
}

/// The UTF-8 of a binary.
fn text(c: &Ctx, t: Term) -> Result<alloc::string::String, Exception> {
    let bits = c.heap().as_bits(t).filter(|b| b.is_binary()).ok_or_else(|| c.badarg())?;
    alloc::string::String::from_utf8(bits.to_bytes().into_owned()).map_err(|_| c.badarg())
}

// ---- the natives ----

/// `new(W, H)`: a blank buffer, owned by the caller.
fn new(c: &mut Ctx, a: &[Term]) -> R {
    let (w, h) = (side(c, a[0])?, side(c, a[1])?);
    let count = count(c);
    if count.load(Ordering::Relaxed) >= MAX_BUFFERS {
        return Err(c.system_limit());
    }
    let b = Buffer::new(w, h).map_err(|r| refused(c, r))?;
    count.fetch_add(1, Ordering::Relaxed);
    let bytes = b.bytes();
    charge(c, usize::from(w) * usize::from(h));
    let grid = Grid { owner: c.p.pid, buffer: Lock::new(b), count };
    Ok(c.new_resource_sized(grid, bytes))
}

/// `resize(B, W, H)`: a new size, blank; the next `diff/1` clears the screen and sends it all.
fn resize(c: &mut Ctx, a: &[Term]) -> R {
    let held = buffer(c, a[0])?;
    let (w, h) = (side(c, a[1])?, side(c, a[2])?);
    let bytes = {
        let mut b = held.buffer.lock();
        b.resize(w, h).map_err(|r| refused(c, r))?;
        b.bytes()
    };
    c.resize_resource(a[0], bytes);
    charge(c, usize::from(w) * usize::from(h));
    Ok(c.ok())
}

/// `put(B, X, Y, Graphemes, Style)`: the graphemes along row `Y` from `X`; the columns written.
fn put(c: &mut Ctx, a: &[Term]) -> R {
    let held = buffer(c, a[0])?;
    let (x, y, style) = (u16_of(c, a[1])?, u16_of(c, a[2])?, style(c, a[4])?);
    // No more graphemes are read than the row has cells from X.
    let room = held.buffer.lock().width().saturating_sub(x);
    let mut graphemes = Vec::new();
    for item in c.heap().list_iter(a[3]).take(usize::from(room)) {
        let item = item.map_err(|_| c.badarg())?;
        graphemes.push(text(c, item)?);
    }
    let written = held
        .buffer
        .lock()
        .put(x, y, graphemes.iter().map(|g| g.as_str()), style)
        .map_err(|r| refused(c, r))?;
    charge(c, graphemes.len());
    Ok(Term::Int(i64::from(written)))
}

/// `fill(B, {X, Y, W, H}, Symbol, Style)`: one symbol over a rectangle, clipped.
fn fill(c: &mut Ctx, a: &[Term]) -> R {
    let held = buffer(c, a[0])?;
    let (r, symbol, style) = (rect(c, a[1])?, text(c, a[2])?, style(c, a[3])?);
    held.buffer.lock().fill(r, &symbol, style).map_err(|e| refused(c, e))?;
    charge(c, usize::from(r.w) * usize::from(r.h));
    Ok(c.ok())
}

/// `plot(B, {X, Y, W, H}, Dots, Style)`: a Braille bitmap, one byte a cell.
fn plot(c: &mut Ctx, a: &[Term]) -> R {
    let held = buffer(c, a[0])?;
    let (r, style) = (rect(c, a[1])?, style(c, a[3])?);
    let dots = c.heap().as_bits(a[2]).filter(|b| b.is_binary()).ok_or_else(|| c.badarg())?;
    held.buffer.lock().plot(r, &dots.to_bytes(), style).map_err(|e| refused(c, e))?;
    charge(c, usize::from(r.w) * usize::from(r.h));
    Ok(c.ok())
}

/// `diff(B)`: the cells changed since the last call, as a `cells` frame's bytes.
fn diff(c: &mut Ctx, a: &[Term]) -> R {
    let held = buffer(c, a[0])?;
    let (frame, cells) = {
        let mut b = held.buffer.lock();
        let cells = usize::from(b.width()) * usize::from(b.height());
        (b.diff(), cells)
    };
    let bytes = cells::encode(&frame);
    charge(c, cells);
    Ok(c.binary(&bytes))
}
