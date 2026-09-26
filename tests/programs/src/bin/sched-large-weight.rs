//! A large-weight server keeps its share under load (R12): a server budget of weight 1000 (the
//! driver and steward weight in servers/init.md) against eight users of weight 100, all spinning,
//! gets 1000/1800 of the CPU.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, Role};

const WINDOW: u64 = 2_000_000;
const TOL: u64 = 50;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("large-weight");
    let server = b.budget(rd::SYSTEM, 1000, 1, rd::FOREVER);
    let s = b.start(server, Role::Spin, &[], &[]);
    for _ in 0..8 {
        let u = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        b.start(u, Role::Spin, &[], &[]);
    }
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(9);
    let ss = b.share(counts[s], end - start);
    b.check(
        ss + TOL >= 1000 * 1000 / 1800,
        format_args!("the weight-1000 server got {} of 1000, want {}", ss, 1000 * 1000 / 1800),
    );
    b.finish("SCHED-LARGE-WEIGHT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("large-weight", info) }
