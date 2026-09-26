// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::mem;
/// The current process's bookkeeping lives at a fixed virtual address that the loader
/// maps, to a different physical page, in every address space. So this one pointer always
/// refers to whichever process is currently active.
const PROCESS: *mut ProcessImpl = redoubt_abi::arch::THREAD_CONTEXT_AREA as *mut ProcessImpl;

/// The current process's `ProcessImpl`.
///
/// # Safety of the body
/// `PROCESS` points at a live, aligned `ProcessImpl` (mapped by the loader and by
/// `MemoryMapping::allocate` in every address space). The kernel runs on a single hart
/// with interrupts disabled, so these references never overlap in time and are unique
/// while held. Callers must not hold two of them across each other.
#[allow(clippy::mut_from_ref)]
fn process_impl() -> &'static mut ProcessImpl {
    // SAFETY: see the function's doc comment.
    unsafe { &mut *PROCESS }
}
/// A thread's number within its process: an index into its saved contexts.
pub type TID = usize;

pub const MAX_THREAD: TID = 31;
pub const EXCEPTION_TID: TID = 1;
pub const INITIAL_TID: TID = 2;
pub const IRQ_TID: TID = 0;

use redoubt_sys::PAGE_SIZE;
use redoubt_abi::PID;

use crate::cell::KernelCell;
use crate::services::ProcessInner;

// use crate::args::KernelArguments;
pub const DEFAULT_STACK_SIZE: usize = 128 * 1024;
pub const MAX_PROCESS_COUNT: usize = 64;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// Base of a range of addresses that are never mapped. Jumping to one of them faults into
/// the kernel, which uses the faulting address to tell what the program is returning from.
#[cfg(target_pointer_width = "32")]
const MAGIC_RETURN_BASE: usize = 0xff80_0000;
#[cfg(target_pointer_width = "64")]
const MAGIC_RETURN_BASE: usize = redoubt_abi::arch::PROCESS_AREA + 0x80_0000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = MAGIC_RETURN_BASE + 0x3000;

/// Support processing interrupts, which normally are TID 0. Since
/// the TID is a NonZeroU8, we must pick a value here that can be
/// used throughout the rest of the kernel.
const IRQ_TID_SENTINAL: TID = 255;

// Thread IDs have three possible meaning:
// Logical Thread ID: What the user sees
// Thread Context Index: An index into the thread slice
// Hardware Thread ID: The index that the ISR uses
//
// The Hardware Thread ID is always equal to the Thread Context
// Index, minus one. For example, the default thread ID is
// Hardware Thread ID 1 is Thread Context Index 0.
// The Logical Thread ID is equal to the Hardware Thread ID
// plus one again. This is because the ISR context is Thread
// Context Index 0.
// Therefore, the first Logical Thread ID is 1, which maps
// to Hardware Thread ID 2, which is Thread Context Index 1.
//
// +-----------------+-----------------+-----------------+
// |    Thread ID    |  Context Index  | Hardware Thread |
// +=================+=================+=================+
// |   ISR Context   |        0        |        1        |
// |        1        |        1        |        2        |
// |        2        |        2        |        3        |

// ProcessImpl occupies a multiple of pages mapped to virtual address `0xff80_1000`.
// Each thread is 128 bytes (32 4-byte registers). The first "thread" does not exist,
// and instead is any bookkeeping information related to the process.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct ProcessImpl {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The currently-active thread for this process. This must
    /// be the 2nd item, because the ISR directly writes this value.
    hardware_thread: usize,

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// Allocated contexts, independent of their untrusted program counters.
    allocated_threads: u32,

    /// The last thread ID that was allocated
    last_tid_allocated: u8,

    /// Pad the header out to the size of one `Thread`, so that the header is
    /// "context 0" and the ISR can find context N at `N * size_of::<Thread>()`.
    _padding: [u8; HEADER_PADDING],

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    threads: [Thread; MAX_THREAD],
}

const HEADER_PADDING: usize =
    mem::size_of::<Thread>() - (2 * mem::size_of::<usize>() + mem::size_of::<ProcessInner>() + 4 + 1);

/// Number of pages `ProcessImpl` occupies at `THREAD_CONTEXT_AREA`: 1 on rv32, 2 on rv64.
#[allow(dead_code)] // used by the loader handoff on rv64
pub const PROCESS_IMPL_PAGES: usize = mem::size_of::<ProcessImpl>() / PAGE_SIZE;

