//! Arguments and results too big for registers: fixed-length arrays of `u64` slots in the
//! caller's memory, the same layout on both widths. Each type encodes to and decodes from its
//! array; decoding rejects every malformed array with an error and never panics.

use crate::regs::{Reader, Writer};
use crate::{Error, Handle, MAX_LABELS, MAX_MSG_HANDLES, Pages, WORDS};

/// At most `N` items. The unused tail always holds `T::default()`, so two lists are equal
/// exactly when their items are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct List<T, const N: usize> {
    items: [T; N],
    len: usize,
}

/// The handles a message carries.
pub type Handles = List<Handle, MAX_MSG_HANDLES>;
/// A budget's labels, as sent: the kernel sorts and deduplicates them.
pub type Labels = List<u64, MAX_LABELS>;

impl<T: Copy + Default, const N: usize> List<T, N> {
    pub fn new() -> Self { List { items: [T::default(); N], len: 0 } }

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
        let slot = self.items.get_mut(self.len).ok_or(Error::TooLarge)?;
        *slot = item;
        self.len += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[T] { self.items.get(..self.len).unwrap_or(&[]) }

    /// Slots: the count, then `N` items (unused ones 0).
    fn write(&self, w: &mut Writer<u64>, to_raw: impl Fn(T) -> u64) {
        w.usize(self.len);
        for item in self.items {
            w.u64(to_raw(item));
        }
    }

    /// A count above `N` is `TooLarge`; a non-zero unused slot is `InvalidArgument`.
    fn read(r: &mut Reader<u64>, from_raw: impl Fn(u64) -> Result<T, Error>) -> Result<Self, Error> {
        let len = r.raw();
        if len > N as u64 {
            return Err(Error::TooLarge);
        }
        let mut list = List::new();
        for index in 0..N as u64 {
            let raw = r.raw();
            if index < len {
                list.push(from_raw(raw)?)?;
            } else if raw != 0 {
                return Err(Error::InvalidArgument);
            }
        }
        Ok(list)
    }
}

impl<T: Copy + Default, const N: usize> Default for List<T, N> {
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
        self.write(&mut Writer::new(&mut slots));
        slots
    }

    pub fn decode(slots: &[u64; BODY_SLOTS]) -> Result<Body, Error> {
        let mut r = Reader::new(slots);
        let body = Body::read(&mut r)?;
        r.finish()?;
        Ok(body)
    }

    fn write(&self, w: &mut Writer<u64>) {
        for word in self.words {
            w.usize(word);
        }
        self.handles.write(w, Handle::to_raw);
    }

    fn read(r: &mut Reader<u64>) -> Result<Body, Error> {
        let mut words = [0; WORDS];
        for word in &mut words {
            *word = r.usize()?;
        }
        Ok(Body { words, handles: Handles::read(r, Handle::from_raw)? })
    }
}

/// Slots in a [`Received`] record; the longest kind is a message: kind, id, badge, account,
/// labels (count and `MAX_LABELS`), body, buffer (kind, address, pages).
pub const RECEIVED_SLOTS: usize = 4 + 1 + MAX_LABELS + BODY_SLOTS + 3;

/// What `receive` returns, written by the kernel to the call's `record`. `Timeout` is an error,
/// not a record. Slot 0 is the kind: 1 message, 2 interrupt, 3 exit notice; then the variant's
/// fields in declaration order; every slot after them is 0.
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
    pub msg_id: u64,
    pub badge: u64,
    pub account: u64,
    pub labels: Labels,
    pub body: Body,
    pub buffer: Option<Buffer>,
}

/// The buffer a message brought, as mapped in the receiver. In slots: kind (0 none, 1 lend,
/// 2 transfer), address, pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Buffer {
    /// Lent by a `call`; returned to the caller by `reply`.
    Lend(Pages),
    /// Transferred by a `send`; the receiver's for good.
    Transfer(Pages),
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
        let w = &mut Writer::new(&mut slots);
        match self {
            Received::Message(m) => {
                w.u64(1);
                w.u64(m.msg_id);
                w.u64(m.badge);
                w.u64(m.account);
                m.labels.write(w, |label| label);
                m.body.write(w);
                let (kind, pages) = match m.buffer {
                    None => (0, Pages { addr: 0, npages: 0 }),
                    Some(Buffer::Lend(pages)) => (1, pages),
                    Some(Buffer::Transfer(pages)) => (2, pages),
                };
                w.u64(kind);
                w.usize(pages.addr);
                w.usize(pages.npages);
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
    /// malformed one (`InvalidArgument`, or `TooLarge`/`BadHandle` for a list or handle) rather
    /// than panicking.
    pub fn decode(slots: &[u64; RECEIVED_SLOTS]) -> Result<Received, Error> {
        let mut reader = Reader::new(slots);
        let r = &mut reader;
        let received = match r.raw() {
            1 => {
                let (msg_id, badge, account) = (r.u64(), r.u64(), r.u64());
                let labels = Labels::read(r, Ok)?;
                let body = Body::read(r)?;
                let kind = r.raw();
                let pages = Pages { addr: r.usize()?, npages: r.usize()? };
                let buffer = match kind {
                    0 if pages == (Pages { addr: 0, npages: 0 }) => None,
                    1 => Some(Buffer::Lend(pages)),
                    2 => Some(Buffer::Transfer(pages)),
                    _ => return Err(Error::InvalidArgument),
                };
                Received::Message(Message { msg_id, badge, account, labels, body, buffer })
            }
            2 => Received::Interrupt(Handle::from_raw(r.raw())?),
            3 => {
                let pid = r.u32()?;
                let cause = match r.raw() {
                    1 => Cause::Exited,
                    2 => Cause::Faulted,
                    3 => Cause::Killed,
                    _ => return Err(Error::InvalidArgument),
                };
                Received::Exit(ExitNotice { pid, cause, code: r.u32()?, blamed_account: r.u64() })
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
        let w = &mut Writer::new(&mut slots);
        w.u64(self.pages);
        w.u32(self.processes);
        w.u32(self.weight);
        w.u64(self.class as u64);
        self.labels.write(w, |label| label);
        w.u64(self.account);
        w.u64(self.deadline);
        slots
    }

    /// Kernel side: every malformed spec is an error.
    pub fn decode(slots: &[u64; BUDGET_SPEC_SLOTS]) -> Result<BudgetSpec, Error> {
        let mut r = Reader::new(slots);
        let pages = r.u64();
        let processes = r.u32()?;
        let weight = r.u32()?;
        let class = match r.raw() {
            1 => Class::User,
            2 => Class::System,
            _ => return Err(Error::InvalidArgument),
        };
        let labels = Labels::read(&mut r, Ok)?;
        let spec =
            BudgetSpec { pages, processes, weight, class, labels, account: r.u64(), deadline: r.u64() };
        r.finish()?;
        Ok(spec)
    }
}
