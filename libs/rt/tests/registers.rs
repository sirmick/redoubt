//! `Registers::unmap`: a program gives up a device's registers by unmapping them through the one
//! value that reaches them (servers/init.md: `init` unmaps the UART before `consoled` maps it).
//! That nothing can reach them afterwards is the compiler's check (the `compile_fail` example on
//! `Registers::unmap`); this is the call it makes, and the kernel's answer handed back.

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, PAGE_SIZE};
use redoubt_rt::handle::Mmio;

#[test]
fn registers_are_unmapped_through_the_value_with_the_kernels_answer() {
    let f = fake();
    let driver = f.process(0, &[]);
    let (mmio, _irq) = f.device(driver, 4096);
    f.refuse(driver, "unmap", Error::NotPermitted);
    let answer = f.run(driver, move || {
        let registers = Mmio::from_handle(mmio).registers().expect("map_device");
        u32::from(registers.unmap() == Err(Error::NotPermitted))
    });
    assert_eq!(answer.join().unwrap(), 1, "the kernel's refusal is handed back");
    assert_eq!(f.calls(driver), ["map_device", "unmap"]);
}

/// Every access lands inside the mapping `map_device` reported, and one past its end is refused:
/// the keeper of `Registers`' two volatile accesses, run under Miri (`rt-miri`) over the fake's
/// real memory, a page of it, so an access outside the allocation would be Miri's error.
#[test]
fn registers_reach_exactly_their_mapping() {
    let f = fake();
    let driver = f.process(0, &[]);
    let (mmio, _irq) = f.device(driver, PAGE_SIZE);
    f.registers(driver, mmio)[PAGE_SIZE - 1] = 0x5a;
    let answer = f.run(driver, move || {
        let registers = Mmio::from_handle(mmio).registers().expect("map_device");
        assert_eq!(registers.len(), PAGE_SIZE);
        let last = PAGE_SIZE - 1;
        assert_eq!(registers.read_u8(last), Some(0x5a), "the last byte is the device's");
        assert!(registers.write_u8(0, 0xa5) && registers.write_u8(last, 0x3c));
        assert_eq!((registers.read_u8(0), registers.read_u8(last)), (Some(0xa5), Some(0x3c)));
        // One past the end, and far past it, are refused, not reached.
        for beyond in [PAGE_SIZE, PAGE_SIZE + 1, usize::MAX] {
            assert_eq!(registers.read_u8(beyond), None);
            assert!(!registers.write_u8(beyond, 0xff));
        }
        0
    });
    assert_eq!(answer.join().unwrap(), 0);
    let hardware = f.registers(driver, mmio);
    assert_eq!((hardware[0], hardware[PAGE_SIZE - 1]), (0xa5, 0x3c), "the writes reached the device");
}
