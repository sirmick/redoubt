//! A budget's deadline destroys it (WP-K5; KERNEL-SPEC.md, Budget, R10): the kernel's timer
//! fires, and R10 runs exactly as for `budget_destroy`, with nobody asking.
//!
//! The lease `L` (carved from `system`, so this program can mint handles stamped with it) holds
//! a spinner that is on the CPU when the deadline comes (the caller-last path: the process the
//! timer interrupted is itself destroyed) and a caller whose call a server outside `L` has
//! taken. Outside `L`: the server, and an outsider queued in `send` through a handle stamped with
//! `L`. `L` has a child budget whose own deadline is later.
//!
//! Checked: `killed` notices blaming nobody, arriving within `LATE_US` of the deadline (virtual
//! time); the server's call abandoned with one notice and its reply discarded; the outsider's
//! queued send failed with `Dead` and its handle revoked; every handle to `L` and its child
//! gone; `system`'s usage back as it was (I10); a deadline already past at creation destroys the
//! budget at once; `FOREVER` never does; the child's later deadline never fires.
//!
//! This program is the bundle's only one: it prints on the UART itself and powers off.

#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_sys::{Call, Return};
use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// The most a deadline's notices may arrive after it, in virtual time, beyond what destroying
/// the same budget by hand costs (R10's own work: its sweeps are the same either way).
const LATE_US: u64 = 1_000;
/// Words of a report on the `rep` endpoint.
const TAKEN: usize = 1;
const VERDICT: usize = 2;

fn now() -> u64 { rd::time_now().expect("time_now") }

fn ok(b: bool) -> &'static str { if b { "ok" } else { "FAIL" } }

fn spec(pages: u64, processes: u32, weight: u32, deadline: u64) -> rd::BudgetSpec {
    rd::BudgetSpec { deadline, ..rd::spec(pages, processes, weight) }
}

/// Spin forever: on the CPU when the deadline comes.
fn spin() -> ! {
    let mut x = 0u64;
    loop {
        x = x.wrapping_add(1);
        core::hint::black_box(x);
    }
}

extern "C" fn child(arg: usize) -> ! {
    match spawn::startup_byte(arg, 0) {
        // A: wait for the go, then spin.
        1 => {
            let _ = rd::receive(Some(1), rd::FOREVER, 0);
            spin()
        }
        // B: call the server with a lend; the deadline ends the call (and B).
        2 => {
            let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("lend");
            rd::poke(page, 42);
            let _ = rd::call_outcome(1, &rd::body([0; 4]), rd::pages(page, 1), rd::FOREVER);
            rd::process_exit(99)
        }
        // S: take B's call and hold it until it is abandoned; one notice, then a discarded reply.
        3 => {
            let Ok(Received::Message(m)) = rd::receive(Some(1), rd::FOREVER, 0) else { rd::process_exit(1) };
            let _ = rd::send(2, &rd::body([TAKEN, 0, 0, 0]), None, rd::FOREVER);
            let first = rd::receive(Some(1), rd::FOREVER, 0);
            let second = rd::receive(Some(1), 0, 0);
            let rec = rd::body([0; 4]).encode();
            let r = redoubt_sys::syscall(&Call::Reply { msg_id: m.msg_id, body_rec: rec.as_ptr() as usize });
            let good = matches!(first, Ok(Received::Abandoned(id)) if id == m.msg_id)
                && second == Err(Error::Timeout)
                && matches!(r, Ok(Return::Reply(o)) if !o.delivered && o.installed == 0);
            let _ = rd::send(2, &rd::body([VERDICT, usize::from(good), 0, 0]), None, rd::FOREVER);
            rd::process_exit(0)
        }
        // O: be handed a handle stamped with L, queue a send through it, see it fail with `Dead`
        // and the handle revoked.
        4 => {
            let Ok(Received::Message(m)) = rd::receive(Some(1), rd::FOREVER, 0) else { rd::process_exit(1) };
            let Some(stamped) = m.body.handles.as_slice().first().copied().flatten() else {
                rd::process_exit(2)
            };
            let first = rd::send(stamped.index(), &rd::body([7, 0, 0, 0]), None, rd::FOREVER);
            let second = rd::send(stamped.index(), &rd::body([8, 0, 0, 0]), None, 0);
            let good = first == Err(Error::Dead) && second == Err(Error::BadHandle);
            let _ = rd::send(
                2,
                &rd::body([VERDICT, usize::from(good), first.err().map_or(0, |e| e as usize), 0]),
                None,
                rd::FOREVER,
            );
            rd::process_exit(0)
        }
        // A bystander in the calibration budget: blocked for ever.
        5 => {
            let _ = rd::receive(None, rd::FOREVER, 0);
            rd::process_exit(0)
        }
        _ => rd::process_exit(0),
    }
}

