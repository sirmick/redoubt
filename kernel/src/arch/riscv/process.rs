// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::convert::TryFrom;
use core::mem;
use core::sync::atomic::{AtomicUsize, Ordering};

/// The current process's header page lives at a fixed virtual address that the loader
/// maps, to a different physical page, in every address space. So this one address always
/// refers to whichever process is currently active.
const PROCESS: usize = redoubt_layout::PROCESS_AREA;

/// The kernel's view of a `T` at `addr`: only ever the running process's header at `PROCESS`
/// ([`process_impl`]) or a saved context ([`context`]): a thread's, or the hart's own for the
/// kernel's thread.
///
/// # Safety of the body
/// Both kinds of `addr` hold a live, aligned value of their type in every address space: the
/// header is mapped at `PROCESS` by the loader and by `MemoryMapping::allocate`, and a context
/// is either the hart's block's kernel area (`hart.rs`, kernel data) or the last
/// `size_of::<Thread>()` bytes of a thread's
/// IPC page, a kernel object frame that no process maps, reached through the physmap, which maps
/// all of RAM read-write in every address space (kernel/memory-layout.md). Every bit pattern is
/// valid for both types (integers only). The kernel runs on a single hart with interrupts
/// disabled, so these references never overlap in time and are unique while held. Callers must
/// not hold two of them across each other.
#[allow(clippy::mut_from_ref)]
fn kernel_ref<T>(addr: usize) -> &'static mut T {
    // SAFETY: see the function's doc comment.
    unsafe { &mut *(addr as *mut T) }
}

/// The current process's header page.
fn process_impl() -> &'static mut ProcessImpl { kernel_ref(PROCESS) }

/// The physical address of RAM's first frame, which object frame indices count from: set once
/// at boot (`MemoryManager::init_from_memory`), so that finding a thread's context never needs
/// the memory manager.
static RAM_START: AtomicUsize = AtomicUsize::new(0);

pub fn set_ram_start(start: usize) { RAM_START.store(start, Ordering::Relaxed) }

/// Where a thread's saved context lies in its IPC page: the page's last bytes, after every word
/// of IPC state (`message.rs` asserts that its words end before it).
pub const CONTEXT_OFFSET: usize = PAGE_SIZE - mem::size_of::<Thread>();

/// The address of thread `tid`'s context in the current process: in its IPC page, or, for the
/// kernel's own thread (PID 1 has no IPC pages), the hart's own area in its block.
fn context_addr(tid: TID) -> usize {
    match process_impl().ipc[tid] {
        0 => {
            assert_eq!(current_pid(), redoubt_layout::KERNEL_PID, "thread {} has no IPC page", tid);
            super::hart::kernel_context()
        }
        frame => {
            let phys = RAM_START.load(Ordering::Relaxed) + frame as usize * PAGE_SIZE;
            redoubt_layout::physmap_virt(phys) + CONTEXT_OFFSET
        }
    }
}

/// Thread `tid`'s saved context in the current process.
fn context(tid: TID) -> &'static mut Thread { kernel_ref(context_addr(tid)) }

/// A thread's number within its process, `1..=MAX_THREADS` (kernel/processes.md). Thread `tid`'s
/// saved context is at the end of its IPC page, whose frame is entry `tid` of the header's table
/// (entry 0 names no thread).
pub type TID = usize;

/// The first thread of every process.
pub const INITIAL_TID: TID = 1;

/// Words in a [`TidMask`].
const TID_WORDS: usize = 4;
// Every TID, 0 included, has a bit, and every TID fits `last_tid_allocated: u8`, the field a
// free-TID search starts from.
const _: () = assert!(MAX_THREADS < TID_WORDS * u64::BITS as usize && MAX_THREADS <= u8::MAX as usize);

/// A set of TIDs, bit `tid` for thread `tid` (bit 0, TID 0, names no thread).
pub type TidMask = crate::bits::Bits<TID_WORDS>;

