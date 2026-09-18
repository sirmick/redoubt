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
mod image;
mod paging;

use core::arch::{asm, global_asm};
use core::ops::Range;

use fdt::Fdt;
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

    // SAFETY: the SBI boot protocol passes the address of a device tree blob in `a1`. The
    // parser validates the header and never reads past the size it declares.
    let fdt = unsafe { Fdt::from_ptr(dtb as *const u8) }.expect("invalid device tree");
    let ram = main_memory(&fdt).expect("device tree describes no memory");
    let bundle = initrd(&fdt).expect("no boot bundle: pass one with -initrd");
    println!("  model: {}, {} hart(s)", fdt.root().model(), fdt.cpus().count());
    println!("  ram: {:#x}..{:#x} ({} MiB)", ram.start, ram.end, ram.len() >> 20);
    println!("  bundle: {:#x}..{:#x}", bundle.start, bundle.end);

    let firmware = ram.start..(&raw const _start) as usize;
    let loader = firmware.end..(&raw const _loader_end) as usize;
    let dtb = dtb..dtb + fdt.total_size();

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

    let extra_pages: usize = extra_regions(&fdt, &ram).map(|r| r.range.len().div_ceil(PAGE_SIZE)).sum();
    let xpt = alloc.alloc_contiguous(extra_pages.div_ceil(PAGE_SIZE).max(1), KERNEL_PID);

    let args_base = alloc.alloc_contiguous(ARGS_PAGES, KERNEL_PID);
    // SAFETY: a fresh, zeroed, page-aligned allocation of exactly this size, referenced
    // from nowhere else.
    let args_buffer =
        unsafe { core::slice::from_raw_parts_mut(args_base as *mut u32, ARGS_PAGES * PAGE_SIZE / 4) };
    let mut args = args::ArgsBuilder::new(args_buffer);
    args.begin(b"MREx");
    for region in extra_regions(&fdt, &ram) {
        args.word64(region.range.start as u64);
        args.word64(region.range.len().next_multiple_of(PAGE_SIZE) as u64);
        args.word(u32::from_le_bytes(region.name));
        args.word(0);
    }
    args.end();

    match plic(&fdt, hart_id) {
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

    // Entropy for the kernel's RNG. Server IDs are drawn from it, and knowing a server
    // ID is what allows a process to connect, so this must not be guessable.
    match fdt.find_node("/chosen").and_then(|chosen| chosen.property("rng-seed")) {
        Some(seed) if seed.value.len() >= 16 => {
            println!("  rng-seed: {} bytes", seed.value.len());
            args.begin(b"Seed");
            args.bytes(seed.value);
            args.end();
        }
        // Fail closed: the kernel has no other source of entropy at boot.
        _ => panic!("the device tree has no usable /chosen/rng-seed"),
    }

    // Ticks per second of the `time` CSR, for whoever ends up driving the hart timer.
    if let Some(timebase) = fdt.find_node("/cpus").and_then(|cpus| cpus.property("timebase-frequency")) {
        args.begin(b"Time");
        args.word64(timebase.as_usize().unwrap_or(0) as u64);
        args.end();
    }

    // The table of initial processes, kernel first. One page bounds how many there can be.
    // SAFETY: a fresh, zeroed, page-aligned allocation referenced from nowhere else.
    // `InitialProcess` is four `usize`s, for which all-zeroes is valid.
    let processes: &mut [InitialProcess] = unsafe {
        let page = alloc.alloc(KERNEL_PID) as *mut InitialProcess;
        core::slice::from_raw_parts_mut(page, PAGE_SIZE / core::mem::size_of::<InitialProcess>())
    };
    // SAFETY: the firmware placed the bundle at this range (from the device tree), it is
    // reserved in the allocator so nothing overwrites it, and it is only read.
    let bundle = unsafe { core::slice::from_raw_parts(bundle.start as *const u8, bundle.len()) };
    let archive = TarArchiveRef::new(bundle).expect("boot bundle is not a tar archive");
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
        let pid = count as Pid + 1;
        let name = entry.filename();
        let name = name.as_str().unwrap_or("?");

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

/// The first region of the first node with `device_type = "memory"`. (The `fdt` crate's
/// `memory()` helper looks the node up by name and panics if that fails, which it does
/// with the device tree RustSBI hands over.)
fn main_memory(fdt: &Fdt) -> Option<Range<usize>> {
    let node = fdt
        .all_nodes()
        .find(|node| node.property("device_type").and_then(|p| p.as_str()) == Some("memory"))?;
    let region = node.reg()?.next()?;
    let start = region.starting_address as usize;
    Some(start..start + region.size?)
}

/// The boot bundle's location, from `/chosen`. The properties are one or two cells wide
/// depending on who wrote the device tree.
fn initrd(fdt: &Fdt) -> Option<Range<usize>> {
    let chosen = fdt.find_node("/chosen")?;
    let cell = |name| {
        let value = chosen.property(name)?.value;
        Some(value.iter().fold(0usize, |acc, byte| acc << 8 | *byte as usize))
    };
    Some(cell("linux,initrd-start")?..cell("linux,initrd-end")?)
}

struct ExtraRegion {
    range: Range<usize>,
    name: [u8; 4],
}

/// Every memory-mapped device in the device tree. The kernel lets processes claim pages
/// inside these regions (and nowhere else outside RAM), and tracks who owns them.
fn extra_regions<'a>(fdt: &'a Fdt<'a>, ram: &'a Range<usize>) -> impl Iterator<Item = ExtraRegion> + 'a {
    fdt.all_nodes()
        .filter(|node| node.property("device_type").and_then(|p| p.as_str()) != Some("memory"))
        .flat_map(|node| {
            let mut name = *b"    ";
            let len = node.name.len().min(4);
            name[..len].copy_from_slice(&node.name.as_bytes()[..len]);
            node.reg().into_iter().flatten().map(move |reg| (name, reg))
        })
        .filter_map(move |(name, reg)| {
            let start = reg.starting_address as usize;
            let size = reg.size.filter(|size| *size > 0)?;
            let outside_ram = start >= ram.end || start + size <= ram.start;
            outside_ram.then(|| ExtraRegion { range: start..start + size, name })
        })
}

