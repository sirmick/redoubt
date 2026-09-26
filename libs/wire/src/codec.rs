//! 9P's encoding (servers/wire.md), shared by 9P and the typed messages: little-endian `u8`,
//! `u16`, `u32`, `u64`; strings as a `u16` length and UTF-8; byte arrays as a `u32` length
//! and the bytes.
//!
//! [`Reader`] hands out borrowed views of its input and never reads past it: a length
//! field can claim anything, but the bytes it names must be present, so every length is
//! bounded by the buffer. [`Writer`] fills a caller-supplied buffer and refuses to run
//! past its end.

/// Why a message was refused. One enum for 9P and typed messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The input ended inside a field, or a length names bytes that are not there.
    Short,
    /// Bytes are left over after the last field.
    Trailing,
    /// A string is not UTF-8.
    BadUtf8,
    /// The output buffer is too small, or a value is too long for its length field or for
    /// `msize`.
    TooLarge,
    /// A 9P size field is out of range or disagrees with the bytes it frames.
    BadSize,
    /// An unknown 9P message type, or a message type that never travels (`Terror`).
    BadType,
    /// An opcode the protocol does not define.
    BadOpcode,
    /// A message word does not fit in 32 bits, or a word the layout does not use is not zero.
    BadWords,
    /// An inline message arrived with a buffer.
    UnexpectedBuffer,
    /// The message carries a different number of handles than its layout names (or has
    /// handles where none can travel: an error reply, a message written into a file).
    BadHandles,
    /// An error reply whose status is not in the protocol's error table.
    BadStatus,
    /// A 9P walk with more than `MAXWELEM` (16) names or qids.
    TooManyElements,
}

/// Reads fields from the front of a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    /// The next `n` bytes, or `Short` if the input has fewer.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Short)?;
        let bytes = self.buf.get(self.pos..end).ok_or(Error::Short)?;
        self.pos = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| Error::Short)
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// A `u16` length and that many bytes of UTF-8.
    pub fn string(&mut self) -> Result<&'a str, Error> {
        let len = usize::from(self.u16()?);
        core::str::from_utf8(self.take(len)?).map_err(|_| Error::BadUtf8)
    }

    /// A `u32` length and that many bytes.
    pub fn bytes(&mut self) -> Result<&'a [u8], Error> {
        // A length that does not fit in usize cannot be present in the buffer either.
        let len = usize::try_from(self.u32()?).map_err(|_| Error::Short)?;
        self.take(len)
    }

    /// The bytes not yet read.
    pub fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or(&[])
    }

    /// Succeeds only if every byte was read: an encoding has no slack.
    pub fn finish(&self) -> Result<(), Error> {
        if self.rest().is_empty() { Ok(()) } else { Err(Error::Trailing) }
    }

    /// Succeeds only if every byte left is zero: the padding of an inline typed message.
    pub fn finish_padding(&self) -> Result<(), Error> {
        if self.rest().iter().all(|&b| b == 0) { Ok(()) } else { Err(Error::Trailing) }
    }
}

