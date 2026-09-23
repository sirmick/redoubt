// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Redoubt system calls (KERNEL-SPEC.md), decoded by `redoubt-sys`.
//!
//! Until WP-K6 deletes it, the legacy Redoubt interface is served beside this one: the trap handler
//! sends every `ecall` whose `a0` is at least `redoubt_sys::NUMBER_BASE` here, and the rest to
//! `syscall.rs`. The two share nothing but the kernel's objects.
//!
//! Every call runs the spec's stages in order (KERNEL-SPEC.md, Errors and the order of checks):
//! `Call::decode` checks the registers; the records a call passes are then checked and copied
//! (alignment, then lying in the caller's own memory, then their slots); then the call's own
//! checks, which live with the objects (`budget.rs`, `handle.rs`).
//!
//! Implemented calls (WP-K1 through WP-K4): `handle_close`, `budget_create`, `budget_destroy`,
//! `budget_usage`, `time_now`, `random`, `endpoint_create`, `mint`, `call`, `send`, `receive`,
//! `reply`, `serve`, `map_anon`, `unmap`, `set_flags`, `map_device`, `dma_alloc`,
//! `system_reset` and the `process_*` and `thread_*` families.

use redoubt_abi::{PID, TID};
use redoubt_sys::{
    BUDGET_SPEC_SLOTS, BudgetSpec, Call, CallOutcome, Error, LendDisposition, Number, REGS, Return,
    USAGE_SLOTS, encode_result,
};

use crate::kframe;
use crate::mem::MemoryManager;
use crate::message::MsgKind;
use crate::services::SystemServices;

