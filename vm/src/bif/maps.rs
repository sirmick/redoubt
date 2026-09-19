//! `maps` BIFs and the map BIFs of `erlang`.

use alloc::rc::Rc;
use alloc::vec::Vec;

use super::Ctx;
use crate::process::Exception;
use crate::term::{Map, MapKey, Term};

type R = Result<Term, Exception>;

fn map<'t>(c: &Ctx, t: &'t Term) -> Result<&'t Rc<Map>, Exception> {
    match t {
        Term::Map(m) => Ok(m),
        _ => Err(c.error_with(&c.sys.atoms.badmap, t.clone())),
    }
}

fn badkey(c: &Ctx, k: &Term) -> Exception {
    c.error_with(&c.sys.atoms.badkey, k.clone())
}

pub fn map_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(map(c, &a[0])?.len() as i64))
}

/// `erlang:map_get(Key, Map)`.
pub fn get(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    m.get(&MapKey(a[0].clone())).cloned().ok_or_else(|| badkey(c, &a[0]))
}

/// `maps:get(Key, Map)`.
pub fn get_rev(c: &mut Ctx, a: &[Term]) -> R {
    get(c, a)
}

pub fn find(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(match m.get(&MapKey(a[0].clone())) {
        Some(v) => Term::tuple(alloc::vec![c.ok(), v.clone()]),
        None => Term::Atom(c.sys.atoms.error.clone()),
    })
}

pub fn is_key(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[1])?;
    Ok(c.bool(m.contains_key(&MapKey(a[0].clone()))))
}

/// `erlang:is_map_key(Key, Map)`, same argument order as `maps:is_key/2`.
pub fn is_key_rev(c: &mut Ctx, a: &[Term]) -> R {
    is_key(c, a)
}

pub fn put(c: &mut Ctx, a: &[Term]) -> R {
    let mut m = map(c, &a[2])?.clone();
    Rc::make_mut(&mut m).insert(MapKey(a[0].clone()), a[1].clone());
    Ok(Term::Map(m))
}

pub fn remove(c: &mut Ctx, a: &[Term]) -> R {
    let mut m = map(c, &a[1])?.clone();
    Rc::make_mut(&mut m).remove(&MapKey(a[0].clone()));
    Ok(Term::Map(m))
}

pub fn take(c: &mut Ctx, a: &[Term]) -> R {
    let mut m = map(c, &a[1])?.clone();
    match Rc::make_mut(&mut m).remove(&MapKey(a[0].clone())) {
        Some(v) => Ok(Term::tuple(alloc::vec![v, Term::Map(m)])),
        None => Ok(Term::Atom(c.sys.atoms.error.clone())),
    }
}

pub fn update(c: &mut Ctx, a: &[Term]) -> R {
    let mut m = map(c, &a[2])?.clone();
    let k = MapKey(a[0].clone());
    if !m.contains_key(&k) {
        return Err(badkey(c, &a[0]));
    }
    Rc::make_mut(&mut m).insert(k, a[1].clone());
    Ok(Term::Map(m))
}

pub fn merge(c: &mut Ctx, a: &[Term]) -> R {
    let mut left = map(c, &a[0])?.clone();
    let right = map(c, &a[1])?;
    let l = Rc::make_mut(&mut left);
    for (k, v) in right.iter() {
        l.insert(k.clone(), v.clone());
    }
    Ok(Term::Map(left))
}

pub fn keys(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::list(map(c, &a[0])?.keys().map(|k| k.0.clone()).collect::<Vec<_>>()))
}

pub fn values(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::list(map(c, &a[0])?.values().cloned().collect::<Vec<_>>()))
}

pub fn to_list(c: &mut Ctx, a: &[Term]) -> R {
    let m = map(c, &a[0])?;
    Ok(Term::list(m.iter().map(|(k, v)| Term::tuple(alloc::vec![k.0.clone(), v.clone()])).collect::<Vec<_>>()))
}

pub fn from_list(c: &mut Ctx, a: &[Term]) -> R {
    let items = a[0].to_vec().ok_or_else(|| c.badarg())?;
    let mut m = Map::new();
    for item in items {
        match item.as_tuple() {
            Some([k, v]) => {
                // Later pairs win, as in Erlang.
                m.insert(MapKey(k.clone()), v.clone());
            }
            _ => return Err(c.badarg()),
        }
    }
    Ok(Term::map(m))
}

pub fn from_keys(c: &mut Ctx, a: &[Term]) -> R {
    let keys = a[0].to_vec().ok_or_else(|| c.badarg())?;
    Ok(Term::map(keys.into_iter().map(|k| (MapKey(k), a[1].clone())).collect()))
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
    let m = match &a[1] {
        Term::Map(m) => m.clone(),
        _ => return Err(c.badarg()),
    };
    let iterator = a[2].is_atom(&c.sys.atom("iterator"));
    if !iterator {
        if !matches!(a[2], Term::Nil | Term::Cons(_)) || !a[0].is_integer() {
            return Err(c.badarg());
        }
        let pairs: Vec<Term> = m.iter().map(|(k, v)| Term::tuple(alloc::vec![k.0.clone(), v.clone()])).collect();
        return Ok(Term::list_with_tail(pairs, a[2].clone()));
    }
    let keys: Term = match &a[0] {
        Term::Int(0) => Term::list(m.keys().map(|k| k.0.clone()).collect::<Vec<_>>()),
        Term::Nil | Term::Cons(_) => a[0].clone(),
        _ => return Err(c.badarg()),
    };
    match &keys {
        Term::Nil => Ok(c.atom("none")),
        Term::Cons(cell) => {
            let v = m.get(&MapKey(cell.head.clone())).cloned().ok_or_else(|| c.badarg())?;
            let next = match &cell.tail {
                Term::Nil => c.atom("none"),
                rest => Term::cons(rest.clone(), a[1].clone()),
            };
            Ok(Term::tuple(alloc::vec![cell.head.clone(), v, next]))
        }
        _ => Err(c.badarg()),
    }
}
