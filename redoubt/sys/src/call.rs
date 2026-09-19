//! The system calls and their argument registers (KERNEL-SPEC.md, System calls).

use crate::Error;
use crate::regs::{REGS, Reader, Register, Writer};

/// An index into the calling process's handle table. `u32::MAX` is never an index: in a register
/// it means "no handle".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Handle(u32);

impl Handle {
    const NONE: u64 = u32::MAX as u64;

    /// `None` for `u32::MAX`, which is reserved.
    pub const fn new(index: u32) -> Option<Handle> {
        if index == u32::MAX { None } else { Some(Handle(index)) }
    }

    pub const fn index(self) -> u32 { self.0 }

    /// The handle as a register or buffer slot.
    pub const fn to_raw(self) -> u64 { self.0 as u64 }

    /// A handle from a register or buffer slot (the `process_start` list); anything that is not
    /// an index is `BadHandle`.
    pub fn from_raw(raw: u64) -> Result<Handle, Error> {
        if raw < Handle::NONE { Ok(Handle(raw as u32)) } else { Err(Error::BadHandle) }
    }

    pub(crate) fn raw(handle: Option<Handle>) -> u64 { handle.map_or(Handle::NONE, |h| h.0.into()) }

    pub(crate) fn from_raw_optional(raw: u64) -> Result<Option<Handle>, Error> {
        if raw == Handle::NONE { Ok(None) } else { Handle::from_raw(raw).map(Some) }
    }
}

/// Access to a mapping. The kernel refuses writable and executable together (R11); this type can
/// express it so that the refusal is the kernel's, with the kernel's error.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemFlags(u32);

impl MemFlags {
    const ALL: u32 = 7;
    pub const EXECUTE: MemFlags = MemFlags(4);
    pub const NONE: MemFlags = MemFlags(0);
    pub const READ: MemFlags = MemFlags(1);
    pub const WRITE: MemFlags = MemFlags(2);

    pub const fn bits(self) -> u32 { self.0 }

    /// `None` if any unknown bit is set.
    pub const fn from_bits(bits: u32) -> Option<MemFlags> {
        if bits & !MemFlags::ALL == 0 { Some(MemFlags(bits)) } else { None }
    }

    pub const fn contains(self, other: MemFlags) -> bool { self.0 & other.0 == other.0 }
}

impl core::ops::BitOr for MemFlags {
    type Output = MemFlags;

    fn bitor(self, other: MemFlags) -> MemFlags { MemFlags(self.0 | other.0) }
}

/// A page range: a lend (`call`) or a transfer (`send`). In registers, `None` is (0, 0), so
/// `Some` of (0, 0) is sent as `None`. Alignment and size are the kernel's checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pages {
    pub addr: usize,
    pub npages: usize,
}

/// What `mint` derives the new handle from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MintSource {
    /// A message id the caller is serving.
    Message(u64),
    /// A badge-0 endpoint handle the caller holds.
    Handle(Handle),
}

/// What `system_reset` does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetKind {
    PowerOff = 1,
    Reboot = 2,
}

/// The call numbers, in KERNEL-SPEC.md's table order. They travel in `a0`; 0 is not a call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Number {
    MapAnon = 1,
    Unmap = 2,
    SetFlags = 3,
    MapDevice = 4,
    DmaAlloc = 5,
    ThreadCreate = 6,
    ThreadExit = 7,
    ProcessExit = 8,
    ProcessCreate = 9,
    ProcessMap = 10,
    ProcessStart = 11,
    EndpointCreate = 12,
    Mint = 13,
    Call = 14,
    Send = 15,
    Receive = 16,
    Reply = 17,
    HandleClose = 18,
    BudgetCreate = 19,
    BudgetDestroy = 20,
    BudgetUsage = 21,
    TimeNow = 22,
    Random = 23,
    SystemReset = 24,
}

impl Number {
    /// Every call, in number order.
    pub const ALL: [Number; 24] = [
        Number::MapAnon,
        Number::Unmap,
        Number::SetFlags,
        Number::MapDevice,
        Number::DmaAlloc,
        Number::ThreadCreate,
        Number::ThreadExit,
        Number::ProcessExit,
        Number::ProcessCreate,
        Number::ProcessMap,
        Number::ProcessStart,
        Number::EndpointCreate,
        Number::Mint,
        Number::Call,
        Number::Send,
        Number::Receive,
        Number::Reply,
        Number::HandleClose,
        Number::BudgetCreate,
        Number::BudgetDestroy,
        Number::BudgetUsage,
        Number::TimeNow,
        Number::Random,
        Number::SystemReset,
    ];

