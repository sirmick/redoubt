// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use redoubt_layout::{KERNEL_PID, Pid};

use crate::arch;
use crate::arch::mem::MemoryMapping;
pub use crate::arch::process::Process as ArchProcess;
pub use crate::arch::process::Thread;
pub use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT};
use crate::arch::process::{TID, TidMask};
use crate::cell::KernelCell;

/// Why the process table refused a step. Kernel-internal: a system call maps it explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError {
    /// No such process, or its slot is not in the state the step needs.
    NotFound,
    /// The thread is not one that can run now.
    NotReady,
    /// The address space could not be made (`MemoryMapping::allocate`).
    Page(crate::mem::PageError),
}

/// A big unifying struct containing all of the system state.
/// This is inherited from the stage 1 bootloader.
pub struct ProcessTable {
    /// A table of all processes in the system
    pub processes: [Process; MAX_PROCESS_COUNT],
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProcessState {
    /// This is an unallocated, free process
    Free,

    /// This process has been allocated, but has no threads yet
    Allocated,

    /// `init`, the one process the loader starts, before it has run: its first thread starts at
    /// `entry` with stack pointer `sp` and the bundle's address and length in `a0` and `a1` when
    /// it is first switched to (kernel/boot.md).
    Setup { entry: usize, sp: usize, a0: usize, a1: usize },

    /// This process is able to be run.  The context bitmask describes contexts
    /// that are ready.
    Ready(TidMask),

    /// One hart or more runs one of its threads. The bitmask is the threads that are ready and
    /// that no hart runs; each hart's thread is in its block (`arch::hart`).
    Running(TidMask),

    /// This process is waiting for an event, such as as message or an
    /// interrupt.  There are no contexts that can be run. This is
    /// functionally equivalent to the invalid `Ready` with no thread.
    Sleeping,
}

impl core::fmt::Debug for ProcessState {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        use ProcessState::*;
        match *self {
            Free => write!(fmt, "Free"),
            Allocated => write!(fmt, "Allocated"),
            Setup { entry, sp, a0, a1 } => {
                write!(fmt, "Setup {{ entry: {:#x}, sp: {:#x}, a0: {:#x}, a1: {:#x} }}", entry, sp, a0, a1)
            }
            Ready(rt) => write!(fmt, "Ready({:?})", rt),
            Running(rt) => write!(fmt, "Running({:?})", rt),
            Sleeping => write!(fmt, "Sleeping"),
        }
    }
}

impl Default for ProcessState {
    fn default() -> ProcessState { ProcessState::Free }
}

#[derive(Copy, Clone, PartialEq)]
pub struct Process {
    /// The absolute MMU address.  If 0, then this process is free.  This needs
    /// to be available so we can switch to this process at any time, so it
    /// cannot go into the "inner" struct.
    pub mapping: MemoryMapping,

    /// Where this process is in terms of lifecycle
    state: ProcessState,

    /// This process' PID. This should match up with the index in the process table. `None` in
    /// a slot never used, so that the table's starting value is all zeros and it is `.bss`.
    pid: Option<Pid>,

    /// The thread last switched to, where the turn among its ready threads goes on from. The
    /// thread each hart runs is in that hart's block.
    pub current_thread: TID,
}

/// This is per-process data.  The arch-specific definitions will instantiate
/// this struct in order to avoid the need to statically-allocate this for
/// all possible processes.
/// Note that this data is only available when the current process is active.
#[repr(C)]
#[derive(Debug, PartialEq, Copy, Clone)]
/// Default virtual address when MapMemory is called with no `virt`
pub struct ProcessInner {
    pub mem_default_base: usize,

    /// The last address allocated from
    pub mem_default_last: usize,

    /// Address where messages are passed into
    pub mem_message_base: usize,

    /// The last address that was allocated from
    pub mem_message_last: usize,
}

impl Default for ProcessInner {
    fn default() -> Self {
        ProcessInner {
            mem_default_base: crate::mem::DEFAULT_BASE,
            mem_default_last: crate::mem::DEFAULT_BASE,
            mem_message_base: crate::mem::DEFAULT_MESSAGE_BASE,
            mem_message_last: crate::mem::DEFAULT_MESSAGE_BASE,
        }
    }
}

impl Process {
    /// This process slot is unallocated and may be turn into a process
    pub fn free(&self) -> bool { matches!(self.state, ProcessState::Free) }

