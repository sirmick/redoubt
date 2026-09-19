//! The NIFs of OTP's `zlib` module, over `miniz_oxide`: streams opened with `zlib:open/0`,
//! deflate and inflate in raw, zlib or gzip format (`WindowBits` negative, 8..15, 16 more, or 32
//! more to detect zlib or gzip when inflating). OTP's `zlib.erl` runs unchanged on top: it queues
//! input and asks for output a chunk at a time, which bounds what one call may produce (and so
//! what hostile input can make `safeInflate` allocate).
//!
//! Not provided: preset dictionaries (`deflateSetDictionary`, `inflateSetDictionary`, raising
//! `not_supported`) and compression strategies other than the default (accepted, ignored).

use crate::sync::Lock;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

use miniz_oxide::deflate::core::{
    compress, create_comp_flags_from_zip_params, CompressorOxide, TDEFLFlush, TDEFLStatus,
};
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use super::Ctx;
use crate::process::Exception;
use crate::term::{OwnedTerm, Pid, Resource, Term};

type R = Result<Term, Exception>;

/// Most bytes one stream may hold queued (input not yet consumed plus output not yet taken).
const MAX_QUEUED: usize = 1 << 28;

/// What to do with data after the end of a compressed stream (`zlib:inflateInit/3`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterEnd {
    Error,
    Reset,
    Cut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wrap {
    Raw,
    Zlib,
    Gzip,
    /// Inflate only: zlib or gzip, decided by the first bytes.
    Detect,
}

struct Deflater {
    core: Box<CompressorOxide>,
    gzip: bool,
    header_done: bool,
    crc: u32,
    size: u32,
    /// Some input was given (ending the stream early is then an error, as in zlib).
    used: bool,
    finished: bool,
}

/// Where inflating a gzip stream has got to.
enum GzipStage {
    Header,
    Body,
    Trailer,
    Done,
}

struct Inflater {
    state: Box<InflateState>,
    /// The format asked for, and the one in use (they differ once `Detect` has decided).
    initial: Wrap,
    wrap: Wrap,
    after_end: AfterEnd,
    stage: GzipStage,
    /// Bytes of a gzip header or trailer gathered so far.
    frame: Vec<u8>,
    crc: u32,
    size: u32,
    /// The end of the stream has been reached.
    ended: bool,
}

enum Codec {
    None,
    Deflate(Deflater),
    Inflate(Inflater),
}

struct Stream {
    owner: Lock<Pid>,
    input: Lock<VecDeque<u8>>,
    /// Output made but not yet handed out (it comes out a chunk at a time).
    output: Lock<VecDeque<u8>>,
    codec: Lock<Codec>,
    /// Kept outside the heap of whoever set it.
    stash: Lock<Option<OwnedTerm>>,
}

/// A stream held through its resource (so the caller's heap stays free for building results).
struct StreamRef(alloc::sync::Arc<Resource>);

impl core::ops::Deref for StreamRef {
    type Target = Stream;
    fn deref(&self) -> &Stream {
        self.0.get::<Stream>().expect("checked when made")
    }
}

// ---- errors ----

fn raise(c: &mut Ctx, what: &str) -> Exception {
    Exception::error(c.atom(what))
}

/// The stream an argument names, if the caller controls it.
fn stream(c: &mut Ctx, t: &Term) -> Result<StreamRef, Exception> {
    let r = c
        .heap()
        .as_resource(*t)
        .filter(|r| r.get::<Stream>().is_some())
        .cloned()
        .ok_or_else(|| c.badarg())?;
    let s = StreamRef(r);
    if *s.owner.lock() != c.p.pid {
        return Err(raise(c, "not_on_controlling_process"));
    }
    Ok(s)
}

fn int(c: &Ctx, t: &Term) -> Result<i64, Exception> {
    match t {
        Term::Int(n) => Ok(*n),
        _ => Err(c.badarg()),
    }
}

// ---- gzip framing ----

const GZIP_HEADER: [u8; 10] = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];

/// The length of the gzip header at the start of `h`, once all of it is there.
fn gzip_header_len(h: &[u8]) -> Result<Option<usize>, ()> {
    if h.len() < 10 {
        return Ok(None);
    }
    if h[0] != 0x1f || h[1] != 0x8b || h[2] != 8 {
        return Err(());
    }
    let flags = h[3];
    let mut n = 10;
    if flags & 4 != 0 {
        // FEXTRA: a two-byte length, then that many bytes.
        let Some(len) = h.get(n..n + 2) else {
            return Ok(None);
        };
        n += 2 + u16::from_le_bytes([len[0], len[1]]) as usize;
    }
    for flag in [8, 16] {
        // FNAME, FCOMMENT: zero-terminated.
        if flags & flag != 0 {
            match h.get(n..).and_then(|r| r.iter().position(|&b| b == 0)) {
                Some(z) => n += z + 1,
                None => return Ok(None),
            }
        }
    }
    if flags & 2 != 0 {
        n += 2; // FHCRC
    }
    Ok(if h.len() >= n { Some(n) } else { None })
}

