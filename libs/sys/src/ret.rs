//! Results in registers. IPC call status does not erase ownership or reply validity
//! (kernel/ipc.md, "How a call completes"); its status lives inside [`CallOutcome`], even when it
//! is an error.

use crate::regs::{REGS, Reader, Writer};
use crate::{Error, Handle, MAX_MSG_HANDLES, Number};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LendDisposition {
    None,
    Returned,
    Consumed,
}

/// A call's status and ownership facts, including on error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallOutcome {
    pub status: Result<(), Error>,
    pub lend: LendDisposition,
    pub reply_present: bool,
}

/// Successful closure of an open call. Delivery means record commit, not acknowledgement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplyOutcome {
    pub delivered: bool,
    pub installed: u32,
}

impl ReplyOutcome {
    /// All required positional handle slots were installed in a committed reply.
    pub fn accepted(self, required: u32) -> bool { self.delivered && self.installed & required == required }

    /// Validate against the handle count supplied to this particular reply.
    pub fn validate(self, handles: usize) -> Result<Self, Error> {
        if handles > MAX_MSG_HANDLES
            || self.installed >> handles != 0
            || (!self.delivered && self.installed != 0)
        {
            Err(Error::InvalidArgument)
        } else {
            Ok(self)
        }
    }
}

/// A decoded result. `Call` includes its own status: outer `Ok` means the outcome decoded,
/// not that the IPC succeeded. Other variants describe syscall success.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Return {
    Call(CallOutcome),
    Reply(ReplyOutcome),
    /// Calls with no value, and those whose result is in memory (`receive`,
    /// `budget_usage`).
    Nothing,
    /// `map_anon`.
    Addr(usize),
    /// `map_device`: where the device's registers are, and how many bytes of them. A driver
    /// needs the length to know what it may touch; which device the handle names comes from
    /// the boot manifest, so the kernel says nothing about it.
    /// See kernel/devices.md, `map_device`.
    Mapping {
        addr: usize,
        len: usize,
    },
    /// `dma_alloc`. `phys` is a `u64` on both widths because Sv32 physical addresses are 34
    /// bits.
    Dma {
        addr: usize,
        phys: u64,
    },
    /// `thread_create`.
    Tid(u32),
    /// `process_create`, `endpoint_create`, `mint`, `budget_create`.
    Handle(Handle),
    /// `time_now`: microseconds since boot.
    Time(u64),
    /// `random`: one value from the kernel's CSPRNG.
    Random(u64),
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
                Return::Call(outcome) => {
                    // Status shares a0 with ordinary errors, but its payload is never erased.
                    regs[0] = outcome.status.err().map_or(0, |e| e as u64);
                    regs[1] = match outcome.lend {
                        LendDisposition::None => 0,
                        LendDisposition::Returned => 1,
                        LendDisposition::Consumed => 2,
                    };
                    regs[2] = u64::from(outcome.reply_present);
                }
                Return::Reply(outcome) => {
                    w.u32(u32::from(outcome.delivered));
                    w.u32(outcome.installed);
                }
                Return::Nothing => {}
                Return::Addr(addr) => w.usize(addr),
                Return::Mapping { addr, len } => {
                    w.usize(addr);
                    w.usize(len);
                }
                Return::Dma { addr, phys } => {
                    w.usize(addr);
                    w.u64(phys);
                }
                Return::Tid(tid) => w.u32(tid),
                Return::Handle(h) => w.u32(h.index()),
                Return::Time(time) => w.u64(time),
                Return::Random(value) => w.u64(value),
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
    if number == Number::Call {
        let status =
            if code == 0 { Ok(()) } else { Err(Error::from_code(code).ok_or(Error::InvalidArgument)?) };
        let lend = match r.u32()? {
            0 => LendDisposition::None,
            1 => LendDisposition::Returned,
            2 => LendDisposition::Consumed,
            _ => return Err(Error::InvalidArgument),
        };
        let reply_present = match r.u32()? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidArgument),
        };
        r.finish()?;
        if (reply_present && !matches!(status, Ok(()) | Err(Error::OutOfMemory)))
            || (status.is_ok() && !reply_present)
            || (lend == LendDisposition::Consumed
                && (reply_present || !matches!(status, Err(Error::Timeout | Error::Dead))))
        {
            return Err(Error::InvalidArgument);
        }
        return Ok(Return::Call(CallOutcome { status, lend, reply_present }));
    }
    if code != 0 {
        let error = Error::from_code(code).ok_or(Error::InvalidArgument)?;
        r.finish()?;
        return Err(error);
    }
    let value = match number {
        Number::Reply => {
            let delivered = match r.u32()? {
                0 => false,
                1 => true,
                _ => return Err(Error::InvalidArgument),
            };
            Return::Reply(ReplyOutcome { delivered, installed: r.u32()? }.validate(MAX_MSG_HANDLES)?)
        }
        Number::MapAnon => Return::Addr(r.usize()?),
        // The address and the length (kernel/devices.md, `map_device`).
        Number::MapDevice => Return::Mapping { addr: r.usize()?, len: r.usize()? },
        Number::DmaAlloc => Return::Dma { addr: r.usize()?, phys: r.u64()? },
        Number::ThreadCreate => Return::Tid(r.u32()?),
        Number::ProcessCreate | Number::EndpointCreate | Number::Mint | Number::BudgetCreate => {
            Return::Handle(Handle::from_raw(r.raw())?)
        }
        Number::TimeNow => Return::Time(r.u64()?),
        Number::Random => Return::Random(r.u64()?),
        Number::Unmap
        | Number::SetFlags
        | Number::ThreadExit
        | Number::ProcessExit
        | Number::ProcessMap
        | Number::ProcessStart
        | Number::Send
        | Number::Receive
        | Number::Serve
        | Number::HandleClose
        | Number::BudgetDestroy
        | Number::BudgetUsage
        | Number::SystemReset
        | Number::MapFixed => Return::Nothing,
        Number::Call => unreachable!(),
    };
    r.finish()?;
    Ok(value)
}
