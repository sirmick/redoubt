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

/// A handle as it arrives in a message or a reply: 0 is `None`, a handle revoked while its
/// message was in flight (R10), or one a reply's caller could not take (R4). It keeps its slot,
/// so the handles after it keep their positions (servers/wire.md numbers them).
impl Slot for Option<Handle> {
    const FILL: Self = None;

    fn to_slot(self) -> u64 { self.map_or(0, Handle::to_raw) }

    fn from_slot(raw: u64) -> Result<Self, Error> {
        if raw == 0 { Ok(None) } else { Handle::from_raw(raw).map(Some) }
    }
}

/// At most `N` items. Only the first `len` mean anything: equality and `Debug` see those, and a
/// record's unused slots are always written as 0.
#[derive(Clone, Copy)]
pub struct List<T, const N: usize> {
    items: [T; N],
    len: usize,
}

/// The handles a message carries, as sent: every one a handle.
pub type Handles = List<Handle, MAX_MSG_HANDLES>;
/// The handles a message or a reply carries, as received: a slot may be 0 (`None`).
pub type ReceivedHandles = List<Option<Handle>, MAX_MSG_HANDLES>;
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

impl<T: Slot + core::fmt::Debug, const N: usize> core::fmt::Debug for List<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result { self.as_slice().fmt(f) }
}

impl<T: Slot, const N: usize> Default for List<T, N> {
    fn default() -> Self { List::new() }
}

/// Slots in a [`Body`]: the words, the handle count, the handles.
pub const BODY_SLOTS: usize = WORDS + 1 + MAX_MSG_HANDLES;

/// What a message carries besides its buffer, with handles of type `H`: [`Body`] as sent,
/// [`ReceivedBody`] as received.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BodyOf<H: Slot> {
    pub words: [usize; WORDS],
    pub handles: List<H, MAX_MSG_HANDLES>,
}

/// A body as sent: by `call` (the request), `send` and `reply`. Every handle is one; a slot of 0
/// within the count is `BadHandle`.
pub type Body = BodyOf<Handle>;

/// A body as received: inside a [`Message`], and the reply `call` writes back over its request.
/// A handle slot within the count may be 0 (`None`): the handle was revoked while the message was
/// in flight (R10), or, in a reply, did not fit the caller's table (R4: the reply is still
/// delivered, without it, and the `call` returns `OutOfMemory`). Slots past the count are still 0.
pub type ReceivedBody = BodyOf<Option<Handle>>;

impl<H: Slot> Default for BodyOf<H> {
    fn default() -> Self { BodyOf { words: [0; WORDS], handles: List::new() } }
}

impl<H: Slot> BodyOf<H> {
    pub fn encode(&self) -> [u64; BODY_SLOTS] {
        let mut slots = [0; BODY_SLOTS];
        self.write(&mut Writer::record(&mut slots));
        slots
    }

    pub fn decode(slots: &[u64; BODY_SLOTS]) -> Result<Self, Error> {
        let mut r = Reader::record(slots);
        let body = Self::read(&mut r)?;
        r.finish()?;
        Ok(body)
    }

    fn write(&self, w: &mut Writer) {
        for word in self.words {
            w.usize(word);
        }
        self.handles.write(w);
    }

    fn read(r: &mut Reader) -> Result<Self, Error> {
        let mut words = [0; WORDS];
        for word in &mut words {
            *word = r.usize()?;
        }
        Ok(BodyOf { words, handles: List::read(r)? })
    }
}

/// Slots in a [`Received`] record, one layout for every kind (kernel/abi.md, "The receive
/// record"): kind, message id, badge, account, labels (count and `MAX_LABELS`), body (words,
/// handle count, handles), buffer (address, pages).
pub const RECEIVED_SLOTS: usize = 4 + 1 + MAX_LABELS + BODY_SLOTS + 2;

/// The record kinds, in slot 0.
const CALL: u64 = 1;
const SEND: u64 = 2;
const INTERRUPT: u64 = 3;
const EXIT: u64 = 4;
const ABANDONED: u64 = 5;