use redoubt_layout::Pid;
use redoubt_sys::{MAX_THREADS, PAGE_SIZE};

use crate::cell::KernelCell;
use crate::ptable::ProcessInner;

pub const MAX_PROCESS_COUNT: usize = 511;

/// The width of `satp`'s ASID field: 9 bits in Sv32, 16 in Sv39 (the privileged spec).
#[cfg(target_pointer_width = "32")]
pub const ASID_BITS: u32 = 9;
#[cfg(target_pointer_width = "64")]
pub const ASID_BITS: u32 = 16;

// Every PID, 1..=MAX_PROCESS_COUNT, fits the ASID field, so a PID is its own ASID with no table
// between them (kernel/memory-layout.md, `satp`).
const _: () = assert!(MAX_PROCESS_COUNT < 1 << ASID_BITS);
const _: () = assert!(ASID_BITS == paging::SATP.asid_bits);

/// The boot's first step on `satp` (kernel/boot.md, "Hardware bounds"): a hart may implement
/// fewer ASID bits than the field has, so the kernel writes ones to the field and counts the bits
/// that stuck. One narrower than `ASID_BITS` is refused (R17); otherwise the kernel takes its own
/// ASID, PID 1.
pub fn check_asid_field() {
    match paging::SATP.asid_width(crate::arch::mem::read_back_asid_ones(), ASID_BITS) {
        Ok(width) => println!("asid: {} bits", width),
        Err(width) => {
            panic!("R17: the hart's satp ASID field holds {} bits; every PID needs {}", width, ASID_BITS)
        }
    }
    crate::arch::mem::enter_kernel_asid();
}

/// Base of a range of addresses that are never mapped. Jumping to one of them faults into
/// the kernel, which uses the faulting address to tell what the program is returning from.
#[cfg(target_pointer_width = "32")]
const MAGIC_RETURN_BASE: usize = 0xff80_0000;
#[cfg(target_pointer_width = "64")]
const MAGIC_RETURN_BASE: usize = redoubt_layout::PROCESS_AREA + 0x80_0000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = MAGIC_RETURN_BASE + 0x3000;

/// A process's header: one page at `PROCESS_AREA` on both widths (kernel/memory-layout.md,
/// "Per-process kernel data"). The threads' saved contexts are not here but in their IPC pages,
/// so a process pays for the threads it has, not for `MAX_THREADS`.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct ProcessImpl {
    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// Allocated contexts, independent of their untrusted program counters.
    allocated_threads: TidMask,

    /// The last thread ID that was allocated
    last_tid_allocated: u8,

    /// Each thread's IPC page, by object frame index, indexed by TID; 0 for none. The memory
    /// manager keeps it (`budget.rs`), reading another process's through the physmap.
    ipc: [u32; MAX_THREADS + 1],
}

const _: () = assert!(mem::size_of::<Thread>() == 32 * mem::size_of::<usize>());
// The header is one page, the one the loader and `MemoryMapping::allocate` map.
const _: () = assert!(mem::size_of::<ProcessImpl>() <= PAGE_SIZE);

/// Where the header's TID -> IPC-frame table lies in its page (`budget.rs`).
pub const IPC_TABLE_OFFSET: usize = mem::offset_of!(ProcessImpl, ipc);

/// Which PIDs have an address space the hardware may switch to. Which one each hart runs is in
/// its block (`hart.rs`). The process table proper is `ptable::ProcessTable`; this is the arch
/// layer's view of it.
struct PidSlots {
    /// The actual table contents. `true` if a process is allocated,
    /// `false` if it is free.
    table: [bool; MAX_PROCESS_COUNT],
}

static PID_SLOTS: KernelCell<PidSlots> = KernelCell::new(PidSlots { table: [false; MAX_PROCESS_COUNT] });