// The trap handler in asm indexes contexts as `THREAD_CONTEXT_AREA + (n << log2(size_of::<Thread>()))`.
const _: () = assert!(mem::size_of::<Thread>() == 32 * mem::size_of::<usize>());
const _: () = assert!(mem::size_of::<ProcessImpl>() == (MAX_THREAD + 1) * mem::size_of::<Thread>());
const _: () = assert!(mem::size_of::<ProcessImpl>() % PAGE_SIZE == 0);
// The loader maps this many pages for PID 1 and for every initial process.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(PROCESS_IMPL_PAGES == redoubt_abi::arch::THREAD_CONTEXT_PAGES);

/// Singleton process table. Each process in the system gets allocated from this table.
struct ProcessTable {
    /// The process upon which the current syscall is operating
    current: PID,

    /// The actual table contents. `true` if a process is allocated,
    /// `false` if it is free.
    table: [bool; MAX_PROCESS_COUNT],
}

static PROCESS_TABLE: KernelCell<ProcessTable> =
    KernelCell::new(ProcessTable { current: crate::services::KERNEL_PID, table: [false; MAX_PROCESS_COUNT] });

#[repr(C)]
#[derive(Debug, Copy, Clone)]
/// The stage1 bootloader sets up some initial processes.  These are reported
/// to us as (satp, entrypoint, sp) tuples, which can be turned into a structure.
/// The first element is always the kernel.
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    pub satp: usize,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,
}

