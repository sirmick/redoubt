//! Records: arguments and results too big for registers, as fixed-length arrays of `u64` slots in
//! the caller's memory, the same layout on both widths (crate docs, Records). Each type encodes
//! to and decodes from its array; decoding rejects every malformed array with an error and never
//! panics.

use core::num::{NonZeroU32, NonZeroU64};

use crate::regs::{Reader, Writer};
use crate::{Error, Handle, MAX_LABELS, MAX_MSG_HANDLES, Pages, WORDS};

/// A value that takes one record slot.
pub trait Slot: Copy + Eq {
    /// What fills a [`List`]'s unused tail in memory; never read or encoded.
    const FILL: Self;
    fn to_slot(self) -> u64;
    fn from_slot(raw: u64) -> Result<Self, Error>;
}

impl Slot for u64 {
    const FILL: Self = 0;

    fn to_slot(self) -> u64 { self }

    fn from_slot(raw: u64) -> Result<Self, Error> { Ok(raw) }
}

impl Slot for Handle {
    const FILL: Self = Handle(NonZeroU32::MAX);

    fn to_slot(self) -> u64 { self.to_raw() }

    fn from_slot(raw: u64) -> Result<Self, Error> { Handle::from_raw(raw) }
}

/// At most `N` items. Only the first `len` mean anything: equality compares those, and a
/// record's unused slots are always written as 0.
#[derive(Clone, Copy, Debug)]
pub struct List<T, const N: usize> {
    items: [T; N],
    len: usize,
}

/// The handles a message carries.
pub type Handles = List<Handle, MAX_MSG_HANDLES>;
/// A budget's labels, as sent: the kernel sorts and deduplicates them.
pub type Labels = List<u64, MAX_LABELS>;

impl<T: Slot, const N: usize> List<T, N> {
    pub fn new() -> Self { List { items: [T::FILL; N], len: 0 } }

    /// `TooLarge` if there are more than `N`.
    pub fn from_slice(items: &[T]) -> Result<Self, Error> {
        let mut list = List::new();
        for item in items {
            list.push(*item)?;
        }
        Ok(list)
    }

    /// `TooLarge` if the list is full.
    pub fn push(&mut self, item: T) -> Result<(), Error> {
        *self.items.get_mut(self.len).ok_or(Error::TooLarge)? = item;
        self.len += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[T] { self.items.get(..self.len).unwrap_or(&[]) }

    /// Slots: the count, then `N` items, unused ones 0.
    fn write(&self, w: &mut Writer) {
        w.usize(self.len);
        for index in 0..N {
            w.u64(self.as_slice().get(index).map_or(0, |item| item.to_slot()));
        }
    }

    /// A count above `N` is `TooLarge`; a non-zero unused slot is `InvalidArgument`.
    fn read(r: &mut Reader) -> Result<Self, Error> {
        let len = r.raw();
        if len > N as u64 {
            return Err(Error::TooLarge);
        }
        let mut list = List::new();
        for index in 0..N as u64 {
            let raw = r.raw();
            if index < len {
                list.push(T::from_slot(raw)?)?;
            } else if raw != 0 {
                return Err(Error::InvalidArgument);
            }
        }
        Ok(list)
    }
}

impl<T: Slot, const N: usize> PartialEq for List<T, N> {
    fn eq(&self, other: &Self) -> bool { self.as_slice() == other.as_slice() }
}

impl<T: Slot, const N: usize> Eq for List<T, N> {}

impl<T: Slot, const N: usize> Default for List<T, N> {
    fn default() -> Self { List::new() }
}

/// Slots in a [`Body`]: the words, the handle count, the handles.
pub const BODY_SLOTS: usize = WORDS + 1 + MAX_MSG_HANDLES;

/// What a message carries besides its buffer: sent by `call` (and the reply written back over
/// it), `send` and `reply`, and delivered inside [`Message`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Body {
    pub words: [usize; WORDS],
    pub handles: Handles,
}

impl Body {
    pub fn encode(&self) -> [u64; BODY_SLOTS] {
        let mut slots = [0; BODY_SLOTS];
        self.write(&mut Writer::record(&mut slots));
        slots
    }

