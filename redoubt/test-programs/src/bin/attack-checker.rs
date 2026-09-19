//! Ends an attack case from outside the attacker (redoubt/README.md, "Writing an attack
//! case"). It waits for an attacker to say it has finished its attempts, then reports, under
//! its own PID, that the kernel is still delivering messages and scheduling it, and powers the
//! machine off. With `poweroff = true` the case passes only on that clean power-off with no
//! forbidden line (a kernel panic, a breach an attacker reports) on the way.
//!
//! It asserts that the system survived the attack, which only it can say; whether each attempt
//! was refused is still the attacker's own report, used only to fail a case.
//! Needs a grant for the power-off device.

#![no_std]
#![no_main]

use test_programs::{checker, log, Logger};
use xous::{MemoryAddress, MemoryFlags, Message};

/// QEMU `virt`'s test device ("sifive,test0"): writing 0x5555 powers off.
const POWEROFF: usize = 0x0010_0000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let poweroff = xous::map_memory(MemoryAddress::new(POWEROFF), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't map the power-off device");
    let sid = xous::create_server_with_address(checker::ADDRESS).expect("couldn't create the checker's server");
    loop {
        let envelope = xous::receive_message(sid).expect("couldn't receive");
        let Message::BlockingScalar(m) = &envelope.body else { continue };
        if m.id != checker::DONE {
            continue;
        }
        // The PID comes from the kernel, not from the message: an attacker cannot name another.
        let attacker = envelope.sender.pid().map_or(0, |pid| pid.get());
        log!(logger, "[checker] PID {} finished its attempts; the kernel still serves; powering off", attacker);
        xous::return_scalar(envelope.sender, 0).ok();
        // SAFETY: `poweroff` maps the test device's page; its first register is 32 bits wide.
        unsafe { (poweroff.as_mut_ptr() as *mut u32).write_volatile(0x5555) };
        test_programs::park()
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
