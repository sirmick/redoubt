//! Shared pieces of the loader stub (`docs/servers/init.md`, "Launching through the loader
//! stub"): the fixed address every launcher maps it at, and the segment bounds-checking logic
//! (host-testable, no syscalls).
//!
//! `main.rs` is the on-target binary: `_start`, and the actual mapping calls `plan` only
//! describes. Splitting them here lets `plan`'s hostile-input logic run as ordinary host unit
//! tests, with no kernel and no QEMU (TENETS.md 6: "fuzz what parses").
//!
//! This crate depends on `redoubt-wire` and `redoubt-sys` directly, never `redoubt-rt`: the
//! stub's mapped region carries no writable statics (`link.x`), but `redoubt-rt` unconditionally
//! defines a `#[global_allocator]` and a `#[panic_handler]` for the machine at its crate root
//! (`libs/rt/src/lib.rs`, `libs/rt/src/start.rs`), pulled in whole by depending on any part of
//! it. `read_image` below reads `image_addr`/`image_len` straight off the decoded message
//! (servers/init.md, "The startup block"), the same rule `redoubt_rt::startup::parse_image`
//! applies for ordinary programs, without building the allocated entries table that rule's own
//! callers need and this one does not.
#![no_std]

use elf::ElfBytes;
use elf::abi::{EM_RISCV, ET_EXEC, PF_R, PF_W, PF_X, PT_LOAD};
use elf::endian::LittleEndian;
use elf::file::Class;
use elf::segment::ProgramHeader;
use redoubt_sys::{MemFlags, PAGE_SIZE};
use redoubt_wire::proto::startup::Message;

/// A generous but small bound on `e_phnum` (elf crate honours `PN_XNUM`, so an attacker can
/// otherwise claim a program header count up to roughly `image_len / 56`, close to
/// `MAX_IMAGE_LEN / 56`, about 9.5 million entries): `plan`'s segment-vs-segment overlap check is
/// `O(K*N)` in the number of headers `N` by design (host-testable, no allocator), so an
/// unbounded `N` lets a hostile image burn CPU proportional to its own header count times its
/// accepted `PT_LOAD` count. No honest program needs anywhere near this many segments.
const MAX_PHNUM: usize = 64;

/// The ELF class this build's stub accepts: `usize`-width segment addresses only fit this
/// target's own class, and the stub links for one width at a time.
#[cfg(target_pointer_width = "32")]
const ELF_CLASS: Class = Class::ELF32;
#[cfg(target_pointer_width = "64")]
const ELF_CLASS: Class = Class::ELF64;

/// Exits the calling process with `code`: the same call `redoubt_rt::handle::process_exit`
/// wraps, duplicated here so this crate's `[[bin]]`s (the stub itself, and the `fixture-child`
/// test fixture) never depend on `redoubt-rt` (module doc above). `redoubt_sys::syscall` exists
/// only on the riscv targets these binaries actually run on, not host unit tests.
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub fn process_exit(code: u32) -> ! {
    loop {
        let _ = redoubt_sys::syscall(&redoubt_sys::Call::ProcessExit { code });
    }
}

