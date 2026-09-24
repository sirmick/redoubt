//! Attack test for `map_fixed(addr, len, flags)` (KERNEL-SPEC.md R11, answer 172).
//!
//! Every failing case must map nothing and charge nothing: verified by `system`'s page usage
//! (`rd::usage(rd::SYSTEM)`, R6) before and after, and, where relevant, by `unmap` on the
//! attempted target succeeding only when it should not have been created. This program is the
//! loader's trusted first process, holding UART and Reset directly (as `write-only-attack.rs`
//! does), so every verdict below is a kernel result, not this program's own claim. The one
//! verdict it cannot see itself, a fault, comes from the kernel's exit notice for a child (as
//! `wx-test.rs` does).
//!
//! Not covered here: a startup page and a loader-stub page. This bench's test programs are
//! started directly (`process_start` with `arg` = 0, no startup block), and the loader stub
//! (WP-R2) is not yet wired into any boot path, so neither address exists in any process's
//! space to attack yet. The occupied-range rule is still exercised against this process's own
//! stack (touched and untouched), a `map_anon` region and both sides of a lend, which are real,
//! unconditionally present mappings.
#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use redoubt_abi::arch::{PAGE_SIZE as ABI_PAGE_SIZE, USER_AREA_END, USER_STACK_TOP};
use test_programs::rd::{self, Cause, Error, FOREVER, MemFlags, MessageKind, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

const PAGE: usize = rd::PAGE_SIZE;
const _: () = assert!(PAGE == ABI_PAGE_SIZE, "redoubt-sys and redoubt-abi must agree on PAGE_SIZE");

/// The legacy stack reservation the kernel gives a loader-started process: `[sp - 128 KiB, sp)`
/// as reservation PTEs, backed only when touched (kernel/src/services.rs, `DEFAULT_STACK_SIZE`
/// in kernel/src/arch/riscv/process.rs). A copy: the kernel's constant is not exported.
const LEGACY_STACK_SIZE: usize = 128 * 1024;

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
    // The last page below USER_AREA_END is accepted (checked as a success case below); an
    // aligned range starting there and running one page past it is refused whole.
    refused(c, "straddles USER_AREA_END", USER_AREA_END - PAGE, 2 * PAGE, rd::rw(), Error::InvalidArgument);
    // On Sv39 both cases above are also refused by the overlap walk: USER_AREA_END is root slot
    // 256, the physmap every process copies. Root slot 320 is kernel half but empty, so only
    // `user_range`'s end check refuses this one (without it, the kernel panics mapping it).
    #[cfg(target_pointer_width = "64")]
    refused(c, "outside user space, empty root slot", 0x50_0000_0000, PAGE, rd::rw(), Error::InvalidArgument);
}

fn bad_flags(c: &mut Checker) {
    let addr = 0x5001_0000;
    refused(c, "W+X", addr, PAGE, MemFlags::WRITE | MemFlags::EXECUTE, Error::InvalidArgument);
    refused(c, "W without R", addr, PAGE, MemFlags::WRITE, Error::InvalidArgument);
    refused(c, "flags 0", addr, PAGE, MemFlags::NONE, Error::InvalidArgument);
}

