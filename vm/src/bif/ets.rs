//! The `ets` module's native functions. Tables and match specifications are in `crate::ets`;
//! the rest of `ets` (`tab2list`, `foldl`, ...) is the real OTP module.

use alloc::vec::Vec;

use super::Ctx;
use crate::ets::{self, Access, Bindings, Clause, Key, Kind, Table};
use crate::process::Exception;
use crate::term::{OwnedTerm, Pid, Ref, Term};

type R = Result<Term, Exception>;

/// How deeply a match specification's guard and body expressions may nest.
const MAX_EXPR_DEPTH: usize = 64;

fn end_of_table(c: &mut Ctx) -> Term {
    c.atom("$end_of_table")
}

/// The table `id` names, if the caller may read it (or, with `write`, change it). The error's
/// cause is `id` for no such table and `access` for a table the caller may not use, as in BEAM.
fn table_id(c: &Ctx, id: &Term, write: bool) -> Result<u64, Exception> {
    let because = |cause: &str| {
        let mut e = c.badarg();
        e.cause = c.sys.atom_table.existing(cause).map(Term::Atom);
        e
    };
    let tid = c.sys.ets.resolve(*id).ok_or_else(|| because("id"))?;
    let t = c.sys.ets.get(tid).expect("resolved");
    let allowed = if write { t.may_write(c.p.pid) } else { t.may_read(c.p.pid) };
    if allowed {
        Ok(tid)
    } else {
        Err(because("access"))
    }
}

fn table<'c>(c: &'c Ctx, id: &Term) -> Result<&'c Table, Exception> {
    let tid = table_id(c, id, false)?;
    Ok(c.sys.ets.get(tid).expect("resolved"))
}

fn table_mut<'c>(c: &'c mut Ctx, id: &Term) -> Result<&'c mut Table, Exception> {
    let tid = table_id(c, id, true)?;
    Ok(c.sys.ets.get_mut(tid).expect("resolved"))
}

/// Copies of `objs` on the caller's heap, as a list.
fn objects_out(c: &mut Ctx, tid: u64, key: &Key) -> Term {
    let t = c.sys.ets.get(tid).expect("resolved");
    let copies: Vec<Term> = t.lookup(key).iter().map(|o| o.copy_into(&mut c.p.heap)).collect();
    c.p.heap.list(copies)
}

/// The identifier `ets:new/2` returns: the name for a named table, else its reference.
fn table_term(t: &Table) -> Term {
    if t.named {
        Term::Atom(t.name)
    } else {
        Term::Ref(Ref(t.tid))
    }
}

pub fn new(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = a[0] else { return Err(c.badarg()) };
    let (mut kind, mut access, mut keypos, mut named, mut heir) = (Kind::Set, Access::Protected, 1usize, false, None);
    for o in c.list_arg(a[1])? {
        match (o, c.heap().as_tuple(o)) {
            (Term::Atom(x), _) => match x.as_str() {
                "set" => kind = Kind::Set,
                "ordered_set" => kind = Kind::OrderedSet,
                "bag" => kind = Kind::Bag,
                "duplicate_bag" => kind = Kind::DuplicateBag,
                "public" => access = Access::Public,
                "protected" => access = Access::Protected,
                "private" => access = Access::Private,
                "named_table" => named = true,
                // Performance hints mean nothing here.
                "compressed" => {}
                _ => return Err(c.badarg()),
            },
            (_, Some(&[Term::Atom(k), v])) if k.as_str() == "keypos" => {
                keypos = v.as_usize().filter(|p| *p >= 1).ok_or_else(|| c.badarg())?;
            }
            (_, Some(&[Term::Atom(k), Term::Atom(none)])) if k.as_str() == "heir" && none.as_str() == "none" => heir = None,
            (_, Some(&[Term::Atom(k), Term::Pid(p), data])) if k.as_str() == "heir" => heir = Some((p, c.own(data))),
            (_, Some(&[Term::Atom(k), _]))
                if matches!(k.as_str(), "read_concurrency" | "write_concurrency" | "decentralized_counters") => {}
            _ => return Err(c.badarg()),
        }
    }
    let tid = c.sys.make_ref().0;
    let t = Table::new(tid, c.p.pid, ets::Options { name, named, kind, access, keypos, heir });
    let id = table_term(&t);
    c.sys.ets.create(t).map_err(|e| match e {
        ets::TableError::NameTaken => c.badarg(),
        ets::TableError::TooMany => c.system_limit(),
    })?;
    Ok(id)
}