/// `redoubt-wire` declares `extern crate alloc` (only its JSON parser actually allocates; typed
/// decoding, which `read_image` uses, never does), so any `no_std`/`no_main` binary that links
/// this crate needs *some* `#[global_allocator]` even though it never calls one. Each of this
/// crate's `[[bin]]`s instantiates one `static ALLOC: NullAlloc = NullAlloc;` with
/// `#[global_allocator]` (a `#[global_allocator]` static must live in the binary crate, not the
/// lib): zero-sized, so it adds no heap or writable static to either binary's page.
pub struct NullAlloc;
// SAFETY: neither binary's code path ever allocates (the stub only uses `redoubt-wire`'s typed
// decoding, which borrows into its input; the fixture fixture does not touch `redoubt-wire` at
// all), so `alloc` is unreachable; returning null is the documented way to report failure if it
// ever is.
unsafe impl core::alloc::GlobalAlloc for NullAlloc {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 { core::ptr::null_mut() }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

/// Where a launcher maps the stub's flat binary in every child, and the `entry` it passes to
/// `process_start` (servers/init.md, "Launching through the loader stub"). Same value on both
/// widths.
///
/// Not a kernel constant: `process_map`'s destination is caller-chosen (kernel/processes.md,
/// "Creating and starting"), so this is an implementation convention every launcher and this
/// crate's own `link.x` agree on, not a kernel mechanism (kernel/memory-layout.md, "The loader
/// stub and the link range"). Chosen well clear of
/// `tests/programs/src/spawn.rs::IMAGE_BASE` (0x1_0000, where test images and, by convention,
/// most real program images are expected to start) and far below `STACK_TOP`/`STARTUP_AT`.
pub const STUB_ENTRY: usize = 0x1FF0_0000;

/// A defensive cap on `image_len` (servers/init.md, "The startup block"), checked before this stub
/// ever reads the image bytes it names: kernel/memory-layout.md's own bound on program link space
/// ("just under 512 MiB" below `STUB_ENTRY`), so no honest image needs more than this. Refusing an
/// absurd `image_len` up front keeps `validate`'s per-segment work (and `main.rs`'s eventual raw
/// read of `[image_addr, image_addr + image_len)`) bounded by a number grounded in the loading
/// convention, rather than only by `usize::MAX`/page-alignment overflow checks.
pub const MAX_IMAGE_LEN: usize = STUB_ENTRY;

/// Why an image was refused whole (servers/init.md R32: a hostile image hurts only its process
/// -- every one of these is a refusal, never a panic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadImage {
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
    /// `e_machine` is not `EM_RISCV`, or the file's class does not match this stub's own width
    /// (a 32-bit image on a 64-bit stub or the reverse): every address in this file would be
    /// read at the wrong width.
    WrongMachine,
    /// `p_align` is not a power of two, or `p_vaddr` and `p_offset` disagree mod `p_align` (the
    /// ELF rule this stub's page-granular mapping otherwise never needs to check, since a
    /// segment that breaks it would put file bytes at the wrong offset within the mapped page).
    BadAlign,
    /// `e_entry` does not fall inside any segment this file maps executable: the stub would jump
    /// into memory the child never mapped, or into data.
    EntryNotExecutable,
    /// `e_phnum` (after resolving `PN_XNUM`) is over [`MAX_PHNUM`].
    TooManySegments,
    /// A segment's page range touches page 0, or reaches past `STUB_ENTRY`
    /// (kernel/memory-layout.md, "The loader stub and the link range"): checked directly,
    /// rather than relying only on `exclude` naming the stub's own current region correctly.
    OutOfLinkRange,
    /// `e_type` is not `ET_EXEC` (userland/native.md: "No dynamic linking"): an
    /// `ET_DYN` image would map unrelocated at whatever `p_vaddr` its segments claim, most often
    /// page 0.
    WrongType,
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
    // e_machine/class: every field below is read at this stub's own width, so a file built for
    // the other one would have every address misread rather than refused.
    if elf.ehdr.e_machine != EM_RISCV || elf.ehdr.class != ELF_CLASS {
        return Err(Either::A(BadImage::WrongMachine));
    }
    // Every native Redoubt binary is a fixed-address, non-relocatable static (userland/native.md:
    // "No dynamic linking"); refuse anything else (e.g. `ET_DYN`) before trusting its `p_vaddr`s.
    if elf.ehdr.e_type != ET_EXEC {
        return Err(Either::A(BadImage::WrongType));
    }
    let entry = usize::try_from(elf.ehdr.e_entry).map_err(|_| Either::A(BadImage::Overflow))?;
    let segments = elf.segments().ok_or(Either::A(BadImage::Malformed))?;
    // `segments.len()` already resolves `PN_XNUM` (elf crate): cap it before any loop below scans
    // it, rather than only bounding the `PT_LOAD` count the loop below actually maps.
    if segments.len() > MAX_PHNUM {
        return Err(Either::A(BadImage::TooManySegments));
    }
    let loads = || segments.iter().filter(|s| s.p_type == PT_LOAD && s.p_memsz > 0);
    let mut entry_executable = false;
    for (i, header) in loads().enumerate() {
        let segment = validate(image, image_addr, exclude, &header).map_err(Either::A)?;
        let seg_end = segment.first_page + segment.pages * PAGE_SIZE;
        // Segments only check against the image, the startup page and the stub above
        // (`exclude`); an ELF may still describe two segments that overlap each other, e.g. one
        // hostile segment's pages silently overwriting another's mapping order. `i` earlier
        // segments were already validated to their own final page ranges above, so re-deriving
        // theirs here (rather than storing a growing list, which would need an allocator) is
        // just repeated pure work, bounded by this image's own segment count.
        for other in loads().take(i) {
            let other_segment = validate(image, image_addr, exclude, &other).map_err(Either::A)?;
            let other_end = other_segment.first_page + other_segment.pages * PAGE_SIZE;
            if overlaps(segment.first_page, seg_end, other_segment.first_page, other_end) {
                return Err(Either::A(BadImage::Overlaps));
            }
        }
        if segment.flags.bits() & MemFlags::EXECUTE.bits() != 0
            && entry >= segment.first_page
            && entry < seg_end
        {
            entry_executable = true;
        }
        on_segment(segment).map_err(Either::B)?;
    }
    if !entry_executable {
        return Err(Either::A(BadImage::EntryNotExecutable));
    }
    Ok(entry)
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
    let align = usize::try_from(header.p_align).map_err(|_| BadImage::Overflow)?;
    // ELF: `p_align` is 0 or 1 (no constraint) or a power of two, and `p_vaddr` must then equal
    // `p_offset`, modulo `p_align`. This stub always maps at a page boundary regardless of
    // `p_align` (`first_page`, below), so a violation cannot misalign the mapping itself; it is
    // refused anyway because it means the file was not built for this loading convention and its
    // `p_vaddr`/`p_offset` pairing should not be trusted for anything downstream.
    if align > 1 {
        if !align.is_power_of_two() || vaddr % align != offset % align {
            return Err(BadImage::BadAlign);
        }
    }
    if filesz > memsz {
        return Err(BadImage::OutOfImage);
    }
    let seg_end = vaddr.checked_add(memsz).ok_or(BadImage::Overflow)?;
    let file_end = offset.checked_add(filesz).ok_or(BadImage::Overflow)?;
    let file = image.get(offset..file_end).ok_or(BadImage::OutOfImage)?;

