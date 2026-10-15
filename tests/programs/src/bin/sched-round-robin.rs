//! Round-robin within a budget (kernel/scheduling.md, "One flat stride queue"): each pick runs the
//! budget's next runnable thread after the one it last picked, in (pid, tid) order, wrapping.
//!
//! One budget, two processes, two counting threads each, and nothing else runnable: the queue
//! picks this budget at every slice's end, so which thread runs is the round-robin's alone. Each
//! thread notes when the CPU came back to it after a gap of over half a slice (its resumptions)
//! and sends its first twelve, its slices and its count. The order of the four first runs is the
//! cycle; it must be (pid, tid) order from wherever the cursor stood: each process's two threads
//! adjacent, the main thread (the lower tid) first, and the second process's pair after the first's
//! or before it (PIDs are the kernel's). Every later round must run the cycle in the same order,
//! each thread once, and the four counts must be alike.
//!
//! One hart: on several a budget runs on as many harts as it has runnable threads, and the
//! rotation is among the threads no hart runs. A release build: the thread's gap detector reads
//! `rdtime`, and a checked build's audits at a slice's end would show as gaps of their own.

#![no_std]
#![no_main]

use test_programs::rd;
use test_programs::sched::{Bench, RESUMPTIONS, Role};

/// The rounds judged: the first runs and the resumptions after them.
const ROUNDS: usize = RESUMPTIONS;
/// Processes in the budget, threads in each.
const PROCESSES: usize = 2;
const THREADS: usize = 2;

/// A thread's reports: its resumptions (`rdtime` µs), its slices and its count.
#[derive(Clone, Copy, Default)]
struct Thread {
    resumed: [u32; RESUMPTIONS],
    got: usize,
    slices: u64,
    count: u64,
    counted: bool,
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("round-robin");
    let budget = b.budget(rd::USERS, 100, PROCESSES as u32, rd::FOREVER);
    let mut kids = [0usize; PROCESSES];
    for kid in kids.iter_mut() {
        *kid = b.start(budget, Role::RoundRobin, &[], &[]);
    }
    b.go(50_000, 100_000);
    // Each thread's five messages: four of resumptions, one of its slices and count.
    let mut threads = [[Thread::default(); THREADS]; PROCESSES];
    let mut messages = 0;
    while messages < PROCESSES * THREADS * (RESUMPTIONS / 3 + 1) {
        let Some((kid, w)) = b.receive_report() else { break };
        let Some(p) = kids.iter().position(|k| *k == kid) else { continue };
        let (k, seq) = (w[3] >> 8, w[3] & 0xff);
        if k >= THREADS {
            continue;
        }
        let t = &mut threads[p][k];
        if seq == 0xf {
            t.slices = w[0] as u64;
            t.count = w[1] as u32 as u64 | (w[2] as u32 as u64) << 32;
            t.counted = true;
        } else if seq < RESUMPTIONS / 3 {
            for (i, v) in w[..3].iter().enumerate() {
                t.resumed[seq * 3 + i] = *v as u32;
            }
            t.got += 3;
        }
        messages += 1;
    }
    let all = threads.iter().flatten().all(|t| t.counted && t.got == RESUMPTIONS);
    b.check(all, format_args!("every thread reported {} resumptions, its slices and its count", RESUMPTIONS));
    // The cycle: the four threads by their first run.
    let mut cycle = [(0usize, 0usize); PROCESSES * THREADS];
    for (i, slot) in cycle.iter_mut().enumerate() {
        *slot = (i / THREADS, i % THREADS);
    }
    cycle.sort_unstable_by_key(|&(p, k)| threads[p][k].resumed[0]);
    b.note(format_args!(
        "the cycle, by first run: {:?} (process, thread); first runs at {:?} µs",
        cycle,
        cycle.map(|(p, k)| threads[p][k].resumed[0])
    ));
    // (pid, tid) order from the cursor: each process's main thread, then its other, the two
    // processes in either order, as a rotation of the cycle.
    let in_order = (0..PROCESSES * THREADS).any(|phase| {
        let at = |i: usize| cycle[(phase + i) % (PROCESSES * THREADS)];
        (0..PROCESSES).all(|p| {
            let (first, second) = (at(p * THREADS), at(p * THREADS + 1));
            first.0 == second.0 && first.1 == 0 && second.1 == 1
        })
    });
    b.check(in_order, format_args!("the first runs follow (pid, tid) order: each process's two threads adjacent, the main thread first"));
    // Every round runs the cycle in the same order, each thread once, before the next round.
    let mut broken = None;
    for j in 0..ROUNDS {
        for i in 0..cycle.len() {
            let (p, k) = cycle[i];
            let next = if i + 1 < cycle.len() { (cycle[i + 1], j) } else { (cycle[0], j + 1) };
            if next.1 >= ROUNDS {
                continue;
            }
            let (here, there) = (threads[p][k].resumed[j], threads[next.0.0][next.0.1].resumed[next.1]);
            if here >= there && broken.is_none() {
                broken = Some((j, i, here, there));
            }
        }
    }
    match broken {
        None => b.check(true, format_args!("{} rounds ran the cycle in order, each thread once a round, wrapping", ROUNDS)),
        Some((j, i, here, there)) => b.check(
            false,
            format_args!(
                "{} rounds ran the cycle in order, each thread once a round, wrapping (round {}, position {}: resumed at {} µs, the next at {} µs)",
                ROUNDS, j, i, here, there
            ),
        ),
    }
    let counts = threads.iter().flatten().map(|t| t.count);
    let (least, most) = (counts.clone().min().unwrap_or(0), counts.max().unwrap_or(1));
    let slices = threads.iter().flatten().map(|t| t.slices);
    b.note(format_args!(
        "slices {:?}, counts {:?}",
        threads.map(|ts| ts.map(|t| t.slices)),
        threads.map(|ts| ts.map(|t| t.count))
    ));
    b.check(
        least * 1000 >= most * 900 && slices.clone().max().unwrap_or(0) <= slices.min().unwrap_or(0) + 1,
        format_args!("the four threads' counts are within a tenth of each other and their slices within one"),
    );
    rd::destroy(budget).unwrap();
    b.finish("SCHED-ROUND-ROBIN")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("round-robin", info) }