    pub fn decode(slots: &[u64; BODY_SLOTS]) -> Result<Body, Error> {
        let mut r = Reader::record(slots);
        let body = Body::read(&mut r)?;
        r.finish()?;
        Ok(body)
    }

    fn write(&self, w: &mut Writer) {
        for word in self.words {
            w.usize(word);
        }
        self.handles.write(w);
    }

    fn read(r: &mut Reader) -> Result<Body, Error> {
        let mut words = [0; WORDS];
        for word in &mut words {
            *word = r.usize()?;
        }
        Ok(Body { words, handles: Handles::read(r)? })
    }
}

/// Slots in a [`Received`] record; the longest kind is a message: kind, id, badge, account,
/// labels (count and `MAX_LABELS`), body, message kind, buffer (address, pages).
pub const RECEIVED_SLOTS: usize = 4 + 1 + MAX_LABELS + BODY_SLOTS + 3;

/// What `receive` returns, written by the kernel to the call's `received_rec`. `Timeout` is an
/// error, not a record. Slot 0 is the kind: 1 message, 2 interrupt, 3 exit notice; then the
/// variant's fields in declaration order; every slot after them is 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Received {
    Message(Message),
    /// The IRQ handle that fired.
    Interrupt(Handle),
    Exit(ExitNotice),
}

/// A delivered message. The kernel attaches the badge, account, labels and id (KERNEL-SPEC.md,
/// Messages); the handles are indices in the receiver's own table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Message {
    pub msg_id: NonZeroU64,
    pub badge: u64,
    pub account: u64,
    pub labels: Labels,
    pub body: Body,
    pub kind: MessageKind,
}

/// How a message was sent, and the buffer it brought, as mapped in the receiver. In slots: the
/// kind (1 call, 2 send), then the [`Pages`] ((0, 0) for none).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageKind {
    /// By `call`: a reply is owed. The lend, if any, returns to the caller at `reply`.
    Call { lend: Option<Pages> },
    /// By `send`: no reply. The transfer, if any, is the receiver's for good.
    Send { transfer: Option<Pages> },
}

/// A process's exit, on the endpoint its creator named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitNotice {
    pub pid: u32,
    pub cause: Cause,
    pub code: u32,
    /// The account the faulting thread was serving; 0 if none.
    pub blamed_account: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    Exited = 1,
    Faulted = 2,
    Killed = 3,
}

impl Received {
    pub fn encode(&self) -> [u64; RECEIVED_SLOTS] {
        let mut slots = [0; RECEIVED_SLOTS];
        let w = &mut Writer::record(&mut slots);
        match self {
            Received::Message(m) => {
                w.u64(1);
                w.u64(m.msg_id.get());
                w.u64(m.badge);
                w.u64(m.account);
                m.labels.write(w);
                m.body.write(w);
                let (kind, pages) = match m.kind {
                    MessageKind::Call { lend } => (1, lend),
                    MessageKind::Send { transfer } => (2, transfer),
                };
                w.u64(kind);
                Pages::write(pages, w);
            }
            Received::Interrupt(irq) => {
                w.u64(2);
                w.u64(irq.to_raw());
            }
            Received::Exit(notice) => {
                w.u64(3);
                w.u32(notice.pid);
                w.u64(notice.cause as u64);
                w.u32(notice.code);
                w.u64(notice.blamed_account);
            }
        }
        slots
    }

    /// Userspace side. The kernel is trusted to write a valid record; decoding still rejects a
    /// malformed one (crate docs, Decoding) rather than panicking.
    pub fn decode(slots: &[u64; RECEIVED_SLOTS]) -> Result<Received, Error> {
        let mut reader = Reader::record(slots);
        let r = &mut reader;
        let received = match r.raw() {
            1 => {
                let msg_id = NonZeroU64::new(r.u64()?).ok_or(Error::InvalidArgument)?;
                let (badge, account) = (r.u64()?, r.u64()?);
                let labels = Labels::read(r)?;
                let body = Body::read(r)?;
                let kind = match (r.raw(), Pages::read(r)?) {
                    (1, lend) => MessageKind::Call { lend },
                    (2, transfer) => MessageKind::Send { transfer },
                    _ => return Err(Error::InvalidArgument),
                };
                Received::Message(Message { msg_id, badge, account, labels, body, kind })
            }
            2 => Received::Interrupt(Handle::from_raw(r.raw())?),
            3 => {
                let pid = r.u32()?;
                let cause = r.tag(&[Cause::Exited, Cause::Faulted, Cause::Killed])?;
                Received::Exit(ExitNotice { pid, cause, code: r.u32()?, blamed_account: r.u64()? })
            }
            _ => return Err(Error::InvalidArgument),
        };
        reader.finish()?;
        Ok(received)
    }
}