    let first_page = vaddr & !(PAGE_SIZE - 1);
    // `seg_end` not overflowing (above) does not mean rounding it up to a page does: an attacker
    // picks `p_vaddr`/`p_memsz` freely, so `seg_end` can sit anywhere below `usize::MAX`, and
    // `next_multiple_of` would wrap the addition past it. `page_align_up` refuses that instead of
    // silently computing a wrong (too-small) `page_end`, which would let a segment through with a
    // page range shorter than the bytes it actually claims.
    let page_end = page_align_up(seg_end)?;
    // kernel/memory-layout.md: programs link at 0x1_0000 and their segments must end by
    // `STUB_ENTRY` (0x1FF0_0000) -- page 0 is never a valid link address either. Checked directly
    // against `STUB_ENTRY` rather than only through `exclude`, which only ever names the stub's own
    // *current* region (`main.rs`'s `stub_region`), not everything above it.
    if first_page < PAGE_SIZE || page_end > STUB_ENTRY {
        return Err(BadImage::OutOfLinkRange);
    }
    let image_end = image_addr.checked_add(image.len()).ok_or(BadImage::Overflow)?;
    let image_pages = (image_addr & !(PAGE_SIZE - 1), page_align_up(image_end)?);
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

/// True when `[image_addr, image_addr + image_len)` overlaps any of `exclude` (the stub's own
/// region, the startup page): `plan` already refuses a *segment* that overlaps them
/// (`validate`'s `exclude` check), but nothing stopped the image range itself from doing so.
/// `main.rs::run` naming `image_addr == STUB_ENTRY` would make its own later "free the image"
/// unmap remove the stub's own mapped code out from under itself before the jump. `image_len`
/// is not re-validated here (the caller already ran it through [`read_image`], which bounds it
/// and its overflow).
pub fn image_in_bounds(image_addr: usize, image_len: usize, exclude: &[(usize, usize)]) -> bool {
    let image_end = image_addr.saturating_add(image_len);
    !exclude.iter().any(|&(start, end)| overlaps(image_addr, image_end, start, end))
}

/// Rounds `addr` up to the next page boundary, refusing rather than wrapping when that would
/// overflow `usize`.
fn page_align_up(addr: usize) -> Result<usize, BadImage> {
    addr.checked_add(PAGE_SIZE - 1).map(|v| v & !(PAGE_SIZE - 1)).ok_or(BadImage::Overflow)
}

/// The length word in front of the message (servers/init.md, "The startup block").
const FRAME_HEADER: usize = 4;
/// One whole page: the startup block never holds more (servers/init.md, "The startup block").
const MAX_BLOCK: usize = PAGE_SIZE;
/// The only `startup` block version this stub understands (servers/init.md, "The startup
/// block").
const VERSION: u32 = 1;

/// Why the startup page's image fields could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadStartup {
    /// The page holds fewer bytes than the block's length says, or the block's length is
    /// impossible.
    Short,
    /// The message does not decode as `startup`, or is not the version this stub understands.
    Malformed,
    /// `image_addr`/`image_len` break servers/init.md's rule: one is 0 and the other is not,
    /// `image_addr` is not page-aligned, or `image_addr + image_len` overflows or does not fit
    /// this target's `usize`.
    BadImage,
}

