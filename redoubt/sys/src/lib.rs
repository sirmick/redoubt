//! The Redoubt system call ABI, shared by the kernel and every process.
//!
//! The calls, errors and constants are KERNEL-SPEC.md's, under the same names. This crate only
//! says how they travel: which registers, which records, which numbers (KERNEL-SPEC.md, ABI: the
//! spec owns the calls, this crate their encoding). What a call *does* is the kernel's business.
//!
//! # Registers
//!
//! A call is an `ecall` with `a0` = the call's [`Number`] (from [`NUMBER_BASE`] + 1) and its arguments in
//! `a1..=a7`, in the order listed on each [`Call`] variant. The kernel answers in the same eight registers:
//! `a0` = 0 and the result in `a1..=a7` (see [`Return`]), or `a0` = an [`Error`] code and
//! `a1..=a7` = 0. The kernel preserves every register other than `a0..=a7` across the `ecall`.
//!
//! The layout is the same on both widths: in this crate a register is a `u64` holding the
//! register's value, and no register ever holds more than 32 bits or one `usize`, so rv32 carries
//! every encoding.
//!
//! | Argument kind | Registers | Rule |
//! | --- | --- | --- |
//! | address, length, word (`usize`) | 1 | |
//! | 64-bit value (`u64`: ids, badges, accounts, time, `random`'s value) | 2: low half, high half | each half fits in 32 bits |
//! | small value (`u32`: tid, pid, exit code, weight, count) | 1 | must fit in 32 bits |
//! | [`MemFlags`] | 1 | only `READ`, `WRITE`, `EXECUTE`; never `WRITE` with `EXECUTE` |
//! | handle | 1 | an index, 1..=`u32::MAX` |
//! | optional handle | 1 | 0 = none (index 0 is never allocated) |
//! | message id, badge (`NonZeroU64`) | 2: low half, high half | never 0 as an argument (a received badge may be 0: the receive right's) |
//! | optional [`Pages`] (lend, transfer) | 2: address, pages | (0, 0) = none; one of them 0 is invalid |
//! | enum tag (reset kind, mint source; in records: the record's kind, an exit's cause) | 1 | numbered from 1; 0 is never a valid tag |
//! | flag (in records: `budget_create`'s `first`) | 1 | 0 or 1 |
//!
//! Registers a call does not use must be 0.
//!
//! Timeouts are relative microseconds. The kernel turns one into a deadline with
//! `saturating_add`, so [`FOREVER`] never wraps and never expires.
//!
//! # Records
//!
//! What does not fit in seven registers goes through a record in the caller's memory, passed by
//! address (the fields named `*_rec`). A record is a fixed-length array of `u64` slots,
//! little-endian and 8-byte aligned (a misaligned record is `InvalidArgument`), with the same
//! layout on both widths; unused slots must be 0. A `usize` field in a slot must fit the target's
//! `usize`: on rv32 a value above `u32::MAX` is `InvalidArgument`. The kernel copies the record in,
//! decodes it here, and (for results) encodes and copies out. "Buffer" keeps the spec's meaning:
//! the pages of a lend or a transfer.
//!
//! | Record | Slots | Used by |
//! | --- | --- | --- |
//! | [`Body`]: words, handle count, handles | [`BODY_SLOTS`] | `call` (request in), `send`, `reply` |
//! | [`ReceivedBody`]: the same slots; a handle slot within the count may be 0 | [`BODY_SLOTS`] | `call` (reply out) |
//! | [`Received`]: kind, msg_id, badge, account, labels, words, handles, buffer, pages; one layout for a message, an interrupt, an exit notice and an abandoned-call notice | [`RECEIVED_SLOTS`] | `receive` (out) |
//! | [`BudgetSpec`]: pages, processes, weight, first, labels, account, deadline | [`BUDGET_SPEC_SLOTS`] | `budget_create` (in) |
//! | [`Usage`]: page limit and usage, process limit and usage, weight limit and carved | [`USAGE_SLOTS`] | `budget_usage` (out) |
//! | handle list: one handle per slot ([`Handle::from_raw`]) | the call's count, at most [`MAX_START_HANDLES`] | `process_start` (in) |
//!
//! A list in a record (labels, handles) is a count, then its capacity's slots, the unused ones 0.
//! Handles going in are all handles. Handles coming out ([`ReceivedHandles`]: in a message, and in
//! the reply `call` writes back) may have a slot of 0 within the count, which keeps its place: a
//! handle revoked while its message was in flight (R10), or a reply's handle the caller could not
//! take (QUESTIONS.md 116, pending).
//!
//! A record's page must already be backed: the kernel does not allocate while it decodes, so an
//! untouched page is `InvalidArgument` (QUESTIONS.md 115, pending; the check is the kernel's).
//!
//! Records can overlap the pages a call acts on; the kernel must copy a record in before it
//! changes those pages, and copy results out only to memory still the caller's. For example a
//! `call`'s body record may lie inside its lend (which is unmapped from the caller during the
//! call), and a `process_start` handle list may lie in pages just moved away by `process_map`.
//!
//! # Decoding
//!
//! Decoding in the kernel's direction ([`Call::decode`], [`Body::decode`],
//! [`BudgetSpec::decode`]) rejects every malformed value with an error and never panics
//! (KERNEL-SPEC.md I14). It checks the encoding (unknown numbers, tags and flag bits, values too
//! wide for their field, lists longer than their array, non-zero unused registers and slots) and
//! the few rules a single value states by itself: no W+X flags, no badge or message id 0,
//! `process_start`'s count at most [`MAX_START_HANDLES`]. Everything else (does the handle exist,
//! is the range page-aligned) is the kernel's check.
//!
//! Which error, and in what order, is the spec's (KERNEL-SPEC.md, Errors and the order of checks):
//! decoding is its stage 1, and this crate implements that stage for registers and slots. In
//! summary: the first malformed value in register (then slot) order wins; `BadHandle` for a
//! required handle that is 0 or wider than 32 bits, `TooLarge` for a count above its limit,
//! `InvalidArgument` for everything else. One case needs care: `mint`'s source is a tag and a
//! 64-bit value in two registers, and each half is checked for width (`InvalidArgument`) when
//! it is read, before the tag is looked at; so a handle source whose low half is wider than 32
//! bits (possible only on rv64) is `InvalidArgument`, and one whose high half is not 0 is
//! `BadHandle`. A record's alignment and whether it lies in the caller's memory come before its
//! slots and are the kernel's to check.
//!
//! The userspace decoders ([`decode_result`], [`Received::decode`], [`ReceivedBody::decode`],
//! [`Usage::decode`]) use the same errors, with one rule of their own: a `Received` record's kind
//! is read first, and any non-zero slot outside the fields that kind fills (a list's count
//! included) is `InvalidArgument` before any field is read.
//!
//! Which errors each call can return at all, its row in the spec's error table, is
//! [`Number::can_return`].

