//! Destroying a budget that owns a full lease of endpoints is one bounded walk, not a scan of
//! every thread per endpoint (kernel/budgets.md, "Residual risks"; R10).
//!
//! A filler runs in its own budget and creates endpoints until its handle table is full
//! (`MAX_HANDLES`), each owned by the budget, keeping every handle. The judge then destroys that
//! budget while the filler still runs, so the destruction ends a full table as well as the
//! endpoints, and measures it. The sched-trace kernel brackets every R10 with `X`/`Y` records;
//! the bench's oracle (`post_check = "sched_oracle r10_p99_us=30000"`) bounds their p99, which is
//! what the pre-fix per-endpoint all-thread scans blew past (~1.9 s for a full lease).

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::Bench;
use test_programs::spawn;

/// The filler budget's pages: one per endpoint, the table's 64 pages and the filler process,
/// with room left, so the table fills before the pages run out.
const FILL_PAGES: u64 = 6_000;
/// The least the filler must have made: a full table less the handles it was started with, the
/// containment gate's full fill (4,091).
const MIN_ENDPOINTS: usize = 4_000;

/// The filler: endpoints in its own budget until its table is full, keeping each handle. It
/// reports how many it made on slot 1 and waits for its budget's end.
extern "C" fn filler(_: usize) -> ! {
    let mut made = 0;
    let refused = loop {
        match rd::endpoint_create() {
            Ok(_) => made += 1,
            Err(e) => break e,
        }
    };
    let _ = rd::send(1, &rd::body([made, refused as usize, 0, 0]), None, rd::FOREVER);
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("destroy-full");
    let image = spawn::image();
    let exit = b.exit_endpoint();

    let report = rd::endpoint_create().expect("the report endpoint");
    let to_report = rd::mint_from_handle(report, 1, None).expect("a send right");
    let budget = rd::create(rd::USERS, &rd::spec(FILL_PAGES, 1, 100)).expect("the filler's budget");
    spawn::spawn(&image, budget, exit, filler as *const () as usize, &[], &[to_report]).expect("the filler");
    let (made, refused) = match rd::receive(Some(report), 600_000_000, 0) {
        Ok(rd::Received::Message(m)) => (m.body.words[0], m.body.words[1]),
        _ => (0, 0),
    };
    b.check(
        made >= MIN_ENDPOINTS && refused == rd::Error::TooLarge as usize,
        format_args!("the filler holds a full table of endpoints: {}", made),
    );

    // Destroy the budget that owns them. R10's kernel time is the trace's to bound; the guest
    // clock is a second look at the same walk.
    let before = rd::time_now().expect("time before");
    rd::destroy(budget).expect("destroying the filler's budget");
    let took = rd::time_now().expect("time after") - before;
    b.note(format_args!("budget_destroy of {} endpoints: {} us", made, took));
    while rd::receive(Some(exit), 0, 0).is_ok() {}
    b.finish("ENDPOINT-DESTROY-FULL")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("destroy-full", info) }
