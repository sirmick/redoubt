//! The one error enum every call returns (KERNEL-SPEC.md, Errors).

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

    pub fn code(self) -> u32 { self as u32 }

    /// The error with this code; `None` for 0 and unknown codes.
    pub fn from_code(code: u64) -> Option<Error> {
        Error::ALL.iter().copied().find(|e| u64::from(e.code()) == code)
    }
}
