//! The startup block (INIT.md): one page a parent writes for its child, naming the handles it
//! installed in the child's slots 1..=n (`process_start`). The parent may be hostile, so parsing
//! bounds everything, checks everything up front, and never panics.
//!
//! # Format
//!
//! The kernel argument block's tag format (BOOT.md, `loader/src/args.rs`): little-endian `u32`
//! words; each entry is a 4-byte ASCII tag, one word holding a CRC-16/X-25 of its data (low half)
//! and its data length in words (high half), then the data. INIT.md owns the format (answer
//! 39); the entries:
//!
//! | Tag | Data | Meaning |
//! | --- | --- | --- |
//! | `SBlk` | version (1), block length in words, handle count n | first, exactly once |
//! | `NmSp` | handle, byte length, path | namespace entry: a clean absolute path (`/`, `/dev/cons`) |
//! | `Hndl` | handle, byte length, name | a named handle: services (`keys`), devices, `budget` |
//! | `Argv` | byte length, argument | one argument, in order |
//!
//! Strings are UTF-8, zero-padded to a whole word, and the entry's length must be exactly what
//! its string needs. Handles are 1..=n, and n is at most `MAX_START_HANDLES` (what
//! `process_start` can install). Paths and names are unique. Nothing may follow the last
//! entry inside the block's length; the rest of the page is not read.

use alloc::vec::Vec;

use redoubt_sys::{Handle, MAX_START_HANDLES, PAGE_SIZE};

use crate::path;

/// The largest block: one page.
pub const MAX_BLOCK: usize = PAGE_SIZE;
const VERSION: u32 = 1;
const HEADER: [u8; 4] = *b"SBlk";
const NAMESPACE: [u8; 4] = *b"NmSp";
const HANDLE: [u8; 4] = *b"Hndl";
const ARG: [u8; 4] = *b"Argv";
/// The header entry's length in bytes: tag, CRC and length, three data words.
const HEADER_BYTES: usize = 20;

/// Why a startup block was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupError {
    /// An entry, or the block, runs past the bytes given.
    Short,
    /// The first entry is not the header, or a later one is, or a tag is unknown.
    BadTag,
    BadCrc,
    /// An entry's length disagrees with its contents, or the block's with its entries.
    BadLength,
    BadVersion,
    /// A handle outside 1..=n.
    BadHandle,
    /// A string that is not UTF-8, has non-zero padding, or is not a valid path or name.
    BadString,
    /// A path or name given twice.
    Duplicate,
    /// Longer than [`MAX_BLOCK`].
    TooLarge,
}