struct PlicInfo {
    range: Range<usize>,
    /// Index of the (hart, S-mode) context that external interrupts for `hart_id` arrive on.
    context: usize,
}

/// Locate the PLIC and the S-mode context wired to `hart_id`.
///
/// `interrupts-extended` on the PLIC is a list of (hart interrupt controller phandle,
/// hart interrupt number) pairs, one per context, in context order. Supervisor external
/// interrupt is number 9.
fn plic(fdt: &Fdt, hart_id: usize) -> Option<PlicInfo> {
    const SUPERVISOR_EXTERNAL: u32 = 9;

    let node = fdt.find_compatible(&["sifive,plic-1.0.0", "riscv,plic0"])?;
    let reg = node.reg()?.next()?;
    let start = reg.starting_address as usize;

    // The phandle of the interrupt controller nested inside this hart's cpu node.
    let hart_intc = fdt.find_all_nodes("/cpus/cpu").find_map(|cpu| {
        let id = cpu.reg()?.next()?.starting_address as usize;
        let intc = cpu.children().find(|child| child.name.starts_with("interrupt-controller"))?;
        (id == hart_id).then(|| intc.property("phandle")?.as_usize())?
    })? as u32;

    let contexts = node.property("interrupts-extended")?.value.chunks_exact(8);
    let context = contexts.enumerate().find_map(|(index, pair)| {
        let phandle = u32::from_be_bytes(pair[..4].try_into().unwrap());
        let irq = u32::from_be_bytes(pair[4..].try_into().unwrap());
        (phandle == hart_intc && irq == SUPERVISOR_EXTERNAL).then_some(index)
    })?;

    Some(PlicInfo { range: start..start + reg.size?, context })
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
