//! The steward's constants (servers/steward.md, "The powerbox and approvals"): changed only by a
//! new system bundle, never per principal. Times are in microseconds, the kernel's unit.

/// Pending requests per domain.
pub const PENDING_CAP: usize = 4;
/// A requester-supplied field is cut to this many characters on a screen.
pub const FIELD_CAP: usize = 64;
/// A declassified item is at most this many bytes.
pub const DECLASSIFY_MAX: usize = 256;
/// This many crashes blamed on one domain within `BLAME_WINDOW` end its sessions and leases.
pub const BLAME_COUNT: usize = 3;
pub const BLAME_WINDOW: u64 = 10 * 60 * 1_000_000;
/// The longest lease: 24 hours.
pub const MAX_LEASE: u64 = 24 * 3600 * 1_000_000;
/// How long a reader or writer budget may live: it dies after its one item, or at this deadline.
pub const CROSSING_LIFE: u64 = 1_000_000;
/// The fresh random words an event carries for the ids it may need (R36).
pub const RANDOM_WORDS: usize = 8;
/// A label set's length, the kernel's.
pub const MAX_LABELS: usize = redoubt_sys::MAX_LABELS;
