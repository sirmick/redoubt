//! Terms on per-process heaps.
//!
//! A [`Term`] is a 16-byte `Copy` value. Numbers, atoms, `[]`, pids and references are held in
//! it directly; anything larger is a [`Ptr`] to an object on a [`Heap`]: a header cell followed by
//! the object's cells (a list cell is two cells with no header). A heap is a `Vec<Term>`, so a
//! pointer is an index: growing the heap moves nothing, and nothing here needs `unsafe`.
//!
//! Bytes of binaries, bignums and resources live off the heap, behind `Arc`s in the heap's
//! off-heap table; an object refers to one by index ([`Term::OffHeap`]).
//!
//! A pointer names a *space* as well as an index: space 0 is the heap the term was read from;
//! any other is a literal chunk ([`Literals`]), immutable and never freed, which every heap may
//! point into. So literals are never copied, even between processes.
//!
//! A term only means something together with the heap it came from. Moving a term to another
//! heap is a copy ([`copy()`]); what is kept outside any process is an [`OwnedTerm`].
//!
//! Nothing recurses on the Rust stack over a term's depth: copying, comparing, printing and
//! collecting use work lists or Cheney's scan.

mod cmp;
mod copy;
mod gc;
mod map;
mod show;
#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::atom::Atom;

pub use cmp::compare;
pub use copy::{copy, OwnedTerm};
pub use gc::Collector;
pub use show::Show;

/// A process identifier: an index into the VM's process table plus a serial number, so a stale
/// pid never names a newer process that reuses the slot.
///
/// A port is a process too (one running the embedded driver `beamlet_port`), marked `port`: to
/// Erlang code it is a port (`#Port<0.N>`, `is_port`, ordered before pids), and links,
/// monitors, exit signals and registered names work for it as they do for any process.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pid {
    pub serial: u32,
    pub index: u32,
    pub port: bool,
}

impl Pid {
    pub const fn process(index: u32, serial: u32) -> Pid {
        Pid {
            serial,
            index,
            port: false,
        }
    }
}

/// A reference, unique within one VM.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ref(pub u64);

/// Where an object is: a space (0 for the heap being read, otherwise a literal chunk) and the
/// index of its first cell there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ptr {
    space: u32,
    index: u32,
}

impl Ptr {
    fn own(index: usize) -> Ptr {
        Ptr {
            space: 0,
            index: u32::try_from(index).expect("a heap is under 2^32 cells"),
        }
    }

    fn at(self) -> usize {
        self.index as usize
    }
}

/// What an object is. The header's `len` is the number of cells after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Tuple,
    /// A map: `[len, root]`, the root a [`Term::Node`] or `[]`.
    Map,
    /// A map's tree node: `[key, value, left, right, height]`.
    Node,
    /// `[OffHeap(bytes), offset in bits, length in bits]`.
    Bits,
    /// `[module, index, arity, uniq, name, env...]`.
    FunLocal,
    /// `[module, function, arity]`.
    FunExport,
    /// `[OffHeap(bignum)]`.
    Big,
    /// `[OffHeap(resource)]`.
    Resource,
    /// A binary match in progress: `[the Bits term, position in bits]` (the position changes in
    /// place).
    Match,
    /// During a collection: the object has moved; `len` is its new index.
    Forward,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    pub kind: Kind,
    pub len: u32,
}

/// An Erlang term, or (the last three variants) a cell of an object that no Erlang code sees.
#[derive(Clone, Copy)]
pub enum Term {
    /// Integer that fits in 64 bits. Arithmetic moves to [`Term::Big`] on overflow and back when
    /// the result fits again, so an integer has exactly one representation.
    Int(i64),
    /// A finite double. NaN and infinities are never stored (arithmetic raises `badarith`).
    Float(f64),
    Atom(Atom),
    /// The empty list `[]`.
    Nil,
    Pid(Pid),
    Ref(Ref),
    Cons(Ptr),
    Tuple(Ptr),
    Map(Ptr),
    /// A binary or bitstring.
    Bits(Ptr),
    Fun(Ptr),
    /// Integer outside the `i64` range. Never holds a value that would fit in [`Term::Int`].
    Big(Ptr),
    /// A native object (a hash state, a cipher context, ...) held by Erlang code. Opaque to
    /// Erlang: it is a reference as far as type tests and ordering go, as in BEAM.
    Resource(Ptr),
    /// A binary match in progress (internal: created by `bs_start_match4`, never visible to
    /// Erlang code as a value, which the compiler guarantees).
    Match(Ptr),
    /// A map's tree node (inside maps only).
    Node(Ptr),
    /// An object's first cell.
    Header(Header),
    /// An entry of the off-heap table of the space holding the object.
    OffHeap(u32),
}

