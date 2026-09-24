//! The loader stub (WP-R2): entered directly by `process_start` (no `redoubt-rt::entry!`, since
//! it never returns an exit code of its own -- it jumps into the program it just mapped). See
//! `stub::plan` for the segment bounds-checking this drives.
#![no_std]
#![no_main]

use core::arch::asm;

use redoubt_rt::handle::{process_exit, set_flags};
use redoubt_rt::startup::Startup;
use redoubt_sys::{Error, MemFlags, PAGE_SIZE};
use stub::{Either, STUB_ENTRY};

/// Exit codes the stub itself uses. Distinct from anything the started program can produce:
/// once the jump below happens, this process's exit code is that program's to choose.
mod exit {
    /// No startup page, one that does not parse, or one naming no image.
    pub const BAD_STARTUP: u32 = 110;
    /// The ELF did not parse, a segment failed its bounds/flags check, or `map_fixed` refused it
    /// (including the WP-K5a shim, which always does): the parent (or its package) handed this
    /// child a hostile image, and only this child pays for it (PACKAGES.md: "A malicious ELF can
    /// at most compromise the process it was going to become").
    pub const BAD_IMAGE: u32 = 111;
}

#[no_mangle]
pub extern "C" fn _start(arg: usize) -> ! {
    match run(arg) {
        Ok(entry) => jump(entry, arg),
        Err(code) => process_exit(code),
    }
}

fn run(arg: usize) -> Result<usize, u32> {
    if arg == 0 || !arg.is_multiple_of(PAGE_SIZE) {
        return Err(exit::BAD_STARTUP);
    }
    // SAFETY: `arg` is the startup page address the kernel gave this thread (`process_start`'s
    // `arg`, INIT.md, Startup block); the parent mapped one whole page here, read-only, before
    // starting this process, and it stays mapped for the life of the process.
    let page = unsafe { core::slice::from_raw_parts(arg as *const u8, PAGE_SIZE) };
    let startup = Startup::parse(page).map_err(|_| exit::BAD_STARTUP)?;
    let (image_addr, image_len) = startup.image().ok_or(exit::BAD_STARTUP)?;
    // SAFETY: the parent's `process_map` step (PACKAGES.md step 4) put exactly this range in
    // this process's own memory, read-write, before `process_start`; `Startup::image` already
    // checked `image_addr + image_len` does not overflow (INIT.md, Startup block). A parent that
    // named a range it did not actually map only faults this read, which hurts nobody but this
    // child (PACKAGES.md).
    let image = unsafe { core::slice::from_raw_parts(image_addr as *const u8, image_len) };

    let startup_page = (arg, arg + PAGE_SIZE);
    let stub_region = (STUB_ENTRY & !(PAGE_SIZE - 1), STUB_ENTRY + stub_len());
    let exclude = [startup_page, stub_region];

    // PACKAGES.md step 5: segments are mapped at their link addresses with `map_fixed`, before
    // the stub maps anything else; a segment overlapping the stub, the startup page or the image
    // makes it exit (`stub::plan`'s `exclude` argument, checked before any mapping call below).
    stub::plan(image, image_addr, &exclude, |segment| map_segment(&segment)).map_err(|e| match e {
        Either::A(_bad_image) => exit::BAD_IMAGE,
        Either::B(_map_error) => exit::BAD_IMAGE,
    })
}

/// Maps one validated segment at its own `vaddr` (`segment.first_page`) with `map_fixed`
/// (KERNEL-SPEC.md, answer 172), executable or read-only per `segment.flags`, never both
/// writable and executable at once (R11, TENETS.md 2).
fn map_segment(segment: &stub::Segment) -> Result<(), Error> {
    let len = segment.pages * PAGE_SIZE;
    map_fixed(segment.first_page, len, MemFlags::READ | MemFlags::WRITE)?;
    // SAFETY: `map_fixed` just mapped `segment.first_page..segment.first_page+len` read-write in
    // this process's own memory, zeroed by the kernel; `file_offset..file_offset+file.len()` lies
    // inside it (`validate` in `stub::lib` guarantees `file.len() <= memsz` and `file_offset +
    // memsz <= pages * PAGE_SIZE`), and `segment.file` is this process's own read of the image
    // (`run`, above).
    unsafe {
        core::ptr::copy_nonoverlapping(
            segment.file.as_ptr(),
            (segment.first_page + segment.file_offset) as *mut u8,
            segment.file.len(),
        );
    }
    if segment.flags != (MemFlags::READ | MemFlags::WRITE) {
        set_flags(segment.first_page, len, segment.flags)?;
    }
    Ok(())
}

/// **Blocked on WP-K5a** (docs/WORKSPACE-QA.md, `R2-stub-self-map`; answer 172;
/// KERNEL-SPEC.md, System calls: `map_fixed(addr, len, flags)`, last in the call table). The
/// kernel does not implement it yet, and `redoubt-sys`'s call table has no variant for it (both
/// outside R2's owned paths: the sole kernel writer lands them as a follow-up package). This
/// shim already has the signature and error shape KERNEL-SPEC.md specifies -- `map_segment`
/// above will not change when WP-K5a lands, only this function's body, becoming a real
/// `redoubt_sys::Call::MapFixed`. Until then it fails closed, mapping nothing.
fn map_fixed(_addr: usize, _len: usize, _flags: MemFlags) -> Result<(), Error> { Err(Error::InvalidArgument) }

/// The stub's own mapped length, rounded to whole pages: `_stub_end` is defined by `link.x` at
/// the end of its last section.
fn stub_len() -> usize {
    extern "C" {
        static _stub_end: u8;
    }
    // `_stub_end` is a linker symbol, not a real object; only its address is taken, never
    // dereferenced, so this needs no `unsafe`.
    let end = core::ptr::addr_of!(_stub_end) as usize;
    (end - STUB_ENTRY).next_multiple_of(PAGE_SIZE)
}

/// Jumps to the started program's entry with `arg` unchanged in `a0` (INIT.md, Startup block;
/// `redoubt_rt::start::start`'s `_start(startup: usize)` is exactly this contract). Never
/// returns: there is nothing to return to, and this stack is the program's own now.
fn jump(entry: usize, arg: usize) -> ! {
    // SAFETY: `entry` is `validate`'s checked `e_entry` (within the image `plan` already
    // bounds-checked every segment of); every segment it names has just been mapped executable
    // or read-only by `map_segment` above. `arg` is passed through unchanged in `a0`, the same
    // register the stub itself received it in, matching `process_start`'s contract.
    unsafe {
        asm!(
            "jr {entry}",
            entry = in(reg) entry,
            in("a0") arg,
            options(noreturn)
        )
    }
}
