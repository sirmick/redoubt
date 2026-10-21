//! Typed handles and the system calls that are not IPC (kernel/abi.md). IPC is in
//! [`crate::ipc`].
//!
//! A typed handle only says what the program expects the handle to be: the kernel checks the
//! object's kind on every use (`WrongObject`), so wrapping the wrong kind is a refused call, never
//! a confusion. Handles are not closed on drop: a server keeps them in tables and hands out
//! copies, and an implicit close would be an easy use-after-close. Close them with `close`.

use core::sync::atomic::{AtomicBool, Ordering};

use redoubt_sys::{
    BUDGET_SPEC_SLOTS, BudgetSpec, Call, DeviceInfo, Error, Handle, Hold, MAX_RANGE_PAGES, MAX_START_HANDLES,
    MemFlags, PAGE_SIZE, ResetKind, Return, USAGE_SLOTS, Usage, range_chunks,
};

use crate::ipc::Buffer;
use crate::sys::{Record, syscall};

macro_rules! typed_handles {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Debug, PartialEq, Eq)]
        pub struct $name(Handle);

        impl $name {
            /// Wraps a handle the program expects to be of this kind.
            pub const fn from_handle(handle: Handle) -> Self { $name(handle) }

            pub const fn handle(&self) -> Handle { self.0 }

            /// Closes the handle (`handle_close`).
            pub fn close(self) -> Result<(), Error> { close(self.0) }
        }
    )*};
}

typed_handles! {
    /// An endpoint: badge 0 is the receive right; any other badge can only call and send.
    Endpoint;
    Budget;
    Process;
    /// An MMIO device.
    Mmio;
    /// An interrupt.
    Irq;
    /// The right to power off or reboot.
    Reset;
}

/// `handle_close` on any handle.
pub fn close(handle: Handle) -> Result<(), Error> { nothing(syscall(&Call::HandleClose { handle })) }

pub(crate) fn nothing(result: Result<Return, Error>) -> Result<(), Error> {
    match result? {
        Return::Nothing => Ok(()),
        // redoubt-sys decodes a result by the call's number, so another shape cannot arrive.
        _ => Err(Error::InvalidArgument),
    }
}

pub(crate) fn handle(result: Result<Return, Error>) -> Result<Handle, Error> {
    match result? {
        Return::Handle(handle) => Ok(handle),
        _ => Err(Error::InvalidArgument),
    }
}

fn addr(result: Result<Return, Error>) -> Result<usize, Error> {
    match result? {
        Return::Addr(addr) => Ok(addr),
        _ => Err(Error::InvalidArgument),
    }
}

/// The bytes one range call may name: `MAX_RANGE_PAGES` pages (kernel/memory.md, R22). A longer
/// range is the runtime's to split, a range at a time ([`range_chunks`]).
const RANGE: usize = MAX_RANGE_PAGES * PAGE_SIZE;

/// Maps `len` bytes (whole pages) of zeroed memory; returns its address. One run, wherever its
/// length: the kernel's `map_anon` places a run of `len` where all of it is free, by its one
/// search, and maps its first range ([`RANGE`]); the ranges after it are mapped by `map_fixed`
/// (kernel/memory.md, R22). Nothing holds them free between the two: a page taken there meanwhile,
/// by a placement this lock does not cover (`map_device`, `dma_alloc`, a lend landing from another
/// thread), refuses the extension, which gives back what it mapped and asks again, the kernel's
/// next search starting where it placed the refused run, so it passes that page. The search ends
/// as the kernel's one pass does: once the runs placed have wrapped (one below the one before), one
/// at or past the first's means no gap holds the run, `OutOfMemory`, with nothing left mapped. A
/// refusal needs a placement of the process's own that the lock does not cover, so repeated
/// re-searches, and an `OutOfMemory` though a gap held the run before them, are the process's own
/// doing, and the one pass bounds them.
///
/// The process's every `map_anon` through the runtime takes one lock ([`Placing`]), the heap's and
/// a [`crate::ipc::Buffer`]'s alike, so no other placement of the runtime's lands in a run while it
/// is extended; a program on the runtime makes no `map_anon` of its own.
pub fn map_anon(len: usize, flags: MemFlags) -> Result<usize, Error> {
    let _placing = Placing::take();
    let (mut origin, mut last, mut wrapped) = (None, 0, false);
    loop {
        let first = addr(syscall(&Call::MapAnon { len, flags }))?;
        if len <= RANGE {
            return Ok(first);
        }
        wrapped |= origin.is_some() && first < last;
        if wrapped && origin.is_some_and(|o| first >= o) {
            let _ = unmap(first, RANGE);
            return Err(Error::OutOfMemory);
        }
        origin.get_or_insert(first);
        last = first;
        match map_fixed(first + RANGE, len - RANGE, flags) {
            Ok(()) => return Ok(first),
            Err(e) => {
                let _ = unmap(first, RANGE);
                if e != Error::InvalidArgument {
                    return Err(e);
                }
            }
        }
    }
}

