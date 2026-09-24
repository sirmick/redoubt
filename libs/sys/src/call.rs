//! The system calls and their argument registers (KERNEL-SPEC.md, System calls).

use core::num::{NonZeroU32, NonZeroU64, NonZeroUsize};

use crate::regs::{REGS, Reader, Writer};
use crate::{Error, MAX_START_HANDLES};

/// An index into the calling process's handle table. Index 0 is never allocated: in a register
/// or slot it means "no handle" (KERNEL-SPEC.md, Handle).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle(pub(crate) NonZeroU32);

impl Handle {
    /// `None` for 0, which is never an index.
    pub const fn new(index: u32) -> Option<Handle> {
        match NonZeroU32::new(index) {
            Some(index) => Some(Handle(index)),
            None => None,
        }
    }

    pub const fn index(self) -> u32 { self.0.get() }

    /// The handle as a register or record slot.
    pub const fn to_raw(self) -> u64 { self.0.get() as u64 }

    /// A handle from a register or record slot (the `process_start` list); anything that is not
    /// an index (0, or wider than 32 bits) is `BadHandle`.
    pub fn from_raw(raw: u64) -> Result<Handle, Error> {
        u32::try_from(raw).ok().and_then(Handle::new).ok_or(Error::BadHandle)
    }
}

/// Access to a mapping. Writable and executable together cannot be decoded: this is the one point
/// every call passes through, so refusing W+X here (`InvalidArgument`) backs up the kernel's own
/// check (R11) rather than replacing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemFlags(u32);

impl MemFlags {
    pub const EXECUTE: MemFlags = MemFlags(4);
    pub const NONE: MemFlags = MemFlags(0);
    pub const READ: MemFlags = MemFlags(1);
    pub const WRITE: MemFlags = MemFlags(2);

    pub const fn bits(self) -> u32 { self.0 }

    /// `None` if an unknown bit is set, or both `WRITE` and `EXECUTE`.
    pub const fn from_bits(bits: u32) -> Option<MemFlags> {
        let wx = MemFlags::WRITE.0 | MemFlags::EXECUTE.0;
        if bits & !7 == 0 && bits & wx != wx { Some(MemFlags(bits)) } else { None }
    }
}

impl core::ops::BitOr for MemFlags {
    type Output = MemFlags;

    fn bitor(self, other: MemFlags) -> MemFlags { MemFlags(self.0 | other.0) }
}

/// A page range: a lend (`call`), a transfer (`send`) or what a message brought. Two registers
/// or slots, address then page count; (0, 0) is "none", and exactly one of them 0 is
/// `InvalidArgument`, so each value has one encoding. Alignment and size are the kernel's checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pages {
    pub addr: usize,
    pub npages: NonZeroUsize,
}

impl Pages {
    pub(crate) fn write(pages: Option<Pages>, w: &mut Writer) {
        let (addr, npages) = pages.map_or((0, 0), |p| (p.addr, p.npages.get()));
        w.usize(addr);
        w.usize(npages);
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Option<Pages>, Error> {
        match (r.usize()?, NonZeroUsize::new(r.usize()?)) {
            (0, None) => Ok(None),
            (addr, Some(npages)) if addr != 0 => Ok(Some(Pages { addr, npages })),
            _ => Err(Error::InvalidArgument),
        }
    }
}

/// What `mint` derives the new handle from. Registers: a tag (1 message, 2 handle), then the
/// value as a `u64` (low half, high half).
///
/// Decoding reads the tag register, then both halves, each checked for width when it is read,
/// and only then looks at the tag. So a low half wider than 32 bits (rv64 only) is
/// `InvalidArgument` whatever the tag, and with tag 2 a value that needs the high half is
/// `BadHandle` (a handle wider than 32 bits). The executable model follows this order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MintSource {
    /// The message id of an open call of the caller's thread (a `send`'s id is refused).
    Message(NonZeroU64),
    /// A badge-0 endpoint handle the caller holds.
    Handle(Handle),
}

/// What `system_reset` does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetKind {
    PowerOff = 1,
    Reboot = 2,
}

/// How each kind of argument travels in registers (the table in the crate docs).
trait Arg: Sized {
    fn write(&self, w: &mut Writer);
    fn read(r: &mut Reader) -> Result<Self, Error>;
}

impl Arg for usize {
    fn write(&self, w: &mut Writer) { w.usize(*self) }

    fn read(r: &mut Reader) -> Result<Self, Error> { r.usize() }
}

impl Arg for u32 {
    fn write(&self, w: &mut Writer) { w.u32(*self) }

    fn read(r: &mut Reader) -> Result<Self, Error> { r.u32() }
}

impl Arg for u64 {
    fn write(&self, w: &mut Writer) { w.u64(*self) }

    fn read(r: &mut Reader) -> Result<Self, Error> { r.u64() }
}

/// A badge or a message id: neither is ever 0 (badge 0 is the receive right; message ids start
/// at 1), so 0 does not decode.
impl Arg for NonZeroU64 {
    fn write(&self, w: &mut Writer) { w.u64(self.get()) }

