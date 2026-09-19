// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! The kernel argument block, produced by the loader (see `planning/redoubt/BOOT.md`).
//! `init` records its address; everything else reads through it. The reads are `unsafe`
//! because they dereference that loader-provided pointer, sound as long as `init` was
//! given the real argument block, which is the loader's contract.

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
            let size = (self.base.add(self.offset / 4 + 1) as *const u16).add(1).read() as usize;
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
        // SAFETY: `tag_name_bytes` is a live 4-byte array; the tag name is ASCII by
        // construction, so treating it as UTF-8 is valid.
        let s = unsafe {
            use core::slice;
            use core::str;
            let slice = slice::from_raw_parts(tag_name_bytes.as_ptr(), 4);
            str::from_utf8_unchecked(slice)
        };

        write!(f, "{} ({:08x}, {} bytes):", s, self.name, self.size)?;
        for word in self.data {
            write!(f, " {:08x}", word)?;
        }
        Ok(())
    }
}
