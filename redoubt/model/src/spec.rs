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
/// Queued messages per group (R2) per endpoint.
pub const WAIT_CAP: u64 = 16;
/// Stride scheduling numerator.
pub const STRIDE: u64 = 1 << 20;
/// Time slice, in microseconds (10 ms).
pub const SLICE: u64 = 10_000;
/// A timeout that never expires.
pub const FOREVER: u64 = u64::MAX;
/// Taken-but-unreplied calls per process (QUESTIONS 2, as answered).
pub const MAX_OPEN_CALLS: u64 = 64;
/// Handles in `process_start`'s list (QUESTIONS 10).
pub const MAX_START_HANDLES: usize = 64;
/// "No handle" in an optional-handle slot, and never a handle index (QUESTIONS 10). The ABI's
/// sentinel; this is the one place the model names it.
pub const NO_HANDLE: u64 = 0;

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
}

/// Budget class. `user < system` (the derived order is the spec's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    User,
    System,
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
}

/// Encoding of `system_reset`'s `kind`.
pub const RESET_POWER_OFF: u64 = 1;
pub const RESET_REBOOT: u64 = 2;

/// Encoding: the largest value of the ABI's 32-bit fields (exit code, pid, tid, weight, process
/// counts) and of a handle index.
pub const U32_MAX: u64 = u32::MAX as u64;

/// The counters `budget_usage` returns (QUESTIONS 11): each carved limit with its usage (R6, R7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Counters {
    pub pages_limit: u64,
    pub pages_usage: u64,
    pub processes_limit: u64,
    pub processes_usage: u64,
    pub weight_limit: u64,
    /// Weight carved out to children.
    pub weight_usage: u64,
}

/// `true` if `outer ⊇ inner`; both are sorted and deduplicated.
pub fn superset(outer: &[u64], inner: &[u64]) -> bool {
    inner.iter().all(|l| outer.binary_search(l).is_ok())
}
