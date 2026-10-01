//! W^X per frame (kernel/memory.md, R11): device registers and `dma_alloc` pages are never
//! executable. `map_device` and `dma_alloc` map read-write, and `set_flags` refuses `EXECUTE` on
//! either, in this program and in children it gives a DMA device handle to.
//!
//! It runs as the bundle's first program, alone, so it holds every device object and judges the
//! kernel itself (docs/testbench.md, "Rule F (trusted verdicts)"). Each child asks for read-execute
//! on its device mapping or DMA page and then jumps into it. The verdict is the exit notice the
//! kernel writes: an instruction page fault (cause 12). Had `set_flags` granted `EXECUTE`, the DMA
//! child's `ret` would run and it would exit with code 99, and the register child would fetch
//! whatever the registers hold instead of faulting on the page.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Error, MemFlags, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

static UART: AtomicUsize = AtomicUsize::new(0);

/// RISC-V `ret`.
const RET: u32 = 0x0000_8067;
/// The instruction page fault cause.
const FETCH_FAULT: u32 = 12;
/// The child's slot for the DMA device handle.
const CHILD_DEVICE: u32 = 1;

fn rx() -> MemFlags { MemFlags::READ | MemFlags::EXECUTE }

/// Jump to `at`. The verdict is what the kernel does with the fetch.
fn jump(at: usize) {
    // SAFETY: deliberately hostile execution of device or DMA memory; the kernel must fault it.
    let f: extern "C" fn() = unsafe { core::mem::transmute(at as *const u8) };
    f();
}

/// A child: role 1 maps the device's registers, role 2 allocates a DMA page and writes `ret`
/// into it. Either asks for read-execute, then jumps into the page. It exits 99 only if the
/// fetch ran, and 1 if its setup failed.
extern "C" fn attack(arg: usize) -> ! {
    let at = match spawn::startup_byte(arg, 0) {
        1 => rd::map_device(CHILD_DEVICE).map(|(at, _)| at),
        _ => rd::dma_alloc(CHILD_DEVICE, 1).map(|(at, _)| {
            // SAFETY: a DMA page this child has just been given, mapped read-write.
            unsafe { (at as *mut u32).write_volatile(RET) };
            at
        }),
    };
    let Ok(at) = at else { rd::process_exit(1) };
    rd::set_flags(at, rd::PAGE_SIZE, rx()).ok();
    jump(at);
    rd::process_exit(99)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();

    // A DMA-capable device: an empty virtio-mmio slot, with nothing behind it to program.
    let Some((dev, dma)) =
        (rd::OTHER_DEVICES..rd::first_free()).find_map(|h| rd::dma_alloc(h, 1).ok().map(|(at, _)| (h, at)))
    else {
        panic!("no device carries the DMA flag")
    };
    let (regs, _) = rd::map_device(dev).expect("the DMA device's registers");
    let (again, _) = rd::map_device(dev).expect("a second mapping of the same registers");

    // Both mappings are read-write: a write and a read-back of the DMA page, and a read of the
    // registers, which a mapping without read would fault.
    // SAFETY: `dma` is a page `dma_alloc` just mapped for this process.
    unsafe { (dma as *mut u32).write_volatile(RET) };
    // SAFETY: as above.
    let back = unsafe { (dma as *const u32).read_volatile() };
    // SAFETY: `regs` is the device's first register page, mapped for this process by the kernel.
    let _magic = unsafe { (regs as *const u32).read_volatile() };
    assert_eq!(back, RET, "the DMA page did not keep a write");
    writeln!(out, "[device-exec] ok: map_device and dma_alloc map read-write").ok();

    // Executable is refused on each, including after the writable permission is dropped, and on
    // the second mapping of the same registers; a read-only change is not.
    for (what, at) in [("registers", regs), ("second mapping", again), ("DMA page", dma)] {
        assert_eq!(rd::set_flags(at, rd::PAGE_SIZE, rx()), Err(Error::InvalidArgument), "{what}: R+X");
        assert_eq!(rd::set_flags(at, rd::PAGE_SIZE, MemFlags::READ), Ok(()), "{what}: R");
        assert_eq!(
            rd::set_flags(at, rd::PAGE_SIZE, rx()),
            Err(Error::InvalidArgument),
            "{what}: R+X after R"
        );
        assert_eq!(rd::set_flags(at, rd::PAGE_SIZE, rd::rw()), Ok(()), "{what}: back to R+W");
    }
    // The same request on ordinary RAM is granted: the refusal is about the frame, not the call.
    let anon = rd::map_anon(rd::PAGE_SIZE, MemFlags::READ).expect("an ordinary page");
    assert_eq!(rd::set_flags(anon, rd::PAGE_SIZE, rx()), Ok(()), "RAM: R+X");
    writeln!(out, "[device-exec] ok: set_flags refuses execute on device registers and DMA pages").ok();

    // Children that hold the device: each fetch must fault, as the kernel reports it.
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let budget = rd::create(rd::USERS, &rd::spec(500, 2, 50)).expect("a budget for the children");
    for (role, what) in [(1u8, "registers"), (2, "DMA page")] {
        spawn::spawn(&image, budget, exit, attack as *const () as usize, &[role], &[dev]).expect("a child");
        let Ok(Received::Exit(n)) = rd::receive(Some(exit), 2_000_000, 0) else { panic!("no notice") };
        assert_eq!((n.cause, n.code), (Cause::Faulted, FETCH_FAULT), "{what}: the child's end");
        writeln!(out, "[device-exec] ok: a fetch by a child from its {what} faulted with cause 12").ok();
    }
    writeln!(out, "[device-exec] DEVICE EXEC REFUSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: this program mapped the UART and has stopped normal execution.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[device-exec] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
