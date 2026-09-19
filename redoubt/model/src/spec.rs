//! The names KERNEL-SPEC.md defines: constants, errors, and the small value types of its calls.
//!
//! Everything here has the spec's name verbatim. The few values the spec leaves to the ABI
//! (flag bits, enum tags, the page size) are marked "encoding"; they are `redoubt-sys`'s values
//! (WP-A1), which owns them.

/// Machine words in a message (KERNEL-SPEC.md, Constants).
pub const WORDS: usize = 4;
/// Handles carried by one message.
pub const MAX_MSG_HANDLES: usize = 4;
/// Pages in one lend.
pub const MAX_LEND_PAGES: u64 = 16;
/// Threads per process.
pub const MAX_THREADS: u64 = 31;
/// Labels per budget.
pub const MAX_LABELS: usize = 8;
/// Budget tree depth, root = 0.
pub const MAX_DEPTH: u64 = 8;
/// Blocked senders per account per endpoint.
pub const WAIT_CAP: u64 = 16;
/// Stride scheduling numerator.
pub const STRIDE: u64 = 1 << 20;
/// Time slice, in microseconds (10 ms).
pub const SLICE: u64 = 10_000;
/// A timeout that never expires.
pub const FOREVER: u64 = u64::MAX;
/// `random`: "`len` at most 64". The spec states the number but does not name it.
pub const RANDOM_MAX_LEN: u64 = 64;

/// Encoding: the page size. KERNEL-SPEC.md counts memory in pages without stating a size;
/// both Sv32 and Sv39 use 4 KiB base pages.
pub const PAGE_SIZE: u64 = 4096;

/// Encoding of mapping flags (`map_anon`, `set_flags`, `process_map`), as `redoubt-sys`'s
/// `MemFlags`.
pub const FLAG_R: u64 = 1;
pub const FLAG_W: u64 = 2;
pub const FLAG_X: u64 = 4;

/// The error enum (KERNEL-SPEC.md, Errors), in the spec's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Error {
    BadHandle,
    WrongObject,
    InvalidArgument,
    /// Page limit.
    OutOfMemory,
    OutOfProcesses,
    TooManyThreads,
    NotPermitted,
    ClassDenied,
    LabelDenied,
    Busy,
    Refused,
    TooLarge,
    Timeout,
    Dead,
}

impl Error {
    pub const ALL: [Error; 14] = [
        Error::BadHandle,
        Error::WrongObject,
        Error::InvalidArgument,
        Error::OutOfMemory,
        Error::OutOfProcesses,
        Error::TooManyThreads,
        Error::NotPermitted,
        Error::ClassDenied,
        Error::LabelDenied,
        Error::Busy,
        Error::Refused,
        Error::TooLarge,
        Error::Timeout,
        Error::Dead,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Error::BadHandle => "BadHandle",
            Error::WrongObject => "WrongObject",
            Error::InvalidArgument => "InvalidArgument",
            Error::OutOfMemory => "OutOfMemory",
            Error::OutOfProcesses => "OutOfProcesses",
            Error::TooManyThreads => "TooManyThreads",
            Error::NotPermitted => "NotPermitted",
            Error::ClassDenied => "ClassDenied",
            Error::LabelDenied => "LabelDenied",
            Error::Busy => "Busy",
            Error::Refused => "Refused",
            Error::TooLarge => "TooLarge",
            Error::Timeout => "Timeout",
            Error::Dead => "Dead",
        }
    }

    pub fn from_name(s: &str) -> Option<Error> { Error::ALL.iter().copied().find(|e| e.name() == s) }
}

/// Budget class. `user < system` (the derived order is the spec's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    User,
    System,
}

impl Class {
    /// Encoding of the `class` argument of `budget_create` (`redoubt-sys` tags from 1).
    pub fn from_raw(v: u64) -> Option<Class> {
        match v {
            1 => Some(Class::User),
            2 => Some(Class::System),
            _ => None,
        }
    }

    pub fn raw(self) -> u64 {
        match self {
            Class::User => 1,
            Class::System => 2,
        }
    }
}

/// `cause` of an exit notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Exited,
    Faulted,
    Killed,
}

impl Cause {
    pub fn name(self) -> &'static str {
        match self {
            Cause::Exited => "exited",
            Cause::Faulted => "faulted",
            Cause::Killed => "killed",
        }
    }

    pub fn from_name(s: &str) -> Option<Cause> {
        [Cause::Exited, Cause::Faulted, Cause::Killed].into_iter().find(|c| c.name() == s)
    }
}

/// Encoding of `system_reset`'s `kind`.
pub const RESET_POWER_OFF: u64 = 1;
pub const RESET_REBOOT: u64 = 2;

/// Encoding: the largest value of the ABI's 32-bit fields (exit code, pid, tid, weight, process
/// counts) and of a handle index; `u32::MAX` itself is "no handle" in a register.
pub const U32_MAX: u64 = u32::MAX as u64;

/// The counters `budget_usage` returns. The spec says "counters"; like `redoubt-sys`, the model
/// returns the page and process limits with their usage (R6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Counters {
    pub pages_limit: u64,
    pub pages_used: u64,
    pub processes_limit: u64,
    pub processes_used: u64,
}

/// `true` if `outer ⊇ inner`; both are sorted and deduplicated.
pub fn superset(outer: &[u64], inner: &[u64]) -> bool { inner.iter().all(|l| outer.binary_search(l).is_ok()) }
