// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering::Relaxed};

use redoubt_abi::arch::PAGE_SIZE;
use redoubt_abi::arch::USER_AREA_END;
use redoubt_abi::*;

use crate::arch;
use crate::arch::process::Process as ArchProcess;
use crate::cell::KernelCell;
use crate::irq::{interrupt_claim, interrupt_free};
use crate::mem::MemoryManager;
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

pub fn handle(pid: PID, tid: TID, in_irq: bool, call: SysCall) -> SysCallResult {
    klog!("KERNEL({}:{}): Syscall {:x?}, in_irq={}", pid, tid, call, in_irq);
    // let call_string = format!("{:x?}", call);
    // let start_time = std::time::Instant::now();
    #[allow(clippy::let_and_return)]
    let result = if in_irq && !call.can_call_from_interrupt() {
        klog!("[!] Called {:?} that's cannot be called from the interrupt handler!", call);
        Err(redoubt_abi::Error::InvalidSyscall)
    } else {
        handle_inner(pid, tid, call)
    };

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
        SysCall::MapMemory(phys, virt, size, req_flags) => {
            MemoryManager::with_mut(|mm| {
                let phys_ptr = phys.map(|x| x.get() as *mut u8).unwrap_or(core::ptr::null_mut());
                let virt_ptr = virt.map(|x| x.get() as *mut u8).unwrap_or(core::ptr::null_mut());

                // Don't let the address exceed the user area (unless it's PID 1)
                if pid.get() != 1
                    && virt.is_some_and(|x| {
                        x.get()
                            .checked_add(size.get())
                            .is_none_or(|end| end >= redoubt_abi::arch::USER_AREA_END)
                    })
                {
                    klog!("Exceeded user area");
                    return Err(redoubt_abi::Error::BadAddress);

                // Don't allow mapping non-page values
                } else if size.get() & (PAGE_SIZE - 1) != 0 {
                    // println!("map: bad alignment of size {:08x}", size);
                    return Err(redoubt_abi::Error::BadAlignment);
                }
                // println!(
                //     "Mapping {:08x} -> {:08x} ({} bytes, flags: {:?})",
                //     phys_ptr as u32, virt_ptr as u32, size, req_flags
                // );

                // An explicit physical address is either device MMIO or a bug/attack.
                //
                // A process must never name a physical RAM frame: it could point at another
                // process's freed page and read what was left there. Anonymous RAM comes from
                // `phys = 0` (which allocates a free frame and zeroes it). So reject any range
                // that touches main RAM, and default-deny device MMIO unless the boot manifest
                // granted it (tenet 2). PID 1 (the kernel) is trusted and maps its own memory.
                if !phys_ptr.is_null() {
                    let base = phys_ptr as usize;
                    if pid.get() != 1
                        && (base..base.saturating_add(size.get()))
                            .step_by(PAGE_SIZE)
                            .any(|page| mm.is_main_memory(page as *mut u8))
                    {
                        klog!("PID {} tried to map physical RAM {:08x} by address", pid.get(), base);
                        return Err(redoubt_abi::Error::InvalidArgument);
                    }
                    // WP-K5b (P2-1): no DMA device through the legacy path, whatever the grant,
                    // so a mapping of one always joins the reset set through `map_device`.
                    if pid.get() != 1 {
                        match crate::dma::overlaps_dma_device(base, size.get()) {
                            None => return Err(redoubt_abi::Error::InvalidArgument),
                            Some(true) => {
                                klog!("PID {} denied DMA device {:08x}", pid.get(), base);
                                return Err(redoubt_abi::Error::AccessDenied);
                            }
                            Some(false) => {}
                        }
                    }
                    if !crate::grants::may_map_device(mm, pid, base, size.get()) {
                        klog!("PID {} denied device {:08x}", pid.get(), base);
                        return Err(redoubt_abi::Error::AccessDenied);
                    }
                }

                let range =
                    mm.map_range(phys_ptr, virt_ptr, size.get(), pid, req_flags, MemoryType::Default)?;

                // The only explicit-address mappings that reach here are device MMIO (not
                // zeroed) and PID 1's own; RAM handed out through `phys = 0` is zeroed by its
                // own path (a demand-paged fault, or the DMA branch of `map_range`).
                if !phys_ptr.is_null() {
                    for offset in
                        (range.as_ptr() as usize..(range.as_ptr() as usize + range.len())).step_by(PAGE_SIZE)
                    {
                        crate::arch::mem::hand_page_to_user(offset as *mut u8)
                            .expect("couldn't hand page to user");
                    }
                }

                Ok(redoubt_abi::Result::MemoryRange(range))
            })
        }
        SysCall::UnmapMemory(range) => MemoryManager::with_mut(|mm| {
            let mut result = Ok(redoubt_abi::Result::Ok);
            let virt = range.as_ptr() as usize;
            let size = range.len();
            if virt & 0xfff != 0 {
                return Err(redoubt_abi::Error::BadAlignment);
            }
            if virt >= USER_AREA_END || virt.saturating_add(size) >= USER_AREA_END {
                // don't allow processes to unmap kernel or page table memory.
                return Err(redoubt_abi::Error::BadAddress);
            }
            for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
                if let Err(e) = mm.unmap_page(addr as *mut usize) {
                    if result.is_ok() {
                        result = Err(e);
                    }
                }
            }
            result
        }),
        SysCall::IncreaseHeap(delta, flags) => {
            if delta & 0xfff != 0 {
                return Err(redoubt_abi::Error::BadAlignment);
            }
            // Special case for a delta of 0 -- just return the current heap size
            if delta == 0 {
                let (start, length) = ArchProcess::with_inner_mut(|process_inner| {
                    (process_inner.mem_heap_base, process_inner.mem_heap_size)
                });
                return Ok(redoubt_abi::Result::MemoryRange(
                    // 0-length MemoryRanges are disallowed -- return 4096 as the minimum even though it's a
                    // lie.
                    crate::mem::memory_range(start, if length == 0 { 4096 } else { length }).unwrap(),
                ));
            }

            let start = {
                ArchProcess::with_inner_mut(|process_inner| {
                    let new_size = process_inner
                        .mem_heap_size
                        .checked_add(delta)
                        .ok_or(redoubt_abi::Error::OutOfMemory)?;
                    if new_size > process_inner.mem_heap_max {
                        return Err(redoubt_abi::Error::OutOfMemory);
                    }

                    let start = process_inner.mem_heap_base + process_inner.mem_heap_size;
                    process_inner.mem_heap_size = new_size;
                    Ok(start as *mut u8)
                })?
            };

            // Mark the new pages as "reserved". If that fails, the heap does not grow: a heap size
            // counting pages that were never reserved would make `DecreaseHeap` unmap nothing.
            MemoryManager::with_mut(|mm| mm.reserve_range(start, delta, flags))
                .inspect_err(|_| {
                    ArchProcess::with_inner_mut(|process_inner| process_inner.mem_heap_size -= delta)
                })
                .map(redoubt_abi::Result::MemoryRange)
        }
        SysCall::DecreaseHeap(delta) => {
            if delta & 0xfff != 0 {
                return Err(redoubt_abi::Error::BadAlignment);
            }
            let (start, size) = ArchProcess::with_inner(|process_inner| {
                (process_inner.mem_heap_base, process_inner.mem_heap_size)
            });
            // Don't allow decreasing the heap beyond the current allocation
            if delta >= size {
                return Err(redoubt_abi::Error::OutOfMemory);
            }
            let end = start + size;

            // Unmap the pages from the heap. A page the process has lent out is refused; the heap
            // then keeps its size, and the pages unmapped so far are simply gone from it.
            MemoryManager::with_mut(|mm| {
                for page in ((end - delta)..end).step_by(redoubt_abi::arch::PAGE_SIZE) {
                    mm.unmap_page(page as *mut usize)?;
                }
                Ok(())
            })?;
            let length = ArchProcess::with_inner_mut(|process_inner| {
                process_inner.mem_heap_size -= delta;
                process_inner.mem_heap_size
            });

            // Return the new size of the heap
            crate::mem::memory_range(start, length).map(redoubt_abi::Result::MemoryRange)
        }
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
        SysCall::ClaimInterrupt(no, callback, arg) => {
            interrupt_claim(no, pid as definitions::PID, callback, arg).map(|_| redoubt_abi::Result::Ok)
        }
        SysCall::FreeInterrupt(no) => {
            interrupt_free(no, pid as definitions::PID).map(|_| redoubt_abi::Result::Ok)
        }
        SysCall::Yield => do_yield(pid, tid),
        // Refused, to everyone (WP-K5): nothing in the tree calls it, and it put the kernel in the
        // state of a running interrupt callback with none running, which held every budget
        // deadline and slice end and refused every Redoubt call. A callback returns through
        // `RETURN_FROM_ISR` (`arch::irq`).
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
        SysCall::UpdateMemoryFlags(range, flags, pid) => {
            // We do not yet support modifying flags for other processes.
            if pid.is_some() {
                return Err(redoubt_abi::Error::ProcessNotChild);
            }

            MemoryManager::with_mut(|mm| mm.update_memory_flags(range, flags))?;
            Ok(redoubt_abi::Result::Ok)
        }
        SysCall::AdjustProcessLimit(index, current, new) => match index {
            1 => arch::process::Process::with_inner_mut(|p| {
                if p.mem_heap_max == current {
                    p.mem_heap_max = new;
                }
                Ok(redoubt_abi::Result::Scalar2(index, p.mem_heap_max))
            }),
            2 => arch::process::Process::with_inner_mut(|p| {
                if p.mem_heap_size == current && new < p.mem_heap_max {
                    p.mem_heap_size = new;
                }
                Ok(redoubt_abi::Result::Scalar2(index, p.mem_heap_size))
            }),
            _ => Err(redoubt_abi::Error::InvalidLimit),
        },

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