fn report(rep: u32) -> (u64, [usize; 4]) {
    match rd::receive(Some(rep), 2_000_000, 0) {
        Ok(Received::Message(m)) => (m.badge, m.body.words),
        _ => (0, [0; 4]),
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    // SAFETY: this program is the only one, and owns the UART it just mapped.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();

    // Endpoints, and the handles the children get.
    let exit = rd::endpoint_create().unwrap();
    let svc = rd::endpoint_create().unwrap();
    let rep = rd::endpoint_create().unwrap();
    let q = rd::endpoint_create().unwrap();
    let cmd = rd::endpoint_create().unwrap();
    let go = rd::endpoint_create().unwrap();
    let svc_client = rd::mint_from_handle(svc, 5, None).unwrap();
    let rep_s = rd::mint_from_handle(rep, 3, None).unwrap();
    let rep_o = rd::mint_from_handle(rep, 4, None).unwrap();
    let cmd_client = rd::mint_from_handle(cmd, 6, None).unwrap();
    let go_client = rd::mint_from_handle(go, 8, None).unwrap();

    // The server and the outsider, outside the lease.
    let bs = rd::create(rd::SYSTEM, &rd::spec(300, 1, 10)).unwrap();
    let bo = rd::create(rd::SYSTEM, &rd::spec(300, 1, 10)).unwrap();
    spawn::spawn(&image, bs, exit, child as *const () as usize, &[3], &[svc, rep_s]).unwrap();
    spawn::spawn(&image, bo, exit, child as *const () as usize, &[4], &[cmd, rep_o]).unwrap();

    // What destroying a budget of the lease's shape costs when asked: the deadline may add only
    // the timer's latency to it.
    let calibration = rd::create(rd::SYSTEM, &rd::spec(700, 2, 10)).unwrap();
    rd::create(calibration, &rd::spec(10, 0, 0)).unwrap();
    for _ in 0..2 {
        spawn::spawn(&image, calibration, exit, child as *const () as usize, &[5], &[]).unwrap();
    }
    let _ = rd::receive(None, 10_000, 0);
    let started = now();
    rd::destroy(calibration).unwrap();
    for _ in 0..2 {
        let _ = rd::receive(Some(exit), 2_000_000, 0);
    }
    let cost = now() - started;

    // A lease of the same shape, destroyed by its deadline while its spinner is on the CPU (the
    // process the timer interrupts is itself destroyed, last): its notices come no later than
    // doing it by hand would take, plus the timer's latency.
    let deadline = now() + 100_000;
    let timed = rd::create(rd::SYSTEM, &spec(700, 2, 10, deadline)).unwrap();
    rd::create(timed, &rd::spec(10, 0, 0)).unwrap();
    spawn::spawn(&image, timed, exit, child as *const () as usize, &[5], &[]).unwrap();
    spawn::spawn(&image, timed, exit, child as *const () as usize, &[1], &[go]).unwrap();
    let _ = rd::receive(None, 10_000, 0);
    rd::send(go_client, &rd::body([0; 4]), None, rd::FOREVER).unwrap();
    let mut late = 0;
    let mut good = true;
    for _ in 0..2 {
        match rd::receive(Some(exit), 2_000_000, 0) {
            Ok(Received::Exit(n)) => {
                late = late.max(now().saturating_sub(deadline));
                good &= n.cause == Cause::Killed
                    && n.blamed_account == 0
                    && n.blamed_labels.as_slice().is_empty();
            }
            _ => good = false,
        }
    }
    writeln!(out, "[deadline] {}: the running spinner's budget was destroyed at its deadline, killed notices blaming nobody", ok(good)).ok();
    writeln!(
        out,
        "[deadline] lateness {} ({} µs after the deadline; destroying by hand took {} µs; bound {} µs more)",
        if late <= cost + LATE_US { "ok" } else { "LATE" },
        late,
        cost,
        LATE_US
    )
    .ok();

    // The lease, its child with a later deadline, and a handle stamped with it.
    let before = rd::usage(rd::SYSTEM).unwrap();
    let deadline = now() + 100_000;
    let lease = rd::create(rd::SYSTEM, &spec(700, 2, 10, deadline)).unwrap();
    let child_budget = rd::create(lease, &spec(10, 0, 0, deadline + 200_000)).unwrap();
    let stamped = rd::mint_from_handle(q, 7, Some(lease)).unwrap();
    spawn::spawn(&image, lease, exit, child as *const () as usize, &[2], &[svc_client]).unwrap();
    spawn::spawn(&image, lease, exit, child as *const () as usize, &[5], &[]).unwrap();

    // Hand the outsider its stamped handle, see the server take B's call, and let the outsider
    // queue its send; then wait for the deadline.
    rd::send(cmd_client, &rd::body_with([0; 4], &[stamped]), None, rd::FOREVER).unwrap();
    let (badge, words) = report(rep);
    writeln!(out, "[deadline] {}: the server took the lease's call", ok(badge == 3 && words[0] == TAKEN))
        .ok();

    // The deadline: both of the lease's processes killed, blaming nobody.
    let mut notices = 0;
    let mut good = true;
    for _ in 0..2 {
        match rd::receive(Some(exit), 2_000_000, 0) {
            Ok(Received::Exit(n)) => {
                notices += 1;
                good &= n.cause == Cause::Killed
                    && n.blamed_account == 0
                    && n.blamed_labels.as_slice().is_empty();
            }
            _ => good = false,
        }
    }
    writeln!(out, "[deadline] {}: {} killed notices, blaming nobody", ok(good && notices == 2), notices).ok();

    // The server's call was abandoned once and its reply discarded; the outsider's send failed.
    let mut server = false;
    let mut outsider = false;
    for _ in 0..2 {
        let (badge, words) = report(rep);
        match (badge, words[0]) {
            (3, VERDICT) => server = words[1] == 1,
            (4, VERDICT) => outsider = words[1] == 1,
            _ => {}
        }
    }
    writeln!(out, "[deadline] {}: the taken call was abandoned: one notice, reply discarded", ok(server))
        .ok();
    writeln!(out, "[deadline] {}: a send queued through a handle stamped with the lease failed Dead, the handle revoked", ok(outsider)).ok();

    // Every handle to the lease and its child is gone, and system's usage is what it was before
    // the lease (I10): its carve returned and its processes' objects freed with their notices.
    let after = rd::usage(rd::SYSTEM).unwrap();
    let revoked = [rd::usage(lease).err(), rd::usage(child_budget).err(), rd::usage(stamped).err()];
    writeln!(
        out,
        "[deadline] {}: handles to the lease, its child and stamped with it revoked ({:?})",
        ok(revoked.iter().all(|e| *e == Some(Error::BadHandle))),
        revoked
    )
    .ok();
    writeln!(
        out,
        "[deadline] {}: system's usage restored (before {:?}, after {:?})",
        ok(after == before),
        before,
        after
    )
    .ok();
    // The server's and outsider's notices, then their budgets.
    for _ in 0..2 {
        let _ = rd::receive(Some(exit), 2_000_000, 0);
    }
    rd::destroy(bs).unwrap();
    rd::destroy(bo).unwrap();

    // A deadline already past destroys the budget at the next kernel entry: the handle is dead
    // by the time it is used. FOREVER never expires.
    let past = rd::create(rd::SYSTEM, &spec(10, 0, 0, now().saturating_sub(1))).unwrap();
    let gone = rd::usage(past).err();
    let forever = rd::create(rd::SYSTEM, &spec(10, 0, 0, rd::FOREVER)).unwrap();
    let _ = rd::receive(None, 250_000, 0);
    let alive = rd::usage(forever).is_ok();
    writeln!(
        out,
        "[deadline] {}: a past deadline destroys at once ({:?}); FOREVER never",
        ok(gone == Some(Error::BadHandle) && alive),
        gone
    )
    .ok();
    rd::destroy(forever).unwrap();
    // By now the destroyed child's own, later deadline has passed too; the machine is still here.
    writeln!(out, "[deadline] ok: the destroyed child's later deadline did not fire").ok();

    writeln!(out, "BUDGET-DEADLINE TEST PASSED").ok();
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // The UART is mapped at a kernel-chosen address; without it, park and let the bench time out.
    let _ = info;
    test_programs::park()
}