    fn read(r: &mut Reader) -> Result<Self, Error> { NonZeroU64::new(r.u64()?).ok_or(Error::InvalidArgument) }
}

impl Arg for Handle {
    fn write(&self, w: &mut Writer) { w.u32(self.index()) }

    fn read(r: &mut Reader) -> Result<Self, Error> { Handle::from_raw(r.raw()) }
}

impl Arg for Option<Handle> {
    fn write(&self, w: &mut Writer) { w.u32(self.map_or(0, Handle::index)) }

    fn read(r: &mut Reader) -> Result<Self, Error> {
        let raw = r.raw();
        if raw == 0 { Ok(None) } else { Handle::from_raw(raw).map(Some) }
    }
}

impl Arg for MemFlags {
    fn write(&self, w: &mut Writer) { w.u32(self.0) }

    fn read(r: &mut Reader) -> Result<Self, Error> {
        MemFlags::from_bits(r.u32()?).ok_or(Error::InvalidArgument)
    }
}

impl Arg for Option<Pages> {
    fn write(&self, w: &mut Writer) { Pages::write(*self, w) }

    fn read(r: &mut Reader) -> Result<Self, Error> { Pages::read(r) }
}

impl Arg for MintSource {
    fn write(&self, w: &mut Writer) {
        let (tag, value) = match *self {
            MintSource::Message(id) => (1, id.get()),
            MintSource::Handle(h) => (2, h.to_raw()),
        };
        w.u32(tag);
        w.u64(value);
    }

    fn read(r: &mut Reader) -> Result<Self, Error> {
        let tag = r.raw();
        let value = r.u64()?;
        match tag {
            1 => Ok(MintSource::Message(NonZeroU64::new(value).ok_or(Error::InvalidArgument)?)),
            2 => Ok(MintSource::Handle(Handle::from_raw(value)?)),
            _ => Err(Error::InvalidArgument),
        }
    }
}

impl Arg for ResetKind {
    fn write(&self, w: &mut Writer) { w.u32(*self as u32) }

    fn read(r: &mut Reader) -> Result<Self, Error> { r.tag(&[ResetKind::PowerOff, ResetKind::Reboot]) }
}

/// Added to every call's number in the table below. Until WP-K6 deletes the legacy Redoubt calls,
/// the kernel serves both interfaces, and the legacy numbers (0..=46, `redoubt_abi::SysCallNumber`) use
/// the same register, `a0`; numbers from here up are disjoint from them, so the kernel routes a
/// call by `a0` alone. WP-K6 can set this to 0.
pub const NUMBER_BASE: u32 = 0x100;

