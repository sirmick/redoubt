//! `maps` BIFs and the map BIFs of `erlang`.

use alloc::vec::Vec;

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// `t` if it is a map, else `{badmap, T}`.
fn map(c: &mut Ctx, t: &Term) -> Result<Term, Exception> {
    match t {
        Term::Map(_) => Ok(*t),
        _ => {
            let tag = c.sys.atoms.badmap;
            Err(c.error_with(&tag, *t))
        }
    }
}

fn badkey(c: &mut Ctx, k: &Term) -> Exception {
    let tag = c.sys.atoms.badkey;
    c.error_with(&tag, *k)
}

fn entries(c: &Ctx, m: Term) -> Vec<(Term, Term)> {
    c.heap().map_entries(m).expect("a map")
}

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
pub fn get_rev(c: &mut Ctx, a: &[Term]) -> R {
    get(c, a)
}

pub fn find(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(match c.heap().map_get(m, a[0]) {
        Some(v) => c.ok_tuple(v),
        None => Term::Atom(c.sys.atoms.error),
    })
}

pub fn is_key(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(c.bool(c.heap().map_get(m, a[0]).is_some()))
}

/// `erlang:is_map_key(Key, Map)`, same argument order as `maps:is_key/2`.
pub fn is_key_rev(c: &mut Ctx, a: &[Term]) -> R {
    is_key(c, a)
}

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
        None => Ok(Term::Atom(c.sys.atoms.error)),
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

pub fn keys(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    let keys: Vec<Term> = entries(c, m).into_iter().map(|(k, _)| k).collect();
    Ok(c.list(keys))
}

pub fn values(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    let values: Vec<Term> = entries(c, m).into_iter().map(|(_, v)| v).collect();
    Ok(c.list(values))
}

pub fn to_list(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    let pairs: Vec<Term> = entries(c, m).into_iter().map(|(k, v)| c.tuple(&[k, v])).collect();
    Ok(c.list(pairs))
}

pub fn from_list(c: &mut Ctx, a: &[Term]) -> R {
    let items = c.list_arg(a[0])?;
    let mut pairs = Vec::with_capacity(items.len());
    for item in items {
        match c.heap().as_tuple(item) {
            Some(&[k, v]) => pairs.push((k, v)),
            _ => return Err(c.badarg()),
        }
    }
    // Later pairs win, as in Erlang.
    Ok(c.map_from(pairs))
}

pub fn from_keys(c: &mut Ctx, a: &[Term]) -> R {
    let keys = c.list_arg(a[0])?;
    let v = a[1];
    Ok(c.map_from(keys.into_iter().map(|k| (k, v))))
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
    let iterator = a[2].is_atom(&c.sys.atom("iterator"));
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
