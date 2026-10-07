//! `budget_reap` (kernel/budgets.md, R10): each call destroys one child of the budget its handle
//! names, that child's whole subtree, keeps the budget and returns how many children are left.
//!
//! This program runs first and judges, from the kernel's own results. It carves P from `system`,
//! and under P first A, then C, and G under C: C is P's first child, the newest. A server in G
//! takes a call on an endpoint this program owns, with a lent page, and holds it; a sender blocks
//! on a second endpoint of this program's through a handle stamped with C. Then:
//! - reaping through A's handle reaches only below A, which has no children: P keeps both;
//! - reaping P destroys C and G: the server is killed, the caller gets `Dead` with its lend back intact
//!   (R4b), the stamped send fails `Dead` and nothing arrives (R10 step 4), C's and G's handles are closed,
//!   and A is untouched;
//! - reaping P again takes A; then P's usage is what it was before its first carve (I10), and a reap of a
//!   budget with no children returns 0 and changes nothing;
//! - a handle that is not a budget is `WrongObject`, a slot that holds nothing `BadHandle`.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Error, Received, Return};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// The value the caller writes into the page it lends, read back once the lend comes back.
const MARK: u64 = 0x5eed_0f_1e4d;

/// The caller's result: 1 for `Dead` with its lend returned and the page as it was, 2 for
/// anything else, 0 until then. The sender's: 1 for `Dead`, 2 for anything else.
static CALLER: AtomicUsize = AtomicUsize::new(0);
static SENDER: AtomicUsize = AtomicUsize::new(0);

/// `budget_reap(h)`.
fn reap(budget: u32) -> Result<u32, Error> {
    match redoubt_sys::syscall(&rd::Call::BudgetReap { budget: rd::h(budget) })? {
        Return::Remaining(n) => Ok(n),
        _ => Err(Error::InvalidArgument),
    }
}

/// The server, in G, with the call endpoint in slot 1 and this program's inbox in slot 2: take
/// one call, say so, and hold it until the reap kills this process.
extern "C" fn server(_: usize) -> ! {
    if let Ok(Received::Message(_)) = rd::receive(Some(1), WAIT, 0) {
        let _ = rd::send_waiting(2, &rd::body([0; 4]), None, WAIT);
    }
    test_programs::park()
}

/// A caller outside the reaped subtree: lend a marked page on a call the server takes and never
/// answers.
fn caller(endpoint: usize) {
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("a page to lend");
    rd::poke(page, MARK);
    let dead = matches!(
        rd::call_outcome(endpoint as u32, &rd::body([0; 4]), rd::pages(page, 1), rd::FOREVER),
        Ok((o, _)) if o.status == Err(Error::Dead) && o.lend == redoubt_sys::LendDisposition::Returned
    );
    CALLER.store(if dead && rd::peek(page) == MARK { 1 } else { 2 }, Ordering::Release);
}

/// A sender through a handle stamped with C, to an endpoint nobody receives on until the reap.
fn sender(stamped: usize) {
    let r = rd::send(stamped as u32, &rd::body([7; 4]), None, rd::FOREVER);
    SENDER.store(if r == Err(Error::Dead) { 1 } else { 2 }, Ordering::Release);
}

fn wait_for(flag: &AtomicUsize) -> usize {
    for _ in 0..1_000 {
        let v = flag.load(Ordering::Acquire);
        if v != 0 {
            return v;
        }
        test_programs::wait_ms(1);
    }
    0
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("budget-reap");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");
    let calls = rd::endpoint_create().expect("the call endpoint");
    let quiet = rd::endpoint_create().expect("the send endpoint");

    let p = rd::create(rd::SYSTEM, &rd::spec(2_400, 3, 30)).expect("P");
    let empty = rd::usage(p).expect("P's usage");
    let a = rd::create(p, &rd::spec(100, 0, 5)).expect("A");
    let c = rd::create(p, &rd::spec(1_000, 1, 10)).expect("C");
    let g = rd::create(c, &rd::spec(800, 1, 5)).expect("G");

    spawn::spawn(&image, g, exit, server as *const () as usize, &[], &[calls, inbox]).expect("the server");
    rd::thread(caller, calls as usize).expect("the caller");
    match rd::receive(Some(inbox), WAIT, 0) {
        Ok(Received::Message(_)) => {}
        other => panic!("the server's word: {other:?}"),
    }
    let stamped = rd::mint_from_handle(quiet, 9, Some(c)).expect("a handle stamped with C");
    rd::thread(sender, stamped as usize).expect("the sender");
    // The sender is blocked on `quiet`.
    test_programs::wait_ms(10);

    let by_child = reap(a);
    let a_kept = rd::usage(a).is_ok();
    b.check(
        by_child == Ok(0) && a_kept,
        format_args!("a reap through a child's handle reaches only below it: {by_child:?}, the child kept"),
    );
    let first = reap(p);
    b.check(first == Ok(1), format_args!("the first reap takes the newest child, one left: {first:?}"));
    let gone =
        [c, g, stamped].iter().all(|h| rd::usage(*h).is_err() && rd::close(*h) == Err(Error::BadHandle));
    b.check(
        gone,
        format_args!("the reaped child's and grandchild's handles, and those stamped with them, are closed"),
    );
    b.check(rd::usage(a).is_ok(), format_args!("the other child is untouched"));
    let killed =
        matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Killed);
    b.check(killed, format_args!("the server in the grandchild is killed"));
    b.check(wait_for(&CALLER) == 1, format_args!("the caller outside gets Dead with its lend back intact"));
    b.check(wait_for(&SENDER) == 1, format_args!("the send stamped with the reaped child fails Dead"));
    let nothing = rd::receive(Some(quiet), 10_000, 0);
    b.check(
        nothing == Err(Error::Timeout),
        format_args!("nothing sent through the stamped handle arrives: {:?}", nothing.map(|_| ())),
    );
    let last = reap(p);
    let emptied = rd::usage(p);
    b.check(
        last == Ok(0) && emptied == Ok(empty),
        format_args!("the last reap leaves the budget's usage as before its first carve: {last:?}"),
    );
    let again = reap(p);
    b.check(
        again == Ok(0) && rd::usage(p) == Ok(empty),
        format_args!("a budget with no children returns 0 and nothing changes: {again:?}"),
    );
    let wrong = reap(calls);
    let missing = reap(rd::first_free());
    b.check(
        wrong == Err(Error::WrongObject) && missing == Err(Error::BadHandle),
        format_args!("an endpoint is WrongObject, an empty slot BadHandle: {wrong:?}, {missing:?}"),
    );
    b.finish("BUDGET-REAP")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("budget-reap", info) }