/// The one table of calls. Each entry gives a [`Number`] variant and its value (plus
/// [`NUMBER_BASE`], it travels in `a0`; 0 is not a call), the spec's name, and the [`Call`] variant's
/// arguments in register order (`a1` first). From it the macro generates `Number`, `Number::ALL`,
/// `Number::name`, `Call`, `Call::number`, `Call::encode` and `Call::decode`.
macro_rules! calls {
    ($( $(#[$doc:meta])* $variant:ident = $number:literal $name:literal
        $({ $($field:ident: $ty:ty),* })? ; )*) => {
        /// The call numbers, in KERNEL-SPEC.md's table order, from [`NUMBER_BASE`] + 1.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u32)]
        pub enum Number { $( $variant = NUMBER_BASE + $number, )* }

        impl Number {
            /// Every call, in number order.
            pub const ALL: [Number; [$($number),*].len()] = [ $( Number::$variant, )* ];

            pub fn from_raw(raw: u64) -> Option<Number> {
                Number::ALL.iter().copied().find(|n| *n as u64 == raw)
            }

            /// The name KERNEL-SPEC.md (and the executable model) uses.
            pub fn name(self) -> &'static str {
                match self { $( Number::$variant => $name, )* }
            }
        }

        /// A system call with its arguments, in register order (`a1` first). Fields named `*_rec`
        /// are addresses of records in the caller's memory (crate docs, Records).
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Call { $( $(#[$doc])* $variant $({ $($field: $ty),* })?, )* }

        impl Call {
            pub fn number(&self) -> Number {
                match self { $( Call::$variant { .. } => Number::$variant, )* }
            }

            /// The registers `a0..=a7` for this call (userspace side).
            pub fn encode(&self) -> [u64; REGS] {
                let mut regs = [0; REGS];
                let w = &mut Writer::regs(&mut regs);
                w.u32(self.number() as u32);
                match self { $( Call::$variant $({ $($field),* })? => { $($( $field.write(w); )*)? } )* }
                regs
            }

            /// The call in registers `a0..=a7` (kernel side). Every malformed encoding is an
            /// error (crate docs, Decoding); what is left is the kernel's to check.
            pub fn decode(regs: &[u64; REGS]) -> Result<Call, Error> {
                let mut r = Reader::regs(regs);
                let number = Number::from_raw(r.raw()).ok_or(Error::InvalidArgument)?;
                let call = match number {
                    $( Number::$variant => Call::$variant $({ $($field: Arg::read(&mut r)?),* })?, )*
                };
                // The one bounded count, `process_start`'s, is its call's last argument, so checking
                // it here, before the unused registers, keeps the first error in register order.
                if let Call::ProcessStart { count, .. } = call {
                    if count as usize > MAX_START_HANDLES {
                        return Err(Error::TooLarge);
                    }
                }
                r.finish()?;
                Ok(call)
            }
        }
    };
}

calls! {
    /// -> `Addr`
    MapAnon = 1 "map_anon" { len: usize, flags: MemFlags };
    Unmap = 2 "unmap" { addr: usize, len: usize };
    SetFlags = 3 "set_flags" { addr: usize, len: usize, flags: MemFlags };
    /// -> `Addr`
    MapDevice = 4 "map_device" { device: Handle };
    /// -> `Dma`
    DmaAlloc = 5 "dma_alloc" { device: Handle, npages: usize };
    /// -> `Tid`
    ThreadCreate = 6 "thread_create" { entry: usize, sp: usize, arg: usize };
    /// Does not return.
    ThreadExit = 7 "thread_exit";
    /// Does not return.
    ProcessExit = 8 "process_exit" { code: u32 };
    /// -> `Handle`
    ProcessCreate = 9 "process_create" { budget: Handle, exit_endpoint: Handle };
    ProcessMap = 10 "process_map" { process: Handle, src: usize, dst: usize, len: usize, flags: MemFlags };
    /// `arg` reaches the child's first thread unchanged, in its first argument register, like
    /// `thread_create`'s (the startup page's address, 0 = none: INIT.md); the kernel does not
    /// check it. `handles_rec` holds `count` slots (at most
    /// [`MAX_START_HANDLES`](crate::MAX_START_HANDLES), else `TooLarge`), one handle each
    /// ([`Handle::from_raw`]), copied into the child's slots 1..=count.
    ProcessStart = 11 "process_start" { process: Handle, entry: usize, sp: usize, arg: usize, handles_rec: usize, count: u32 };
    /// -> `Handle`
    EndpointCreate = 12 "endpoint_create";
    /// -> `Handle`. The badge is never 0 (0 is the receive right).
    Mint = 13 "mint" { source: MintSource, badge: NonZeroU64, budget: Option<Handle> };
    /// `body_rec` is a [`Body`](crate::Body): the request going in, the reply coming out.
    /// `timeout` is relative µs; [`FOREVER`](crate::FOREVER) never expires.
    Call = 14 "call" { endpoint: Handle, body_rec: usize, lend: Option<Pages>, timeout: u64 };
    /// `body_rec` is a [`Body`](crate::Body). `timeout` is relative µs; `FOREVER` never expires.
    Send = 15 "send" { endpoint: Handle, body_rec: usize, transfer: Option<Pages>, timeout: u64 };
    /// `from` is a badge-0 endpoint, an IRQ, or none (sleep). `timeout` is relative µs; `FOREVER`
    /// never expires. `max_transfer` is in pages. The kernel writes a
    /// [`Received`](crate::Received) to `received_rec`: a message, an interrupt, an exit notice
    /// or an abandoned-call notice.
    Receive = 16 "receive" { from: Option<Handle>, timeout: u64, max_transfer: usize, received_rec: usize };
    /// `body_rec` is a [`Body`](crate::Body). `msg_id` must be an open call of the caller's
    /// thread; a reply to a `send`'s message is the kernel's `InvalidArgument`.
    Reply = 17 "reply" { msg_id: NonZeroU64, body_rec: usize };
    /// `msg_id` (an open call of the caller's thread) becomes the thread's current call: the one
    /// a fault blames.
    Serve = 18 "serve" { msg_id: NonZeroU64 };
    HandleClose = 19 "handle_close" { handle: Handle };
    /// `spec_rec` is a [`BudgetSpec`](crate::BudgetSpec). -> `Handle`
    BudgetCreate = 20 "budget_create" { parent: Handle, spec_rec: usize };
    BudgetDestroy = 21 "budget_destroy" { budget: Handle };
    /// The kernel writes a [`Usage`](crate::Usage) to `usage_rec` (six counters do not fit in
    /// the result registers).
    BudgetUsage = 22 "budget_usage" { budget: Handle, usage_rec: usize };
    /// -> `Time`
    TimeNow = 23 "time_now";
    /// -> `Random`: one `u64` from the kernel's CSPRNG.
    Random = 24 "random";
    SystemReset = 25 "system_reset" { device: Handle, kind: ResetKind };
    /// Zeroed pages at exactly `addr`, charged like `map_anon`'s; never replaces a mapping.
    /// Appended last so earlier call numbers keep their values (KERNEL-SPEC.md, R11, answer 172).
    MapFixed = 26 "map_fixed" { addr: usize, len: usize, flags: MemFlags };
}