// ---- NIFs ----

pub fn open(c: &mut Ctx, _a: &[Term]) -> R {
    let s = Stream {
        owner: Lock::new(c.p.pid),
        input: Lock::new(VecDeque::new()),
        output: Lock::new(VecDeque::new()),
        codec: Lock::new(Codec::None),
        stash: Lock::new(None),
    };
    let id = c.sys.make_ref().0;
    Ok(c.heap_mut().resource(Resource {
        id,
        value: Box::new(s),
    }))
}

pub fn close(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    *s.codec.lock() = Codec::None;
    s.input.lock().clear();
    s.output.lock().clear();
    Ok(c.ok())
}

pub fn set_controller(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let Term::Pid(p) = a[1] else {
        return Err(c.badarg());
    };
    *s.owner.lock() = p;
    Ok(c.ok())
}

/// `deflateInit_nif(Z, Level, Method, WindowBits, MemLevel, Strategy)`.
pub fn deflate_init(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let level = int(c, &a[1])?;
    let bits = int(c, &a[3])?;
    let level = if level < 0 { 6 } else { level.min(9) };
    let (gzip, bits) = if bits > 15 {
        (true, -(bits - 16))
    } else {
        (false, bits)
    };
    if !matches!(bits.abs(), 8..=15) {
        return Err(c.badarg());
    }
    if !matches!(*s.codec.lock(), Codec::None) {
        return Err(raise(c, "already_initialized"));
    }
    let flags = create_comp_flags_from_zip_params(level as i32, bits as i32, 0);
    let core = Box::new(CompressorOxide::new(flags));
    *s.codec.lock() = Codec::Deflate(Deflater {
        core,
        gzip,
        header_done: false,
        crc: 0,
        size: 0,
        used: false,
        finished: false,
    });
    Ok(c.ok())
}

/// `inflateInit_nif(Z, WindowBits, EoSBehavior)`.
pub fn inflate_init(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let bits = int(c, &a[1])?;
    let after_end = match int(c, &a[2])? {
        0 => AfterEnd::Error,
        1 => AfterEnd::Reset,
        _ => AfterEnd::Cut,
    };
    let wrap = match bits {
        -15..=-8 => Wrap::Raw,
        8..=15 => Wrap::Zlib,
        24..=31 => Wrap::Gzip,
        40..=47 => Wrap::Detect,
        _ => return Err(c.badarg()),
    };
    if !matches!(*s.codec.lock(), Codec::None) {
        return Err(raise(c, "already_initialized"));
    }
    *s.codec.lock() = Codec::Inflate(new_inflater(wrap, after_end));
    Ok(c.ok())
}

fn new_inflater(wrap: Wrap, after_end: AfterEnd) -> Inflater {
    let format = if wrap == Wrap::Zlib {
        DataFormat::Zlib
    } else {
        DataFormat::Raw
    };
    Inflater {
        state: InflateState::new_boxed(format),
        initial: wrap,
        wrap,
        after_end,
        stage: GzipStage::Header,
        frame: Vec::new(),
        crc: 0,
        size: 0,
        ended: false,
    }
}

pub fn enqueue(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let mut data = Vec::new();
    for b in c.list_arg(a[1])? {
        match c.heap().as_bits(b) {
            Some(b) if b.is_binary() => data.extend_from_slice(&b.to_bytes()),
            _ => return Err(c.badarg()),
        }
    }
    let mut input = s.input.lock();
    if input.len() + data.len() > MAX_QUEUED {
        return Err(c.system_limit());
    }
    input.extend(data);
    Ok(c.ok())
}

/// Take up to `n` queued input bytes.
fn take_input(s: &Stream, n: usize) -> Vec<u8> {
    let mut input = s.input.lock();
    let n = n.min(input.len());
    input.drain(..n).collect()
}

/// Hand out up to `chunk` bytes of output: `{finished, Out}` when nothing more is waiting and
/// the chunk was not filled, else `{continue, Out}`.
fn chunk(c: &mut Ctx, s: &Stream, chunk: usize) -> Term {
    let mut output = s.output.lock();
    let n = chunk.min(output.len());
    let bytes: Vec<u8> = output.drain(..n).collect();
    let done = output.is_empty() && s.input.lock().is_empty() && n < chunk;
    drop(output);
    let out = if bytes.is_empty() {
        Term::Nil
    } else {
        let b = c.binary(&bytes);
        c.list([b])
    };
    let tag = c.atom(if done { "finished" } else { "continue" });
    c.tuple(&[tag, out])
}