/// Without its heap, a term can only show itself as far as it is held directly.
impl core::fmt::Debug for Term {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Term::Int(i) => write!(f, "{i}"),
            Term::Float(x) => write!(f, "{x:?}"),
            Term::Atom(a) => write!(f, "{a:?}"),
            Term::Nil => f.write_str("[]"),
            Term::Pid(p) => write!(f, "{p:?}"),
            Term::Ref(r) => write!(f, "{r:?}"),
            Term::Header(h) => write!(f, "{h:?}"),
            Term::OffHeap(i) => write!(f, "OffHeap({i})"),
            other => write!(f, "#Object<{:?}>", other.ptr()),
        }
    }
}

/// What an object's cells may refer to outside the heap.
#[derive(Clone)]
pub enum OffHeap {
    Bytes(Arc<Vec<u8>>),
    Big(Arc<BigInt>),
    Resource(Arc<Resource>),
}

impl OffHeap {
    /// Bytes held (for memory accounting).
    fn size(&self) -> usize {
        match self {
            OffHeap::Bytes(b) => b.len(),
            OffHeap::Big(b) => (b.bits() as usize).div_ceil(8),
            OffHeap::Resource(_) => 64,
        }
    }

    /// The address of the shared value: the same for every clone of one `Arc`.
    fn addr(&self) -> usize {
        match self {
            OffHeap::Bytes(b) => Arc::as_ptr(b) as *const u8 as usize,
            OffHeap::Big(b) => Arc::as_ptr(b) as *const u8 as usize,
            OffHeap::Resource(r) => Arc::as_ptr(r) as *const u8 as usize,
        }
    }
}

/// A resource: a unique id (from the VM's reference counter) and the native value. Natives
/// that need to change their state keep it in a [`crate::sync::Lock`] inside `value`.
pub struct Resource {
    pub id: u64,
    pub value: Box<crate::sync::AnyShared>,
}

impl Resource {
    /// The value, if it is a `T`.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
}

/// A bitstring read off a heap: a window of `len` bits starting `offset` bits into shared bytes.
/// Sub-binaries share the bytes of the binary they were matched out of.
#[derive(Clone)]
pub struct Bits {
    /// The bytes. A `Vec` so a binary that nobody else holds can be grown in place (see
    /// `bs_create_bin`'s `private_append`); a window never sees bytes past its own `len`.
    pub data: Arc<Vec<u8>>,
    pub offset: usize,
    pub len: usize,
}

impl Bits {
    pub fn from_bytes(bytes: &[u8]) -> Bits {
        Bits {
            data: Arc::new(bytes.to_vec()),
            offset: 0,
            len: bytes.len() * 8,
        }
    }

    /// Whether this is a binary (a whole number of bytes).
    pub fn is_binary(&self) -> bool {
        self.len.is_multiple_of(8)
    }

    /// Bit `i` of the window (0 = first, most significant).
    pub fn bit(&self, i: usize) -> bool {
        let pos = self.offset + i;
        self.data[pos / 8] & (0x80 >> (pos % 8)) != 0
    }

    /// Byte `i` of the window; the last byte of a bitstring is padded with zero bits.
    pub fn byte(&self, i: usize) -> u8 {
        if self.offset.is_multiple_of(8) && (i + 1) * 8 <= self.len {
            return self.data[self.offset / 8 + i];
        }
        let mut b = 0u8;
        for k in 0..8 {
            let bit = i * 8 + k;
            if bit < self.len && self.bit(bit) {
                b |= 0x80 >> k;
            }
        }
        b
    }

