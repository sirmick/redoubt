//! Typed handles and the system calls that are not IPC (KERNEL-SPEC.md, System calls). IPC is
//! in [`crate::ipc`].
//!
//! A typed handle only says what the program expects the handle to be: the kernel checks the
//! object's kind on every use (`WrongObject`), so wrapping the wrong kind is a refused call, never
//! a confusion. Handles are not closed on drop: a server keeps them in tables and hands out
//! copies, and an implicit close would be an easy use-after-close. Close them with `close`.

use redoubt_sys::{
    BUDGET_SPEC_SLOTS, BudgetSpec, Call, Error, Handle, MAX_RANDOM, MAX_START_HANDLES, MemFlags, ResetKind,
    Return, USAGE_SLOTS, Usage,
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

/// Fills `bytes` from the kernel's CSPRNG, `MAX_RANDOM` bytes per call.
pub fn random(bytes: &mut [u8]) -> Result<(), Error> {
    for chunk in bytes.chunks_mut(MAX_RANDOM) {
        let (addr, len) = (chunk.as_mut_ptr() as usize, chunk.len());
        nothing(syscall(&Call::Random { bytes: addr, len }))?;
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

    /// Starts the process at `entry` with stack `sp`; `handles` land in its slots 1..=n.
    pub fn start(&self, entry: usize, sp: usize, handles: &[Handle]) -> Result<(), Error> {
        let mut rec = Record([0; MAX_START_HANDLES]);
        let slots = rec.0.get_mut(..handles.len()).ok_or(Error::TooLarge)?;
        for (slot, handle) in slots.iter_mut().zip(handles) {
            *slot = handle.to_raw();
        }
        // The length fits: it is at most MAX_START_HANDLES (64).
        let count = handles.len() as u32;
        nothing(syscall(&Call::ProcessStart { process: self.0, entry, sp, handles_rec: rec.addr(), count }))
    }
}

impl Mmio {
    /// Maps the device's registers; returns their address.
    pub fn map(&self) -> Result<usize, Error> { addr(syscall(&Call::MapDevice { device: self.0 })) }

    /// `npages` contiguous zeroed pages the device may DMA to: (address, physical address).
    pub fn dma_alloc(&self, npages: usize) -> Result<(usize, u64), Error> {
        match syscall(&Call::DmaAlloc { device: self.0, npages })? {
            Return::Dma { addr, phys } => Ok((addr, phys)),
            _ => Err(Error::InvalidArgument),
        }
    }
}

impl Irq {
    /// Waits for the interrupt (R5: receiving unmasks the source; there is no acknowledge).
    pub fn wait(&self, timeout: u64) -> Result<(), Error> {
        match crate::ipc::receive_raw(Some(self.0), timeout, 0)? {
            crate::ipc::Event::Interrupt(irq) if irq == self.0 => Ok(()),
            _ => Err(Error::InvalidArgument),
        }
    }
}

impl Reset {
    pub fn reset(&self, kind: ResetKind) -> Result<(), Error> {
        nothing(syscall(&Call::SystemReset { device: self.0, kind }))
    }
}
