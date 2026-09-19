//! Erlang terms.
//!
//! Terms are immutable and cannot form cycles, so reference counting frees them exactly. That is
//! the whole memory-management story: no tracing collector, no per-process heaps, no `unsafe`.
//! The price is speed and some memory, which the design notes accept (see DESIGN.md, "Terms").
//!
//! Terms can nest arbitrarily deep (`{{{...}}}` a million levels down is legal Erlang), so nothing
//! here recurses on the Rust stack: dropping, comparing and printing all use explicit work lists.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use num_bigint::BigInt;
use num_traits::float::FloatCore;
use num_traits::{FromPrimitive, ToPrimitive, Zero};

use crate::atom::Atom;
use crate::pmap::PMap;

/// An Erlang term.
#[derive(Clone)]
pub enum Term {
    /// Integer that fits in 64 bits. Arithmetic moves to [`Term::Big`] on overflow and back when
    /// the result fits again, so an integer has exactly one representation.
    Int(i64),
    /// Integer outside the `i64` range. Never holds a value that would fit in [`Term::Int`].
    Big(Rc<BigInt>),
    /// A finite double. NaN and infinities are never stored (arithmetic raises `badarith`).
    Float(f64),
    Atom(Atom),
    /// The empty list `[]`.
    Nil,
    Cons(Rc<Cons>),
    Tuple(Rc<Tuple>),
    Map(Rc<Map>),
    /// A binary or bitstring.
    Bits(Bits),
    Fun(Rc<Fun>),
    Pid(Pid),
    Ref(Ref),
    /// A binary match in progress (internal: created by `bs_start_match4`, never visible to
    /// Erlang code as a value, which the compiler guarantees).
    Match(Rc<MatchState>),
}

/// The state of a binary match: the bits being matched and how far matching has got.
pub struct MatchState {
    pub bits: Bits,
    pub pos: core::cell::Cell<usize>,
}

/// A list cell `[head | tail]`.
pub struct Cons {
    pub head: Term,
    pub tail: Term,
}

/// The elements of a tuple.
#[derive(Clone)]
pub struct Tuple(Vec<Term>);

impl core::ops::Deref for Tuple {
    type Target = [Term];
    fn deref(&self) -> &[Term] {
        &self.0
    }
}

// ---- dropping without recursion ----
//
// Each container's `Drop` hands its children to `drop_flat`, which empties every child it holds
// the last reference to before letting it go. So a child's own `Drop` always finds it empty, and
// however deep the structure, the Rust stack stays two frames deep.

fn drop_flat(mut work: Vec<Term>) {
    while let Some(t) = work.pop() {
        match t {
            Term::Cons(rc) => {
                if let Ok(mut c) = Rc::try_unwrap(rc) {
                    work.push(core::mem::replace(&mut c.head, Term::Nil));
                    work.push(core::mem::replace(&mut c.tail, Term::Nil));
                }
            }
            Term::Tuple(rc) => {
                if let Ok(mut t) = Rc::try_unwrap(rc) {
                    work.append(&mut t.0);
                }
            }
            Term::Map(rc) => {
                if let Ok(mut m) = Rc::try_unwrap(rc) {
                    for (k, v) in core::mem::take(&mut m.0).into_unique_entries() {
                        work.push(k.0);
                        work.push(v);
                    }
                }
            }
            Term::Fun(rc) => {
                if let Ok(mut f) = Rc::try_unwrap(rc) {
                    if let Fun::Local { env, .. } = &mut f {
                        work.append(env);
                    }
                    drop(f);
                }
            }
            _ => {}
        }
    }
}

impl Drop for Cons {
    fn drop(&mut self) {
        let (h, t) = (core::mem::replace(&mut self.head, Term::Nil), core::mem::replace(&mut self.tail, Term::Nil));
        if matches!(h, Term::Cons(_) | Term::Tuple(_) | Term::Map(_) | Term::Fun(_))
            || matches!(t, Term::Cons(_) | Term::Tuple(_) | Term::Map(_) | Term::Fun(_))
        {
            drop_flat(alloc::vec![h, t]);
        }
    }
}

