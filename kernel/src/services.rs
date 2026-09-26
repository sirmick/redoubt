// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0


use redoubt_abi::arch::*;
// use core::mem;
use redoubt_abi::{PID, TID};

use crate::arch;
use crate::arch::mem::MemoryMapping;
pub use crate::arch::process::Process as ArchProcess;
pub use crate::arch::process::Thread;
use crate::cell::KernelCell;

/// The kernel is always process 1.
pub const KERNEL_PID: PID = match PID::new(1) {
    Some(pid) => pid,
    None => unreachable!(),
};
#[allow(dead_code)]
const FIRST_USER_PID: PID = match PID::new(2) {
    Some(pid) => pid,
    None => unreachable!(),
};

pub use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT};

// fn log_process_update(f: &str, l: u32, process: &Process, old_state: ProcessState) {
//     if process.pid.get() == 3 {
//         println!("[{}:{}] Updated PID {:?} state: {:?} -> {:?}", f, l, process.pid, old_state,
// process.state);     }
// }

/// A big unifying struct containing all of the system state.
/// This is inherited from the stage 1 bootloader.
pub struct SystemServices {
    /// A table of all processes in the system
    pub processes: [Process; MAX_PROCESS_COUNT],
}

#[derive(Copy, Clone, PartialEq)]
pub enum ProcessState {
    /// This is an unallocated, free process
    Free,

    /// This process has been allocated, but has no threads yet
    Allocated,

    /// A loader-bundle program that hasn't run yet: its first thread starts at `entry` with stack
    /// pointer `sp` when it is first switched to (INTERIM, until R3's `init` launches them).
    Setup { entry: usize, sp: usize },

    /// This process is able to be run.  The context bitmask describes contexts
    /// that are ready.
    Ready(usize /* context bitmask */),

    /// This is the current active process.  The context bitmask describes
    /// contexts that are ready, excluding the currently-executing context.
    Running(usize /* context bitmask */),

    /// This process is waiting for an event, such as as message or an
    /// interrupt.  There are no contexts that can be run. This is
    /// functionally equivalent to the invalid `Ready(0)` state.
    Sleeping,
}

impl core::fmt::Debug for ProcessState {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        use ProcessState::*;
        match *self {
            Free => write!(fmt, "Free"),
            Allocated => write!(fmt, "Allocated"),
            Setup { entry, sp } => write!(fmt, "Setup {{ entry: {:#x}, sp: {:#x} }}", entry, sp),
            Ready(rt) => write!(fmt, "Ready({:b})", rt),
            Running(rt) => write!(fmt, "Running({:b})", rt),
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

    /// This process' PID. This should match up with the index in the process table.
    pub pid: PID,

    /// The current thread ID
    pub current_thread: TID,
}

impl Default for Process {
    fn default() -> Self {
        Process {
            state: ProcessState::Allocated,
            pid: FIRST_USER_PID,
            current_thread: 0,
            mapping: Default::default(),
        }
    }
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

    /// A copy of this process' ID
    pub pid: PID,

    /// Some reserved data to pad this out to a multiple of 32 bytes.
    pub _reserved: [u8; 1],
}

impl Default for ProcessInner {
    fn default() -> Self {
        ProcessInner {
            mem_default_base: DEFAULT_BASE,
            mem_default_last: DEFAULT_BASE,
            mem_message_base: DEFAULT_MESSAGE_BASE,
            mem_message_last: DEFAULT_MESSAGE_BASE,
            pid: KERNEL_PID,
            _reserved: [0; 1],
        }
    }
}

impl Process {

    /// This process slot is unallocated and may be turn into a process
    pub fn free(&self) -> bool { matches!(self.state, ProcessState::Free) }

    /// The threads the scheduler may run next (`sched.rs`): a bit per thread waiting for the
    /// CPU, or `None` for a process whose next thread `activate_process_thread` chooses itself
    /// (being set up). A running thread is not waiting.
    pub fn ready_threads(&self) -> Option<usize> {
        match self.state {
            ProcessState::Ready(x) | ProcessState::Running(x) => Some(x),
            ProcessState::Setup { .. } => None,
            _ => Some(0),
        }
    }

