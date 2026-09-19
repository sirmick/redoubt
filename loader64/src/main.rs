//! Xous loader for RV64 platforms that boot through SBI firmware with a device tree.
//!
//! The firmware enters `_start` in S-mode on a single boot hart with the MMU off,
//! `a0` = hart ID and `a1` = physical address of the flattened device tree. All other
//! harts stay parked in the firmware until started through the SBI HSM extension.
//!
//! The loader unpacks the boot bundle (see `image.rs`), builds an Sv39 address space for
//! the kernel and for each initial process, describes the machine to the kernel in a
//! tagged argument block, and enters the kernel. Design notes: `planning/xous64/BOOT.md`.

#![no_std]
#![no_main]

mod alloc;
mod args;
mod dt;
mod grants;
mod verify;
mod image;
mod paging;

use core::arch::{asm, global_asm};

use dt::Platform;

use tar_no_std::TarArchiveRef;
use xous::arch::{
    EXCEPTION_STACK_PAGES, EXCEPTION_STACK_TOP, KERNEL_AREA, KERNEL_STACK_PAGES, KERNEL_STACK_TOP,
    PHYSMAP_BASE, THREAD_CONTEXT_AREA, THREAD_CONTEXT_PAGES, USER_AREA_END, USER_STACK_TOP,
};

use crate::alloc::{PageAllocator, Pid, KERNEL_PID};
use crate::paging::{AddressSpace, Pte};

pub const PAGE_SIZE: usize = 4096;

/// Pages of stack reserved for the first thread of an initial process. Only the top
/// page is backed by memory; the kernel demand-pages the rest.
const USER_STACK_PAGES: usize = 32;
/// The ABI wants 16-byte stack alignment; leave one slot free at the very top.
const STACK_PADDING: usize = 16;
const ARGS_PAGES: usize = 4;

global_asm!(
    r#"
    .section .text.init, "ax"
    .global _start
_start:
    // Mask interrupts; nothing is ready to take one yet.
    csrw    sie, zero
    csrw    sip, zero

    la      t0, _sbss
    la      t1, _ebss
1:  bgeu    t0, t1, 2f
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       1b
2:
    la      sp, _stack_top
    // a0 (hart ID) and a1 (DTB) are untouched and become the Rust arguments.
    call    rust_entry
3:  wfi
    j       3b
"#
);

extern "C" {
    static _start: u8;
    static _loader_end: u8;
}

/// What the kernel expects at `init_offset`: one entry per process, kernel first.
/// Must match `InitialProcess` in `kernel/src/arch/riscv/process.rs`.
#[repr(C)]
struct InitialProcess {
    satp: usize,
    entrypoint: usize,
    sp: usize,
    env: usize,
}