#![no_std]
// `deny`, not `forbid`: the `ecall` stub (the only `unsafe` here) must be able to allow it.
#![deny(unsafe_code)]

mod call;
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod ecall;
mod error;
mod record;
mod regs;
mod ret;
#[cfg(test)]
mod tests;

pub use call::{Call, Handle, MemFlags, MintSource, NUMBER_BASE, Number, Pages, ResetKind};
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub use ecall::syscall;
pub use error::Error;
pub use record::{
    BODY_SLOTS, BUDGET_SPEC_SLOTS, Body, BodyOf, BudgetSpec, Cause, ExitNotice, Handles, Labels, List,
    Message, MessageKind, RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, Slot, USAGE_SLOTS, Usage,
};
pub use regs::REGS;
pub use ret::{Return, decode_result, encode_result};

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
/// Queued messages (sent, not yet taken) per group (R2: (account, label set), and budget for
/// account 0) per endpoint; `Busy` beyond.
pub const WAIT_CAP: usize = 16;
/// Stride scheduling numerator.
pub const STRIDE: u64 = 1 << 20;
/// The time slice, in microseconds (10 ms).
pub const SLICE: u64 = 10_000;
/// A timeout that never expires (microseconds); as a budget deadline, none.
pub const FOREVER: u64 = u64::MAX;
/// Open calls per process (taken by `receive`, not yet replied to). At the limit the process
/// takes no more calls, while sends, interrupts and notices still arrive (R4a).
pub const MAX_OPEN_CALLS: usize = 64;
/// Handles one `process_start` copies into the child at most.
pub const MAX_START_HANDLES: usize = 64;
// QUESTIONS.md 102 (pending): not in KERNEL-SPEC.md's constants yet. The recommendation names it
// there; a call that would add a handle past it gets `TooLarge` (`Number::can_return`).
/// Handles one process may hold (the kernel's handle table holds this many).
pub const MAX_HANDLES: usize = 4096;
/// The base page, on both Sv32 and Sv39: the unit of lends, transfers and page counts. What each
/// kernel object costs in pages is KERNEL-SPEC.md's cost table.
pub const PAGE_SIZE: usize = 4096;