    pub fn from_raw(raw: u64) -> Option<Number> { Number::ALL.iter().copied().find(|n| *n as u64 == raw) }

    /// The name KERNEL-SPEC.md (and the executable model) uses.
    pub fn name(self) -> &'static str {
        match self {
            Number::MapAnon => "map_anon",
            Number::Unmap => "unmap",
            Number::SetFlags => "set_flags",
            Number::MapDevice => "map_device",
            Number::DmaAlloc => "dma_alloc",
            Number::ThreadCreate => "thread_create",
            Number::ThreadExit => "thread_exit",
            Number::ProcessExit => "process_exit",
            Number::ProcessCreate => "process_create",
            Number::ProcessMap => "process_map",
            Number::ProcessStart => "process_start",
            Number::EndpointCreate => "endpoint_create",
            Number::Mint => "mint",
            Number::Call => "call",
            Number::Send => "send",
            Number::Receive => "receive",
            Number::Reply => "reply",
            Number::HandleClose => "handle_close",
            Number::BudgetCreate => "budget_create",
            Number::BudgetDestroy => "budget_destroy",
            Number::BudgetUsage => "budget_usage",
            Number::TimeNow => "time_now",
            Number::Random => "random",
            Number::SystemReset => "system_reset",
        }
    }
}

/// A system call with its arguments, in register order (`a1` first). Fields named `*_buf` or
/// `record` are addresses of buffers in the caller's memory (see the crate docs for layouts).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Call {
    /// -> [`Return::Addr`](crate::Return::Addr)
    MapAnon {
        len: usize,
        flags: MemFlags,
    },
    Unmap {
        addr: usize,
        len: usize,
    },
    SetFlags {
        addr: usize,
        len: usize,
        flags: MemFlags,
    },
    /// -> [`Return::Addr`](crate::Return::Addr)
    MapDevice {
        device: Handle,
    },
    /// -> [`Return::Dma`](crate::Return::Dma)
    DmaAlloc {
        device: Handle,
        npages: usize,
    },
    /// -> [`Return::Tid`](crate::Return::Tid)
    ThreadCreate {
        entry: usize,
        sp: usize,
        arg: usize,
    },
    /// Does not return.
    ThreadExit,
    /// Does not return.
    ProcessExit {
        code: u32,
    },
    /// -> [`Return::Handle`](crate::Return::Handle)
    ProcessCreate {
        budget: Handle,
        exit_endpoint: Handle,
    },
    ProcessMap {
        process: Handle,
        src: usize,
        dst: usize,
        len: usize,
        flags: MemFlags,
    },
    /// `handles_buf` holds `count` slots, one handle each, copied into the child's slots 1..=count.
    ProcessStart {
        process: Handle,
        entry: usize,
        sp: usize,
        handles_buf: usize,
        count: usize,
    },
    /// -> [`Return::Handle`](crate::Return::Handle)
    EndpointCreate,
    /// -> [`Return::Handle`](crate::Return::Handle). Registers: source tag (1 message, 2
    /// handle), source value as a `u64`, badge, budget.
    Mint {
        source: MintSource,
        badge: u64,
        budget: Option<Handle>,
    },
    /// `body_buf` is a [`Body`](crate::Body): the request going in, the reply coming out.
    Call {
        endpoint: Handle,
        body_buf: usize,
        lend: Option<Pages>,
        timeout: u64,
    },
    /// `body_buf` is a [`Body`](crate::Body).
    Send {
        endpoint: Handle,
        body_buf: usize,
        transfer: Option<Pages>,
        timeout: u64,
    },
    /// `from` is a badge-0 endpoint, an IRQ, or none (sleep). `max_transfer` is in pages. The
    /// kernel writes a [`Received`](crate::Received) to `record`.
    Receive {
        from: Option<Handle>,
        timeout: u64,
        max_transfer: usize,
        record: usize,
    },
    /// `body_buf` is a [`Body`](crate::Body).
    Reply {
        msg_id: u64,
        body_buf: usize,
    },
    HandleClose {
        handle: Handle,
    },
    /// `spec_buf` is a [`BudgetSpec`](crate::BudgetSpec). -> [`Return::Handle`](crate::Return::Handle)
    BudgetCreate {
        parent: Handle,
        spec_buf: usize,
    },
    BudgetDestroy {
        budget: Handle,
    },
    /// -> [`Return::Usage`](crate::Return::Usage)
    BudgetUsage {
        budget: Handle,
    },
    /// -> [`Return::Time`](crate::Return::Time)
    TimeNow,
    /// The kernel writes `len` random bytes to `buf`.
    Random {
        buf: usize,
        len: usize,
    },
    SystemReset {
        device: Handle,
        kind: ResetKind,
    },
}

