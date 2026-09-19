//! ETS: tables shared between the processes of one VM, and match specifications.
//!
//! A table is a `BTreeMap` from key to objects. `set`s hold one object per key, `bag`s a list.
//! Keys are ordered exactly (`=:=`) except in `ordered_set`, which like BEAM treats `1` and `1.0`
//! as the same key. Iteration (`first`/`next`, `tab2list`, `select`) is in key order for every
//! table type; BEAM's order for hashed tables is unspecified.
//!
//! Access is enforced: a `private` table is usable only by its owner, a `protected` one is
//! readable by all but writable only by the owner. When the owner dies the table is deleted, or
//! given to its heir.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::atom::Atom;
use crate::term::{Pid, Term};

/// Most tables one VM may have (as BEAM's default `ERL_MAX_ETS_TABLES`, roughly).
pub const MAX_TABLES: usize = 8192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Set,
    OrderedSet,
    Bag,
    DuplicateBag,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Public,
    Protected,
    Private,
}

/// A key, ordered exactly or (for `ordered_set`) arithmetically.
#[derive(Clone)]
pub struct Key {
    pub term: Term,
    arith: bool,
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.arith {
            self.term.cmp_term(&other.term)
        } else {
            self.term.cmp_exact(&other.term)
        }
    }
}

pub struct Table {
    /// The reference that identifies an unnamed table (also kept for named ones).
    pub tid: u64,
    pub name: Atom,
    pub named: bool,
    pub kind: Kind,
    pub access: Access,
    /// 1-based position of the key in each object.
    pub keypos: usize,
    pub owner: Pid,
    pub heir: Option<(Pid, Term)>,
    objects: BTreeMap<Key, Vec<Term>>,
    count: usize,
    /// Memory the objects hold, in words (see [`weigh`]).
    words: u64,
}

/// The words an object costs a table: what `memory::Meter` finds in it.
pub fn weigh(obj: &Term) -> u64 {
    let mut m = crate::memory::Meter::new(u64::MAX);
    m.add(obj);
    m.usage().total_words()
}

fn weigh_all(objs: &[Term]) -> u64 {
    objs.iter().map(weigh).sum()
}

impl Table {
    pub fn key(&self, t: Term) -> Key {
        Key { term: t, arith: self.kind == Kind::OrderedSet }
    }

    /// The key of `obj`, if it is a tuple long enough to have one.
    pub fn key_of(&self, obj: &Term) -> Option<Key> {
        obj.as_tuple().and_then(|t| t.get(self.keypos - 1)).map(|k| self.key(k.clone()))
    }

    pub fn size(&self) -> usize {
        self.count
    }

    /// Memory the objects hold, in words.
    pub fn words(&self) -> u64 {
        self.words
    }

    pub fn may_read(&self, who: Pid) -> bool {
        self.access != Access::Private || who == self.owner
    }

    pub fn may_write(&self, who: Pid) -> bool {
        self.access == Access::Public || who == self.owner
    }

    /// Insert one object (already checked to have a key).
    pub fn insert(&mut self, key: Key, obj: Term) {
        let slot = self.objects.entry(key).or_default();
        let w = weigh(&obj);
        match self.kind {
            Kind::Set | Kind::OrderedSet => {
                self.count += 1 - slot.len();
                self.words = self.words.saturating_sub(weigh_all(slot)) + w;
                *slot = alloc::vec![obj];
            }
            Kind::Bag => {
                if !slot.iter().any(|o| o.eq_exact(&obj)) {
                    slot.push(obj);
                    self.count += 1;
                    self.words += w;
                }
            }
            Kind::DuplicateBag => {
                slot.push(obj);
                self.count += 1;
                self.words += w;
            }
        }
    }

    pub fn lookup(&self, key: &Key) -> &[Term] {
        self.objects.get(key).map(|v| &v[..]).unwrap_or(&[])
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.objects.contains_key(key)
    }

    pub fn remove(&mut self, key: &Key) -> Vec<Term> {
        let removed = self.objects.remove(key).unwrap_or_default();
        self.count -= removed.len();
        self.words = self.words.saturating_sub(weigh_all(&removed));
        removed
    }

    /// Remove objects exactly equal to `obj`. Returns how many went.
    pub fn remove_object(&mut self, key: &Key, obj: &Term) -> usize {
        let Some(slot) = self.objects.get_mut(key) else { return 0 };
        let before = slot.len();
        let mut freed = 0;
        slot.retain(|o| {
            let keep = !o.eq_exact(obj);
            if !keep {
                freed += weigh(o);
            }
            keep
        });
        self.words = self.words.saturating_sub(freed);
        let gone = before - slot.len();
        if slot.is_empty() {
            self.objects.remove(key);
        }
        self.count -= gone;
        gone
    }