    /// Whether the process is on the CPU.
    pub fn running(&self) -> bool { matches!(self.state, ProcessState::Running(_)) }

    pub fn activate(&self) -> Result<(), redoubt_abi::Error> {
        crate::arch::process::set_current_pid(self.pid);
        self.mapping.activate()?;
        let mut current_process = ArchProcess::current();
        current_process.activate()
    }

    pub fn terminate(&mut self) -> Result<(), redoubt_abi::Error> {
        if self.free() {
            return Err(redoubt_abi::Error::ProcessNotFound);
        }

        println!("[!] Terminating process with PID {}", self.pid);

        // Free all associated memory pages, and give its budget back what the process had
        // charged to it (budget.rs).
        crate::mem::MemoryManager::with_mut(|mm| {
            // SAFETY: called only here, as the final teardown step for a process that will not run again.
            unsafe { mm.release_all_memory_for_process(self.pid, &self.mapping) };
            // Its DMA frames are pooled only once every device that could hold their address
            // confirms a reset, or quarantined for ever (WP-K5b, `dma.rs`).
            mm.dma_release(self.pid);
            mm.process_ended(self.pid);
        });

        // Remove this PID from the process table
        ArchProcess::destroy(self.pid)?;
        self.state = ProcessState::Free;
        // And forget its address space. Until WP-K4 nothing ever reused a PID, so a terminated
        // process could keep a `satp` naming page tables that had just been freed; now
        // `process_create` draws PIDs from the free ones, and `MemoryMapping::allocate` refuses
        // a mapping that still names an address space.
        self.mapping = Default::default();
        Ok(())
    }
}

/// Taken before `MEMORY_MANAGER`, never after it: the lock order is stated there (mem.rs).
static SYSTEM_SERVICES: KernelCell<SystemServices> = KernelCell::new(SystemServices {
    processes: [Process {
        state: ProcessState::Free,
        pid: KERNEL_PID,
        mapping: arch::mem::DEFAULT_MEMORY_MAPPING,
        current_thread: INITIAL_TID,
    }; MAX_PROCESS_COUNT],
});

impl core::fmt::Debug for Process {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "Process {} state: {:?}  TID: {}  Memory mapping: {:?}",
            self.pid.get(),
            self.state,
            self.current_thread,
            self.mapping
        )
    }
}