/// The scheduler and the memory manager together, in the one order the kernel borrows them.
fn with_both<T>(f: impl FnOnce(&mut SystemServices, &mut MemoryManager) -> T) -> T {
    SystemServices::with_mut(|ss| MemoryManager::with_mut(|mm| f(ss, mm)))
}

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
        SystemServices::with_mut(|ss| MemoryManager::with_mut(|mm| crate::message::expire(ss, mm)));
    }

    let result = if in_irq {
        Err(Error::NotPermitted)
    } else {
        Call::decode(regs).and_then(|c| dispatch(pid, tid, c))
    };
    // Every error a call returns is in its row of the spec's error table (`Number::can_return`).
    // The interim refusal of legacy callbacks is outside the table, and an unknown number has no
    // row (it is `InvalidArgument`).
    if let (Err(error), false, Some(number)) = (&result, in_irq, Number::from_raw(regs[0])) {
        debug_assert!(number.can_return(*error), "{} returned {:?}, outside its row", number.name(), error);
    }
    match result {
        Ok(None) => Outcome::Resume,
        Ok(Some(value)) => Outcome::Return(encode_result(&Ok(value))),
        Err(error) => {
            // Recognized calls retain their raw lend even if an earlier argument did not
            // decode. No memory has been consumed on this path (answers 167-168).
            let result = if Number::from_raw(regs[0]) == Some(Number::Call) {
                Ok(Return::Call(CallOutcome {
                    status: Err(error),
                    lend: if regs[3] == 0 && regs[4] == 0 {
                        LendDisposition::None
                    } else {
                        LendDisposition::Returned
                    },
                    reply_present: false,
                }))
            } else {
                Err(error)
            };
            Outcome::Return(encode_result(&result))
        }
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
            let spec = BudgetSpec::decode(&read_record::<BUDGET_SPEC_SLOTS>(mm, spec_rec, false)?)?;
            let handle = mm.budget_create(pid, parent.index(), &spec)?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::BudgetDestroy { budget } => budget_destroy(pid, tid, budget.index()),
        Call::BudgetUsage { budget, usage_rec } => MemoryManager::with_mut(|mm| {
            let frames = record_frames::<USAGE_SLOTS>(mm, usage_rec, true)?;
            let usage = mm.budget_usage(pid, budget.index())?;
            write_record_to(usage_rec, &frames, &usage.encode());
            Ok(Some(Return::Nothing))
        }),
        Call::EndpointCreate => MemoryManager::with_mut(|mm| {
            let handle = mm.endpoint_create(pid)?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::Mint { source, badge, budget } => MemoryManager::with_mut(|mm| {
            let handle = crate::message::mint(mm, pid, tid, source, badge.get(), budget.map(|h| h.index()))?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::Call { endpoint, body_rec, lend, timeout } => with_both(|ss, mm| {
            crate::message::send(ss, mm, pid, tid, MsgKind::Call, endpoint.index(), body_rec, lend, timeout)
        }),
        Call::Send { endpoint, body_rec, transfer, timeout } => with_both(|ss, mm| {
            crate::message::send(
                ss,
                mm,
                pid,
                tid,
                MsgKind::Send,
                endpoint.index(),
                body_rec,
                transfer,
                timeout,
            )
        }),
        Call::Receive { from, timeout, max_transfer, received_rec } => with_both(|ss, mm| {
            let from = from.map(|h| h.index());
            crate::message::receive(ss, mm, pid, tid, from, timeout, max_transfer, received_rec)
        }),
        Call::Reply { msg_id, body_rec } => with_both(|ss, mm| {
            crate::message::reply(ss, mm, pid, tid, msg_id.get(), body_rec)
                .map(|outcome| Some(Return::Reply(outcome)))
        }),
        Call::Serve { msg_id } => {
            MemoryManager::with_mut(|mm| crate::message::serve(mm, pid, tid, msg_id.get())).map(done)
        }
        Call::MapAnon { len, flags } => {
            MemoryManager::with_mut(|mm| mm.map_anon(pid, len, flags)).map(|at| Some(Return::Addr(at)))
        }
        Call::Unmap { addr, len } => MemoryManager::with_mut(|mm| mm.unmap(pid, addr, len)).map(done),
        Call::SetFlags { addr, len, flags } => {
            MemoryManager::with_mut(|mm| mm.set_flags(pid, addr, len, flags)).map(done)
        }
        Call::MapDevice { device } => MemoryManager::with_mut(|mm| {
            // QUESTIONS.md 146 (pending): the length comes back with the address.
            let (addr, len) = mm.map_device(pid, device.index())?;
            Ok(Some(Return::Mapping { addr, len }))
        }),
        Call::DmaAlloc { device, npages } => MemoryManager::with_mut(|mm| {
            let (addr, phys) = mm.dma_alloc(pid, device.index(), npages)?;
            Ok(Some(Return::Dma { addr, phys }))
        }),
        // On success this does not return: the machine powers off or reboots. The memory
        // manager is let go of first -- the firmware call never comes back, and a kernel cell
        // held for ever is, with `smp`, a spinlock held for ever.
        Call::SystemReset { device, kind } => {
            MemoryManager::with(|mm| mm.check_reset(pid, device.index()))?;
            println!("system_reset: {:?} asked for by PID {}", kind, pid.get());
            crate::platform::reset(kind == redoubt_sys::ResetKind::Reboot)
        }
        Call::TimeNow => Ok(Some(Return::Time(crate::arch::irq::timer::now_us()))),
        Call::Random => {
            let mut bytes = [0u8; 8];
            crate::platform::rand::fill(&mut bytes);
            Ok(Some(Return::Random(u64::from_le_bytes(bytes))))
        }
        Call::ProcessCreate { budget, exit_endpoint } => with_both(|ss, mm| {
            let handle = crate::process::process_create(ss, mm, pid, budget.index(), exit_endpoint.index())?;
            Ok(Some(Return::Handle(redoubt_sys::Handle::new(handle).expect("indices start at 1"))))
        }),
        Call::ProcessMap { process, src, dst, len, flags } => with_both(|ss, mm| {
            crate::process::process_map(ss, mm, pid, process.index(), src, dst, len, flags).map(done)
        }),
        Call::ProcessStart { process, entry, sp, arg, handles_rec, count } => with_both(|ss, mm| {
            let h = process.index();
            crate::process::process_start(ss, mm, pid, h, entry, sp, arg, handles_rec, count).map(done)
        }),
        // These three end a thread or a process, so they take the scheduler alone: tearing a
        // process down borrows the memory manager itself (`process.rs`, Locks).
        Call::ThreadCreate { entry, sp, arg } => {
            SystemServices::with_mut(|ss| crate::process::thread_create(ss, pid, entry, sp, arg))
                .map(|tid| Some(Return::Tid(tid)))
        }
        Call::ThreadExit => {
            SystemServices::with_mut(|ss| crate::process::thread_exit(ss, pid, tid));
            Ok(None)
        }
        Call::ProcessExit { code } => {
            SystemServices::with_mut(|ss| crate::process::process_exit(ss, pid, tid, code));
            Ok(None)
        }
    }
}

/// `budget_destroy(h)` (R10): mark the subtree, kill every process in it (the caller last, if it
/// is one of them), then sweep the handles and free the budgets.
fn budget_destroy(pid: PID, _tid: TID, h: u32) -> Result<Option<Return>, Error> {
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
                // Each gets an exit notice with cause `killed`, unless its process object is
                // charged to a budget in the same doomed subtree (`process.rs`).
                crate::process::killed(ss, victim);
            }
        }
        if caller_doomed {
            crate::process::killed(ss, pid);
        }
        // R10 reaches the process objects charged to the subtree: each is freed, with no notice,
        // its process killed first if it still runs.
        crate::process::budgets_dying(ss);
        // The caller may run outside this subtree but have its process object charged to it.
        // R10 killed it through its creator above; never return registers to that dead PID.
        caller_doomed |= MemoryManager::with(|mm| mm.budget_of(pid).is_none());
        // R10 reaches messages in flight: the endpoints the subtree owns are destroyed, and
        // every message sent through a handle stamped with it fails its sender with `Dead`.
        MemoryManager::with_mut(|mm| {
            crate::message::budgets_dying(ss, mm);
            mm.destroy_marked(top);
        });
        Ok(if caller_doomed { None } else { Some(Return::Nothing) })
    })
}