/// One entry, as parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry<'a> {
    Namespace(&'a str, Handle),
    Handle(&'a str, Handle),
    Arg(&'a str),
}

/// A parsed startup block: its entries, validated by [`Startup::parse`].
#[derive(Clone, Debug)]
pub struct Startup<'a> {
    entries: Vec<Entry<'a>>,
}

impl<'a> Startup<'a> {
    /// A process started with no block.
    pub const EMPTY: Startup<'static> = Startup { entries: Vec::new() };

    /// Parses the block at the front of `bytes` (at most [`MAX_BLOCK`] of them are looked at).
    pub fn parse(bytes: &'a [u8]) -> Result<Startup<'a>, StartupError> {
        let bytes = bytes.get(..MAX_BLOCK).unwrap_or(bytes);
        let mut rest = bytes;
        let (tag, data) = raw_entry(&mut rest)?;
        if tag != HEADER {
            return Err(StartupError::BadTag);
        }
        if data.len() != 12 {
            return Err(StartupError::BadLength);
        }
        let [version, words, handles] = [0, 4, 8].map(|at| word(data, at).unwrap_or(0));
        if version != VERSION {
            return Err(StartupError::BadVersion);
        }
        // `process_start` installs at most MAX_START_HANDLES handles, so no more can be named.
        if handles as usize > MAX_START_HANDLES {
            return Err(StartupError::BadHandle);
        }
        let len = usize::try_from(words).ok().and_then(|w| w.checked_mul(4)).ok_or(StartupError::TooLarge)?;
        if len > MAX_BLOCK {
            return Err(StartupError::TooLarge);
        }
        let mut rest = bytes.get(HEADER_BYTES..len).ok_or(if len < HEADER_BYTES {
            StartupError::BadLength
        } else {
            StartupError::Short
        })?;
        // Decode every entry once, and check that no path or name repeats (at most a few hundred
        // entries fit in a page, so the quadratic check is small).
        let mut entries: Vec<Entry<'a>> = Vec::new();
        while !rest.is_empty() {
            let entry = Self::entry(&mut rest, handles)?;
            let repeated = entries.iter().any(|old| match (old, &entry) {
                (Entry::Namespace(a, _), Entry::Namespace(b, _))
                | (Entry::Handle(a, _), Entry::Handle(b, _)) => a == b,
                _ => false,
            });
            if repeated {
                return Err(StartupError::Duplicate);
            }
            // A block holds a few hundred entries at most; no memory for them is a refusal.
            entries.try_reserve(1).map_err(|_| StartupError::TooLarge)?;
            entries.push(entry);
        }
        Ok(Startup { entries })
    }

    /// Decodes the entry at the front of `rest`; handles must be 1..=`handles`.
    fn entry(rest: &mut &'a [u8], handles: u32) -> Result<Entry<'a>, StartupError> {
        let (tag, data) = raw_entry(rest)?;
        let handle = |raw: u32| match Handle::new(raw) {
            Some(handle) if raw <= handles => Ok(handle),
            _ => Err(StartupError::BadHandle),
        };
        match tag {
            NAMESPACE => {
                let (raw, path) = handle_and_string(data)?;
                if !path::is_clean_absolute(path) {
                    return Err(StartupError::BadString);
                }
                Ok(Entry::Namespace(path, handle(raw)?))
            }
            HANDLE => {
                let (raw, name) = handle_and_string(data)?;
                if name.is_empty() || name.contains('\0') {
                    return Err(StartupError::BadString);
                }
                Ok(Entry::Handle(name, handle(raw)?))
            }
            ARG => Ok(Entry::Arg(string(data)?)),
            _ => Err(StartupError::BadTag),
        }
    }

    fn entries(&self) -> impl Iterator<Item = Entry<'a>> + '_ { self.entries.iter().copied() }

    /// The namespace table: (path, handle), in block order.
    pub fn namespace(&self) -> impl Iterator<Item = (&'a str, Handle)> + '_ {
        self.entries().filter_map(|e| if let Entry::Namespace(path, h) = e { Some((path, h)) } else { None })
    }

    /// The handle named `name`.
    pub fn handle(&self, name: &str) -> Option<Handle> {
        self.entries().find_map(|e| match e {
            Entry::Handle(n, h) if n == name => Some(h),
            _ => None,
        })
    }

    /// The arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.entries().filter_map(|e| if let Entry::Arg(arg) = e { Some(arg) } else { None })
    }

    /// Resolves the clean absolute `path` against the namespace table: the entry with the
    /// longest matching prefix, and what is left of `path` after it (no leading `/`).
    pub fn resolve<'p>(&self, path: &'p str) -> Option<(Handle, &'p str)> {
        let mut best: Option<(usize, Handle, &'p str)> = None;
        for (prefix, handle) in self.namespace() {
            let rest = if prefix == "/" {
                path.strip_prefix('/')
            } else {
                path.strip_prefix(prefix)
                    .and_then(|r| if r.is_empty() { Some(r) } else { r.strip_prefix('/') })
            };
            if let Some(rest) = rest {
                if best.is_none_or(|(len, _, _)| prefix.len() > len) {
                    best = Some((prefix.len(), handle, rest));
                }
            }
        }
        best.map(|(_, handle, rest)| (handle, rest))
    }
}

/// Reads one entry's tag and data words from the front of `rest`, checking its CRC.
fn raw_entry<'a>(rest: &mut &'a [u8]) -> Result<([u8; 4], &'a [u8]), StartupError> {
    let tag: [u8; 4] = rest.get(..4).and_then(|t| t.try_into().ok()).ok_or(StartupError::Short)?;
    let info = word(rest, 4).ok_or(StartupError::Short)?;
    let len = (info >> 16) as usize * 4;
    let data = rest.get(8..8 + len).ok_or(StartupError::Short)?;
    if crc16(data) != info as u16 {
        return Err(StartupError::BadCrc);
    }
    *rest = rest.get(8 + len..).ok_or(StartupError::Short)?;
    Ok((tag, data))
}

/// The little-endian word at byte `at`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(at..end)?.try_into().ok()?))
}

/// A handle word followed by a string.
fn handle_and_string(data: &[u8]) -> Result<(u32, &str), StartupError> {
    let raw = word(data, 0).ok_or(StartupError::BadLength)?;
    Ok((raw, string(data.get(4..).unwrap_or(&[]))?))
}

/// A length word and a zero-padded UTF-8 string that fills the rest of the data exactly.
fn string(data: &[u8]) -> Result<&str, StartupError> {
    let len = word(data, 0).ok_or(StartupError::BadLength)?;
    let padded = data.get(4..).unwrap_or(&[]);
    if padded_len(len).and_then(|p| usize::try_from(p).ok()) != Some(padded.len()) {
        return Err(StartupError::BadLength);
    }
    // In range: `len` <= its padded length, which is `padded.len()`.
    let (text, padding) = padded.split_at(len as usize);
    if padding.iter().any(|b| *b != 0) {
        return Err(StartupError::BadString);
    }
    core::str::from_utf8(text).map_err(|_| StartupError::BadString)
}

/// A string of `len` bytes padded to whole words, in 32-bit arithmetic on both widths (the
/// length word is 32 bits); `None` if that overflows, as it would on rv32 near `u32::MAX`.
fn padded_len(len: u32) -> Option<u32> { len.checked_next_multiple_of(4) }

/// CRC-16/X-25 (reflected polynomial 0x8408, initial and final value 0xffff): the kernel
/// argument block's checksum (the loader's `CRC_16_IBM_SDLC`).
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for byte in data {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x8408 } else { crc >> 1 };
        }
    }
    !crc
}