/// The objects `insert` was given: one tuple or a list of them, each with a key, copied out of
/// the caller's heap. Raises `system_limit` if they would take ETS past `Limits::max_ets_words`.
fn objects(c: &Ctx, t: &Table, arg: &Term) -> Result<Vec<(Key, OwnedTerm)>, Exception> {
    let list = match arg {
        Term::Tuple(_) => alloc::vec![*arg],
        _ => c.list_arg(*arg)?,
    };
    let mut out = Vec::with_capacity(list.len());
    for o in list {
        let k = t.key_of(c.heap(), o).ok_or_else(|| c.badarg())?;
        out.push((k, c.own(o)));
    }
    room_for(c, out.iter().map(|(_, o)| o))?;
    Ok(out)
}

/// `system_limit` unless ETS has room for `objs` besides what it holds. Objects they would
/// replace are not credited: near the limit, an overwrite may be refused.
fn room_for<'t>(c: &Ctx, objs: impl Iterator<Item = &'t OwnedTerm>) -> Result<(), Exception> {
    let room = c.sys.limits.max_ets_words.saturating_sub(c.sys.ets.words());
    let mut need: u64 = 0;
    for o in objs {
        need = need.saturating_add(ets::weigh(o));
        if need > room {
            return Err(c.system_limit());
        }
    }
    Ok(())
}

pub fn insert(c: &mut Ctx, a: &[Term]) -> R {
    let objs = {
        let t = table(c, &a[0])?;
        objects(c, t, &a[1])?
    };
    let t = table_mut(c, &a[0])?;
    for (k, o) in objs {
        t.insert(k, o);
    }
    Ok(Term::Atom(c.sys.atoms.true_))
}

pub fn insert_new(c: &mut Ctx, a: &[Term]) -> R {
    let objs = {
        let t = table(c, &a[0])?;
        objects(c, t, &a[1])?
    };
    let t = table_mut(c, &a[0])?;
    if objs.iter().any(|(k, _)| t.contains(k)) {
        return Ok(c.bool(false));
    }
    for (k, o) in objs {
        t.insert(k, o);
    }
    Ok(c.bool(true))
}

pub fn lookup(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    let key = c.sys.ets.get(tid).expect("resolved").key(&c.p.heap, a[1]);
    Ok(objects_out(c, tid, &key))
}

pub fn member(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let found = t.contains(&t.key(c.heap(), a[1]));
    Ok(c.bool(found))
}

/// `lookup_element(Tab, Key, Pos)` and `lookup_element(Tab, Key, Pos, Default)`.
pub fn lookup_element(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    let pos = a[2].as_usize().filter(|p| *p >= 1).ok_or_else(|| c.badarg())?;
    let t = c.sys.ets.get(tid).expect("resolved");
    let kind = t.kind;
    let objs = t.lookup(&t.key(&c.p.heap, a[1]));
    if objs.is_empty() {
        return a.get(3).copied().ok_or_else(|| c.badarg());
    }
    let mut elems = Vec::new();
    for o in objs {
        let e = o.heap().as_tuple(o.term()).and_then(|e| e.get(pos - 1)).copied();
        let Some(e) = e else { return Err(c.badarg()) };
        elems.push(crate::term::copy(o.heap(), e, &mut c.p.heap));
    }
    Ok(match kind {
        Kind::Set | Kind::OrderedSet => elems.pop().expect("one object"),
        Kind::Bag | Kind::DuplicateBag => c.list(elems),
    })
}

/// `delete(Tab)` deletes the table (owner only); `delete(Tab, Key)` deletes objects.
pub fn delete(c: &mut Ctx, a: &[Term]) -> R {
    if a.len() == 1 {
        let tid = c.sys.ets.resolve(a[0]).ok_or_else(|| c.badarg())?;
        if c.sys.ets.get(tid).expect("resolved").owner != c.p.pid
            && c.sys.ets.get(tid).expect("resolved").access != Access::Public
        {
            return Err(c.badarg());
        }
        c.sys.ets.delete(tid);
    } else {
        let tid = table_id(c, &a[0], true)?;
        let t = c.sys.ets.get_mut(tid).expect("resolved");
        let k = t.key(&c.p.heap, a[1]);
        t.remove(&k);
    }
    Ok(Term::Atom(c.sys.atoms.true_))
}

