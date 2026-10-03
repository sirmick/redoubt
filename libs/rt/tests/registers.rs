//! `Registers::unmap`: a program gives up a device's registers by unmapping them through the one
//! value that reaches them (servers/init.md: `init` unmaps the UART before `consoled` maps it).
//! That nothing can reach them afterwards is the compiler's check (the `compile_fail` example on
//! `Registers::unmap`); this is the call it makes, and the kernel's answer handed back.

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::Error;
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