impl Drop for Tuple {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            drop_flat(core::mem::take(&mut self.0));
        }
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            let mut work = Vec::with_capacity(self.0.len() * 2);
            for (k, v) in core::mem::take(&mut self.0).into_unique_entries() {
                work.push(k.0);
                work.push(v);
            }
            drop_flat(work);
        }
    }
}

impl Drop for Fun {
    fn drop(&mut self) {
        if let Fun::Local { env, .. } = self {
            if !env.is_empty() {
                drop_flat(core::mem::take(env));
            }
        }
    }
}

/// A process identifier: an index into the VM's process table plus a serial number, so a stale
/// pid never names a newer process that reuses the slot.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pid {
    pub serial: u32,
    pub index: u32,
}

/// A reference, unique within one VM.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ref(pub u64);

/// A map, ordered by the exact term order of its keys (see DESIGN.md on iteration order).
/// Persistent: versions share structure, so updating a map others still hold is O(log n).
#[derive(Clone, Default)]
pub struct Map(PMap<MapKey, Term>);

impl Map {
    pub fn new() -> Map {
        Map(PMap::new())
    }
}

impl core::ops::Deref for Map {
    type Target = PMap<MapKey, Term>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for Map {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<(MapKey, Term)> for Map {
    fn from_iter<I: IntoIterator<Item = (MapKey, Term)>>(iter: I) -> Map {
        Map(iter.into_iter().collect())
    }
}

/// A map key: a term ordered by [`Term::cmp_exact`].
#[derive(Clone)]
pub struct MapKey(pub Term);

impl PartialEq for MapKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.cmp_exact(&other.0) == Ordering::Equal
    }
}
impl Eq for MapKey {}
impl PartialOrd for MapKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for MapKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp_exact(&other.0)
    }
}

/// A bitstring: a window of `len` bits starting `offset` bits into shared bytes. Sub-binaries
/// share the bytes of the binary they were matched out of.
#[derive(Clone)]
pub struct Bits {
    pub data: Rc<[u8]>,
    pub offset: usize,
    pub len: usize,
}

impl Bits {
    pub fn from_bytes(bytes: &[u8]) -> Bits {
        Bits { data: Rc::from(bytes), offset: 0, len: bytes.len() * 8 }
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
        Bits { data: self.data.clone(), offset: self.offset + start, len }
    }
}

/// A fun: a closure over a local function, or an external `fun M:F/A`.
pub enum Fun {
    Local {
        module: Atom,
        /// Index into the defining module's fun table.
        index: u32,
        /// Arity of the fun as seen by callers (excludes the free variables).
        arity: u32,
        /// The captured free variables.
        env: Vec<Term>,
        /// The compiler's hash of the fun's code (from the fun table).
        uniq: u32,
    },
    Export { module: Atom, function: Atom, arity: u32 },
}

impl Fun {
    pub fn arity(&self) -> u32 {
        match self {
            Fun::Local { arity, .. } | Fun::Export { arity, .. } => *arity,
        }
    }
}

// ---- construction helpers ----

impl Term {
    pub fn int(i: i64) -> Term {
        Term::Int(i)
    }

    /// Normalize a bignum: values that fit in `i64` become [`Term::Int`].
    pub fn big(b: BigInt) -> Term {
        match b.to_i64() {
            Some(i) => Term::Int(i),
            None => Term::Big(Rc::new(b)),
        }
    }

    pub fn from_i128(i: i128) -> Term {
        match i64::try_from(i) {
            Ok(i) => Term::Int(i),
            Err(_) => Term::Big(Rc::new(BigInt::from(i))),
        }
    }

    pub fn cons(head: Term, tail: Term) -> Term {
        Term::Cons(Rc::new(Cons { head, tail }))
    }

    pub fn tuple(elems: Vec<Term>) -> Term {
        Term::Tuple(Rc::new(Tuple(elems)))
    }

    /// A proper list of `items`.
    pub fn list(items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>) -> Term {
        Term::list_with_tail(items, Term::Nil)
    }

    pub fn list_with_tail(
        items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>,
        tail: Term,
    ) -> Term {
        items.into_iter().rev().fold(tail, |acc, t| Term::cons(t, acc))
    }

    pub fn binary(bytes: &[u8]) -> Term {
        Term::Bits(Bits::from_bytes(bytes))
    }

