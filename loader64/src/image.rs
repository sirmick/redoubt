//! Loading programs from the boot bundle.
//!
//! The bundle is a plain (ustar) tar archive of ELF executables, handed to us as the
//! initrd. The first entry is the kernel; every following entry becomes an initial
//! process, in order, starting at PID 2.

use elf::abi::{PF_R, PF_W, PF_X, PT_LOAD};
use elf::endian::LittleEndian;
use elf::ElfBytes;

use crate::alloc::PageAllocator;
use crate::paging::{AddressSpace, Pte};
use crate::PAGE_SIZE;

/// Map every `PT_LOAD` segment of `image` into `space` and return the entry point.
///
/// Every segment, and the entry point, must lie inside `allowed`. Without this a program
/// image could ask to be mapped over the kernel: the kernel's tables are shared by every
/// address space, so that would reach far beyond the process being loaded.
pub fn load_elf(
    alloc: &mut PageAllocator,
    space: &AddressSpace,
    pid: u8,
    image: &[u8],
    allowed: core::ops::Range<usize>,
    user: bool,
) -> usize {
    let elf = ElfBytes::<LittleEndian>::minimal_parse(image).expect("invalid ELF");
    let segments = elf.segments().expect("ELF has no program headers");

    for segment in segments.iter().filter(|s| s.p_type == PT_LOAD && s.p_memsz > 0) {
        let mut flags = if user { Pte::USER } else { Pte::GLOBAL };
        for (elf_flag, pte_flag) in [(PF_R, Pte::R), (PF_W, Pte::W), (PF_X, Pte::X)] {
            if segment.p_flags & elf_flag != 0 {
                flags |= pte_flag;
            }
        }

        let vaddr = segment.p_vaddr as usize;
        let segment_end = vaddr.checked_add(segment.p_memsz as usize).expect("ELF segment wraps the address space");
        assert!(
            allowed.contains(&vaddr) && segment_end <= allowed.end && segment.p_filesz <= segment.p_memsz,
            "ELF segment {vaddr:#x}..{segment_end:#x} is outside {:#x}..{:#x}",
            allowed.start,
            allowed.end
        );
        let file = &image[segment.p_offset as usize..][..segment.p_filesz as usize];
        let first_page = vaddr & !(PAGE_SIZE - 1);
        let end = vaddr + segment.p_memsz as usize;

        for page_virt in (first_page..end).step_by(PAGE_SIZE) {
            // Pages are zeroed when allocated, which takes care of .bss.
            let page_phys = match space.translate(alloc, page_virt) {
                Some(phys) => phys,
                None => alloc.alloc(pid),
            };
            space.map(alloc, page_phys, page_virt, flags);

            // Copy the part of the file image that lands in this page.
            let copy_start = page_virt.max(vaddr);
            let copy_end = (page_virt + PAGE_SIZE).min(vaddr + file.len());
            if copy_start < copy_end {
                let src = &file[copy_start - vaddr..copy_end - vaddr];
                let dst = (page_phys + (copy_start - page_virt)) as *mut u8;
                unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len()) };
            }
        }
    }
    let entry = elf.ehdr.e_entry as usize;
    assert!(allowed.contains(&entry), "ELF entry point {entry:#x} is outside the allowed range");
    entry
}
