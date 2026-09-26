// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering::Relaxed};

use redoubt_abi::*;

use crate::cell::KernelCell;
use crate::services::SystemServices;

/* Quoth Xobs:
 The idea behind SWITCHTO_CALLER was that you'd have a process act as a scheduler,
 where it would know all of its children processes. It would call SwitchTo(pid, tid)
 on its children, which would call Yield or WaitEvent as necessary that would then
 cause execution to return to the parent process.

 If the timer hit, it would call ReturnToParent() which would also return to the caller.

 Currently (as of Mar 2021) this functionality isn't being used, it's just returning
 back to the kernel, e.g. (PID,TID) = (1,1)

 (Redoubt, WP-K5: ReturnToParent is refused; nothing used it.)
*/
/// This is the PID/TID of the last person that called SwitchTo
static SWITCHTO_CALLER: KernelCell<Option<(PID, TID)>> = KernelCell::new(None);

/// When a process is switched to, take note of the original PID and TID.
/// That way we know whether to give the process its full quantum when
/// messages are Returned. If a process Returns messages while it's in its
/// own quantum, then don't immediately transfer control to the Client.
/// However, for processes that are running on Borrowed Quantum (i.e. when
/// another process sent them a message and they're immediately responding,)
/// return control to the Client.
static ORIGINAL_PID: AtomicU8 = AtomicU8::new(2);
static ORIGINAL_TID: AtomicUsize = AtomicUsize::new(2);

pub fn reset_switchto_caller() { SWITCHTO_CALLER.with(|c| *c = None); }

/// After a blocking Redoubt call switched away, point the scheduler back at the thread whose
/// quantum this is, exactly as `do_yield` does for the legacy calls.
pub fn restore_last_thread(ss: &mut SystemServices) {
    if let Some(pid) = PID::new(ORIGINAL_PID.load(Relaxed)) {
        ss.set_last_thread(pid, ORIGINAL_TID.load(Relaxed)).ok();
    }
}

fn do_yield(_pid: PID, tid: TID) -> SysCallResult {
    // The quantum's owner, normally `kmain` through `SwitchTo`. A preemption or a blocking call
    // may have taken the CPU back to `kmain` since (and forgotten the caller); every process's
    // parent is `kmain` then (WP-K5). A yield must never stop the kernel (I14).
    let (parent_pid, parent_ctx) =
        SWITCHTO_CALLER.with(|c| c.take()).unwrap_or((crate::services::KERNEL_PID, 0));
    //println!("\n\r ***YIELD CALLED***");
    SystemServices::with_mut(|ss| {
        // TODO: Advance thread
        let result = ss
            .activate_process_thread(tid, parent_pid, parent_ctx, true)
            .map(|_| Ok(redoubt_abi::Result::ResumeProcess))
            .unwrap_or(Err(redoubt_abi::Error::ProcessNotFound));

        ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed)).ok();
        result
    })
}

pub fn handle(pid: PID, tid: TID, call: SysCall) -> SysCallResult {
    klog!("KERNEL({}:{}): Syscall {:x?}", pid, tid, call);
    // let call_string = format!("{:x?}", call);
    // let start_time = std::time::Instant::now();
    #[allow(clippy::let_and_return)]
    let result = handle_inner(pid, tid, call);

    // println!("KERNEL [{:2}:{:2}] Syscall took {:7} usec: {}", pid, tid, start_time.elapsed().as_micros(),
    // call_string);

    klog!(
        " -> ({}:{}) {:x?}",
        crate::arch::current_pid(),
        crate::arch::process::Process::current().current_tid(),
        result
    );
    result
}

