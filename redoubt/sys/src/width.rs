//! The target's register width: the one place in this crate that looks at `target_pointer_width`
//! (one of the three places it is allowed; MEMORY-LAYOUT.md). Everything else is generic over
//! [`Register`](crate::Register), so the host tests can run both widths' encodings.

/// The target's register type.
#[cfg(target_pointer_width = "64")]
pub type Reg = u64;
/// The target's register type.
#[cfg(target_pointer_width = "32")]
pub type Reg = u32;
