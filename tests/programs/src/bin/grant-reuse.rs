//! A device grant is its loader program's alone (WP-K5; K5-code-review-2 P3-2): a process that
//! `process_create` gives the PID of a granted program that has exited inherits nothing.
//!
//! Grants are keyed by PID (kernel `grants.rs`, INTERIM until WP-K6), and `process_create` draws
//! a random free PID. So this program, the steward's stand-in, spawns children into `system` and
//! `users` until one of them is PID 3, `grant-donor`'s: each child reads its PID, and PID 3 tries
//! the donor's grants (the legacy interrupt claim, whose callbacks would hold every budget
//! deadline while they run, and the legacy device map). Every other child stays alive, holding
//! its PID, so the draw ends within the 62 free PIDs.
//!
//! This program is the bundle's first: it prints on the UART itself and powers off.

#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_abi::{MemoryAddress, MemoryFlags, SysCall};
use test_programs::rd::{self, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// `grant-donor`'s PID and grants (the case's manifest).
const DONOR: usize = 3;
const IRQ: usize = 40;
const MMIO: usize = 0x0010_1000;

fn never_called(_irq: usize, _arg: *mut usize) {}

extern "C" fn child(_arg: usize) -> ! {
    let pid = redoubt_abi::current_pid().map_or(0, |p| p.get() as usize);
    let mut words = [pid, 0, 0, 0];
    if pid == DONOR {
        let claim = redoubt_abi::claim_interrupt(IRQ, never_called, core::ptr::null_mut());
        let map = redoubt_abi::rsyscall(SysCall::MapMemory(
            MemoryAddress::new(MMIO),
            None,
            redoubt_abi::MemorySize::new(4096).unwrap(),
            MemoryFlags::R | MemoryFlags::W,
        ));
        words[1] = usize::from(claim == Err(redoubt_abi::Error::AccessDenied));
        words[2] = usize::from(map == Err(redoubt_abi::Error::AccessDenied));
        words[3] = usize::from(claim.is_ok() || map.is_ok());
    }
    let _ = rd::send(1, &rd::body(words), None, rd::FOREVER);
    // Hold the PID.
    let _ = rd::receive(None, rd::FOREVER, 0);
    rd::process_exit(0)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    // SAFETY: the bundle's first program owns the UART it just mapped; the donor never prints.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();
    let exit = rd::endpoint_create().unwrap();
    let rep = rd::endpoint_create().unwrap();
    let rep_client = rd::mint_from_handle(rep, 1, None).unwrap();

    // The donor uses its grants and exits.
    let _ = rd::receive(None, 50_000, 0);

    let mut spawned = 0;
    let mut verdict = None;
    'draw: for budget in [rd::SYSTEM, rd::USERS] {
        while spawn::spawn(&image, budget, exit, child as *const () as usize, &[], &[rep_client]).is_ok() {
            spawned += 1;
            match rd::receive(Some(rep), 2_000_000, 0) {
                Ok(Received::Message(m)) if m.body.words[0] == DONOR => {
                    verdict = Some(m.body.words);
                    break 'draw;
                }
                Ok(Received::Message(_)) => {}
                _ => break 'draw,
            }
        }
    }
    match verdict {
        Some([_, 1, 1, 0]) => {
            writeln!(
                out,
                "[grant-reuse] ok: PID {} reused after {} spawns; its grants refused",
                DONOR, spawned
            )
        }
        Some(w) => writeln!(
            out,
            "[grant-reuse] FAIL: PID {} reused after {} spawns: {:?} (BREACH)",
            DONOR, spawned, w
        ),
        None => writeln!(out, "[grant-reuse] FAIL: PID {} never came free ({} spawns)", DONOR, spawned),
    }
    .ok();
    writeln!(out, "GRANT-PID-REUSE TEST DONE").ok();
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
