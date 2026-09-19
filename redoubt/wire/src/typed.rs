//! Framing for typed messages (WIRE.md): everything that is not 9P uses 9P's encoding
//! ([`crate::codec`]), laid out by a table in the owning server's note. The per-protocol
//! codecs in [`crate::proto`] are generated from those tables by `redoubt-wire-gen`; this
//! module is the part they share.
//!
//! A message is `WORDS` (4) machine words, up to 4 handles, and at most one buffer
//! (KERNEL-SPEC.md, Messages). **Word 0 of a request is its opcode.** Each message type is
//! one of two shapes, fixed by its table, and its reply has the same shape:
//!
//! - **inline**: the request's fields and the reply's fields are each fixed-size integers
//!   whose encoding fits in [`INLINE_BYTES`]; the bytes are packed into words 1..=3, four per
//!   word, little-endian, zero-padded; there is no buffer.
//! - **buffer**: the request's or the reply's fields need more room, or are variable-length.
//!   The encoding of the request's fields goes in the buffer (a lend for `call`, a transfer
//!   for `send`) and word 1 holds its length; words 2 and 3 are zero. The reply is written
//!   into the caller's lend, its length in word 1: a `reply` carries only words and handles,
//!   so reply data can only travel in the lend.
//!
//! **Word 0 of a reply is a status**: 0 for success, otherwise a code from the protocol's
//! error table. An error reply has words 1..=3 zero and no handles, and the caller ignores
//! the buffer. The reply does not carry the request's opcode: the caller knows what it sent
//! and names it when decoding.
//!
//! **A typed operation written into a 9P file** (e.g. `ipd`'s `ctl` files) has no words, so
//! its bytes are the opcode as a `u32` followed by the buffer-shape encoding of the fields,
//! one operation per `Twrite`. A message that carries handles cannot be written into a file.
//!
//! Words are passed as `u64` so the same code serves both widths (an rv32 word widens
//! losslessly). Every word this module produces fits in 32 bits, and every word it accepts
//! must, so a layout is the same on rv32 and rv64: an inline message carries at most 12
//! bytes, the capacity of three 32-bit words. Handles travel in the message's handle slots;
//! the codec does not see them, but checks their count against the layout.

use crate::codec::{Error, Reader, Writer};
use crate::MSIZE;

/// Machine words in a message (KERNEL-SPEC.md `WORDS`).
pub const WORDS: usize = 4;
/// Handle slots in a message (KERNEL-SPEC.md `MAX_MSG_HANDLES`).
pub const MAX_MSG_HANDLES: usize = 4;
/// Bytes an inline message carries: words 1..=3 at 32 bits each (see the module docs).
pub const INLINE_BYTES: usize = 12;

/// A message's words, widened to `u64`.
pub type Words = [u64; WORDS];

/// One row of a generated protocol's layout: a request, or the reply to one (keyed by the
/// request's opcode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub opcode: u32,
    pub inline: bool,
    /// How many handles travel with it.
    pub handles: usize,
}

/// The layout for `opcode`, or `BadOpcode`.
pub fn layout(layouts: &[Layout], opcode: u32) -> Result<&Layout, Error> {
    layouts.iter().find(|l| l.opcode == opcode).ok_or(Error::BadOpcode)
}

/// The opcode in word 0 of a request.
pub fn opcode(words: &Words) -> Result<u32, Error> {
    u32::try_from(words[0]).map_err(|_| Error::BadWords)
}

/// The status in word 0 of a reply: `None` for success, `Some(code)` for an error reply,
/// whose other words must be zero and which carries no handles.
pub fn reply_status(words: &Words, handles: usize) -> Result<Option<u32>, Error> {
    let status = u32::try_from(words[0]).map_err(|_| Error::BadWords)?;
    if status == 0 {
        return Ok(None);
    }
    if words[1..].iter().any(|&w| w != 0) {
        return Err(Error::BadWords);
    }
    check_handles(handles, 0)?;
    Ok(Some(status))
}

/// The words of an error reply.
pub fn error_reply(code: u32) -> Words {
    [u64::from(code), 0, 0, 0]
}

