//! The one seam between the runtime and the kernel. Everything else in this crate makes its
//! system calls through [`syscall`], which hands each to one [`Transport`], so the whole crate runs
//! over any transport with no other change.
//!
//! - On the machine (`target_os = "none"`) it is the [`redoubt_sys::Ecall`].
//! - On the host, tests install one with [`install_transport`] once per test binary (the fake kernel), and
//!   every call goes to it. Records and buffers travel by address exactly as they do on the machine, so a
//!   transport reads and writes the same memory the kernel would.

pub use redoubt_sys::Transport;
use redoubt_sys::{Call, Error, Return};

/// Makes one system call.
#[cfg(target_os = "none")]
pub(crate) fn syscall(call: &Call) -> Result<Return, Error> { redoubt_sys::Ecall.call(call) }

/// Makes one system call through the installed [`Transport`]. Calling with none installed is a
/// bug in the test, so it panics.
#[cfg(not(target_os = "none"))]
pub(crate) fn syscall(call: &Call) -> Result<Return, Error> {
    host::TRANSPORT
        .get()
        .expect("no Transport installed: call redoubt_rt::install_transport first")
        .call(call)
}

#[cfg(not(target_os = "none"))]
pub use host::install_transport;

/// A record (redoubt-sys crate docs, Records): `u64` slots at an 8-byte-aligned address. The
/// `align` makes the alignment explicit rather than resting on `u64`'s alignment on each width.
#[repr(C, align(8))]
pub(crate) struct Record<const N: usize>(pub [u64; N]);

impl<const N: usize> Record<N> {
    /// Its address, for a record the kernel only reads.
    pub fn addr(&self) -> usize { self.0.as_ptr() as usize }

    /// Its address, for a record the kernel writes: taken from a unique borrow, so the kernel's
    /// write does not go through a shared one.
    pub fn addr_mut(&mut self) -> usize { self.0.as_mut_ptr() as usize }
}

#[cfg(not(target_os = "none"))]
mod host {
    extern crate std;

    use std::sync::OnceLock;

    use redoubt_sys::Transport;

    pub(super) static TRANSPORT: OnceLock<&'static dyn Transport> = OnceLock::new();

    /// Installs the transport every call takes: on the host, a stand-in for the kernel that sees
    /// each call exactly as the kernel would (registers decoded into a `Call`, records and
    /// buffers as addresses in this process's memory). Only the first call counts, and says so by
    /// returning `true`: a test binary has one kernel, shared by every test in it.
    pub fn install_transport(transport: &'static dyn Transport) -> bool { TRANSPORT.set(transport).is_ok() }
}