    pub fn map(map: Map) -> Term {
        Term::Map(Rc::new(map))
    }
}

// ---- inspection helpers ----

impl Term {
    pub fn is_atom(&self, a: &Atom) -> bool {
        matches!(self, Term::Atom(x) if x == a)
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Term::Int(_) | Term::Big(_) | Term::Float(_))
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Term::Int(_) | Term::Big(_))
    }

    /// Iterate over the elements of a list. Yields `Err(tail)` once if the list is improper.
    pub fn list_iter(&self) -> ListIter {
        ListIter { cur: self.clone() }
    }

    /// The elements of a proper list, or `None` if this is not a proper list.
    pub fn to_vec(&self) -> Option<Vec<Term>> {
        let mut out = Vec::new();
        for item in self.list_iter() {
            out.push(item.ok()?);
        }
        Some(out)
    }

    /// Whether this is a proper list (ends in `[]`).
    pub fn is_proper_list(&self) -> bool {
        self.list_iter().all(|x| x.is_ok())
    }

    /// Size in the sense of `erlang:tuple_size/1`.
    pub fn as_tuple(&self) -> Option<&[Term]> {
        match self {
            Term::Tuple(t) => Some(&t[..]),
            _ => None,
        }
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

    pub fn as_bigint(&self) -> Option<BigInt> {
        match self {
            Term::Int(i) => Some(BigInt::from(*i)),
            Term::Big(b) => Some((**b).clone()),
            _ => None,
        }
    }
}

pub struct ListIter {
    cur: Term,
}

impl Iterator for ListIter {
    type Item = Result<Term, Term>;
    fn next(&mut self) -> Option<Self::Item> {
        match core::mem::replace(&mut self.cur, Term::Nil) {
            Term::Nil => None,
            Term::Cons(c) => {
                self.cur = c.tail.clone();
                Some(Ok(c.head.clone()))
            }
            other => Some(Err(other)),
        }
    }
}

// ---- ordering and equality ----

/// Rank of each type in the standard term order:
/// number < atom < reference < fun < port < pid < tuple < map < nil < list < bitstring.
fn type_rank(t: &Term) -> u8 {
    match t {
        Term::Int(_) | Term::Big(_) | Term::Float(_) => 0,
        Term::Atom(_) => 1,
        Term::Ref(_) => 2,
        Term::Fun(_) => 3,
        Term::Pid(_) => 5,
        Term::Tuple(_) => 6,
        Term::Map(_) => 7,
        Term::Nil => 8,
        Term::Cons(_) => 9,
        Term::Bits(_) | Term::Match(_) => 10,
    }
}

impl Term {
    /// Standard term order, comparing numbers by value (`1 == 1.0`), as `<`, `==`, `lists:sort/1`.
    pub fn cmp_term(&self, other: &Term) -> Ordering {
        compare(self, other, false)
    }

    /// Exact term order (`=:=`, map keys): an integer never equals a float, and when their values
    /// are equal the integer sorts first.
    pub fn cmp_exact(&self, other: &Term) -> Ordering {
        compare(self, other, true)
    }

    /// `==`
    pub fn eq_arith(&self, other: &Term) -> bool {
        self.cmp_term(other) == Ordering::Equal
    }

    /// `=:=`
    pub fn eq_exact(&self, other: &Term) -> bool {
        self.cmp_exact(other) == Ordering::Equal
    }
}