impl Call {
    pub fn number(&self) -> Number {
        match self {
            Call::MapAnon { .. } => Number::MapAnon,
            Call::Unmap { .. } => Number::Unmap,
            Call::SetFlags { .. } => Number::SetFlags,
            Call::MapDevice { .. } => Number::MapDevice,
            Call::DmaAlloc { .. } => Number::DmaAlloc,
            Call::ThreadCreate { .. } => Number::ThreadCreate,
            Call::ThreadExit => Number::ThreadExit,
            Call::ProcessExit { .. } => Number::ProcessExit,
            Call::ProcessCreate { .. } => Number::ProcessCreate,
            Call::ProcessMap { .. } => Number::ProcessMap,
            Call::ProcessStart { .. } => Number::ProcessStart,
            Call::EndpointCreate => Number::EndpointCreate,
            Call::Mint { .. } => Number::Mint,
            Call::Call { .. } => Number::Call,
            Call::Send { .. } => Number::Send,
            Call::Receive { .. } => Number::Receive,
            Call::Reply { .. } => Number::Reply,
            Call::HandleClose { .. } => Number::HandleClose,
            Call::BudgetCreate { .. } => Number::BudgetCreate,
            Call::BudgetDestroy { .. } => Number::BudgetDestroy,
            Call::BudgetUsage { .. } => Number::BudgetUsage,
            Call::TimeNow => Number::TimeNow,
            Call::Random { .. } => Number::Random,
            Call::SystemReset { .. } => Number::SystemReset,
        }
    }

    /// The registers `a0..=a7` for this call (userspace side).
    pub fn encode<R: Register>(&self) -> [R; REGS] {
        let mut regs = [R::ZERO; REGS];
        self.write(&mut Writer::new(&mut regs));
        regs
    }

    pub(crate) fn write<R: Register>(&self, w: &mut Writer<R>) {
        w.u32(self.number() as u32);
        let handle = |w: &mut Writer<R>, h: Handle| w.u32(h.0);
        let pages = |w: &mut Writer<R>, p: Option<Pages>| {
            let p = p.unwrap_or(Pages { addr: 0, npages: 0 });
            w.usize(p.addr);
            w.usize(p.npages);
        };
        match *self {
            Call::MapAnon { len, flags } => {
                w.usize(len);
                w.u32(flags.0);
            }
            Call::Unmap { addr, len } => {
                w.usize(addr);
                w.usize(len);
            }
            Call::SetFlags { addr, len, flags } => {
                w.usize(addr);
                w.usize(len);
                w.u32(flags.0);
            }
            Call::MapDevice { device } => handle(w, device),
            Call::DmaAlloc { device, npages } => {
                handle(w, device);
                w.usize(npages);
            }
            Call::ThreadCreate { entry, sp, arg } => {
                w.usize(entry);
                w.usize(sp);
                w.usize(arg);
            }
            Call::ThreadExit | Call::EndpointCreate | Call::TimeNow => {}
            Call::ProcessExit { code } => w.u32(code),
            Call::ProcessCreate { budget, exit_endpoint } => {
                handle(w, budget);
                handle(w, exit_endpoint);
            }
            Call::ProcessMap { process, src, dst, len, flags } => {
                handle(w, process);
                w.usize(src);
                w.usize(dst);
                w.usize(len);
                w.u32(flags.0);
            }
            Call::ProcessStart { process, entry, sp, handles_buf, count } => {
                handle(w, process);
                w.usize(entry);
                w.usize(sp);
                w.usize(handles_buf);
                w.usize(count);
            }
            Call::Mint { source, badge, budget } => {
                match source {
                    MintSource::Message(id) => {
                        w.u32(1);
                        w.u64(id);
                    }
                    MintSource::Handle(h) => {
                        w.u32(2);
                        w.u64(h.0.into());
                    }
                }
                w.u64(badge);
                w.u32(Handle::raw(budget) as u32);
            }
            Call::Call { endpoint, body_buf, lend: buffer, timeout }
            | Call::Send { endpoint, body_buf, transfer: buffer, timeout } => {
                handle(w, endpoint);
                w.usize(body_buf);
                pages(w, buffer);
                w.u64(timeout);
            }
            Call::Receive { from, timeout, max_transfer, record } => {
                w.u32(Handle::raw(from) as u32);
                w.u64(timeout);
                w.usize(max_transfer);
                w.usize(record);
            }
            Call::Reply { msg_id, body_buf } => {
                w.u64(msg_id);
                w.usize(body_buf);
            }
            Call::HandleClose { handle: h }
            | Call::BudgetDestroy { budget: h }
            | Call::BudgetUsage { budget: h } => handle(w, h),
            Call::BudgetCreate { parent, spec_buf } => {
                handle(w, parent);
                w.usize(spec_buf);
            }
            Call::Random { buf, len } => {
                w.usize(buf);
                w.usize(len);
            }
            Call::SystemReset { device, kind } => {
                handle(w, device);
                w.u32(kind as u32);
            }
        }
    }