impl SystemServices {
    /// Calls the provided function with the current inner process state.
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&SystemServices) -> R,
    {
        SYSTEM_SERVICES.with(|ss| f(ss))
    }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut SystemServices) -> R,
    {
        SYSTEM_SERVICES.with(f)
    }

    /// Create a new "System Services" object based on the arguments from the
    /// kernel. These arguments decide where the memory spaces are located, as
    /// well as where the stack and program counter should initially go.
    pub fn init_from_memory(&mut self, base: *const u32, args: &crate::args::KernelArguments) {
        // Look through the kernel arguments and create a new process for each.
        let init_offsets = {
            // The kernel, then one `IniE` tag per loader process (BOOT.md).
            let init_count = 1 + args.iter().filter(|arg| arg.name == u32::from_le_bytes(*b"IniE")).count();
            // The loader writes the table into one page, one record per process (BOOT.md), and
            // refuses a bundle with more processes than the kernel has room for. This is the
            // kernel's side of that check: a count beyond either limit means the two disagree,
            // and the boot stops here rather than at an index somewhere later.
            let capacity = (redoubt_abi::arch::PAGE_SIZE / size_of::<crate::arch::process::InitialProcess>())
                .min(crate::arch::process::MAX_PROCESS_COUNT);
            assert!(
                init_count <= capacity,
                "the loader reported {} initial processes, room is {}",
                init_count,
                capacity
            );
            // SAFETY: `base` is that page, which the loader allocated, zeroed and filled with
            // one `InitialProcess` per process; it is page-aligned, so aligned for the record,
            // and `init_count` records fit within it.
            unsafe {
                core::slice::from_raw_parts(base as *const crate::arch::process::InitialProcess, init_count)
            }
        };

        // Copy over the initial process list.  The pid is encoded in the SATP
        // value from the bootloader.  For each process, translate it from a raw
        // KernelArguments value to a SystemServices Process value.
        for init in init_offsets.iter() {
            let pid = init.pid().get();
            let proc_idx = pid - 1;
            let process = &mut self.processes[proc_idx as usize];
            // println!(
            //     "Process[{}]: {:?}",
            //     pid - 1,
            //     init,
            // );
            // SAFETY: `from_init_process` records a loader-built satp; the loader guarantees it names a root
            // table.
            unsafe {
                process.mapping.from_init_process(*init);
                process.pid = PID::new(pid as _).unwrap();
            };
            // let old_state = process.state;
            process.state = if pid == 1 {
                ProcessState::Running(0)
            } else {
                ProcessState::Setup { entry: init.entrypoint, sp: init.sp }
            };
            // log_process_update(file!(), line!(), process, old_state);
        }

        // `kmain`'s own context. Its registers are saved at its first switch away (`sched.rs`);
        // until then only its thread number must be valid.
        ArchProcess::claim(KERNEL_PID);
        ArchProcess::setup_empty_process(KERNEL_PID);
        ArchProcess::setup_first_thread(KERNEL_PID, 0, 0, 0);
    }

    /// WP-K4: give `pid` a slot in the process table and an address space, without a thread.
    /// The caller has already reserved the process against its budget (`budget.rs`); everything
    /// the address space takes is charged to that budget as it is allocated.
    pub fn allocate_process_slot(
        &mut self,
        mm: &mut crate::mem::MemoryManager,
        pid: PID,
    ) -> Result<(), redoubt_abi::Error> {
        let entry =
            self.processes.get_mut(pid.get() as usize - 1).ok_or(redoubt_abi::Error::ProcessNotFound)?;
        if entry.state != ProcessState::Free {
            return Err(redoubt_abi::Error::ProcessNotFound);
        }
        entry.pid = pid;
        entry.state = ProcessState::Allocated;
        entry.current_thread = INITIAL_TID as TID;
        entry.mapping.allocate(mm, pid).inspect_err(|_| {
            entry.state = ProcessState::Free;
            entry.mapping = Default::default();
        })?;
        // Only now can the new space be activated: `set_current_pid` refuses a PID the arch's
        // process table does not hold.
        ArchProcess::claim(pid);
        Ok(())
    }

    /// WP-K4: give back the slot of a process that never started (a `process_create` that failed
    /// after its address space was made). Its frames have already been released.
    pub fn free_process_slot(&mut self, pid: PID) {
        ArchProcess::destroy(pid).ok();
        if let Some(entry) = self.processes.get_mut(pid.get() as usize - 1) {
            entry.state = ProcessState::Free;
            entry.mapping = Default::default();
        }
    }

    /// WP-K4: `process_start` has set up the first thread; the process becomes runnable.
    pub fn start_process(&mut self, pid: PID) -> Result<(), redoubt_abi::Error> {
        let process = self.get_process_mut(pid)?;
        match process.state {
            ProcessState::Allocated => {
                process.state = ProcessState::Ready(1 << INITIAL_TID);
                Ok(())
            }
            _ => Err(redoubt_abi::Error::ProcessNotFound),
        }
    }

    /// WP-K4: `thread_create(entry, sp, arg) -> tid` (KERNEL-SPEC.md). As `create_thread`,
    /// without a stack to reserve: a Redoubt thread is given a stack pointer, not a stack
    /// to reserve, and the calling thread keeps running with the new thread's id as its result.
    pub fn create_redoubt_thread(
        &mut self,
        pid: PID,
        entry: usize,
        sp: usize,
        arg: usize,
    ) -> Result<TID, redoubt_sys::Error> {
        let process = self.get_process_mut(pid).map_err(|_| redoubt_sys::Error::NotPermitted)?;
        process.activate().map_err(|_| redoubt_sys::Error::NotPermitted)?;
        let mut arch_process = ArchProcess::current();
        let new_tid = arch_process.find_free_thread().ok_or(redoubt_sys::Error::TooManyThreads)?;
        // A thread costs its budget a page (R6).
        crate::mem::MemoryManager::with_mut(|mm| mm.thread_created(pid, new_tid))?;
        arch_process.setup_redoubt_thread(new_tid, entry, sp, arg);
        let process = self.get_process_mut(pid).map_err(|_| redoubt_sys::Error::NotPermitted)?;
        process.state = match process.state {
            ProcessState::Running(x) => ProcessState::Running(x | (1 << new_tid)),
            ProcessState::Ready(x) => ProcessState::Ready(x | (1 << new_tid)),
            other => panic!("thread_create in a process that is {:?}", other),
        };
        Ok(new_tid)
    }

    pub fn get_process(&self, pid: PID) -> Result<&Process, redoubt_abi::Error> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if pid_idx >= self.processes.len() {
            return Err(redoubt_abi::Error::ProcessNotFound);
        }
        if self.processes[pid_idx].mapping.get_pid() != Some(pid) {
            Err(redoubt_abi::Error::ProcessNotFound)
        } else if self.processes[pid_idx].state == ProcessState::Free {
            Err(redoubt_abi::Error::ProcessNotFound)
        } else {
            Ok(&self.processes[pid_idx])
        }
    }

    pub fn get_process_mut(&mut self, pid: PID) -> Result<&mut Process, redoubt_abi::Error> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if pid_idx >= self.processes.len() {
            return Err(redoubt_abi::Error::ProcessNotFound);
        }
        if self.processes[pid_idx].mapping.get_pid() != Some(pid) {
            Err(redoubt_abi::Error::ProcessNotFound)
        } else if self.processes[pid_idx].state == ProcessState::Free {
            Err(redoubt_abi::Error::ProcessNotFound)
        } else {
            Ok(&mut self.processes[pid_idx])
        }
    }

    pub fn current_pid(&self) -> PID { arch::process::current_pid() }

    /// Mark the specified context as ready to run. If the thread is Sleeping, mark
    /// it as Ready.
    pub fn ready_thread(&mut self, pid: PID, tid: TID) -> Result<(), redoubt_abi::Error> {
        let process = self.get_process_mut(pid)?;
        // let old_state = process.state;
        process.state = match process.state {
            ProcessState::Free => {
                panic!("PID {} was not running, so cannot wake thread {}", pid, tid)
            }
            ProcessState::Running(x) if x & (1 << tid) == 0 => ProcessState::Running(x | (1 << tid)),
            ProcessState::Ready(x) if x & (1 << tid) == 0 => ProcessState::Ready(x | (1 << tid)),
            ProcessState::Sleeping => ProcessState::Ready(1 << tid),
            other => panic!("PID {} was not in a state to wake thread {}: {:?}", pid, tid, other),
        };
        // log_process_update(file!(), line!(), process, old_state);
        klog!("Readying ({}:{}) -> {:?}", pid, tid, process.state);
        Ok(())
    }

    #[cfg(target_pointer_width = "32")]
    pub fn find_next_thread(thread_mask: usize, current_thread: usize) -> usize {
        // From https://graphics.stanford.edu/~seander/bithacks.html#ZerosOnRightMultLookup
        // This platform has a multiplier, so this is fast
        fn trailing_zeros(v: usize) -> usize {
            const MULTIPLY_DEBRUIJN_BIT_POSITION: [usize; 32] = [
                0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16, 7, 26,
                12, 18, 6, 11, 5, 10, 9,
            ];

            // The multiply is a hash: it is meant to wrap, so say so, or a checked build
            // panics here instead of scheduling (docs/testbench.md, "Debug assertions").
            MULTIPLY_DEBRUIJN_BIT_POSITION[((!v.wrapping_sub(1) & v).wrapping_mul(0x077CB531)) >> 27]
        }
        // If there's only one thread runnable, run that one
        if thread_mask == 0 {
            panic!("no threads were available to run");
        }

        // if thread_mask.is_power_of_two() {
        if thread_mask & (thread_mask - 1) == 0 {
            trailing_zeros(thread_mask)
        } else {
            let upper_bits = thread_mask & !((2usize << current_thread) - 1);
            if upper_bits != 0 { trailing_zeros(upper_bits) } else { trailing_zeros(thread_mask) }
        }
    }

    #[cfg(not(target_pointer_width = "32"))]
    pub fn find_next_thread(thread_mask: usize, current_thread: usize) -> usize {
        if thread_mask == 0 {
            panic!("no threads were available to run");
        }
        if thread_mask.is_power_of_two() {
            thread_mask.trailing_zeros() as usize
        } else {
            let upper_bits = thread_mask & !((2usize << current_thread) - 1);
            if upper_bits != 0 {
                upper_bits.trailing_zeros() as usize
            } else {
                thread_mask.trailing_zeros() as usize
            }
        }
    }

    /// Mark the current process as "Ready to run".
    ///
    /// # Panics
    ///
    /// If the current process is not running, or if it's "Running" but has no free contexts
    pub fn switch_to_thread(&mut self, pid: PID, tid: Option<TID>) -> Result<(), redoubt_abi::Error> {
        let process = self.get_process_mut(pid)?;
        // klog!(
        //     "switch_to_thread({}:{:?}): Old state was {:?}",
        //     pid, tid, process.state
        // );

        // let old_state = process.state;
        // Determine which thread to switch to
        process.state = match process.state {
            ProcessState::Free => return Err(redoubt_abi::Error::ProcessNotFound),
            ProcessState::Sleeping => return Err(redoubt_abi::Error::ProcessNotFound),
            ProcessState::Allocated | ProcessState::Setup { .. } => {
                return Err(redoubt_abi::Error::ProcessNotFound)
            }
            ProcessState::Ready(0) => {
                panic!("ProcessState was `Ready(0)`, which is invalid!");
            }
            ProcessState::Ready(ready_threads) => {
                let new_thread =
                    tid.unwrap_or_else(|| Self::find_next_thread(ready_threads, process.current_thread));

                if ready_threads & (1 << new_thread) == 0 {
                    panic!("invalid thread ID");
                }

                process.activate()?;

                ArchProcess::current().set_tid(new_thread)?;
                process.current_thread = new_thread as _;
                ProcessState::Running(ready_threads & !(1 << new_thread))
            }
            ProcessState::Running(ready_threads) => {
                // Ensure we can switch back to this thread, if necessary
                let ready_threads = ready_threads | (1 << process.current_thread);

                let new_thread =
                    tid.unwrap_or_else(|| Self::find_next_thread(ready_threads, process.current_thread));

                // Ensure the specified context is ready to run, or is
                // currently running.
                if ready_threads & (1 << new_thread) == 0 {
                    return Err(redoubt_abi::Error::InvalidThread);
                }

                // Activate this process on this CPU
                ArchProcess::current().set_tid(new_thread)?;
                process.current_thread = new_thread as _;
                ProcessState::Running(ready_threads & !(1 << new_thread))
            }
        };
        // log_process_update(file!(), line!(), process, old_state);

        // println!(
        //     "switch_to_thread({}:{:?}): New state is {:?} Thread is ",
        //     pid, tid, process.state
        // );
        // ArchProcess::with_current(|current| current.print_thread());

        Ok(())
    }

    /// Switches away from the specified process ID and ensures it won't
    /// get scheduled again.
    /// If no thread IDs are available, the process will enter a `Sleeping` state.
    ///
    /// # Panics
    ///
    /// If the current process is not running.
    pub fn unschedule_thread(&mut self, pid: PID, tid: TID) -> Result<(), redoubt_abi::Error> {
        let process = self.get_process_mut(pid)?;
        // klog!(
        //     "unschedule_thread({}:{}): Old state was {:?}",
        //     pid, tid, process.state
        // );
        // ArchProcess::with_current(|current| current.print_thread());

        // let old_state = process.state;
        process.state = match process.state {
            ProcessState::Running(x) if x & (1 << tid) != 0 => panic!(
                "PID {} thread {} was already queued for running when `unschedule_thread()` was called",
                pid, tid
            ),
            ProcessState::Running(0) => ProcessState::Sleeping,
            ProcessState::Running(x) => ProcessState::Ready(x),
            other => {
                panic!("PID {} TID {} was not in a state to be switched from: {:?}", pid, tid, other);
            }
        };
        Ok(())
    }

    /// The address space of `pid`, for the Redoubt memory steps that edit another process's
    /// page tables without switching to it (`message.rs`).
    pub fn mapping_of(&self, pid: PID) -> Option<MemoryMapping> {
        self.get_process(pid).ok().map(|p| p.mapping)
    }

    /// Make `pid`'s address space the active one, for the steps that must run in it (choosing a
    /// buffer's address, writing a record into the receiver's own memory).
    pub fn activate(&self, pid: PID) -> Result<(), redoubt_abi::Error> { self.get_process(pid)?.activate() }

    /// Hand a thread the registers a Redoubt call answers with (`redoubt-sys` encodes them). If
    /// `pid` is not the running process, it visits `pid`'s address space and comes back.
    pub fn set_redoubt_result(
        &mut self,
        pid: PID,
        tid: TID,
        regs: &[u64; redoubt_sys::REGS],
    ) -> Result<(), redoubt_abi::Error> {
        // Every register holds at most 32 bits or one `usize` (redoubt-sys).
        let words = regs.map(|r| r as usize);
        let current_pid = self.current_pid();
        if current_pid == pid {
            ArchProcess::current().set_thread_registers(tid, &words);
            return Ok(());
        }
        self.get_process(pid)?.activate()?;
        ArchProcess::current().set_thread_registers(tid, &words);
        self.get_process(current_pid).expect("couldn't switch back after setting a Redoubt result").activate()
    }

    /// Resume the given process, picking up exactly where it left off. If the
    /// process is in the Setup state, set it up and then resume.
    pub fn activate_process_thread(
        &mut self,
        previous_tid: TID,
        new_pid: PID,
        mut new_tid: TID,
        can_resume: bool,
    ) -> Result<TID, redoubt_abi::Error> {
        let previous_pid = self.current_pid();

        #[cfg(feature = "debug-print")]
        if new_tid != 0 {
            klog!("Activating process {} thread {}", new_pid, new_tid);
        } else {
            klog!("Activating process {} thread ANY", new_pid);
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
                        return Err(redoubt_abi::Error::ProcessNotFound);
                    }
                    ProcessState::Setup { .. } | ProcessState::Allocated => new_tid = INITIAL_TID,
                    ProcessState::Ready(x) => {
                        // If no new context is specified, take the previous
                        // context.  If that is not runnable, do a round-robin
                        // search for the next available context.
                        assert!(x != 0, "process was {:?} but had no runnable threads", new.state);
                        if new_tid == 0 {
                            new_tid = Self::find_next_thread(x, new.current_thread);
                        }
                        if x & (1 << new_tid) == 0 {
                            println!(
                                "process state is {:?}, but new thread {} is not runnable",
                                new.state, new_tid
                            );
                            return Err(redoubt_abi::Error::ProcessNotFound);
                        }
                        new.current_thread = new_tid as _;
                    }
                    ProcessState::Running(_) => {
                        panic!("process was running even though the pid was different")
                    }
                    ProcessState::Sleeping => {
                        return Err(redoubt_abi::Error::ProcessNotFound);
                    }
                }
            }

            // Perform the actual switch to the new memory space.  From this
            // point onward, we will need to activate the previous memory space
            // if we encounter an error.
            let new = self.get_process(new_pid)?;
            new.mapping.activate()?;

            // Set up the new process, if necessary.  Remove the new thread from
            // the list of ready threads.
            // let old_state = new.state;
            let new = self.get_process_mut(new_pid)?;
            new.state = match new.state {
                ProcessState::Setup { entry, sp } => {
                    ArchProcess::setup_loader_process(new_pid, entry, sp);
                    ArchProcess::with_inner_mut(|process_inner| process_inner.pid = new_pid);

                    ProcessState::Running(0)
                }
                ProcessState::Allocated => {
                    ArchProcess::with_inner_mut(|process_inner| process_inner.pid = new_pid);
                    ProcessState::Running(0)
                }
                ProcessState::Free => panic!("process was suddenly Free"),
                ProcessState::Ready(x) | ProcessState::Running(x) => {
                    ProcessState::Running(x & !(1 << new_tid))
                }
                ProcessState::Sleeping => ProcessState::Running(0),
            };
            // log_process_update(file!(), line!(), new, old_state);
            new.activate()?;

            // Mark the previous process as ready to run, since we just switched
            // away
            let previous = self.get_process_mut(previous_pid).expect("couldn't get previous pid");
            let _oldstate = previous.state; // for tracking state in the debug print after the following closure
            if previous.current_thread != previous_tid {
                println!(
                    "WARNING: previous.current_thread {} != previous_tid {}",
                    previous.current_thread, previous_tid
                );
            }
            previous.current_thread = previous_tid;
            previous.state = match previous.state {
                // If the previous process had exactly one thread that can be
                // run, then the Running thread list will be 0.  In that case,
                // we will either need to Sleep this process, or mark it as
                // being Ready to run.
                ProcessState::Running(x) if x == 0 => {
                    if can_resume {
                        ProcessState::Ready(1 << previous_tid)
                    } else {
                        ProcessState::Sleeping
                    }
                }
                // Otherwise, there are additional threads that can be run.
                // Convert the previous process into "Ready", and include the
                // current context number only if `can_resume` is `true`.
                ProcessState::Running(x) => {
                    if can_resume {
                        ProcessState::Ready(x | (1 << previous_tid))
                    } else {
                        ProcessState::Ready(x)
                    }
                }
                other => panic!(
                    "previous process PID {} was in an invalid state (not Running): {:?}",
                    previous_pid, other
                ),
            };
            // log_process_update(file!(), line!(), previous, _oldstate);
            klog!("PID {:?} state change from {:?} -> {:?}", previous_pid, _oldstate, previous.state);
            // klog!(
            //     "Set previous process PID {} state to {:?} (with can_resume = {})",
            //     previous_pid,
            //     previous.state,
            //     can_resume
            // );
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
                process.set_tid(new_tid).unwrap();
                new.current_thread = new_tid;
                return Ok(new_tid);
            }

            // Transition to the new state.
            // let old_state = new.state;
            new.state = if let ProcessState::Running(x) = new.state {
                assert!(x & (1 << new.current_thread) == 0);

                // If the current process can be resumed, add it to the list
                // of potential threads
                let x = x | if can_resume { 1 << new.current_thread } else { 0 };

                // If no new thread is specified, take the previous
                // thread.  If that is not runnable, do a round-robin
                // search for the next available thread.
                if new_tid == 0 {
                    new_tid = Self::find_next_thread(x, new.current_thread);
                }

                if x & (1 << new_tid) == 0 {
                    return Err(redoubt_abi::Error::ThreadNotAvailable);
                }

                new.current_thread = new_tid as _;

                // Remove the new TID from the list of threads that can be run.
                ProcessState::Running(x & !(1 << new_tid))
            } else {
                panic!("PID {} invalid process state (not Running): {:?}", previous_pid, new.state)
            };
            // log_process_update(file!(), line!(), new, old_state);
        }

        // Restore the previous thread, if one exists.
        ArchProcess::current().set_tid(new_tid)?;

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
    pub fn destroy_thread(&mut self, pid: PID, tid: TID) -> Result<bool, redoubt_abi::Error> {
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
        if ArchProcess::current().destroy_thread(tid).is_ok() {
            crate::mem::MemoryManager::with_mut(|mm| mm.thread_ended(pid, tid));
        }

        // Mark this process as `Ready` if there are waiting threads, or `Sleeping` if
        // there are no waiting threads.
        let mut new_pid = pid;
        {
            let process = self.get_process_mut(pid)?;
            // let old_state = process.state;
            process.state = if waiting_threads == 0 {
                new_pid = KERNEL_PID;
                ProcessState::Sleeping
            } else {
                ProcessState::Ready(waiting_threads)
            };
            // log_process_update(file!(), line!(), process, old_state);
        }

        // Switch to the next available TID. This moves the process back to a `Running` state.
        self.switch_to_thread(new_pid, None)?;

        Ok(new_pid != pid)
    }

    /// Terminate the given process, the running one; the CPU goes to `kmain`.
    pub fn terminate_process(&mut self, target_pid: PID) -> Result<(), redoubt_abi::Error> {
        println!("terminate_process: {:?}", target_pid);
        // R4b: every call its threads hold open fails its caller with `Dead`, and every message
        // they were sending is withdrawn, before its memory goes.
        crate::mem::MemoryManager::with_mut(|mm| crate::message::process_ending(self, mm, target_pid));

        let process = self.get_process_mut(target_pid)?;
        process.activate()?;
        process.terminate()?;
        crate::mem::MemoryManager::with_mut(|mm| crate::message::destroy_quarantined_devices(self, mm));

        self.switch_to_thread(KERNEL_PID, None).unwrap();

        Ok(())
    }

    /// End process `target` on behalf of the running process (R10 kills the processes of a
    /// destroyed budget), which keeps running: the same teardown as `terminate_process`, then
    /// the running process's address space is active again. `target` must not be the running
    /// process.
    pub fn kill_process(&mut self, target: PID) -> Result<(), redoubt_abi::Error> {
        let current = self.current_pid();
        assert!(target != current, "kill_process on the running process");
        crate::mem::MemoryManager::with_mut(|mm| crate::message::process_ending(self, mm, target));
        // `terminate` needs no address space: it names the target's mapping itself.
        self.get_process_mut(target)?.terminate()?;
        crate::mem::MemoryManager::with_mut(|mm| crate::message::destroy_quarantined_devices(self, mm));
        self.get_process(current)?.activate()
    }

    /// Returns the process name, if any, of a given PID
    pub fn process_name(&self, pid: PID) -> Option<&str> {
        let args = crate::args::KernelArguments::get();
        for arg in args.iter() {
            if arg.name != u32::from_le_bytes(*b"PNam") {
                continue;
            }
            // SAFETY: `arg.data` is the tag's data, `arg.size` bytes (`arg.data.len()` words) of
            // the kernel argument block. Viewing those same bytes as `u8` keeps the length and
            // needs no more alignment than the words already have.
            let data = unsafe { core::slice::from_raw_parts(arg.data.as_ptr() as *const u8, arg.size) };
            // Each record is a PID word, a length word, then that many bytes, padded to a word.
            // A record that does not fit in the tag means a malformed block: stop reading it.
            let mut offset = 0;
            while offset + 8 <= data.len() {
                let check_pid =
                    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                let str_len = u32::from_le_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]) as usize;
                if str_len > data.len() - offset - 8 {
                    break;
                }
                if check_pid == pid.get() as _ {
                    if let Ok(s) = core::str::from_utf8(&data[offset + 8..offset + 8 + str_len]) {
                        return Some(s);
                    } else {
                        return None;
                    }
                }
                offset += str_len + 8;
                offset += (4 - (offset & 3)) & 3;
            }
        }
        None
    }
}