/// Compare two terms. Pairs still to compare wait on an explicit stack, in the order Erlang
/// compares them, so deep or long terms cannot exhaust the Rust stack.
fn compare(a: &Term, b: &Term, exact: bool) -> Ordering {
    let mut work: Vec<(Term, Term, bool)> = alloc::vec![(a.clone(), b.clone(), exact)];
    while let Some((a, b, exact)) = work.pop() {
        let (ra, rb) = (type_rank(&a), type_rank(&b));
        if ra != rb {
            return ra.cmp(&rb);
        }
        let o = match (&a, &b) {
            (Term::Cons(x), Term::Cons(y)) => {
                if !Rc::ptr_eq(x, y) {
                    // Head first, then the tail.
                    work.push((x.tail.clone(), y.tail.clone(), exact));
                    work.push((x.head.clone(), y.head.clone(), exact));
                }
                Ordering::Equal
            }
            (Term::Tuple(x), Term::Tuple(y)) => {
                if !Rc::ptr_eq(x, y) && x.len() == y.len() {
                    push_pairs(&mut work, x.iter(), y.iter(), exact);
                }
                x.len().cmp(&y.len())
            }
            (Term::Map(x), Term::Map(y)) => {
                // Size first, then all keys in key order (always exactly), then the values.
                if !Rc::ptr_eq(x, y) && x.len() == y.len() {
                    push_pairs(&mut work, x.values(), y.values(), exact);
                    push_pairs(&mut work, x.keys().map(|k| &k.0), y.keys().map(|k| &k.0), true);
                }
                x.len().cmp(&y.len())
            }
            (Term::Fun(x), Term::Fun(y)) => {
                let o = compare_fun_heads(x, y);
                if o == Ordering::Equal {
                    if let (Fun::Local { env: e1, .. }, Fun::Local { env: e2, .. }) = (&**x, &**y) {
                        push_pairs(&mut work, e1.iter(), e2.iter(), exact);
                    }
                }
                o
            }
            _ => compare_one(&a, &b, exact),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// Push element pairs so that the first pair is compared first.
fn push_pairs<'a>(
    work: &mut Vec<(Term, Term, bool)>,
    xs: impl DoubleEndedIterator<Item = &'a Term> + ExactSizeIterator,
    ys: impl DoubleEndedIterator<Item = &'a Term> + ExactSizeIterator,
    exact: bool,
) {
    for (x, y) in xs.zip(ys).rev() {
        work.push((x.clone(), y.clone(), exact));
    }
}

/// Compare two terms of the same type that contain no other terms.
fn compare_one(a: &Term, b: &Term, exact: bool) -> Ordering {
    match (a, b) {
        (Term::Atom(x), Term::Atom(y)) => x.as_str().cmp(y.as_str()),
        (Term::Ref(x), Term::Ref(y)) => x.cmp(y),
        (Term::Pid(x), Term::Pid(y)) => (x.index, x.serial).cmp(&(y.index, y.serial)),
        (Term::Nil, Term::Nil) => Ordering::Equal,
        (Term::Bits(x), Term::Bits(y)) => compare_bits(x, y),
        (Term::Match(x), Term::Match(y)) => x.pos.get().cmp(&y.pos.get()),
        (Term::Match(_), _) => Ordering::Less,
        (_, Term::Match(_)) => Ordering::Greater,
        _ => compare_numbers(a, b, exact),
    }
}

fn compare_bits(x: &Bits, y: &Bits) -> Ordering {
    // Bit by bit; a prefix sorts first.
    let n = x.len.min(y.len);
    let bytes = n / 8;
    for i in 0..bytes {
        let o = x.byte(i).cmp(&y.byte(i));
        if o != Ordering::Equal {
            return o;
        }
    }
    for i in bytes * 8..n {
        let o = x.bit(i).cmp(&y.bit(i));
        if o != Ordering::Equal {
            return o;
        }
    }
    x.len.cmp(&y.len)
}

/// Compare funs by everything except their captured environments.
fn compare_fun_heads(x: &Fun, y: &Fun) -> Ordering {
    match (x, y) {
        (
            Fun::Export { module: m1, function: f1, arity: a1 },
            Fun::Export { module: m2, function: f2, arity: a2 },
        ) => m1
            .as_str()
            .cmp(m2.as_str())
            .then_with(|| f1.as_str().cmp(f2.as_str()))
            .then(a1.cmp(a2)),
        (Fun::Local { .. }, Fun::Export { .. }) => Ordering::Less,
        (Fun::Export { .. }, Fun::Local { .. }) => Ordering::Greater,
        (
            Fun::Local { module: m1, index: i1, env: e1, .. },
            Fun::Local { module: m2, index: i2, env: e2, .. },
        ) => m1.as_str().cmp(m2.as_str()).then(i1.cmp(i2)).then(e1.len().cmp(&e2.len())),
    }
}

fn compare_numbers(a: &Term, b: &Term, exact: bool) -> Ordering {
    match (a, b) {
        (Term::Int(x), Term::Int(y)) => x.cmp(y),
        (Term::Float(x), Term::Float(y)) => {
            // Erlang has no NaN. Under exact comparison 0.0 and -0.0 differ (-0.0 first).
            if exact {
                x.total_cmp(y)
            } else {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
        }
        (Term::Float(x), _) => {
            let o = cmp_float_int(*x, a_int(b));
            if o == Ordering::Equal && exact {
                Ordering::Greater // integers sort before equal floats
            } else {
                o
            }
        }
        (_, Term::Float(y)) => {
            let o = cmp_float_int(*y, a_int(a)).reverse();
            if o == Ordering::Equal && exact {
                Ordering::Less
            } else {
                o
            }
        }
        _ => a_int(a).cmp(&a_int(b)),
    }
}

fn a_int(t: &Term) -> BigInt {
    t.as_bigint().unwrap_or_else(BigInt::zero)
}

/// Compare a finite float with an integer exactly (no rounding of either side).
fn cmp_float_int(f: f64, i: BigInt) -> Ordering {
    let whole = FloatCore::trunc(f);
    // A finite double's integer part is exactly representable as a BigInt.
    let w = BigInt::from_f64(whole).unwrap_or_else(BigInt::zero);
    match w.cmp(&i) {
        Ordering::Equal => {
            let frac = f - whole;
            frac.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        }
        o => o,
    }
}

// ---- printing (the `~w` format) ----

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_term(f, self)
    }
}

impl fmt::Debug for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_term(f, self)
    }
}

