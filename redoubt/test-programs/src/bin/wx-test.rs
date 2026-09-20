//! Attacks the W^X rule through the syscall interface. No mapping may ever be writable
//! and executable at once, whatever a process asks for, and a bad request must come back
//! as an error: it must not panic the kernel.

#![no_std]
#![no_main]

use test_programs::{Logger, log};
use xous::{MemoryFlags, SysCall};

/// Issue a raw `MapMemory`, so that flag combinations the typed API might refuse still reach the kernel.
fn map(flags: MemoryFlags) -> Result<xous::Result, xous::Error> {
    xous::rsyscall(SysCall::MapMemory(None, None, xous::MemorySize::new(4096).unwrap(), flags))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();

    let attempts = [
        ("write+execute", MemoryFlags::R | MemoryFlags::W | MemoryFlags::X, false),
        ("write+execute without read", MemoryFlags::W | MemoryFlags::X, false),
        ("no permissions at all", MemoryFlags::empty(), false),
        ("read+write", MemoryFlags::R | MemoryFlags::W, true),
        ("read+execute", MemoryFlags::R | MemoryFlags::X, true),
    ];
    for (what, flags, allowed) in attempts {
        let result = map(flags);
        let ok = result.is_ok() == allowed;
        log!(
            logger,
            "[wx] {}: mapping {} -> {:?}",
            if ok { "ok" } else { "FAIL" },
            what,
            result.map(|_| "mapped")
        );
    }

    // Permissions can be dropped but never added, so a data page cannot become code later.
    let page =
        xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W).expect("couldn't map a page");
    // Touch it first, so that it is a real, backed page rather than a lazy reservation.
    unsafe { page.as_mut_ptr().write_volatile(0x13) };
    let result = xous::update_memory_flags(page, MemoryFlags::R | MemoryFlags::X);
    let ok = result.is_err();
    log!(
        logger,
        "[wx] {}: adding execute to a writable page -> {:?}",
        if ok { "ok" } else { "FAIL" },
        result
    );

    log!(logger, "[wx] attempts done");
    // The verdict is the checker's, not ours (redoubt/README.md, "Writing an attack case").
    test_programs::checker::done();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    test_programs::park()
}
