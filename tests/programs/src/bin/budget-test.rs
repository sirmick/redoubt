//! Budgets and handle tables as KERNEL-SPEC.md states them (WP-K1, with answer 103's queue), as far as a system-class
//! caller holding the boot budgets can see them: carving and charging (R6, R7), labels added
//! and sorted (part of I6), depth, destruction and its sweep of this process's own table (R10,
//! I2, I10), the handle table's cost (128 handles a page), usage within limits after every step
//! (I5), `time_now` and `random`, and a short sequence whose every result the executable model
//! predicts (written out below, for WP-C1). Must run as the loader's first program, which holds
//! root, system and users in handles 1-3, then a handle per device object the machine has
//! (WP-K3), and lives in system. How many devices there are is the machine's business, so the
//! handles this test creates are numbered from [`rd::first_free`], never from 4.
//!
//! What K1 cannot show from userspace, and which case will: accounts (R8) and stamps other than
//! the caller's (R9) travel only in messages (WP-K2); a process killed by R10 in a budget below
//! `system` needs `process_create` (WP-K4; `budget-destroy-kills` covers `system` itself); a
//! user-class caller (the `ClassDenied` for labels, `budget_usage`'s `LabelDenied`) needs a
//! process in a user budget (WP-K4). Budget ids never being reused (I12) is not visible from
//! here: the kernel checks ids on every handle lookup, and the cycles below only exercise that
//! path.

#![no_std]
#![no_main]
#![allow(unused_must_use)] // `expect!` returns what it checked, for the steps that use it

use test_programs::rd::{self, Error, FOREVER, Labels, Usage};
use test_programs::{Logger, log};

struct T {
    logger: Logger,
    failed: bool,
    /// Every budget handle this test holds, for the I5 check after each step.
    held: [u32; 64],
    nheld: usize,
}

impl T {
    fn check(&mut self, ok: bool, what: core::fmt::Arguments) {
        if !ok {
            self.failed = true;
            self.logger.log(format_args!("[budget] FAIL: {}", what));
        }
    }

    fn hold(&mut self, h: u32) -> u32 {
        if self.nheld < self.held.len() {
            self.held[self.nheld] = h;
            self.nheld += 1;
        }
        h
    }

    /// I5: usage within limits, for every budget still held (a handle that has gone is skipped).
    fn i5(&mut self, step: &str) {
        for i in 0..self.nheld {
            let h = self.held[i];
            if let Ok(u) = rd::usage(h) {
                let ok = u.pages_usage <= u.pages_limit
                    && u.processes_usage <= u.processes_limit
                    && u.weight_carved <= u.weight_limit;
                self.check(ok, format_args!("I5 after {}: handle {} {:?}", step, h, u));
            }
        }
    }
}

macro_rules! expect {
    ($t:expr, $got:expr, $want:expr) => {{
        let got = $got;
        let want = $want;
        $t.check(got == want, format_args!("{} = {:?}, want {:?}", stringify!($got), got, want));
        got
    }};
}

fn usage(pl: u64, pu: u64, prl: u32, pru: u32, wl: u32, wc: u32) -> Usage {
    Usage {
        pages_limit: pl,
        pages_usage: pu,
        processes_limit: prl,
        processes_usage: pru,
        weight_limit: wl,
        weight_carved: wc,
    }
}

fn labelled(pages: u64, labels: &[u64]) -> rd::BudgetSpec {
    rd::BudgetSpec { labels: Labels::from_slice(labels).unwrap(), ..rd::spec(pages, 0, 0) }
}

