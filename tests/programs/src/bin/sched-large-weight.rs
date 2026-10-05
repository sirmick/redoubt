//! A large-weight server keeps its share under load (R12): a server budget of weight 1000 (the
//! driver and steward weight in servers/init.md) against eight users of weight 100, all spinning,
//! gets 1000/1800 of the CPU. The share is the server's count over all nine counts (R12's relative
//! claim: what each budget ran, against the others), within 50 per thousand of 555: the lower side
//! closes a server held below its weight, the upper side the users starved alike to over-serve it.
//! Every user keeps its own: each user's count is within a tenth of the users' mean, which closes
//! one user starved to raise the server's ratio.
//! Each count over what the window would give one loop alone is printed beside, as the useful work
//! the kernel's per-slice time leaves.

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
    let mut users = [0usize; 8];
    for slot in users.iter_mut() {
        let u = b.budget(rd::USERS, 100, 1, rd::FOREVER);
        *slot = b.start(u, Role::Spin, &[], &[]);
    }
    let (start, end) = b.go(50_000, WINDOW);
    let counts = b.collect(9);
    let all: u64 = counts[..9].iter().sum();
    let ss = counts[s] * 1000 / all.max(1);
    let want = 1000 * 1000 / 1800;
    b.check(
        ss + TOL >= want && ss <= want + TOL,
        format_args!("the weight-1000 server got {} of 1000 of all counts, want {} within {}", ss, want, TOL),
    );
    let mean = users.iter().map(|&i| counts[i]).sum::<u64>() / 8;
    let least = users.iter().map(|&i| counts[i] * 1000 / mean.max(1)).min().unwrap_or(0);
    b.check(
        least >= 900,
        format_args!("the least user got {} per 1000 of the users' mean, want at least 900", least),
    );
    b.note(format_args!(
        "useful work: the server {} and all nine {} of 1000 of the window",
        b.share(counts[s], end - start),
        b.share(all, end - start)
    ));
    b.finish("SCHED-LARGE-WEIGHT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("large-weight", info) }