    /// The call in registers `a0..=a7` (kernel side). Every malformed encoding is an error; see
    /// the crate docs for what is checked here and what is left to the kernel.
    pub fn decode<R: Register>(regs: &[R; REGS]) -> Result<Call, Error> {
        let mut r = Reader::new(regs);
        let number = Number::from_raw(r.raw()).ok_or(Error::InvalidArgument)?;
        let handle = |r: &mut Reader<R>| Handle::from_raw(r.raw());
        let flags = |r: &mut Reader<R>| MemFlags::from_bits(r.u32()?).ok_or(Error::InvalidArgument);
        let pages = |r: &mut Reader<R>| -> Result<Option<Pages>, Error> {
            let p = Pages { addr: r.usize()?, npages: r.usize()? };
            Ok(if p.addr == 0 && p.npages == 0 { None } else { Some(p) })
        };
        let call = match number {
            Number::MapAnon => Call::MapAnon { len: r.usize()?, flags: flags(&mut r)? },
            Number::Unmap => Call::Unmap { addr: r.usize()?, len: r.usize()? },
            Number::SetFlags => Call::SetFlags { addr: r.usize()?, len: r.usize()?, flags: flags(&mut r)? },
            Number::MapDevice => Call::MapDevice { device: handle(&mut r)? },
            Number::DmaAlloc => Call::DmaAlloc { device: handle(&mut r)?, npages: r.usize()? },
            Number::ThreadCreate => Call::ThreadCreate { entry: r.usize()?, sp: r.usize()?, arg: r.usize()? },
            Number::ThreadExit => Call::ThreadExit,
            Number::ProcessExit => Call::ProcessExit { code: r.u32()? },
            Number::ProcessCreate => {
                Call::ProcessCreate { budget: handle(&mut r)?, exit_endpoint: handle(&mut r)? }
            }
            Number::ProcessMap => Call::ProcessMap {
                process: handle(&mut r)?,
                src: r.usize()?,
                dst: r.usize()?,
                len: r.usize()?,
                flags: flags(&mut r)?,
            },
            Number::ProcessStart => Call::ProcessStart {
                process: handle(&mut r)?,
                entry: r.usize()?,
                sp: r.usize()?,
                handles_buf: r.usize()?,
                count: r.usize()?,
            },
            Number::EndpointCreate => Call::EndpointCreate,
            Number::Mint => {
                let tag = r.raw();
                let value = r.u64();
                let source = match tag {
                    1 => MintSource::Message(value),
                    2 => MintSource::Handle(Handle::from_raw(value)?),
                    _ => return Err(Error::InvalidArgument),
                };
                Call::Mint { source, badge: r.u64(), budget: Handle::from_raw_optional(r.raw())? }
            }
            Number::Call => Call::Call {
                endpoint: handle(&mut r)?,
                body_buf: r.usize()?,
                lend: pages(&mut r)?,
                timeout: r.u64(),
            },
            Number::Send => Call::Send {
                endpoint: handle(&mut r)?,
                body_buf: r.usize()?,
                transfer: pages(&mut r)?,
                timeout: r.u64(),
            },
            Number::Receive => Call::Receive {
                from: Handle::from_raw_optional(r.raw())?,
                timeout: r.u64(),
                max_transfer: r.usize()?,
                record: r.usize()?,
            },
            Number::Reply => Call::Reply { msg_id: r.u64(), body_buf: r.usize()? },
            Number::HandleClose => Call::HandleClose { handle: handle(&mut r)? },
            Number::BudgetCreate => Call::BudgetCreate { parent: handle(&mut r)?, spec_buf: r.usize()? },
            Number::BudgetDestroy => Call::BudgetDestroy { budget: handle(&mut r)? },
            Number::BudgetUsage => Call::BudgetUsage { budget: handle(&mut r)? },
            Number::TimeNow => Call::TimeNow,
            Number::Random => Call::Random { buf: r.usize()?, len: r.usize()? },
            Number::SystemReset => {
                let device = handle(&mut r)?;
                let kind = match r.raw() {
                    1 => ResetKind::PowerOff,
                    2 => ResetKind::Reboot,
                    _ => return Err(Error::InvalidArgument),
                };
                Call::SystemReset { device, kind }
            }
        };
        r.finish()?;
        Ok(call)
    }
}
