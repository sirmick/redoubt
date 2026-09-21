//! Drives the hart timer the way a ticktimer server would: claim its interrupt, read
//! the timebase, and re-arm a one-shot deadline from the interrupt handler.
//! See `docs/TIMER.md`.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::{log, Logger};
use redoubt_abi::arch::platform_call::{TIMER_IRQ, TIMER_SET_DEADLINE, TIMER_TIMEBASE};
use redoubt_abi::SysCall;

const TICKS_WANTED: usize = 5;
const TICK_HZ: u64 = 20;

static TICKS: AtomicUsize = AtomicUsize::new(0);
static TICK_INTERVAL: AtomicUsize = AtomicUsize::new(0);

/// The kernel lets userspace read the `time` CSR directly.
fn now() -> u64 { test_programs::read_time() }

fn set_deadline(deadline: u64) {
    redoubt_abi::rsyscall(SysCall::PlatformSpecific(TIMER_SET_DEADLINE, deadline as usize, 0, 0, 0, 0, 0))
        .expect("couldn't set the timer deadline");
}

/// Runs in interrupt context. The timer is one-shot, so arm the next tick.
fn on_tick(_irq: usize, _arg: *mut usize) {
    if TICKS.fetch_add(1, Ordering::Relaxed) + 1 < TICKS_WANTED {
        set_deadline(now() + TICK_INTERVAL.load(Ordering::Relaxed) as u64);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();

    // Before the interrupt is claimed, the timer is not ours to program.
    let denied = redoubt_abi::rsyscall(SysCall::PlatformSpecific(TIMER_SET_DEADLINE, 0, 0, 0, 0, 0, 0));
    let access_ok = denied == Err(redoubt_abi::Error::AccessDenied);
    log!(logger, "[timer] {}: set-deadline without owning the irq -> {:?}", if access_ok { "ok" } else { "FAIL" }, denied);

    let timebase = match redoubt_abi::rsyscall(SysCall::PlatformSpecific(TIMER_TIMEBASE, 0, 0, 0, 0, 0, 0)) {
        Ok(redoubt_abi::Result::Scalar1(hz)) => hz as u64,
        other => {
            log!(logger, "[timer] FAIL: unexpected timebase reply {:?}", other);
            0
        }
    };
    log!(logger, "[timer] timebase {} Hz, time {}", timebase, now());
    TICK_INTERVAL.store((timebase / TICK_HZ) as usize, Ordering::Relaxed);

    redoubt_abi::claim_interrupt(TIMER_IRQ, on_tick, core::ptr::null_mut()).expect("couldn't claim the timer interrupt");
    let start = now();
    set_deadline(start + timebase / TICK_HZ);

    let mut reported = 0;
    while reported < TICKS_WANTED {
        let ticks = TICKS.load(Ordering::Relaxed);
        if ticks > reported {
            reported = ticks;
            log!(logger, "[timer] tick {}", ticks);
        }
        redoubt_abi::yield_slice();
    }

    // Five ticks at 20 Hz should take a quarter of a second, give or take emulation.
    let elapsed_ms = (now() - start) * 1000 / timebase.max(1);
    let timing_ok = (200..2000).contains(&elapsed_ms);
    log!(logger, "[timer] {}: {} ticks in {} ms", if timing_ok { "ok" } else { "FAIL" }, reported, elapsed_ms);

    if access_ok && timing_ok && timebase > 0 {
        log!(logger, "TIMER TEST PASSED");
    } else {
        log!(logger, "TIMER TEST FAILED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
