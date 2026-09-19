//! The `ets` module's native functions. Tables and match specifications are in `crate::ets`;
//! the rest of `ets` (`tab2list`, `foldl`, ...) is the real OTP module.

use alloc::vec::Vec;

use super::Ctx;
use crate::ets::{self, Access, Bindings, Clause, Key, Kind, Table};
use crate::process::Exception;
use crate::term::{Pid, Ref, Term};

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
    let tid = c.sys.ets.resolve(id).ok_or_else(|| because("id"))?;
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

/// The identifier `ets:new/2` returns: the name for a named table, else its reference.
fn table_term(t: &Table) -> Term {
    if t.named {
        Term::Atom(t.name.clone())
    } else {
        Term::Ref(Ref(t.tid))
    }
}

pub fn new(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else { return Err(c.badarg()) };
    let (mut kind, mut access, mut keypos, mut named, mut heir) = (Kind::Set, Access::Protected, 1usize, false, None);
    for o in a[1].to_vec().ok_or_else(|| c.badarg())? {
        match &o {
            Term::Atom(x) => match x.as_str() {
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
            Term::Tuple(t) => match (&t[..], t.first()) {
                ([_, v], Some(Term::Atom(k))) if k.as_str() == "keypos" => {
                    keypos = v.as_usize().filter(|p| *p >= 1).ok_or_else(|| c.badarg())?;
                }
                ([_, Term::Atom(none)], Some(Term::Atom(k))) if k.as_str() == "heir" && none.as_str() == "none" => heir = None,
                ([_, Term::Pid(p), data], Some(Term::Atom(k))) if k.as_str() == "heir" => heir = Some((*p, data.clone())),
                ([_, _], Some(Term::Atom(k)))
                    if matches!(k.as_str(), "read_concurrency" | "write_concurrency" | "decentralized_counters") => {}
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    let tid = c.sys.make_ref().0;
    let t = Table::new(tid, c.p.pid, ets::Options { name: name.clone(), named, kind, access, keypos, heir });
    let id = table_term(&t);
    c.sys.ets.create(t).map_err(|e| match e {
        ets::TableError::NameTaken => c.badarg(),
        ets::TableError::TooMany => c.system_limit(),
    })?;
    Ok(id)
}

/// The objects `insert` was given: one tuple or a list of them, each with a key. Raises
/// `system_limit` if they would take ETS past `Limits::max_ets_words`.
fn objects(c: &Ctx, t: &Table, arg: &Term) -> Result<Vec<(Key, Term)>, Exception> {
    let list = match arg {
        Term::Tuple(_) => alloc::vec![arg.clone()],
        _ => arg.to_vec().ok_or_else(|| c.badarg())?,
    };
    room_for(c, list.iter())?;
    list.into_iter().map(|o| t.key_of(&o).map(|k| (k, o)).ok_or_else(|| c.badarg())).collect()
}

/// `system_limit` unless ETS has room for `objs` besides what it holds. Objects they would
/// replace are not credited: near the limit, an overwrite may be refused.
fn room_for<'t>(c: &Ctx, objs: impl Iterator<Item = &'t Term>) -> Result<(), Exception> {
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
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
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
    let t = table(c, &a[0])?;
    Ok(Term::list(t.lookup(&t.key(a[1].clone())).to_vec()))
}

pub fn member(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let found = t.contains(&t.key(a[1].clone()));
    Ok(c.bool(found))
}

/// `lookup_element(Tab, Key, Pos)` and `lookup_element(Tab, Key, Pos, Default)`.
pub fn lookup_element(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let pos = a[2].as_usize().filter(|p| *p >= 1).ok_or_else(|| c.badarg())?;
    let objs = t.lookup(&t.key(a[1].clone()));
    if objs.is_empty() {
        return a.get(3).cloned().ok_or_else(|| c.badarg());
    }
    let mut elems = Vec::new();
    for o in objs {
        elems.push(o.as_tuple().and_then(|e| e.get(pos - 1)).cloned().ok_or_else(|| c.badarg())?);
    }
    Ok(match t.kind {
        Kind::Set | Kind::OrderedSet => elems.pop().expect("one object"),
        Kind::Bag | Kind::DuplicateBag => Term::list(elems),
    })
}

/// `delete(Tab)` deletes the table (owner only); `delete(Tab, Key)` deletes objects.
pub fn delete(c: &mut Ctx, a: &[Term]) -> R {
    if a.len() == 1 {
        let tid = c.sys.ets.resolve(&a[0]).ok_or_else(|| c.badarg())?;
        if c.sys.ets.get(tid).expect("resolved").owner != c.p.pid
            && c.sys.ets.get(tid).expect("resolved").access != Access::Public
        {
            return Err(c.badarg());
        }
        c.sys.ets.delete(tid);
    } else {
        let t = table_mut(c, &a[0])?;
        let k = t.key(a[1].clone());
        t.remove(&k);
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn delete_object(c: &mut Ctx, a: &[Term]) -> R {
    let badarg = c.badarg();
    let t = table_mut(c, &a[0])?;
    let k = t.key_of(&a[1]).ok_or(badarg)?;
    t.remove_object(&k, &a[1]);
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn take(c: &mut Ctx, a: &[Term]) -> R {
    let t = table_mut(c, &a[0])?;
    let k = t.key(a[1].clone());
    Ok(Term::list(t.remove(&k)))
}

pub fn internal_delete_all(c: &mut Ctx, a: &[Term]) -> R {
    let n = table_mut(c, &a[0])?.clear();
    Ok(Term::Int(n as i64))
}

/// One `update_counter` operation on `obj`: `Incr`, `{Pos, Incr}` or
/// `{Pos, Incr, Threshold, SetValue}`. Returns the new object and the new counter value.
fn counter_op(c: &mut Ctx, obj: &Term, keypos: usize, op: &Term) -> Result<(Term, Term), Exception> {
    let (pos, incr, limit) = match op {
        // A bare increment updates the element after the key.
        Term::Int(_) | Term::Big(_) => (keypos + 1, op.clone(), None),
        Term::Tuple(t) => match &t[..] {
            [p, i] => (p.as_usize().ok_or_else(|| c.badarg())?, i.clone(), None),
            [p, i, th, sv] => (p.as_usize().ok_or_else(|| c.badarg())?, i.clone(), Some((th.clone(), sv.clone()))),
            _ => return Err(c.badarg()),
        },
        _ => return Err(c.badarg()),
    };
    let mut elems = obj.as_tuple().ok_or_else(|| c.badarg())?.to_vec();
    if pos == keypos || pos == 0 || pos > elems.len() || !elems[pos - 1].is_integer() || !incr.is_integer() {
        return Err(c.badarg());
    }
    let mut new = super::arith::add(c, &[elems[pos - 1].clone(), incr.clone()])?;
    if let Some((threshold, set_value)) = limit {
        let over = if incr.cmp_term(&Term::Int(0)) != core::cmp::Ordering::Less {
            new.cmp_term(&threshold) == core::cmp::Ordering::Greater
        } else {
            new.cmp_term(&threshold) == core::cmp::Ordering::Less
        };
        if over {
            new = set_value;
        }
    }
    elems[pos - 1] = new.clone();
    Ok((Term::tuple(elems), new))
}

/// `update_counter(Tab, Key, Op | [Op])` and `update_counter(Tab, Key, Op, Default)`.
pub fn update_counter(c: &mut Ctx, a: &[Term]) -> R {
    let (key, keypos, current, kind) = {
        let t = table_mut(c, &a[0])?;
        let key = t.key(a[1].clone());
        (key.clone(), t.keypos, t.lookup(&key).first().cloned(), t.kind)
    };
    if !matches!(kind, Kind::Set | Kind::OrderedSet) {
        return Err(c.badarg());
    }
    let mut obj = match (current, a.get(3)) {
        (Some(o), _) => o,
        (None, Some(default)) => {
            // The default object must have the key in the right place; its key is replaced.
            let mut e = default.as_tuple().ok_or_else(|| c.badarg())?.to_vec();
            if e.len() < keypos {
                return Err(c.badarg());
            }
            e[keypos - 1] = a[1].clone();
            Term::tuple(e)
        }
        (None, None) => return Err(c.badarg()),
    };
    let (ops, many) = match &a[2] {
        Term::Nil | Term::Cons(_) => (a[2].to_vec().ok_or_else(|| c.badarg())?, true),
        op => (alloc::vec![op.clone()], false),
    };
    let mut results = Vec::new();
    for op in &ops {
        let (o, v) = counter_op(c, &obj, keypos, op)?;
        obj = o;
        results.push(v);
    }
    let t = table_mut(c, &a[0])?;
    if t.contains(&key) {
        t.replace(&key, obj);
    } else {
        t.insert(key, obj);
    }
    Ok(if many { Term::list(results) } else { results.pop().expect("one op") })
}

/// `update_element(Tab, Key, {Pos, Value} | [{Pos, Value}])`: `false` if there is no object.
pub fn update_element(c: &mut Ctx, a: &[Term]) -> R {
    let badarg = c.badarg();
    let t = table_mut(c, &a[0])?;
    if !matches!(t.kind, Kind::Set | Kind::OrderedSet) {
        return Err(badarg);
    }
    let key = t.key(a[1].clone());
    let keypos = t.keypos;
    let Some(obj) = t.lookup(&key).first().cloned() else { return Ok(Term::Atom(c.sys.atoms.false_.clone())) };
    let changes = match &a[2] {
        Term::Tuple(_) => alloc::vec![a[2].clone()],
        other => other.to_vec().ok_or_else(|| c.badarg())?,
    };
    let mut e = obj.as_tuple().expect("objects are tuples").to_vec();
    for ch in changes {
        match ch.as_tuple() {
            Some([p, v]) => match p.as_usize() {
                Some(p) if p >= 1 && p <= e.len() && p != keypos => e[p - 1] = v.clone(),
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    let t = table_mut(c, &a[0])?;
    t.replace(&key, Term::tuple(e));
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

// ---- traversal ----

fn key_or_end(c: &mut Ctx, k: Option<Term>) -> Term {
    k.unwrap_or_else(|| end_of_table(c))
}

pub fn first(c: &mut Ctx, a: &[Term]) -> R {
    let k = table(c, &a[0])?.first().map(|k| k.term.clone());
    Ok(key_or_end(c, k))
}

pub fn last(c: &mut Ctx, a: &[Term]) -> R {
    let k = table(c, &a[0])?.last().map(|k| k.term.clone());
    Ok(key_or_end(c, k))
}

pub fn next(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let k = t.next(&t.key(a[1].clone())).map(|k| k.term.clone());
    Ok(key_or_end(c, k))
}

pub fn prev(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let k = t.prev(&t.key(a[1].clone())).map(|k| k.term.clone());
    Ok(key_or_end(c, k))
}

/// `first_lookup/1` and friends: `{Key, Objects}` or `'$end_of_table'`.
fn with_objects(c: &mut Ctx, id: &Term, k: Option<Term>) -> R {
    match k {
        None => Ok(end_of_table(c)),
        Some(k) => {
            let t = table(c, id)?;
            let objs = Term::list(t.lookup(&t.key(k.clone())).to_vec());
            Ok(Term::tuple(alloc::vec![k, objs]))
        }
    }
}

pub fn first_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let k = table(c, &a[0])?.first().map(|k| k.term.clone());
    with_objects(c, &a[0], k)
}

pub fn last_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let k = table(c, &a[0])?.last().map(|k| k.term.clone());
    with_objects(c, &a[0], k)
}

pub fn next_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let k = t.next(&t.key(a[1].clone())).map(|k| k.term.clone());
    with_objects(c, &a[0], k)
}

pub fn prev_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let t = table(c, &a[0])?;
    let k = t.prev(&t.key(a[1].clone())).map(|k| k.term.clone());
    with_objects(c, &a[0], k)
}

// ---- matching ----

/// Evaluate a guard or body expression of a match specification.
fn eval(c: &mut Ctx, e: &Term, obj: &Term, b: &Bindings, depth: usize) -> Option<Term> {
    if depth > MAX_EXPR_DEPTH {
        return None;
    }
    Some(match e {
        Term::Atom(a) if a.as_str() == "$_" => obj.clone(),
        Term::Atom(a) if a.as_str() == "$$" => ets::all_bindings(b),
        Term::Atom(a) if ets::variable(a).is_some() => b.get(&ets::variable(a).expect("checked"))?.clone(),
        Term::Cons(_) => {
            let mut items = Vec::new();
            let mut tail = Term::Nil;
            for x in e.list_iter() {
                match x {
                    Ok(x) => items.push(eval(c, &x, obj, b, depth + 1)?),
                    Err(t) => tail = eval(c, &t, obj, b, depth + 1)?,
                }
            }
            Term::list_with_tail(items, tail)
        }
        Term::Map(m) => {
            let mut out = crate::term::Map::new();
            for (k, v) in m.iter() {
                out.insert(k.clone(), eval(c, v, obj, b, depth + 1)?);
            }
            Term::map(out)
        }
        Term::Tuple(t) => match &t[..] {
            // {const, X}: X, unevaluated. {{...}}: a tuple of evaluated elements.
            [Term::Atom(k), x] if k.as_str() == "const" => x.clone(),
            [Term::Tuple(inner)] => {
                let mut elems = Vec::with_capacity(inner.len());
                for x in inner.iter() {
                    elems.push(eval(c, x, obj, b, depth + 1)?);
                }
                Term::tuple(elems)
            }
            [Term::Atom(f), args @ ..] => call(c, f.as_str(), args, obj, b, depth)?,
            _ => return None,
        },
        other => other.clone(),
    })
}

fn call(c: &mut Ctx, f: &str, args: &[Term], obj: &Term, b: &Bindings, depth: usize) -> Option<Term> {
    let t = Term::Atom(c.sys.atoms.true_.clone());
    match f {
        "andalso" | "orelse" => {
            // Short-circuit, left to right.
            let stop_on = f == "orelse";
            for x in args {
                let v = eval(c, x, obj, b, depth + 1)?;
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
                vals.push(eval(c, x, obj, b, depth + 1)?);
            }
            let erlang = c.sys.atoms.erlang.clone();
            let fname = c.sys.atom(f);
            let n = c.sys.native(&erlang, &fname, args.len() as u32)?;
            let _ = t;
            n(c, &vals).ok()
        }
        _ => None,
    }
}

/// Run a match specification over one object: the body's last value, if some clause matches.
fn run_spec(c: &mut Ctx, spec: &[Clause], obj: &Term) -> Option<Term> {
    for clause in spec {
        let mut b = Bindings::new();
        if !ets::pattern_match(&clause.head, obj, &mut b) {
            continue;
        }
        let guards_pass = clause.guards.iter().all(|g| {
            eval(c, g, obj, &b, 0).is_some_and(|v| v.is_atom(&c.sys.atoms.true_))
        });
        if !guards_pass {
            continue;
        }
        let mut result = None;
        for expr in &clause.body {
            result = Some(eval(c, expr, obj, &b, 0)?);
        }
        return result;
    }
    None
}

fn spec(c: &Ctx, t: &Term) -> Result<Vec<Clause>, Exception> {
    // A "compiled" match spec (from `match_spec_compile/1`) is the spec itself.
    let t = match t.as_tuple() {
        Some([Term::Atom(tag), ms]) if tag.as_str() == "$ms" => ms,
        _ => t,
    };
    ets::parse_spec(t).ok_or_else(|| c.badarg())
}

/// Objects of `id` for which `spec` gives a result, with those results, in key order.
fn select_all(c: &mut Ctx, id: &Term, spec: &[Clause]) -> Result<Vec<(Term, Term)>, Exception> {
    let objs = table(c, id)?.all();
    let mut out = Vec::new();
    for o in objs {
        if let Some(r) = run_spec(c, spec, &o) {
            out.push((o, r));
        }
    }
    Ok(out)
}

pub fn select(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    Ok(Term::list(found.into_iter().map(|(_, r)| r).collect::<Vec<_>>()))
}

/// `select(Tab, MS, Limit)`: everything in one chunk, then `'$end_of_table'`.
pub fn select3(c: &mut Ctx, a: &[Term]) -> R {
    a[2].as_usize().filter(|n| *n >= 1).ok_or_else(|| c.badarg())?;
    let all = select(c, &a[..2])?;
    if matches!(all, Term::Nil) {
        return Ok(end_of_table(c));
    }
    let end = end_of_table(c);
    Ok(Term::tuple(alloc::vec![all, end]))
}

/// `select(Continuation)`: this VM always returns everything in the first chunk.
pub fn select1(c: &mut Ctx, a: &[Term]) -> R {
    if a[0].is_atom(&c.sys.atom("$end_of_table")) {
        Ok(end_of_table(c))
    } else {
        Err(c.badarg())
    }
}

pub fn select_reverse(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    Ok(Term::list(found.into_iter().rev().map(|(_, r)| r).collect::<Vec<_>>()))
}

pub fn select_count(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let found = select_all(c, &a[0], &s)?;
    let n = found.iter().filter(|(_, r)| r.is_atom(&c.sys.atoms.true_)).count();
    Ok(Term::Int(n as i64))
}

pub fn internal_select_delete(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    table_id(c, &a[0], true)?;
    let found = select_all(c, &a[0], &s)?;
    let doomed: Vec<Term> = found.into_iter().filter(|(_, r)| r.is_atom(&c.sys.atoms.true_)).map(|(o, _)| o).collect();
    let t = table_mut(c, &a[0])?;
    let mut n = 0;
    for o in &doomed {
        let k = t.key_of(o).expect("stored objects have keys");
        n += t.remove_object(&k, o);
    }
    Ok(Term::Int(n as i64))
}

/// `select_replace(Tab, MS)`: replace each matching object with the body's result, which must
/// keep the key. Returns the number replaced.
pub fn select_replace(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    table_id(c, &a[0], true)?;
    let found = select_all(c, &a[0], &s)?;
    room_for(c, found.iter().map(|(_, new)| new))?;
    let t = table_mut(c, &a[0])?;
    let mut n = 0;
    for (old, new) in found {
        let (Some(k_old), Some(k_new)) = (t.key_of(&old), t.key_of(&new)) else { continue };
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
    alloc::vec![Clause { head: pattern.clone(), guards: Vec::new(), body: alloc::vec![b] }]
}

/// `match(Tab, Pattern)`: the bindings of each match, as lists.
pub fn match_(c: &mut Ctx, a: &[Term]) -> R {
    let s = pattern_spec(c, &a[1], "$$");
    let found = select_all(c, &a[0], &s)?;
    Ok(Term::list(found.into_iter().map(|(_, r)| r).collect::<Vec<_>>()))
}

pub fn match_object(c: &mut Ctx, a: &[Term]) -> R {
    let s = pattern_spec(c, &a[1], "$_");
    let found = select_all(c, &a[0], &s)?;
    Ok(Term::list(found.into_iter().map(|(o, _)| o).collect::<Vec<_>>()))
}

pub fn match_spec_compile(c: &mut Ctx, a: &[Term]) -> R {
    spec(c, &a[0])?;
    Ok(Term::tuple(alloc::vec![c.atom("$ms"), a[0].clone()]))
}

pub fn is_compiled_ms(c: &mut Ctx, a: &[Term]) -> R {
    let ok = matches!(a[0].as_tuple(), Some([Term::Atom(tag), _]) if tag.as_str() == "$ms");
    Ok(c.bool(ok))
}

/// `match_spec_run_r(List, CompiledMS, Acc)`: results for `List`, reversed onto `Acc`.
pub fn match_spec_run_r(c: &mut Ctx, a: &[Term]) -> R {
    let s = spec(c, &a[1])?;
    let mut acc = a[2].clone();
    for o in a[0].to_vec().ok_or_else(|| c.badarg())? {
        if let Some(r) = run_spec(c, &s, &o) {
            acc = Term::cons(r, acc);
        }
    }
    Ok(acc)
}

// ---- table information and ownership ----

fn info_value(c: &mut Ctx, t_id: u64, item: &str) -> Option<Term> {
    let t = c.sys.ets.get(t_id)?;
    let (name, named, kind, access, keypos, owner, size, heir) =
        (t.name.clone(), t.named, t.kind, t.access, t.keypos, t.owner, t.size(), t.heir.clone());
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
            Some((p, _)) => Term::Pid(p),
            None => c.atom("none"),
        },
        "compressed" | "read_concurrency" | "write_concurrency" | "decentralized_counters" => c.bool(false),
        _ => return None,
    })
}

pub fn info(c: &mut Ctx, a: &[Term]) -> R {
    let Some(tid) = c.sys.ets.resolve(&a[0]) else {
        return if matches!(a[0], Term::Atom(_) | Term::Ref(_)) { Ok(Term::Atom(c.sys.atoms.undefined.clone())) } else { Err(c.badarg()) };
    };
    match a.get(1) {
        Some(Term::Atom(item)) => info_value(c, tid, item.as_str()).ok_or_else(|| c.badarg()),
        Some(_) => Err(c.badarg()),
        None => {
            let mut out = Vec::new();
            for item in ["id", "name", "named_table", "type", "protection", "keypos", "owner", "size", "heir"] {
                let v = info_value(c, tid, item).expect("known item");
                out.push(Term::tuple(alloc::vec![c.atom(item), v]));
            }
            Ok(Term::list(out))
        }
    }
}

pub fn whereis(c: &mut Ctx, a: &[Term]) -> R {
    Ok(match c.sys.ets.resolve(&a[0]) {
        Some(tid) if matches!(a[0], Term::Atom(_)) => Term::Ref(Ref(tid)),
        _ => Term::Atom(c.sys.atoms.undefined.clone()),
    })
}

pub fn rename(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[1] else { return Err(c.badarg()) };
    let tid = table_id(c, &a[0], true)?;
    c.sys.ets.rename(tid, name.clone()).map_err(|_| c.badarg())?;
    Ok(a[1].clone())
}

/// Transfer a table to a new owner, which receives `{'ETS-TRANSFER', Tab, FromPid, Data}`.
pub(crate) fn transfer(c: &mut Ctx, tid: u64, to: Pid, data: Term) {
    let Some(t) = c.sys.ets.get_mut(tid) else { return };
    let from = t.owner;
    t.owner = to;
    let id = table_term(t);
    let msg = Term::tuple(alloc::vec![c.atom("ETS-TRANSFER"), id, Term::Pid(from), data]);
    super::proc::send_to(c, to, msg);
}

pub fn give_away(c: &mut Ctx, a: &[Term]) -> R {
    let tid = c.sys.ets.resolve(&a[0]).ok_or_else(|| c.badarg())?;
    let Term::Pid(to) = a[1] else { return Err(c.badarg()) };
    let owner = c.sys.ets.get(tid).expect("resolved").owner;
    if owner != c.p.pid || to == c.p.pid || !c.sys.procs.is_alive(to) {
        return Err(c.badarg());
    }
    transfer(c, tid, to, a[2].clone());
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

/// `setopts(Tab, Opts)`: only `{heir, ...}` is supported (by the owner).
pub fn setopts(c: &mut Ctx, a: &[Term]) -> R {
    let tid = c.sys.ets.resolve(&a[0]).ok_or_else(|| c.badarg())?;
    if c.sys.ets.get(tid).expect("resolved").owner != c.p.pid {
        return Err(c.badarg());
    }
    let opts = match &a[1] {
        Term::Tuple(_) => alloc::vec![a[1].clone()],
        other => other.to_vec().ok_or_else(|| c.badarg())?,
    };
    for o in opts {
        let heir = match o.as_tuple() {
            Some([Term::Atom(k), Term::Atom(none)]) if k.as_str() == "heir" && none.as_str() == "none" => None,
            Some([Term::Atom(k), Term::Pid(p), data]) if k.as_str() == "heir" => Some((*p, data.clone())),
            _ => return Err(c.badarg()),
        };
        c.sys.ets.get_mut(tid).expect("resolved").heir = heir;
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn safe_fixtable(c: &mut Ctx, a: &[Term]) -> R {
    table(c, &a[0])?;
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn all(c: &mut Ctx, _a: &[Term]) -> R {
    let mut out = Vec::new();
    for tid in c.sys.ets.tids() {
        let t = c.sys.ets.get(tid).expect("listed");
        out.push(table_term(t));
    }
    Ok(Term::list(out))
}