#[no_mangle]
extern "C" fn rust_entry(hart_id: usize, dtb: usize) -> ! {
    println!();
    println!("loader64: Xous RV64 loader, boot hart {}", hart_id);

    // SAFETY: the SBI boot protocol passes the device-tree address in `a1`.
    let platform = unsafe { Platform::read(dtb) };
    let ram = platform.ram.clone();
    let bundle = platform.initrd.clone();
    println!("  {} hart(s), timebase {} Hz", platform.cpu_count, platform.timebase_hz);
    println!("  ram: {:#x}..{:#x} ({} MiB)", ram.start, ram.end, ram.len() >> 20);
    println!("  bundle: {:#x}..{:#x}", bundle.start, bundle.end);

    let firmware = ram.start..(&raw const _start) as usize;
    let loader = firmware.end..(&raw const _loader_end) as usize;
    let dtb = platform.dtb..platform.dtb + platform.total_size;

    let mut alloc = PageAllocator::new(ram.clone());
    alloc.reserve(firmware.clone());
    alloc.reserve(loader);
    alloc.reserve(dtb.clone());
    alloc.reserve(bundle.clone());
    alloc.init_rpt();
    // The firmware stays resident, and a userspace device manager will want the device
    // tree. The loader and the bundle are left unowned, so the kernel reuses them.
    alloc.set_owner(firmware, KERNEL_PID);
    alloc.set_owner(dtb, KERNEL_PID);

    let extra_pages: usize = platform.mmio().iter().map(|r| r.range.len().div_ceil(PAGE_SIZE)).sum();
    let xpt = alloc.alloc_contiguous(extra_pages.div_ceil(PAGE_SIZE).max(1), KERNEL_PID);

    let args_base = alloc.alloc_contiguous(ARGS_PAGES, KERNEL_PID);
    // SAFETY: a fresh, zeroed, page-aligned allocation of exactly this size, referenced
    // from nowhere else.
    let args_buffer =
        unsafe { core::slice::from_raw_parts_mut(args_base as *mut u32, ARGS_PAGES * PAGE_SIZE / 4) };
    let mut args = args::ArgsBuilder::new(args_buffer);
    args.begin(b"MREx");
    for region in platform.mmio() {
        args.word64(region.range.start as u64);
        args.word64(region.range.len().next_multiple_of(PAGE_SIZE) as u64);
        args.word(u32::from_le_bytes(region.name));
        args.word(0);
    }
    args.end();

    match &platform.plic {
        Some(plic) => {
            println!("  plic: {:#x}, S-mode context {}", plic.range.start, plic.context);
            args.begin(b"Plic");
            args.word64(plic.range.start as u64);
            args.word64(plic.range.len() as u64);
            args.word(plic.context as u32);
            args.word(0);
            args.end();
        }
        None => println!("  no PLIC found in the device tree"),
    }

    // Entropy for the kernel's RNG. Server IDs are drawn from it, so it must not be guessable.
    let seed = platform.rng_seed();
    if seed.len() >= 16 {
        println!("  rng-seed: {} bytes", seed.len());
        args.begin(b"Seed");
        args.bytes(seed);
        args.end();
    } else {
        // Fail closed: the kernel has no other source of entropy at boot.
        panic!("the device tree has no usable /chosen/rng-seed");
    }

    // Ticks per second of the `time` CSR, for whoever ends up driving the hart timer.
    if platform.timebase_hz != 0 {
        args.begin(b"Time");
        args.word64(platform.timebase_hz);
        args.end();
    }

    // The table of initial processes, kernel first. One page bounds how many there can be.
    // SAFETY: a fresh, zeroed, page-aligned allocation referenced from nowhere else.
    // `InitialProcess` is four `usize`s, for which all-zeroes is valid.
    let processes: &mut [InitialProcess] = unsafe {
        let page = alloc.alloc(KERNEL_PID) as *mut InitialProcess;
        core::slice::from_raw_parts_mut(page, PAGE_SIZE / core::mem::size_of::<InitialProcess>())
    };
    // SAFETY: the firmware placed the initrd at this range (from the device tree), it is
    // reserved in the allocator so nothing overwrites it, and it is only read.
    let initrd = unsafe { core::slice::from_raw_parts(bundle.start as *const u8, bundle.len()) };
    let bundle = verify::authenticated_bundle(initrd);
    println!("  bundle signature ok ({} bytes)", bundle.len());
    let archive = TarArchiveRef::new(bundle).expect("boot bundle is not a tar archive");
    // The device-grant manifest, if present, is a `grants` entry (not a process).
    let manifest = archive
        .entries()
        .find(|e| e.filename().as_str() == Ok("grants"))
        .and_then(|e| core::str::from_utf8(e.data()).ok())
        .unwrap_or("");
    let mut entries = archive.entries();

    // The kernel is PID 1 and the first entry of the bundle.
    let kernel_image = entries.next().expect("boot bundle is empty");
    let kernel = AddressSpace::new_kernel(&mut alloc, KERNEL_PID);
    let kernel_flags = Pte::R | Pte::W | Pte::GLOBAL;
    let kernel_entry =
        image::load_elf(&mut alloc, &kernel, KERNEL_PID, kernel_image.data(), KERNEL_AREA..usize::MAX, false);
    kernel.map_stack(&mut alloc, KERNEL_STACK_TOP, KERNEL_STACK_PAGES, kernel_flags);
    kernel.map_stack(&mut alloc, EXCEPTION_STACK_TOP, EXCEPTION_STACK_PAGES, kernel_flags);
    map_context(&mut alloc, &kernel, KERNEL_PID);
    let kernel_process = InitialProcess {
        satp: kernel.satp(),
        entrypoint: kernel_entry,
        sp: KERNEL_STACK_TOP - STACK_PADDING,
        env: 0,
    };
    println!("  PID 1: {} -> {:#x}", kernel_image.filename().as_str().unwrap_or("?"), kernel_entry);

    let mut count = 1;
    for entry in entries {
        let name = entry.filename();
        let name = name.as_str().unwrap_or("?");
        if name == "grants" {
            continue;
        }
        let pid = count as Pid + 1;

        let space = AddressSpace::new_user(&mut alloc, pid, &kernel);
        let entrypoint = image::load_elf(&mut alloc, &space, pid, entry.data(), PAGE_SIZE..USER_AREA_END, true);
        let stack_flags = Pte::R | Pte::W | Pte::USER;
        space.map_stack(&mut alloc, USER_STACK_TOP, 1, stack_flags);
        for page in 2..=USER_STACK_PAGES {
            space.reserve(&mut alloc, USER_STACK_TOP - page * PAGE_SIZE, stack_flags);
        }
        map_context(&mut alloc, &space, pid);
        println!("  PID {}: {} -> {:#x}", pid, name, entrypoint);

        let process =
            InitialProcess { satp: space.satp(), entrypoint, sp: USER_STACK_TOP - STACK_PADDING, env: 0 };
        *processes.get_mut(count).expect("too many initial processes") = process;
        count += 1;

        // The kernel only needs these tags to count processes and to find `.eh_frame`.
        // TODO(xous64): report the `.eh_frame` address so `std` can unwind.
        args.begin(b"IniE");
        args.word(0);
        args.word(0);
        args.end();

        args.begin(b"PNam");
        args.word(pid as u32);
        args.word(name.len() as u32);
        args.bytes(name.as_bytes());
        args.end();

        grants::emit(&mut args, manifest, name, pid);
    }
    processes[0] = kernel_process;
    args.finish(ram.start, ram.len(), b"sram");

    println!("  {} MiB free, entering kernel", alloc.free_bytes() >> 20);
    // SAFETY: `kernel.satp()` names the address space built above, in which `kernel_entry`
    // and the kernel stack are mapped, and the four pointers are physmap addresses of
    // pages owned by PID 1.
    unsafe {
        enter_kernel(
            PHYSMAP_BASE + args_base,
            PHYSMAP_BASE + processes.as_ptr() as usize,
            PHYSMAP_BASE + alloc.rpt_base(),
            PHYSMAP_BASE + xpt,
            kernel.satp(),
            kernel_entry,
            KERNEL_STACK_TOP - STACK_PADDING,
        )
    }
}

