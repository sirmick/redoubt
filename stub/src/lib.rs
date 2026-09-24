//! Shared pieces of the loader stub (WP-R2; `docs/PACKAGES.md`, Launching a process;
//! `docs/BUILD-PLAN.md`, WP-R2): the fixed address every launcher maps it at, and the segment
//! bounds-checking logic (host-testable, no syscalls).
//!
//! `main.rs` is the on-target binary: `_start`, and the actual mapping calls `plan` only
//! describes. Splitting them here lets `plan`'s hostile-input logic run as ordinary host unit
//! tests, with no kernel and no QEMU (TENETS.md 6: "fuzz what parses"). The startup page's
//! `image_addr`/`image_len` fields (INIT.md, Startup block) are read through
//! `redoubt_rt::startup::Startup::image`, not duplicated here.
#![no_std]

use elf::ElfBytes;
use elf::abi::{PF_R, PF_W, PF_X, PT_LOAD};
use elf::endian::LittleEndian;
use elf::segment::ProgramHeader;
use redoubt_sys::{MemFlags, PAGE_SIZE};

/// Where a launcher maps the stub's flat binary in every child, and the `entry` it passes to
/// `process_start` (PACKAGES.md: "a flat binary, one code region at a fixed address... the same
/// for everyone"). Same value on both widths.
///
/// Not a KERNEL-SPEC.md or INIT.md constant: `process_map`'s destination is caller-chosen
/// (KERNEL-SPEC.md), so this is an implementation convention every launcher and this crate's own
/// `link.x` agree on, not a kernel mechanism. Chosen well clear of
/// `tests/programs/src/spawn.rs::IMAGE_BASE` (0x1_0000, where test images and, by convention,
/// most real program images are expected to start) and far below `STACK_TOP`/`STARTUP_AT`.
pub const STUB_ENTRY: usize = 0x0020_0000;

/// Why an image was refused whole (PACKAGES.md: "A malicious ELF can at most compromise the
/// process it was going to become" -- every one of these is a refusal, never a panic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadImage {
    /// The startup page named no image, was misaligned, or did not parse at all.
    NoImage,
    /// `ElfBytes::minimal_parse` or `.segments()` refused it.
    Malformed,
    /// A field does not fit `usize` on this width, or an address/length computation would wrap.
    Overflow,
    /// A segment's file range reaches outside the image the parent named.
    OutOfImage,
    /// A segment's page range overlaps the image bytes it came from, the startup page, or the
    /// stub's own fixed region: an ELF may describe only itself.
    Overlaps,
    /// A segment asks for both writable and executable, or for neither read nor write nor
    /// execute (R11, W^X; TENETS.md 2).
    BadFlags,
}

/// One validated `PT_LOAD` segment, ready to be mapped.
pub struct Segment<'a> {
    /// Its first page, page-aligned.
    pub first_page: usize,
    /// Its length in whole pages.
    pub pages: usize,
    /// Where in `first_page`'s mapping `file` starts (`p_vaddr - first_page`).
    pub file_offset: usize,
    /// Its file bytes, sliced from the image; may be shorter than `pages * PAGE_SIZE` (the rest
    /// is `.bss`, zero).
    pub file: &'a [u8],
    /// Its final permissions (never both `WRITE` and `EXECUTE`; never empty).
    pub flags: MemFlags,
}