/// What `receive` returns, written by the kernel to the call's `received_rec`. `Timeout` is an
/// error, not a record.
///
/// Every kind uses the one layout `(kind, msg_id, badge, account, labels, words, handles,
/// buffer, pages)`, and a field a kind does not use is 0 or empty. `kind` is 1 `call`, 2 `send`,
/// 3 `interrupt`, 4 `exit`, 5 `abandoned`:
///
/// | Kind | Fields it fills |
/// | --- | --- |
/// | `call`, `send` | all of them ([`Message`]) |
/// | `interrupt` | none (it arrives only on the IRQ handle `receive` named) |
/// | `exit` | words 0-2: `pid`, `cause`, `code`; `account`: `blamed_account`; `labels`: `blamed_labels` |
/// | `abandoned` | `msg_id`: the abandoned call's id |
///
/// Handles carry no kind here: a handle is checked by use (`WrongObject`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Received {
    Message(Message),
    /// The IRQ handle `receive` named fired.
    Interrupt,
    Exit(ExitNotice),
    /// The open call with this id, held by the receiving thread, was abandoned (R3): its caller
    /// is gone. The thread replies to it to free it; the reply reaches nobody. Returned once, by
    /// the holding thread's next `receive` on the endpoint the call arrived on.
    Abandoned(NonZeroU64),
}

/// A delivered message. The kernel attaches the badge, account, labels and id (kernel/ipc.md,
/// "Messages"); the handles are indices in the receiver's own table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Message {
    pub kind: MessageKind,
    pub msg_id: NonZeroU64,
    pub badge: u64,
    pub account: u64,
    pub labels: Labels,
    /// Words and handles; a handle revoked in flight arrives as `None` (R10).
    pub body: ReceivedBody,
}

/// How a message was sent, and the buffer it brought, as mapped in the receiver: record kind 1
/// or 2, and the [`Pages`] ((0, 0) for none) in the last two slots.
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
    /// For `Faulted`: the account of the sender of the current call of the thread that failed;
    /// 0 when nobody is blamed, and for `Exited` and `Killed`. That is the kernel's rule
    /// (kernel/processes.md R21): the type holds any value, and decoding does not check it.
    pub blamed_account: u64,
    /// That sender's labels; empty whenever nobody is blamed (the kernel's rule, as above).
    pub blamed_labels: Labels,
}

/// Why a process ended, in word 1 of an exit notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    Exited = 1,
    /// A fault, or `process_exit` while the process held open calls.
    Faulted = 2,
    Killed = 3,
}

/// The record's fields as they lie in its slots, before a kind gives them meaning.
#[derive(Default)]
struct Fields {
    kind: u64,
    msg_id: u64,
    badge: u64,
    account: u64,
    labels: Labels,
    words: [u64; WORDS],
    handles: ReceivedHandles,
    pages: Option<Pages>,
}

/// Where the fields lie: `account` is slot 3, word 0 follows the labels.
const ACCOUNT: usize = 3;
const WORD0: usize = 4 + 1 + MAX_LABELS;

impl Fields {
    fn write(&self, w: &mut Writer) {
        w.u64(self.kind);
        w.u64(self.msg_id);
        w.u64(self.badge);
        w.u64(self.account);
        self.labels.write(w);
        for word in self.words {
            w.u64(word);
        }
        self.handles.write(w);
        Pages::write(self.pages, w);
    }

    fn read(r: &mut Reader) -> Result<Fields, Error> {
        let (kind, msg_id, badge, account) = (r.raw(), r.raw(), r.raw(), r.raw());
        let labels = Labels::read(r)?;
        let mut words = [0; WORDS];
        for word in &mut words {
            *word = r.raw();
        }
        Ok(Fields {
            kind,
            msg_id,
            badge,
            account,
            labels,
            words,
            handles: ReceivedHandles::read(r)?,
            pages: Pages::read(r)?,
        })
    }
}

impl Received {
    pub fn encode(&self) -> [u64; RECEIVED_SLOTS] {
        let fields = match *self {
            Received::Message(m) => {
                let (kind, pages) = match m.kind {
                    MessageKind::Call { lend } => (CALL, lend),
                    MessageKind::Send { transfer } => (SEND, transfer),
                };
                Fields {
                    kind,
                    msg_id: m.msg_id.get(),
                    badge: m.badge,
                    account: m.account,
                    labels: m.labels,
                    words: m.body.words.map(|w| w as u64),
                    handles: m.body.handles,
                    pages,
                }
            }
            Received::Interrupt => Fields { kind: INTERRUPT, ..Fields::default() },
            Received::Exit(n) => Fields {
                kind: EXIT,
                account: n.blamed_account,
                labels: n.blamed_labels,
                words: [n.pid.into(), n.cause as u64, n.code.into(), 0],
                ..Fields::default()
            },
            Received::Abandoned(id) => Fields { kind: ABANDONED, msg_id: id.get(), ..Fields::default() },
        };
        let mut slots = [0; RECEIVED_SLOTS];
        fields.write(&mut Writer::record(&mut slots));
        slots
    }