/// Map the zeroed pages the kernel keeps its per-process state in.
fn map_context(alloc: &mut PageAllocator, space: &AddressSpace, pid: Pid) {
    for page in 0..THREAD_CONTEXT_PAGES {
        let phys = alloc.alloc(pid);
        space.map(alloc, phys, THREAD_CONTEXT_AREA + page * PAGE_SIZE, Pte::R | Pte::W);
    }
}

/// Turn on the kernel's address space and jump to its entry point.
///
/// The loader is not mapped in that address space, so there is nothing to return to and
/// no instruction after the `satp` write can be fetched. Instead of building a throwaway
/// identity mapping, point `stvec` at the kernel entry: the fetch after `csrw satp`
/// faults, and the hart "traps" straight into the kernel with a0-a3 and sp intact.
///
/// # Safety
/// `satp` must name a complete Sv39 address space in which `entry` is mapped executable
/// and `sp` is the top of a mapped, writable stack. This never returns, and nothing of
/// the loader survives it.
unsafe fn enter_kernel(
    args: usize,
    processes: usize,
    rpt: usize,
    xpt: usize,
    satp: usize,
    entry: usize,
    sp: usize,
) -> ! {
    asm!(
        "csrw stvec, {entry}",
        "mv sp, {sp}",
        "sfence.vma",
        "csrw satp, {satp}",
        "unimp",
        entry = in(reg) entry,
        sp = in(reg) sp,
        satp = in(reg) satp,
        in("a0") args,
        in("a1") processes,
        in("a2") rpt,
        in("a3") xpt,
        options(noreturn),
    )
}







fn shutdown() -> ! {
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::SystemFailure);
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("loader64 PANIC: {}", info);
    shutdown()
}

pub struct Console;

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            sbi_rt::console_write_byte(b);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    () => {{ let _ = core::fmt::Write::write_str(&mut $crate::Console, "\n"); }};
    ($($arg:tt)*) => {{
        let _ = core::fmt::Write::write_fmt(&mut $crate::Console, format_args!($($arg)*));
        let _ = core::fmt::Write::write_str(&mut $crate::Console, "\n");
    }};
}