pub fn delete_object(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], true)?;
    let t = c.sys.ets.get_mut(tid).expect("resolved");
    let Some(k) = t.key_of(&c.p.heap, a[1]) else { return Err(c.badarg()) };
    let obj = OwnedTerm::new(&c.p.heap, a[1]);
    t.remove_object(&k, &obj);
    Ok(Term::Atom(c.sys.atoms.true_))
}

pub fn take(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], true)?;
    let t = c.sys.ets.get_mut(tid).expect("resolved");
    let k = t.key(&c.p.heap, a[1]);
    let gone = t.remove(&k);
    let copies: Vec<Term> = gone.iter().map(|o| o.copy_into(&mut c.p.heap)).collect();
    Ok(c.list(copies))
}

pub fn internal_delete_all(c: &mut Ctx, a: &[Term]) -> R {
    let n = table_mut(c, &a[0])?.clear();
    Ok(Term::Int(n as i64))
}

/// One `update_counter` operation on `obj` (on the caller's heap): `Incr`, `{Pos, Incr}` or
/// `{Pos, Incr, Threshold, SetValue}`. Returns the new object and the new counter value.
fn counter_op(c: &mut Ctx, obj: &Term, keypos: usize, op: &Term) -> Result<(Term, Term), Exception> {
    let (pos, incr, limit) = match (op, c.heap().as_tuple(*op)) {
        // A bare increment updates the element after the key.
        (Term::Int(_) | Term::Big(_), _) => (keypos + 1, *op, None),
        (_, Some(&[p, i])) => (p.as_usize().ok_or_else(|| c.badarg())?, i, None),
        (_, Some(&[p, i, th, sv])) => (p.as_usize().ok_or_else(|| c.badarg())?, i, Some((th, sv))),
        _ => return Err(c.badarg()),
    };
    let mut elems = c.tuple_elems(*obj).ok_or_else(|| c.badarg())?;
    if pos == keypos || pos == 0 || pos > elems.len() || !elems[pos - 1].is_integer() || !incr.is_integer() {
        return Err(c.badarg());
    }
    let mut new = super::arith::add(c, &[elems[pos - 1], incr])?;
    if let Some((threshold, set_value)) = limit {
        let h = c.heap();
        let over = if h.cmp_term(incr, Term::Int(0)) != core::cmp::Ordering::Less {
            h.cmp_term(new, threshold) == core::cmp::Ordering::Greater
        } else {
            h.cmp_term(new, threshold) == core::cmp::Ordering::Less
        };
        if over {
            new = set_value;
        }
    }
    elems[pos - 1] = new;
    Ok((c.tuple(&elems), new))
}

/// `update_counter(Tab, Key, Op | [Op])` and `update_counter(Tab, Key, Op, Default)`.
pub fn update_counter(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], true)?;
    let t = c.sys.ets.get(tid).expect("resolved");
    let (keypos, kind) = (t.keypos, t.kind);
    let key = t.key(&c.p.heap, a[1]);
    let current = t.lookup(&key).first().map(|o| o.copy_into(&mut c.p.heap));
    if !matches!(kind, Kind::Set | Kind::OrderedSet) {
        return Err(c.badarg());
    }
    let mut obj = match (current, a.get(3)) {
        (Some(o), _) => o,
        (None, Some(default)) => {
            // The default object must have the key in the right place; its key is replaced.
            let mut e = c.tuple_elems(*default).ok_or_else(|| c.badarg())?;
            if e.len() < keypos {
                return Err(c.badarg());
            }
            e[keypos - 1] = a[1];
            c.tuple(&e)
        }
        (None, None) => return Err(c.badarg()),
    };
    let (ops, many) = match a[2] {
        Term::Nil | Term::Cons(_) => (c.list_arg(a[2])?, true),
        op => (alloc::vec![op], false),
    };
    let mut results = Vec::new();
    for op in &ops {
        let (o, v) = counter_op(c, &obj, keypos, op)?;
        obj = o;
        results.push(v);
    }
    let obj = c.own(obj);
    let t = c.sys.ets.get_mut(tid).expect("resolved");
    if t.contains(&key) {
        t.replace(&key, obj);
    } else {
        t.insert(key, obj);
    }
    Ok(if many { c.list(results) } else { results.pop().expect("one op") })
}

