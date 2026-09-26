use core::panic::PanicInfo;

use crate::arch;

#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC in PID {}: {}", crate::arch::current_pid(), _arg);
    // Under an emulator or a test harness, a hung machine is indistinguishable from a
    // slow one. Power off so the failure is visible.
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::SystemFailure);
    loop {
        arch::idle();
    }
}