/// The lock every [`map_anon`] of the process holds for its whole placement.
struct Placing;

static PLACING: AtomicBool = AtomicBool::new(false);

impl Placing {
    fn take() -> Placing {
        while PLACING.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        Placing
    }
}

impl Drop for Placing {
    fn drop(&mut self) { PLACING.store(false, Ordering::Release); }
}

/// Maps `len` bytes (whole pages) of zeroed memory at exactly `addr`, which the kernel refuses
/// (`InvalidArgument`, nothing mapped) if any page of the range is taken (kernel/memory.md,
/// `map_fixed`). A range at a time ([`range_chunks`]): a refused range unmaps those before it, so
/// the call maps all of `len` or none of it. The runtime's own: [`crate::thread::spawn`] calls it
/// to place a thread's stack above its guard page, and [`map_anon`] to extend a run, and the pages
/// are adopted by a [`Buffer`] at once. Public, it would let a program fill the thread-stack area
/// under the runtime's placement.
pub(crate) fn map_fixed(addr: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    for (at, n) in range_chunks(addr, len) {
        if let Err(e) = nothing(syscall(&Call::MapFixed { addr: at, len: n, flags })) {
            if at > addr {
                let _ = unmap(addr, at - addr);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// Unmaps `len` bytes at `addr`. The runtime's own: only an owner of the pages (the heap, a
/// [`crate::ipc::Buffer`]) calls it, on pages it holds and no view borrows. Public, it would let
/// safe code unmap a page the heap or a lend still owns, and the owner's next access would fault
/// or read reused memory (userland/native.md, "redoubt-rt"):
///
/// ```compile_fail
/// redoubt_rt::handle::unmap(0x1000, 4096);
/// ```
///
/// Changing a mapping's flags is not offered at all, for the same reason: made read-only under a
/// `Buffer` or a lend, a page faults its owner's next write.
///
/// ```compile_fail
/// redoubt_rt::handle::set_flags(0x1000, 4096, redoubt_rt::abi::MemFlags::READ);
/// ```
///
/// A range at a time ([`range_chunks`]): a refused range stops the call, those before it
/// unmapped. Its callers unmap only what they hold whole, so a refusal means the pages were
/// already gone.
pub(crate) fn unmap(addr: usize, len: usize) -> Result<(), Error> {
    range_chunks(addr, len).try_for_each(|(at, n)| nothing(syscall(&Call::Unmap { addr: at, len: n })))
}

/// Starts a thread at `entry` with `arg` in its first argument register, on `stack`, which it
/// takes for good: the thread's stack grows down from the top of the pages, and they are never
/// unmapped, even after it exits, so no owner can hand them out underneath it. If the kernel
/// refuses the thread, the stack is dropped, but not on `InvalidArgument`
/// ([`spawn`](crate::thread::spawn) says why). The entry is a function and the stack a
/// [`Buffer`], so there is no raw form to reach:
///
/// ```compile_fail
/// redoubt_rt::handle::thread_create(0x1000, 0x2000, 0);
/// ```
pub fn thread_create(entry: extern "C" fn(usize) -> !, stack: Buffer, arg: usize) -> Result<u32, Error> {
    // The top of page-aligned pages is aligned for any call frame.
    let sp = stack.as_ptr() as usize + stack.len();
    match syscall(&Call::ThreadCreate { entry: entry as usize, sp, arg }) {
        Err(e) if e != Error::InvalidArgument => Err(e),
        Ok(Return::Tid(tid)) => {
            stack.into_pages();
            Ok(tid)
        }
        _ => {
            stack.into_pages();
            Err(Error::InvalidArgument)
        }
    }
}

pub fn thread_exit() -> ! {
    loop {
        // Only returns if the kernel refused, which it cannot for this call; asking again is
        // the only safe thing to do.
        let _ = syscall(&Call::ThreadExit);
    }
}

pub fn process_exit(code: u32) -> ! {
    loop {
        let _ = syscall(&Call::ProcessExit { code });
    }
}

/// Microseconds since boot.
pub fn time_now() -> Result<u64, Error> {
    match syscall(&Call::TimeNow)? {
        Return::Time(time) => Ok(time),
        _ => Err(Error::InvalidArgument),
    }
}

/// One `u64` from the kernel's CSPRNG.
pub fn random_u64() -> Result<u64, Error> {
    match syscall(&Call::Random)? {
        Return::Random(value) => Ok(value),
        _ => Err(Error::InvalidArgument),
    }
}

/// Fills `bytes` from the kernel's CSPRNG, eight bytes per call.
pub fn random(bytes: &mut [u8]) -> Result<(), Error> {
    for chunk in bytes.chunks_mut(8) {
        let value = random_u64()?.to_le_bytes();
        chunk.copy_from_slice(&value[..chunk.len()]);
    }
    Ok(())
}

/// Sleeps for `timeout` microseconds: `receive` from nothing, which can only time out.
///
/// `sleep(0)` is the runtime's yield: it enters the kernel and returns at once. Redoubt's kernel
/// runs no other thread first: a deadline already past times out without blocking, and the caller
/// resumes without the scheduler picking again. A backend that switches threads cooperatively
/// switches there.
pub fn sleep(timeout: u64) -> Result<(), Error> {
    match crate::ipc::receive_raw(None, timeout, 0) {
        Err(Error::Timeout) => Ok(()),
        Err(e) => Err(e),
        // Nothing can be received from nothing.
        Ok(_) => Err(Error::InvalidArgument),
    }
}

impl Budget {
    /// A child budget (`budget_create`).
    pub fn create_child(&self, spec: &BudgetSpec) -> Result<Budget, Error> {
        let rec = Record::<BUDGET_SPEC_SLOTS>(spec.encode());
        handle(syscall(&Call::BudgetCreate { parent: self.0, spec_rec: rec.addr() })).map(Budget)
    }

    /// Destroys the budget and everything charged to it (R10), then closes this handle: the
    /// destruction swept it, and a swept handle stays in the table, dead, until its holder
    /// closes it (I17). A handle that was dead already is closed too; one the call refused for
    /// any other reason is kept.
    pub fn destroy(self) -> Result<(), Error> {
        let destroyed = self.destroy_keeping();
        if matches!(destroyed, Ok(()) | Err(Error::Dead)) {
            let _ = close(self.0);
        }
        destroyed
    }

    /// As [`Budget::destroy`], but the handle stays, dead, for the record that owns it to close:
    /// for a caller that does not own the handle it destroys by.
    pub fn destroy_keeping(&self) -> Result<(), Error> {
        nothing(syscall(&Call::BudgetDestroy { budget: self.0 }))
    }

    /// Destroys the budget's first child and everything below it, keeps the budget, and returns
    /// how many children it still has (`budget_reap`, R10).
    pub fn reap(&self) -> Result<u32, Error> {
        match syscall(&Call::BudgetReap { budget: self.0 })? {
            Return::Remaining(n) => Ok(n),
            _ => Err(Error::InvalidArgument),
        }
    }

    pub fn usage(&self) -> Result<Usage, Error> {
        let mut rec = Record([0; USAGE_SLOTS]);
        nothing(syscall(&Call::BudgetUsage { budget: self.0, usage_rec: rec.addr_mut() }))?;
        Usage::decode(&rec.0)
    }
}

impl Process {
    /// An empty process in `budget`, whose exit notice goes to `exit_endpoint`.
    pub fn create(budget: &Budget, exit_endpoint: &Endpoint) -> Result<Process, Error> {
        let call = Call::ProcessCreate { budget: budget.handle(), exit_endpoint: exit_endpoint.handle() };
        handle(syscall(&call)).map(Process)
    }

    /// Moves `pages` to `dst` in the (unstarted) process: they are the child's from then on. It
    /// takes the pages by their owner, so no other view of them is left behind. A range at a time
    /// ([`range_chunks`]): refused, the pages not yet moved are unmapped here, and those moved before
    /// stay in the child, which has not started, as a refused later `map` leaves the earlier.
    pub fn map(&self, pages: Buffer, dst: usize, flags: MemFlags) -> Result<(), Error> {
        let len = pages.npages() * PAGE_SIZE;
        let src = pages.into_pages();
        for (at, n) in range_chunks(src.addr, len) {
            let to = dst + (at - src.addr);
            if let Err(e) =
                nothing(syscall(&Call::ProcessMap { process: self.0, src: at, dst: to, len: n, flags }))
            {
                let _ = unmap(at, src.addr + len - at);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Starts the process at `entry` with stack `sp` and `arg` in its first thread's first
    /// argument register (the startup page's address, 0 for none: servers/init.md); `handles`
    /// land in its slots 1..=n.
    pub fn start(&self, entry: usize, sp: usize, arg: usize, handles: &[Handle]) -> Result<(), Error> {
        let mut rec = Record([0; MAX_START_HANDLES]);
        let slots = rec.0.get_mut(..handles.len()).ok_or(Error::TooLarge)?;
        for (slot, handle) in slots.iter_mut().zip(handles) {
            *slot = handle.to_raw();
        }
        // The length fits: it is at most MAX_START_HANDLES (64).
        let count = handles.len() as u32;
        let call = Call::ProcessStart { process: self.0, entry, sp, arg, handles_rec: rec.addr(), count };
        nothing(syscall(&call))
    }
}

/// Which device `device` names, an MMIO range, an interrupt or the Reset right
/// (kernel/devices.md, `device_info`): any device handle, whichever typed handle holds it.
pub fn device_info(device: Handle) -> Result<DeviceInfo, Error> {
    match syscall(&Call::DeviceInfo { device })? {
        Return::Device(info) => Ok(info),
        _ => Err(Error::InvalidArgument),
    }
}

impl Mmio {
    /// Maps the device's registers: their address and how many bytes of them
    /// (kernel/devices.md, `map_device`). Which device this is comes from the boot manifest, not
    /// from the kernel.
    pub fn map(&self) -> Result<(usize, usize), Error> {
        match syscall(&Call::MapDevice { device: self.0 })? {
            Return::Mapping { addr, len } => Ok((addr, len)),
            _ => Err(Error::InvalidArgument),
        }
    }

    /// Maps the device's registers and hands them back as a checked region, which is the only
    /// way a driver reaches MMIO without `unsafe` of its own.
    pub fn registers(&self) -> Result<Registers, Error> {
        let (base, len) = self.map()?;
        // The kernel maps whole pages, so a non-zero length is at least one page.
        Ok(Registers { base, len, _not_sync: core::marker::PhantomData })
    }

    /// Takes or gives back the console's hold, on the console's registers (kernel/devices.md,
    /// "The console's one writer"): while it is held the kernel's lines wait, whole, and they go
    /// out when it is given back. A take answers whether a kernel line ended the caller's
    /// unfinished line since it last gave the hold back; a release says with `Hold::ReleaseOpen`
    /// that its line is unfinished. `WrongObject` on any other device; `Busy` if another process
    /// holds it.
    pub fn console_hold(&self, hold: Hold) -> Result<bool, Error> {
        syscall(&Call::ConsoleHold { device: self.0, hold }).map(|r| matches!(r, Return::Wrote(true)))
    }

    /// Runs `write`, a write of the console's registers, inside the console's hold, so no kernel
    /// line lands inside it. `write` is told whether a kernel line ended the line its last write
    /// left unfinished, so it starts its line again, and answers whether it leaves one unfinished
    /// now, so the kernel ends it before a line of its own. A hold refused, or a call the kernel
    /// lacks, is a write as before: the hold only keeps lines whole, and nothing waits on it.
    pub fn console_held<R>(&self, write: impl FnOnce(bool) -> (R, bool)) -> R {
        let held = self.console_hold(Hold::Take);
        let (result, open) = write(held.unwrap_or(false));
        if held.is_ok() {
            let _ = self.console_hold(if open { Hold::ReleaseOpen } else { Hold::Release });
        }
        result
    }

    /// `npages` contiguous zeroed pages the device may DMA to, held by a [`Dma`].
    pub fn dma_alloc(&self, npages: usize) -> Result<Dma, Error> {
        let len = npages.checked_mul(PAGE_SIZE).ok_or(Error::TooLarge)?;
        match syscall(&Call::DmaAlloc { device: self.0, npages })? {
            Return::Dma { addr, phys } => Ok(Dma { addr, phys, len }),
            _ => Err(Error::InvalidArgument),
        }
    }
}

/// A run of pages [`Mmio::dma_alloc`] mapped: their address in this process and the physical
/// address the device is programmed with. The mapping is this value's alone and dropping it
/// unmaps it, so no safe code can unmap it underneath a driver that holds one.
///
/// Dropping a `Dma` unmaps only this process's mapping: the frames stay put, held, charged and
/// out of the pool until the process ends (kernel/devices.md, `dma_alloc`). It does not free
/// the device's memory.
#[derive(Debug)]
pub struct Dma {
    addr: usize,
    phys: u64,
    len: usize,
}

impl Dma {
    /// Where the pages are mapped in this process.
    pub fn addr(&self) -> usize { self.addr }

    /// Their physical address, which is what the device is programmed with.
    pub fn phys(&self) -> u64 { self.phys }
}

impl Drop for Dma {
    fn drop(&mut self) {
        // A failure means the pages are already gone; there is nothing else to do.
        let _ = unmap(self.addr, self.len);
    }
}

/// A device's registers, as [`Mmio::registers`] mapped them: the one safe way to reach MMIO, so
/// a driver needs no `unsafe` (TENETS.md 2, the `unsafe` budget). Every access is bounds-checked
/// against the length `map_device` reported, so an offset taken from a device tree, a manifest
/// or a device is a refusal rather than a read outside the mapping.
///
/// Accesses are volatile: the compiler may neither drop, duplicate nor reorder them among
/// themselves, which is what a device's registers need (reading one can pop a FIFO). A
/// `Registers` is `Send` but not `Sync`: it may be moved to the thread that drives the device,
/// and cannot be shared, so two threads never touch one device's registers through one value.
#[derive(Debug)]
pub struct Registers {
    base: usize,
    len: usize,
    /// Makes the type `!Sync` (and keeps it `Send`).
    _not_sync: core::marker::PhantomData<core::cell::Cell<u8>>,
}

impl Registers {
    /// How many bytes of registers are mapped.
    pub fn len(&self) -> usize { self.len }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// The byte at `offset`, or `None` if it is outside the mapping.
    pub fn read_u8(&self, offset: usize) -> Option<u8> {
        if offset >= self.len {
            return None;
        }
        // SAFETY: `base + offset` is a byte of a device mapping that stays mapped while this value
        // lives, and that no other thread reaches through it; a `u8` needs no alignment. Who
        // guarantees it: the installed transport (the kernel, on the machine) made
        // `base..base + len` for this process in `map_device` (a mapping of its own each time,
        // outliving its handle, kernel/devices.md); `Registers` ends it only in `unmap`, which
        // consumes the value, and is not `Sync`; `offset < len` is checked just above.
        Some(unsafe { ((self.base + offset) as *const u8).read_volatile() })
    }

    /// Writes `value` at `offset`; false if it is outside the mapping.
    pub fn write_u8(&self, offset: usize, value: u8) -> bool {
        if offset >= self.len {
            return false;
        }
        // SAFETY: as in `read_u8`; the mapping is read-write.
        unsafe { ((self.base + offset) as *mut u8).write_volatile(value) };
        true
    }

    /// Unmaps the registers: what a program does before it hands the device to another, as
    /// `init` hands the UART to `consoled` (servers/init.md). It takes the value, so nothing is
    /// left to reach the range afterwards, refused or not.
    ///
    /// ```compile_fail
    /// fn after(registers: redoubt_rt::handle::Registers) {
    ///     let _ = registers.unmap();
    ///     registers.read_u8(0);
    /// }
    /// ```
    pub fn unmap(self) -> Result<(), Error> { unmap(self.base, self.len) }
}

impl Irq {
    /// Waits for the interrupt (R5: receiving unmasks the source; there is no acknowledge).
    pub fn wait(&self, timeout: u64) -> Result<(), Error> {
        match crate::ipc::receive_raw(Some(self.0), timeout, 0)? {
            // Only the IRQ handle named can fire in this `receive`.
            crate::ipc::Event::Interrupt => Ok(()),
            _ => Err(Error::InvalidArgument),
        }
    }
}

impl Reset {
    pub fn reset(&self, kind: ResetKind) -> Result<(), Error> {
        nothing(syscall(&Call::SystemReset { device: self.0, kind }))
    }
}
