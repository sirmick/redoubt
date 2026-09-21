//! Ends an attack case from outside the attacker (redoubt/README.md, "Writing an attack
//! case"). Some process reports to it (`test_programs::checker::done()`): a victim once its
//! verdict is in, or, where there is no victim, the attacker once it has made its attempts.
//! The checker then says, under its own PID, which PID reported (as the kernel names it) and
//! that the kernel still delivers messages, and powers the machine off. With `poweroff = true`
//! a case passes only on that clean power-off, so a forged console line alone cannot pass it,
//! and its expected line names the reporter, so a report from anyone else does not count.
//!
//! Where the attacker reports, the checker asserts only that the system survived; whether each
//! attempt was refused stays the attacker's own report, used only as progress.
//! Needs a grant for the power-off device.

#![no_std]
#![no_main]

use test_programs::{checker, log, Logger};
use redoubt_abi::{MemoryAddress, MemoryFlags, Message};

/// QEMU `virt`'s test device ("sifive,test0"): writing 0x5555 powers off.
const POWEROFF: usize = 0x0010_0000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let poweroff = redoubt_abi::map_memory(MemoryAddress::new(POWEROFF), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't map the power-off device");
    let sid = redoubt_abi::create_server_with_address(checker::ADDRESS).expect("couldn't create the checker's server");
    loop {
        let envelope = redoubt_abi::receive_message(sid).expect("couldn't receive");
        let Message::BlockingScalar(m) = &envelope.body else { continue };
        if m.id != checker::DONE {
            continue;
        }
        // The PID comes from the kernel, not from the message: nobody can report as another.
        let reporter = envelope.sender.pid().map_or(0, |pid| pid.get());
        log!(logger, "[checker] PID {} reported; the kernel still serves; powering off", reporter);
        redoubt_abi::return_scalar(envelope.sender, 0).ok();
        // SAFETY: `poweroff` maps the test device's page; its first register is 32 bits wide.
        unsafe { (poweroff.as_mut_ptr() as *mut u32).write_volatile(0x5555) };
        test_programs::park()
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
