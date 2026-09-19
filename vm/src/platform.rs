//! Everything the VM needs from the operating system, and nothing more.
//!
//! The VM is `no_std` and has no other way to reach the outside world. A [`Platform`] is chosen
//! by whoever embeds the VM: `beamlet-posix` for development and differential testing on a host,
//! and later a Xous one whose methods are IPC calls to servers. Keeping this surface small is
//! what makes the VM auditable: to know what BEAM code can do to the system, read this trait.
//!
//! Capability discipline: the platform decides what a VM instance may reach. The VM itself holds
//! no ambient authority (no filesystem, no network, no clock it did not get from here).

use alloc::vec::Vec;

/// Services the host operating system provides to one VM instance.
pub trait Platform {
    /// Monotonic time in microseconds since an arbitrary fixed point. Never goes backwards.
    fn monotonic_us(&mut self) -> u64;

    /// Wall-clock time in microseconds since the Unix epoch, if the platform has a clock.
    fn system_time_us(&mut self) -> Option<u64>;

    /// Block until `deadline` (a [`Platform::monotonic_us`] value) passes, or indefinitely for
    /// `None`. Called only when no process can run. May return early (spuriously, or because an
    /// external event arrived); the VM rechecks its timers either way.
    fn idle(&mut self, deadline: Option<u64>);

    /// Write bytes to the VM's console (the `user` I/O device).
    fn console_write(&mut self, bytes: &[u8]);

    /// Fill `buf` from a cryptographically secure source. On failure the VM raises rather than
    /// using a weaker source.
    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError>;

    /// The bytes of the `.beam` file for `module`, if this VM may load it. This is the only way
    /// code enters the VM, so it is where a platform enforces signing or an allowlist.
    fn load_module(&mut self, module: &str) -> Option<Vec<u8>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    Unavailable,
}