/// `update_element(Tab, Key, {Pos, Value} | [{Pos, Value}])`: `false` if there is no object.
pub fn update_element(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], true)?;
    let t = c.sys.ets.get(tid).expect("resolved");
    if !matches!(t.kind, Kind::Set | Kind::OrderedSet) {
        return Err(c.badarg());
    }
    let key = t.key(&c.p.heap, a[1]);
    let keypos = t.keypos;
    let Some(obj) = t.lookup(&key).first().map(|o| o.copy_into(&mut c.p.heap)) else {
        return Ok(Term::Atom(c.sys.atoms.false_));
    };
    let changes = match a[2] {
        Term::Tuple(_) => alloc::vec![a[2]],
        other => c.list_arg(other)?,
    };
    let mut e = c.tuple_elems(obj).expect("objects are tuples");
    for ch in changes {
        match c.heap().as_tuple(ch) {
            Some(&[p, v]) => match p.as_usize() {
                Some(p) if p >= 1 && p <= e.len() && p != keypos => e[p - 1] = v,
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    let new = c.tuple(&e);
    let new = c.own(new);
    c.sys.ets.get_mut(tid).expect("resolved").replace(&key, new);
    Ok(Term::Atom(c.sys.atoms.true_))
}

// ---- traversal ----

/// A key found in table `tid` (by `find`), copied to the caller's heap, or `'$end_of_table'`.
fn key_out(c: &mut Ctx, tid: u64, find: impl Fn(&Table, &crate::term::Heap) -> Option<Key>) -> Term {
    let t = c.sys.ets.get(tid).expect("resolved");
    match find(t, &c.p.heap) {
        Some(k) => k.term.copy_into(&mut c.p.heap),
        None => end_of_table(c),
    }
}

pub fn first(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    Ok(key_out(c, tid, |t, _| t.first().cloned()))
}

pub fn last(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    Ok(key_out(c, tid, |t, _| t.last().cloned()))
}

pub fn next(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    let k = a[1];
    Ok(key_out(c, tid, |t, h| t.next(&t.key(h, k)).cloned()))
}

pub fn prev(c: &mut Ctx, a: &[Term]) -> R {
    let tid = table_id(c, &a[0], false)?;
    let k = a[1];
    Ok(key_out(c, tid, |t, h| t.prev(&t.key(h, k)).cloned()))
}

/// `first_lookup/1` and friends: `{Key, Objects}` for the key `find` gives, or
/// `'$end_of_table'`.
fn with_objects(c: &mut Ctx, id: &Term, find: impl Fn(&Table, &crate::term::Heap) -> Option<Key>) -> R {
    let tid = table_id(c, id, false)?;
    let t = c.sys.ets.get(tid).expect("resolved");
    let Some(k) = find(t, &c.p.heap) else { return Ok(end_of_table(c)) };
    let key = k.term.copy_into(&mut c.p.heap);
    let objs = objects_out(c, tid, &k);
    Ok(c.tuple(&[key, objs]))
}

pub fn first_lookup(c: &mut Ctx, a: &[Term]) -> R {
    with_objects(c, &a[0], |t, _| t.first().cloned())
}

pub fn last_lookup(c: &mut Ctx, a: &[Term]) -> R {
    with_objects(c, &a[0], |t, _| t.last().cloned())
}

pub fn next_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let k = a[1];
    with_objects(c, &a[0], |t, h| t.next(&t.key(h, k)).cloned())
}

pub fn prev_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let k = a[1];
    with_objects(c, &a[0], |t, h| t.prev(&t.key(h, k)).cloned())
}

// ---- matching ----