/// What is left to print: terms, and the punctuation between them.
enum Out {
    Term(Term),
    Text(&'static str),
}

fn write_term(f: &mut fmt::Formatter<'_>, t: &Term) -> fmt::Result {
    let mut work = alloc::vec![Out::Term(t.clone())];
    while let Some(item) = work.pop() {
        let t = match item {
            Out::Text(s) => {
                f.write_str(s)?;
                continue;
            }
            Out::Term(t) => t,
        };
        // Compound terms push their parts in reverse, so they come off the stack in order.
        match &t {
            Term::Cons(_) => {
                let mut parts = alloc::vec![Out::Text("[")];
                for (i, item) in t.list_iter().enumerate() {
                    match item {
                        Ok(x) => {
                            if i > 0 {
                                parts.push(Out::Text(","));
                            }
                            parts.push(Out::Term(x));
                        }
                        Err(tail) => {
                            parts.push(Out::Text("|"));
                            parts.push(Out::Term(tail));
                        }
                    }
                }
                parts.push(Out::Text("]"));
                work.extend(parts.into_iter().rev());
            }
            Term::Tuple(elems) => {
                work.push(Out::Text("}"));
                for (i, e) in elems.iter().enumerate().rev() {
                    work.push(Out::Term(e.clone()));
                    if i > 0 {
                        work.push(Out::Text(","));
                    }
                }
                work.push(Out::Text("{"));
            }
            Term::Map(m) => {
                work.push(Out::Text("}"));
                for (i, (k, v)) in m.iter().enumerate().rev() {
                    work.push(Out::Term(v.clone()));
                    work.push(Out::Text(" => "));
                    work.push(Out::Term(k.0.clone()));
                    if i > 0 {
                        work.push(Out::Text(","));
                    }
                }
                work.push(Out::Text("#{"));
            }
            _ => write_leaf(f, &t)?,
        }
    }
    Ok(())
}

/// Print a term that contains no other terms.
fn write_leaf(f: &mut fmt::Formatter<'_>, t: &Term) -> fmt::Result {
    match t {
        Term::Int(i) => write!(f, "{i}"),
        Term::Big(b) => write!(f, "{b}"),
        Term::Float(x) => write_float(f, *x),
        Term::Atom(a) => write_atom(f, a.as_str()),
        Term::Nil => f.write_str("[]"),
        Term::Bits(b) => {
            f.write_str("<<")?;
            let whole = b.len / 8;
            for i in 0..whole {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{}", b.byte(i))?;
            }
            let rest = b.len % 8;
            if rest != 0 {
                let mut v = 0u8;
                for k in 0..rest {
                    v = (v << 1) | b.bit(whole * 8 + k) as u8;
                }
                if whole > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{v}:{rest}")?;
            }
            f.write_str(">>")
        }
        Term::Fun(fun) => match &**fun {
            Fun::Export { module, function, arity } => {
                f.write_str("fun ")?;
                write_atom(f, module.as_str())?;
                f.write_str(":")?;
                write_atom(f, function.as_str())?;
                write!(f, "/{arity}")
            }
            Fun::Local { module, index, uniq, .. } => write!(f, "#Fun<{}.{}.{}>", module.as_str(), index, uniq),
        },
        Term::Pid(p) => write!(f, "<0.{}.{}>", p.index, p.serial),
        Term::Ref(r) => write!(f, "#Ref<0.0.0.{}>", r.0),
        Term::Match(_) => f.write_str("#MatchState<>"),
        Term::Cons(_) | Term::Tuple(_) | Term::Map(_) => unreachable!("containers are handled by write_term"),
    }
}

/// Floats print as the shortest decimal that reads back as the same double, in Erlang's
/// layout: always a `.`, and an exponent only when that is shorter (as `float_to_list(F, [short])`).
fn write_float(f: &mut fmt::Formatter<'_>, x: f64) -> fmt::Result {
    f.write_str(&crate::float::format_short(x))
}

pub(crate) fn atom_needs_quotes(s: &str) -> bool {
    const RESERVED: &[&str] = &[
        "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor", "case",
        "catch", "cond", "div", "else", "end", "fun", "if", "let", "maybe", "not", "of", "or",
        "orelse", "receive", "rem", "try", "when", "xor",
    ];
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || ('ß'..='ÿ').contains(&c) && c != '÷' => {}
        _ => return true,
    }
    if !chars.all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || c == '@'
            || (('À'..='ÿ').contains(&c) && c != '×' && c != '÷')
    }) {
        return true;
    }
    RESERVED.contains(&s)
}