    /// The threads the scheduler may run next (`sched.rs`): a bit per thread waiting for the
    /// CPU, or `None` for a process whose next thread `activate_process_thread` chooses itself
    /// (being set up). A running thread is not waiting.
    pub fn ready_threads(&self) -> Option<TidMask> {
        match self.state {
            ProcessState::Ready(x) | ProcessState::Running(x) => Some(x),
            ProcessState::Setup { .. } => None,
            _ => Some(TidMask::EMPTY),
        }
    }

    /// How many threads wait for the CPU: those of [`Process::ready_threads`], and one for a
    /// process being set up. The scheduler counts them for its budget (`sched.rs`).
    pub fn ready_count(&self) -> u32 { self.ready_threads().map_or(1, |x| x.iter().count() as u32) }

    /// Every change of state goes through here: one that changes how many threads wait for the
    /// CPU (a thread becomes ready or stops being, the process starts or ends) marks the process
    /// for the scheduler's next reconcile.
    fn set_state(&mut self, state: ProcessState) {
        let was = self.ready_count();
        self.state = state;
        if self.ready_count() != was {
            crate::sched::mark(self.pid());
        }
    }

    /// Whether the process is on the CPU: a hart runs one of its threads (the checked build's
    /// audit of the harts, `sched.rs`).
    #[cfg(debug_assertions)]
    pub fn running(&self) -> bool { matches!(self.state, ProcessState::Running(_)) }

    /// This process's PID; a slot is given one before it is first used.
    pub fn pid(&self) -> Pid { self.pid.expect("a process slot in use has a PID") }

    pub fn activate(&self) {
        crate::arch::process::set_current_pid(self.pid());
        self.mapping.activate();
    }

    pub fn terminate(&mut self) -> Result<(), ProcessError> {
        if self.free() {
            return Err(ProcessError::NotFound);
        }

        let pid = self.pid();
        println!("[!] Terminating process with PID {}", pid);

        // Free all associated memory pages, and give its budget back what the process had
        // charged to it (budget.rs).
        crate::mem::MemoryManager::with_mut(|mm| {
            mm.release_ipc_frames(pid);
            // The final teardown step: a frame given back here may be handed to another process
            // at once, so this process never runs again.
            mm.release_owned_frames(pid, &self.mapping);
            // Its DMA frames are pooled only once every device that could hold their address
            // confirms a reset, or quarantined for ever (kernel/devices.md, `dma.rs`).
            mm.dma_release(pid);
            mm.process_ended(pid);
        });

        // Remove this PID from the process table
        ArchProcess::destroy(pid);
        self.set_state(ProcessState::Free);
        // And forget its address space. `process_create` draws PIDs from the free ones, and
        // `MemoryMapping::allocate` refuses a mapping that still names an address space, so a
        // terminated process must not keep a `satp` naming page tables that were just freed.
        self.mapping = Default::default();
        Ok(())
    }
}

/// Taken before `MEMORY_MANAGER`, never after it: the lock order is stated there (mem.rs).
static PROCESS_TABLE: KernelCell<ProcessTable> = KernelCell::new(ProcessTable {
    processes: [Process {
        state: ProcessState::Free,
        pid: None,
        mapping: arch::mem::DEFAULT_MEMORY_MAPPING,
        current_thread: 0,
    }; MAX_PROCESS_COUNT],
});

impl core::fmt::Debug for Process {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "Process {} state: {:?}  TID: {}  Memory mapping: {:?}",
            self.pid.map_or(0, |pid| pid.get()),
            self.state,
            self.current_thread,
            self.mapping
        )
    }
}

