//! The kernel containment gate (kernel/README.md, "Containment"): one composite boot showing
//! that the kernel's primitives alone contain hostile code before `init`, the steward or any
//! server is built on them.
//!
//! One program, the bundle's only one. It holds `root`, `system`, `users`, the console and the
//! Reset right, builds the workload, and prints every verdict line on the UART itself. Its
//! children hold no device and have no path to the console; every verdict comes from the kernel's
//! exit notices and trace, the victims and the oracle post-check, never from a hostile agent
//! (rule F, docs/testbench.md).
//!
//! The workload is the latency case's: a driver stand-in on the goldfish RTC's alarm interrupt,
//! a steward stand-in on timeouts, and a bystander session. The steward carves hostile leases
//! from a `sessions` budget under `users`, two slots (D: ended by deadline, H: ended by its
//! decision 5 ms after arming), running nine leases each. Each agent spins, enters the kernel in
//! a tight loop, parks lend calls and blocked sends at the victim through lease-stamped handles,
//! runs a sub-agent with a later deadline, and fills its lease and handle table. Targets are the
//! responsiveness ones, and the gate adds none: the program judges none of them, it prints each
//! sample's window (`LATENCY-SAMPLE`), and the bench's post-check judges them net of the checked
//! build's audits (the bounds are tests/kernel-containment.toml's). The bystander's share (R12)
//! is the post-check's too: its share of the CPU the kernel charged under `users`, from the trace
//! (`CHARGED-SHARE`), in a window the steward opens once both slots' leases run; the program
//! prints its count beside, as its useful work.
//!
//! See `tests/kernel-containment.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Received};
use test_programs::sched::{
    Bench, CT_CALLERS, CT_LEASES, CT_STEADY, CT_VERDICT, K, R10_P99, Role, Stats, containment_child, join,
    rtc,
};

/// How far the bystander's share may lie from its weight's, thousandths (R12's 50).
const TOLERANCE: u64 = 50;
/// The weights of the empty children that mark the bystander and `sessions` in the kernel's trace
/// for the post-check: no lease (10) or sub-agent (1) weighs either.
const BYSTANDER_MARK: u32 = 2;
const SESSIONS_MARK: u32 = 3;

/// Mark `budget` in the kernel's trace: carve an empty child of weight `weight` and destroy it, so
/// the trace's lift names `budget` as its parent.
fn mark(budget: u32, weight: u32) {
    let child = rd::create(budget, &rd::spec(0, 0, weight)).expect("a mark");
    rd::destroy(child).expect("a mark's destruction");
}

fn verdict(met: bool) -> &'static str { if met { "met" } else { "missed" } }

fn receive_message(from: u32) -> rd::Message {
    match rd::receive(Some(from), 30_000_000, 0).expect("a message") {
        Received::Message(m) => m,
        _ => panic!("expected a message"),
    }
}