/// `deflate_nif(Z, InputChunk, OutputChunk, Flush)`: flush is 0 (none), 2 (sync), 3 (full) or
/// 4 (finish), applied once the queued input is used up.
pub fn deflate(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let (in_chunk, out_chunk, flush) = (int(c, &a[1])?, int(c, &a[2])?, int(c, &a[3])?);
    let (in_chunk, out_chunk) = (in_chunk.max(1) as usize, out_chunk.max(1) as usize);
    let flush = match flush {
        0 => TDEFLFlush::None,
        2 => TDEFLFlush::Sync,
        3 => TDEFLFlush::Full,
        4 => TDEFLFlush::Finish,
        _ => return Err(c.badarg()),
    };
    let mut codec = s.codec.lock();
    let Codec::Deflate(d) = &mut *codec else {
        return Err(raise(c, "not_initialized"));
    };
    let mut out = Vec::new();
    if d.gzip && !d.header_done {
        out.extend_from_slice(&GZIP_HEADER);
        d.header_done = true;
    }
    let input = take_input(&s, in_chunk);
    if !input.is_empty() {
        d.used = true;
        if d.gzip {
            d.crc = super::info::crc32_update(d.crc, &input);
            d.size = d.size.wrapping_add(input.len() as u32);
        }
    }
    let mut buf = alloc::vec![0u8; 1 << 16];
    let mut rest = &input[..];
    while !rest.is_empty() {
        let (status, used, made) = compress(&mut d.core, rest, &mut buf, TDEFLFlush::None);
        out.extend_from_slice(&buf[..made]);
        rest = &rest[used..];
        if status != TDEFLStatus::Okay {
            return Err(raise(c, "stream_error"));
        }
    }
    if s.input.lock().is_empty() && flush != TDEFLFlush::None && !d.finished {
        loop {
            let (status, _, made) = compress(&mut d.core, &[], &mut buf, flush);
            out.extend_from_slice(&buf[..made]);
            match status {
                TDEFLStatus::Done => {
                    d.finished = true;
                    if d.gzip {
                        out.extend_from_slice(&d.crc.to_le_bytes());
                        out.extend_from_slice(&d.size.to_le_bytes());
                    }
                    break;
                }
                TDEFLStatus::Okay if made < buf.len() => break,
                TDEFLStatus::Okay => {}
                _ => return Err(raise(c, "stream_error")),
            }
        }
    }
    drop(codec);
    s.output.lock().extend(out);
    Ok(chunk(c, &s, out_chunk))
}

/// `inflate_nif(Z, InputChunk, OutputChunk, Flush)`.
pub fn inflate_nif(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let (in_chunk, out_chunk) = (
        int(c, &a[1])?.max(1) as usize,
        int(c, &a[2])?.max(1) as usize,
    );
    let mut codec = s.codec.lock();
    let Codec::Inflate(inf) = &mut *codec else {
        return Err(raise(c, "not_initialized"));
    };
    // Data after the end of the stream.
    if inf.ended && !s.input.lock().is_empty() {
        match inf.after_end {
            AfterEnd::Error => return Err(raise(c, "data_error")),
            AfterEnd::Reset => *inf = new_inflater(inf.initial, inf.after_end),
            AfterEnd::Cut => s.input.lock().clear(),
        }
    }
    // Produce until the chunk is full or the input is used up.
    let mut out: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; out_chunk];
    let mut consumed_total = 0;
    while out.len() < out_chunk && !inf.ended && consumed_total < in_chunk {
        let avail = s.input.lock().len();
        if avail == 0 {
            break;
        }
        // Frames (gzip header and trailer) are gathered byte by byte from the queue.
        if matches!(inf.wrap, Wrap::Detect) {
            let first = s.input.lock()[0];
            inf.wrap = if first == 0x1f {
                Wrap::Gzip
            } else {
                Wrap::Zlib
            };
            if inf.wrap == Wrap::Zlib {
                inf.state = InflateState::new_boxed(DataFormat::Zlib);
            }
            continue;
        }
        if inf.wrap == Wrap::Gzip && !matches!(inf.stage, GzipStage::Body) {
            let b = s.input.lock().pop_front().expect("available");
            consumed_total += 1;
            inf.frame.push(b);
            match inf.stage {
                GzipStage::Header => match gzip_header_len(&inf.frame) {
                    Err(()) => return Err(raise(c, "data_error")),
                    Ok(Some(_)) => {
                        inf.frame.clear();
                        inf.stage = GzipStage::Body;
                    }
                    Ok(None) => {}
                },
                GzipStage::Trailer if inf.frame.len() == 8 => {
                    let f = &inf.frame;
                    let crc = u32::from_le_bytes([f[0], f[1], f[2], f[3]]);
                    let size = u32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                    if crc != inf.crc || size != inf.size {
                        return Err(raise(c, "data_error"));
                    }
                    inf.stage = GzipStage::Done;
                    inf.ended = true;
                }
                _ => {}
            }
            continue;
        }
        let input: Vec<u8> = {
            let q = s.input.lock();
            q.iter().take(in_chunk - consumed_total).copied().collect()
        };
        let room = out_chunk - out.len();
        let r = inflate(&mut inf.state, &input, &mut buf[..room], MZFlush::None);
        s.input.lock().drain(..r.bytes_consumed);
        consumed_total += r.bytes_consumed;
        let made = &buf[..r.bytes_written];
        if inf.wrap == Wrap::Gzip {
            inf.crc = super::info::crc32_update(inf.crc, made);
            inf.size = inf.size.wrapping_add(made.len() as u32);
        }
        out.extend_from_slice(made);
        match r.status {
            Ok(MZStatus::StreamEnd) => {
                if inf.wrap == Wrap::Gzip {
                    inf.stage = GzipStage::Trailer;
                } else {
                    inf.ended = true;
                }
            }
            Ok(_) => {}
            // No progress possible until more input comes.
            Err(MZError::Buf) => break,
            Err(_) => return Err(raise(c, "data_error")),
        }
        if r.bytes_consumed == 0 && r.bytes_written == 0 {
            break;
        }
    }
    drop(codec);
    s.output.lock().extend(out);
    Ok(chunk(c, &s, out_chunk))
}