/// Evaluate a guard or body expression of a match specification. The expression, the object
/// and the bindings are all terms of the caller's heap.
fn eval(c: &mut Ctx, e: Term, obj: Term, b: &Bindings, depth: usize) -> Option<Term> {
    if depth > MAX_EXPR_DEPTH {
        return None;
    }
    Some(match e {
        Term::Atom(a) if a.as_str() == "$_" => obj,
        Term::Atom(a) if a.as_str() == "$$" => ets::all_bindings(c.heap_mut(), b),
        Term::Atom(a) if ets::variable(&a).is_some() => *b.get(&ets::variable(&a).expect("checked"))?,
        Term::Cons(_) => {
            let mut items = Vec::new();
            let mut tail = Term::Nil;
            let parts: Vec<Result<Term, Term>> = c.heap().list_iter(e).collect();
            for x in parts {
                match x {
                    Ok(x) => items.push(eval(c, x, obj, b, depth + 1)?),
                    Err(t) => tail = eval(c, t, obj, b, depth + 1)?,
                }
            }
            c.list_with_tail(items, tail)
        }
        Term::Map(_) => {
            let mut pairs = Vec::new();
            for (k, v) in c.heap().map_entries(e).expect("a map") {
                pairs.push((k, eval(c, v, obj, b, depth + 1)?));
            }
            c.map_from(pairs)
        }
        Term::Tuple(_) => {
            let t = c.tuple_elems(e).expect("a tuple");
            match t[..] {
                // {const, X}: X, unevaluated. {{...}}: a tuple of evaluated elements.
                [Term::Atom(k), x] if k.as_str() == "const" => x,
                [inner @ Term::Tuple(_)] => {
                    let inner = c.tuple_elems(inner).expect("a tuple");
                    let mut elems = Vec::with_capacity(inner.len());
                    for x in inner {
                        elems.push(eval(c, x, obj, b, depth + 1)?);
                    }
                    c.tuple(&elems)
                }
                [Term::Atom(f), ref args @ ..] => call(c, f.as_str(), args, obj, b, depth)?,
                _ => return None,
            }
        }
        other => other,
    })
}

fn call(c: &mut Ctx, f: &str, args: &[Term], obj: Term, b: &Bindings, depth: usize) -> Option<Term> {
    match f {
        "andalso" | "orelse" => {
            // Short-circuit, left to right.
            let stop_on = f == "orelse";
            for x in args {
                let v = eval(c, *x, obj, b, depth + 1)?;
                if v.is_atom(&c.sys.atoms.true_) == stop_on {
                    return Some(c.bool(stop_on));
                }
                if !v.is_atom(&c.sys.atoms.true_) && !v.is_atom(&c.sys.atoms.false_) {
                    return None;
                }
            }
            Some(c.bool(!stop_on))
        }
        _ if ets::guard_function_allowed(f, args.len()) => {
            let mut vals = Vec::with_capacity(args.len());
            for x in args {
                vals.push(eval(c, *x, obj, b, depth + 1)?);
            }
            let erlang = c.sys.atoms.erlang;
            let fname = c.sys.atom(f);
            let n = c.sys.native(&erlang, &fname, args.len() as u32)?;
            n(c, &vals).ok()
        }
        _ => None,
    }
}

/// Run a match specification over one object (on the caller's heap): the body's last value, if
/// some clause matches.
fn run_spec(c: &mut Ctx, spec: &[Clause], obj: Term) -> Option<Term> {
    for clause in spec {
        let mut b = Bindings::new();
        if !ets::pattern_match(c.heap(), clause.head, obj, &mut b) {
            continue;
        }
        let mut guards_pass = true;
        for g in &clause.guards {
            if !eval(c, *g, obj, &b, 0).is_some_and(|v| v.is_atom(&c.sys.atoms.true_)) {
                guards_pass = false;
                break;
            }
        }
        if !guards_pass {
            continue;
        }
        let mut result = None;
        for expr in &clause.body {
            result = Some(eval(c, *expr, obj, &b, 0)?);
        }
        return result;
    }
    None
}

fn spec(c: &Ctx, t: &Term) -> Result<Vec<Clause>, Exception> {
    // A "compiled" match spec (from `match_spec_compile/1`) is the spec itself.
    let t = match c.heap().as_tuple(*t) {
        Some(&[Term::Atom(tag), ms]) if tag.as_str() == "$ms" => ms,
        _ => *t,
    };
    ets::parse_spec(c.heap(), t).ok_or_else(|| c.badarg())
}

/// Objects of `id` for which `spec` gives a result, with those results, in key order. Each
/// object is copied to the caller's heap to be matched there.
fn select_all(c: &mut Ctx, id: &Term, spec: &[Clause]) -> Result<Vec<(Term, Term)>, Exception> {
    let tid = table_id(c, id, false)?;
    let t = c.sys.ets.get(tid).expect("resolved");
    let objs: Vec<Term> = t.all().map(|o| o.copy_into(&mut c.p.heap)).collect();
    let mut out = Vec::new();
    for o in objs {
        if let Some(r) = run_spec(c, spec, o) {
            out.push((o, r));
        }
    }
    Ok(out)
}

