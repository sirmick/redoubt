//! A UART echo driver, and the first userspace program that ran on xous64.
//!
//! It exercises the path from the loader to U-mode and back: ELF loading, the first
//! context switch, `ecall` traps, claiming device memory that the loader described from
//! the device tree, and interrupt delivery to a userspace handler through the PLIC. It
//! deliberately avoids `std`, which is not ported to rv64 yet.

#![no_std]
#![no_main]

use core::fmt::Write;

use uart_16550::MmioSerialPort;
use xous::{MemoryAddress, MemoryFlags};

// The ns16550 on QEMU's `virt` machine. A real driver would get these from the device tree.
const UART_BASE: usize = 0x1000_0000;
const UART_IRQ: usize = 10;

/// Runs in interrupt context whenever the UART has received data.
fn on_uart_irq(_irq: usize, arg: *mut usize) {
    let port = unsafe { &mut *(arg as *mut MmioSerialPort) };
    while let Ok(byte) = port.try_receive() {
        writeln!(port, "irq {}: received {:?}", UART_IRQ, byte as char).ok();
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let uart =
        xous::map_memory(MemoryAddress::new(UART_BASE), None, 4096, MemoryFlags::R | MemoryFlags::W)
            .expect("couldn't claim the UART");

    // `init()` also enables the UART's "received data available" interrupt.
    let mut port = unsafe { MmioSerialPort::new(uart.as_ptr() as usize) };
    port.init();
    writeln!(port, "hello from userspace! PID {}, UART mapped at {:p}", xous::current_pid().unwrap(), uart.as_ptr())
        .ok();

    xous::claim_interrupt(UART_IRQ, on_uart_irq, &mut port as *mut MmioSerialPort as *mut usize)
        .expect("couldn't claim the UART interrupt");
    writeln!(port, "claimed irq {}, type something", UART_IRQ).ok();

    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
