//! A fresh lift on a small shared parent delays the sibling made under it (kernel/scheduling.md,
//! "Inheritance" and "Residual risks"). Under `users`, sixteen spinners of weight 20 run; three
//! phases each carve a parent P of the peers' weight, half or a tenth of it (20, 10, 2), holding
//! no process, and a child C of one less with a spinner. Picked after a round, C wakes the
//! launcher and spins on: the launcher, a waker at the floor, is picked when C's slice ends and
//! destroys C, whose lead over the floor is then that slice or a little more. The very next call
//! makes the sibling S under P (weight 2, 5 and 1), which enters at P's lifted pass; S reads
//! `time_now` first thing and reports it. The bench's oracle (`lift-delay`) finds each phase from
//! P's records, computes the rounds the lift should cost S from the lift's own records, and counts
//! how often any other budget was picked before S's first run.
//!
//! The peers weigh 20, not more, so that the launcher's 1,000 takes most of the CPU while it
//! destroys C and starts S: the floor the peers move meanwhile eats into P's lead, and at the
//! peers' 100 starting S cost about a round, all of the lead on a parent of their weight.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{self, Bench, Role, join};
use test_programs::spawn;

const N: u64 = 16;

/// The spinners' weight.
const PEER: u32 = 20;

/// Each phase: P's weight, C's, S's.
const PHASES: [(u32, u32, u32); 3] =
    [(PEER, PEER - 1, 2), (PEER / 2, PEER / 2 - 1, 5), (PEER / 10, PEER / 10 - 1, 1)];

/// A gap in C's `rdtime` this long is a pick after it waited out the others (a round of sixteen
/// slices at least), not an interrupt or an audit inside its slice.
const GAP_US: u64 = 5_000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("lift-delay");
    for _ in 0..N {
        let s = b.budget(rd::USERS, PEER, 1, rd::FOREVER);
        b.start(s, Role::Spin, &[], &[]);
    }
    b.go(50_000, 20_000_000);
    // The spinners are running before the first phase.
    let _ = rd::receive(None, 100_000, 0);
    b.set_entry(lift_child);
    let mut ran = 0;
    for (n, &(wp, wc, ws)) in PHASES.iter().enumerate() {
        let p = b.budget(rd::USERS, wp, 2, rd::FOREVER);
        let c = b.budget(p, wc, 1, rd::FOREVER);
        let ci = b.start(c, Role::Spin, &[], &[]);
        // C wakes the launcher at the start of one of its slices; the launcher runs when it ends.
        report_of(&mut b, ci);
        rd::destroy(c).unwrap();
        let s = b.budget(p, ws, 1, rd::FOREVER);
        let created = b.now_us();
        let si = b.start(s, Role::Probe, &[], &[]);
        if let Some(w) = report_of(&mut b, si) {
            let at = join(w[0], w[1]);
            b.note(format_args!(
                "phase {}: S ran at {at} us, {} us after its creation (P {wp}, C {wc}, S {ws})",
                n + 1,
                at.saturating_sub(created)
            ));
            ran += 1;
        }
        rd::destroy(s).unwrap();
        rd::destroy(p).unwrap();
    }
    b.check(ran == PHASES.len(), format_args!("the sibling ran in each phase (its delay is the oracle's)"));
    b.finish("SCHED-LIFT-DELAY")
}

/// The phases' children: C (`Role::Spin`) and S (`Role::Probe`).
extern "C" fn lift_child(arg: usize) -> ! {
    if spawn::startup_byte(arg, 0) == Role::Probe as u8 {
        report(rd::time_now().unwrap_or(0));
        rd::process_exit(0)
    }
    // The launcher's ticks per µs, the last startup word.
    let tpu =
        (0..8).fold(0u64, |v, i| v | u64::from(spawn::startup_byte(arg, 1 + 7 * 8 + i)) << (8 * i)).max(1);
    // Spin until a pick after a round, then wake the launcher and spin until destroyed.
    let mut last = sched::ticks();
    loop {
        let now = sched::ticks();
        if now - last > GAP_US * tpu {
            break;
        }
        last = now;
    }
    report(0);
    loop {
        core::hint::spin_loop();
    }
}

/// The report of child `i`, skipping any other's (a spinner's at its window's end).
fn report_of(b: &mut Bench, i: usize) -> Option<[usize; 4]> {
    loop {
        match b.receive_report() {
            Some((from, w)) if from == i => return Some(w),
            Some(_) => {}
            None => return None,
        }
    }
}

/// Report `value` to the launcher (slot 1).
fn report(value: u64) {
    let _ = rd::send(
        1,
        &rd::body([value as u32 as usize, (value >> 32) as u32 as usize, 0, 0]),
        None,
        rd::FOREVER,
    );
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { sched::panicked("lift-delay", info) }
