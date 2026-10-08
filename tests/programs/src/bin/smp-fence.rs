//! New code reaches a hart already running its process (kernel/memory.md, "Instruction fetch
//! after mapping"; `bench:smp-fence`), at 2 harts, in a trace build.
//!
//! P's sibling S spins on one hart, polling a word. P's thread T, on the other, maps a page
//! writable, writes a function into it, makes it read-execute with `set_flags`, and stores its
//! address into the word. S calls the function and tells the judge what it returned.
//!
//! QEMU keeps instruction fetch coherent with stores, so no run can see a missing `fence.i`: the
//! verdict is the trace's (`post_check = "smp_fence"`). The `set_flags` shot P down on S's hart,
//! which ran `fence.i` and acknowledged, and its shootdown record says so. Without the shootdown
//! (`smp-no-shootdown`) there is no such record, and the check fails.
#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, MemFlags, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// What the function returns.
const ANSWER: usize = 0x5f;
/// The function, on both widths: `addi a0, zero, 0x5f` then `ret`, little-endian.
const CODE: u64 = 0x0000_8067_05f0_0513;
/// Message tags.
const RAN: usize = 1;

/// S: wait for the code's address in `word`, call it, and report what it returned.
fn sibling(word: usize) {
    rd::poke(word + 8, 1);
    while rd::peek(word) == 0 {}
    // SAFETY: T stored the address of a page of this process it made read-execute after writing a
    // whole function into it, `CODE`, which takes no arguments, returns a word in `a0` and touches
    // nothing else.
    let code: extern "C" fn() -> usize = unsafe { core::mem::transmute(rd::peek(word) as usize) };
    let answer = code();
    let _ = rd::send(1, &rd::body([RAN, answer, 0, 0]), None, WAIT);
    rd::process_exit(0)
}

extern "C" fn writer(_: usize) -> ! {
    let word = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(1));
    rd::poke(word, 0);
    rd::poke(word + 8, 0);
    if rd::thread(sibling, word).is_err() {
        rd::process_exit(1);
    }
    // S is on the other hart, polling, before the code is made.
    let start = rd::time_now().unwrap_or(0);
    while rd::peek(word + 8) == 0 {
        if rd::time_now().unwrap_or(u64::MAX) > start + WAIT {
            rd::process_exit(2);
        }
    }
    let code = rd::map_anon(rd::PAGE_SIZE, rd::rw()).unwrap_or_else(|_| rd::process_exit(3));
    rd::poke(code, CODE);
    if rd::set_flags(code, rd::PAGE_SIZE, MemFlags::READ | MemFlags::EXECUTE).is_err() {
        rd::process_exit(4);
    }
    rd::poke(word, code as u64);
    loop {
        let _ = rd::receive(None, WAIT, 0);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("smp-fence");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");
    let p = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("P's budget");
    spawn::spawn(&image, p, exit, writer as *const () as usize, &[], &[inbox]).expect("P");
    let answer = match rd::receive(Some(inbox), WAIT, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == RAN => Some(m.body.words[1]),
        _ => None,
    };
    b.check(answer == Some(ANSWER), format_args!("S ran the code T made on the other hart ({:?})", answer));
    let ended = matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Exited);
    b.check(ended, format_args!("P exited"));
    b.finish("SMP-FENCE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("smp-fence", info) }