    /// The window as bytes, with the last partial byte zero-padded. Borrows when aligned.
    pub fn to_bytes(&self) -> alloc::borrow::Cow<'_, [u8]> {
        if self.offset.is_multiple_of(8) && self.len.is_multiple_of(8) {
            let start = self.offset / 8;
            alloc::borrow::Cow::Borrowed(&self.data[start..start + self.len / 8])
        } else {
            alloc::borrow::Cow::Owned((0..self.len.div_ceil(8)).map(|i| self.byte(i)).collect())
        }
    }

    /// The sub-window of `len` bits starting `start` bits into this one. Caller checks bounds.
    pub fn slice(&self, start: usize, len: usize) -> Bits {
        debug_assert!(start + len <= self.len);
        Bits {
            data: self.data.clone(),
            offset: self.offset + start,
            len,
        }
    }
}

/// A fun read off a heap.
#[derive(Clone, Copy)]
pub enum FunView<'h> {
    Local {
        module: Atom,
        /// Index into the defining module's fun table.
        index: u32,
        /// Arity of the fun as seen by callers (excludes the free variables).
        arity: u32,
        /// The compiler's hash of the fun's code (from the fun table).
        uniq: u32,
        /// The name of the function implementing it (`'-f/1-fun-0-'`), kept so `fun_info/2`
        /// can still name a fun whose module has since been reloaded or deleted, as BEAM can.
        name: Atom,
        /// The captured free variables.
        env: &'h [Term],
    },
    Export {
        module: Atom,
        function: Atom,
        arity: u32,
    },
}

impl FunView<'_> {
    pub fn arity(&self) -> u32 {
        match self {
            FunView::Local { arity, .. } | FunView::Export { arity, .. } => *arity,
        }
    }
}

/// A literal chunk: objects that never change or die (a module's literals, a persistent term).
/// Its pointers name its own space.
pub struct Chunk {
    terms: Vec<Term>,
    offheap: Vec<OffHeap>,
}

/// The literal chunks, by space (chunk `i` is space `i + 1`). Cloning is cheap: heaps each hold
/// a snapshot, taken when they are made and refreshed when a term is copied into them.
#[derive(Clone, Default)]
pub struct Literals(Arc<Vec<Arc<Chunk>>>);

/// `t`, a term of a heap that has become literal chunk `space`, as a term of that chunk.
pub fn relocate(t: &mut Term, space: u32) {
    if let Some(p) = t.ptr_mut() {
        if p.space == 0 {
            p.space = space;
        }
    }
}

impl Literals {
    fn chunk(&self, space: u32) -> &Chunk {
        &self.0[space as usize - 1]
    }

    /// Whether this snapshot has every chunk `other` has.
    /// How many chunks there are.
    pub fn chunks(&self) -> usize {
        self.0.len()
    }

    fn covers(&self, other: &Literals) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.len() >= other.0.len()
    }

    /// Make the objects of `heap` a literal chunk, and return `roots` (terms of that heap) as
    /// terms that point into it. The heap should hold only what `roots` reach.
    pub fn add(&mut self, heap: Heap, roots: &mut [Term]) -> u32 {
        let space = u32::try_from(self.0.len() + 1).expect("under 2^32 literal chunks");
        let Heap {
            mut terms, offheap, ..
        } = heap;
        let relocate = |t: &mut Term| {
            if let Some(p) = t.ptr_mut() {
                if p.space == 0 {
                    p.space = space;
                }
            }
        };
        terms.iter_mut().for_each(relocate);
        roots.iter_mut().for_each(relocate);
        Arc::make_mut(&mut self.0).push(Arc::new(Chunk { terms, offheap }));
        space
    }

    /// Cells in all chunks (for memory reports).
    pub fn cells(&self) -> usize {
        self.0.iter().map(|c| c.terms.len()).sum()
    }
}

/// A heap: the objects of one process (or of one [`OwnedTerm`]).
pub struct Heap {
    terms: Vec<Term>,
    offheap: Vec<OffHeap>,
    /// Each off-heap value's entry, by its address. One entry per value, however many terms
    /// refer to it (every sub-binary of a buffer), so its bytes are counted once and a buffer
    /// only this heap holds is seen to be unique. An address cannot be reused while it is here:
    /// the entry keeps the value alive.
    offheap_index: BTreeMap<usize, u32>,
    /// Bytes held off the heap by this heap's own table.
    offheap_bytes: usize,
    lits: Literals,
}