    /// Replace the (single) object under `key`; the key itself must not change.
    pub fn replace(&mut self, key: &Key, obj: Term) {
        if let Some(slot) = self.objects.get_mut(key) {
            self.words = self.words.saturating_sub(weigh_all(slot)) + weigh(&obj);
            *slot = alloc::vec![obj];
        }
    }

    pub fn clear(&mut self) -> usize {
        let n = self.count;
        self.objects.clear();
        self.count = 0;
        self.words = 0;
        n
    }

    /// Every object, in key order.
    pub fn all(&self) -> Vec<Term> {
        self.objects.values().flatten().cloned().collect()
    }

    pub fn first(&self) -> Option<&Key> {
        self.objects.keys().next()
    }

    pub fn last(&self) -> Option<&Key> {
        self.objects.keys().next_back()
    }

    /// The key after `key` (which need not be in the table).
    pub fn next(&self, key: &Key) -> Option<&Key> {
        use core::ops::Bound::{Excluded, Unbounded};
        self.objects.range((Excluded(key), Unbounded)).next().map(|(k, _)| k)
    }

    pub fn prev(&self, key: &Key) -> Option<&Key> {
        self.objects.range(..key).next_back().map(|(k, _)| k)
    }
}

/// All the tables of one VM.
#[derive(Default)]
pub struct Tables {
    by_tid: BTreeMap<u64, Table>,
    by_name: BTreeMap<String, u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TableError {
    NameTaken,
    TooMany,
}

impl Tables {
    pub fn create(&mut self, t: Table) -> Result<(), TableError> {
        if self.by_tid.len() >= MAX_TABLES {
            return Err(TableError::TooMany);
        }
        if t.named {
            if self.by_name.contains_key(t.name.as_str()) {
                return Err(TableError::NameTaken);
            }
            self.by_name.insert(t.name.as_str().into(), t.tid);
        }
        self.by_tid.insert(t.tid, t);
        Ok(())
    }

    /// Resolve a table identifier: a named table's name, or an unnamed table's reference.
    pub fn resolve(&self, id: &Term) -> Option<u64> {
        match id {
            Term::Atom(a) => self.by_name.get(a.as_str()).copied(),
            Term::Ref(r) => self.by_tid.contains_key(&r.0).then_some(r.0),
            _ => None,
        }
    }

    pub fn get(&self, tid: u64) -> Option<&Table> {
        self.by_tid.get(&tid)
    }

    pub fn get_mut(&mut self, tid: u64) -> Option<&mut Table> {
        self.by_tid.get_mut(&tid)
    }

    pub fn delete(&mut self, tid: u64) -> Option<Table> {
        let t = self.by_tid.remove(&tid)?;
        if t.named {
            self.by_name.remove(t.name.as_str());
        }
        Some(t)
    }

    pub fn rename(&mut self, tid: u64, name: Atom) -> Result<(), TableError> {
        if self.by_name.contains_key(name.as_str()) {
            return Err(TableError::NameTaken);
        }
        let t = self.by_tid.get_mut(&tid).expect("caller resolved the table");
        if t.named {
            self.by_name.remove(t.name.as_str());
            self.by_name.insert(name.as_str().into(), tid);
        }
        t.name = name;
        Ok(())
    }

    /// Tables owned by `pid`, for cleanup when it dies.
    pub fn owned_by(&self, pid: Pid) -> Vec<u64> {
        self.by_tid.values().filter(|t| t.owner == pid).map(|t| t.tid).collect()
    }

    pub fn tids(&self) -> Vec<u64> {
        self.by_tid.keys().copied().collect()
    }