/// Encodes a request (`word0` is its opcode) or a reply (`word0` is 0) in its layout's
/// shape: `body` writes the fields.
pub fn encode(
    layout: &Layout,
    word0: u32,
    buf: &mut [u8],
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<Words, Error> {
    if layout.inline {
        encode_inline(word0, body)
    } else {
        encode_buffer(word0, buf, body)
    }
}

/// Decodes the fields of a request or reply whose layout is known. `read_inline` reads the
/// fields of inline messages (from a copy of words 1..=3, so it cannot borrow from them);
/// `read_buffer` reads any message's fields from the buffer.
pub fn decode<'a, T>(
    layout: &Layout,
    words: &Words,
    buf: &'a [u8],
    handles: usize,
    read_inline: impl FnOnce(u32, &mut Reader<'_>) -> Result<T, Error>,
    read_buffer: impl FnOnce(u32, &mut Reader<'a>) -> Result<T, Error>,
) -> Result<T, Error> {
    check_handles(handles, layout.handles)?;
    if layout.inline {
        let bytes = inline_bytes(words, buf)?;
        let mut r = Reader::new(&bytes);
        let value = read_inline(layout.opcode, &mut r)?;
        r.finish_padding()?;
        Ok(value)
    } else {
        let mut r = Reader::new(buffer_body(words, buf)?);
        let value = read_buffer(layout.opcode, &mut r)?;
        r.finish()?;
        Ok(value)
    }
}

/// Writes a request into the front of `out` in the file framing (the module docs) and
/// returns its length. Atomic: on failure nothing is left in `out`.
pub fn encode_file(
    layout: &Layout,
    out: &mut [u8],
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<usize, Error> {
    check_handles(layout.handles, 0)?;
    Writer::new(out).atomic(|w| {
        w.u32(layout.opcode)?;
        body(w)?;
        if w.position() > MSIZE {
            return Err(Error::TooLarge);
        }
        Ok(w.position())
    })
}

/// Decodes a request written into a file: all of `bytes` is one operation.
pub fn decode_file<'a, T>(
    layouts: &[Layout],
    bytes: &'a [u8],
    read_buffer: impl FnOnce(u32, &mut Reader<'a>) -> Result<T, Error>,
) -> Result<T, Error> {
    if bytes.len() > MSIZE {
        return Err(Error::TooLarge);
    }
    let mut r = Reader::new(bytes);
    let layout = layout(layouts, r.u32()?)?;
    check_handles(layout.handles, 0)?;
    let value = read_buffer(layout.opcode, &mut r)?;
    r.finish()?;
    Ok(value)
}

/// Encodes an inline message: `body` writes the fields, which the generator has checked
/// fit in [`INLINE_BYTES`] (a longer body fails with `TooLarge`, it is never truncated).
pub fn encode_inline(
    word0: u32,
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<Words, Error> {
    let mut bytes = [0u8; INLINE_BYTES];
    body(&mut Writer::new(&mut bytes))?;
    let mut words = [u64::from(word0), 0, 0, 0];
    for (word, chunk) in words.iter_mut().skip(1).zip(bytes.chunks_exact(4)) {
        let chunk: [u8; 4] = chunk.try_into().map_err(|_| Error::TooLarge)?;
        *word = u64::from(u32::from_le_bytes(chunk));
    }
    Ok(words)
}

/// The inline bytes of a received message, unpacked from words 1..=3. An inline message
/// has no buffer, so `buf` (whatever arrived with it) must be empty.
pub fn inline_bytes(words: &Words, buf: &[u8]) -> Result<[u8; INLINE_BYTES], Error> {
    if !buf.is_empty() {
        return Err(Error::UnexpectedBuffer);
    }
    let mut bytes = [0u8; INLINE_BYTES];
    for (chunk, word) in bytes.chunks_exact_mut(4).zip(words.iter().skip(1)) {
        let word = u32::try_from(*word).map_err(|_| Error::BadWords)?;
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

/// Encodes a buffer message: `body` writes the fields into `buf`; word 1 gets their length.
/// Atomic: on failure nothing is left in `buf`.
pub fn encode_buffer(
    word0: u32,
    buf: &mut [u8],
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<Words, Error> {
    let len = Writer::new(buf).atomic(|w| {
        body(w)?;
        if w.position() > MSIZE {
            return Err(Error::TooLarge);
        }
        u32::try_from(w.position()).map_err(|_| Error::TooLarge)
    })?;
    Ok([u64::from(word0), u64::from(len), 0, 0])
}

/// The encoded fields of a received buffer message: the first `words[1]` bytes of `buf`.
/// `buf` is whatever arrived (a lent or transferred buffer, possibly longer); the length
/// must be within it and within [`MSIZE`].
pub fn buffer_body<'a>(words: &Words, buf: &'a [u8]) -> Result<&'a [u8], Error> {
    if words[2] != 0 || words[3] != 0 {
        return Err(Error::BadWords);
    }
    let len = usize::try_from(words[1]).map_err(|_| Error::BadWords)?;
    if len > MSIZE {
        return Err(Error::BadWords);
    }
    buf.get(..len).ok_or(Error::Short)
}

/// Checks the handle count a message arrived with against its layout's.
pub fn check_handles(got: usize, layout: usize) -> Result<(), Error> {
    if got == layout { Ok(()) } else { Err(Error::BadHandles) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_packs_little_endian_into_three_words() {
        let words = encode_inline(7, |w| {
            w.u8(1)?;
            w.u64(0x0908_0706_0504_0302)?;
            w.u16(0x0b0a)
        })
        .unwrap();
        assert_eq!(words, [7, 0x0403_0201, 0x0807_0605, 0x000b_0a09]);
        let bytes = inline_bytes(&words, &[]).unwrap();
        assert_eq!(inline_bytes(&words, &[0]), Err(Error::UnexpectedBuffer));
        assert_eq!(bytes, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0]);
    }

    #[test]
    fn inline_overflow_is_refused() {
        assert_eq!(encode_inline(1, |w| w.put(&[0; 13])), Err(Error::TooLarge));
    }

    #[test]
    fn words_must_fit_in_32_bits() {
        assert_eq!(opcode(&[1 << 32, 0, 0, 0]), Err(Error::BadWords));
        assert_eq!(inline_bytes(&[0, 0, 1 << 32, 0], &[]), Err(Error::BadWords));
        assert_eq!(buffer_body(&[0, 1 << 32, 0, 0], &[]), Err(Error::BadWords));
        assert_eq!(reply_status(&[1 << 32, 0, 0, 0], 0), Err(Error::BadWords));
    }

    #[test]
    fn buffer_length_is_bounded() {
        let buf = [0u8; 8];
        assert_eq!(buffer_body(&[0, 8, 0, 0], &buf), Ok(&buf[..]));
        assert_eq!(buffer_body(&[0, 9, 0, 0], &buf), Err(Error::Short));
        assert_eq!(buffer_body(&[0, 4, 1, 0], &buf), Err(Error::BadWords));
        assert_eq!(buffer_body(&[0, MSIZE as u64 + 1, 0, 0], &buf), Err(Error::BadWords));
        let mut big = alloc::vec![0u8; MSIZE + 1];
        assert_eq!(encode_buffer(1, &mut big, |w| w.put(&[1; MSIZE + 1])), Err(Error::TooLarge));
        assert!(big.iter().all(|&b| b == 0), "a failed encode leaves nothing behind");
    }

    #[test]
    fn error_replies_carry_only_a_status() {
        assert_eq!(reply_status(&[0, 5, 6, 7], 2), Ok(None));
        assert_eq!(reply_status(&error_reply(3), 0), Ok(Some(3)));
        assert_eq!(reply_status(&[3, 0, 0, 1], 0), Err(Error::BadWords));
        assert_eq!(reply_status(&[3, 0, 0, 0], 1), Err(Error::BadHandles));
    }

    #[test]
    fn file_framing() {
        let layouts = [Layout { opcode: 9, inline: true, handles: 0 }, Layout { opcode: 10, inline: false, handles: 1 }];
        let mut out = [0u8; 8];
        let n = encode_file(&layouts[0], &mut out, |w| w.u16(0x0201)).unwrap();
        assert_eq!(out[..n], [9, 0, 0, 0, 1, 2]);
        let read = |_, r: &mut Reader<'_>| r.u16();
        assert_eq!(decode_file(&layouts, &out[..n], read), Ok(0x0201));
        assert_eq!(decode_file(&layouts, &out[..n - 1], read), Err(Error::Short));
        assert_eq!(decode_file(&layouts, &[9, 0, 0, 0, 1, 2, 3], read), Err(Error::Trailing));
        assert_eq!(decode_file(&layouts, &[8, 0, 0, 0], read), Err(Error::BadOpcode));
        assert_eq!(decode_file(&layouts, &[9, 0, 0], read), Err(Error::Short));
        // A message with handles cannot be written into a file.
        assert_eq!(decode_file(&layouts, &[10, 0, 0, 0, 1, 2], read), Err(Error::BadHandles));
        assert_eq!(encode_file(&layouts[1], &mut out, |w| w.u16(1)), Err(Error::BadHandles));
        // Too small an output: nothing written.
        let mut small = [0u8; 5];
        assert_eq!(encode_file(&layouts[0], &mut small, |w| w.u16(1)), Err(Error::TooLarge));
        assert_eq!(small, [0; 5]);
    }
}
