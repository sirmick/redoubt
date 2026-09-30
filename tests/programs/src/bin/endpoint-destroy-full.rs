//! Destroying a budget that owns a full lease of endpoints is one bounded walk, not a scan of
//! every thread per endpoint (kernel/budgets.md, "Residual risks"; R10).
//!
//! A filler runs in its own budget and creates endpoints until the budget is full, each owned by
//! the budget (closing the handle does not end an endpoint). The judge then destroys that budget
//! and measures the destruction. The sched-trace kernel brackets every R10 with `X`/`Y` records;
//! the bench's oracle (`post_check = "sched_oracle r10_p99_us=30000"`) bounds their p99, which is
//! what the pre-fix per-endpoint all-thread scans blew past (~1.9 s for a full lease).

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::Bench;
use test_programs::spawn;

/// The filler budget's pages: about one endpoint each, plus the filler process.
const FILL_PAGES: u64 = 1_600;
/// The least the filler must have made: past the containment gate's full fill (~1460). The fill
/// reaches 1575 on rv64 and 1578 on rv32; a shorter one no longer tests a full lease.
const MIN_ENDPOINTS: u32 = 1_500;

/// The filler: endpoints in its own budget until it is refused, closing each handle (an endpoint
/// lives until its budget does). Exits with how many it made.
extern "C" fn filler(_: usize) -> ! {
    let mut made = 0u32;
    while let Ok(h) = rd::endpoint_create() {
        let _ = rd::close(h);
        made += 1;
    }
    rd::process_exit(made)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("destroy-full");
    let image = spawn::image();
    let exit = b.exit_endpoint();

    let budget = rd::create(rd::USERS, &rd::spec(FILL_PAGES, 1, 100)).expect("the filler's budget");
    spawn::spawn(&image, budget, exit, filler as *const () as usize, &[], &[]).expect("the filler");
    let made = match rd::receive(Some(exit), 600_000_000, 0) {
        Ok(rd::Received::Exit(n)) => n.code,
        _ => 0,
    };
    while rd::receive(Some(exit), 0, 0).is_ok() {}
    b.check(made >= MIN_ENDPOINTS, format_args!("the filler owns a full lease of endpoints: {}", made));

    // Destroy the budget that owns them. R10's kernel time is the trace's to bound; the guest
    // clock is a second look at the same walk.
    let before = rd::time_now().expect("time before");
    rd::destroy(budget).expect("destroying the filler's budget");
    let took = rd::time_now().expect("time after") - before;
    b.note(format_args!("budget_destroy of {} endpoints: {} us", made, took));
    b.finish("ENDPOINT-DESTROY-FULL")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("destroy-full", info) }
