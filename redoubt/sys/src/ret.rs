//! What a call returns, in registers: `a0` = 0 and the value in `a1..=a7`, or `a0` = the error's
//! code and `a1..=a7` = 0. Which [`Return`] a call gives is fixed by its [`Number`].

use crate::regs::{REGS, Reader, Register, Writer};
use crate::{Error, Handle, Number};

/// A successful call's value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Return {
    /// Calls with no value, and those whose result is in a buffer (`call`, `receive`, `random`).
    Nothing,
    /// `map_anon`, `map_device`.
    Addr(usize),
    /// `dma_alloc`: registers addr, phys.
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
    pub pages_used: u64,
    pub processes_limit: u32,
    pub processes_used: u32,
}

/// The shape of a successful result for each call.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Nothing,
    Addr,
    Dma,
    Tid,
    Handle,
    Usage,
    Time,
}

fn shape(number: Number) -> Shape {
    match number {
        Number::MapAnon | Number::MapDevice => Shape::Addr,
        Number::DmaAlloc => Shape::Dma,
        Number::ThreadCreate => Shape::Tid,
        Number::ProcessCreate | Number::EndpointCreate | Number::Mint | Number::BudgetCreate => Shape::Handle,
        Number::BudgetUsage => Shape::Usage,
        Number::TimeNow => Shape::Time,
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
        | Number::SystemReset => Shape::Nothing,
    }
}

/// The registers `a0..=a7` for a call's outcome (kernel side).
pub fn encode_result<R: Register>(result: &Result<Return, Error>) -> [R; REGS] {
    let mut regs = [R::ZERO; REGS];
    write_result(result, &mut Writer::new(&mut regs));
    regs
}

pub(crate) fn write_result<R: Register>(result: &Result<Return, Error>, w: &mut Writer<R>) {
    let value = match result {
        Err(e) => return w.u32(e.code()),
        Ok(value) => value,
    };
    w.u32(0);
    match *value {
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
            w.u64(u.pages_used);
            w.u32(u.processes_limit);
            w.u32(u.processes_used);
        }
        Return::Time(time) => w.u64(time),
    }
}

/// A call's outcome from `a0..=a7` (userspace side), given the call it answers. The kernel is
/// trusted to encode correctly; a malformed result (unknown code, wrong shape, stray non-zero
/// register) still decodes to `Err(InvalidArgument)` rather than panicking.
pub fn decode_result<R: Register>(number: Number, regs: &[R; REGS]) -> Result<Return, Error> {
    let mut r = Reader::new(regs);
    let code = r.raw();
    if code != 0 {
        let error = Error::from_code(code).ok_or(Error::InvalidArgument)?;
        r.finish()?;
        return Err(error);
    }
    let value = match shape(number) {
        Shape::Nothing => Return::Nothing,
        Shape::Addr => Return::Addr(r.usize()?),
        Shape::Dma => Return::Dma { addr: r.usize()?, phys: r.u64() },
        Shape::Tid => Return::Tid(r.u32()?),
        Shape::Handle => Return::Handle(Handle::from_raw(r.raw()).map_err(|_| Error::InvalidArgument)?),
        Shape::Usage => Return::Usage(Usage {
            pages_limit: r.u64(),
            pages_used: r.u64(),
            processes_limit: r.u32()?,
            processes_used: r.u32()?,
        }),
        Shape::Time => Return::Time(r.u64()),
    };
    r.finish()?;
    Ok(value)
}