/// A budget's class. `User < System` (KERNEL-SPEC.md, Budget).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Class {
    User = 1,
    System = 2,
}

/// Slots in a [`BudgetSpec`]: pages, processes, weight, class, labels (count and `MAX_LABELS`),
/// account, deadline.
pub const BUDGET_SPEC_SLOTS: usize = 4 + 1 + MAX_LABELS + 2;

/// The new budget's fields for `budget_create` (KERNEL-SPEC.md, Budget), in slot order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetSpec {
    /// Page limit.
    pub pages: u64,
    /// Process limit.
    pub processes: u32,
    pub weight: u32,
    pub class: Class,
    pub labels: Labels,
    /// Honoured only when the parent's account is 0 (R8).
    pub account: u64,
    /// Time (µs since boot) at which the kernel destroys the budget; [`FOREVER`](crate::FOREVER)
    /// for none.
    pub deadline: u64,
}

impl BudgetSpec {
    pub fn encode(&self) -> [u64; BUDGET_SPEC_SLOTS] {
        let mut slots = [0; BUDGET_SPEC_SLOTS];
        let w = &mut Writer::record(&mut slots);
        w.u64(self.pages);
        w.u32(self.processes);
        w.u32(self.weight);
        w.u64(self.class as u64);
        self.labels.write(w);
        w.u64(self.account);
        w.u64(self.deadline);
        slots
    }

    /// Kernel side: every malformed spec is an error.
    pub fn decode(slots: &[u64; BUDGET_SPEC_SLOTS]) -> Result<BudgetSpec, Error> {
        let mut r = Reader::record(slots);
        let pages = r.u64()?;
        let processes = r.u32()?;
        let weight = r.u32()?;
        let class = r.tag(&[Class::User, Class::System])?;
        let labels = Labels::read(&mut r)?;
        let spec =
            BudgetSpec { pages, processes, weight, class, labels, account: r.u64()?, deadline: r.u64()? };
        r.finish()?;
        Ok(spec)
    }
}

/// Slots in a [`Usage`] record.
pub const USAGE_SLOTS: usize = 6;

/// What `budget_usage` writes to its `usage_rec`, in slot order (KERNEL-SPEC.md, `budget_usage`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Usage {
    pub pages_limit: u64,
    pub pages_usage: u64,
    pub processes_limit: u32,
    pub processes_usage: u32,
    pub weight_limit: u32,
    /// Weight carved out to children (R7); the free weight is `weight_limit - weight_carved`.
    pub weight_carved: u32,
}

impl Usage {
    pub fn encode(&self) -> [u64; USAGE_SLOTS] {
        let mut slots = [0; USAGE_SLOTS];
        let w = &mut Writer::record(&mut slots);
        w.u64(self.pages_limit);
        w.u64(self.pages_usage);
        w.u32(self.processes_limit);
        w.u32(self.processes_usage);
        w.u32(self.weight_limit);
        w.u32(self.weight_carved);
        slots
    }

    /// Userspace side; a malformed record is an error, never a panic.
    pub fn decode(slots: &[u64; USAGE_SLOTS]) -> Result<Usage, Error> {
        let mut r = Reader::record(slots);
        let usage = Usage {
            pages_limit: r.u64()?,
            pages_usage: r.u64()?,
            processes_limit: r.u32()?,
            processes_usage: r.u32()?,
            weight_limit: r.u32()?,
            weight_carved: r.u32()?,
        };
        r.finish()?;
        Ok(usage)
    }
}