fn write_atom(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    if !atom_needs_quotes(s) {
        return f.write_str(s);
    }
    f.write_str("'")?;
    for c in s.chars() {
        match c {
            '\'' => f.write_str("\\'")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\t' => f.write_str("\\t")?,
            '\r' => f.write_str("\\r")?,
            '\x08' => f.write_str("\\b")?,
            '\x0c' => f.write_str("\\f")?,
            '\x0b' => f.write_str("\\v")?,
            '\x1b' => f.write_str("\\e")?,
            '\x7f' => f.write_str("\\d")?,
            c if (c as u32) < 0x20 => write!(f, "\\^{}", ((c as u8) + 0x40) as char)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    const DEEP: usize = 1_000_000;

    fn nested_tuples(depth: usize) -> Term {
        (0..depth).fold(Term::Nil, |acc, _| Term::tuple(alloc::vec![acc]))
    }

    fn nested_heads(depth: usize) -> Term {
        (0..depth).fold(Term::Nil, |acc, _| Term::cons(acc, Term::Nil))
    }

    fn nested_maps(depth: usize) -> Term {
        (0..depth).fold(Term::Nil, |acc, _| {
            let mut m = Map::new();
            m.insert(MapKey(Term::Int(1)), acc);
            Term::map(m)
        })
    }

    /// A million levels of nesting, which is legal Erlang, must drop, compare and print without
    /// touching the Rust stack more than a few frames deep.
    #[test]
    fn deep_terms_are_handled_iteratively() {
        for make in [nested_tuples, nested_heads, nested_maps] {
            let a = make(DEEP);
            let b = make(DEEP);
            assert!(a.eq_exact(&b));
            assert_ne!(a.cmp_term(&make(DEEP - 1)), Ordering::Equal);
            let printed = a.to_string();
            assert!(printed.len() > DEEP);
            drop(a);
            drop(b);
        }
    }

    #[test]
    fn printing_matches_otp() {
        let t = Term::tuple(alloc::vec![
            Term::list(alloc::vec![Term::Int(1), Term::Int(2)]),
            Term::list_with_tail(alloc::vec![Term::Int(1)], Term::Int(2)),
            Term::tuple(alloc::vec![]),
            Term::Nil,
            Term::map(Map::new()),
        ]);
        assert_eq!(t.to_string(), "{[1,2],[1|2],{},[],#{}}");
    }
}
