//! A data entry read back from the bundle's pages (docs/testbench.md, "Bundle files"): the
//! loader starts only the kernel and the program in `init`'s place, so a `[[file]]` entry is data
//! that reaches the guest signed, and this program, in that place, finds it in the bundle mapped
//! at `a0`/`a1` and compares it, byte for byte, with the file the case injected, which it was
//! built with. It holds the console and the Reset right, and ends the case with a power-off.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::bundle::Bundle;
use test_programs::console::{self, Console};
use test_programs::rd;

/// The entry's name, and the bytes the case packs under it (`tests/bench-bundle-file.toml`).
const NAME: &[u8] = b"data";
static INJECTED: &[u8] = include_bytes!("../../../data/bundle-file.txt");

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    // SAFETY: the loader mapped the whole verified bundle here, read-only, `len` bytes from
    // `bundle`, for as long as this program runs (kernel/boot.md).
    let bundle = unsafe { Bundle::at(bundle, len) };
    let names = bundle.map_or(0, |b| b.entries().count());
    let data = bundle.and_then(|b| b.find(NAME));
    match data {
        Some(entry) if entry.data == INJECTED => {
            writeln!(out, "[bundle-file] ok: the data entry is the injected file, {} bytes", entry.data.len())
                .ok()
        }
        Some(entry) => {
            writeln!(out, "[bundle-file] FAIL: the data entry differs ({} bytes)", entry.data.len()).ok()
        }
        None => writeln!(out, "[bundle-file] FAIL: no data entry among {} entries", names).ok(),
    };
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
