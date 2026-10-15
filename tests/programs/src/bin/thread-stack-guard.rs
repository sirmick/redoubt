//! A spawned thread's stack has an unmapped guard page below it (userland/native.md,
//! "redoubt-rt", `thread`; kernel/memory-layout.md, "Regions"). In `init`'s place, this tester
//! carves a budget and launches `thread-stack-overflow` in it through the real stub with the
//! client library's `Launch`. The child says it is about to spawn a helper that recurses without
//! end, and the tester waits for its end. The verdicts are the kernel's: its fault line names the
//! guard page (the case's expectation), and the child's exit notice says `Faulted`, not an exit
//! by any code of the child's own and no later crash elsewhere.

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

/// The child's budget, in pages: its stub, its image, its stack, its tables, its heap and its
/// helper's stack.
const CHILD_PAGES: u64 = 512;
/// What the child says, word 0, as `thread-stack-overflow` defines it.
const SPAWNING: u64 = 1;

fn h(index: u32) -> Handle { Handle::new(index).expect("a slot is at least 1") }

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    let mut out = Console;
    say!(out, "\n[thread-stack-guard] starting");
    // SAFETY: the loader mapped the whole verified bundle here, read-only, `len` bytes from
    // `bundle`, for as long as this program runs (kernel/boot.md).
    let bundle = unsafe { Bundle::at(bundle, len) };
    match bundle.ok_or("no bundle").and_then(|b| run(&mut out, b)) {
        Ok(()) => say!(out, "[thread-stack-guard] THREAD STACK GUARD TEST PASSED"),
        Err(why) => say!(out, "[thread-stack-guard] FAIL: {why}"),
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
    let image = bundle
        .find(b"thread-stack-overflow")
        .map(|e| e.data)
        .ok_or("thread-stack-overflow missing from the bundle")?;
    let reports = Endpoint::create().map_err(|_| "create the reports endpoint")?;
    let exit = Endpoint::create().map_err(|_| "create the exit endpoint")?;
    let mut launch = Launch::new(STUB_BIN, image, budget, exit);
    launch.handle("tester", reports.handle());
    let mut job = launch.start().map_err(|_| "start thread-stack-overflow")?;

    let spawning = next(&reports)?;
    if spawning.words[0] != SPAWNING {
        return Err("the child's word is not its spawn");
    }
    answer(spawning);
    say!(out, "[thread-stack-guard] trace: the child is about to spawn its helper");

    // The helper overflows its stack into the guard page: the kernel ends the process there.
    let ended = job.wait(FOREVER).map_err(|_| "wait for the child's end")?;
    say!(
        out,
        "[thread-stack-guard] trace: the child ended, cause {:?}, code {}",
        ended.notice.cause,
        ended.notice.code
    );
    if ended.notice.cause != Cause::Faulted {
        return Err("the helper's overflow did not end the child by a fault");
    }
    say!(
        out,
        "[thread-stack-guard] ok: the helper's overflow ended the child at its guard page: cause Faulted"
    );
    Ok(())
}