impl Term {
    pub fn int(i: i64) -> Term {
        Term::Int(i)
    }

    pub fn is_atom(&self, a: &Atom) -> bool {
        matches!(self, Term::Atom(x) if x == a)
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Term::Int(_) | Term::Big(_) | Term::Float(_))
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Term::Int(_) | Term::Big(_))
    }

    /// Small non-negative integer as `usize`.
    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Term::Int(i) => usize::try_from(*i).ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Term::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The object this term points to, if it points to one.
    pub fn ptr(&self) -> Option<Ptr> {
        match *self {
            Term::Cons(p)
            | Term::Tuple(p)
            | Term::Map(p)
            | Term::Bits(p)
            | Term::Fun(p)
            | Term::Big(p)
            | Term::Resource(p)
            | Term::Match(p)
            | Term::Node(p) => Some(p),
            _ => None,
        }
    }

    fn ptr_mut(&mut self) -> Option<&mut Ptr> {
        match self {
            Term::Cons(p)
            | Term::Tuple(p)
            | Term::Map(p)
            | Term::Bits(p)
            | Term::Fun(p)
            | Term::Big(p)
            | Term::Resource(p)
            | Term::Match(p)
            | Term::Node(p) => Some(p),
            _ => None,
        }
    }

    /// Whether this is a term with parts (so comparing or copying it must look inside).
    fn is_container(&self) -> bool {
        matches!(
            self,
            Term::Cons(_) | Term::Tuple(_) | Term::Map(_) | Term::Fun(_)
        )
    }
}

impl Heap {
    pub fn new(lits: &Literals) -> Heap {
        Heap {
            terms: Vec::new(),
            offheap: Vec::new(),
            offheap_index: BTreeMap::new(),
            offheap_bytes: 0,
            lits: lits.clone(),
        }
    }

    /// A heap with room for `cells` cells.
    pub fn with_capacity(lits: &Literals, cells: usize) -> Heap {
        Heap {
            terms: Vec::with_capacity(cells),
            offheap: Vec::new(),
            offheap_index: BTreeMap::new(),
            offheap_bytes: 0,
            lits: lits.clone(),
        }
    }

    /// Cells in use.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Bytes held off the heap by this heap's own table (binaries, bignums).
    pub fn offheap_bytes(&self) -> usize {
        self.offheap_bytes
    }

    /// Memory in 8-byte words: two per cell, and the off-heap bytes.
    pub fn words(&self) -> u64 {
        (self.terms.len() * 2 + self.offheap_bytes.div_ceil(8)) as u64
    }

    pub fn literals(&self) -> &Literals {
        &self.lits
    }

    /// Use `lits` from now on, if it has more chunks than this heap knows.
    pub fn refresh(&mut self, lits: &Literals) {
        if !self.lits.covers(lits) {
            self.lits = lits.clone();
        }
    }

    // ---- reading ----

    fn space(&self, space: u32) -> (&[Term], &[OffHeap]) {
        if space == 0 {
            (&self.terms, &self.offheap)
        } else {
            let c = self.lits.chunk(space);
            (&c.terms, &c.offheap)
        }
    }

    /// The header and cells of the object at `p`.
    fn object(&self, p: Ptr) -> (Header, &[Term]) {
        let (terms, _) = self.space(p.space);
        let Term::Header(h) = terms[p.at()] else {
            unreachable!("an object starts with a header")
        };
        (h, &terms[p.at() + 1..p.at() + 1 + h.len as usize])
    }

    fn off(&self, space: u32, t: Term) -> &OffHeap {
        let Term::OffHeap(i) = t else {
            unreachable!("an off-heap cell")
        };
        &self.space(space).1[i as usize]
    }

    /// `[Head | Tail]`, if `t` is a list cell.
    pub fn as_cons(&self, t: Term) -> Option<(Term, Term)> {
        let Term::Cons(p) = t else { return None };
        let (terms, _) = self.space(p.space);
        Some((terms[p.at()], terms[p.at() + 1]))
    }