fn handle(m: &rd::Message, i: usize) -> u32 {
    m.body.handles.as_slice().get(i).copied().flatten().map(|h| h.index()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::first_free();
    let mut b = Bench::new("containment");
    b.set_entry(containment_child);
    let Some((rtc_mmio, rtc_base, rtc_irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("KERNEL-CONTAINMENT")
    };
    // The RTC must run on the virtual clock (`-rtc clock=vm`) for its column to be virtual time.
    let (r0, u0) = (rtc::now_ns(rtc_base), b.now_us());
    let _ = rd::receive(None, 100_000, 0);
    let (r1, u1) = (rtc::now_ns(rtc_base), b.now_us());
    let (rtc_us, us) = ((r1 - r0) / 1000, u1 - u0);
    b.note(format_args!(
        "the RTC keeps virtual time: {} ({} µs of RTC over {} µs of time_now)",
        verdict(rtc_us.abs_diff(us) * 100 <= us),
        rtc_us,
        us
    ));

    // The endpoint maker, once, in `users` itself: it makes the victim's endpoints and the
    // steward's progress endpoint, stamped and owned by `users`, hands their copies over, and
    // exits. An endpoint outlives its maker; the steward mints into each lease from the
    // `users`-stamped receive rights it gets here.
    let handoff = rd::endpoint_create().expect("the handoff");
    let handoff_send = rd::mint_from_handle(handoff, 0x77, None).expect("handoff send");
    let mut mk = [0u8; 1 + 8 * 8];
    mk[0] = Role::EndpointMaker as u8;
    test_programs::spawn::spawn(
        b.image(),
        rd::USERS,
        b.exit_endpoint(),
        containment_child as *const () as usize,
        &mk,
        &[handoff_send],
    )
    .expect("the endpoint maker");
    let (mut call_rx, mut send_d, mut send_h) = (0u32, 0u32, 0u32);
    let (mut queue_d, mut queue_h, mut progress) = (0u32, 0u32, 0u32);
    let (mut gift_rx, mut gift_send) = (0u32, 0u32);
    let (mut queue_d_send, mut queue_h_send) = (0u32, 0u32);
    for _ in 0..5 {
        let m = receive_message(handoff);
        match m.body.words[0] {
            1 => {
                call_rx = handle(&m, 0);
                send_d = handle(&m, 1);
                send_h = handle(&m, 2);
                queue_d = handle(&m, 3);
            }
            2 => {
                call_rx = handle(&m, 0);
                send_d = handle(&m, 1);
                send_h = handle(&m, 2);
                progress = handle(&m, 3);
            }
            3 => gift_send = handle(&m, 0),
            4 => {
                gift_rx = handle(&m, 0);
                queue_d_send = handle(&m, 1);
                queue_h_send = handle(&m, 2);
            }
            5 => queue_h = handle(&m, 0),
            _ => {}
        }
    }

    // The budget tree at `init`'s weights (servers/init.md): a driver and the steward at 1000, a
    // victim and a bystander at 100, and the sessions budget the steward carves leases from.
    let driver_b = rd::create(rd::SYSTEM, &rd::spec(400, 2, 1000)).expect("driver budget");
    let steward_b = rd::create(rd::SYSTEM, &rd::spec(2000, 8, 1000)).expect("steward budget");
    let victim_b = rd::create(rd::SYSTEM, &rd::spec(400, 2, 100)).expect("victim budget");
    let sessions = rd::create(rd::USERS, &rd::spec(20000, 16, 100)).expect("sessions");
    let bystander_b = rd::create(rd::USERS, &rd::spec(400, 2, 100)).expect("bystander budget");
    // `users` holds these two: the bystander's share is of what the kernel charges under them.
    mark(bystander_b, BYSTANDER_MARK);
    mark(sessions, SESSIONS_MARK);
    // The bystander's window comes from the steward, once both slots' leases run.
    let window_rx = rd::endpoint_create().expect("the bystander's window");
    let window_send = rd::mint_from_handle(window_rx, 0x78, None).expect("the window's send");
    // The driver and the steward hold their samples' windows until asked, below.
    let d = b.start(driver_b, Role::Driver, &[K, 1], &[rtc_mmio, rtc_irq]);
    let s = b.start(
        steward_b,
        Role::Containment,
        &[],
        &[sessions, call_rx, send_d, send_h, progress, gift_send, window_send],
    );
    let v = b.start(victim_b, Role::Victim, &[], &[call_rx, send_d, send_h, queue_d, queue_h, victim_b]);
    let y = b.start(bystander_b, Role::Bystander, &[], &[gift_rx, queue_d_send, queue_h_send, window_rx]);

    // The children wait for the go; none counts to the go window's end (the bystander's window is
    // the steward's).
    b.go(50_000, 0);
    // Driver 2, steward 5, victim 1, bystander 1.
    let mut words = [[[0usize; 4]; 32]; 64];
    let mut seen = [0usize; 64];
    let (mut have_s, mut have_v, mut have_y, mut have_d) = (false, false, false, false);
    loop {
        if let Some((i, w)) = b.receive_report() {
            if i < 64 && seen[i] < 32 {
                words[i][seen[i]] = w;
                seen[i] += 1;
            }
            if i == s && w[3] & 0xff == CT_VERDICT && w[3] >> 8 > 0 {
                have_s = true;
            }
            if i == d && w[3] & 0xff == Stats::DRIVER_LOST {
                have_d = true;
            }
            if i == v {
                have_v = true;
            }
            if i == y {
                have_y = true;
            }
            if have_s && have_v && have_y && have_d {
                break;
            }
        }
    }
    let stat = |i: usize, tag: usize| words[i].iter().find(|w| w[3] & 0xff == tag && w[3] >> 8 > 0).copied();

    b.note(format_args!("latencies in µs of virtual (instruction) time: p50 / p99 / max (samples)"));
    // Gross, as measured; the post-check judges each net of the audits inside its windows.
    for (i, tag, what) in [
        (d, Stats::DRIVER_WAKE, "driver wake"),
        (s, Stats::TIMER_WAKE, "steward timer wake"),
        (s, Stats::DECISION_WAKE, "steward decision wake"),
        (s, Stats::DEADLINE, "deadline notice"),
    ] {
        match stat(i, tag) {
            Some(w) => b.note(format_args!(
                "{}: {} / {} / {} ({}): gross, audits included; net in the post-check",
                what,
                w[0],
                w[1],
                w[2],
                w[3] >> 8
            )),
            None => b.check(false, format_args!("{}: no samples", what)),
        }
    }
    match stat(s, Stats::DESTROY) {
        Some(w) => b.note(format_args!(
            "budget_destroy, call to return: {} / {} / {} ({}); recorded, R10 p99 target {} µs",
            w[0],
            w[1],
            w[2],
            w[3] >> 8,
            R10_P99
        )),
        None => b.check(false, format_args!("budget_destroy: no samples")),
    }
    b.samples(d, &words[d][..seen[d]], format_args!("gate"));
    b.samples(s, &words[s][..seen[s]], format_args!("gate"));

    // The steward's own checks: every process killed blaming nobody, every handle closed, and
    // sessions' usage returned (I10).
    // Every lease the steward made, the slot D leases made again included.
    let mut leases = None;
    match stat(s, CT_VERDICT) {
        Some(w) => {
            leases = Some(2 * CT_LEASES + w[1]);
            let fails = w[0] as u64;
            b.check(
                fails & 8 == 0,
                format_args!("every process in a lease, its sub-agent included, was killed blaming nobody"),
            );
            b.check(fails & 1 == 0, format_args!("every hand destroy succeeded"));
            b.check(
                fails & 32 == 0,
                format_args!("every lease filled its handle table to MAX_HANDLES before its pages ran out"),
            );
            b.check(
                fails & 64 == 0,
                format_args!("the bystander's window opened under both slots' leases and closed before slot D's deadline"),
            );
            b.check(
                fails & 16 == 0,
                format_args!("every lease armed and was given its carried handle while it lived"),
            );
            b.note(format_args!("slot D leases made again because their deadline came first: {}", w[1]));
            b.check(fails & 2 == 0, format_args!("every handle the steward held to a lease was closed"));
            b.check(
                fails & 4 == 0,
                format_args!("sessions' usage returned to what it was before each round"),
            );
        }
        None => b.check(false, format_args!("the steward: no verdict")),
    }

    // The victim's checks: every lease's calls held and each abandoned once, lend bytes intact,
    // replies discarded with mask 0; nothing sent through a lease's handles left after its end,
    // the queued stamped handle arrived as 0; usage back. The victim tells leases apart by the
    // badges the steward minted and counts each lease's calls, so a call no lease made, or one
    // missing from any lease, fails the row.
    let vw = words[v][0];
    let vf = vw[0] as u64;
    let calls = leases.map(|l| l * CT_CALLERS);
    b.check(
        vf & (1 | 16 | 32 | 64 | 256) == 0 && calls == Some(vw[2]) && vw[3] == 0,
        format_args!(
            "every held call was abandoned once, its lend intact and its reply discarded ({} of {:?} checked, {} left)",
            vw[2], calls, vw[3]
        ),
    );
    b.check(
        vf & (2 | 4 | 8) == 0,
        format_args!("nothing sent through a lease's handles outlived it, and no queued message did"),
    );
    b.check(vf & 128 == 0, format_args!("the victim's usage returned to its start"));

    // The bystander's share (R12): of the CPU the kernel charged under `users` in the window the
    // steward opened, which the post-check reads from the trace. Its count is its useful work,
    // printed beside.
    match stat(s, CT_STEADY) {
        Some(w) => {
            let start = join(w[0], w[1]);
            let length = w[2] as u64;
            b.charged_share(
                "bystander",
                (start, start + length),
                TOLERANCE,
                &[BYSTANDER_MARK, SESSIONS_MARK],
            );
            b.note(format_args!(
                "the bystander's count under both slots' leases: {} of 1000 of the window ({} µs), gross: its useful work; its share of the CPU charged under users is the post-check's",
                b.share(words[y][0][0] as u64, length),
                length
            ));
        }
        None => b.check(false, format_args!("the steward opened no window for the bystander")),
    }

    b.finish("KERNEL-CONTAINMENT")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("containment", info) }
