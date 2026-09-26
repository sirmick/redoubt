//! A panic in `netd` stops the device before the process dies (servers/netd.md R57): the
//! runtime's panic handler runs the hook first (`redoubt_rt::start`), and `netd`'s hook writes
//! status 0 to the registers it was armed with and reads it back.
//!
//! On the machine `redoubt-rt`'s `#[panic_handler]` calls `run_panic_hook` before anything else.
//! A host test runs under `std`'s panic machinery instead, so a `std` panic hook stands in for
//! that handler here, calling the same `run_panic_hook`. This is its own test binary because the
//! hook may be set, and runs, once per process.

use redoubt_netd::kernel::{REGS_NEEDED, Regs};
use redoubt_netd::virtio::{reg, status};

#[test]
fn a_panic_resets_the_device() {
    // A running device's registers: status says DRIVER_OK.
    let mut words = vec![0u32; REGS_NEEDED / 4].into_boxed_slice();
    words[reg::STATUS / 4] = status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK;
    let regs = Regs::in_memory(Box::leak(words));

    assert!(regs.arm_panic_reset(), "armed once");
    assert!(!regs.arm_panic_reset(), "and only once");
    // A second mapping cannot re-arm it, nor pair its length with the first one's base: the hook
    // below resets the first registers, whose status it reads back as 0.
    let other = Regs::in_memory(Box::leak(vec![7u32; 1].into_boxed_slice()));
    assert!(!other.arm_panic_reset());
    assert_ne!(regs.read_register(reg::STATUS), Ok(0), "arming alone touches nothing");

    std::panic::set_hook(Box::new(|_| redoubt_rt::start::run_panic_hook()));
    let panicked = std::panic::catch_unwind(|| panic!("a bug in netd"));
    let _ = std::panic::take_hook();

    assert!(panicked.is_err());
    assert_eq!(regs.read_register(reg::STATUS), Ok(0), "the panic reset the device");
}
