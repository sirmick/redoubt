//! Launching a child process from a test program (KERNEL-SPEC.md, `process_*`; PACKAGES.md,
//! launching).
//!
//! A real launcher maps the loader stub into the child and copies the program's ELF bytes in as
//! data, and the stub parses them inside the child's own budget (PACKAGES.md). There is no stub
//! yet (WP-R2), and a test program has no file to read anyway, so these cases do the one thing a
//! program can do with nothing but its own memory: **the child is another copy of the caller**.
//! The caller reads its own ELF program headers -- the loader mapped its ELF header at the first
//! segment's address, so they are simply there in memory -- allocates fresh pages, copies each
//! loadable segment into them and hands them to the child at the very same addresses with the
//! very same permissions. Identical addresses mean the copy's code, constants and statics all
//! resolve as they do here, so the child runs ordinary Rust; which part of it runs is `arg`'s to
//! say.
//!
//! This exercises exactly what WP-K4 built: `process_create`, `process_map` (which *moves* the
//! pages, so the caller's own image is never at risk), `process_start` with its handle list and
//! its `arg`, and the exit notice that comes back afterwards.

use crate::rd::{self, Error, MemFlags};

/// Where a test program's image starts: the lowest address the linker gives a segment, and
/// therefore where the loader mapped the ELF header (`loader/src/image.rs`).
pub const IMAGE_BASE: usize = 0x1_0000;
/// The top of a child's stack. Below the heap (`DEFAULT_HEAP_BASE`) and far above any image.
pub const STACK_TOP: usize = 0x1000_0000;
/// Stack pages a child gets.
pub const STACK_PAGES: usize = 8;
/// Where a child's startup page lands; `process_start`'s `arg` (INIT.md, Startup block).
pub const STARTUP_AT: usize = 0x0f00_0000;

const PAGE: usize = 4096;
/// Pages of image this helper can carry. Test programs are far smaller than this.
const MAX_IMAGE_PAGES: usize = 256;

/// ELF segment permissions, as the per-page map below records them.
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// The pages of this program's image and what permissions each needs.
pub struct Image {
    /// The first page's address; pages are consecutive from here.
    pub base: usize,
    /// ELF `p_flags` for each page, 0 for a page no segment covers.
    flags: [u32; MAX_IMAGE_PAGES],
    pages: usize,
}

impl Image {
    pub fn pages(&self) -> usize {
        self.pages
    }
}

/// Read a little-endian value of `n` bytes at `at`.
fn read(at: usize, n: usize) -> u64 {
    let mut value = 0u64;
    for i in 0..n {
        // SAFETY: `at + i` is inside this program's own mapped ELF image, which the caller has
        // checked starts with the ELF magic; a one-byte read of it.
        let byte = unsafe { ((at + i) as *const u8).read_volatile() };
        value |= u64::from(byte) << (8 * i);
    }
    value
}

/// This program's own loadable segments, from the ELF header the loader mapped at
/// [`IMAGE_BASE`]. Panics if what is there is not this program's ELF header, or if it does not
/// fit the fixed page map: both are bugs in the test, not things a case should tolerate.
pub fn image() -> Image {
    assert_eq!(read(IMAGE_BASE, 4), 0x464c_457f, "no ELF header at the image base");
    let elf64 = cfg!(target_pointer_width = "64");
    let word = if elf64 { 8 } else { 4 };
    let (phoff_at, phentsize_at, phnum_at) = if elf64 { (0x20, 0x36, 0x38) } else { (0x1c, 0x2a, 0x2c) };
    let phoff = read(IMAGE_BASE + phoff_at, word) as usize;
    let phentsize = read(IMAGE_BASE + phentsize_at, 2) as usize;
    let phnum = read(IMAGE_BASE + phnum_at, 2) as usize;
    let (vaddr_at, filesz_at, flags_at) = if elf64 { (16, 32, 4) } else { (8, 16, 24) };
    let memsz_at = filesz_at + word;

    let mut lo = usize::MAX;
    let mut hi = 0usize;
    for i in 0..phnum {
        let ph = IMAGE_BASE + phoff + i * phentsize;
        if read(ph, 4) != 1 {
            continue; // not PT_LOAD
        }
        let vaddr = read(ph + vaddr_at, word) as usize;
        let memsz = read(ph + memsz_at, word) as usize;
        if memsz == 0 {
            continue;
        }
        lo = lo.min(vaddr & !(PAGE - 1));
        hi = hi.max((vaddr + memsz).next_multiple_of(PAGE));
    }
    assert!(lo != usize::MAX && hi > lo, "no loadable segments");
    let pages = (hi - lo) / PAGE;
    assert!(pages <= MAX_IMAGE_PAGES, "the image is larger than this helper carries");

    let mut flags = [0u32; MAX_IMAGE_PAGES];
    for i in 0..phnum {
        let ph = IMAGE_BASE + phoff + i * phentsize;
        if read(ph, 4) != 1 {
            continue;
        }
        let vaddr = read(ph + vaddr_at, word) as usize;
        let memsz = read(ph + memsz_at, word) as usize;
        if memsz == 0 {
            continue;
        }
        let first = ((vaddr & !(PAGE - 1)) - lo) / PAGE;
        let last = ((vaddr + memsz).next_multiple_of(PAGE) - lo) / PAGE;
        for slot in flags[first..last].iter_mut() {
            *slot |= read(ph + flags_at, 4) as u32;
        }
    }
    // W^X, before the kernel is ever asked: a page carrying both a writable and an executable
    // segment could not be given to a child at all (R11), and no linker we use produces one.
    assert!(
        flags[..pages].iter().all(|f| *f & (PF_W | PF_X) != (PF_W | PF_X)),
        "a page of this image is both writable and executable"
    );
    Image { base: lo, flags, pages }
}

