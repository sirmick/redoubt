//! The loader stub (WP-R2): entered directly by `process_start` (no `redoubt-rt::entry!`, since
//! it never returns an exit code of its own -- it jumps into the program it just mapped). See
//! `stub::plan` for the segment bounds-checking this drives.
//!
//! Depends on `redoubt-sys` only, never `redoubt-rt` (`stub::lib`'s module doc): every call here
//! goes straight through `redoubt_sys::syscall`, and this file supplies its own `#[panic_handler]`
//! (below) rather than pulling in `redoubt-rt`'s console-reporting one and the statics it needs.
#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

use redoubt_sys::{Call, Error, MemFlags, PAGE_SIZE, Return, syscall};
use stub::{Either, STUB_ENTRY, process_exit, read_image};

/// Exit codes the stub itself uses. Distinct from anything the started program can produce:
/// once the jump below happens, this process's exit code is that program's to choose.
mod exit {
    /// No startup page, one that does not parse, or one naming no image.
    pub const BAD_STARTUP: u32 = 110;
    /// The ELF did not parse, or a segment failed one of `stub::plan`'s own bounds/flags checks:
    /// the parent (or its package) handed this child a hostile image, and only this child pays
    /// for it (PACKAGES.md: "A malicious ELF can at most compromise the process it was going to
    /// become").
    pub const BAD_IMAGE: u32 = 111;
    /// `map_fixed` refused a segment `stub::plan` already accepted -- today this is always the
    /// **WP-K5a shim** (`map_fixed`, below), which fails closed unconditionally until the real
    /// syscall lands; kept distinct from `BAD_IMAGE` so this case (a system limitation, not a
    /// hostile image) is not indistinguishable from one on target (round-2 red team P3-7).
    pub const MAP_UNAVAILABLE: u32 = 112;
    /// The stub itself panicked (matches `redoubt_rt::start::exit::PANIC`, though this binary
    /// never links `redoubt-rt`: any caller inspecting exit codes sees the same value either way).
    pub const PANIC: u32 = 101;
}

/// `.text.init` (`link.x`) is the only section `KEEP`'d first in `.text`, and `link.x` asserts
/// this symbol lands at `ORIGIN(RAM)` (`STUB_ENTRY`): without this section, the linker is free
/// to place `_start` anywhere in `.text`, so every launcher's `process_start(..., STUB_ENTRY,
/// ...)` would jump into whatever code happened to land first instead (round-2 red team P1-1).
#[no_mangle]
#[link_section = ".text.init"]
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
    let (image_addr, image_len) = match read_image(page) {
        Ok(Some(image)) => image,
        Ok(None) | Err(_) => return Err(exit::BAD_STARTUP),
    };

    let startup_page = (arg, arg + PAGE_SIZE);
    let stub_region = (STUB_ENTRY & !(PAGE_SIZE - 1), STUB_ENTRY + stub_len());
    let exclude = [startup_page, stub_region];

    // The image range itself, not just a segment inside it, must not overlap the stub or the
    // startup page (round-2 red team P3-6): otherwise the raw read below could alias the stub's
    // own mapped code, and the later "free the image" unmap could remove it out from under this
    // process before the jump.
    if !stub::image_in_bounds(image_addr, image_len, &exclude) {
        return Err(exit::BAD_STARTUP);
    }
    // SAFETY: the parent's `process_map` step (PACKAGES.md step 4) put exactly this range in
    // this process's own memory, read-write, before `process_start`; `read_image` already checked
    // `image_addr + image_len` does not overflow (INIT.md, Startup block), and the check above
    // rules out this range aliasing the stub's own mapped code or the startup page. A parent that
    // named a range it did not actually map only faults this read, which hurts nobody but this
    // child (PACKAGES.md).
    let image = unsafe { core::slice::from_raw_parts(image_addr as *const u8, image_len) };

    // PACKAGES.md step 5: segments are mapped at their link addresses with `map_fixed`, before
    // the stub maps anything else; a segment overlapping the stub, the startup page or the image
    // makes it exit (`stub::plan`'s `exclude` argument, checked before any mapping call below).
    let entry =
        stub::plan(image, image_addr, &exclude, |segment| map_segment(&segment)).map_err(|e| match e {
            Either::A(_bad_image) => exit::BAD_IMAGE,
            // The map_fixed shim always refuses (below): while it does, every mapping failure
            // here comes from it, not from a hostile segment `plan` already accepted, so it gets
            // its own code rather than being indistinguishable from BAD_IMAGE (round-2 red team
            // P3-7).
            Either::B(_map_error) => exit::MAP_UNAVAILABLE,
        })?;

    // PACKAGES.md, Launching a process: "free the image". Every segment's bytes are now copied
    // into their own mapping (`map_segment`, above); the parent's copy is no longer read. Best
    // effort: an unmap failure here only wastes this child's own address space, so it does not
    // block the jump (`report_panic`'s "gives up quietly" precedent, `redoubt_rt::start`).
    let _ = unmap(image_addr, page_align_up(image_len));

    Ok(entry)
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

/// One system call expecting `Return::Nothing`, exactly as `redoubt_rt::handle`'s wrappers do
/// (duplicated rather than depending on `redoubt-rt`: see `stub::lib`'s module doc).
fn nothing(call: &Call) -> Result<(), Error> {
    match syscall(call)? {
        Return::Nothing => Ok(()),
        // redoubt-sys decodes a result by the call's number, so another shape cannot arrive.
        _ => Err(Error::InvalidArgument),
    }
}

fn set_flags(addr: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    nothing(&Call::SetFlags { addr, len, flags })
}

fn unmap(addr: usize, len: usize) -> Result<(), Error> { nothing(&Call::Unmap { addr, len }) }

/// Rounds `len` up to a whole number of pages.
fn page_align_up(len: usize) -> usize {
    match len.checked_add(PAGE_SIZE - 1) {
        Some(v) => v & !(PAGE_SIZE - 1),
        // Cannot actually happen: `read_image` already checked `image_addr + image_len` (a
        // nonzero `image_addr`) does not overflow, so `image_len` alone has headroom. Round down
        // instead of up rather than pass an even larger `len` to `unmap`.
        None => len & !(PAGE_SIZE - 1),
    }
}

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
    // register the stub itself received it in, matching `process_start`'s contract. `fence.i`
    // runs first: `map_segment` wrote the program's code with ordinary stores
    // (`copy_nonoverlapping`), which only the data cache and store buffer see on RISC-V until an
    // explicit `fence.i` orders them against instruction fetch (unpriv spec, `Zifencei`); without
    // it this hart could fetch stale (e.g. zeroed) bytes at `entry`.
    unsafe {
        asm!(
            "fence.i",
            "jr {entry}",
            entry = in(reg) entry,
            in("a0") arg,
            options(noreturn)
        )
    }
}

/// This binary's one panic handler (a `no_std`/`no_main` binary needs exactly one in its whole
/// link graph): unlike `redoubt_rt::start`'s, it prints nothing (no console handle to report to,
/// no allocator to build one with -- `stub::lib`'s module doc) and just exits, the same way every
/// other refusal here does.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! { process_exit(exit::PANIC) }

/// See `stub::NullAlloc`'s doc: `redoubt-wire` needs a `#[global_allocator]` in the link graph
/// even though this binary never allocates.
#[global_allocator]
static ALLOC: stub::NullAlloc = stub::NullAlloc;