/// Writes fields into a caller-supplied buffer.
#[derive(Debug)]
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Writer { buf, pos: 0 }
    }

    /// Bytes written so far.
    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn put(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let end = self.pos.checked_add(bytes.len()).ok_or(Error::TooLarge)?;
        self.buf.get_mut(self.pos..end).ok_or(Error::TooLarge)?.copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }

    pub fn u8(&mut self, v: u8) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }

    pub fn u16(&mut self, v: u16) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }

    pub fn u32(&mut self, v: u32) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }

    pub fn u64(&mut self, v: u64) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }

    pub fn string(&mut self, s: &str) -> Result<(), Error> {
        self.u16(u16::try_from(s.len()).map_err(|_| Error::TooLarge)?)?;
        self.put(s.as_bytes())
    }

    pub fn bytes(&mut self, b: &[u8]) -> Result<(), Error> {
        self.u32(u32::try_from(b.len()).map_err(|_| Error::TooLarge)?)?;
        self.put(b)
    }

    /// Overwrites bytes written earlier at `at` (a size field known only at the end).
    pub fn patch(&mut self, at: usize, bytes: &[u8]) -> Result<(), Error> {
        let end = at.checked_add(bytes.len()).filter(|end| *end <= self.pos).ok_or(Error::TooLarge)?;
        self.buf.get_mut(at..end).ok_or(Error::TooLarge)?.copy_from_slice(bytes);
        Ok(())
    }

    /// Runs `write` as one unit: if it fails, the writer is put back where it was and the
    /// bytes it wrote are zeroed, so a failed write never leaves half an entry for a
    /// caller to send (a directory read that fills a buffer until an entry does not fit).
    pub fn atomic<T>(&mut self, write: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let start = self.pos;
        let result = write(self);
        if result.is_err() {
            if let Some(partial) = self.buf.get_mut(start..self.pos) {
                partial.fill(0);
            }
            self.pos = start;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_little_endian() {
        let mut buf = [0u8; 15];
        let mut w = Writer::new(&mut buf);
        w.u8(1).unwrap();
        w.u16(0x0302).unwrap();
        w.u32(0x0706_0504).unwrap();
        w.u64(0x0f0e_0d0c_0b0a_0908).unwrap();
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        let mut r = Reader::new(&buf);
        assert_eq!(r.u8(), Ok(1));
        assert_eq!(r.u16(), Ok(0x0302));
        assert_eq!(r.u32(), Ok(0x0706_0504));
        assert_eq!(r.u64(), Ok(0x0f0e_0d0c_0b0a_0908));
        assert_eq!(r.finish(), Ok(()));
    }

    #[test]
    fn lengths_are_bounded_by_the_input() {
        // A string claiming 65535 bytes with 1 present, and a byte array claiming 4 GiB.
        assert_eq!(Reader::new(&[0xff, 0xff, b'a']).string(), Err(Error::Short));
        assert_eq!(Reader::new(&[0xff, 0xff, 0xff, 0xff, 1]).bytes(), Err(Error::Short));
        assert_eq!(Reader::new(&[1, 0, 0xff]).string(), Err(Error::BadUtf8));
        assert_eq!(Reader::new(&[1]).u16(), Err(Error::Short));
    }

    #[test]
    fn writer_refuses_overflow() {
        let mut buf = [0u8; 3];
        let mut w = Writer::new(&mut buf);
        assert_eq!(w.string("ab"), Err(Error::TooLarge));
        assert_eq!(w.patch(1, &[0; 4]), Err(Error::TooLarge));
        let long = [b'a'; 70_000];
        let mut big = [0u8; 80_000];
        let s = core::str::from_utf8(&long).unwrap();
        assert_eq!(Writer::new(&mut big).string(s), Err(Error::TooLarge));
    }

    #[test]
    fn atomic_writes_leave_nothing_on_failure() {
        let mut buf = [0u8; 6];
        let mut w = Writer::new(&mut buf);
        w.u16(0x0101).unwrap();
        assert_eq!(w.atomic(|w| { w.u16(0x0202)?; w.u32(0x0303_0303) }), Err(Error::TooLarge));
        assert_eq!(w.position(), 2);
        assert_eq!(w.atomic(|w| w.u16(0x0404)), Ok(()));
        // Patching past what was written is refused, even inside the buffer.
        assert_eq!(w.patch(3, &[9, 9]), Err(Error::TooLarge));
        assert_eq!(buf, [1, 1, 4, 4, 0, 0]);
    }

    #[test]
    fn trailing_and_padding() {
        let mut r = Reader::new(&[1, 0, 0]);
        r.u8().unwrap();
        assert_eq!(r.finish(), Err(Error::Trailing));
        assert_eq!(r.finish_padding(), Ok(()));
        let mut r = Reader::new(&[1, 0, 2]);
        r.u8().unwrap();
        assert_eq!(r.finish_padding(), Err(Error::Trailing));
    }
}
