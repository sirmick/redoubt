//! User-mode access to the mapped kernel half (`tests/kernel-half-attack.toml`;
//! kernel/memory-layout.md, "The split by root entry": no kernel-half entry has `U`).
//!
//! At each of four kernel-half addresses mapped in every address space, a child makes the
//! accesses the mapping itself permits, so that each would succeed if the entry had `U`: only
//! the missing `U` can make them fault. The addresses come from `redoubt_layout`:
//! - the physmap over the start of RAM, `physmap_virt(RAM)` (read-write): load and store;
//! - the kernel area's kernel stack, its top page (read-write): load and store;
//! - the child's own per-process context page, `PROCESS_AREA` (read-write), where the kernel saves its
//!   registers: load (it must fault, not read them) and store;
//! - the kernel's code, `KERNEL_TEXT` (read and execute): load and fetch.
//!
//! The verdict for each access is the kernel's exit notice for the child: `Faulted` with the
//! RISC-V cause, 13 for a load, 15 for a store and 12 for a fetch. The child exits with code 1
//! only if the access returned; a control child loads from its own image and must exit so, which
//! shows a child reaches its access and does not fault before it. A spawned child is a copy of
//! this image, statics and all, so no child uses `Logger`.

#![no_std]
#![no_main]

use redoubt_layout::{KERNEL_STACK_TOP, KERNEL_TEXT, PROCESS_AREA, physmap_virt};
use test_programs::rd::{self, Cause, Received};
use test_programs::{Logger, checker, log, spawn};

/// The start of RAM on QEMU `virt`, on both widths: always inside the physmap.
const RAM: usize = 0x8000_0000;

const LOAD: u8 = 0;
const STORE: u8 = 1;
const FETCH: u8 = 2;

const WAIT: u64 = 2_000_000;

/// The child: the startup block holds the address (8 bytes, little-endian) and the access.
extern "C" fn access(arg: usize) -> ! {
    let at = (0..8).fold(0u64, |v, i| v | (spawn::startup_byte(arg, i) as u64) << (8 * i)) as usize;
    match spawn::startup_byte(arg, 8) {
        LOAD => {
            // SAFETY: none needed by this program: the load is meant to fault, and the kernel's
            // verdict is the point.
            let _ = unsafe { (at as *const usize).read_volatile() };
        }
        STORE => {
            // SAFETY: as above, for a store.
            unsafe { (at as *mut usize).write_volatile(0) };
        }
        _ => {
            // SAFETY: as above: the jump leaves the program for good.
            unsafe { core::arch::asm!("jr {0}", in(reg) at, options(noreturn)) }
        }
    }
    rd::process_exit(1)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let kids = rd::create(rd::GIVEN, &rd::spec(400, 2, 10)).expect("the children's budget");
    let image = spawn::image();
    let entry = access as *const () as usize;
    // One child per access; its exit notice, from the kernel, as (cause, code).
    let mut run = |at: usize, op: u8| {
        let mut startup = [0u8; 9];
        startup[..8].copy_from_slice(&(at as u64).to_le_bytes());
        startup[8] = op;
        spawn::spawn(&image, kids, exit, entry, &startup, &[]).expect("a child");
        match rd::receive(Some(exit), WAIT, 0) {
            Ok(Received::Exit(n)) => Some((n.cause, n.code)),
            _ => None,
        }
    };
    let returned = run(spawn::IMAGE_BASE, LOAD) == Some((Cause::Exited, 1));
    log!(logger, "[kernel-half] control: a load of its own image returned: {}", returned);
    for (name, at, ops) in [
        ("the physmap over RAM", physmap_virt(RAM), [LOAD, STORE]),
        ("the kernel stack", KERNEL_STACK_TOP - rd::PAGE_SIZE, [LOAD, STORE]),
        ("its own context page", PROCESS_AREA, [LOAD, STORE]),
        ("the kernel text", KERNEL_TEXT, [LOAD, FETCH]),
    ] {
        for op in ops {
            let (what, cause) = match op {
                LOAD => ("load", 13),
                STORE => ("store", 15),
                _ => ("fetch", 12),
            };
            if run(at, op) == Some((Cause::Faulted, cause)) {
                log!(logger, "[kernel-half] a {} at {} faulted with cause {}", what, name, cause);
            } else {
                log!(logger, "[kernel-half] FAIL: a {} at {} did not fault with cause {}", what, name, cause);
            }
        }
    }
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
