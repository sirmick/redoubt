//! `maps` BIFs and the map BIFs of `erlang`.

use alloc::vec::Vec;

use super::Ctx;
use crate::process::Exception;
use crate::term::{Heap, Term};

type R = Result<Term, Exception>;

/// `t` if it is a map, else `{badmap, T}`.
fn map(c: &mut Ctx, t: &Term) -> Result<Term, Exception> {
    match t {
        Term::Map(_) => Ok(*t),
        _ => {
            let tag = c.atoms.badmap;
            Err(c.error_with(&tag, *t))
        }
    }
}

fn badkey(c: &mut Ctx, k: &Term) -> Exception {
    let tag = c.atoms.badkey;
    c.error_with(&tag, *k)
}

fn entries(c: &Ctx, m: Term) -> Vec<(Term, Term)> { c.heap().map_entries(m).expect("a map") }

pub fn map_size(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    Ok(Term::Int(c.heap().map_len(m).expect("a map") as i64))
}

/// `erlang:map_get(Key, Map)`.
pub fn get(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    match c.heap().map_get(m, a[0]) {
        Some(v) => Ok(v),
        None => Err(badkey(c, &a[0])),
    }
}

/// `maps:get(Key, Map)`.
pub fn get_rev(c: &mut Ctx, a: &[Term]) -> R { get(c, a) }

pub fn find(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(match c.heap().map_get(m, a[0]) {
        Some(v) => c.ok_tuple(v),
        None => Term::Atom(c.atoms.error),
    })
}

pub fn is_key(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(c.bool(c.heap().map_get(m, a[0]).is_some()))
}

/// `erlang:is_map_key(Key, Map)`, same argument order as `maps:is_key/2`.
pub fn is_key_rev(c: &mut Ctx, a: &[Term]) -> R { is_key(c, a) }

pub fn put(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[2])?;
    Ok(c.heap_mut().map_put(m, a[0], a[1]))
}

pub fn remove(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(c.heap_mut().map_remove(m, a[0]))
}

pub fn take(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    match c.heap().map_get(m, a[0]) {
        Some(v) => {
            let rest = c.heap_mut().map_remove(m, a[0]);
            Ok(c.tuple(&[v, rest]))
        }
        None => Ok(Term::Atom(c.atoms.error)),
    }
}

pub fn update(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[2])?;
    if c.heap().map_get(m, a[0]).is_none() {
        return Err(badkey(c, &a[0]));
    }
    Ok(c.heap_mut().map_put(m, a[0], a[1]))
}

pub fn merge(c: &mut Ctx, a: &[Term]) -> R {
    let mut left = map(c, &a[0])?;
    let right = map(c, &a[1])?;
    for (k, v) in entries(c, right) {
        left = c.heap_mut().map_put(left, k, v);
    }
    Ok(left)
}

/// The entries of map `m` as a list in key order, each `cells` cells made by `entry`, their
/// room asked for first.
fn listed(c: &mut Ctx, m: Term, cells: usize, entry: impl Fn(&mut Heap, Term, Term) -> Term) -> R {
    let n = c.heap().map_len(m).expect("a map");
    c.room(n * (2 + cells))?;
    Ok(c.heap_mut().map_list(m, entry))
}

pub fn keys(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    listed(c, m, 0, |_, k, _| k)
}

pub fn values(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    listed(c, m, 0, |_, _, v| v)
}

pub fn to_list(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    listed(c, m, 3, |h, k, v| h.tuple(&[k, v]))
}

/// A map of the `(key, value)` pairs `pair` finds in each element of list `t` (`badarg` for one
/// it refuses), later pairs winning, as in Erlang: the pairs held aside and the map's room
/// asked for first.
fn map_of(c: &mut Ctx, t: Term, pair: impl Fn(&Heap, Term) -> Option<(Term, Term)>) -> R {
    let n = c.list_len(t)?;
    let mut pairs = Vec::new();
    c.held(&mut pairs, n)?;
    for (i, item) in c.heap().list_iter(t).map_while(Result::ok).enumerate() {
        let (k, v) = pair(c.heap(), item).ok_or_else(|| c.badarg())?;
        pairs.push((k, v, i));
    }
    c.room(Heap::map_cells(n))?;
    Ok(c.heap_mut().map_of(&mut pairs))
}

pub fn from_list(c: &mut Ctx, a: &[Term]) -> R {
    map_of(c, a[0], |h, item| match h.as_tuple(item) {
        Some(&[k, v]) => Some((k, v)),
        _ => None,
    })
}

pub fn from_keys(c: &mut Ctx, a: &[Term]) -> R {
    let v = a[1];
    map_of(c, a[0], |_, k| Some((k, v)))
}

/// `erts_internal:map_next(Path, Map, Mode)`, the engine behind `maps:next/1`, `maps:to_list/1`
/// and every `maps` function that iterates.
///
/// Mode `iterator`: return `{K, V, Next}` or `none`. `Path` is either an integer (a fresh
/// iterator) or the list of keys still to visit (an ordered iterator, or one we started). An
/// integer path is turned into a key list on the first step, so walking a map is linear.
///
/// Mode `Acc` (a list): return every pair prepended to `Acc`, in key order.
pub fn map_next(c: &mut Ctx, a: &[Term]) -> R {
    let m = match a[1] {
        Term::Map(_) => a[1],
        _ => return Err(c.badarg()),
    };
    let iterator = a[2].is_atom(&c.sys().atom("iterator"));
    if !iterator {
        if !matches!(a[2], Term::Nil | Term::Cons(_)) || !a[0].is_integer() {
            return Err(c.badarg());
        }
        let pairs: Vec<Term> = entries(c, m).into_iter().map(|(k, v)| c.tuple(&[k, v])).collect();
        return Ok(c.list_with_tail(pairs, a[2]));
    }
    let keys: Term = match a[0] {
        Term::Int(0) => {
            let keys: Vec<Term> = entries(c, m).into_iter().map(|(k, _)| k).collect();
            c.list(keys)
        }
        Term::Nil | Term::Cons(_) => a[0],
        _ => return Err(c.badarg()),
    };
    match c.heap().as_cons(keys) {
        None if matches!(keys, Term::Nil) => Ok(c.atom("none")),
        Some((key, rest)) => {
            let v = c.heap().map_get(m, key).ok_or_else(|| c.badarg())?;
            let next = match rest {
                Term::Nil => c.atom("none"),
                rest => c.cons(rest, m),
            };
            Ok(c.tuple(&[key, v, next]))
        }
        None => Err(c.badarg()),
    }
}