impl ProcessTable {
    /// Calls the provided function with the current inner process state.
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&ProcessTable) -> R,
    {
        PROCESS_TABLE.with(|ss| f(ss))
    }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessTable) -> R,
    {
        PROCESS_TABLE.with(f)
    }

    /// Create a new "System Services" object based on the arguments from the
    /// kernel. These arguments decide where the memory spaces are located, as
    /// well as where the stack and program counter should initially go.
    pub fn init_from_memory(&mut self, base: *const u32) {
        // The kernel, then `init`: the loader starts exactly one process (kernel/boot.md).
        // SAFETY: `base` is a page the loader allocated, zeroed and filled with these two
        // `InitialProcess` records; it is page-aligned, so aligned for the record, and two fit.
        let init_offsets =
            unsafe { core::slice::from_raw_parts(base as *const crate::arch::process::InitialProcess, 2) };

        // Copy over the initial process list. Each record names its PID in a field of its own
        // (`satp` carries none). For each process, translate it from a raw KernelArguments value
        // to a ProcessTable Process value.
        for init in init_offsets.iter() {
            let pid = init.pid();
            let process = &mut self.processes[usize::from(pid.get()) - 1];
            // SAFETY: `from_init_process` records a loader-built satp; the loader guarantees it names a root
            // table.
            unsafe { process.mapping.from_init_process(*init) };
            process.pid = Some(pid);
            process.current_thread = INITIAL_TID;
            process.set_state(if pid == KERNEL_PID {
                ProcessState::Running(TidMask::EMPTY)
            } else {
                ProcessState::Setup { entry: init.entrypoint, sp: init.sp, a0: init.a0, a1: init.a1 }
            });
        }

        // `kmain`'s own context. Its registers are saved at its first switch away (`sched.rs`);
        // until then only its thread number must be valid.
        ArchProcess::claim(KERNEL_PID);
        ArchProcess::setup_empty_process(KERNEL_PID);
        ArchProcess::setup_first_thread(KERNEL_PID, 0, 0, 0);
    }

    /// Give `pid` a slot in the process table and an address space, without a thread.
    /// The caller has already reserved the process against its budget (`budget.rs`); everything
    /// the address space takes is charged to that budget as it is allocated.
    pub fn allocate_process_slot(
        &mut self,
        mm: &mut crate::mem::MemoryManager,
        pid: Pid,
    ) -> Result<(), ProcessError> {
        let entry = self.processes.get_mut(pid.get() as usize - 1).ok_or(ProcessError::NotFound)?;
        if entry.state != ProcessState::Free {
            return Err(ProcessError::NotFound);
        }
        entry.pid = Some(pid);
        entry.set_state(ProcessState::Allocated);
        entry.current_thread = INITIAL_TID as TID;
        entry.mapping.allocate(mm, pid).map_err(ProcessError::Page).inspect_err(|_| {
            entry.set_state(ProcessState::Free);
            entry.mapping = Default::default();
        })?;
        // Only now can the new space be activated: `set_current_pid` refuses a PID the arch's
        // process table does not hold.
        ArchProcess::claim(pid);
        Ok(())
    }

    /// Give back the slot of a process that never started (a `process_create` that failed after
    /// its address space was made). Its frames have already been released.
    pub fn free_process_slot(&mut self, pid: Pid) {
        ArchProcess::destroy(pid);
        if let Some(entry) = self.processes.get_mut(pid.get() as usize - 1) {
            entry.set_state(ProcessState::Free);
            entry.mapping = Default::default();
        }
    }

    /// `process_start` has set up the first thread; the process becomes runnable.
    pub fn start_process(&mut self, pid: Pid) -> Result<(), ProcessError> {
        let process = self.get_process_mut(pid)?;
        match process.state {
            ProcessState::Allocated => {
                process.set_state(ProcessState::Ready(TidMask::of(INITIAL_TID)));
                Ok(())
            }
            _ => Err(ProcessError::NotFound),
        }
    }

    /// `thread_create(entry, sp, arg) -> tid` (kernel/processes.md). As `create_thread`, without
    /// a stack to reserve: a Redoubt thread is given a stack pointer, not a stack to reserve, and
    /// the calling thread keeps running with the new thread's id as its result.
    pub fn create_redoubt_thread(
        &mut self,
        pid: Pid,
        entry: usize,
        sp: usize,
        arg: usize,
    ) -> Result<TID, redoubt_sys::Error> {
        let process = self.get_process_mut(pid).map_err(|_| redoubt_sys::Error::NotPermitted)?;
        process.activate();
        let mut arch_process = ArchProcess::current();
        let new_tid = arch_process.find_free_thread().ok_or(redoubt_sys::Error::TooManyThreads)?;
        // A thread costs its budget a page (R6).
        crate::mem::MemoryManager::with_mut(|mm| mm.thread_created(pid, new_tid))?;
        arch_process.setup_redoubt_thread(new_tid, entry, sp, arg);
        let process = self.get_process_mut(pid).map_err(|_| redoubt_sys::Error::NotPermitted)?;
        process.set_state(match process.state {
            ProcessState::Running(x) => ProcessState::Running(x.with(new_tid)),
            ProcessState::Ready(x) => ProcessState::Ready(x.with(new_tid)),
            other => panic!("thread_create in a process that is {:?}", other),
        });
        Ok(new_tid)
    }

    pub fn get_process(&self, pid: Pid) -> Result<&Process, ProcessError> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if pid_idx >= self.processes.len() {
            return Err(ProcessError::NotFound);
        }
        if self.processes[pid_idx].pid != Some(pid) {
            Err(ProcessError::NotFound)
        } else if self.processes[pid_idx].state == ProcessState::Free {
            Err(ProcessError::NotFound)
        } else {
            Ok(&self.processes[pid_idx])
        }
    }

    pub fn get_process_mut(&mut self, pid: Pid) -> Result<&mut Process, ProcessError> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if pid_idx >= self.processes.len() {
            return Err(ProcessError::NotFound);
        }
        if self.processes[pid_idx].pid != Some(pid) {
            Err(ProcessError::NotFound)
        } else if self.processes[pid_idx].state == ProcessState::Free {
            Err(ProcessError::NotFound)
        } else {
            Ok(&mut self.processes[pid_idx])
        }
    }

    pub fn current_pid(&self) -> Pid { arch::process::current_pid() }

    /// Mark the specified context as ready to run. If the thread is Sleeping, mark
    /// it as Ready.
    pub fn ready_thread(&mut self, pid: Pid, tid: TID) -> Result<(), ProcessError> {
        let process = self.get_process_mut(pid)?;
        process.set_state(match process.state {
            ProcessState::Free => {
                panic!("PID {} was not running, so cannot wake thread {}", pid, tid)
            }
            ProcessState::Running(x) if !x.contains(tid) => ProcessState::Running(x.with(tid)),
            ProcessState::Ready(x) if !x.contains(tid) => ProcessState::Ready(x.with(tid)),
            ProcessState::Sleeping => ProcessState::Ready(TidMask::of(tid)),
            other => panic!("PID {} was not in a state to wake thread {}: {:?}", pid, tid, other),
        });
        klog!("Readying ({}:{}) -> {:?}", pid, tid, process.state);
        Ok(())
    }

    /// The ready thread to run after `current_thread`: the next TID above it, or else the lowest.
    pub fn find_next_thread(thread_mask: TidMask, current_thread: usize) -> usize {
        let lowest = thread_mask.iter().next().expect("no threads were available to run");
        thread_mask.iter().find(|tid| *tid > current_thread).unwrap_or(lowest)
    }

    /// Mark the current process as "Ready to run".
    ///
    /// # Panics
    ///
    /// If the current process is not running, or if it's "Running" but has no free contexts
    pub fn switch_to_thread(&mut self, pid: Pid, tid: Option<TID>) -> Result<(), ProcessError> {
        // `kmain` runs on every hart at once: the kernel's process stays `Running`.
        if pid == KERNEL_PID {
            self.get_process(KERNEL_PID)?.activate();
            ArchProcess::current().set_tid(INITIAL_TID);
            return Ok(());
        }
        let process = self.get_process_mut(pid)?;

        // Determine which thread to switch to
        let state = match process.state {
            ProcessState::Free => return Err(ProcessError::NotFound),
            ProcessState::Sleeping => return Err(ProcessError::NotFound),
            ProcessState::Allocated | ProcessState::Setup { .. } => return Err(ProcessError::NotFound),
            ProcessState::Ready(TidMask::EMPTY) => {
                panic!("ProcessState was `Ready` with no thread, which is invalid!");
            }
            // `Running`: another hart runs another of its threads. Either way the thread is one of
            // the ready ones, which no hart runs.
            ProcessState::Ready(ready_threads) | ProcessState::Running(ready_threads) => {
                let new_thread =
                    tid.unwrap_or_else(|| Self::find_next_thread(ready_threads, process.current_thread));

                if !ready_threads.contains(new_thread) {
                    return Err(ProcessError::NotReady);
                }

                process.activate();

                ArchProcess::current().set_tid(new_thread);
                process.current_thread = new_thread as _;
                ProcessState::Running(ready_threads.without(new_thread))
            }
        };
        process.set_state(state);

        Ok(())
    }

    /// Switches away from the specified process ID and ensures it won't
    /// get scheduled again.
    /// If no thread IDs are available, the process will enter a `Sleeping` state.
    ///
    /// # Panics
    ///
    /// If the current process is not running.
    pub fn unschedule_thread(&mut self, pid: Pid, tid: TID) -> Result<(), ProcessError> {
        let process = self.get_process_mut(pid)?;

        process.set_state(match process.state {
            ProcessState::Running(x) if x.contains(tid) => panic!(
                "PID {} thread {} was already queued for running when `unschedule_thread()` was called",
                pid, tid
            ),
            ProcessState::Running(x) => off_hart(pid, x),
            other => {
                panic!("PID {} TID {} was not in a state to be switched from: {:?}", pid, tid, other);
            }
        });
        Ok(())
    }

    /// The address space of `pid`, for the Redoubt memory steps that edit another process's
    /// page tables without switching to it (`message.rs`).
    pub fn mapping_of(&self, pid: Pid) -> Option<MemoryMapping> {
        self.get_process(pid).ok().map(|p| p.mapping)
    }

    /// Make `pid`'s address space the active one, for the steps that must run in it (choosing a
    /// buffer's address, writing a record into the receiver's own memory).
    pub fn activate(&self, pid: Pid) -> Result<(), ProcessError> {
        self.get_process(pid)?.activate();
        Ok(())
    }

    /// Hand a thread the registers a Redoubt call answers with (`redoubt-sys` encodes them). If
    /// `pid` is not the running process, it visits `pid`'s address space and comes back.
    pub fn set_redoubt_result(
        &mut self,
        pid: Pid,
        tid: TID,
        regs: &[u64; redoubt_sys::REGS],
    ) -> Result<(), ProcessError> {
        // Every register holds at most 32 bits or one `usize` (redoubt-sys).
        let words = regs.map(|r| r as usize);
        let current_pid = self.current_pid();
        if current_pid == pid {
            ArchProcess::current().set_thread_registers(tid, &words);
            return Ok(());
        }
        self.get_process(pid)?.activate();
        ArchProcess::current().set_thread_registers(tid, &words);
        self.get_process(current_pid)
            .expect("couldn't switch back after setting a Redoubt result")
            .activate();
        Ok(())
    }

    /// The running thread `previous_tid` of `previous_pid` is switched away from: it is ready
    /// again if `can_resume`.
    fn leave_previous(&mut self, previous_pid: Pid, previous_tid: TID, can_resume: bool) {
        let previous = self.get_process_mut(previous_pid).expect("couldn't get previous pid");
        let _oldstate = previous.state; // for tracking state in the debug print after the following closure
        previous.current_thread = previous_tid;
        previous.set_state(match previous.state {
            // The thread joins the ready ones only if `can_resume`; the process stays on the CPU
            // while another hart runs it.
            ProcessState::Running(x) => {
                off_hart(previous_pid, if can_resume { x.with(previous_tid) } else { x })
            }
            other => panic!(
                "previous process PID {} was in an invalid state (not Running): {:?}",
                previous_pid, other
            ),
        });
        klog!("PID {:?} state change from {:?} -> {:?}", previous_pid, _oldstate, previous.state);
    }

    /// Resume the given process, picking up exactly where it left off. If the
    /// process is in the Setup state, set it up and then resume.
    pub fn activate_process_thread(
        &mut self,
        previous_tid: TID,
        new_pid: Pid,
        mut new_tid: TID,
        can_resume: bool,
    ) -> Result<TID, ProcessError> {
        let previous_pid = self.current_pid();

        #[cfg(feature = "debug-print")]
        if new_tid != 0 {
            klog!("Activating process {} thread {}", new_pid, new_tid);
        } else {
            klog!("Activating process {} thread ANY", new_pid);
        }

        // `kmain` runs on every hart at once, so the kernel's process stays `Running` and only
        // the thread switched away from changes state.
        if new_pid == KERNEL_PID && previous_pid != KERNEL_PID {
            self.leave_previous(previous_pid, previous_tid, can_resume);
            self.get_process(KERNEL_PID)?.activate();
            ArchProcess::current().set_tid(INITIAL_TID);
            return Ok(INITIAL_TID);
        }

        // Save state if the PID has changed.  This will activate the new memory
        // space.
        if new_pid != previous_pid {
            {
                let new = self.get_process_mut(new_pid)?;
                klog!("New process original state: {:?}", new.state);

                // Ensure the new process can be run.
                match new.state {
                    ProcessState::Free => {
                        klog!("PID {} was free", new_pid);
                        return Err(ProcessError::NotFound);
                    }
                    ProcessState::Setup { .. } | ProcessState::Allocated => new_tid = INITIAL_TID,
                    ProcessState::Running(_) if !crate::arch::hart::runs_elsewhere(new_pid) => {
                        panic!("process was running even though no hart runs it")
                    }
                    // `Running`: another hart runs another of its threads, and this one joins it.
                    ProcessState::Ready(x) | ProcessState::Running(x) => {
                        // If no new context is specified, take the previous
                        // context.  If that is not runnable, do a round-robin
                        // search for the next available context.
                        assert!(!x.is_empty(), "process was {:?} but had no runnable threads", new.state);
                        if new_tid == 0 {
                            new_tid = Self::find_next_thread(x, new.current_thread);
                        }
                        if !x.contains(new_tid) {
                            println!(
                                "process state is {:?}, but new thread {} is not runnable",
                                new.state, new_tid
                            );
                            return Err(ProcessError::NotFound);
                        }
                        new.current_thread = new_tid as _;
                    }
                    ProcessState::Sleeping => {
                        return Err(ProcessError::NotFound);
                    }
                }
            }

            // Perform the actual switch to the new memory space.  From this
            // point onward, we will need to activate the previous memory space
            // if we encounter an error.
            let new = self.get_process(new_pid)?;
            new.mapping.activate();

            // Set up the new process, if necessary.  Remove the new thread from
            // the list of ready threads.
            let new = self.get_process_mut(new_pid)?;
            new.set_state(match new.state {
                ProcessState::Setup { entry, sp, a0, a1 } => {
                    ArchProcess::setup_loader_process(new_pid, entry, sp, a0, a1);

                    ProcessState::Running(TidMask::EMPTY)
                }
                ProcessState::Allocated => ProcessState::Running(TidMask::EMPTY),
                ProcessState::Free => panic!("process was suddenly Free"),
                ProcessState::Ready(x) | ProcessState::Running(x) => {
                    ProcessState::Running(x.without(new_tid))
                }
                ProcessState::Sleeping => ProcessState::Running(TidMask::EMPTY),
            });
            new.activate();

            // Mark the previous process as ready to run, since we just switched away. The
            // kernel's process stays `Running`: `kmain` runs on every hart.
            if previous_pid != KERNEL_PID {
                self.leave_previous(previous_pid, previous_tid, can_resume);
            }
        } else {
            let new = self.get_process_mut(new_pid)?;

            // If we wanted to switch to a "new" thread, and it's the same
            // as the one we just switched from, do nothing.
            if previous_tid == new_tid {
                if !can_resume {
                    panic!(
                        "tried to switch to our own thread without resume (current_thread: {}  previous_tid: {}  new_tid: {})",
                        new.current_thread, previous_tid, new_tid
                    );
                }
                let mut process = ArchProcess::current();
                process.set_tid(new_tid);
                new.current_thread = new_tid;
                return Ok(new_tid);
            }

            // Transition to the new state.
            // The thread this hart ran is `previous_tid`; other harts may run others of the
            // process, none of them among the ready ones.
            let state = if let ProcessState::Running(x) = new.state {
                assert!(!x.contains(previous_tid));

                // If the current process can be resumed, add it to the list
                // of potential threads
                let x = if can_resume { x.with(previous_tid) } else { x };

                // If no new thread is specified, take the previous
                // thread.  If that is not runnable, do a round-robin
                // search for the next available thread.
                if new_tid == 0 {
                    new_tid = Self::find_next_thread(x, previous_tid);
                }

                if !x.contains(new_tid) {
                    return Err(ProcessError::NotReady);
                }

                new.current_thread = new_tid as _;

                // Remove the new TID from the list of threads that can be run.
                ProcessState::Running(x.without(new_tid))
            } else {
                panic!("PID {} invalid process state (not Running): {:?}", previous_pid, new.state)
            };
            new.set_state(state);
        }

        // Restore the previous thread, if one exists.
        ArchProcess::current().set_tid(new_tid);

        klog!(
            "Activated process {}:{}, new state: {:?}",
            new_pid,
            new_tid,
            self.get_process_mut(new_pid)?.state
        );

        Ok(new_tid)
    }

    /// Destroy the given thread. Returns `true` if the PID has been updated.
    /// # Errors
    ///
    /// * **ThreadNotAvailable**: The thread does not exist in this process
    pub fn destroy_thread(&mut self, pid: Pid, tid: TID) -> Result<bool, ProcessError> {
        let current_pid = self.current_pid();
        assert_eq!(pid, current_pid);

        // R4b: whatever this thread was waiting for is withdrawn, and every call it holds
        // open fails its caller with `Dead`, before the thread's own state goes. It runs first,
        // because a caller it wakes is one of the runnable threads read just below.
        crate::mem::MemoryManager::with_mut(|mm| crate::message::thread_ending(self, mm, pid, tid));

        let waiting_threads = match self.get_process_mut(pid)?.state {
            ProcessState::Running(x) => x,
            state => panic!("Process was in an invalid state: {:?}", state),
        };

        // Destroy the thread at a hardware level
        if ArchProcess::current().destroy_thread(tid) {
            crate::mem::MemoryManager::with_mut(|mm| mm.thread_ended(pid, tid));
        }

        // Off this hart: `Ready` with waiting threads, `Sleeping` without, and still `Running`
        // while another hart runs it. With none waiting, this hart goes to `kmain`.
        let new_pid = if waiting_threads.is_empty() { KERNEL_PID } else { pid };
        self.get_process_mut(pid)?.set_state(off_hart(pid, waiting_threads));

        // Switch to the next available TID. This moves the process back to a `Running` state.
        self.switch_to_thread(new_pid, None)?;

        Ok(new_pid != pid)
    }

    /// Terminate the given process, the running one; the CPU goes to `kmain`.
    pub fn terminate_process(&mut self, target_pid: Pid) -> Result<(), ProcessError> {
        #[cfg(feature = "debug-print")]
        println!("terminate_process: {:?}", target_pid);
        // Another hart may be running another of its threads: shoot it down there before any of
        // its memory goes.
        evict(target_pid);
        // R4b: every call its threads hold open fails its caller with `Dead`, and every message
        // they were sending is withdrawn, before its memory goes.
        crate::mem::MemoryManager::with_mut(|mm| crate::message::process_ending(self, mm, target_pid));

        let process = self.get_process_mut(target_pid)?;
        process.activate();
        process.terminate()?;
        crate::mem::MemoryManager::with_mut(|mm| crate::message::destroy_quarantined_devices(self, mm));

        self.switch_to_thread(KERNEL_PID, None).unwrap();

        Ok(())
    }

    /// End process `target` on behalf of the running process (R10 kills the processes of a
    /// destroyed budget), which keeps running: the same teardown as `terminate_process`, then
    /// the running process's address space is active again. `target` must not be the running
    /// process.
    pub fn kill_process(&mut self, target: Pid) -> Result<(), ProcessError> {
        let current = self.current_pid();
        assert!(target != current, "kill_process on the running process");
        // Another hart may be running it: shoot it down there before any of its memory goes.
        evict(target);
        crate::mem::MemoryManager::with_mut(|mm| crate::message::process_ending(self, mm, target));
        // `terminate` needs no address space: it names the target's mapping itself.
        self.get_process_mut(target)?.terminate()?;
        crate::mem::MemoryManager::with_mut(|mm| crate::message::destroy_quarantined_devices(self, mm));
        self.get_process(current)?.activate();
        Ok(())
    }
}

/// The state of a running process that one of its threads just left a hart in, `ready` its ready
/// threads now: still `Running` while another hart runs it, else `Ready`, or `Sleeping` with none.
fn off_hart(pid: Pid, ready: TidMask) -> ProcessState {
    if crate::arch::hart::runs_elsewhere(pid) {
        ProcessState::Running(ready)
    } else if ready.is_empty() {
        ProcessState::Sleeping
    } else {
        ProcessState::Ready(ready)
    }
}

/// Process `pid` is ending: shoot it down on every other hart running it, which leaves its space,
/// before any of its memory goes (kernel/memory.md, "Residual risks").
fn evict(pid: Pid) {
    // Debug only, never in a bench build but one recorded negative run: no hart is shot down.
    if !cfg!(feature = "smp-no-evict") {
        let _asked = crate::arch::hart::shootdown(pid, crate::arch::hart::Shot::Leave);
        // The checked build's word for `smp-evict`: every hart running it has left it.
        #[cfg(debug_assertions)]
        if _asked != 0 {
            println!(
                "shootdown: PID {} stopped on hart(s) {:#b} before any of its frames is freed",
                pid, _asked
            );
        }
    }
    #[cfg(debug_assertions)]
    crate::arch::hart::audit_left(pid);
}
