//! The legacy interface is gone (`tests/legacy-gone.toml`): every `a0` below the Redoubt call
//! table, the first number above it, and the kernel's private switch tag are refused, from user
//! mode, with `InvalidArgument` in the Redoubt encoding (`a1..=a7` zero), and the caller lives.
//! On rv64 each legacy number is also tried with bit 32 and with bit 63 set, so no decoder may
//! truncate `a0`.
//!
//! The legacy numbers (0..=46) get the arguments their old calls took, so a surviving decoder
//! would act on them. What each would have done is then checked from outside the call's own
//! result:
//! - the `MapMemory` rows name a fixed address `V` (anonymous, then by physical address: the
//!   console, a virtio slot and RAM), and `map_fixed(V)` must succeed afterwards;
//! - the `ClaimInterrupt(10, cb)`, `CreateThread(cb)` and `SetExceptionHandler(cb)` rows pass a
//!   `cb` that sets a flag, which must still be clear after the console's interrupt (the bench
//!   types a byte, which log-server echoes);
//! - a child jumping to the old `RETURN_FROM_ISR` or `RETURN_FROM_EXCEPTION_HANDLER` address
//!   faults, and this program gets its `faulted` exit notice.
//!
//! A spawned child is a copy of this image, statics and all, so no child uses `Logger`.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use test_programs::rd::{self, Cause, Error, Number, Received};
use test_programs::{Logger, checker, log, spawn};

/// The legacy call numbers: `Invalid` (0) to `PlatformSpecific` (46).
const LEGACY: usize = 47;
const MAP_MEMORY: usize = 2;
const CLAIM_INTERRUPT: usize = 5;
const FREE_INTERRUPT: usize = 6;
const SWITCH_TO: usize = 7;
const UPDATE_MEMORY_FLAGS: usize = 12;
const CREATE_THREAD: usize = 18;
const UNMAP_MEMORY: usize = 19;
const TERMINATE_PROCESS: usize = 22;
const SHUTDOWN: usize = 23;
const JOIN_THREAD: usize = 36;
const SET_EXCEPTION_HANDLER: usize = 37;
/// The legacy `MemoryFlags` R | W.
const LEGACY_RW: usize = 0b110;
/// The kernel's private tag for `kmain`'s switch (`kernel/src/sched.rs`, `SWITCH_TAG`). Only an
/// S-mode `ecall` reaches the switch; from user mode it is an unknown number.
const SWITCH_TAG: usize = 0x5357_4954;
/// The console's interrupt on QEMU virt.
const CONSOLE_IRQ: usize = 10;
/// Physical addresses a legacy `MapMemory` once reached: the console, a virtio slot, RAM.
const PHYS: [usize; 3] = [0x1000_0000, 0x1000_1000, 0x8000_0000];
/// The fixed address every `MapMemory` row names: above the image, below a child's startup page.
const V: usize = 0x0e00_0000;

/// Where the kernel's magic return addresses were (`kernel/src/arch/riscv/process.rs`).
#[cfg(target_pointer_width = "32")]
const MAGIC_RETURN_BASE: usize = 0xff80_0000;
#[cfg(target_pointer_width = "64")]
const MAGIC_RETURN_BASE: usize = 0xffff_ffff_8080_0000;
const RETURN_FROM_ISR: usize = MAGIC_RETURN_BASE + 0x2000;
const RETURN_FROM_EXCEPTION_HANDLER: usize = MAGIC_RETURN_BASE + 0x4000;

const WAIT: u64 = 2_000_000;

/// Set by `cb`, which only a legacy decoder would ever run.
static RAN: AtomicBool = AtomicBool::new(false);

extern "C" fn cb(_: usize) { RAN.store(true, Ordering::Release) }

/// The arguments the legacy call `n` took, as plausible values.
fn legacy_args(n: usize, stack: usize) -> [usize; 7] {
    let cb = cb as *const () as usize;
    match n {
        MAP_MEMORY => [0, V, rd::PAGE_SIZE, LEGACY_RW, 0, 0, 0],
        CLAIM_INTERRUPT => [CONSOLE_IRQ, cb, 0, 0, 0, 0, 0],
        FREE_INTERRUPT => [CONSOLE_IRQ, 0, 0, 0, 0, 0, 0],
        // log-server's first thread.
        SWITCH_TO => [2, 2, 0, 0, 0, 0, 0],
        UPDATE_MEMORY_FLAGS => [V, rd::PAGE_SIZE, LEGACY_RW, 0, 0, 0, 0],
        CREATE_THREAD => [cb, stack, rd::PAGE_SIZE, 0, 0, 0, 0],
        UNMAP_MEMORY => [V, rd::PAGE_SIZE, 0, 0, 0, 0, 0],
        TERMINATE_PROCESS => [0; 7],
        JOIN_THREAD => [3, 0, 0, 0, 0, 0, 0],
        SET_EXCEPTION_HANDLER => [cb, stack + rd::PAGE_SIZE, 0, 0, 0, 0, 0],
        _ => [1, 0, 0, 0, 0, 0, 0],
    }
}

