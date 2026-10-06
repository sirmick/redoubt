//! The boot pack (docs/userland/beamlet.md, "beamlet on Redoubt"): the modules and application
//! resources the shell loads to its prompt, in one file at the root of the userland volume,
//! `boot.pack`, which the image's builder made from the same files it puts beside it. beamlet
//! reads it whole at start, in one sequential read, and its lookups take each name from it
//! before they ask the volume; a name it does not hold is read from the volume as before.
//!
//! The format, every integer little-endian:
//!
//! | Bytes | What |
//! | --- | --- |
//! | 4 | [`MAGIC`] |
//! | 4 | [`VERSION`] |
//! | 4 | the entry count |
//! | per entry | its name's length (2 bytes) and the name, the bytes' offset in the file (4) and their length (4) |
//! | the rest | each entry's bytes, in the index's order, with nothing between or after them |
//!
//! The index is sorted by name, each name once. The pack is on the verified volume, so its bytes
//! are the image builder's or none at all (R75 (verified userland)); it is checked whole at start
//! all the same, so a builder's mistake is one line naming it, never a module that loads wrong.

use alloc::vec::Vec;

use crate::userland::valid_name;

/// The pack's file at the root of the userland volume.
pub const FILE: &str = "boot.pack";
/// The pack's first four bytes.
pub const MAGIC: [u8; 4] = *b"RBPK";
/// The format above.
pub const VERSION: u32 = 1;

/// One entry: where its name and its bytes are in the pack.
struct Entry {
    name: (usize, usize),
    bytes: (usize, usize),
    taken: bool,
}

/// A pack read whole and checked: one allocation, which goes once every entry has been taken.
pub struct Pack {
    bytes: Vec<u8>,
    index: Vec<Entry>,
    /// Entries taken at least once: when every one has been, the pack is spent.
    taken: usize,
}

/// Reads `n` bytes at `at` of `bytes`, or says the pack ends first.
fn field<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
    let end = at.checked_add(n).filter(|&end| end <= bytes.len()).ok_or("its index is truncated")?;
    let field = &bytes[*at..end];
    *at = end;
    Ok(field)
}

fn word(bytes: &[u8], at: &mut usize) -> Result<usize, &'static str> {
    let w = field(bytes, at, 4)?;
    Ok(u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize)
}

impl Pack {
    /// `bytes`, checked: the magic and the version; every name a module's or a resource's file
    /// name, in strictly ascending order; the entries' bytes back to back, from the index's end to
    /// the file's; and each module's own name, its first atom, the one its entry says.
    pub fn parse(bytes: Vec<u8>) -> Result<Pack, &'static str> {
        let mut at = 0;
        if field(&bytes, &mut at, 4)? != MAGIC {
            return Err("it is not a boot pack");
        }
        if word(&bytes, &mut at)? != VERSION as usize {
            return Err("its version is not this beamlet's");
        }
        let count = word(&bytes, &mut at)?;
        // Each entry takes at least ten bytes of index: a count beyond that is no pack's.
        if count > bytes.len() / 10 {
            return Err("its entry count is larger than the file");
        }
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            let len = field(&bytes, &mut at, 2)?;
            let len = u16::from_le_bytes([len[0], len[1]]) as usize;
            let name = (at, len);
            let text = core::str::from_utf8(field(&bytes, &mut at, len)?)
                .map_err(|_| "an entry's name is not UTF-8")?;
            if !valid_name(text) {
                return Err("an entry's name is not a module's or a resource's");
            }
            if let Some(last) = index.last() {
                if name_of(&bytes, last) >= text {
                    return Err("its index is not in strictly ascending name order");
                }
            }
            let offset = word(&bytes, &mut at)?;
            let length = word(&bytes, &mut at)?;
            index.push(Entry { name, bytes: (offset, length), taken: false });
        }
        // The entries' bytes follow the index, one after another, and end the file.
        let mut next = at;
        for entry in &index {
            let (offset, length) = entry.bytes;
            if offset != next {
                return Err("an entry's bytes are not where the one before it ends");
            }
            next = offset
                .checked_add(length)
                .filter(|&end| end <= bytes.len())
                .ok_or("an entry runs past the file's end")?;
            let name = name_of(&bytes, entry);
            if let Some(module) = name.strip_suffix(".beam") {
                if module_name(&bytes[offset..next]) != Some(module) {
                    return Err("an entry's module is not the one its name says");
                }
            }
        }
        if next != bytes.len() {
            return Err("the file goes on past its last entry");
        }
        Ok(Pack { bytes, index, taken: 0 })
    }

    /// The entries.
    pub fn len(&self) -> usize { self.index.len() }

    pub fn is_empty(&self) -> bool { self.index.is_empty() }

    /// The pack's size in bytes.
    pub fn size(&self) -> usize { self.bytes.len() }

    fn find(&self, file: &str) -> Option<usize> {
        self.index.binary_search_by(|e| name_of(&self.bytes, e).cmp(file)).ok()
    }

    /// Whether the pack holds `file`.
    pub fn holds(&self, file: &str) -> bool { self.find(file).is_some() }

    /// A copy of `file`'s bytes, for the VM to decode, if the pack holds it.
    pub fn take(&mut self, file: &str) -> Option<Vec<u8>> {
        let i = self.find(file)?;
        let entry = &mut self.index[i];
        if !entry.taken {
            entry.taken = true;
            self.taken += 1;
        }
        let (offset, length) = entry.bytes;
        Some(self.bytes[offset..offset + length].to_vec())
    }

    /// Whether every entry has been taken: the pack has given the VM all it holds, and goes.
    pub fn spent(&self) -> bool { self.taken == self.index.len() }
}

fn name_of<'a>(bytes: &'a [u8], entry: &Entry) -> &'a str {
    let (at, len) = entry.name;
    // Checked as UTF-8 when the index was read.
    core::str::from_utf8(&bytes[at..at + len]).unwrap_or("")
}

/// A module's own name, the first atom of its `AtU8` chunk, as the VM's loader reads it: `None`
/// if `beam` is not a module the loader would take that far.
fn module_name(beam: &[u8]) -> Option<&str> {
    if beam.len() < 12 || &beam[0..4] != b"FOR1" || &beam[8..12] != b"BEAM" {
        return None;
    }
    let size = u32::from_be_bytes(beam[4..8].try_into().ok()?) as usize;
    let body = beam.get(8..8usize.checked_add(size)?)?;
    let mut pos = 4;
    while pos < body.len() {
        let id = body.get(pos..pos + 4)?;
        let len = u32::from_be_bytes(body.get(pos + 4..pos + 8)?.try_into().ok()?) as usize;
        let data = body.get(pos + 8..(pos + 8).checked_add(len)?)?;
        if id == b"AtU8" {
            // OTP 28's form: a negative count, then each atom's length as a compact `u` term.
            let count = i32::from_be_bytes(data.get(0..4)?.try_into().ok()?);
            let tag = *data.get(4)?;
            if count >= 0 || tag & 0x07 != 0 {
                return None;
            }
            let (len, at): (usize, usize) = match tag {
                t if t & 0x08 == 0 => (usize::from(t >> 4), 5),
                t if t & 0x10 == 0 => ((usize::from(t & 0xe0) << 3) | usize::from(*data.get(5)?), 6),
                _ => return None,
            };
            return core::str::from_utf8(data.get(at..at.checked_add(len)?)?).ok();
        }
        pos = (pos + 8 + len).checked_add(3)? & !3;
    }
    None
}