/// Fault in 16 KiB of stack below this frame, so later calls grow no stack.
#[inline(never)]
fn warm_stack() {
    let mut buf = [0u8; 16384];
    for i in (0..buf.len()).step_by(512) {
        // SAFETY: an in-bounds write to a local array; volatile so it is not optimised away.
        unsafe { core::ptr::write_volatile(&mut buf[i], 1) };
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let logger = test_programs::logsrv::start();
    let mut t = T { logger, failed: false, held: [0; 64], nheld: 0 };
    for h in [rd::ROOT, rd::SYSTEM, rd::USERS] {
        t.hold(h);
    }
    // Where this process's own handles start: after root, system, users and the machine's
    // device objects (kernel `device.rs`, INTERIM).
    let base = rd::first_free();
    // `system` pays for every page this process and log-server fault in, so exact checks of its
    // usage need both to be done growing first: touch the stack this test will use, and let
    // log-server map what serving a message needs. Nothing is logged between checks (only a
    // failure logs).
    warm_stack();
    log!(t.logger, "[budget] warming up");
    let system0 = rd::usage(rd::SYSTEM).unwrap();
    log!(t.logger, "[budget] system {:?}", system0);
    test_programs::wait_ms(20);
    let system0 = rd::usage(rd::SYSTEM).unwrap();

    // --- R6, R7: carving and charging -------------------------------------------------------
    let a = expect!(t, rd::create(rd::SYSTEM, &rd::spec(100, 2, 50)), Ok(base)).unwrap_or(base);
    t.hold(a);
    // The parent pays the child's own page and counts its limits, never its usage (R6, answer 76).
    let s = system0;
    expect!(t, rd::usage(rd::SYSTEM), Ok(Usage {
        pages_usage: s.pages_usage + 101,
        processes_usage: s.processes_usage + 2,
        weight_carved: s.weight_carved + 50,
        ..s
    }));
    expect!(t, rd::usage(a), Ok(usage(100, 0, 2, 0, 50, 0)));
    // A revocation scope costs its parent one page, like any budget, and has no usage of its own.
    let scope = expect!(t, rd::create(a, &rd::spec(0, 0, 0)), Ok(base + 1)).unwrap_or(base + 1);
    t.hold(scope);
    expect!(t, rd::usage(scope), Ok(usage(0, 0, 0, 0, 0, 0)));
    expect!(t, rd::usage(a), Ok(usage(100, 1, 2, 0, 50, 0)));
    t.i5("carving");
    // Beyond the parent: each limit alone, then the order when several are exceeded.
    expect!(t, rd::create(a, &rd::spec(99, 0, 0)), Err(Error::OutOfMemory));
    expect!(t, rd::create(a, &rd::spec(1, 3, 0)), Err(Error::OutOfProcesses));
    expect!(t, rd::create(a, &rd::spec(1, 0, 51)), Err(Error::InvalidArgument));
    expect!(t, rd::create(a, &rd::spec(99, 3, 51)), Err(Error::OutOfMemory));
    expect!(t, rd::create(a, &rd::spec(1, 3, 51)), Err(Error::OutOfProcesses));
    // No pages of its own is fine: its object is the parent's to pay for.
    let pageless = expect!(t, rd::create(a, &rd::spec(0, 1, 0)), Ok(base + 2)).unwrap_or(base + 2);
    expect!(t, rd::usage(pageless), Ok(usage(0, 0, 1, 0, 0, 0)));
    expect!(t, rd::destroy(pageless), Ok(()));
    // Exactly the parent's free limits fit (98 pages and the object's one); then not even a scope.
    let full = expect!(t, rd::create(a, &rd::spec(98, 2, 50)), Ok(base + 2)).unwrap_or(base + 2);
    expect!(t, rd::usage(a), Ok(usage(100, 100, 2, 2, 50, 50)));
    expect!(t, rd::create(a, &rd::spec(0, 0, 0)), Err(Error::OutOfMemory));
    expect!(t, rd::create(full, &rd::spec(98, 0, 0)), Err(Error::OutOfMemory));
    t.i5("carving to the limit");
    expect!(t, rd::destroy(full), Ok(()));
    expect!(t, rd::usage(a), Ok(usage(100, 1, 2, 0, 50, 0)));
    // The freed index is the lowest free one, and is reused.
    let again = expect!(t, rd::create(a, &rd::spec(1, 0, 0)), Ok(base + 2)).unwrap_or(base + 2);
    expect!(t, rd::destroy(again), Ok(()));

    // --- Classes and labels (part of I6 and I8; the rest needs a user-class caller, WP-K4) ----
    // A child's class is its parent's (answer 73), and nothing in a spec asks for a place in the
    // queue: there is one stride queue, ordered by weight alone (answer 103).
    let quick = expect!(t, rd::create(a, &labelled(1, &[])), Ok(base + 2)).unwrap_or(base + 2);
    expect!(t, rd::create(quick, &labelled(0, &[])), Ok(base + 3));
    expect!(t, rd::destroy(quick), Ok(()));
    // A labelled user-class parent: a child dropping the label is refused whoever asks.
    let user_lab = expect!(t, rd::create(rd::USERS, &labelled(1, &[9])), Ok(base + 2)).unwrap_or(base + 2);
    expect!(t, rd::create(user_lab, &labelled(0, &[])), Err(Error::LabelDenied));
    expect!(t, rd::destroy(user_lab), Ok(()));
    // A system-class caller may add labels; they are sorted and deduplicated.
    let lab = expect!(t, rd::create(a, &labelled(20, &[5, 3, 5])), Ok(base + 2)).unwrap_or(base + 2);
    t.hold(lab);
    expect!(t, rd::create(lab, &labelled(1, &[3])), Err(Error::LabelDenied));
    expect!(t, rd::create(lab, &labelled(1, &[])), Err(Error::LabelDenied));
    let same =
        expect!(t, rd::create(lab, &labelled(1, &[5, 3, 5, 3, 5, 3, 5, 3])), Ok(base + 3))
            .unwrap_or(base + 3);
    let more = expect!(t, rd::create(lab, &labelled(1, &[7, 5, 3])), Ok(base + 4)).unwrap_or(base + 4);
    expect!(t, rd::destroy(more), Ok(()));
    expect!(t, rd::destroy(same), Ok(()));

    // --- MAX_DEPTH: system is at depth 1, so seven more levels fit and the eighth is refused.
    // `a` is at depth 2, so chain[1..=5] are at depths 3 to 7, and a child of chain[5] would be
    // at MAX_DEPTH = 8. Each level leaves its parent one free page.
    let mut chain = [0u32; 6];
    chain[0] = a;
    for depth in 1..6 {
        let pages = 20 - 2 * depth as u64;
        chain[depth] =
            expect!(t, rd::create(chain[depth - 1], &rd::spec(pages, 0, 0)), Ok(base + 2 + depth as u32))
                .unwrap_or(0);
    }
    expect!(t, rd::create(chain[5], &rd::spec(1, 0, 0)), Err(Error::TooLarge));
    expect!(t, rd::create(chain[5], &labelled(1, &[])), Err(Error::TooLarge));
    t.i5("depth");

    // --- R10, I2, I10: destroy a subtree ----------------------------------------------------
    // `chain[1]` holds chain[2..=5]; handles to every one of them go, and `a` gets back exactly
    // what chain[1] carved from it.
    let before = rd::usage(a).unwrap();
    // chain[1] carved 18 pages from `a`, and `a` paid its object's page.
    let tree = expect!(t, rd::create(chain[1], &rd::spec(0, 0, 0)), Ok(base + 8)).unwrap_or(base + 8);
    expect!(t, rd::destroy(chain[1]), Ok(()));
    for gone in [chain[1], chain[2], chain[3], chain[4], chain[5], tree] {
        expect!(t, rd::usage(gone), Err(Error::BadHandle));
        expect!(t, rd::close(gone), Err(Error::BadHandle));
        expect!(t, rd::destroy(gone), Err(Error::BadHandle));
    }
    expect!(t, rd::usage(a), Ok(Usage { pages_usage: before.pages_usage - 19, ..before }));
    // What was never carved from `a`'s subtree is untouched: the scope and the labelled budget.
    expect!(t, rd::usage(scope), Ok(usage(0, 0, 0, 0, 0, 0)));
    expect!(t, rd::usage(lab).map(|u| u.pages_limit), Ok(20));
    // Destroying `a` itself returns system to where it started (I10), handle table included.
    expect!(t, rd::destroy(a), Ok(()));
    for gone in [a, scope, lab] {
        expect!(t, rd::usage(gone), Err(Error::BadHandle));
    }
    expect!(t, rd::usage(rd::SYSTEM), Ok(system0));
    t.nheld = 3;
    t.i5("destroy");

    // --- The handle table: 128 handles a page, charged to the caller's budget ----------------
    let x = expect!(t, rd::create(rd::SYSTEM, &rd::spec(400, 0, 0)), Ok(base)).unwrap_or(base);
    let with_x = rd::usage(rd::SYSTEM).unwrap();
    // Handles 1..=base are held; scopes take the rest of the first table page.
    for index in base + 1..=128 {
        if rd::create(x, &rd::spec(0, 0, 0)) != Ok(index) {
            t.check(false, format_args!("scope {} did not get handle {}", index, index));
            break;
        }
    }
    expect!(t, rd::usage(rd::SYSTEM), Ok(with_x));
    // The 129th handle needs a second page: one more page from system, not from x.
    expect!(t, rd::create(x, &rd::spec(0, 0, 0)), Ok(129));
    expect!(t, rd::usage(rd::SYSTEM), Ok(Usage { pages_usage: with_x.pages_usage + 1, ..with_x }));
    // One page per scope, all paid by x: the rest of the first page, plus the 129th.
    expect!(t, rd::usage(x).map(|u| u.pages_usage), Ok(129 - base as u64));
    // Closing the second page's only handle frees that page.
    expect!(t, rd::close(129), Ok(()));
    expect!(t, rd::usage(rd::SYSTEM), Ok(with_x));
    expect!(t, rd::close(129), Err(Error::BadHandle));
    // Destroying x sweeps every scope out of the table.
    expect!(t, rd::destroy(x), Ok(()));
    expect!(t, rd::usage(rd::SYSTEM), Ok(system0));
    expect!(t, rd::usage(128), Err(Error::BadHandle));

    // --- A sequence the model predicts, result by result (WP-C1 compares these) -------------
    // In the trace format (redoubt/model/README.md), from a fresh budget M = (10 pages,
    // 1 process, weight 10) in system, M = h:4 in the model's numbering (here `base`, since
    // the boot handles come first), with each budget's own page charged to its parent
    // (answer 76):
    //   budget_create h:4 5 0 0 user [] 0 forever  -> ok h:5
    //   budget_usage h:4                           -> ok usage [10,6,1,0,10,0]
    //   budget_create h:4 5 0 0 user [] 0 forever  -> err OutOfMemory
    //   budget_create h:4 0 0 0 user [] 0 forever  -> ok h:6
    //   budget_create h:4 2 2 0 user [] 0 forever  -> err OutOfProcesses
    //   budget_create h:4 2 0 11 user [] 0 forever -> err InvalidArgument
    //   budget_usage h:4                           -> ok usage [10,7,1,0,10,0]
    //   budget_destroy h:5                         -> ok
    //   budget_usage h:4                           -> ok usage [10,1,1,0,10,0]
    //   budget_usage h:5                           -> err BadHandle
    //   handle_close h:6                           -> ok
    //   budget_usage h:4                           -> ok usage [10,1,1,0,10,0]
    //   budget_destroy h:4                         -> ok
    //   budget_usage h:4                           -> err BadHandle
    let m = expect!(t, rd::create(rd::SYSTEM, &rd::spec(10, 1, 10)), Ok(base)).unwrap_or(base);
    expect!(t, rd::create(m, &rd::spec(5, 0, 0)), Ok(base + 1));
    expect!(t, rd::usage(m), Ok(usage(10, 6, 1, 0, 10, 0)));
    expect!(t, rd::create(m, &rd::spec(5, 0, 0)), Err(Error::OutOfMemory));
    expect!(t, rd::create(m, &rd::spec(0, 0, 0)), Ok(base + 2));
    expect!(t, rd::create(m, &rd::spec(2, 2, 0)), Err(Error::OutOfProcesses));
    expect!(t, rd::create(m, &rd::spec(2, 0, 11)), Err(Error::InvalidArgument));
    expect!(t, rd::usage(m), Ok(usage(10, 7, 1, 0, 10, 0)));
    expect!(t, rd::destroy(base + 1), Ok(()));
    expect!(t, rd::usage(m), Ok(usage(10, 1, 1, 0, 10, 0)));
    expect!(t, rd::usage(base + 1), Err(Error::BadHandle));
    // Closing a scope's handle does not destroy the scope: it still costs m its page, until m
    // itself goes.
    expect!(t, rd::close(base + 2), Ok(()));
    expect!(t, rd::usage(m), Ok(usage(10, 1, 1, 0, 10, 0)));
    expect!(t, rd::destroy(m), Ok(()));
    expect!(t, rd::usage(m), Err(Error::BadHandle));
    expect!(t, rd::usage(rd::SYSTEM), Ok(system0));

    // --- I10 over many cycles: create, nest, destroy; nothing leaks ----------------------------
    for cycle in 0..500u64 {
        let b = rd::create(rd::SYSTEM, &rd::spec(8, 1, 1));
        let c = b.and_then(|b| rd::create(b, &rd::spec(3, 0, 0)));
        let ok = b == Ok(base)
            && c == Ok(base + 1)
            && rd::destroy(base) == Ok(())
            && rd::usage(base + 1) == Err(Error::BadHandle);
        if !ok {
            t.check(false, format_args!("cycle {}: {:?} {:?}", cycle, b, c));
            break;
        }
    }
    expect!(t, rd::usage(rd::SYSTEM), Ok(system0));
    // A deadline is accepted at creation; this one is an hour away, so the budget is destroyed by
    // hand first. (Enforcement: `budget-deadline`.)
    let lease = rd::BudgetSpec { deadline: rd::time_now().unwrap() + 3_600_000_000, ..rd::spec(2, 0, 0) };
    let l = expect!(t, rd::create(rd::SYSTEM, &lease), Ok(base)).unwrap_or(base);
    expect!(t, rd::destroy(l), Ok(()));
    let _ = FOREVER;

    // --- time_now and random ------------------------------------------------------------------
    let t1 = rd::time_now().unwrap_or(0);
    test_programs::wait_ms(5);
    let t2 = rd::time_now().unwrap_or(0);
    t.check(t2 >= t1 + 4_000 && t2 < t1 + 1_000_000, format_args!("time_now {} then {}", t1, t2));
    // `random` returns one u64 (answer 77): eight draws, all different (a repeat among eight
    // CSPRNG values has probability about 2^-59).
    let mut draws = [Ok(0); 8];
    for draw in draws.iter_mut() {
        *draw = rd::random();
    }
    let distinct = draws.iter().enumerate().all(|(i, d)| d.is_ok() && !draws[..i].contains(d));
    t.check(distinct, format_args!("random values {:x?}", draws));

    if t.failed {
        log!(t.logger, "BUDGET TEST FAILED");
    } else {
        log!(t.logger, "BUDGET TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
