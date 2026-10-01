//! `device_info` (kernel/devices.md): the kernel says which device a handle names, in the form of
//! the `Devs` entry the object was made from, and refuses every handle that is not a device.
//!
//! It runs as the bundle's first program, so it holds every device object (kernel/boot.md,
//! "Devices handed to the first program"), the Reset right and the console, and it prints through
//! the console it maps itself. The answers are the kernel's: this program only compares them with
//! what QEMU `virt` puts where, and ends the case with `system_reset`, so a verdict needs both its
//! lines and a power-off through the Reset right.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::console::{self, Console};
use test_programs::rd::{self, DeviceInfo, Error, ResetKind};

/// QEMU `virt`'s UART: its registers at 0x1000_0000, one page once the loader rounds the
/// region up to whole pages, and PLIC source 10.
const VIRT_UART: DeviceInfo = DeviceInfo::Mmio { base: 0x1000_0000, size: 0x1000, dma: false };
const VIRT_UART_IRQ: DeviceInfo = DeviceInfo::Irq(10);

/// The PLIC numbers its sources 1 to 1023 (kernel/device.rs, `MAX_IRQS`).
const MAX_IRQS: u32 = 1024;

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out, "[device-info] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out, $($arg)*).ok();
    }};
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "[device-info] mapped the console");
    // The devices end where the log endpoint's receive right is, before anything is added.
    let devices_end = rd::first_free();

    // --- every device handle -------------------------------------------------------------
    // The three the order pins, against QEMU `virt`'s device tree.
    check!(out, rd::device_info(rd::RESET) == Ok(DeviceInfo::Reset), "handle 4 is the Reset right");
    let console = rd::device_info(rd::CONSOLE_MMIO);
    check!(out, console == Ok(VIRT_UART), "the console is virt's UART: {:x?}", console);
    let irq = rd::device_info(rd::CONSOLE_IRQ);
    check!(out, irq == Ok(VIRT_UART_IRQ), "the console's interrupt is virt's: {:x?}", irq);

    // Every other one: MMIO regions in device-tree order, then interrupts ascending, each the
    // shape its `Devs` kind allows.
    let (mut mmio, mut dma, mut irqs, mut last_irq, mut in_irqs) = (0, 0, 0, 0, false);
    let mut shapes = true;
    for h in rd::OTHER_DEVICES..devices_end {
        match rd::device_info(h) {
            Ok(DeviceInfo::Mmio { base, size, dma: d }) if !in_irqs => {
                let page = rd::PAGE_SIZE as u64;
                shapes &=
                    size != 0 && base % page == 0 && size % page == 0 && base.checked_add(size).is_some();
                mmio += 1;
                dma += usize::from(d);
            }
            Ok(DeviceInfo::Irq(n)) => {
                shapes &= n > last_irq && n < MAX_IRQS && Ok(DeviceInfo::Irq(n)) != irq;
                last_irq = n;
                in_irqs = true;
                irqs += 1;
            }
            other => {
                say!(out, "[device-info] FAIL: handle {} answered {:?}", h, other);
                shapes = false;
            }
        }
    }
    check!(
        out,
        shapes && mmio > 0 && irqs > 0,
        "{} other MMIO regions ({} with DMA), then {} interrupts ascending",
        mmio,
        dma,
        irqs
    );

    // --- what is not a device, in the order of checks --------------------------------------
    let endpoint = rd::endpoint_create().expect("an endpoint");
    let closed = rd::endpoint_create().expect("a second endpoint");
    rd::close(closed).expect("close it");
    let free = rd::first_free();
    check!(out, rd::device_info(rd::SYSTEM) == Err(Error::WrongObject), "a budget -> WrongObject");
    check!(out, rd::device_info(endpoint) == Err(Error::WrongObject), "an endpoint -> WrongObject");
    check!(out, rd::device_info(closed) == Err(Error::BadHandle), "a closed handle -> BadHandle");
    check!(
        out,
        rd::device_info(free) == Err(Error::BadHandle) && rd::device_info(u32::MAX) == Err(Error::BadHandle),
        "an index it does not hold -> BadHandle"
    );
    // All ones in the handle register: wider than 32 bits on rv64, refused while it decodes;
    // u32::MAX on rv32, an index no table holds (kernel/abi.md, "Errors and the order of checks").
    let wide = rd::raw_registers([rd::number(rd::Number::DeviceInfo), usize::MAX, 0, 0, 0, 0, 0, 0]);
    check!(
        out,
        rd::raw_error(wide[0]) == Some(Error::BadHandle) && wide[1..].iter().all(|r| *r == 0),
        "all ones in the handle register -> BadHandle, and nothing else in the registers"
    );

    // --- nothing mapped, nothing charged ---------------------------------------------------
    let before = rd::usage(rd::SYSTEM).expect("system usage");
    for h in rd::RESET..devices_end {
        rd::device_info(h).ok();
    }
    let after = rd::usage(rd::SYSTEM).expect("system usage");
    check!(out, after.pages_usage == before.pages_usage, "device_info maps nothing and charges nothing");

    say!(out, "[device-info] DEVICE INFO ATTACK TEST PASSED");
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[device-info] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
