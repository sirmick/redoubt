//! The real loader stub (WP-R2), launched exactly as PACKAGES.md's "Launching a process"
//! describes: a flat binary mapped at a fixed address, a copied ELF image, a startup page naming
//! it. Two cases: a well-formed child (`fixture-child`, `stub/src/bin/fixture-child.rs`) and a
//! hostile one (its ELF header corrupted).
//!
//! **Not registered as a bench case yet** (`docs/WORKSPACE-QA.md`, `R2-stub-self-map`, answer
//! 172, WP-K5a): the stub's mapping call (`map_fixed`) is a shim that always refuses until the
//! kernel implements it, so both cases below currently end the same way -- the child exits with
//! `stub::exit::BAD_IMAGE` (111), never reaching its own entry. That is not yet a meaningful
//! attack-vs-happy-path distinction, so no `tests/*.toml` case names this bin. Once WP-K5a lands:
//! the well-formed case should exit `fixture_child::OK` (77) and the hostile one should still
//! be refused (111, or `Faulted` if a segment slips past `plan`'s checks into a real fault) --
//! update `expect_ok` below and add the two cases then.
#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_rt::startup::StartupBuilder;
use stub::STUB_ENTRY;
use test_programs::rd::{self, Cause, ExitNotice, Received};
use uart_16550::MmioSerialPort;

/// The stub's own flat binary (objcopied by `build.rs`).
static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));
/// A well-formed ELF the stub should map and jump to.
static CHILD_ELF: &[u8] = include_bytes!(env!("STUB_CHILD_ELF"));

const PAGE: usize = 4096;
/// Where this launcher puts the copied program image in the child (page-aligned, clear of
/// `STUB_ENTRY`'s region and the startup page).
const IMAGE_AT: usize = 0x0500_0000;
const STACK_TOP: usize = 0x1000_0000;
const STACK_PAGES: usize = 8;
const STARTUP_AT: usize = 0x0f00_0000;
const WAIT: u64 = 2_000_000;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

#[no_mangle]
pub extern "C" fn _start(_arg: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[stub-launch] starting");

    let well_formed = launch(CHILD_ELF);
    say!(out, "[stub-launch] well-formed child: {}", describe(&well_formed));

    let mut hostile_buf = [0u8; MAX_CHILD_ELF];
    assert!(CHILD_ELF.len() <= MAX_CHILD_ELF, "fixture-child grew past MAX_CHILD_ELF");
    hostile_buf[..CHILD_ELF.len()].copy_from_slice(CHILD_ELF);
    hostile_buf[6] = 0xff; // EI_DATA: corrupts the ELF header itself.
    let hostile = launch(&hostile_buf[..CHILD_ELF.len()]);
    say!(out, "[stub-launch] hostile child: {}", describe(&hostile));

    say!(out, "[stub-launch] done (pending WP-K5a; see module doc)");
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

/// Starts `image` through the real stub, exactly as PACKAGES.md's "Launching a process"
/// describes, and returns its exit notice.
fn launch(image: &[u8]) -> Option<ExitNotice> {
    let exit = rd::endpoint_create().expect("an exit endpoint");
    let process = rd::process_create(rd::SYSTEM, exit).expect("process_create");

    // 3. Map the loader stub, a flat binary, read+exec, at its fixed address.
    let stub_pages = STUB_BIN.len().next_multiple_of(PAGE);
    let scratch =
        rd::map_anon(stub_pages, rd::MemFlags::READ | rd::MemFlags::WRITE).expect("scratch for stub");
    copy_in(scratch, STUB_BIN);
    rd::process_map(process, scratch, STUB_ENTRY, stub_pages, rd::MemFlags::READ | rd::MemFlags::EXECUTE)
        .expect("map the stub");

    // 4. Copy the program's ELF bytes in, read-write, as data.
    let image_pages = image.len().next_multiple_of(PAGE);
    let scratch =
        rd::map_anon(image_pages, rd::MemFlags::READ | rd::MemFlags::WRITE).expect("scratch for image");
    copy_in(scratch, image);
    rd::process_map(process, scratch, IMAGE_AT, image_pages, rd::MemFlags::READ | rd::MemFlags::WRITE)
        .expect("map the image");

    // A stack (`process_start`'s `sp`, the stub's own and then the started program's).
    let stack_len = STACK_PAGES * PAGE;
    let stack_scratch =
        rd::map_anon(stack_len, rd::MemFlags::READ | rd::MemFlags::WRITE).expect("scratch for stack");
    rd::process_map(
        process,
        stack_scratch,
        STACK_TOP - stack_len,
        stack_len,
        rd::MemFlags::READ | rd::MemFlags::WRITE,
    )
    .expect("map the stack");

    // The startup page, naming the image (INIT.md, Startup block; PACKAGES.md step 4).
    let block = StartupBuilder::new(0).image(IMAGE_AT, image.len()).finish().expect("a startup block");
    let page_scratch =
        rd::map_anon(PAGE, rd::MemFlags::READ | rd::MemFlags::WRITE).expect("scratch for startup");
    copy_in(page_scratch, &block);
    rd::process_map(process, page_scratch, STARTUP_AT, PAGE, rd::MemFlags::READ)
        .expect("map the startup page");

    rd::process_start(process, STUB_ENTRY, STACK_TOP - 16, STARTUP_AT, &[]).expect("process_start");

    match rd::receive(Some(exit), WAIT, 0) {
        Ok(Received::Exit(notice)) => Some(notice),
        _ => None,
    }
}

fn copy_in(dst: usize, src: &[u8]) {
    for (i, byte) in src.iter().enumerate() {
        // SAFETY: `dst..dst+src.len()` is memory this process just mapped read-write via
        // `map_anon`; `i < src.len()`.
        unsafe { (dst as *mut u8).add(i).write_volatile(*byte) };
    }
}

fn describe(notice: &Option<ExitNotice>) -> &'static str {
    match notice {
        None => "no notice",
        Some(n) if n.cause == Cause::Exited => "exited",
        Some(n) if n.cause == Cause::Faulted => "faulted",
        Some(_) => "other",
    }
}

/// An upper bound on `fixture-child`'s size, for the one mutable copy this bin needs (no `alloc`
/// here, unlike the stub crate).
const MAX_CHILD_ELF: usize = 4096;

// No local `#[panic_handler]`: this bin depends on `redoubt-rt` (for `StartupBuilder` and the
// `stub` crate), which already provides one (`redoubt_rt::start`), unlike the other bins here
// that stay on the raw `rd` wrapper.
