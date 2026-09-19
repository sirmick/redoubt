//! Reports which virtio devices QEMU attached, then powers the machine off. The bench's
//! self-checks use it to prove that a case's `disk` and `net` reach the guest: it prints
//! each device's kind and what identifies it (a disk's size, a network card's MAC).
//!
//! It reads only the virtio-mmio identification registers and the start of each device's
//! configuration space, both of which are plain loads; it drives no queues. Needs grants for
//! the eight virtio-mmio slots and the power-off device.

#![no_std]
#![no_main]

use test_programs::{log, Logger};
use xous::{MemoryAddress, MemoryFlags};

/// QEMU `virt`'s virtio-mmio transports: eight slots of 0x1000 bytes. A real driver would
/// read these from the device tree. QEMU fills them from the top down: the first `-device`
/// on its command line (the bench adds the disk before the network card) takes the highest
/// slot, so this program, scanning upwards, reports the network card first.
const VIRTIO_BASE: usize = 0x1000_1000;
const VIRTIO_SLOTS: usize = 8;
/// QEMU `virt`'s test device ("sifive,test0"): writing 0x5555 powers off.
const POWEROFF: usize = 0x0010_0000;

// Register offsets within a slot (virtio 1.2, section 4.2.2).
const MAGIC: usize = 0x000; // "virt"
const DEVICE_ID: usize = 0x008; // 0 for an empty slot
const CONFIG: usize = 0x100; // device-specific configuration space

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let virtio = xous::map_memory(
        MemoryAddress::new(VIRTIO_BASE),
        None,
        VIRTIO_SLOTS * 0x1000,
        MemoryFlags::R | MemoryFlags::W,
    )
    .expect("couldn't map the virtio slots");
    let read = |slot: usize, offset: usize| -> u32 {
        // SAFETY: `virtio` maps all eight slots, and slot * 0x1000 + offset stays inside it
        // (offset <= 0x104); the registers are 32-bit aligned device memory, read volatile.
        unsafe { (virtio.as_ptr() as *const u32).add((slot * 0x1000 + offset) / 4).read_volatile() }
    };

    let mut found = 0;
    for slot in 0..VIRTIO_SLOTS {
        let address = VIRTIO_BASE + slot * 0x1000;
        if read(slot, MAGIC) != u32::from_le_bytes(*b"virt") {
            log!(logger, "[virtio] {:#x}: no virtio-mmio transport", address);
            continue;
        }
        match read(slot, DEVICE_ID) {
            0 => continue,
            1 => {
                let (low, high) = (read(slot, CONFIG).to_le_bytes(), read(slot, CONFIG + 4).to_le_bytes());
                log!(
                    logger,
                    "[virtio] {:#x}: network device, mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    address, low[0], low[1], low[2], low[3], high[0], high[1]
                );
            }
            2 => {
                let sectors = read(slot, CONFIG) as u64 | (read(slot, CONFIG + 4) as u64) << 32;
                log!(logger, "[virtio] {:#x}: block device, {} sectors", address, sectors);
            }
            other => log!(logger, "[virtio] {:#x}: device type {}", address, other),
        }
        found += 1;
    }
    log!(logger, "[virtio] {} device(s); powering off", found);

    // Stop here rather than idle: a case expecting a device that is missing fails at once
    // instead of at its timeout, and `poweroff = true` cases get a clean end to check.
    let poweroff = xous::map_memory(MemoryAddress::new(POWEROFF), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't map the power-off device");
    // SAFETY: `poweroff` maps the test device's page; its first register is 32 bits wide.
    unsafe { (poweroff.as_mut_ptr() as *mut u32).write_volatile(0x5555) };
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