/// The frames behind a record: backed, aligned, permitted, and owned RAM. The caller
/// holds the memory-manager guard through validation and copying, excluding unmap/remap,
/// permission changes and teardown. Device mappings and borrowed pages are not records.
fn record_frames<const N: usize>(mm: &MemoryManager, addr: usize, write: bool) -> Result<[usize; N], Error> {
    if addr % 8 != 0 {
        return Err(Error::InvalidArgument);
    }
    let pid = crate::arch::process::current_pid();
    let mut frames = [0; N];
    for (i, frame) in frames.iter_mut().enumerate() {
        let slot = addr.checked_add(i * 8).ok_or(Error::InvalidArgument)?;
        *frame = crate::arch::mem::user_frame(slot, write)?;
        if !mm.is_main_memory(*frame as *mut u8) {
            return Err(Error::InvalidArgument);
        }
        mm.check_owned_range(pid, slot, 8).map_err(|_| Error::InvalidArgument)?;
    }
    Ok(frames)
}

fn write_record_to<const N: usize>(addr: usize, frames: &[usize; N], slots: &[u64; N]) {
    for i in 0..N {
        kframe::write(frames[i], (addr + i * 8) % redoubt_abi::arch::PAGE_SIZE, slots[i]);
    }
}

/// Copy an input record after validating every slot. Calls also need writable output.
pub fn read_record<const N: usize>(mm: &MemoryManager, addr: usize, output: bool) -> Result<[u64; N], Error> {
    let frames = record_frames::<N>(mm, addr, false)?;
    if output {
        record_frames::<N>(mm, addr, true)?;
    }
    Ok(core::array::from_fn(|i| kframe::read(frames[i], (addr + i * 8) % redoubt_abi::arch::PAGE_SIZE)))
}

/// Read a handle-list record under the ownership guard used by fixed records.
pub fn read_slots<const N: usize>(mm: &MemoryManager, addr: usize, n: usize) -> Result<[u64; N], Error> {
    if n > N {
        return Err(Error::TooLarge);
    }
    let mut slots = [0u64; N];
    let mut frames = [0usize; N];
    for (i, frame) in frames.iter_mut().enumerate().take(n) {
        let at = addr.checked_add(i * 8).ok_or(Error::InvalidArgument)?;
        *frame = record_frames::<1>(mm, at, false)?[0];
    }
    for i in 0..n {
        slots[i] = kframe::read(frames[i], (addr + i * 8) % redoubt_abi::arch::PAGE_SIZE);
    }
    Ok(slots)
}

pub fn check_record<const N: usize>(mm: &MemoryManager, addr: usize) -> Result<(), Error> {
    record_frames::<N>(mm, addr, true).map(|_| ())
}

pub fn write_record<const N: usize>(mm: &MemoryManager, addr: usize, slots: &[u64; N]) -> Result<(), Error> {
    let frames = record_frames::<N>(mm, addr, true)?;
    write_record_to(addr, &frames, slots);
    Ok(())
}