    /// Memory all tables hold, in words.
    pub fn words(&self) -> u64 {
        self.by_tid.values().map(Table::words).sum()
    }
}

/// What `ets:new/2` was asked for.
pub struct Options {
    pub name: Atom,
    pub named: bool,
    pub kind: Kind,
    pub access: Access,
    pub keypos: usize,
    pub heir: Option<(Pid, Term)>,
}

impl Table {
    pub fn new(tid: u64, owner: Pid, o: Options) -> Table {
        Table {
            tid,
            name: o.name,
            named: o.named,
            kind: o.kind,
            access: o.access,
            keypos: o.keypos,
            owner,
            heir: o.heir,
            objects: BTreeMap::new(),
            count: 0,
            words: 0,
        }
    }
}

// ---- match specifications ----

/// Variable bindings of one match: `'$0'`, `'$1'`, ... by number.
pub type Bindings = BTreeMap<u32, Term>;

/// The number of a match variable (`'$3'` is 3), or `None` for any other atom.
pub fn variable(a: &Atom) -> Option<u32> {
    let digits = a.as_str().strip_prefix('$')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.len() > 9 {
        return None;
    }
    digits.parse().ok()
}

/// Match `pattern` against `value`, extending `b`. Patterns are ETS match patterns: `'_'` matches
/// anything, `'$N'` binds (or must equal its earlier binding), everything else matches exactly.
/// Uses a work list, so deep patterns and values cannot exhaust the Rust stack.
pub fn pattern_match(pattern: &Term, value: &Term, b: &mut Bindings) -> bool {
    let mut work = alloc::vec![(pattern.clone(), value.clone())];
    while let Some((p, v)) = work.pop() {
        match &p {
            Term::Atom(a) if a.as_str() == "_" => {}
            Term::Atom(a) if variable(a).is_some() => {
                let n = variable(a).expect("checked");
                match b.get(&n) {
                    Some(bound) => {
                        if !bound.eq_exact(&v) {
                            return false;
                        }
                    }
                    None => {
                        b.insert(n, v.clone());
                    }
                }
            }
            Term::Tuple(pt) => match v.as_tuple() {
                Some(vt) if vt.len() == pt.len() => {
                    work.extend(pt.iter().cloned().zip(vt.iter().cloned()));
                }
                _ => return false,
            },
            Term::Cons(pc) => match &v {
                Term::Cons(vc) => {
                    work.push((pc.head.clone(), vc.head.clone()));
                    work.push((pc.tail.clone(), vc.tail.clone()));
                }
                _ => return false,
            },
            Term::Map(pm) => match &v {
                // A map pattern matches a map that has at least its keys.
                Term::Map(vm) => {
                    for (k, pv) in pm.iter() {
                        match vm.get(k) {
                            Some(vv) => work.push((pv.clone(), vv.clone())),
                            None => return false,
                        }
                    }
                }
                _ => return false,
            },
            _ => {
                if !p.eq_exact(&v) {
                    return false;
                }
            }
        }
    }
    true
}

/// `'$$'`: all bound variables, in number order.
pub fn all_bindings(b: &Bindings) -> Term {
    Term::list(b.values().cloned().collect::<Vec<_>>())
}

/// A `{Head, Guards, Body}` clause of a match specification.
pub struct Clause {
    pub head: Term,
    pub guards: Vec<Term>,
    pub body: Vec<Term>,
}

/// Check the shape of a match specification: a list of three-tuples with list guards and body.
pub fn parse_spec(ms: &Term) -> Option<Vec<Clause>> {
    let mut out = Vec::new();
    for item in ms.list_iter() {
        let item = item.ok()?;
        let [head, guards, body] = item.as_tuple()? else { return None };
        out.push(Clause { head: head.clone(), guards: guards.to_vec()?, body: body.to_vec()? });
    }
    Some(out)
}

/// Functions a match specification guard or body may call: pure BIFs, plus `self()`/`node()`.
/// Anything with side effects (sending, spawning, exiting) is refused.
pub fn guard_function_allowed(name: &str, arity: usize) -> bool {
    const ALLOWED: &[(&str, usize)] = &[
        ("is_atom", 1), ("is_binary", 1), ("is_bitstring", 1), ("is_boolean", 1), ("is_float", 1),
        ("is_function", 1), ("is_function", 2), ("is_integer", 1), ("is_list", 1), ("is_map", 1),
        ("is_map_key", 2), ("is_number", 1), ("is_pid", 1), ("is_port", 1), ("is_reference", 1),
        ("is_tuple", 1), ("is_record", 2), ("is_record", 3), ("abs", 1), ("element", 2), ("hd", 1),
        ("tl", 1), ("length", 1), ("size", 1), ("tuple_size", 1), ("map_size", 1), ("map_get", 2),
        ("byte_size", 1), ("bit_size", 1), ("binary_part", 3), ("float", 1), ("trunc", 1),
        ("round", 1), ("floor", 1), ("ceil", 1), ("min", 2), ("max", 2), ("node", 0), ("node", 1),
        ("self", 0), ("not", 1), ("and", 2), ("or", 2), ("xor", 2), ("+", 2), ("-", 2), ("*", 2),
        ("/", 2), ("div", 2), ("rem", 2), ("band", 2), ("bor", 2), ("bxor", 2), ("bnot", 1),
        ("bsl", 2), ("bsr", 2), ("-", 1), ("+", 1), ("==", 2), ("/=", 2), ("=:=", 2), ("=/=", 2),
        ("<", 2), (">", 2), ("=<", 2), (">=", 2),
    ];
    ALLOWED.contains(&(name, arity))
}
