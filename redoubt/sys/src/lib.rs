//! The Redoubt system call ABI, shared by the kernel and every process.
//!
//! The calls, errors and constants are KERNEL-SPEC.md's, under the same names. This crate only
//! says how they travel: which registers, which buffers, which numbers. What a call *does* is the
//! kernel's business.
//!
//! # Registers
//!
//! A call is an `ecall` with `a0` = the call's [`Number`] and its arguments in `a1..=a7`, in the
//! order listed on each [`Call`] variant. The kernel answers in the same eight registers:
//! `a0` = 0 and the result in `a1..=a7` (see [`Return`]), or `a0` = an [`Error`] code and
//! `a1..=a7` = 0.
//!
//! | Argument kind | Registers | Rule |
//! | --- | --- | --- |
//! | address, length, word (`usize`) | 1 | |
//! | 64-bit value (`u64`: ids, badges, accounts, time) | 1 on rv64, 2 on rv32 (low, high) | |
//! | small value (`u32`: tid, pid, exit code, weight) | 1 | must fit in 32 bits |
//! | handle | 1 | an index below `u32::MAX` |
//! | optional handle | 1 | `u32::MAX` = none |
//! | optional page range (lend, transfer) | 2: address, pages | (0, 0) = none |
//! | enum tag (reset kind, mint source; in buffers: class, kinds) | 1 | numbered from 1; 0 is never a valid tag, except "no buffer" in a [`Received`] message |
//!
//! Registers a call does not use must be 0. Decoding in the kernel's direction ([`Call::decode`])
//! rejects every malformed value with an error and never panics (KERNEL-SPEC.md I14). It checks
//! only the encoding: unknown numbers, tags and flag bits, values too wide for their field, lists
//! longer than their array, non-zero unused registers and slots. Everything else (does the handle
//! exist, is the range page-aligned, is `len` at most 64) is the kernel's check. A decoding
//! failure is `BadHandle` for a handle that is not an index, `TooLarge` for a list count above its
//! capacity, and `InvalidArgument` for everything else.
//!
//! # Buffers
//!
//! What does not fit in seven registers goes through a buffer in the caller's memory, passed by
//! address. Every buffer is a fixed-length array of `u64` slots, little-endian, the same layout on
//! both widths; a `usize` field in a slot must fit the target's `usize`. Unused slots must be 0.
//! The kernel copies the buffer in, decodes it here, and (for results) encodes and copies out.
//!
//! | Buffer | Slots | Used by |
//! | --- | --- | --- |
//! | [`Body`]: words, handle count, handles | [`BODY_SLOTS`] | `call` (request in, reply out), `send`, `reply` |
//! | [`Received`]: what `receive` returned | [`RECEIVED_SLOTS`] | `receive` (out) |
//! | [`BudgetSpec`]: a new budget's fields | [`BUDGET_SPEC_SLOTS`] | `budget_create` (in) |
//! | handle list: one handle per slot ([`Handle::from_raw`]) | the call's count | `process_start` (in) |
//! | bytes | the call's length | `random` (out) |
//!
//! # Widths
//!
//! The encodings are generic over the register type ([`Register`], `u32` or `u64`), so the host
//! tests exercise both. [`Reg`] is the target's register type; `width.rs` is the one place in this
//! crate that looks at `target_pointer_width`.

#![no_std]
// `deny`, not `forbid`: the `ecall` stub (the only `unsafe` here) must be able to allow it.
#![deny(unsafe_code)]

mod buffer;
mod call;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod ecall;
mod error;
mod regs;
mod ret;
#[cfg(test)]
mod tests;
mod width;

pub use buffer::{
    BODY_SLOTS, BUDGET_SPEC_SLOTS, Body, BudgetSpec, Buffer, Cause, Class, ExitNotice, Handles, Labels, List,
    Message, RECEIVED_SLOTS, Received,
};
pub use call::{Call, Handle, MemFlags, MintSource, Number, Pages, ResetKind};
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub use ecall::syscall;
pub use error::Error;
pub use regs::{REGS, Register};
pub use ret::{Return, Usage, decode_result, encode_result};
pub use width::Reg;

/// Machine words in a message.
pub const WORDS: usize = 4;
/// Handles carried by one message.
pub const MAX_MSG_HANDLES: usize = 4;
/// Pages in one lend (= the 9P `msize`, 64 KiB; WIRE.md).
pub const MAX_LEND_PAGES: usize = 16;
/// Threads per process.
pub const MAX_THREADS: usize = 31;
/// Labels per budget.
pub const MAX_LABELS: usize = 8;
/// Budget tree depth; the root is at depth 0.
pub const MAX_DEPTH: usize = 8;
/// Blocked senders per account per endpoint.
pub const WAIT_CAP: usize = 16;
/// Stride scheduling numerator.
pub const STRIDE: u64 = 1 << 20;
/// The time slice, in microseconds (10 ms).
pub const SLICE: u64 = 10_000;
/// A timeout that never expires (microseconds).
pub const FOREVER: u64 = u64::MAX;