pub fn select(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    Ok(c.list(found.into_iter().map(|(_, r)| r).collect::<Vec<_>>()))
}

/// Hand out `results` `limit` at a time, as `{Chunk, Continuation}` and finally
/// `'$end_of_table'`. The chunks come from the end for hash tables (as BEAM's do, which code
/// such as Elixir's `Module.get_last_attribute/2` relies on) and from the front otherwise.
/// The results are computed once, when the traversal starts: later changes to the table do not
/// show, which BEAM does not promise either.
fn chunk(c: &mut Ctx, mut results: Vec<Term>, limit: usize, from_end: bool) -> Term {
    if results.is_empty() {
        return end_of_table(c);
    }
    let n = limit.min(results.len());
    let chunk: Vec<Term> = if from_end { results.split_off(results.len() - n) } else { results.drain(..n).collect() };
    let cont = if results.is_empty() {
        end_of_table(c)
    } else {
        let tag = c.atom("$beamlet_select");
        let rest = c.list(results);
        let from_end = c.bool(from_end);
        c.tuple(&[tag, rest, Term::Int(limit as i64), from_end])
    };
    let chunk = c.list(chunk);
    c.tuple(&[chunk, cont])
}

fn limit(c: &Ctx, t: &Term) -> Result<usize, Exception> {
    t.as_usize().filter(|n| *n >= 1).ok_or_else(|| c.badarg())
}

/// Whether chunks of a traversal of table `id` come from the end (hash tables).
fn hash_table(c: &Ctx, id: &Term) -> Result<bool, Exception> {
    Ok(table(c, id)?.kind != Kind::OrderedSet)
}

/// `select(Tab, MS, Limit)`.
pub fn select3(c: &mut Ctx, a: &[Term]) -> R {
    let n = limit(c, &a[2])?;
    let from_end = hash_table(c, &a[0])?;
    let all = select(c, &a[..2])?;
    let all = c.heap().to_vec(all).expect("a list");
    Ok(chunk(c, all, n, from_end))
}

/// `select_reverse(Tab, MS, Limit)`: an ordered set from its last key; a hash table as `select/3`.
pub fn select_reverse3(c: &mut Ctx, a: &[Term]) -> R {
    let n = limit(c, &a[2])?;
    let from_end = hash_table(c, &a[0])?;
    let all = if from_end { select(c, &a[..2])? } else { select_reverse(c, &a[..2])? };
    let all = c.heap().to_vec(all).expect("a list");
    Ok(chunk(c, all, n, from_end))
}

/// `match(Tab, Pattern, Limit)` and `match_object(Tab, Pattern, Limit)`.
pub fn match3(c: &mut Ctx, a: &[Term]) -> R {
    let n = limit(c, &a[2])?;
    let from_end = hash_table(c, &a[0])?;
    let all = match_(c, &a[..2])?;
    let all = c.heap().to_vec(all).expect("a list");
    Ok(chunk(c, all, n, from_end))
}

pub fn match_object3(c: &mut Ctx, a: &[Term]) -> R {
    let n = limit(c, &a[2])?;
    let from_end = hash_table(c, &a[0])?;
    let all = match_object(c, &a[..2])?;
    let all = c.heap().to_vec(all).expect("a list");
    Ok(chunk(c, all, n, from_end))
}

/// `select(Continuation)` (also `match/1`, `match_object/1`, `select_reverse/1`).
pub fn select1(c: &mut Ctx, a: &[Term]) -> R {
    if a[0].is_atom(&c.sys.atom("$end_of_table")) {
        return Ok(end_of_table(c));
    }
    match c.heap().as_tuple(a[0]) {
        Some(&[Term::Atom(tag), rest, Term::Int(n), from_end]) if tag.as_str() == "$beamlet_select" && n >= 1 => {
            let rest = c.list_arg(rest)?;
            let from_end = from_end.is_atom(&c.sys.atoms.true_);
            Ok(chunk(c, rest, n as usize, from_end))
        }
        _ => Err(c.badarg()),
    }
}

pub fn select_reverse(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    Ok(c.list(found.into_iter().rev().map(|(_, r)| r).collect::<Vec<_>>()))
}

pub fn select_count(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    let n = found.iter().filter(|(_, r)| r.is_atom(&c.sys.atoms.true_)).count();
    Ok(Term::Int(n as i64))
}

