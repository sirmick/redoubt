//! The one error enum every call returns (KERNEL-SPEC.md, Errors), and which errors each call can
//! return.

use Error::*;

use crate::Number;

/// Why a call failed. The code travels in `a0`; 0 there means success, so codes start at 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Error {
    BadHandle = 1,
    WrongObject = 2,
    InvalidArgument = 3,
    /// Page limit.
    OutOfMemory = 4,
    OutOfProcesses = 5,
    TooManyThreads = 6,
    NotPermitted = 7,
    ClassDenied = 8,
    LabelDenied = 9,
    Busy = 10,
    Refused = 11,
    TooLarge = 12,
    Timeout = 13,
    Dead = 14,
}

impl Error {
    /// Every error, in code order.
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

    /// The error with this code; `None` for 0 and unknown codes.
    pub fn from_code(code: u64) -> Option<Error> { Error::ALL.iter().copied().find(|e| *e as u64 == code) }
}

/// A set of errors, one bit per code.
#[derive(Clone, Copy)]
struct Errors(u32);

const fn set(errors: &[Error]) -> Errors {
    let mut bits = 0;
    let mut i = 0;
    while i < errors.len() {
        bits |= 1 << errors[i] as u32;
        i += 1;
    }
    Errors(bits)
}

impl Errors {
    const fn with(self, other: Errors) -> Errors { Errors(self.0 | other.0) }
}

/// Every call can fail decoding in the general ways of stage 1: a non-zero unused register, a
/// value too wide for its field.
const DECODING: Errors = set(&[InvalidArgument]);

/// A call that adds a handle to its caller's table can find the caller's budget unable to pay
/// for a new table page (KERNEL-SPEC.md, above the error table).
const ADDS_HANDLE: Errors = set(&[OutOfMemory]).with(HANDLE_LIMIT);

// QUESTIONS.md 102 (pending): a process holds at most `MAX_HANDLES` handles, and a call that
// would add one more to its caller's table gets `TooLarge`. Under a different answer this set is
// empty.
const HANDLE_LIMIT: Errors = set(&[TooLarge]);

impl Number {
    /// Whether this call can return `error`: the errors of its row in KERNEL-SPEC.md's error
    /// table (in what order they are checked is the spec's), plus decoding's general
    /// `InvalidArgument`, and `OutOfMemory` for a call that adds a handle to its caller's table
    /// (but not `call` for its reply's handles: QUESTIONS.md 116). The kernel's interim refusals
    /// (a call not built yet, or made from a legacy interrupt callback) are outside it; the
    /// kernel checks every error it returns against this in debug builds.
    pub fn can_return(self, error: Error) -> bool { self.errors().0 & 1 << error as u32 != 0 }

    fn errors(self) -> Errors {
        let row = match self {
            Number::MapAnon => set(&[InvalidArgument, OutOfMemory]),
            Number::Unmap | Number::SetFlags => set(&[InvalidArgument]),
            Number::MapDevice => set(&[BadHandle, WrongObject, OutOfMemory]),
            Number::DmaAlloc => set(&[BadHandle, WrongObject, InvalidArgument, NotPermitted, OutOfMemory]),
            Number::ThreadCreate => set(&[TooManyThreads, OutOfMemory]),
            Number::ThreadExit => set(&[]),
            Number::ProcessExit => set(&[InvalidArgument]),
            // `NotPermitted`: the exit endpoint's badge is not 0.
            Number::ProcessCreate => {
                set(&[BadHandle, WrongObject, InvalidArgument, NotPermitted, OutOfProcesses])
                    .with(ADDS_HANDLE)
            }
            Number::ProcessMap => set(&[BadHandle, WrongObject, InvalidArgument, NotPermitted, OutOfMemory]),
            Number::ProcessStart => set(&[BadHandle, TooLarge, WrongObject, NotPermitted, OutOfMemory]),
            Number::EndpointCreate => ADDS_HANDLE,
            Number::Mint => {
                set(&[InvalidArgument, BadHandle, Dead, WrongObject, NotPermitted]).with(ADDS_HANDLE)
            }
            // QUESTIONS.md 116 (pending): a reply is delivered even when some of its handles do
            // not fit the caller's table (budget or `MAX_HANDLES`); those slots arrive as 0, so
            // `call` adds no error for them. Under 107's reading instead, the call is the caller's
            // `OutOfMemory` (and `TooLarge` past `MAX_HANDLES`): add `.with(ADDS_HANDLE)` below.
            Number::Call => set(&[
                BadHandle,
                TooLarge,
                InvalidArgument,
                WrongObject,
                LabelDenied,
                Busy,
                Refused,
                Timeout,
                Dead,
            ]),
            Number::Send => set(&[
                BadHandle,
                TooLarge,
                InvalidArgument,
                WrongObject,
                LabelDenied,
                Busy,
                Refused,
                Timeout,
                Dead,
            ]),
            // QUESTIONS.md 105 (pending): at `MAX_OPEN_CALLS` calls stay queued, so `receive` is
            // never `Busy`; nor `OutOfMemory`, since a message the receiver cannot pay for is its
            // sender's `Refused` (R4).
            Number::Receive => set(&[BadHandle, WrongObject, NotPermitted, Timeout, Dead]),
            Number::Reply => set(&[InvalidArgument, TooLarge, BadHandle]),
            Number::Serve => set(&[InvalidArgument]),
            Number::HandleClose => set(&[BadHandle]),
            Number::BudgetCreate => set(&[
                BadHandle,
                TooLarge,
                InvalidArgument,
                WrongObject,
                ClassDenied,
                LabelDenied,
                OutOfMemory,
                OutOfProcesses,
            ])
            .with(ADDS_HANDLE),
            Number::BudgetDestroy => set(&[BadHandle, WrongObject]),
            Number::BudgetUsage => set(&[BadHandle, WrongObject, LabelDenied]),
            Number::TimeNow | Number::Random => set(&[]),
            Number::SystemReset => set(&[BadHandle, InvalidArgument, WrongObject]),
        };
        row.with(DECODING)
    }
}