pub fn handle_inner(pid: PID, tid: TID, call: SysCall) -> SysCallResult {
    match call {
        SysCall::SwitchTo(new_pid, new_tid) => SystemServices::with_mut(|ss| {
            SWITCHTO_CALLER.with(|caller| {
                assert!(
                    caller.is_none(),
                    "SWITCHTO_CALLER was {:?} and not None, indicating SwitchTo was called twice",
                    caller,
                );
                *caller = Some((pid, tid));
            });
            // println!(
            //     "Activating process thread {} in pid {} coming from pid {} thread {}",
            //     new_context, new_pid, pid, tid
            // );
            let new_tid = match ss.activate_process_thread(tid, new_pid, new_tid, true)
            {
                Ok(t) => t,
                Err(e) => {
                    // Nothing was switched: the caller picks again (`main.rs`).
                    SWITCHTO_CALLER.with(|c| *c = None);
                    return Err(e);
                }
            };
            ORIGINAL_PID.store(new_pid.get(), Relaxed);
            ORIGINAL_TID.store(new_tid, Relaxed);
            Ok(redoubt_abi::Result::ResumeProcess)
        }),
        SysCall::Yield => do_yield(pid, tid),
        // Refused, to everyone (WP-K5): nothing in the tree calls it.
        SysCall::ReturnToParent(_pid, _cpuid) => Err(redoubt_abi::Error::UnhandledSyscall),
        SysCall::WaitEvent => SystemServices::with_mut(|ss| {
            let process = ss.get_process(pid).expect("Can't get current process");
            let ppid = process.ppid;
            SWITCHTO_CALLER.with(|c| *c = None);
            // TODO: Advance thread
            let result = ss
                .activate_process_thread(tid, ppid, 0, false)
                .map(|_| Ok(redoubt_abi::Result::ResumeProcess))
                .unwrap_or(Err(redoubt_abi::Error::ProcessNotFound));
            ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed))
                .ok();
            result
        }),
        SysCall::CreateThread(thread_init) => SystemServices::with_mut(|ss| {
            ss.create_thread(pid, thread_init).map(|new_tid| {
                // Set the return value of the existing thread to be the new thread ID
                // Immediately switch to the new thread
                ss.switch_to_thread(pid, Some(new_tid)).expect("couldn't activate new thread");
                ss.set_thread_result(pid, tid, redoubt_abi::Result::ThreadID(new_tid))
                    .expect("couldn't set new thread ID");

                // Return `ResumeProcess` since we're switching threads
                redoubt_abi::Result::ResumeProcess
            })
        }),
        // The legacy `CreateProcess` is gone on bare metal: it made processes outside every
        // budget (R6), no program uses it, and WP-K4 brings `process_create`. It falls through to
        // `UnhandledSyscall` below.
        // The legacy exit, which every `no_std` program still uses. It is `process_exit` with
        // another number: a process the Redoubt calls created gets its exit notice either way
        // (KERNEL-SPEC.md, `process_exit`), so a program does not have to be rewritten before its
        // parent can be told how it ended. (INTERIM until WP-K6 deletes the legacy interface.)
        SysCall::TerminateProcess(ret) => {
            SystemServices::with_mut(|ss| crate::process::process_exit(ss, pid, tid, ret.into()));
            Ok(redoubt_abi::Result::ResumeProcess)
        }
        SysCall::Shutdown => SystemServices::with_mut(|ss| ss.shutdown().map(|_| redoubt_abi::Result::Ok)),
        SysCall::GetProcessId => Ok(redoubt_abi::Result::ProcessID(pid)),
        SysCall::GetThreadId => Ok(redoubt_abi::Result::ThreadID(tid)),

        SysCall::JoinThread(other_tid) => {
            if other_tid >= crate::arch::process::MAX_THREAD {
                return Err(redoubt_abi::Error::ThreadNotAvailable);
            }
            SystemServices::with_mut(|ss| ss.join_thread(pid, tid, other_tid)).map(|ret| {
                // Successfully joining a thread causes this thread to sleep while the parent process
                // is resumed. This is the same as a `Yield`
                if ret == redoubt_abi::Result::ResumeProcess {
                    SWITCHTO_CALLER.with(|c| *c = None);
                }
                ret
            })
        }
        #[cfg(feature = "sbi")]
        SysCall::PlatformSpecific(op, a2, a3, _a4, _a5, _a6, _a7) => {
            crate::platform::sbi::platform_call(pid, op, a2, a3)
        }

        #[cfg(not(feature = "sbi"))]
        SysCall::PlatformSpecific(_a1, _a2, _a3, _a4, _a5, _a6, _a7) => {
            unimplemented!("No platform specific calls for this platform")
        }

        /* https://github.com/betrusted-io/xous-core-core/issues/90
        SysCall::SetExceptionHandler(pc, sp) => SystemServices::with_mut(|ss| {
            ss.set_exception_handler(pid, pc, sp)
                .and(Ok(redoubt_abi::Result::Ok))
        }),
        */
        _ => Err(redoubt_abi::Error::UnhandledSyscall),
    }
}