fn occupied_ranges(c: &mut Checker) {
    // This process's own stack: by now (deep in `_start`), the page below the stack top has
    // certainly been touched, so it is a live mapping.
    refused(c, "own stack page", USER_STACK_TOP - PAGE, PAGE, rd::rw(), Error::InvalidArgument);
    // The bottom of the legacy stack reservation, which this program never gets near: a
    // reservation PTE, not a mapping. It is the one case where `range_available_in`'s
    // "any nonzero PTE" and `is_occupied` (valid or S) disagree, so it is what fails if the
    // overlap check is ever weakened to `is_occupied`.
    let untouched = USER_STACK_TOP - LEGACY_STACK_SIZE;
    refused(c, "untouched stack reservation", untouched, PAGE, rd::rw(), Error::InvalidArgument);

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

static LEND_ENDPOINT: AtomicUsize = AtomicUsize::new(0);
static LENT_IN_REFUSED: AtomicBool = AtomicBool::new(false);
static LENT_IN_UNCHARGED: AtomicBool = AtomicBool::new(false);
static LENT_OUT_REFUSED: AtomicBool = AtomicBool::new(false);
static LENT_OUT_UNCHARGED: AtomicBool = AtomicBool::new(false);

/// The borrowing thread. While it holds the lend, this one process has both sides of it: the
/// kernel's alias of the page (lent in, an S-marked PTE) and the lender's own page (lent out,
/// its PTE the lender's only record of the loan). `map_fixed` must refuse both. The results go
/// back through statics for the main thread to report, since only it holds the checker.
fn lend_server(_: usize) {
    let endpoint = LEND_ENDPOINT.load(Ordering::Acquire) as u32;
    let Received::Message(m) = rd::receive(Some(endpoint), FOREVER, 0).expect("receive the lend") else {
        panic!("expected the lend")
    };
    let MessageKind::Call { lend: Some(lend) } = m.kind else { panic!("expected a lend") };
    let before = usage_pages();
    LENT_IN_REFUSED
        .store(map_fixed(lend.addr, PAGE, rd::rw()) == Err(Error::InvalidArgument), Ordering::Release);
    LENT_IN_UNCHARGED.store(usage_pages() == before, Ordering::Release);
    let before = usage_pages();
    let lent_out = m.body.words[0];
    LENT_OUT_REFUSED
        .store(map_fixed(lent_out, PAGE, rd::rw()) == Err(Error::InvalidArgument), Ordering::Release);
    LENT_OUT_UNCHARGED.store(usage_pages() == before, Ordering::Release);
    rd::reply(m.msg_id.get(), &rd::body([0; 4])).expect("reply");
    // Block for good rather than exit, so nothing this thread owns changes `system`'s usage
    // under the checks that follow.
    loop {
        rd::receive(Some(endpoint), FOREVER, 0).ok();
    }
}

fn lent_ranges(c: &mut Checker) {
    let endpoint = rd::endpoint_create().expect("endpoint");
    LEND_ENDPOINT.store(endpoint as usize, Ordering::Release);
    redoubt_abi::create_thread_1(lend_server, 0).expect("lend server thread");
    let page = rd::map_anon(PAGE, rd::rw()).expect("map_anon the page to lend");
    rd::poke(page, 42);
    let reply = rd::call(endpoint, &rd::body([page, 0, 0, 0]), rd::pages(page, 1), FOREVER);
    c.check(reply.is_ok() && rd::peek(page) == 42, "the lend came back intact");
    c.check(LENT_IN_REFUSED.load(Ordering::Acquire), "a lent-in page");
    c.check(LENT_IN_UNCHARGED.load(Ordering::Acquire), "nothing charged");
    c.check(LENT_OUT_REFUSED.load(Ordering::Acquire), "a lent-out page");
    c.check(LENT_OUT_UNCHARGED.load(Ordering::Acquire), "nothing charged");
    rd::unmap(page, PAGE).expect("unmap the returned page");
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

/// The "page tables" half of the charge check, which the "pages" half alone never reaches: a
/// range whose pages fit but whose page tables do not.
///
/// The range is `[4 GiB - PAGE, 4 GiB + PAGE)`: the last page of the fourth gigabyte and the
/// first of the fifth, so it crosses a 1 GiB boundary as well as a 2 MiB one. Nothing else in
/// this process lives in either gigabyte (image at 0x1_0000, heap 0x2000_0000, the `map_anon`
/// window 0x6000_0000, the stack below 0x8000_0000: gigabytes 0 and 1; the filler below:
/// gigabyte 2), so on Sv39 each page needs a level-1 table (one per gigabyte, from its root
/// entry) and a level-0 table (one per 2 MiB): 2 pages + 4 tables = 6. (Round 1's review
/// counted 5, for one gigabyte.) That also covers `tables_needed` across root entries.
#[cfg(target_pointer_width = "64")]
fn page_table_charge(c: &mut Checker) {
    const AT: usize = 0x1_0000_0000 - PAGE;
    const PAGES: u64 = 2;
    const TABLES: u64 = 4;
    // Filler: 2 MiB level-0 tables in gigabyte 2, each opened by mapping its first page, then
    // filled a slot at a time. Once a table exists, each page in it costs exactly one page,
    // so the budget can be brought to an exact count. Below 4 GiB, so the tables it leaves
    // behind are not in `huge_len_is_prompt`'s range.
    const FILL: usize = 0x8000_0000;
    const SPAN: usize = 2 << 20;
    const SLOTS: usize = SPAN / PAGE - 1;
    let target = PAGES + TABLES - 1;
    let (mut opened, mut filled) = (0usize, 0usize);
    loop {
        let short = rd::free(rd::SYSTEM).checked_sub(target).expect("the filler overshot") as usize;
        if short == 0 {
            break;
        }
        if short > opened * SLOTS - filled {
            // Opening costs 2 (3 for the first: gigabyte 2's level-1 table as well). Opening
            // only while more slots are still needed keeps `short` far above that here.
            assert!(opened < (1 << 30) / SPAN, "the filler must stay inside gigabyte 2");
            map_fixed(FILL + opened * SPAN, PAGE, rd::rw()).expect("open a filler table");
            opened += 1;
            continue;
        }
        let (table, slot) = (filled / SLOTS, filled % SLOTS);
        let n = short.min(SLOTS - slot);
        map_fixed(FILL + table * SPAN + (1 + slot) * PAGE, n * PAGE, rd::rw()).expect("fill");
        filled += n;
    }

    let before = usage_pages();
    let r = map_fixed(AT, PAGES as usize * PAGE, rd::rw());
    c.check(r == Err(Error::OutOfMemory), "pages fit, their page tables do not");
    c.check(usage_pages() == before, "nothing charged");

    // Give back exactly one page: the first filler table's first page (the kernel keeps the
    // table, so only the page returns).
    rd::unmap(FILL, PAGE).expect("free one filler page");
    c.check(rd::free(rd::SYSTEM) == PAGES + TABLES, "one page freed");
    let before = usage_pages();
    c.check(
        map_fixed(AT, PAGES as usize * PAGE, rd::rw()).is_ok(),
        "exactly enough for pages and page tables",
    );
    c.check(usage_pages() == before + PAGES + TABLES, "charged 2 pages and 4 page tables");

    rd::unmap(AT, PAGES as usize * PAGE).expect("unmap");
    for table in 0..opened {
        let slots = filled.saturating_sub(table * SLOTS).min(SLOTS);
        let base = FILL + table * SPAN;
        if table == 0 {
            if slots > 0 {
                rd::unmap(base + PAGE, slots * PAGE).expect("unmap the filler");
            }
        } else {
            rd::unmap(base, (1 + slots) * PAGE).expect("unmap the filler");
        }
    }
}

/// A range of about 2^26 pages (4 GiB to USER_AREA_END, 252 GiB) must be refused at once: the
/// overlap walk skips absent subtrees and the pages check runs before `tables_needed` ever
/// walks the range (P1-2). The range is free (everything else here is below 4 GiB; the only
/// tables in it are the two `page_table_charge` left at 4 GiB, which the kernel does not free
/// on `unmap`), so it is the budget check that refuses it, not an occupied page.
///
/// The bound is 10 ms. The fixed path reads 252 root entries plus those two tables' 1024
/// entries (under 1300 PTE reads, far under a millisecond even under QEMU TCG), plus one
/// syscall. A regression to one walk per page does 2^26 walks: at even 100 ns each that is
/// ~7 s, and a per-page `tables_needed` is 2^26 x 3 levels. 10 ms sits two orders above the
/// first and nearly three below the second, and absorbs a timer tick or two (the kernel's
/// slice is milliseconds). `time_now` is in microseconds (KERNEL-SPEC.md, System calls).
#[cfg(target_pointer_width = "64")]
fn huge_len_is_prompt(c: &mut Checker) {
    const BOUND_US: u64 = 10_000;
    let addr = 0x1_0000_0000;
    let before = usage_pages();
    let t0 = rd::time_now().unwrap();
    let r = map_fixed(addr, USER_AREA_END - addr, rd::rw());
    let elapsed = rd::time_now().unwrap() - t0;
    c.check(r == Err(Error::OutOfMemory), "a huge len is refused");
    c.check(usage_pages() == before, "nothing charged");
    writeln!(c.0, "[map-fixed] huge len took {} us", elapsed).ok();
    c.check(elapsed < BOUND_US, "a huge len is refused within 10 ms");
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

    // The last page below USER_AREA_END is accepted, and stays mapped while `map_anon` runs:
    // the kernel half of P1-1 (a mapping near the top must not starve the kernel's choice).
    let last = USER_AREA_END - PAGE;
    map_fixed(last, PAGE, MemFlags::READ).expect("the last page below USER_AREA_END is accepted");
    c.check(
        matches!(rd::map_anon(PAGE, rd::rw()), Ok(_)),
        "map_anon still succeeds with the last page mapped",
    );

    // `map_anon` still succeeds after a `map_fixed` well outside its own bounded window
    // (kernel/src/mem.rs's `find_virtual_address`, `DEFAULT_BASE..+256 MiB`).
    let low = 0x1000_0000;
    map_fixed(low, PAGE, rd::rw()).expect("a low map_fixed succeeds");
    c.check(
        matches!(rd::map_anon(PAGE, rd::rw()), Ok(_)),
        "map_anon still succeeds after an unrelated map_fixed",
    );
    rd::unmap(last, PAGE).expect("unmap the last page");
}

/// Where the child maps its read-only page. Free in a child `spawn` builds (image at 0x1_0000,
/// startup page 0x0f00_0000, stack below 0x1000_0000).
const READ_ONLY_AT: usize = 0x5005_0000;

/// The child: map a page read-only, check it reads zero, then write it. The write must fault;
/// the exit codes below say how it got anywhere else.
extern "C" fn write_read_only(_arg: usize) -> ! {
    if map_fixed(READ_ONLY_AT, PAGE, MemFlags::READ).is_err() {
        rd::process_exit(1)
    }
    if rd::peek(READ_ONLY_AT) != 0 {
        rd::process_exit(2)
    }
    rd::poke(READ_ONLY_AT, 1);
    rd::process_exit(3)
}

/// Flags are enforced: a write to an R-only `map_fixed` page faults. A process cannot watch
/// itself fault, so a child does the write and the kernel's exit notice gives the verdict:
/// `Faulted` with cause 15, a store page fault (as `wx-test.rs` checks for writes to code).
fn read_only_faults(c: &mut Checker) {
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let budget = rd::create(rd::USERS, &rd::spec(500, 2, 50)).expect("child budget");
    spawn::spawn(&image, budget, exit, write_read_only as *const () as usize, &[], &[]).expect("spawn");
    let Received::Exit(n) = rd::receive(Some(exit), 2_000_000, 0).expect("exit notice") else {
        panic!("expected an exit notice")
    };
    c.check((n.cause, n.code) == (Cause::Faulted, 15), "a write to an R-only page faults");
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
    lent_ranges(&mut c);
    exhausted_budget(&mut c);
    #[cfg(target_pointer_width = "64")]
    page_table_charge(&mut c);
    #[cfg(target_pointer_width = "64")]
    huge_len_is_prompt(&mut c);
    success_and_addr_zero(&mut c);
    read_only_faults(&mut c);
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
