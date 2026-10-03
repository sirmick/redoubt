//! A process has exactly `MAX_THREADS` threads, its initial one included, numbered
//! `1..=MAX_THREADS` (kernel/processes.md, "Threads"), and each costs the budget it runs in one
//! page (kernel/budgets.md, R6). The holder is a child in a budget of its own, so this program
//! reads that budget's usage at each step. The verdicts are the kernel's: the TIDs
//! `thread_create` returns, `TooManyThreads` for the one past the limit, and `budget_usage`.

#![no_std]
#![no_main]

use redoubt_sys::MAX_THREADS;
use test_programs::rd::{self, Cause, Error, Received};
use test_programs::{Logger, log, spawn};

const WAIT: u64 = 5_000_000;
/// The holder's send right to this program, in its slot 1.
const PARENT: u32 = 1;
/// Page-table levels: a new address space has a table at each on the way to its header page
/// (kernel/memory-layout.md).
const LEVELS: u64 = if cfg!(target_pointer_width = "64") { 3 } else { 2 };

extern "C" fn parked(_arg: usize) -> ! { test_programs::park() }

/// The child: every stack first, then a call, so its budget moves only for the threads between
/// that call and the next; then it reports what the kernel answered and exits.
extern "C" fn holder(_arg: usize) -> ! {
    let stacks = rd::map_anon(MAX_THREADS * rd::PAGE_SIZE, rd::rw()).expect("the stacks");
    rd::call(PARENT, &rd::body([0; 4]), None, WAIT).expect("the stacks are mapped");
    // The initial thread holds one TID; every other one is created here, each on its own page.
    // One bit per TID 0..=MAX_THREADS.
    let mut seen = [0u64; MAX_THREADS / 64 + 1];
    let bit = |tid: usize| (tid / 64, 1u64 << (tid % 64));
    seen[0] = 1 << 1;
    let mut in_range = true;
    let mut threads = 1;
    let refusal = loop {
        if threads > MAX_THREADS {
            break Error::InvalidArgument;
        }
        let stack = stacks + threads * rd::PAGE_SIZE - 16;
        match rd::thread_create(parked as *const () as usize, stack, 0) {
            Ok(tid) => {
                in_range &= (1..=MAX_THREADS as u32).contains(&tid);
                if in_range {
                    let (word, mask) = bit(tid as usize);
                    in_range &= seen[word] & mask == 0;
                    seen[word] |= mask;
                }
                threads += 1;
            }
            Err(e) => break e,
        }
    };
    let all = (1..=MAX_THREADS).all(|tid| {
        let (word, mask) = bit(tid);
        seen[word] & mask != 0
    });
    let words = [threads, usize::from(in_range && all), usize::from(refusal == Error::TooManyThreads), 0];
    rd::call(PARENT, &rd::body(words), None, WAIT).expect("the report");
    rd::process_exit(0)
}

/// Take the holder's next call and reply to it once `budget` is read: its words, and the usage.
fn take(from: u32, budget: u32) -> ([usize; 4], u64) {
    let Received::Message(m) = rd::receive(Some(from), WAIT, 0).expect("the holder's call") else {
        panic!("expected the holder's call")
    };
    let pages = rd::usage(budget).expect("usage").pages_usage;
    rd::reply(m.msg_id.get(), &rd::body([0; 4])).expect("reply");
    (m.body.words, pages)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = test_programs::logsrv::start();
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let calls = rd::endpoint_create().expect("the holder's endpoint");
    let to_parent = rd::mint_from_handle(calls, 1, None).expect("a send right");
    let budget = rd::create(rd::USERS, &rd::spec(1024, 1, 100)).expect("the holder's budget");
    let pages = || rd::usage(budget).expect("usage").pages_usage;

    let empty = pages();
    let process = rd::process_create(budget, exit).expect("the holder");
    let created = pages() - empty == 1 + LEVELS;
    log!(logger, "[thread-limit] process_create charges the header page and {} tables: {}", LEVELS, created);
    spawn::start(process, &image, holder as *const () as usize, &[], &[to_parent]).expect("start");
    let (_, before) = take(calls, budget);
    let ([threads, tids, refused, _], after) = take(calls, budget);
    log!(logger, "[thread-limit] {} threads, the initial one included", threads);
    log!(logger, "[thread-limit] TIDs distinct and within 1..={}, all used: {}", MAX_THREADS, tids == 1);
    log!(logger, "[thread-limit] the next thread_create: TooManyThreads: {}", refused == 1);
    log!(logger, "[thread-limit] {} threads created, {} pages charged", threads - 1, after - before);

    let Received::Exit(notice) = rd::receive(Some(exit), WAIT, 0).expect("the exit notice") else {
        panic!("expected the holder's exit notice")
    };
    let back = notice.cause == Cause::Exited && pages() == empty;
    log!(logger, "[thread-limit] the holder exited and every page came back: {}", back);
    let charged = after - before == threads as u64 - 1;
    if created && threads == MAX_THREADS && tids == 1 && refused == 1 && charged && back {
        log!(logger, "THREAD LIMIT TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[thread-limit] FAIL: panic: {}", info);
    test_programs::park()
}
