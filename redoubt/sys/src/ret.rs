//! What a call returns, in registers: `a0` = 0 and the value in `a1..=a7`, or `a0` = the error's
//! code and `a1..=a7` = 0. Which [`Return`] a call gives is fixed by its [`Number`].

use crate::regs::{REGS, Reader, Writer};
use crate::{Error, Handle, Number};

/// A successful call's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Return {
    /// Calls with no value, and those whose result is in memory (`call`, `receive`, `random`).
    Nothing,
    /// `map_anon`, `map_device`.
    Addr(usize),
    /// `dma_alloc`. `phys` is a `u64` on both widths because Sv32 physical addresses are 34
    /// bits.
    Dma { addr: usize, phys: u64 },
    /// `thread_create`.
    Tid(u32),
    /// `process_create`, `endpoint_create`, `mint`, `budget_create`.
    Handle(Handle),
    /// `budget_usage`.
    Usage(Usage),
    /// `time_now`: microseconds since boot.
    Time(u64),
}

/// `budget_usage`'s counters, in register order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Usage {
    pub pages_limit: u64,
    pub pages_usage: u64,
    pub processes_limit: u32,
    pub processes_usage: u32,
}

/// The registers `a0..=a7` for a call's outcome (kernel side).
pub fn encode_result(result: &Result<Return, Error>) -> [u64; REGS] {
    let mut regs = [0; REGS];
    let w = &mut Writer::regs(&mut regs);
    match *result {
        Err(e) => w.u32(e as u32),
        Ok(value) => {
            w.u32(0);
            match value {
                Return::Nothing => {}
                Return::Addr(addr) => w.usize(addr),
                Return::Dma { addr, phys } => {
                    w.usize(addr);
                    w.u64(phys);
                }
                Return::Tid(tid) => w.u32(tid),
                Return::Handle(h) => w.u32(h.index()),
                Return::Usage(u) => {
                    w.u64(u.pages_limit);
                    w.u64(u.pages_usage);
                    w.u32(u.processes_limit);
                    w.u32(u.processes_usage);
                }
                Return::Time(time) => w.u64(time),
            }
        }
    }
    regs
}

/// A call's outcome from `a0..=a7` (userspace side), given the call it answers. The kernel is
/// trusted to encode correctly; a malformed result (unknown code, wrong shape, stray non-zero
/// register) is still an error (crate docs, Decoding) rather than a panic.
pub fn decode_result(number: Number, regs: &[u64; REGS]) -> Result<Return, Error> {
    let mut r = Reader::regs(regs);
    let code = r.raw();
    if code != 0 {
        let error = Error::from_code(code).ok_or(Error::InvalidArgument)?;
        r.finish()?;
        return Err(error);
    }
    let value = match number {
        Number::MapAnon | Number::MapDevice => Return::Addr(r.usize()?),
        Number::DmaAlloc => Return::Dma { addr: r.usize()?, phys: r.u64()? },
        Number::ThreadCreate => Return::Tid(r.u32()?),
        Number::ProcessCreate | Number::EndpointCreate | Number::Mint | Number::BudgetCreate => {
            Return::Handle(Handle::from_raw(r.raw())?)
        }
        Number::BudgetUsage => Return::Usage(Usage {
            pages_limit: r.u64()?,
            pages_usage: r.u64()?,
            processes_limit: r.u32()?,
            processes_usage: r.u32()?,
        }),
        Number::TimeNow => Return::Time(r.u64()?),
        Number::Unmap
        | Number::SetFlags
        | Number::ThreadExit
        | Number::ProcessExit
        | Number::ProcessMap
        | Number::ProcessStart
        | Number::Call
        | Number::Send
        | Number::Receive
        | Number::Reply
        | Number::HandleClose
        | Number::BudgetDestroy
        | Number::Random
        | Number::SystemReset => Return::Nothing,
    };
    r.finish()?;
    Ok(value)
}