    /// The elements, if `t` is a tuple.
    pub fn as_tuple(&self, t: Term) -> Option<&[Term]> {
        let Term::Tuple(p) = t else { return None };
        Some(self.object(p).1)
    }

    /// The bits, if `t` is a bitstring.
    pub fn as_bits(&self, t: Term) -> Option<Bits> {
        let Term::Bits(p) = t else { return None };
        let cells = self.object(p).1;
        let OffHeap::Bytes(data) = self.off(p.space, cells[0]) else {
            unreachable!("bytes")
        };
        let (Term::Int(offset), Term::Int(len)) = (cells[1], cells[2]) else {
            unreachable!("sizes")
        };
        Some(Bits {
            data: data.clone(),
            offset: offset as usize,
            len: len as usize,
        })
    }

    /// The length in bits, if `t` is a bitstring (without touching the bytes).
    pub fn bit_len(&self, t: Term) -> Option<usize> {
        let Term::Bits(p) = t else { return None };
        let Term::Int(len) = self.object(p).1[2] else {
            unreachable!("sizes")
        };
        Some(len as usize)
    }

    /// The value, if `t` is a bignum.
    pub fn as_big(&self, t: Term) -> Option<&BigInt> {
        let Term::Big(p) = t else { return None };
        let OffHeap::Big(b) = self.off(p.space, self.object(p).1[0]) else {
            unreachable!("bignum")
        };
        Some(b)
    }

    /// The value of an integer, however large.
    pub fn as_bigint(&self, t: Term) -> Option<BigInt> {
        match t {
            Term::Int(i) => Some(BigInt::from(i)),
            Term::Big(_) => self.as_big(t).cloned(),
            _ => None,
        }
    }