/// Whether `a0` with `args` is refused as an unknown number, in the Redoubt encoding.
fn refused(a0: usize, args: [usize; 7]) -> bool {
    let mut regs = [a0; 8];
    regs[1..].copy_from_slice(&args);
    let out = rd::raw_registers(regs);
    rd::raw_error(out[0]) == Some(Error::InvalidArgument) && out[1..].iter().all(|&r| r == 0)
}

/// A child that jumps to `arg`'s address, which is mapped nowhere.
extern "C" fn jumper(arg: usize) -> ! {
    let to = (0..8).fold(0u64, |at, i| at | (spawn::startup_byte(arg, i) as u64) << (8 * i)) as usize;
    // SAFETY: none needed by this program: the jump leaves it for good, and the kernel's verdict
    // (a fault, reported to the parent) is the point.
    unsafe { core::arch::asm!("jr {0}", in(reg) to, options(noreturn)) }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let gifts = rd::take_gifts().expect("the budgets");
    let stack = rd::map_anon(2 * rd::PAGE_SIZE, rd::rw()).expect("a stack for the thread rows");

    // --- The sweep ----------------------------------------------------------------------------
    let mut swept = 0;
    let mut check = |a0: usize, args: [usize; 7]| {
        swept += 1;
        if !refused(a0, args) {
            log!(logger, "[legacy-gone] FAIL: a0 = {:#x} was not refused as unknown", a0);
        }
    };
    let base = redoubt_sys::NUMBER_BASE as usize;
    for n in 0..=base {
        check(n, if n < LEGACY { legacy_args(n, stack) } else { [0; 7] });
    }
    check(base + Number::ALL.len() + 1, [0; 7]);
    for phys in PHYS {
        check(MAP_MEMORY, [phys, V, rd::PAGE_SIZE, LEGACY_RW, 0, 0, 0]);
    }
    #[cfg(target_pointer_width = "64")]
    for n in 0..LEGACY {
        check(n | 1 << 32, legacy_args(n, stack));
        check(n | 1 << 63, legacy_args(n, stack));
    }
    let private = refused(SWITCH_TAG, [2, 2, 0, 0, 0, 0, 0]);
    log!(logger, "[legacy-gone] {} numbers swept", swept);
    // Named as well, the two most dangerous: a user-mode SwitchTo could assert-panic the kernel,
    // and any process's Shutdown ended every process.
    for (name, n) in [("SwitchTo", SWITCH_TO), ("Shutdown", SHUTDOWN)] {
        log!(logger, "[legacy-gone] {} ({}) -> refused: {}", name, n, refused(n, legacy_args(n, stack)));
    }
    log!(logger, "[legacy-gone] the private switch tag from user mode -> refused: {}", private);

    // --- Nothing the legacy rows asked for happened --------------------------------------------
    let mapped = rd::map_fixed(V, rd::PAGE_SIZE, rd::rw()).is_ok() && rd::peek(V) == 0;
    log!(logger, "[legacy-gone] map_fixed at the MapMemory rows' address -> fresh page: {}", mapped);

    let exit = rd::endpoint_create().expect("an exit endpoint");
    let kids = rd::create(gifts.users, &rd::spec(400, 2, 10)).expect("the children's budget");
    let image = spawn::image();
    for (name, to) in [("RETURN_FROM_ISR", RETURN_FROM_ISR), ("RETURN_FROM_EXCEPTION_HANDLER", RETURN_FROM_EXCEPTION_HANDLER)]
    {
        let entry = jumper as *const () as usize;
        spawn::spawn(&image, kids, exit, entry, &(to as u64).to_le_bytes(), &[]).expect("a jumping child");
        let faulted = match rd::receive(Some(exit), WAIT, 0) {
            Ok(Received::Exit(n)) => n.cause == Cause::Faulted,
            _ => false,
        };
        log!(logger, "[legacy-gone] a child jumping to the old {} faulted: {}", name, faulted);
    }

    // The bench types a byte after this line; log-server's interrupt thread echoes it.
    log!(logger, "[legacy-gone] waiting for the console interrupt");
    test_programs::wait_ms(2000);
    log!(logger, "[legacy-gone] no callback ran: {}", !RAN.load(Ordering::Acquire));

    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
