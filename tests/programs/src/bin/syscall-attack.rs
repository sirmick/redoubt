//! Attack test: syscall arguments that once reached a kernel panic, each of which must now be a
//! clean error. See `tests/syscall-attack.toml`; the case's verdict is survival only,
//! from `attack-checker`, which this program reports to once it is done.

#![no_std]
#![no_main]

use test_programs::{Logger, checker, log, op};
use redoubt_abi::{MemoryFlags, Message, SysCall};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let rw = MemoryFlags::R | MemoryFlags::W;

    // (1) Grow the heap with flags the kernel refuses (W and X), then shrink it. The failed
    // growth once left the heap's size counting pages that were never reserved, and the shrink
    // then `.expect()`ed on unmapping one and halted the machine.
    let grow = redoubt_abi::rsyscall(SysCall::IncreaseHeap(2 * 4096, rw | MemoryFlags::X));
    let shrink = redoubt_abi::rsyscall(SysCall::DecreaseHeap(4096));
    log!(logger, "[syscall] heap grow W+X -> {:?}; shrink -> {:?}", grow, shrink);

    // (2) Lend more than the message region (one 4 MiB superpage) can hold. Finding room for it
    // in the destination computed the region end minus the size, which underflowed.
    let big = redoubt_abi::map_memory(None, None, 5 << 20, rw).expect("couldn't reserve 5 MiB");
    let lend = redoubt_abi::send_message(logger.cid, Message::new_lend(op::PRINT, big, None, None));
    redoubt_abi::unmap_memory(big).expect("couldn't unmap the 5 MiB range");
    log!(logger, "[syscall] oversized lend -> {:?}", lend);

    // Survived every attempt; the checker powers off under its own PID.
    checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
