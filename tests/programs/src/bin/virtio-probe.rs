//! Reports which virtio devices QEMU attached, then powers the machine off. The bench's
//! self-checks use it to prove that a case's `disk` and `net` reach the guest: it prints
//! each device's kind and what identifies it (a disk's size, a network card's MAC).
//!
//! It runs as the bundle's first program, so it holds every device object the loader made
//! (INTERIM, kernel `device.rs`), maps each through its handle, and prints through the console
//! it maps itself, as `device-test` does. No virtio slot is reachable by legacy `MapMemory`
//! (WP-K5b). It reads only the virtio-mmio identification registers and the start of each
//! device's configuration space, both of which are plain loads; it drives no queues.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, ResetKind};
use uart_16550::MmioSerialPort;

// Register offsets within a slot (virtio 1.2, section 4.2.2).
const MAGIC: usize = 0x000; // "virt"
const DEVICE_ID: usize = 0x008; // 0 for an empty slot
const CONFIG: usize = 0x100; // device-specific configuration space

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "");

    let mut found = 0;
    // Every MMIO device object; an interrupt or the Reset right is `WrongObject` and skipped.
    for handle in rd::OTHER_DEVICES..rd::log_rx() {
        let Ok((at, len)) = rd::map_device(handle) else { continue };
        let read = |offset: usize| -> u32 {
            // SAFETY: `at` maps `len` bytes of this device's registers, whole pages, and every
            // offset read here is below 0x108, inside the first page; 32-bit aligned, volatile.
            unsafe { ((at + offset) as *const u32).read_volatile() }
        };
        if len < rd::PAGE_SIZE || read(MAGIC) != u32::from_le_bytes(*b"virt") {
            continue;
        }
        match read(DEVICE_ID) {
            0 => continue,
            1 => {
                let (low, high) = (read(CONFIG).to_le_bytes(), read(CONFIG + 4).to_le_bytes());
                say!(
                    out,
                    "[virtio] handle {}: network device, mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    handle,
                    low[0],
                    low[1],
                    low[2],
                    low[3],
                    high[0],
                    high[1]
                );
            }
            2 => {
                let sectors = read(CONFIG) as u64 | (read(CONFIG + 4) as u64) << 32;
                say!(out, "[virtio] handle {}: block device, {} sectors", handle, sectors);
            }
            other => say!(out, "[virtio] handle {}: device type {}", handle, other),
        }
        found += 1;
    }
    say!(out, "[virtio] {} device(s); powering off", found);

    // Stop here rather than idle: a case expecting a device that is missing fails at once
    // instead of at its timeout, and `poweroff = true` cases get a clean end to check.
    rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
    say!(out, "[virtio] FAIL: system_reset returned");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
