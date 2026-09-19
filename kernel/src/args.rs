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

impl KernelArgument {
    pub fn new(base: *const u32, offset: usize) -> Self {
        // SAFETY: `base` is the argument block and `offset` is a tag boundary within it
        // (the iterator only ever advances by whole tags). A tag is name, size, then data.
        unsafe {
            let name = base.add(offset / 4).read();
            let size = (base.add(offset / 4 + 1) as *const u16).add(1).read() as usize;
            let data = core::slice::from_raw_parts(base.add(offset / 4 + 2), size);
            KernelArgument { name, size: size * 4, data }
        }
    }
}

impl Iterator for KernelArgumentsIterator {
    type Item = KernelArgument;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.size {
            None
        } else {
            let new_arg = KernelArgument::new(self.base, self.offset);
            self.offset += new_arg.size + 8;
            Some(new_arg)
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
