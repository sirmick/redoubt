//! One expiry ends a budget's deadline `D` and, after it, an unrelated thread's receive timeout
//! `T` just past `D` (kernel/timer.md, "Expiry"): the expiry lists both due before it ends
//! either, and the destruction at `D` runs while `T`'s wait is still listed. The checked build's
//! audits must hold through it.
//!
//! Both fall due inside one long system call, so the expiry at the next kernel entry finds them
//! together: the call is a destruction by hand of a calibration budget's twin, made to straddle
//! `D` by half its measured cost. `T` is one microsecond after `D`, plus the receive's own entry.
//!
//! Checked: the machine is still here (a checked build); the lease `X` is gone, its process
//! killed; `R`, in a budget of its own, got `Timeout`, and by the time it ran again its handle
//! stamped with `X` was revoked; the call did straddle `D`.
//!
//! This program is the bundle's only one: it prints on the UART itself and powers off.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

fn now() -> u64 { rd::time_now().expect("time_now") }

fn ok(b: bool) -> &'static str { if b { "ok" } else { "FAIL" } }

extern "C" fn child(arg: usize) -> ! {
    match spawn::startup_byte(arg, 0) {
        // Blocked for ever, in the budget its parent destroys.
        1 => {
            let _ = rd::receive(None, rd::FOREVER, 0);
            rd::process_exit(0)
        }
        // R: be told `D`, wait on an endpoint nobody sends on until just past it, then try the
        // handle stamped with `X` and report.
        2 => {
            let Ok(Received::Message(m)) = rd::receive(Some(1), rd::FOREVER, 0) else { rd::process_exit(1) };
            let d = m.body.words[0] as u64 | (m.body.words[1] as u64) << 32;
            let r = rd::receive(Some(2), (d + 1).saturating_sub(now()), 0);
            let after = rd::send(3, &rd::body([0; 4]), None, 0);
            let good = r == Err(Error::Timeout) && after == Err(Error::BadHandle);
            let _ = rd::send(4, &rd::body([usize::from(good), 0, 0, 0]), None, rd::FOREVER);
            rd::process_exit(0)
        }
        _ => rd::process_exit(0),
    }
}

/// A budget of two processes blocked for ever: one destruction's worth of kernel time.
fn populated(image: &spawn::Image, exit: u32) -> u32 {
    let b = rd::create(rd::SYSTEM, &rd::spec(700, 2, 10)).unwrap();
    for _ in 0..2 {
        spawn::spawn(image, b, exit, child as *const () as usize, &[1], &[]).unwrap();
    }
    b
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    // SAFETY: this program is the only one, and owns the UART it just mapped.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();

    let exit = rd::endpoint_create().unwrap();
    let cmd = rd::endpoint_create().unwrap();
    let idle = rd::endpoint_create().unwrap();
    let probe = rd::endpoint_create().unwrap();
    let rep = rd::endpoint_create().unwrap();
    let cmd_client = rd::mint_from_handle(cmd, 1, None).unwrap();
    let rep_r = rd::mint_from_handle(rep, 2, None).unwrap();

    // What the long call costs: a destruction by hand of a budget of the twin's shape.
    let calibration = populated(&image, exit);
    let _ = rd::receive(None, 10_000, 0);
    let started = now();
    rd::destroy(calibration).unwrap();
    let cost = now() - started;
    for _ in 0..2 {
        let _ = rd::receive(Some(exit), 2_000_000, 0);
    }
    let twin = populated(&image, exit);

    // The lease X with its deadline D and a process in it; R outside it, holding a handle stamped
    // with it, told D, and left waiting until just past it.
    let d = now() + 200_000;
    let lease = rd::create(rd::SYSTEM, &rd::BudgetSpec { deadline: d, ..rd::spec(350, 1, 10) }).unwrap();
    spawn::spawn(&image, lease, exit, child as *const () as usize, &[1], &[]).unwrap();
    let stamped = rd::mint_from_handle(probe, 3, Some(lease)).unwrap();
    let br = rd::create(rd::SYSTEM, &rd::spec(350, 1, 10)).unwrap();
    spawn::spawn(&image, br, exit, child as *const () as usize, &[2], &[cmd, idle, stamped, rep_r]).unwrap();
    let words = [(d & 0xffff_ffff) as usize, (d >> 32) as usize, 0, 0];
    rd::send(cmd_client, &rd::body(words), None, rd::FOREVER).unwrap();
    let _ = rd::receive(None, 20_000, 0);

    // The long call, from half its cost before D: both fall due inside it.
    let from = d.saturating_sub(cost / 2);
    let early = now() < from;
    while now() < from {}
    let begun = now();
    rd::destroy(twin).unwrap();
    let ended = now();
    writeln!(
        out,
        "[expiry] {}: the call straddled the deadline (began {} µs before it, ended {} µs after; cost {} µs)",
        ok(early && begun < d && ended > d + 1),
        d.saturating_sub(begun),
        ended.saturating_sub(d),
        cost
    )
    .ok();

    // X went at D, its process killed; R timed out after it, its stamped handle already revoked.
    let gone = rd::usage(lease).err();
    let good = match rd::receive(Some(rep), 2_000_000, 0) {
        Ok(Received::Message(m)) => m.badge == 2 && m.body.words[0] == 1,
        _ => false,
    };
    let mut killed = 0;
    let mut exited = 0;
    for _ in 0..4 {
        match rd::receive(Some(exit), 2_000_000, 0) {
            Ok(Received::Exit(n)) if n.cause == Cause::Killed => killed += 1,
            Ok(Received::Exit(n)) if n.cause == Cause::Exited && n.code == 0 => exited += 1,
            _ => {}
        }
    }
    writeln!(
        out,
        "[expiry] {}: the lease ended at its deadline ({:?}), its process and the twin's two killed ({}), R exited ({})",
        ok(gone == Some(Error::BadHandle) && killed == 3 && exited == 1),
        gone,
        killed,
        exited
    )
    .ok();
    writeln!(
        out,
        "[expiry] {}: R timed out after the lease's destruction, its stamped handle revoked",
        ok(good)
    )
    .ok();
    rd::destroy(br).unwrap();

    writeln!(out, "EXPIRY-DEADLINE-THEN-TIMEOUT TEST PASSED").ok();
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // The UART is mapped at a kernel-chosen address; without it, park and let the bench time out.
    let _ = info;
    test_programs::park()
}