impl InitialProcess {
    pub fn pid(&self) -> PID {
        let pid = crate::arch::mem::pid_from_satp(self.satp);
        // The loader wrote this value. Check it rather than trust it: a zero here would be
        // undefined behaviour in a `NonZeroU8`.
        PID::new(pid as u8).expect("initial process has PID 0")
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Process {
    pid: PID,
}

fn fixup_irq(tid: TID) -> TID { if tid == IRQ_TID_SENTINAL { 0 } else { tid } }

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct Thread {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this thread was created because of an
    /// `ecall` instruction, you will need to add `4` to this before returning,
    /// to prevent that instruction from getting executed again. If this is 0,
    /// then this thread is not valid.
    pub sepc: usize,
}

impl Process {
    pub fn current() -> Process {
        let pid = PROCESS_TABLE.with(|pt| pt.current);
        let hardware_pid = crate::arch::mem::pid_from_satp(riscv::register::satp::read().bits());
        assert_eq!((pid.get() as usize), hardware_pid);
        Process { pid }
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&Process) -> R,
    {
        let process = Self::current();
        f(&process)
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_current_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Process) -> R,
    {
        let mut process = Self::current();
        f(&mut process)
    }

    pub fn with_inner_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessInner) -> R,
    {
        let process = process_impl();
        f(&mut process.inner)
    }

    pub fn current_thread_mut(&mut self) -> &mut Thread {
        let process = process_impl();
        assert!(process.hardware_thread != 0, "thread number was 0");
        &mut process.threads[process.hardware_thread - 1]
    }

    pub fn current_thread(&self) -> &Thread {
        let process = process_impl();
        &mut process.threads[process.hardware_thread - 1]
        // self.thread(process.hardware_thread - 1)
    }

    pub fn current_tid(&self) -> TID {
        let process = process_impl();
        process.hardware_thread - 1
    }

    pub fn thread_exists(&self, tid: TID) -> bool {
        let tid = fixup_irq(tid);
        tid < MAX_THREAD && process_impl().allocated_threads & (1 << tid) != 0
    }

    /// Set the current thread number.
    pub fn set_tid(&mut self, tid: TID) {
        let process = process_impl();
        let tid = fixup_irq(tid);
        klog!("Switching to thread {}", tid);
        assert!(tid < process.threads.len(), "attempt to switch to an invalid thread {}", tid);
        process.hardware_thread = tid + 1;
        if tid == IRQ_TID || tid == EXCEPTION_TID {
            process.allocated_threads |= 1 << tid;
        }
    }

    pub fn thread_mut(&mut self, tid: TID) -> &mut Thread {
        let process = process_impl();
        let tid = fixup_irq(tid);
        assert!(tid < process.threads.len(), "attempt to retrieve an invalid thread {}", tid);
        &mut process.threads[tid]
    }

    pub fn find_free_thread(&self) -> Option<TID> {
        let process = process_impl();
        let start = process.last_tid_allocated as usize;
        for offset in 0..MAX_THREAD {
            let tid = (start + offset) % MAX_THREAD;
            if tid != IRQ_TID && tid != EXCEPTION_TID && process.allocated_threads & (1 << tid) == 0 {
                process.last_tid_allocated = tid as u8;
                return Some(tid);
            }
        }
        None
    }

    /// The Redoubt result registers `a0..=a7` of a thread that was waiting (`redoubt-sys`
    /// encodes them).
    pub fn set_thread_registers(&mut self, thread_nr: TID, regs: &[usize; 8]) {
        let thread = self.thread_mut(thread_nr);
        for (src, dest) in regs.iter().zip(thread.registers[9..].iter_mut()) {
            *dest = *src;
        }
    }

    /// The first run of a loader-bundle program (INTERIM, until R3's `init` launches them), in
    /// its own address space: claim its slot, reset its contexts, start its first thread at
    /// `entry` with stack pointer `sp`, and reserve its stack for demand paging (OD6).
    pub fn setup_loader_process(pid: PID, entry: usize, sp: usize) {
        Self::claim(pid);
        Self::setup_empty_process(pid);
        Self::setup_first_thread(pid, entry, sp, 0);
        let stack = (sp - DEFAULT_STACK_SIZE) & !(PAGE_SIZE - 1);
        crate::mem::MemoryManager::with_mut(|mm| {
            mm.reserve_range(
                stack as *mut u8,
                DEFAULT_STACK_SIZE,
                redoubt_sys::MemFlags::READ | redoubt_sys::MemFlags::WRITE,
            )
            .expect("couldn't reserve stack")
        });
    }

    /// WP-K4: claim `pid` in the process table, so that its address space can be activated. It
    /// is a separate step from `setup_empty_process`, which needs that space to be active
    /// already: `set_current_pid` refuses a PID the table does not hold.
    pub fn claim(pid: PID) {
        let pid_idx = (pid.get() as usize) - 1;
        PROCESS_TABLE.with(|pt| {
            assert!(!pt.table[pid_idx], "process {} is already allocated", pid);
            pt.table[pid_idx] = true;
        });
    }

    /// Initialize the context storage of the address space `process_create` just allocated. It has no thread
    /// yet: nothing can run in it until `process_start`.
    ///
    /// The process's own address space must be the active one, as `setup_first_thread` requires, so
    /// that `process_impl()` names *its* saved contexts. `MemoryMapping::allocate` zeroed those
    /// frames, and all-zeroes is not a valid `ProcessInner` (its `pid` is a `NonZeroU8`), so
    /// nothing may read them before this runs.
    pub fn setup_empty_process(pid: PID) {
        let process = process_impl();
        assert_eq!(pid, crate::arch::current_pid(), "hardware pid does not match setup pid");
        // By convention thread 0 is the trap thread, so the first ordinary thread is
        // `INITIAL_TID`; the hardware thread number is one more than the TID.
        process.hardware_thread = INITIAL_TID + 1;
        process.allocated_threads = 0;
        process.last_tid_allocated = INITIAL_TID as u8;
        for thread in process.threads.iter_mut() {
            *thread = Default::default();
        }
        process.inner = Default::default();
        process.inner.pid = pid;
    }

    /// WP-K4: the first thread of a process `process_start` is starting, at `entry` with stack
    /// pointer `sp` and one argument. Unlike `setup_loader_process` this reserves no stack: a Redoubt
    /// process is given every page it has by its parent (`process_map`), so `sp` is an address
    /// the parent has already mapped and the kernel only loads it.
    ///
    /// The process's own address space must be the active one.
    pub fn setup_first_thread(pid: PID, entry: usize, sp: usize, arg: usize) {
        let process = process_impl();
        assert_eq!(pid, crate::arch::current_pid(), "hardware pid does not match setup pid");
        process.allocated_threads |= 1 << INITIAL_TID;
        let thread = &mut process.threads[INITIAL_TID];
        *thread = Default::default();
        thread.sepc = entry;
        thread.registers[1] = sp;
        thread.registers[9] = arg;
    }

    /// WP-K4: `thread_create(entry, sp, arg)`. The caller has mapped its own stack and passes
    /// `sp`.
    pub fn setup_redoubt_thread(&mut self, new_tid: TID, entry: usize, sp: usize, arg: usize) {
        let process = process_impl();
        let thread = &mut process.threads[new_tid];
        *thread = Default::default();
        thread.sepc = entry;
        thread.registers[0] = EXIT_THREAD;
        thread.registers[1] = sp;
        thread.registers[9] = arg;
        process.allocated_threads |= 1 << new_tid;
    }

    /// Destroy a given thread: `false` if it did not exist.
    pub fn destroy_thread(&mut self, tid: TID) -> bool {
        // Ensure this thread is allocated, regardless of the PC it was given.
        if !self.thread_exists(tid) || tid == IRQ_TID {
            return false;
        }

        let thread = self.thread_mut(tid);
        for val in &mut thread.registers {
            *val = 0;
        }
        thread.sepc = 0;
        process_impl().allocated_threads &= !(1 << tid);
        true
    }

    pub fn print_all_threads(&self) {
        let process = process_impl();
        for (tid_idx, &thread) in process.threads.iter().enumerate() {
            let tid = tid_idx;
            if thread.registers[1] != 0 {
                Self::print_thread(tid, &thread);
            }
        }
    }

    pub fn print_current_thread(&self) {
        let thread = self.current_thread();
        let tid = self.current_tid();
        Self::print_thread(tid, thread);
    }

    pub fn print_thread(_tid: TID, _thread: &Thread) {
        println!("Thread {}:", _tid);
        print!("{}", _thread);
    }

    pub fn destroy(pid: PID) {
        let pid_idx = pid.get() as usize - 1;
        PROCESS_TABLE.with(|pt| {
            if pid_idx >= pt.table.len() {
                panic!("attempted to destroy PID that exceeds table index: {}", pid);
            }
            pt.table[pid_idx] = false;
        });
    }

    /// This is used by debugging routines to sanity check state, which are typically #[cfg]'d out
    /// but with complicated overlapping rules that constantly change. Hence, the #[allow(dead_code)].
    #[allow(dead_code)]
    pub fn pid(&self) -> PID { self.pid }
}

impl core::fmt::Display for Thread {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "PC:{:08x}   SP:{:08x}   RA:{:08x}", self.sepc, self.registers[1], self.registers[0])?;
        writeln!(f, "GP:{:08x}   TP:{:08x}", self.registers[2], self.registers[3])?;
        writeln!(
            f,
            "T0:{:08x}   T1:{:08x}   T2:{:08x}",
            self.registers[4], self.registers[5], self.registers[6]
        )?;
        writeln!(
            f,
            "T3:{:08x}   T4:{:08x}   T5:{:08x}   T6:{:08x}",
            self.registers[27], self.registers[28], self.registers[29], self.registers[30]
        )?;
        writeln!(
            f,
            "S0:{:08x}   S1:{:08x}   S2:{:08x}   S3:{:08x}",
            self.registers[7], self.registers[8], self.registers[17], self.registers[18]
        )?;
        writeln!(
            f,
            "S4:{:08x}   S5:{:08x}   S6:{:08x}   S7:{:08x}",
            self.registers[19], self.registers[20], self.registers[21], self.registers[22]
        )?;
        writeln!(
            f,
            "S8:{:08x}   S9:{:08x}  S10:{:08x}  S11:{:08x}",
            self.registers[23], self.registers[24], self.registers[25], self.registers[26]
        )?;
        writeln!(
            f,
            "A0:{:08x}   A1:{:08x}   A2:{:08x}   A3:{:08x}",
            self.registers[9], self.registers[10], self.registers[11], self.registers[12]
        )?;
        writeln!(
            f,
            "A4:{:08x}   A5:{:08x}   A6:{:08x}   A7:{:08x}",
            self.registers[13], self.registers[14], self.registers[15], self.registers[16]
        )?;
        Ok(())
    }
}

pub fn set_current_pid(pid: PID) {
    let pid_idx = (pid.get() - 1) as usize;
    PROCESS_TABLE.with(|pt| {
        match pt.table.get(pid_idx) {
            None | Some(false) => panic!("PID {} does not exist", pid),
            _ => (),
        }
        pt.current = pid;
    });
}

pub fn current_pid() -> PID { PROCESS_TABLE.with(|pt| pt.current) }
