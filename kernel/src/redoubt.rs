// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Redoubt system calls (KERNEL-SPEC.md), decoded by `redoubt-sys`.
//!
//! Until WP-K6 deletes it, the legacy Xous interface is served beside this one: the trap handler
//! sends every `ecall` whose `a0` is at least `redoubt_sys::NUMBER_BASE` here, and the rest to
//! `syscall.rs`. The two share nothing but the kernel's objects.
//!
//! Every call runs the spec's stages in order (KERNEL-SPEC.md, Errors and the order of checks):
//! `Call::decode` checks the registers; the records a call passes are then checked and copied
//! (alignment, then lying in the caller's own memory, then their slots); then the call's own
//! checks, which live with the objects (`budget.rs`, `handle.rs`).
//!
//! Built so far (WP-K1, WP-K2): `handle_close`, `budget_create`, `budget_destroy`,
//! `budget_usage`, `time_now`, `random`, `endpoint_create`, `mint`, `call`, `send`, `receive`,
//! `reply`, `serve`. Every other call decodes, then gets `InvalidArgument` until its package
//! builds it (WP-K3 to WP-K5).

use redoubt_sys::{
    BUDGET_SPEC_SLOTS, BudgetSpec, Call, Error, Number, REGS, Return, USAGE_SLOTS,
    encode_result,
};
use xous_kernel::{PID, TID};

use crate::kframe;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// What the trap handler does after a call.
pub enum Outcome {
    /// Return to the caller with these registers.
    Return([u64; REGS]),
    /// The caller no longer exists (it destroyed its own budget); run whatever is current.
    Resume,
}

pub fn handle(pid: PID, tid: TID, in_irq: bool, regs: &[u64; REGS]) -> Outcome {
    // A legacy interrupt callback runs on borrowed time inside another process's quantum; it
    // gets none of these calls. (INTERIM: WP-K3 replaces callbacks with IRQ handles.)
    // I13, until WP-K5 arms the timer: every deadline that has passed is answered before this
    // call is, so a blocking call returns by its timeout as soon as anything enters the kernel.
    if !in_irq {
        SystemServices::with_mut(crate::message::expire);
    }

    let result =
        if in_irq { Err(Error::NotPermitted) } else { Call::decode(regs).and_then(|c| dispatch(pid, tid, c)) };
    // Every error a call returns is in its row of the spec's error table (`Number::can_return`).
    // The interim refusal of legacy callbacks is outside the table, and an unknown number has no
    // row (it is `InvalidArgument`).
    if let (Err(error), false, Some(number)) = (&result, in_irq, Number::from_raw(regs[0])) {
        debug_assert!(number.can_return(*error), "{} returned {:?}, outside its row", number.name(), error);
    }
    match result {
        Ok(None) => Outcome::Resume,
        Ok(Some(value)) => Outcome::Return(encode_result(&Ok(value))),
        Err(error) => Outcome::Return(encode_result(&Err(error))),
    }
}

