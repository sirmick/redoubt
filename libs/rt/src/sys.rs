//! The one seam between the runtime and the kernel. Everything else in this crate makes its
//! system calls through [`syscall`], so on the host the whole crate runs against a fake kernel
//! with no other change.
//!
//! - On the machine (`target_os = "none"`) a call is `redoubt_sys::syscall`: the `ecall`.
//! - On the host, tests install a [`HostKernel`] once per test binary, and every call goes to it. Records and
//!   buffers travel by address exactly as they do on the machine, so the fake reads and writes the same
//!   memory the kernel would.

use redoubt_sys::{Call, Error, Return};

/// Makes one system call.
#[cfg(target_os = "none")]
pub(crate) fn syscall(call: &Call) -> Result<Return, Error> { redoubt_sys::syscall(call) }

/// Makes one system call against the installed [`HostKernel`]. Calling with none installed is a
/// bug in the test, so it panics.
#[cfg(not(target_os = "none"))]
pub(crate) fn syscall(call: &Call) -> Result<Return, Error> {
    host::KERNEL
        .get()
        .expect("no HostKernel installed: call redoubt_rt::install_host_kernel first")
        .syscall(call)
}

#[cfg(not(target_os = "none"))]
pub use host::{HostKernel, install_host_kernel};

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

    use redoubt_sys::{Call, Error, Return};

    /// A stand-in for the kernel, for host tests. It sees each call exactly as the kernel would:
    /// registers decoded into a [`Call`], records and buffers as addresses in this (host)
    /// process's memory.
    pub trait HostKernel: Send + Sync {
        fn syscall(&self, call: &Call) -> Result<Return, Error>;
    }

    pub(super) static KERNEL: OnceLock<&'static dyn HostKernel> = OnceLock::new();

    /// Installs the fake kernel for this test binary. Only the first call counts: a test binary
    /// has one kernel, shared by every test in it.
    pub fn install_host_kernel(kernel: &'static dyn HostKernel) { let _ = KERNEL.set(kernel); }
}