    /// Userspace side. The kernel is trusted to write a valid record; decoding still rejects a
    /// malformed one (crate docs, Decoding) rather than panicking.
    ///
    /// The kind comes first. A notice fills one run of slots (none for an interrupt), and every
    /// slot outside it must be 0, checked before any field is read: a stray value there, a list's
    /// count included, is `InvalidArgument`. Then the fields are read, each checked as its type
    /// requires (a list's count over its capacity `TooLarge`, a handle wider than 32 bits
    /// `BadHandle`, anything else malformed `InvalidArgument`).
    pub fn decode(slots: &[u64; RECEIVED_SLOTS]) -> Result<Received, Error> {
        let filled = match slots[0] {
            CALL | SEND => 1..RECEIVED_SLOTS,
            INTERRUPT => 1..1,
            // `blamed_account`, `blamed_labels`, words 0-2.
            EXIT => ACCOUNT..WORD0 + 3,
            ABANDONED => 1..2,
            _ => return Err(Error::InvalidArgument),
        };
        if slots.iter().enumerate().skip(1).any(|(i, slot)| *slot != 0 && !filled.contains(&i)) {
            return Err(Error::InvalidArgument);
        }
        let mut r = Reader::record(slots);
        let f = Fields::read(&mut r)?;
        r.finish()?;
        let msg_id = NonZeroU64::new(f.msg_id).ok_or(Error::InvalidArgument);
        let u32_of = |raw: u64| u32::try_from(raw).map_err(|_| Error::InvalidArgument);
        Ok(match f.kind {
            CALL | SEND => {
                let mut body = ReceivedBody { words: [0; WORDS], handles: f.handles };
                for (word, raw) in body.words.iter_mut().zip(f.words) {
                    *word = usize::try_from(raw).map_err(|_| Error::InvalidArgument)?;
                }
                let kind = if f.kind == CALL {
                    MessageKind::Call { lend: f.pages }
                } else {
                    MessageKind::Send { transfer: f.pages }
                };
                let (badge, account, labels) = (f.badge, f.account, f.labels);
                Received::Message(Message { kind, msg_id: msg_id?, badge, account, labels, body })
            }
            INTERRUPT => Received::Interrupt,
            EXIT => Received::Exit(ExitNotice {
                pid: u32_of(f.words[0])?,
                cause: match f.words[1] {
                    1 => Cause::Exited,
                    2 => Cause::Faulted,
                    3 => Cause::Killed,
                    _ => return Err(Error::InvalidArgument),
                },
                code: u32_of(f.words[2])?,
                blamed_account: f.account,
                blamed_labels: f.labels,
            }),
            // ABANDONED: the only kind left, the match above having refused the rest.
            _ => Received::Abandoned(msg_id?),
        })
    }
}

/// Slots in a [`BudgetSpec`]: pages, processes, weight, labels (count and `MAX_LABELS`),
/// account, deadline.
pub const BUDGET_SPEC_SLOTS: usize = 3 + 1 + MAX_LABELS + 2;

/// The new budget's fields for `budget_create` (kernel/budgets.md, "The budget object"), in slot
/// order. There is no class: a child's class is its parent's (I8). There is no scheduling flag
/// either: every budget is in the one stride queue, and what runs first is a matter of weight
/// (kernel/scheduling.md R12).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetSpec {
    /// Page limit.
    pub pages: u64,
    /// Process limit.
    pub processes: u32,
    pub weight: u32,
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
        self.labels.write(w);
        w.u64(self.account);
        w.u64(self.deadline);
        slots
    }

    /// Kernel side: every malformed spec is an error, the first in slot order.
    pub fn decode(slots: &[u64; BUDGET_SPEC_SLOTS]) -> Result<BudgetSpec, Error> {
        let mut r = Reader::record(slots);
        let pages = r.u64()?;
        let processes = r.u32()?;
        let weight = r.u32()?;
        let labels = Labels::read(&mut r)?;
        let spec = BudgetSpec { pages, processes, weight, labels, account: r.u64()?, deadline: r.u64()? };
        r.finish()?;
        Ok(spec)
    }
}

/// Slots in a [`Usage`] record.
pub const USAGE_SLOTS: usize = 6;

/// What `budget_usage` writes to its `usage_rec`, in slot order (kernel/budgets.md,
/// `budget_usage`).
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