/// `Ok(None)`: the caller is gone.
fn dispatch(pid: PID, tid: TID, call: Call) -> Result<Option<Return>, Error> {
    let done = |_| Some(Return::Nothing);
    match call {
        Call::HandleClose { handle } => {
            MemoryManager::with_mut(|mm| mm.handle_close(pid, handle.index())).map(done)
        }
        Call::BudgetCreate { parent, spec_rec } => MemoryManager::with_mut(|mm| {
            let spec = BudgetSpec::decode(&read_record::<BUDGET_SPEC_SLOTS>(spec_rec)?)?;
            let handle = mm.budget_create(pid, parent.index(), &spec)?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::BudgetDestroy { budget } => budget_destroy(pid, tid, budget.index()),
        Call::BudgetUsage { budget, usage_rec } => MemoryManager::with_mut(|mm| {
            let frames = record_frames::<USAGE_SLOTS>(usage_rec, true)?;
            let usage = mm.budget_usage(pid, budget.index())?;
            write_record_to(usage_rec, &frames, &usage.encode());
            Ok(Some(Return::Nothing))
        }),
        Call::EndpointCreate => MemoryManager::with_mut(|mm| {
            let handle = mm.endpoint_create(pid)?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::Mint { source, badge, budget } => {
            let handle = crate::message::mint(pid, tid, source, badge.get(), budget.map(|h| h.index()))?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }
        Call::Call { endpoint, body_rec, lend, timeout } => SystemServices::with_mut(|ss| {
            crate::message::call(ss, pid, tid, endpoint.index(), body_rec, lend, timeout)
        }),
        Call::Send { endpoint, body_rec, transfer, timeout } => SystemServices::with_mut(|ss| {
            crate::message::send(ss, pid, tid, endpoint.index(), body_rec, transfer, timeout)
        }),
        Call::Receive { from, timeout, max_transfer, received_rec } => SystemServices::with_mut(|ss| {
            let from = from.map(|h| h.index());
            crate::message::receive(ss, pid, tid, from, timeout, max_transfer, received_rec)
        }),
        Call::Reply { msg_id, body_rec } => SystemServices::with_mut(|ss| {
            crate::message::reply(ss, pid, tid, msg_id.get(), body_rec).map(done)
        }),
        Call::Serve { msg_id } => crate::message::serve(pid, tid, msg_id.get()).map(done),
        Call::TimeNow => Ok(Some(Return::Time(crate::arch::irq::timer::now_us()))),
        Call::Random => {
            let mut bytes = [0u8; 8];
            crate::platform::rand::fill(&mut bytes);
            Ok(Some(Return::Random(u64::from_le_bytes(bytes))))
        }
        // Decoded, not built yet (WP-K3 to WP-K5).
        _ => Err(Error::InvalidArgument),
    }
}

/// `budget_destroy(h)` (R10): mark the subtree, kill every process in it (the caller last, if it
/// is one of them), then sweep the handles and free the budgets.
fn budget_destroy(pid: PID, tid: TID, h: u32) -> Result<Option<Return>, Error> {
    SystemServices::with_mut(|ss| {
        let top = MemoryManager::with_mut(|mm| mm.destroy_begin(pid, h))?;
        let mut caller_doomed = false;
        for index in 1..=crate::arch::process::MAX_PROCESS_COUNT {
            let Some(victim) = PID::new(index as u8) else { continue };
            if !MemoryManager::with(|mm| mm.process_is_doomed(victim)) {
                continue;
            }
            if victim == pid {
                caller_doomed = true;
            } else {
                ss.kill_process(victim).expect("a process with an account exists");
            }
        }
        if caller_doomed {
            // As the legacy `TerminateProcess` does: the caller's thread stops, and the kernel
            // picks what runs next.
            ss.unschedule_thread(pid, tid).expect("the caller is running");
            ss.terminate_process(pid).expect("the caller exists");
            crate::syscall::reset_switchto_caller();
        }
        // R10 reaches messages in flight: the endpoints the subtree owns are destroyed, and
        // every message sent through a handle stamped with it fails its sender with `Dead`.
        crate::message::budgets_dying(ss);
        MemoryManager::with_mut(|mm| mm.destroy_marked(top));
        Ok(if caller_doomed { None } else { Some(Return::Nothing) })
    })
}

/// The frames behind each slot of an `N`-slot record at `addr`: aligned, and all the caller's
/// own memory, readable (and, with `write`, writable). Checked in full before any slot is used.
fn record_frames<const N: usize>(addr: usize, write: bool) -> Result<[usize; N], Error> {
    if addr % 8 != 0 {
        return Err(Error::InvalidArgument);
    }
    let mut frames = [0; N];
    for (i, frame) in frames.iter_mut().enumerate() {
        let slot = addr.checked_add(i * 8).ok_or(Error::InvalidArgument)?;
        *frame = crate::arch::mem::user_frame(slot, write)?;
    }
    Ok(frames)
}

/// Copy in an `N`-slot input record.
pub fn read_record<const N: usize>(addr: usize) -> Result<[u64; N], Error> {
    let frames = record_frames::<N>(addr, false)?;
    Ok(core::array::from_fn(|i| kframe::read(frames[i], (addr + i * 8) % xous_kernel::arch::PAGE_SIZE)))
}

fn write_record_to<const N: usize>(addr: usize, frames: &[usize; N], slots: &[u64; N]) {
    for i in 0..N {
        kframe::write(frames[i], (addr + i * 8) % xous_kernel::arch::PAGE_SIZE, slots[i]);
    }
}

/// Copy out an `N`-slot output record: `receive`'s and the reply `call` writes back. The
/// receiving process's address space must be the active one.
pub fn write_record<const N: usize>(addr: usize, slots: &[u64; N]) -> Result<(), Error> {
    let frames = record_frames::<N>(addr, true)?;
    write_record_to(addr, &frames, slots);
    Ok(())
}

/// Check an `N`-slot output record without writing it, so that `receive` refuses a record it
/// could not fill before it blocks on one.
pub fn check_record<const N: usize>(addr: usize) -> Result<(), Error> {
    record_frames::<N>(addr, true).map(|_| ())
}