/// Parses `image` (the bytes at `[image_addr, image_addr + image.len())` in this process's own
/// memory) and calls `on_segment` for each validated `PT_LOAD` segment, in file order. Returns
/// the entry point on success.
///
/// Pure and allocation-free: it never maps or unmaps anything itself, so every hostile-input
/// path here runs as a host unit test with no kernel and no QEMU. `on_segment` is where the
/// actual `unsafe` mapping calls belong (`main.rs`); a test's `on_segment` can just record what
/// it was asked to do.
///
/// `exclude` names page ranges (`[start, end)`, already page-aligned) no segment may overlap:
/// the image's own bytes, the startup page, and the stub's own fixed region. An ELF may describe
/// only itself.
pub fn plan<'a, E>(
    image: &'a [u8],
    image_addr: usize,
    exclude: &[(usize, usize)],
    mut on_segment: impl FnMut(Segment<'a>) -> Result<(), E>,
) -> Result<usize, Either<BadImage, E>> {
    let elf = ElfBytes::<LittleEndian>::minimal_parse(image).map_err(|_| Either::A(BadImage::Malformed))?;
    let segments = elf.segments().ok_or(Either::A(BadImage::Malformed))?;
    for header in segments.iter().filter(|s| s.p_type == PT_LOAD && s.p_memsz > 0) {
        let segment = validate(image, image_addr, exclude, &header).map_err(Either::A)?;
        on_segment(segment).map_err(Either::B)?;
    }
    usize::try_from(elf.ehdr.e_entry).map_err(|_| Either::A(BadImage::Overflow))
}

/// Bounds-checks one segment against the image it came from and `exclude`, and computes its
/// final permissions. Never trusts a single field of `header` on its own: every offset and
/// length is checked against `image` and against overflow before it is used (`header` is
/// attacker-controlled).
fn validate<'a>(
    image: &'a [u8],
    image_addr: usize,
    exclude: &[(usize, usize)],
    header: &ProgramHeader,
) -> Result<Segment<'a>, BadImage> {
    let vaddr = usize::try_from(header.p_vaddr).map_err(|_| BadImage::Overflow)?;
    let memsz = usize::try_from(header.p_memsz).map_err(|_| BadImage::Overflow)?;
    let filesz = usize::try_from(header.p_filesz).map_err(|_| BadImage::Overflow)?;
    let offset = usize::try_from(header.p_offset).map_err(|_| BadImage::Overflow)?;
    if filesz > memsz {
        return Err(BadImage::OutOfImage);
    }
    let seg_end = vaddr.checked_add(memsz).ok_or(BadImage::Overflow)?;
    let file_end = offset.checked_add(filesz).ok_or(BadImage::Overflow)?;
    let file = image.get(offset..file_end).ok_or(BadImage::OutOfImage)?;

    let first_page = vaddr & !(PAGE_SIZE - 1);
    let page_end = seg_end.next_multiple_of(PAGE_SIZE);
    let image_pages = (image_addr & !(PAGE_SIZE - 1), (image_addr + image.len()).next_multiple_of(PAGE_SIZE));
    if overlaps(first_page, page_end, image_pages.0, image_pages.1)
        || exclude.iter().any(|&(start, end)| overlaps(first_page, page_end, start, end))
    {
        return Err(BadImage::Overlaps);
    }

    let mut flags = MemFlags::NONE;
    if header.p_flags & PF_R != 0 {
        flags = flags | MemFlags::READ;
    }
    if header.p_flags & PF_W != 0 {
        flags = flags | MemFlags::WRITE;
    }
    if header.p_flags & PF_X != 0 {
        flags = flags | MemFlags::EXECUTE;
    }
    // R11: never both writable and executable, and a mapping is refused if it is writable
    // without being readable; TENETS.md 2 (W^X everywhere) rules out W+X outright, so a
    // conforming segment is at least readable and never both W and X.
    let bits = flags.bits();
    let writable = bits & MemFlags::WRITE.bits() != 0;
    let executable = bits & MemFlags::EXECUTE.bits() != 0;
    let readable = bits & MemFlags::READ.bits() != 0;
    if (writable && executable) || bits == 0 || (writable && !readable) {
        return Err(BadImage::BadFlags);
    }

    Ok(Segment {
        first_page,
        pages: (page_end - first_page) / PAGE_SIZE,
        file_offset: vaddr - first_page,
        file,
        flags,
    })
}