/// Reads `image_addr`/`image_len` out of the startup page at `page` (servers/init.md, "The startup
/// block"; `arg`, "Launching through the loader stub" step 3), without allocating:
/// `Some((addr, len))` when the block names an image, `None` for a process started at its own
/// entry rather than through the stub.
///
/// Applies the same `image_addr`/`image_len` rule as `redoubt_rt::startup::Startup::image`
/// (`libs/rt/src/startup.rs`'s private `parse_image`), which this duplicates rather than calls:
/// see the module doc for why the stub cannot depend on `redoubt-rt`.
pub fn read_image(page: &[u8]) -> Result<Option<(usize, usize)>, BadStartup> {
    let page = page.get(..MAX_BLOCK).unwrap_or(page);
    let word = page.get(..FRAME_HEADER).ok_or(BadStartup::Short)?;
    let len = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    let len = usize::try_from(len).map_err(|_| BadStartup::Short)?;
    if len > MAX_BLOCK - FRAME_HEADER {
        return Err(BadStartup::Short);
    }
    let message = page.get(FRAME_HEADER..FRAME_HEADER + len).ok_or(BadStartup::Short)?;
    let Message::Startup(fields) = Message::decode_file(message).map_err(|_| BadStartup::Malformed)?;
    if fields.version != VERSION {
        return Err(BadStartup::Malformed);
    }
    if fields.image_addr == 0 && fields.image_len == 0 {
        return Ok(None);
    }
    if fields.image_addr == 0 || fields.image_len == 0 {
        return Err(BadStartup::BadImage);
    }
    let addr = usize::try_from(fields.image_addr).map_err(|_| BadStartup::BadImage)?;
    let len = usize::try_from(fields.image_len).map_err(|_| BadStartup::BadImage)?;
    if !addr.is_multiple_of(PAGE_SIZE) {
        return Err(BadStartup::BadImage);
    }
    if len > MAX_IMAGE_LEN {
        return Err(BadStartup::BadImage);
    }
    addr.checked_add(len).ok_or(BadStartup::BadImage)?;
    Ok(Some((addr, len)))
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
            // Real linkers place file bytes so `p_offset % p_align == p_vaddr % p_align` (ELF;
            // `validate` checks it): pad with zero bytes rather than start each segment's bytes
            // wherever the previous one happened to end.
            let want = vaddr % PAGE_SIZE as u64;
            let have = data_offset % PAGE_SIZE as u64;
            let pad = if have <= want { want - have } else { PAGE_SIZE as u64 - have + want };
            data.extend(core::iter::repeat(0u8).take(pad as usize));
            data_offset += pad;
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
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
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
        let mut image = elf64(0x1_0000, &[(PF_R, 0x1_0000, &[1, 2, 3, 4], PAGE_SIZE as u64)]);
        // Corrupt p_filesz (at phoff + 32) to claim far more than the image holds.
        image[EHDR_LEN + 32..EHDR_LEN + 40].copy_from_slice(&(u32::MAX as u64).to_le_bytes());
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::OutOfImage)));
    }

    #[test]
    fn plan_refuses_a_segment_overlapping_an_excluded_range() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        // The startup page, say, sits right where this segment would land.
        let exclude = [(0x1_0000, 0x1_1000)];
        let result = plan::<()>(&image, 0x2000_0000, &exclude, |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::Overlaps)));
    }

    #[test]
    fn plan_refuses_writable_and_executable() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_W | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::BadFlags)));
    }

    #[test]
    fn plan_refuses_writable_without_readable() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_W, 0x1_0000, &code, PAGE_SIZE as u64)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::BadFlags)));
    }

    #[test]
    fn plan_surfaces_the_callers_own_error() {
        let code = [0u8; 4];
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        let result = plan(&image, 0x2000_0000, &[], |_| Err(42));
        assert_eq!(result, Err(Either::B(42)));
    }

    #[test]
    fn read_image_reads_the_field_a_real_builder_writes() {
        let mut builder = redoubt_rt::startup::StartupBuilder::new(0);
        builder.image(0x2000_0000, 0x3000);
        let page = builder.finish().unwrap();
        assert_eq!(read_image(&page), Ok(Some((0x2000_0000, 0x3000))));
    }

    #[test]
    fn read_image_is_none_for_no_image() {
        let page = redoubt_rt::startup::StartupBuilder::new(0).finish().unwrap();
        assert_eq!(read_image(&page), Ok(None));
    }

    #[test]
    fn read_image_refuses_a_short_page() {
        assert_eq!(read_image(&[0u8; 3]), Err(BadStartup::Short));
    }

    #[test]
    fn read_image_refuses_an_image_len_over_the_cap() {
        let mut builder = redoubt_rt::startup::StartupBuilder::new(0);
        builder.image(0x2000_0000, MAX_IMAGE_LEN + 1);
        let page = builder.finish().unwrap();
        assert_eq!(read_image(&page), Err(BadStartup::BadImage));
    }

    #[test]
    fn plan_refuses_a_non_riscv_machine() {
        let code = [0u8; 4];
        let mut image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        // e_machine sits right after e_ident (16) and e_type (2).
        image[18..20].copy_from_slice(&0u16.to_le_bytes()); // EM_NONE
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::WrongMachine)));
    }

    #[test]
    fn plan_refuses_an_entry_outside_any_executable_segment() {
        let code = [0u8; 4];
        // Readable only: e_entry names a byte inside it, but nothing here is executable.
        let image = elf64(0x1_0000, &[(PF_R, 0x1_0000, &code, PAGE_SIZE as u64)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::EntryNotExecutable)));
    }

    #[test]
    fn plan_refuses_two_segments_that_overlap_each_other() {
        let code = [0u8; 4];
        // Neither overlaps `exclude`; they overlap each other instead.
        let image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64), (PF_R, 0x1_0000, &code, PAGE_SIZE as u64)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::Overlaps)));
    }

    #[test]
    fn plan_refuses_a_misaligned_p_align() {
        let code = [0u8; 4];
        let mut image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        // p_align is the phdr's last 8-byte field, right after the one program header (EHDR_LEN
        // + p_type + p_flags + p_offset + p_vaddr + p_paddr + p_filesz + p_memsz = 64 + 48).
        image[112..120].copy_from_slice(&3u64.to_le_bytes()); // not a power of two
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::BadAlign)));
    }

    #[test]
    fn plan_refuses_more_than_max_phnum_segments() {
        let code = [0u8; 4];
        let segments: Vec<_> =
            (0..MAX_PHNUM + 1).map(|_| (PF_R | PF_X, 0x1_0000, code.as_slice(), PAGE_SIZE as u64)).collect();
        let image = elf64(0x1_0000, &segments);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::TooManySegments)));
    }

    #[test]
    fn plan_refuses_a_segment_touching_page_zero() {
        let code = [0u8; 4];
        // vaddr 0: kernel/memory-layout.md's link range starts at 0x1_0000, never page 0.
        let image = elf64(0, &[(PF_R | PF_X, 0, &code, PAGE_SIZE as u64)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::OutOfLinkRange)));
    }

    #[test]
    fn plan_refuses_a_segment_reaching_into_the_stub_region() {
        let code = [0u8; 4];
        let vaddr = STUB_ENTRY as u64 - 100;
        // memsz alone pushes this segment's (page-rounded) end past STUB_ENTRY.
        let image = elf64(vaddr, &[(PF_R | PF_X, vaddr, &code, 200)]);
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::OutOfLinkRange)));
    }

    #[test]
    fn plan_refuses_a_non_exec_type() {
        let code = [0u8; 4];
        let mut image = elf64(0x1_0000, &[(PF_R | PF_X, 0x1_0000, &code, PAGE_SIZE as u64)]);
        image[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN, not ET_EXEC
        let result = plan::<()>(&image, 0x2000_0000, &[], |_| Ok(()));
        assert_eq!(result, Err(Either::A(BadImage::WrongType)));
    }

    #[test]
    fn image_in_bounds_refuses_an_image_overlapping_the_stub() {
        let stub_region = (STUB_ENTRY, STUB_ENTRY + 0x4_0000);
        assert!(!image_in_bounds(STUB_ENTRY, 0x1000, &[stub_region]));
    }

    #[test]
    fn image_in_bounds_refuses_an_image_overlapping_the_startup_page() {
        let startup_page = (0x3000_0000, 0x3000_1000);
        assert!(!image_in_bounds(0x2FFF_F000, 0x2000, &[startup_page]));
    }

    #[test]
    fn image_in_bounds_accepts_a_disjoint_image() {
        let exclude = [(STUB_ENTRY, STUB_ENTRY + 0x4_0000), (0x3000_0000, 0x3000_1000)];
        assert!(image_in_bounds(0x2000_0000, 0x1000, &exclude));
    }
}