/// Writes a startup block, for launchers (`init`, the steward) and tests. [`finish`] parses the
/// result, so a builder can only produce blocks the parser accepts.
///
/// **A launcher never passes its own connection to a child** (answer 50): every handle named in a
/// child's namespace is a fresh connection the server made for that child. A copied connection
/// would share the launcher's fids and admission with the child.
///
/// [`finish`]: StartupBuilder::finish
pub struct StartupBuilder {
    bytes: Vec<u8>,
    handles: u32,
}

impl StartupBuilder {
    /// A block for a child given `handles` handles (its slots 1..=handles).
    pub fn new(handles: u32) -> StartupBuilder {
        StartupBuilder { bytes: alloc::vec![0; HEADER_BYTES], handles }
    }

    fn entry(&mut self, tag: [u8; 4], words: &[u32], text: &str) -> &mut Self {
        let mut data = Vec::new();
        for w in words {
            data.extend_from_slice(&w.to_le_bytes());
        }
        // A string longer than a page cannot fit; its length word saturates and `finish` refuses.
        data.extend_from_slice(&u32::try_from(text.len()).unwrap_or(u32::MAX).to_le_bytes());
        data.extend_from_slice(text.as_bytes());
        data.resize(data.len().div_ceil(4) * 4, 0);
        self.push(tag, &data);
        self
    }

    fn push(&mut self, tag: [u8; 4], data: &[u8]) {
        let words = u32::try_from(data.len() / 4).unwrap_or(u32::MAX).min(0xffff);
        self.bytes.extend_from_slice(&tag);
        self.bytes.extend_from_slice(&(u32::from(crc16(data)) | words << 16).to_le_bytes());
        self.bytes.extend_from_slice(data);
    }

    pub fn namespace(&mut self, path: &str, handle: Handle) -> &mut Self {
        self.entry(NAMESPACE, &[handle.index()], path)
    }

    pub fn handle(&mut self, name: &str, handle: Handle) -> &mut Self {
        self.entry(HANDLE, &[handle.index()], name)
    }

    pub fn arg(&mut self, arg: &str) -> &mut Self { self.entry(ARG, &[], arg) }

    /// The block's bytes, checked by [`Startup::parse`].
    pub fn finish(&self) -> Result<Vec<u8>, StartupError> {
        if self.bytes.len() > MAX_BLOCK {
            return Err(StartupError::TooLarge);
        }
        // The length fits: at most MAX_BLOCK / 4 words.
        let header = [VERSION, (self.bytes.len() / 4) as u32, self.handles].map(u32::to_le_bytes).concat();
        let mut block = StartupBuilder { bytes: Vec::new(), handles: 0 };
        block.push(HEADER, &header);
        block.bytes.extend_from_slice(&self.bytes[HEADER_BYTES..]);
        Startup::parse(&block.bytes)?;
        Ok(block.bytes)
    }
}

#[cfg(test)]
mod tests {

    use alloc::vec;

    use super::*;

    fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

    fn sample() -> Vec<u8> {
        StartupBuilder::new(4)
            .namespace("/", h(1))
            .namespace("/dev/cons", h(2))
            .handle("keys", h(3))
            .handle("budget", h(4))
            .arg("--verbose")
            .arg("")
            .arg("naïve")
            .finish()
            .unwrap()
    }

    #[test]
    fn crc_matches_x25() { assert_eq!(crc16(b"123456789"), 0x906e) }

    #[test]
    fn round_trip() {
        let bytes = sample();
        let s = Startup::parse(&bytes).unwrap();
        assert_eq!(s.namespace().collect::<Vec<_>>(), vec![("/", h(1)), ("/dev/cons", h(2))]);
        assert_eq!(s.handle("keys"), Some(h(3)));
        assert_eq!(s.handle("budget"), Some(h(4)));
        assert_eq!(s.handle("nope"), None);
        assert_eq!(s.args().collect::<Vec<_>>(), vec!["--verbose", "", "naïve"]);
        // The rest of the page is not read.
        let mut page = bytes.clone();
        page.resize(MAX_BLOCK, 0xaa);
        assert_eq!(Startup::parse(&page).unwrap().args().count(), 3);
    }

