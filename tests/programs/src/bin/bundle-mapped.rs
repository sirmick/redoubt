//! The bundle, mapped into the program in `init`'s place (kernel/boot.md, "The loader loads only
//! the kernel and `init`"): its first thread starts with the bundle's address in `a0` and its
//! length in `a1`. The loader verified it, signature and archive, before it parsed it.
//!
//! This program is that process, alone: it holds the console and the Reset right. It reads the
//! archive from the mapping, finds the kernel's entry first and its own second, and compares its
//! own entry's read-only segments with the image the loader mapped it from. It checks that
//! `root` pays for the bundle's frames, and that `map_fixed` over it is refused. Then it writes
//! to the mapping, which must fault: the verdict for that is the kernel's own `PROGRAM HALT`
//! line, which nothing in this program prints, and the program's line after the write is
//! forbidden.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::bundle::Bundle;
use test_programs::console::{self, Console};
use test_programs::rd;

/// The name this program has in the bundle: the bench packs a program under its binary's name.
const OWN_NAME: &[u8] = b"bundle-mapped";
/// The signature before the archive (kernel/boot.md, "Verified boot").
const SIGNATURE_LEN: usize = 64;

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out, "[bundle-mapped] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out, $($arg)*).ok();
    }};
}

/// A little-endian value of `n` bytes at `at` in `bytes`.
fn read(bytes: &[u8], at: usize, n: usize) -> usize {
    bytes[at..at + n].iter().rev().fold(0, |value, b| (value << 8) | usize::from(*b))
}

/// Whether every read-only `PT_LOAD` segment of `elf` is, byte for byte, what is mapped at its
/// address in this process; and how many bytes that compared.
fn matches_own_image(elf: &[u8]) -> (bool, usize) {
    if elf.get(..4) != Some(b"\x7fELF".as_slice()) {
        return (false, 0);
    }
    let elf64 = cfg!(target_pointer_width = "64");
    let word = if elf64 { 8 } else { 4 };
    let (phoff_at, phentsize_at, phnum_at) = if elf64 { (0x20, 0x36, 0x38) } else { (0x1c, 0x2a, 0x2c) };
    let (offset_at, vaddr_at, filesz_at, flags_at) = if elf64 { (8, 16, 32, 4) } else { (4, 8, 16, 24) };
    let (phoff, phentsize, phnum) =
        (read(elf, phoff_at, word), read(elf, phentsize_at, 2), read(elf, phnum_at, 2));
    let mut compared = 0;
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        const PT_LOAD: usize = 1;
        const PF_W: usize = 2;
        if read(elf, ph, 4) != PT_LOAD || read(elf, ph + flags_at, 4) & PF_W != 0 {
            continue;
        }
        let (offset, vaddr, filesz) = (
            read(elf, ph + offset_at, word),
            read(elf, ph + vaddr_at, word),
            read(elf, ph + filesz_at, word),
        );
        let Some(file) = elf.get(offset..offset + filesz) else { return (false, compared) };
        // SAFETY: the loader mapped this read-only segment of this program at `vaddr`, `filesz`
        // bytes of it from the file; a read of memory this program runs from.
        let mapped = unsafe { core::slice::from_raw_parts(vaddr as *const u8, filesz) };
        if file != mapped {
            return (false, compared);
        }
        compared += filesz;
    }
    (compared != 0, compared)
}

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "[bundle-mapped] the bundle is at {:#x}, {} bytes", bundle, len);
    check!(out, bundle % rd::PAGE_SIZE == 0 && len > SIGNATURE_LEN, "a0 is a page, a1 more than a signature");

    // SAFETY: the loader mapped the whole verified bundle here, readable, `len` bytes from
    // `bundle` (kernel/boot.md), and nothing in this process writes it before the end.
    let mut entries = unsafe { Bundle::at(bundle, len) }.expect("a bundle past its signature").entries();
    let (kernel, own) = (entries.next(), entries.next());
    check!(
        out,
        kernel
            .as_ref()
            .is_some_and(|e| e.name == b"kernel" && e.data.get(..4) == Some(b"\x7fELF".as_slice())),
        "the first entry is the kernel's ELF"
    );
    check!(out, own.as_ref().is_some_and(|e| e.name == OWN_NAME), "the second entry is this program");
    let (same, compared) = own.map_or((false, 0), |e| matches_own_image(e.data));
    check!(out, same, "its read-only segments are this program's own image ({} bytes compared)", compared);

    // The boot table (kernel/budgets.md, "The tree from the boot manifest"): `root` carves 15
    // processes into `system` and 47 into `users`, and keeps one, this process's.
    let (root, system, users) = (rd::usage(rd::ROOT), rd::usage(rd::SYSTEM), rd::usage(rd::USERS));
    let (root, system, users) = (root.expect("root"), system.expect("system"), users.expect("users"));
    let own_processes = root.processes_usage - system.processes_limit - users.processes_limit;
    check!(
        out,
        system.processes_limit == 15 && users.processes_limit == 47 && own_processes == 1,
        "system 15 processes, users 47, and root keeps one for this one"
    );
    // The bundle's frames are `init`'s, charged to `root` with the rest of what the loader gave
    // it: what `root` pays beyond the two budgets it carved and their own pages.
    let own_pages = root.pages_usage - system.pages_limit - users.pages_limit - 2;
    let pages = len.div_ceil(rd::PAGE_SIZE) as u64;
    check!(
        out,
        own_pages > pages,
        "root pays for the bundle's {} pages and the rest of this process ({})",
        pages,
        own_pages
    );

    // The region is taken: fresh pages over it are refused as an overlap, and charge nothing.
    let before = rd::usage(rd::ROOT).expect("root's usage").pages_usage;
    let over = rd::map_fixed(bundle, rd::PAGE_SIZE, rd::rw());
    let after = rd::usage(rd::ROOT).expect("root's usage").pages_usage;
    check!(
        out,
        over == Err(rd::Error::InvalidArgument) && after == before,
        "map_fixed over the bundle -> {:?}, nothing charged",
        over
    );

    say!(out, "[bundle-mapped] writing to the bundle");
    // SAFETY: deliberately hostile: the mapping is read-only, and the kernel must fault this
    // store and end the process.
    unsafe { (bundle as *mut u8).write_volatile(0) };
    say!(out, "[bundle-mapped] FAIL: the write went through");
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
