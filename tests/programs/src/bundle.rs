//! The verified bundle, as the program in `init`'s place finds it (kernel/boot.md, "The loader
//! loads only the kernel and `init`"): mapped read-only, its address in the first thread's `a0`
//! and its length in `a1`. It is `signature (64 bytes) || tar`, and the loader verified it
//! before it parsed a byte of it. This reads the archive's entries from there, within that
//! length, and refuses (`None`) a header it cannot read rather than guess.
//!
//! It reads ustar itself rather than through the loader's `tar-no-std`: the cases that read the
//! bundle back (`bundle-mapped`, `bench-bundle-file`) check what the loader did, and a reader
//! sharing the loader's parser would share its mistakes.

/// The signature before the archive (kernel/boot.md, "Verified boot").
const SIGNATURE_LEN: usize = 64;
/// A ustar header, and the unit the archive's data is padded to.
const BLOCK: usize = 512;

/// One archive entry: its name and its data, both inside the bundle.
#[derive(Clone, Copy)]
pub struct Entry {
    pub name: &'static [u8],
    pub data: &'static [u8],
}

/// The bundle's archive.
#[derive(Clone, Copy)]
pub struct Bundle {
    archive: &'static [u8],
}

impl Bundle {
    /// The bundle at `addr`, `len` bytes: what the first thread found in `a0` and `a1`.
    ///
    /// # Safety
    /// `addr..addr + len` must be readable for the rest of the program and never written: the
    /// loader's mapping of the bundle is.
    pub unsafe fn at(addr: usize, len: usize) -> Option<Bundle> {
        // SAFETY: the caller's promise.
        let initrd: &'static [u8] = unsafe { core::slice::from_raw_parts(addr as *const u8, len) };
        Some(Bundle { archive: initrd.get(SIGNATURE_LEN..)? })
    }

    /// The entries, in archive order.
    pub fn entries(&self) -> Entries { Entries { archive: self.archive, at: 0 } }

    /// The first entry named `name`.
    pub fn find(&self, name: &[u8]) -> Option<Entry> { self.entries().find(|e| e.name == name) }
}

pub struct Entries {
    archive: &'static [u8],
    at: usize,
}

impl Iterator for Entries {
    type Item = Entry;

    /// The entry whose header starts here; `None` at the end of the archive (a zero block) or at
    /// a header this cannot read.
    fn next(&mut self) -> Option<Entry> {
        let header = self.archive.get(self.at..self.at.checked_add(BLOCK)?)?;
        if header.iter().all(|b| *b == 0) {
            return None;
        }
        let name = &header[..100];
        let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(100)];
        // The size: octal digits, ended by a space or a NUL.
        let mut size = 0usize;
        for b in header[124..136].iter().take_while(|b| (b'0'..=b'7').contains(b)) {
            size = size.checked_mul(8)?.checked_add(usize::from(b - b'0'))?;
        }
        let start = self.at + BLOCK;
        let data = self.archive.get(start..start.checked_add(size)?)?;
        self.at = start + size.next_multiple_of(BLOCK);
        Some(Entry { name, data })
    }
}
