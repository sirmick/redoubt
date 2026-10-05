//! A server's heap cap is refused in its runtime, before the kernel is asked (servers/init.md,
//! "Heaps"). In `init`'s place, since servers under `init` hold no budget handle (R33), this
//! tester carves a budget and launches `heap-capped` in it through the real stub with the client
//! library's `Launch`, its heap capped at `CAP` pages. The child takes pages until its runtime
//! refuses one and says so; the tester reads the child's budget (`budget_usage`); the child asks
//! for `PAST` pages more, which its budget has room for, and says whether that was refused; the
//! tester reads the budget again. Last the child asks for them infallibly. The verdicts are the
//! kernel's: the budget was not charged for the refused pages, though it could have held them,
//! and the child's exit notice carries the runtime's panic code.

#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_client::launch::Launch;
use redoubt_rt::abi::{BudgetSpec, Cause, FOREVER, Handle, Handles, Labels};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::typed::{Outcome, finish};
use test_programs::bundle::Bundle;
use test_programs::console::{self, Console};
use test_programs::rd;

redoubt_rt::panic_handler!();

static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out, $($arg)*).ok(); }};
}

/// The child's budget, in pages: its stub, its image, its stack, its tables and its heap, with
/// room to spare past the cap.
const CHILD_PAGES: u64 = 512;
/// The child's heap cap.
const CAP: u32 = 16;
/// What the child says, word 0, as `heap-capped` defines them.
const AT_CAP: u64 = 1;
const REFUSED: u64 = 2;

fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "\n[heap-cap] starting");
    // SAFETY: the loader mapped the whole verified bundle here, read-only, `len` bytes from
    // `bundle`, for as long as this program runs (kernel/boot.md).
    let bundle = unsafe { Bundle::at(bundle, len) };
    match bundle.ok_or("no bundle").and_then(|b| run(&mut out, b)) {
        Ok(()) => say!(out, "[heap-cap] HEAP CAP TEST PASSED"),
        Err(why) => say!(out, "[heap-cap] FAIL: {why}"),
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

/// The child's next word, answered at once so it goes on.
fn next(reports: &Endpoint) -> Result<Request, &'static str> {
    loop {
        match reports.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => return Ok(request),
            Ok(_) => {}
            Err(_) => return Err("receive the child's word"),
        }
    }
}

fn answer(request: Request) {
    let none = Handles::new();
    let _ = finish(request, &Outcome { words: [0; 4], send: none, close: none });
}

fn run(out: &mut Console, bundle: Bundle) -> Result<(), &'static str> {
    let spec = BudgetSpec {
        pages: CHILD_PAGES,
        processes: 1,
        weight: 100,
        labels: Labels::from_slice(&[]).map_err(|_| "an empty label set")?,
        account: 0,
        deadline: FOREVER,
    };
    let budget = Budget::from_handle(h(rd::SYSTEM)).create_child(&spec).map_err(|_| "carve the budget")?;
    let image = bundle.find(b"heap-capped").map(|e| e.data).ok_or("heap-capped missing from the bundle")?;
    let reports = Endpoint::create().map_err(|_| "create the reports endpoint")?;
    let exit = Endpoint::create().map_err(|_| "create the exit endpoint")?;
    let mut launch = Launch::new(STUB_BIN, image, budget, exit);
    launch.handle("tester", reports.handle()).heap_pages(CAP);
    let mut job = launch.start().map_err(|_| "start heap-capped")?;

    let at_cap = next(&reports)?;
    if at_cap.words[0] != AT_CAP {
        return Err("the child's first word is not its cap");
    }
    let got = at_cap.words[1];
    let before = job.budget().usage().map_err(|_| "read the child's budget")?;
    answer(at_cap);
    say!(out, "[heap-cap] trace: the child holds {got} heap pages at its cap of {CAP}");

    let refused = next(&reports)?;
    if refused.words[0] != REFUSED {
        return Err("the child's second word is not its refusal");
    }
    let (asked, was_refused) = (refused.words[1], refused.words[2] == 1);
    let after = job.budget().usage().map_err(|_| "read the child's budget")?;
    answer(refused);
    let room = before.pages_limit - before.pages_usage;
    say!(
        out,
        "[heap-cap] ok: {asked} pages past the cap: {}; the budget held {} pages before and {} after, with room for {room}",
        if was_refused { "refused" } else { "granted" },
        before.pages_usage,
        after.pages_usage
    );
    if !was_refused || got > u64::from(CAP) {
        return Err("the runtime let the heap past its cap");
    }
    if after.pages_usage != before.pages_usage {
        return Err("the budget was charged for the refused pages");
    }
    if room < asked {
        return Err("the budget had no room for the pages: the kernel, not the cap, would refuse them");
    }
    // Then it asks infallibly: the runtime refuses, Rust's handler panics, and the runtime exits
    // with its panic code, as for an exhausted budget.
    let ended = job.wait(FOREVER).map_err(|_| "wait for the child's end")?;
    say!(
        out,
        "[heap-cap] ok: an infallible allocation past the cap ended the child, code {}",
        ended.notice.code
    );
    if ended.notice.cause != Cause::Exited || ended.notice.code != redoubt_rt::exit::PANIC {
        return Err("the infallible allocation past the cap did not end the child by its panic");
    }
    Ok(())
}
