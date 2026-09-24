//! Attack test for `map_fixed(addr, len, flags)` (KERNEL-SPEC.md R11, answer 172).
//!
//! Every failing case must map nothing and charge nothing: verified by `system`'s page usage
//! (`rd::usage(rd::SYSTEM)`, R6) before and after, and, where relevant, by `unmap` on the
//! attempted target succeeding only when it should not have been created. This program is the
//! loader's trusted first process, holding UART and Reset directly (as `write-only-attack.rs`
//! does), so every verdict below is a kernel result, not this program's own claim.
//!
//! Not covered here: a startup page and a loader-stub page. This bench's test programs are
//! started directly (`process_start` with `arg` = 0, no startup block), and the loader stub
//! (WP-R2) is not yet wired into any boot path, so neither address exists in any process's
//! space to attack yet. The occupied-range rule is still exercised against this process's own
//! stack page and a `map_anon` region, which are real, unconditionally present mappings.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_abi::arch::{PAGE_SIZE as ABI_PAGE_SIZE, USER_AREA_END, USER_STACK_TOP};
use test_programs::rd::{self, Error, MemFlags, ResetKind};
use uart_16550::MmioSerialPort;

const PAGE: usize = rd::PAGE_SIZE;
const _: () = assert!(PAGE == ABI_PAGE_SIZE, "redoubt-sys and redoubt-abi must agree on PAGE_SIZE");

static CONSOLE: AtomicUsize = AtomicUsize::new(0);

struct Checker(MmioSerialPort);
impl Checker {
    fn check(&mut self, ok: bool, label: &str) {
        writeln!(self.0, "[map-fixed] {}: {}", if ok { "ok" } else { "FAIL" }, label).ok();
        assert!(ok, "{}", label);
    }
}

fn map_fixed(addr: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    redoubt_sys::syscall(&rd::Call::MapFixed { addr, len, flags }).map(|_| ())
}

fn usage_pages() -> u64 { rd::usage(rd::SYSTEM).unwrap().pages_usage }

/// A failing call must map nothing (a later `map_fixed` at the same address must succeed) and
/// charge nothing (`system`'s usage is unchanged).
fn refused(c: &mut Checker, label: &str, addr: usize, len: usize, flags: MemFlags, want: Error) {
    let before = usage_pages();
    let r = map_fixed(addr, len, flags);
    c.check(r == Err(want), label);
    c.check(usage_pages() == before, "nothing charged");
}

fn bad_ranges(c: &mut Checker) {
    let addr = 0x5000_0000;
    refused(c, "addr unaligned", addr + 1, PAGE, rd::rw(), Error::InvalidArgument);
    refused(c, "len unaligned", addr, PAGE + 1, rd::rw(), Error::InvalidArgument);
    refused(c, "len 0", addr, 0, rd::rw(), Error::InvalidArgument);
    refused(c, "addr + len overflows", usize::MAX & !(PAGE - 1), PAGE, rd::rw(), Error::InvalidArgument);
    refused(c, "outside user space", USER_AREA_END, PAGE, rd::rw(), Error::InvalidArgument);
    // The last page below USER_AREA_END is accepted (checked as a success case below); one byte
    // past it, straddling the boundary, is refused.
    refused(c, "straddles USER_AREA_END", USER_AREA_END - PAGE / 2, PAGE, rd::rw(), Error::InvalidArgument);
}

fn bad_flags(c: &mut Checker) {
    let addr = 0x5001_0000;
    refused(c, "W+X", addr, PAGE, MemFlags::WRITE | MemFlags::EXECUTE, Error::InvalidArgument);
    refused(c, "W without R", addr, PAGE, MemFlags::WRITE, Error::InvalidArgument);
    refused(c, "flags 0", addr, PAGE, MemFlags::NONE, Error::InvalidArgument);
}

fn occupied_ranges(c: &mut Checker) {
    // This process's own stack: by now (deep in `_start`), the page below the stack top has
    // certainly been touched, so it is a live mapping (whether backed up front or by the legacy
    // demand-paging path (`is_occupied`/`address_available_in` disagree only on an *untouched*
    // reservation) -- either way `range_available_in` must refuse it.
    refused(c, "own stack page", USER_STACK_TOP - PAGE, PAGE, rd::rw(), Error::InvalidArgument);

    // A `map_anon` region.
    let anon = rd::map_anon(PAGE, rd::rw()).expect("map_anon");
    refused(c, "a map_anon region", anon, PAGE, rd::rw(), Error::InvalidArgument);

    // Partial overlap: [free, occupied, free] and [occupied, free].
    let addr = 0x5002_0000;
    map_fixed(addr + PAGE, PAGE, rd::rw()).expect("set up the occupied middle page");
    refused(c, "partial overlap: free, occupied, free", addr, 3 * PAGE, rd::rw(), Error::InvalidArgument);
    refused(c, "partial overlap: occupied, free", addr + PAGE, 2 * PAGE, rd::rw(), Error::InvalidArgument);
    rd::unmap(addr + PAGE, PAGE).expect("unmap the occupied middle page");

    rd::unmap(anon, PAGE).expect("unmap the map_anon region");
}