    pub fn as_fun(&self, t: Term) -> Option<FunView<'_>> {
        let Term::Fun(p) = t else { return None };
        let (h, cells) = self.object(p);
        let atom = |t: Term| match t {
            Term::Atom(a) => a,
            _ => unreachable!("an atom"),
        };
        let int = |t: Term| match t {
            Term::Int(i) => i as u32,
            _ => unreachable!("an integer"),
        };
        Some(match h.kind {
            Kind::FunLocal => FunView::Local {
                module: atom(cells[0]),
                index: int(cells[1]),
                arity: int(cells[2]),
                uniq: int(cells[3]),
                name: atom(cells[4]),
                env: &cells[5..],
            },
            _ => FunView::Export {
                module: atom(cells[0]),
                function: atom(cells[1]),
                arity: int(cells[2]),
            },
        })
    }

    pub fn as_resource(&self, t: Term) -> Option<&Arc<Resource>> {
        let Term::Resource(p) = t else { return None };
        let OffHeap::Resource(r) = self.off(p.space, self.object(p).1[0]) else {
            unreachable!("resource")
        };
        Some(r)
    }

    /// The bitstring being matched and the position, if `t` is a match state.
    pub fn as_match(&self, t: Term) -> Option<(Term, usize)> {
        let Term::Match(p) = t else { return None };
        let cells = self.object(p).1;
        let Term::Int(pos) = cells[1] else {
            unreachable!("a position")
        };
        Some((cells[0], pos as usize))
    }

    /// Move a match state (which is always on this heap: it is never a literal).
    pub fn set_match_pos(&mut self, t: Term, pos: usize) {
        let Term::Match(p) = t else { return };
        debug_assert_eq!(p.space, 0);
        self.terms[p.at() + 2] = Term::Int(pos as i64);
    }

    /// Iterate over the elements of a list. Yields `Err(tail)` once if the list is improper.
    pub fn list_iter(&self, t: Term) -> ListIter<'_> {
        ListIter { heap: self, cur: t }
    }

    /// The elements of a proper list, or `None` if `t` is not one.
    pub fn to_vec(&self, t: Term) -> Option<Vec<Term>> {
        let mut out = Vec::new();
        for item in self.list_iter(t) {
            out.push(item.ok()?);
        }
        Some(out)
    }

    /// Whether `t` is a proper list (ends in `[]`).
    pub fn is_proper_list(&self, t: Term) -> bool {
        self.list_iter(t).all(|x| x.is_ok())
    }

    /// The bytes of iodata: a binary, or a possibly nested list of bytes and binaries ending in
    /// `[]` or a binary. `None` for anything else (including bitstrings).
    pub fn iodata_bytes(&self, t: Term) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut work = alloc::vec![t];
        while let Some(t) = work.pop() {
            match t {
                Term::Nil => {}
                Term::Bits(_) => {
                    let b = self.as_bits(t)?;
                    if !b.is_binary() {
                        return None;
                    }
                    out.extend_from_slice(&b.to_bytes());
                }
                Term::Cons(_) => {
                    let (head, tail) = self.as_cons(t)?;
                    if !matches!(tail, Term::Nil | Term::Cons(_) | Term::Bits(_)) {
                        return None;
                    }
                    match head {
                        Term::Int(i) if (0..=255).contains(&i) => {
                            out.push(i as u8);
                            work.push(tail);
                        }
                        Term::Nil | Term::Cons(_) | Term::Bits(_) => {
                            work.push(tail);
                            work.push(head);
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        Some(out)
    }

    // ---- building ----

    fn push_object(&mut self, kind: Kind, cells: &[Term]) -> Ptr {
        let at = self.terms.len();
        let len = u32::try_from(cells.len()).expect("an object is under 2^32 cells");
        self.terms.push(Term::Header(Header { kind, len }));
        self.terms.extend_from_slice(cells);
        Ptr::own(at)
    }

    /// The entry for an off-heap value: its existing one if this heap already refers to it.
    fn push_offheap(&mut self, o: OffHeap) -> Term {
        let addr = o.addr();
        // Fast path: slices of one buffer tend to be made one after another.
        if let Some(last) = self.offheap.last() {
            if last.addr() == addr {
                return Term::OffHeap(self.offheap.len() as u32 - 1);
            }
        }
        if let Some(&i) = self.offheap_index.get(&addr) {
            return Term::OffHeap(i);
        }
        self.offheap_bytes += o.size();
        let i = u32::try_from(self.offheap.len()).expect("under 2^32 off-heap entries");
        self.offheap.push(o);
        self.offheap_index.insert(addr, i);
        Term::OffHeap(i)
    }

    pub fn cons(&mut self, head: Term, tail: Term) -> Term {
        let at = self.terms.len();
        self.terms.push(head);
        self.terms.push(tail);
        Term::Cons(Ptr::own(at))
    }

    pub fn tuple(&mut self, elems: &[Term]) -> Term {
        Term::Tuple(self.push_object(Kind::Tuple, elems))
    }

    /// A proper list of `items`.
    pub fn list(
        &mut self,
        items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>,
    ) -> Term {
        self.list_with_tail(items, Term::Nil)
    }

    pub fn list_with_tail(
        &mut self,
        items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>,
        tail: Term,
    ) -> Term {
        items
            .into_iter()
            .rev()
            .fold(tail, |acc, t| self.cons(t, acc))
    }

    /// A string as a list of characters.
    pub fn string(&mut self, s: &str) -> Term {
        let chars: Vec<Term> = s.chars().map(|c| Term::Int(c as i64)).collect();
        self.list(chars)
    }

    pub fn bits(&mut self, b: Bits) -> Term {
        let data = self.push_offheap(OffHeap::Bytes(b.data));
        Term::Bits(self.push_object(
            Kind::Bits,
            &[data, Term::Int(b.offset as i64), Term::Int(b.len as i64)],
        ))
    }

    pub fn binary(&mut self, bytes: &[u8]) -> Term {
        self.bits(Bits::from_bytes(bytes))
    }

    /// An integer: a [`Term::Int`] if it fits, else a bignum.
    pub fn big(&mut self, b: BigInt) -> Term {
        match b.to_i64() {
            Some(i) => Term::Int(i),
            None => {
                let v = self.push_offheap(OffHeap::Big(Arc::new(b)));
                Term::Big(self.push_object(Kind::Big, &[v]))
            }
        }
    }

    pub fn from_i128(&mut self, i: i128) -> Term {
        match i64::try_from(i) {
            Ok(i) => Term::Int(i),
            Err(_) => self.big(BigInt::from(i)),
        }
    }

    pub fn from_u64(&mut self, i: u64) -> Term {
        self.from_i128(i as i128)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fun_local(
        &mut self,
        module: Atom,
        index: u32,
        arity: u32,
        uniq: u32,
        name: Atom,
        env: &[Term],
    ) -> Term {
        let mut cells = alloc::vec![
            Term::Atom(module),
            Term::Int(index as i64),
            Term::Int(arity as i64),
            Term::Int(uniq as i64),
            Term::Atom(name),
        ];
        cells.extend_from_slice(env);
        Term::Fun(self.push_object(Kind::FunLocal, &cells))
    }

    pub fn fun_export(&mut self, module: Atom, function: Atom, arity: u32) -> Term {
        Term::Fun(self.push_object(
            Kind::FunExport,
            &[
                Term::Atom(module),
                Term::Atom(function),
                Term::Int(arity as i64),
            ],
        ))
    }

    pub fn resource(&mut self, r: Resource) -> Term {
        self.resource_shared(Arc::new(r))
    }

    pub fn resource_shared(&mut self, r: Arc<Resource>) -> Term {
        let v = self.push_offheap(OffHeap::Resource(r));
        Term::Resource(self.push_object(Kind::Resource, &[v]))
    }

    /// The bytes of bitstring `t`, taken to append to in place: `Some((entry, bytes, len))` if
    /// `t` is on this heap, its window starts at its bytes' start and ends at their end, and no
    /// one else holds them (other terms of this heap may share the entry: their windows end
    /// earlier, so appending past them changes nothing they see). Put the bytes back with
    /// [`Heap::finish_append`] before anything else reads this heap. This is BEAM's writable
    /// binary: `<<Acc/binary, X>>` in a loop does not copy `Acc` each time.
    pub fn take_for_append(&mut self, t: Term) -> Option<(u32, Vec<u8>, usize)> {
        let Term::Bits(p) = t else { return None };
        if p.space != 0 {
            return None;
        }
        let (Term::OffHeap(i), Term::Int(0), Term::Int(len)) = (
            self.terms[p.at() + 1],
            self.terms[p.at() + 2],
            self.terms[p.at() + 3],
        ) else {
            return None;
        };
        let OffHeap::Bytes(arc) = &mut self.offheap[i as usize] else {
            return None;
        };
        let bytes = Arc::get_mut(arc)?;
        if (len as usize).div_ceil(8) != bytes.len() {
            return None;
        }
        let bytes = core::mem::take(bytes);
        self.offheap_bytes -= bytes.len();
        Some((i, bytes, len as usize))
    }

    /// A bitstring of `len` bits over `bytes`, put back into the entry they were taken from by
    /// [`Heap::take_for_append`].
    pub fn finish_append(&mut self, entry: u32, bytes: Vec<u8>, len: usize) -> Term {
        self.offheap_bytes += bytes.len();
        let OffHeap::Bytes(arc) = &mut self.offheap[entry as usize] else {
            unreachable!("taken for appending")
        };
        *Arc::get_mut(arc).expect("taken for appending") = bytes;
        Term::Bits(self.push_object(
            Kind::Bits,
            &[Term::OffHeap(entry), Term::Int(0), Term::Int(len as i64)],
        ))
    }

    /// A match state over the bitstring `bits`, at `pos`.
    pub fn match_state(&mut self, bits: Term, pos: usize) -> Term {
        Term::Match(self.push_object(Kind::Match, &[bits, Term::Int(pos as i64)]))
    }
}

pub struct ListIter<'h> {
    heap: &'h Heap,
    cur: Term,
}

impl Iterator for ListIter<'_> {
    type Item = Result<Term, Term>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.cur {
            Term::Nil => None,
            Term::Cons(_) => {
                let (head, tail) = self.heap.as_cons(self.cur).expect("a list cell");
                self.cur = tail;
                Some(Ok(head))
            }
            other => {
                self.cur = Term::Nil;
                Some(Err(other))
            }
        }
    }
}