#[repr(C)]
#[derive(Debug, Copy, Clone)]
/// The loader sets up two processes, the kernel and then `init`, and reports each as one of
/// these records (kernel/boot.md, "What the loader does").
pub struct InitialProcess {
    /// The process's PID, in a field of its own: `satp` carries none.
    pub pid: usize,

    /// The RISC-V `satp` value: the root page table, with ASID 0.
    pub satp: usize,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,

    /// The first thread's `a0` and `a1`: for `init`, the bundle's address and length.
    pub a0: usize,
    pub a1: usize,
}

impl InitialProcess {
    pub fn pid(&self) -> Pid {
        // The loader wrote this value. Check it rather than trust it: a zero or a value too wide
        // for a PID stops the boot.
        crate::budget::pid_from(self.pid).expect("initial process has no valid PID")
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Process {
    pid: Pid,
}

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
    pub fn current() -> Process { Process { pid: current_pid() } }

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
        let tid = self.current_tid();
        self.thread_mut(tid)
    }

    pub fn current_thread(&self) -> &Thread {
        let tid = self.current_tid();
        assert!(valid_tid(tid), "no current thread");
        kernel_ref(super::hart::context())
    }

    /// The thread this hart runs.
    pub fn current_tid(&self) -> TID { super::hart::tid() }

    pub fn thread_exists(&self, tid: TID) -> bool {
        valid_tid(tid) && process_impl().allocated_threads.contains(tid)
    }

    /// Set this hart's thread, and the context the trap handler saves into.
    pub fn set_tid(&mut self, tid: TID) {
        klog!("Switching to thread {}", tid);
        assert!(valid_tid(tid), "attempt to switch to an invalid thread {}", tid);
        super::hart::set_thread(tid, context_addr(tid));
    }

    pub fn thread_mut(&mut self, tid: TID) -> &mut Thread {
        assert!(valid_tid(tid), "attempt to retrieve an invalid thread {}", tid);
        context(tid)
    }