/// ELF `p_flags` as the ABI's memory flags. A page no segment covers is not mapped at all.
fn mem_flags(p_flags: u32) -> MemFlags {
    let mut flags = MemFlags::NONE;
    if p_flags & PF_R != 0 {
        flags = flags | MemFlags::READ;
    }
    if p_flags & PF_W != 0 {
        flags = flags | MemFlags::WRITE;
    }
    if p_flags & PF_X != 0 {
        flags = flags | MemFlags::EXECUTE;
    }
    flags
}

/// Copy `len` bytes from `src` to `dst`, neither of which overlaps the other.
fn copy(dst: usize, src: usize, len: usize) {
    for i in 0..len {
        // SAFETY: `src` is a page of this program's own image and `dst` a page it has just
        // mapped read-write; both are `len` bytes long and they do not overlap.
        unsafe { (dst as *mut u8).add(i).write_volatile(((src + i) as *const u8).read_volatile()) };
    }
}

/// A child of this process: the handle `process_create` returned.
pub struct Child {
    pub process: u32,
    /// The address its startup page landed at, which is also its `arg`; 0 if it has none.
    pub arg: usize,
}

/// Create a process in `budget` reporting to `exit` (a badge-0 endpoint handle), give it a copy
/// of this program's image, a stack and, if `startup` is not empty, a read-only page holding it,
/// and start it at `entry` with `handles` in its slots 1..n.
///
/// Everything before `process_start` is undoable by closing the process handle's budget; nothing
/// here leaves the caller's own image or handles changed.
pub fn spawn(
    image: &Image,
    budget: u32,
    exit: u32,
    entry: usize,
    startup: &[u8],
    handles: &[u32],
) -> Result<Child, Error> {
    let process = rd::process_create(budget, exit)?;
    give_image(process, image)?;
    give_stack(process)?;
    let arg = if startup.is_empty() { 0 } else { give_startup(process, startup)? };
    rd::process_start(process, entry, STACK_TOP - 16, arg, handles)?;
    Ok(Child { process, arg })
}

/// Copy this program's image into fresh pages and hand each run of equally-permissioned pages to
/// the child at the address it has here.
pub fn give_image(process: u32, image: &Image) -> Result<(), Error> {
    let scratch = rd::map_anon(image.pages * PAGE, rd::rw())?;
    for page in 0..image.pages {
        if image.flags[page] != 0 {
            copy(scratch + page * PAGE, image.base + page * PAGE, PAGE);
        }
    }
    let mut page = 0;
    while page < image.pages {
        let flags = image.flags[page];
        let mut end = page;
        while end < image.pages && image.flags[end] == flags {
            end += 1;
        }
        if flags != 0 {
            let len = (end - page) * PAGE;
            rd::process_map(process, scratch + page * PAGE, image.base + page * PAGE, len, mem_flags(flags))?;
        }
        page = end;
    }
    Ok(())
}

/// The child's stack: read-write, never executable, at a fixed address it knows nothing about
/// (its stack pointer arrives in a register).
fn give_stack(process: u32) -> Result<(), Error> {
    let scratch = rd::map_anon(STACK_PAGES * PAGE, rd::rw())?;
    let len = STACK_PAGES * PAGE;
    rd::process_map(process, scratch, STACK_TOP - len, len, rd::rw())
}

/// The startup page (INIT.md, Startup block): one ordinary page, mapped **read-only**, whose
/// address the child receives as `arg`.
fn give_startup(process: u32, startup: &[u8]) -> Result<usize, Error> {
    assert!(startup.len() <= PAGE, "a startup block fits in one page");
    let scratch = rd::map_anon(PAGE, rd::rw())?;
    for (i, byte) in startup.iter().enumerate() {
        // SAFETY: `scratch` is a page this process has just mapped read-write, and `i` is
        // within it (asserted above); a one-byte write.
        unsafe { (scratch as *mut u8).add(i).write_volatile(*byte) };
    }
    rd::process_map(process, scratch, STARTUP_AT, PAGE, MemFlags::READ)?;
    Ok(STARTUP_AT)
}

/// Read a byte of this process's startup page, wherever `arg` put it.
pub fn startup_byte(arg: usize, i: usize) -> u8 {
    // SAFETY: `arg` is the address the parent mapped a readable page at, and `i` is inside it
    // (the caller passes an offset below `PAGE`).
    unsafe { ((arg + i) as *const u8).read_volatile() }
}