    #[test]
    fn resolve_takes_the_longest_prefix() {
        let bytes = sample();
        let s = Startup::parse(&bytes).unwrap();
        assert_eq!(s.resolve("/dev/cons"), Some((h(2), "")));
        assert_eq!(s.resolve("/dev/consx"), Some((h(1), "dev/consx")));
        assert_eq!(s.resolve("/dev/cons/x"), Some((h(2), "x")));
        assert_eq!(s.resolve("/home/a"), Some((h(1), "home/a")));
        assert_eq!(s.resolve("relative"), None);
        assert_eq!(Startup::EMPTY.resolve("/"), None);
    }

    #[test]
    fn hostile_blocks_are_refused() {
        let good = sample();
        let reject = |bytes: &[u8]| Startup::parse(bytes).err();
        assert_eq!(reject(&[]), Some(StartupError::Short));
        assert_eq!(reject(&good[..good.len() - 1]), Some(StartupError::Short));
        // Any flipped bit in the block is caught: the CRC, a length, a tag, or a string rule.
        for at in 0..good.len() {
            for bit in 0..8 {
                let mut bad = good.clone();
                bad[at] ^= 1 << bit;
                assert!(Startup::parse(&bad).is_err(), "bit {bit} of byte {at} went unnoticed");
            }
        }
        // Each builder step the parser must refuse, and why.
        type Case<'a> = (&'a dyn Fn(&mut StartupBuilder), Option<StartupError>);
        let cases: [Case; 9] = [
            (&|s| _ = s.namespace("/", h(3)), Some(StartupError::BadHandle)),
            (&|s| _ = s.namespace("/a/../b", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.namespace("dev", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("a\0", h(1)), Some(StartupError::BadString)),
            (&|s| _ = s.handle("x", h(1)).handle("x", h(2)), Some(StartupError::Duplicate)),
            (&|s| _ = s.namespace("/", h(1)).namespace("/", h(2)), Some(StartupError::Duplicate)),
            (&|s| _ = s.arg(&"x".repeat(MAX_BLOCK)), Some(StartupError::TooLarge)),
            // A name and a path may be spelled alike: different tables.
            (&|s| _ = s.namespace("/", h(1)).handle("/", h(2)), None),
        ];
        for (i, (build, expected)) in cases.iter().enumerate() {
            let mut builder = StartupBuilder::new(2);
            build(&mut builder);
            assert_eq!(builder.finish().err(), *expected, "case {i}");
        }
        // An entry after the block's stated length is not part of it; one the length cuts is short.
        let mut cut = good.clone();
        cut.truncate(good.len() - 4);
        assert_eq!(reject(&cut), Some(StartupError::Short));
    }

    #[test]
    fn handle_counts_are_what_process_start_can_install() {
        let most = MAX_START_HANDLES as u32;
        assert!(StartupBuilder::new(most).handle("last", h(most)).finish().is_ok());
        assert_eq!(StartupBuilder::new(most + 1).finish().err(), Some(StartupError::BadHandle));
        assert_eq!(StartupBuilder::new(u32::MAX).finish().err(), Some(StartupError::BadHandle));
    }

    #[test]
    fn string_lengths_use_checked_32_bit_arithmetic() {
        // The length word is 32 bits; rounding it up to whole words must not wrap, as it would
        // in rv32's `usize` (0xffff_ffff rounded up wraps to 0, matching an empty tail).
        assert_eq!(padded_len(0), Some(0));
        assert_eq!(padded_len(5), Some(8));
        assert_eq!(padded_len(u32::MAX - 3), Some(u32::MAX - 3));
        assert_eq!(padded_len(u32::MAX - 2), None);
        assert_eq!(padded_len(u32::MAX), None);
        // The attack itself: an argument claiming 0xffff_ffff bytes with none present.
        let mut builder = StartupBuilder::new(0);
        builder.push(ARG, &u32::MAX.to_le_bytes());
        assert_eq!(builder.finish().err(), Some(StartupError::BadLength));
    }

    #[test]
    fn random_bytes_never_panic() {
        // A tiny xorshift: deterministic, no dependency.
        let mut x = 0x2545_f491_4f6c_dd1d_u64;
        let good = sample();
        for round in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut bytes = if round % 2 == 0 { good.clone() } else { vec![0; (x % 256) as usize] };
            for (i, byte) in bytes.iter_mut().enumerate() {
                if (x >> (i % 64)) & 7 == 0 {
                    *byte = (x >> 8) as u8 ^ i as u8;
                }
            }
            let _ = Startup::parse(&bytes);
        }
    }
}
