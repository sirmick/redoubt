// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! The kernel argument block, produced by the loader (see `docs/BOOT.md`).
//! `init` records its address; everything else reads through it. The reads are `unsafe`
//! because they dereference that loader-provided pointer, sound as long as `init` was
//! given the real argument block, which is the loader's contract.

use core::convert::TryFrom;
use core::fmt;

use crate::cell::KernelCell;

/// Address of the argument block, as a `usize` (a raw pointer is not `Sync`). Set once by
/// `init`, read-only thereafter.
static KERNEL_ARGUMENTS_BASE: KernelCell<usize> = KernelCell::new(0);

pub struct KernelArguments {
    pub base: *const u32,
}

pub struct KernelArgumentsIterator {
    base: *const u32,
    size: usize,
    offset: usize,
}

#[allow(dead_code)]
impl KernelArguments {
    pub fn get() -> Self { KernelArguments { base: KERNEL_ARGUMENTS_BASE.with(|b| *b) as *const u32 } }

    /// # Safety
    /// `base` must point at the argument block the loader built.
    pub unsafe fn init(base: *const u32) { KERNEL_ARGUMENTS_BASE.with(|b| *b = base as usize); }

    pub fn iter(&self) -> KernelArgumentsIterator {
        KernelArgumentsIterator { base: self.base, size: self.size(), offset: 0 }
    }

    /// Get the size of the entire kernel argument structure
    pub fn size(&self) -> usize {
        // SAFETY: `self.base` is the argument block; word 2 is its total size in words.
        unsafe { self.base.add(2).read() as usize * 4 }
    }
}

pub struct KernelArgument {
    pub name: u32,
    pub size: usize,
    pub data: &'static [u32],
}

/// The 64-bit value in `words[index..index + 2]` (low word first), narrowed to a `usize`.
///
/// The loader is one binary for both widths, so every address and size in the block is two
/// words wide. Rebuilding through `u64` avoids a `<< 32` that overflows a 32-bit `usize`, and
/// the conversion is checked: on rv32 the high word is zero for everything a 32-bit machine
/// can address, so a value that does not fit means the loader and the kernel disagree about
/// the machine, and the boot stops here instead of running on a truncated address.
pub fn wide(words: &[u32], index: usize) -> usize {
    let value = words[index] as u64 | (words[index + 1] as u64) << 32;
    usize::try_from(value).expect("args: a 64-bit value in the argument block does not fit a usize")
}

impl Iterator for KernelArgumentsIterator {
    type Item = KernelArgument;

    /// Reads the tag at the current offset: name, then size (the upper half of the second
    /// word, counted in words), then that many data words.
    fn next(&mut self) -> Option<Self::Item> {
        // A tag is its two header words and its data; a header that does not fit ends the block.
        if self.offset + 8 > self.size {
            return None;
        }
        let words_left = (self.size - self.offset - 8) / 4;
        // SAFETY: `self.base` is the argument block the loader built and `self.offset` a tag
        // boundary within it: this is the only place the offset moves, and it moves by whole
        // tags. The header therefore lies in the block, and the data slice does too, because
        // the assertion keeps its length within the `words_left` words the block still has.
        // The block is word-aligned, initialised and lives as long as the kernel.
        unsafe {
            let name = self.base.add(self.offset / 4).read();
            // The second word is the CRC in its low half and the data's length, in words,
            // in its high half.
            let size = (self.base.add(self.offset / 4 + 1).read() >> 16) as usize;
            assert!(size <= words_left, "args: a tag's data runs past the end of the argument block");
            let data = core::slice::from_raw_parts(self.base.add(self.offset / 4 + 2), size);
            self.offset += size * 4 + 8;
            Some(KernelArgument { name, size: size * 4, data })
        }
    }
}

impl fmt::Display for KernelArgument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag_name_bytes = self.name.to_le_bytes();
        // A tag name is four ASCII bytes; anything else is a block the kernel cannot read
        // anyway, and this is a debug print, so it says so rather than failing.
        let s = core::str::from_utf8(&tag_name_bytes).unwrap_or("????");

        write!(f, "{} ({:08x}, {} bytes):", s, self.name, self.size)?;
        for word in self.data {
            write!(f, " {:08x}", word)?;
        }
        Ok(())
    }
}
