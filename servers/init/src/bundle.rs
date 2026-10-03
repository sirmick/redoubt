//! The verified bundle as `init` finds it (kernel/boot.md, "The loader loads only the kernel and
//! `init`"): `signature (64 bytes) || tar`, mapped read-only, its address and length in the first
//! thread's `a0` and `a1`. The loader verified the signature before it parsed a byte, so these
//! bytes are the signer's; this still reads them within their length and refuses (`None`) a header
//! it cannot read rather than guess.
//!
//! The archive holds the kernel, then `init`, then the manifest and the servers' images, by entry
//! name ([`Bundle::after_init`]).

/// The signature before the archive (kernel/boot.md, "Verified boot").
pub const SIGNATURE_LEN: usize = 64;
/// A ustar header, and the unit the archive's data is padded to.
const BLOCK: usize = 512;
/// The archive's entries before the ones `init` reads: the kernel's and its own.
const BOOT_ENTRIES: usize = 2;

/// One archive entry: its name and its data, both inside the bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}

/// The bundle's archive.
#[derive(Clone, Copy, Debug)]
pub struct Bundle<'a> {
    archive: &'a [u8],
}

impl<'a> Bundle<'a> {
    /// The bundle in `initrd`, the loader's mapping: `None` if it is too short to hold the
    /// signature.
    pub fn new(initrd: &'a [u8]) -> Option<Bundle<'a>> {
        Some(Bundle { archive: initrd.get(SIGNATURE_LEN..)? })
    }

    /// Every entry after the kernel's and `init`'s, in archive order, or `None` if a header does
    /// not read: the boot refuses a bundle it cannot read whole rather than start part of it.
    pub fn after_init(&self) -> Option<alloc::vec::Vec<Entry<'a>>> {
        let mut entries = alloc::vec::Vec::new();
        let mut at = 0usize;
        loop {
            let header = self.archive.get(at..at.checked_add(BLOCK)?)?;
            if header.iter().all(|b| *b == 0) {
                break;
            }
            let name = &header[..100];
            let name =
                core::str::from_utf8(&name[..name.iter().position(|b| *b == 0).unwrap_or(100)]).ok()?;
            // The size: octal digits, then a space or a NUL, and nothing else.
            let field = &header[124..136];
            let digits = field.iter().take_while(|b| (b'0'..=b'7').contains(b)).count();
            if digits == 0 || !field[digits..].iter().all(|b| *b == b' ' || *b == 0) {
                return None;
            }
            let size = field[..digits]
                .iter()
                .try_fold(0usize, |size, b| size.checked_mul(8)?.checked_add(usize::from(b - b'0')))?;
            let start = at + BLOCK;
            let data = self.archive.get(start..start.checked_add(size)?)?;
            entries.push(Entry { name, data });
            at = start.checked_add(size.checked_next_multiple_of(BLOCK)?)?;
        }
        (entries.len() >= BOOT_ENTRIES).then(|| entries.split_off(BOOT_ENTRIES))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::*;

    /// A ustar entry as the bench's builder writes it: a 512-byte header with the name and an
    /// octal size, then the data padded to a block.
    fn entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = std::format!("{:011o}\0", data.len());
        header[124..136].copy_from_slice(size.as_bytes());
        let mut out = header.to_vec();
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(BLOCK), 0);
        out
    }

    fn initrd(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::vec![0xaa; SIGNATURE_LEN];
        for (name, data) in entries {
            out.extend(entry(name, data));
        }
        out.extend([0; 2 * BLOCK]);
        out
    }

    #[test]
    fn the_entries_after_the_kernel_and_init_in_order() {
        let bytes = initrd(&[("kernel", b"k"), ("init", b"i"), ("manifest", b"{}"), ("keyd", &[7; 600])]);
        let entries = Bundle::new(&bytes).unwrap().after_init().unwrap();
        assert_eq!(
            entries,
            [Entry { name: "manifest", data: b"{}" }, Entry { name: "keyd", data: &[7; 600] }]
        );
    }

    #[test]
    fn a_header_that_does_not_read_refuses_the_whole_bundle() {
        let good = initrd(&[("kernel", b"k"), ("init", b"i"), ("manifest", b"{}")]);
        assert!(Bundle::new(&good[..SIGNATURE_LEN - 1]).is_none());
        // Cut inside the last entry's data.
        let header = SIGNATURE_LEN + 4 * BLOCK;
        assert!(Bundle::new(&good[..header + 1]).unwrap().after_init().is_none());
        // A size that is not octal, and one larger than the archive.
        for size in [&b"0000000000x\0"[..], b"77777777777\0"] {
            let mut bad = good.clone();
            bad[header + 124..header + 136].copy_from_slice(size);
            assert!(Bundle::new(&bad).unwrap().after_init().is_none(), "{size:?}");
        }
        // Fewer entries than the kernel and init.
        assert!(Bundle::new(&initrd(&[("kernel", b"k")])).unwrap().after_init().is_none());
    }
}