/// `deflateReset_nif` and `inflateReset_nif`: start a new stream with the same settings.
pub fn reset(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let mut codec = s.codec.lock();
    match &mut *codec {
        Codec::Deflate(d) => {
            d.core.reset();
            (d.header_done, d.crc, d.size, d.used, d.finished) = (false, 0, 0, false, false);
        }
        Codec::Inflate(i) => *i = new_inflater(i.initial, i.after_end),
        Codec::None => return Err(raise(c, "not_initialized")),
    }
    drop(codec);
    s.input.lock().clear();
    s.output.lock().clear();
    Ok(c.ok())
}

/// `deflateEnd_nif`: `data_error` if the stream was used and not finished, as in zlib.
pub fn deflate_end(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let bad = match &*s.codec.lock() {
        Codec::Deflate(d) => d.used && !d.finished,
        _ => return Err(raise(c, "not_initialized")),
    };
    *s.codec.lock() = Codec::None;
    s.input.lock().clear();
    s.output.lock().clear();
    if bad {
        return Err(raise(c, "data_error"));
    }
    Ok(c.ok())
}

/// `inflateEnd_nif`: `data_error` unless the whole stream was read.
pub fn inflate_end(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let bad = match &*s.codec.lock() {
        Codec::Inflate(i) => !i.ended || !s.input.lock().is_empty(),
        _ => return Err(raise(c, "not_initialized")),
    };
    *s.codec.lock() = Codec::None;
    s.input.lock().clear();
    s.output.lock().clear();
    if bad {
        return Err(raise(c, "data_error"));
    }
    Ok(c.ok())
}

/// `deflateParams_nif(Z, Level, Strategy)`: the level changes; the strategy is ignored.
pub fn deflate_params(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let level = int(c, &a[1])?;
    let level = if level < 0 { 6 } else { level.min(9) } as u8;
    match &mut *s.codec.lock() {
        Codec::Deflate(d) => d.core.set_compression_level_raw(level),
        _ => return Err(raise(c, "not_initialized")),
    }
    Ok(c.ok())
}

pub fn not_supported(c: &mut Ctx, a: &[Term]) -> R {
    stream(c, &a[0])?;
    Err(raise(c, "not_supported"))
}

pub fn get_stash(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    let stash = s.stash.lock().clone();
    Ok(match stash {
        Some(t) => {
            let t = c.copy_in(&t);
            c.ok_tuple(t)
        }
        None => c.atom("empty"),
    })
}

pub fn set_stash(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    if s.stash.lock().is_some() {
        return Err(raise(c, "error"));
    }
    *s.stash.lock() = Some(c.own(a[1]));
    Ok(c.ok())
}

pub fn clear_stash(c: &mut Ctx, a: &[Term]) -> R {
    let s = stream(c, &a[0])?;
    if s.stash.lock().take().is_none() {
        return Err(raise(c, "error"));
    }
    Ok(c.ok())
}
