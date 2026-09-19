//! Framing for typed messages (WIRE.md): everything that is not 9P uses 9P's encoding
//! ([`crate::codec`]), laid out by a table in the owning server's note. The per-protocol
//! codecs in [`crate::proto`] are generated from those tables by `redoubt-wire-gen`; this
//! module is the part they share.
//!
//! A message is `WORDS` (4) machine words, up to 4 handles, and at most one buffer
//! (KERNEL-SPEC.md, Messages). **Word 0 is the opcode.** Each message type is one of two
//! shapes, fixed by its table:
//!
//! - **inline**: every field is a fixed-size integer and their encoding fits in
//!   [`INLINE_BYTES`]; the bytes are packed into words 1..=3, four per word, little-endian,
//!   zero-padded; there is no buffer.
//! - **buffer**: the encoding of the fields goes in the buffer and word 1 holds its length;
//!   words 2 and 3 are zero.
//!
//! Words are passed as `u64` so the same code serves both widths (an rv32 word widens
//! losslessly). Every word this module produces fits in 32 bits, and every word it accepts
//! must, so a layout is the same on rv32 and rv64: an inline message carries at most 12
//! bytes, the capacity of three 32-bit words. Handles travel in the message's handle slots;
//! the codec does not see them, but checks their count against the layout.

use crate::codec::{Error, Writer};
use crate::MSIZE;

/// Machine words in a message (KERNEL-SPEC.md `WORDS`).
pub const WORDS: usize = 4;
/// Handle slots in a message (KERNEL-SPEC.md `MAX_MSG_HANDLES`).
pub const MAX_MSG_HANDLES: usize = 4;
/// Bytes an inline message carries: words 1..=3 at 32 bits each (see the module docs).
pub const INLINE_BYTES: usize = 12;

/// A message's words, widened to `u64`.
pub type Words = [u64; WORDS];

/// One decoded field, for code that walks a message generically (logging, the test
/// vectors). Handles are not fields; they are in the message's handle slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    Str(&'a str),
    Bytes(&'a [u8]),
}

/// The opcode in word 0.
pub fn opcode(words: &Words) -> Result<u32, Error> {
    u32::try_from(words[0]).map_err(|_| Error::BadWords)
}

/// Encodes an inline message: `body` writes the fields, which the generator has checked
/// fit in [`INLINE_BYTES`] (a longer body fails with `TooLarge`, it is never truncated).
pub fn encode_inline(
    opcode: u32,
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<Words, Error> {
    let mut bytes = [0u8; INLINE_BYTES];
    body(&mut Writer::new(&mut bytes))?;
    let mut words = [u64::from(opcode), 0, 0, 0];
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
pub fn encode_buffer(
    opcode: u32,
    buf: &mut [u8],
    body: impl FnOnce(&mut Writer<'_>) -> Result<(), Error>,
) -> Result<Words, Error> {
    let mut w = Writer::new(buf);
    body(&mut w)?;
    let len = w.position();
    if len > MSIZE {
        return Err(Error::TooLarge);
    }
    let len = u32::try_from(len).map_err(|_| Error::TooLarge)?;
    Ok([u64::from(opcode), u64::from(len), 0, 0])
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
    }

    #[test]
    fn buffer_length_is_bounded() {
        let buf = [0u8; 8];
        assert_eq!(buffer_body(&[0, 8, 0, 0], &buf), Ok(&buf[..]));
        assert_eq!(buffer_body(&[0, 9, 0, 0], &buf), Err(Error::Short));
        assert_eq!(buffer_body(&[0, 4, 1, 0], &buf), Err(Error::BadWords));
        assert_eq!(buffer_body(&[0, MSIZE as u64 + 1, 0, 0], &buf), Err(Error::BadWords));
        let mut big = alloc::vec![0u8; MSIZE + 1];
        assert_eq!(encode_buffer(1, &mut big, |w| w.put(&[0; MSIZE + 1])), Err(Error::TooLarge));
    }
}