fn overlaps(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

/// Two error kinds, kept distinct rather than merged into one enum: `plan`'s own refusals
/// ([`BadImage`]) are always the same reasons regardless of caller; `on_segment`'s are whatever
/// the mapping calls it made can fail with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Either<A, B> {
    A(A),
    B(B),
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::*;

    const PT_LOAD_TAG: u32 = PT_LOAD;
    const EHDR_LEN: usize = 64;
    const PHDR_LEN: usize = 56;

    /// One ELF64 LE image, its program header table right after the file header, and `segments`'
    /// bytes placed back to back after that -- everything a hostile-input test needs to control.
    fn elf64(entry: u64, segments: &[(u32, u64, &[u8], u64)]) -> Vec<u8> {
        // (flags, vaddr, file bytes, memsz)
        let phoff = EHDR_LEN as u64;
        let mut data_offset = phoff + (segments.len() * PHDR_LEN) as u64;
        let mut phdrs = Vec::new();
        let mut data = Vec::new();
        for &(flags, vaddr, file, memsz) in segments {
            let offset = data_offset;
            phdrs.extend_from_slice(&PT_LOAD_TAG.to_le_bytes());
            phdrs.extend_from_slice(&flags.to_le_bytes());
            phdrs.extend_from_slice(&offset.to_le_bytes());
            phdrs.extend_from_slice(&vaddr.to_le_bytes());
            phdrs.extend_from_slice(&vaddr.to_le_bytes()); // p_paddr, unused
            phdrs.extend_from_slice(&(file.len() as u64).to_le_bytes());
            phdrs.extend_from_slice(&memsz.to_le_bytes());
            phdrs.extend_from_slice(&PAGE_SIZE.to_le_bytes()); // p_align
            data.extend_from_slice(file);
            data_offset += file.len() as u64;
        }
        let mut out = Vec::new();
        out.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // e_ident
        out.extend_from_slice(&2u16.to_le_bytes()); // e_type: ET_EXEC
        out.extend_from_slice(&0xf3u16.to_le_bytes()); // e_machine: EM_RISCV
        out.extend_from_slice(&1u32.to_le_bytes()); // e_version
        out.extend_from_slice(&entry.to_le_bytes());
        out.extend_from_slice(&phoff.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
        out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        out.extend_from_slice(&(EHDR_LEN as u16).to_le_bytes());
        out.extend_from_slice(&(PHDR_LEN as u16).to_le_bytes());
        out.extend_from_slice(&(segments.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
        out.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
        out.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx
        out.extend_from_slice(&phdrs);
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn plan_maps_a_well_formed_segment() {
        let code = [0x13, 0x00, 0x00, 0x00]; // a RISC-V nop, as ordinary file bytes
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, 4096)]);
        let mut mapped = Vec::new();
        let entry = plan::<()>(&image, 0x2000_0000, &[], |segment| {
            mapped.push((segment.first_page, segment.pages, segment.file_offset, segment.flags));
            Ok(())
        })
        .unwrap();
        assert_eq!(entry, 0x1_0000);
        assert_eq!(mapped, [(0x1_0000, 1, 0, MemFlags::READ | MemFlags::EXECUTE)]);
    }

    #[test]
    fn plan_refuses_a_segment_reaching_outside_the_image() {
        // filesz claims more bytes than the image actually holds after this segment's offset.
        let mut image = elf64(0x1_0000, &[(PF_R, 0x1_0000, &[1, 2, 3, 4], 4096)]);
        // Corrupt p_filesz (at phoff + 32) to claim far more than the image holds.
        image[EHDR_LEN + 32..EHDR_LEN + 40].copy_from_slice(&(u32::MAX as u64).to_le_bytes());
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::OutOfImage)));
    }

    #[test]
    fn plan_refuses_a_segment_overlapping_an_excluded_range() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, 4096)]);
        // The startup page, say, sits right where this segment would land.
        let exclude = [(0x1_0000, 0x1_1000)];
        let result = plan::<()>(&image, 0x2000_0000, &exclude, |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::Overlaps)));
    }

    #[test]
    fn plan_refuses_writable_and_executable() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_W | PF_X, 0x1_0000, &code, 4096)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::BadFlags)));
    }

    #[test]
    fn plan_refuses_writable_without_readable() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_W, 0x1_0000, &code, 4096)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::BadFlags)));
    }

    #[test]
    fn plan_surfaces_the_callers_own_error() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, 4096)]);
        let result = plan(&image, 0x2000_0000, &[], |_| Err(42));
        assert_eq!(result, Err(Either::B(42)));
    }
}