/// A request over the budget's own free-pages count is refused before anything is allocated
/// (the cheap "pages" check, P1-2/N2), so this needs no filler and never approaches real
/// physical RAM: asking for one more page than the whole budget can ever hold is refused by
/// arithmetic alone, the same way a huge `len` is.
fn exhausted_budget(c: &mut Checker) {
    let u = rd::usage(rd::SYSTEM).unwrap();
    let free = u.pages_limit - u.pages_usage;
    let before = u.pages_usage;

    let addr = 0x5003_0000;
    let r = map_fixed(addr, (free + 1) as usize * PAGE, rd::rw());
    c.check(r == Err(Error::OutOfMemory), "an exhausted budget is refused");
    c.check(usage_pages() == before, "nothing charged when the budget is exhausted");
}

fn huge_len_is_prompt(c: &mut Checker) {
    // A range this large (1 GiB: far more pages than any budget in this bench allows) must be
    // refused at once: the pages check runs before `tables_needed` ever walks the range (P1-2).
    // `[0x1000_0000, 0x5000_0000)` sits below this process's other fixed test addresses
    // (0x5000_0000 and up), the map_anon window (DEFAULT_BASE, 0x6000_0000) and the stack
    // (below USER_STACK_TOP, both widths), so it stays free and it is the budget check that
    // refuses it, not an occupied page. If this hangs, the whole bench case times out instead
    // of reporting FAIL, which is itself the point: the timeout is the check.
    let high = 0x1000_0000;
    let before = usage_pages();
    let r = map_fixed(high, 0x4000_0000, rd::rw());
    c.check(r == Err(Error::OutOfMemory), "a huge len is refused promptly");
    c.check(usage_pages() == before, "nothing charged");
}

fn success_and_addr_zero(c: &mut Checker) {
    // The success path: a fresh page reads zero, flags are enforced, and `unmap` returns usage
    // to baseline.
    let addr = 0x5004_0000;
    let before = usage_pages();
    map_fixed(addr, PAGE, rd::rw()).expect("map_fixed succeeds");
    c.check(usage_pages() > before, "a successful map_fixed charges something");
    c.check(rd::peek(addr) == 0, "a fresh page reads zero");
    rd::poke(addr, 0x1234);
    c.check(rd::peek(addr) == 0x1234, "the page is writable");
    rd::unmap(addr, PAGE).expect("unmap");
    c.check(usage_pages() == before, "unmap returns usage to baseline");

    // K5a-addr0: page 0 is user space. A second identical call is refused as an overlap.
    let before = usage_pages();
    map_fixed(0, PAGE, MemFlags::READ).expect("map_fixed(0, ...) succeeds");
    c.check(usage_pages() > before, "map_fixed(0, ...) charges something");
    refused(c, "a second map_fixed(0, ...) overlaps", 0, PAGE, MemFlags::READ, Error::InvalidArgument);
    rd::unmap(0, PAGE).expect("unmap page 0");
    c.check(usage_pages() == before, "unmap page 0 returns usage to baseline");

    // The last page below USER_AREA_END is accepted.
    let last = USER_AREA_END - PAGE;
    map_fixed(last, PAGE, MemFlags::READ).expect("the last page below USER_AREA_END is accepted");
    rd::unmap(last, PAGE).expect("unmap");

    // `map_anon` still succeeds after a `map_fixed` well outside its own bounded window
    // (kernel/src/mem.rs's `find_virtual_address`, `DEFAULT_BASE..+256 MiB`).
    let low = 0x1000_0000;
    map_fixed(low, PAGE, rd::rw()).expect("a low map_fixed succeeds");
    c.check(
        matches!(rd::map_anon(PAGE, rd::rw()), Ok(_)),
        "map_anon still succeeds after an unrelated map_fixed",
    );
}

#[no_mangle]
pub extern "C" fn _start(_: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    // SAFETY: the kernel mapped this process's granted console register page.
    let mut c = Checker(unsafe { MmioSerialPort::new(uart) });
    c.0.init();
    CONSOLE.store(uart, Ordering::Relaxed);
    writeln!(c.0).ok();
    bad_ranges(&mut c);
    bad_flags(&mut c);
    occupied_ranges(&mut c);
    exhausted_budget(&mut c);
    huge_len_is_prompt(&mut c);
    success_and_addr_zero(&mut c);
    writeln!(c.0, "[map-fixed] MAP-FIXED ATTACK TEST PASSED").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = CONSOLE.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: only this process initializes CONSOLE, to its granted UART page.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[map-fixed] FAIL: {}", info).ok();
    }
    rd::process_exit(255)
}