pub fn internal_select_delete(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let tid = table_id(c, &a[0], true)?;
    let found = select_all(c, &a[0], &s)?;
    let doomed: Vec<Term> = found.into_iter().filter(|(_, r)| r.is_atom(&c.sys.atoms.true_)).map(|(o, _)| o).collect();
    let t = c.sys.ets.get_mut(tid).expect("resolved");
    let mut n = 0;
    for o in doomed {
        let k = t.key_of(&c.p.heap, o).expect("stored objects have keys");
        n += t.remove_object(&k, &OwnedTerm::new(&c.p.heap, o));
    }
    Ok(Term::Int(n as i64))
}

/// `select_replace(Tab, MS)`: replace each matching object with the body's result, which must
/// keep the key. Returns the number replaced.
pub fn select_replace(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let tid = table_id(c, &a[0], true)?;
    let found = select_all(c, &a[0], &s)?;
    let owned: Vec<(OwnedTerm, OwnedTerm)> = found.into_iter().map(|(old, new)| (c.own(old), c.own(new))).collect();
    room_for(c, owned.iter().map(|(_, new)| new))?;
    let t = c.sys.ets.get_mut(tid).expect("resolved");
    let mut n = 0;
    for (old, new) in owned {
        let (Some(k_old), Some(k_new)) = (t.key_of(old.heap(), old.term()), t.key_of(new.heap(), new.term())) else { continue };
        if k_old == k_new {
            t.remove_object(&k_old, &old);
            t.insert(k_new, new);
            n += 1;
        }
    }
    Ok(Term::Int(n))
}

fn pattern_spec(c: &mut Ctx, pattern: &Term, body: &str) -> Vec<Clause> {
    let b = c.atom(body);
    alloc::vec![Clause { head: *pattern, guards: Vec::new(), body: alloc::vec![b] }]
}

/// `match(Tab, Pattern)`: the bindings of each match, as lists.
pub fn match_(c: &mut Ctx, a: &[Term]) -> R {
    let s = pattern_spec(c, &a[1], "$$");
    let found = select_all(c, &a[0], &s)?;
    Ok(c.list(found.into_iter().map(|(_, r)| r).collect::<Vec<_>>()))
}

pub fn match_object(c: &mut Ctx, a: &[Term]) -> R {
    let s = pattern_spec(c, &a[1], "$_");
    let found = select_all(c, &a[0], &s)?;
    Ok(c.list(found.into_iter().map(|(o, _)| o).collect::<Vec<_>>()))
}

pub fn match_spec_compile(c: &mut Ctx, a: &[Term]) -> R {
    spec(c, &a[0])?;
    let tag = c.atom("$ms");
    Ok(c.tuple(&[tag, a[0]]))
}

pub fn is_compiled_ms(c: &mut Ctx, a: &[Term]) -> R {
    let ok = matches!(c.heap().as_tuple(a[0]), Some(&[Term::Atom(tag), _]) if tag.as_str() == "$ms");
    Ok(c.bool(ok))
}

/// `match_spec_run_r(List, CompiledMS, Acc)`: results for `List`, reversed onto `Acc`.
pub fn match_spec_run_r(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let mut acc = a[2];
    for o in c.list_arg(a[0])? {
        if let Some(r) = run_spec(c, &s, o) {
            acc = c.cons(r, acc);
        }
    }
    Ok(acc)
}

// ---- table information and ownership ----

fn info_value(c: &mut Ctx, t_id: u64, item: &str) -> Option<Term> {
    let t = c.sys.ets.get(t_id)?;
    let (name, named, kind, access, keypos, owner, size) = (t.name, t.named, t.kind, t.access, t.keypos, t.owner, t.size());
    let heir = t.heir.as_ref().map(|(p, _)| *p);
    let tid = t.tid;
    Some(match item {
        "name" => Term::Atom(name),
        "named_table" => c.bool(named),
        "type" => c.atom(match kind {
            Kind::Set => "set",
            Kind::OrderedSet => "ordered_set",
            Kind::Bag => "bag",
            Kind::DuplicateBag => "duplicate_bag",
        }),
        "protection" => c.atom(match access {
            Access::Public => "public",
            Access::Protected => "protected",
            Access::Private => "private",
        }),
        "keypos" => Term::Int(keypos as i64),
        "owner" => Term::Pid(owner),
        "size" => Term::Int(size as i64),
        "id" => Term::Ref(Ref(tid)),
        "heir" => match heir {
            Some(p) => Term::Pid(p),
            None => c.atom("none"),
        },
        "compressed" | "read_concurrency" | "write_concurrency" | "decentralized_counters" => c.bool(false),
        _ => return None,
    })
}

