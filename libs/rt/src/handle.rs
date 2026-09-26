//! Typed handles and the system calls that are not IPC (kernel/abi.md). IPC is in
//! [`crate::ipc`].
//!
//! A typed handle only says what the program expects the handle to be: the kernel checks the
//! object's kind on every use (`WrongObject`), so wrapping the wrong kind is a refused call, never
//! a confusion. Handles are not closed on drop: a server keeps them in tables and hands out
//! copies, and an implicit close would be an easy use-after-close. Close them with `close`.

use redoubt_sys::{
    BUDGET_SPEC_SLOTS, BudgetSpec, Call, Error, Handle, MAX_START_HANDLES, MemFlags, ResetKind, Return,
    USAGE_SLOTS, Usage,
};

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

/// Maps `len` bytes (whole pages) of zeroed memory; returns its address.
pub fn map_anon(len: usize, flags: MemFlags) -> Result<usize, Error> {
    addr(syscall(&Call::MapAnon { len, flags }))
}

pub fn unmap(addr: usize, len: usize) -> Result<(), Error> { nothing(syscall(&Call::Unmap { addr, len })) }

pub fn set_flags(addr: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
    nothing(syscall(&Call::SetFlags { addr, len, flags }))
}

/// Starts a thread at `entry` with stack `sp` and `arg` in its first argument register.
pub fn thread_create(entry: usize, sp: usize, arg: usize) -> Result<u32, Error> {
    match syscall(&Call::ThreadCreate { entry, sp, arg })? {
        Return::Tid(tid) => Ok(tid),
        _ => Err(Error::InvalidArgument),
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

    /// Destroys the budget and everything charged to it (R10).
    pub fn destroy(self) -> Result<(), Error> { nothing(syscall(&Call::BudgetDestroy { budget: self.0 })) }

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

    /// Moves the caller's pages at `src` to `dst` in the (unstarted) process.
    pub fn map(&self, src: usize, dst: usize, len: usize, flags: MemFlags) -> Result<(), Error> {
        nothing(syscall(&Call::ProcessMap { process: self.0, src, dst, len, flags }))
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

    /// `npages` contiguous zeroed pages the device may DMA to: (address, physical address).
    pub fn dma_alloc(&self, npages: usize) -> Result<(usize, u64), Error> {
        match syscall(&Call::DmaAlloc { device: self.0, npages })? {
            Return::Dma { addr, phys } => Ok((addr, phys)),
            _ => Err(Error::InvalidArgument),
        }
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
        // SAFETY: `base..base + len` is the device mapping the kernel made for this process in
        // `map_device`; it stays mapped for the life of the process (a device mapping outlives
        // its handle, kernel/devices.md), `offset < len` is checked just above, and a `u8` needs no
        // alignment. `Registers` is not `Sync`, so no other thread holds this same region.
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