    /// A free TID, searching round from the last one handed out; `None` once `MAX_THREADS`
    /// threads exist (the initial thread counts; kernel/processes.md).
    pub fn find_free_thread(&self) -> Option<TID> {
        let process = process_impl();
        let start = process.last_tid_allocated as usize;
        for offset in 0..MAX_THREADS {
            let tid = (start + offset) % MAX_THREADS + 1;
            if !process.allocated_threads.contains(tid) {
                process.last_tid_allocated = u8::try_from(tid).expect("a TID fits a byte");
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

    /// The first run of `init`, the one process the loader starts, in its own address space:
    /// claim its slot, reset its contexts, and start its first thread at `entry` with stack
    /// pointer `sp` and `a0`/`a1` (the bundle's address and length, kernel/boot.md). Its stack is
    /// the loader's to reserve, and only the loader's (kernel/memory-layout.md, "Regions").
    pub fn setup_loader_process(pid: Pid, entry: usize, sp: usize, a0: usize, a1: usize) {
        Self::claim(pid);
        // Its space is active already (`switch_to`); the PID is the kernel's to record.
        set_current_pid(pid);
        Self::setup_empty_process(pid);
        Self::setup_first_thread(pid, entry, sp, a0);
        context(INITIAL_TID).registers[10] = a1;
    }

    /// Claim `pid` in the process table, so that its address space can be activated. It is a
    /// separate step from `setup_empty_process`, which needs that space to be active already:
    /// `set_current_pid` refuses a PID the table does not hold.
    pub fn claim(pid: Pid) {
        let pid_idx = (pid.get() as usize) - 1;
        PID_SLOTS.with(|pt| {
            assert!(!pt.table[pid_idx], "process {} is already allocated", pid);
            pt.table[pid_idx] = true;
        });
    }

    /// Initialize the context storage of the address space `process_create` just allocated. It has no thread
    /// yet: nothing can run in it until `process_start`.
    ///
    /// The process's own address space must be the active one, as `setup_first_thread` requires, so
    /// that `process_impl()` names *its* header. `MemoryMapping::allocate` (or the loader) zeroed
    /// that page; this gives the header and `ProcessInner` their starting values. The IPC-frame
    /// table is the memory manager's and is left alone: `init`'s first thread has its page before
    /// `init` first runs.
    pub fn setup_empty_process(pid: Pid) {
        assert_eq!(pid, crate::arch::current_pid(), "hardware pid does not match setup pid");
        let process = process_impl();
        process.allocated_threads = TidMask::EMPTY;
        process.last_tid_allocated = u8::try_from(INITIAL_TID).expect("a TID fits a byte");
        process.inner = Default::default();
    }

    /// The first thread of a process `process_start` is starting, at `entry` with stack pointer
    /// `sp` and one argument. Unlike `setup_loader_process` this reserves no stack: a Redoubt
    /// process is given every page it has by its parent (`process_map`), so `sp` is an address the
    /// parent has already mapped and the kernel only loads it.
    ///
    /// The process's own address space must be the active one.
    pub fn setup_first_thread(pid: Pid, entry: usize, sp: usize, arg: usize) {
        assert_eq!(pid, crate::arch::current_pid(), "hardware pid does not match setup pid");
        {
            let process = process_impl();
            process.allocated_threads = process.allocated_threads.with(INITIAL_TID);
        }
        let thread = context(INITIAL_TID);
        *thread = Default::default();
        thread.sepc = entry;
        thread.registers[1] = sp;
        thread.registers[9] = arg;
    }

    /// `thread_create(entry, sp, arg)`. The caller has mapped its own stack and passes `sp`.
    pub fn setup_redoubt_thread(&mut self, new_tid: TID, entry: usize, sp: usize, arg: usize) {
        assert!(valid_tid(new_tid), "attempt to create an invalid thread {}", new_tid);
        let thread = context(new_tid);
        *thread = Default::default();
        thread.sepc = entry;
        thread.registers[0] = EXIT_THREAD;
        thread.registers[1] = sp;
        thread.registers[9] = arg;
        let process = process_impl();
        process.allocated_threads = process.allocated_threads.with(new_tid);
    }

    /// Destroy a given thread: `false` if it did not exist.
    pub fn destroy_thread(&mut self, tid: TID) -> bool {
        // Ensure this thread is allocated, regardless of the PC it was given.
        if !self.thread_exists(tid) {
            return false;
        }

        let thread = self.thread_mut(tid);
        for val in &mut thread.registers {
            *val = 0;
        }
        thread.sepc = 0;
        let process = process_impl();
        process.allocated_threads = process.allocated_threads.without(tid);
        // Its IPC page goes back next (`thread_ended`): a trap must never save into it after.
        if super::hart::tid() == tid {
            super::hart::set_context(super::hart::kernel_context());
        }
        true
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

    pub fn destroy(pid: Pid) {
        let pid_idx = pid.get() as usize - 1;
        PID_SLOTS.with(|pt| {
            if pid_idx >= pt.table.len() {
                panic!("attempted to destroy PID that exceeds table index: {}", pid);
            }
            pt.table[pid_idx] = false;
        });
    }
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

/// Whether `tid` names a thread slot: `1..=MAX_THREADS`.
fn valid_tid(tid: TID) -> bool { (1..=MAX_THREADS).contains(&tid) }

/// This hart runs `pid` from now on (kernel/memory-layout.md, "`satp`").
pub fn set_current_pid(pid: Pid) {
    let pid_idx = usize::from(pid.get()) - 1;
    PID_SLOTS.with(|pt| match pt.table.get(pid_idx) {
        None | Some(false) => panic!("PID {} does not exist", pid),
        _ => (),
    });
    super::hart::set_pid(pid);
}

/// The PID this hart runs: the kernel until its first switch.
pub fn current_pid() -> Pid { super::hart::pid() }