pub fn info(c: &mut Ctx, a: &[Term]) -> R {
    let Some(tid) = c.sys.ets.resolve(a[0]) else {
        return if matches!(a[0], Term::Atom(_) | Term::Ref(_)) { Ok(Term::Atom(c.sys.atoms.undefined)) } else { Err(c.badarg()) };
    };
    match a.get(1) {
        Some(Term::Atom(item)) => info_value(c, tid, item.as_str()).ok_or_else(|| c.badarg()),
        Some(_) => Err(c.badarg()),
        None => {
            let mut out = Vec::new();
            for item in ["id", "name", "named_table", "type", "protection", "keypos", "owner", "size", "heir"] {
                let v = info_value(c, tid, item).expect("known item");
                let k = c.atom(item);
                out.push(c.tuple(&[k, v]));
            }
            Ok(c.list(out))
        }
    }
}

pub fn whereis(c: &mut Ctx, a: &[Term]) -> R {
    Ok(match c.sys.ets.resolve(a[0]) {
        Some(tid) if matches!(a[0], Term::Atom(_)) => Term::Ref(Ref(tid)),
        _ => Term::Atom(c.sys.atoms.undefined),
    })
}

pub fn rename(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = a[1] else { return Err(c.badarg()) };
    let tid = table_id(c, &a[0], true)?;
    c.sys.ets.rename(tid, name).map_err(|_| c.badarg())?;
    Ok(a[1])
}

/// Transfer a table to a new owner, which receives `{'ETS-TRANSFER', Tab, FromPid, Data}`
/// (`data` a term of the caller's heap).
pub(crate) fn transfer(c: &mut Ctx, tid: u64, to: Pid, data: Term) {
    let Some(t) = c.sys.ets.get_mut(tid) else { return };
    let from = t.owner;
    t.owner = to;
    let id = table_term(t);
    let tag = c.atom("ETS-TRANSFER");
    let msg = c.tuple(&[tag, id, Term::Pid(from), data]);
    super::proc::send_to(c, to, msg);
}

pub fn give_away(c: &mut Ctx, a: &[Term]) -> R {
    let tid = c.sys.ets.resolve(a[0]).ok_or_else(|| c.badarg())?;
    let Term::Pid(to) = a[1] else { return Err(c.badarg()) };
    let owner = c.sys.ets.get(tid).expect("resolved").owner;
    if owner != c.p.pid || to == c.p.pid || !c.sys.procs.is_alive(to) {
        return Err(c.badarg());
    }
    transfer(c, tid, to, a[2]);
    Ok(Term::Atom(c.sys.atoms.true_))
}

/// `setopts(Tab, Opts)`: only `{heir, ...}` is supported (by the owner).
pub fn setopts(c: &mut Ctx, a: &[Term]) -> R {
    let tid = c.sys.ets.resolve(a[0]).ok_or_else(|| c.badarg())?;
    if c.sys.ets.get(tid).expect("resolved").owner != c.p.pid {
        return Err(c.badarg());
    }
    let opts = match a[1] {
        Term::Tuple(_) => alloc::vec![a[1]],
        other => c.list_arg(other)?,
    };
    for o in opts {
        let heir = match c.heap().as_tuple(o) {
            Some(&[Term::Atom(k), Term::Atom(none)]) if k.as_str() == "heir" && none.as_str() == "none" => None,
            Some(&[Term::Atom(k), Term::Pid(p), data]) if k.as_str() == "heir" => Some((p, c.own(data))),
            _ => return Err(c.badarg()),
        };
        c.sys.ets.get_mut(tid).expect("resolved").heir = heir;
    }
    Ok(Term::Atom(c.sys.atoms.true_))
}

pub fn safe_fixtable(c: &mut Ctx, a: &[Term]) -> R {
    table(c, &a[0])?;
    Ok(Term::Atom(c.sys.atoms.true_))
}

pub fn all(c: &mut Ctx, _a: &[Term]) -> R {
    let mut out = Vec::new();
    for tid in c.sys.ets.tids() {
        let t = c.sys.ets.get(tid).expect("listed");
        out.push(table_term(t));
    }
    Ok(c.list(out))
}
