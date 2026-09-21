//! `atomics` and `counters`: arrays of 64-bit integers shared by reference between processes.
//!
//! The VM runs one process at a time, so a `RefCell` gives every operation the atomicity the
//! Erlang API promises. Values wrap around at 64 bits, as BEAM's do; an array is either signed
//! (`-2^63..2^63-1`) or unsigned (`0..2^64-1`), and `counters` are always signed.

use crate::sync::Lock;
use alloc::vec::Vec;

use num_bigint::BigInt;

use super::Ctx;
use crate::process::Exception;
use crate::term::{Resource, Term};

type R = Result<Term, Exception>;

/// Longest array one call may create.
const MAX_SIZE: usize = 1 << 24;

struct Atomics {
    signed: bool,
    cells: Lock<Vec<u64>>,
}

fn new(c: &mut Ctx, size: &Term, signed: bool) -> R {
    // Too large is a system limit (as in BEAM); zero, negative or not an integer is badarg.
    let n = match size {
        Term::Int(n) if *n >= 1 && (*n as u64) <= MAX_SIZE as u64 => *n as usize,
        Term::Int(n) if *n >= 1 => return Err(c.system_limit()),
        Term::Big(_)
            if c.heap()
                .as_big(*size)
                .is_some_and(|b| b.sign() == num_bigint::Sign::Plus) =>
        {
            return Err(c.system_limit())
        }
        _ => return Err(c.badarg()),
    };
    let id = c.sys().make_ref().0;
    let a = Atomics {
        signed,
        cells: Lock::new(alloc::vec![0; n]),
    };
    Ok(c.heap_mut().resource(Resource {
        id,
        value: alloc::boxed::Box::new(a),
    }))
}

/// `erts_internal:atomics_new(Arity, EncodedOpts)`: bit 0 of the options is `signed`.
pub fn atomics_new(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Int(opts) = a[1] else {
        return Err(c.badarg());
    };
    new(c, &a[0], opts & 1 != 0)
}

/// `erts_internal:counters_new(Size)`.
pub fn counters_new(c: &mut Ctx, a: &[Term]) -> R {
    new(c, &a[0], true)
}

/// The array and the 0-based index an `(Ref, Ix)` pair names. The array is shared (an `Arc`),
/// so the caller's heap stays free for building the result.
fn cell(c: &Ctx, r: &Term, ix: &Term) -> Result<(alloc::sync::Arc<Resource>, usize), Exception> {
    let res = c.heap().as_resource(*r).ok_or_else(|| c.badarg())?.clone();
    let a = res.get::<Atomics>().ok_or_else(|| c.badarg())?;
    let i = ix
        .as_usize()
        .filter(|&i| i >= 1 && i <= a.cells.lock().len())
        .ok_or_else(|| c.badarg())?;
    Ok((res, i - 1))
}

fn atomics(r: &Resource) -> &Atomics {
    r.get::<Atomics>().expect("checked by cell")
}

fn to_term(c: &mut Ctx, a: &Atomics, v: u64) -> Term {
    if a.signed {
        Term::Int(v as i64)
    } else {
        c.big(BigInt::from(v))
    }
}

/// An integer argument, however large.
fn integer(c: &Ctx, t: &Term) -> Option<i128> {
    match t {
        Term::Int(i) => Some(*i as i128),
        Term::Big(_) => i128::try_from(c.heap().as_big(*t)?).ok(),
        _ => None,
    }
}

/// A value to store: in the array's range.
fn value(c: &Ctx, a: &Atomics, t: &Term) -> Result<u64, Exception> {
    let n = integer(c, t);
    let n = n.ok_or_else(|| c.badarg())?;
    let ok = if a.signed {
        i64::try_from(n).is_ok()
    } else {
        u64::try_from(n).is_ok()
    };
    if ok {
        Ok(n as u64)
    } else {
        Err(c.badarg())
    }
}

/// An increment: any integer that fits in 64 bits either way; it wraps like the cell does.
fn incr(c: &Ctx, t: &Term) -> Result<u64, Exception> {
    let n = integer(c, t);
    match n {
        Some(n) if i64::try_from(n).is_ok() || u64::try_from(n).is_ok() => Ok(n as u64),
        _ => Err(c.badarg()),
    }
}

pub fn put(c: &mut Ctx, a: &[Term]) -> R {
    let (res, i) = cell(c, &a[0], &a[1])?;
    let at = atomics(&res);
    let v = value(c, at, &a[2])?;
    at.cells.lock()[i] = v;
    Ok(c.ok())
}

pub fn get(c: &mut Ctx, a: &[Term]) -> R {
    let (res, i) = cell(c, &a[0], &a[1])?;
    let at = atomics(&res);
    let v = at.cells.lock()[i];
    Ok(to_term(c, at, v))
}

pub fn add(c: &mut Ctx, a: &[Term]) -> R {
    add_get(c, a)?;
    Ok(c.ok())
}

pub fn add_get(c: &mut Ctx, a: &[Term]) -> R {
    let (res, i) = cell(c, &a[0], &a[1])?;
    let at = atomics(&res);
    let d = incr(c, &a[2])?;
    let v = {
        let mut cells = at.cells.lock();
        cells[i] = cells[i].wrapping_add(d);
        cells[i]
    };
    Ok(to_term(c, at, v))
}

pub fn exchange(c: &mut Ctx, a: &[Term]) -> R {
    let (res, i) = cell(c, &a[0], &a[1])?;
    let at = atomics(&res);
    let v = value(c, at, &a[2])?;
    let old = core::mem::replace(&mut at.cells.lock()[i], v);
    Ok(to_term(c, at, old))
}

/// `compare_exchange(Ref, Ix, Expected, Desired)`: `ok`, or the value found instead.
pub fn compare_exchange(c: &mut Ctx, a: &[Term]) -> R {
    let (res, i) = cell(c, &a[0], &a[1])?;
    let at = atomics(&res);
    let (expected, desired) = (value(c, at, &a[2])?, value(c, at, &a[3])?);
    let found = {
        let mut cells = at.cells.lock();
        if cells[i] == expected {
            cells[i] = desired;
            return Ok(c.ok());
        }
        cells[i]
    };
    Ok(to_term(c, at, found))
}

/// `info(Ref)`: `#{size, max, min, memory}` for atomics, `#{size, memory}` for counters.
fn info(c: &mut Ctx, a: &[Term], counters: bool) -> R {
    let res = c
        .heap()
        .as_resource(a[0])
        .ok_or_else(|| c.badarg())?
        .clone();
    let at = res.get::<Atomics>().ok_or_else(|| c.badarg())?;
    let n = at.cells.lock().len();
    let mut items: Vec<(&str, Term)> = alloc::vec![
        ("size", Term::Int(n as i64)),
        ("memory", Term::Int((n * 8 + 32) as i64))
    ];
    if !counters {
        let (min, max) = if at.signed {
            (Term::Int(i64::MIN), Term::Int(i64::MAX))
        } else {
            (Term::Int(0), c.big(BigInt::from(u64::MAX)))
        };
        items.push(("min", min));
        items.push(("max", max));
    }
    let pairs: Vec<(Term, Term)> = items.into_iter().map(|(k, v)| (c.atom(k), v)).collect();
    Ok(c.map_from(pairs))
}

pub fn atomics_info(c: &mut Ctx, a: &[Term]) -> R {
    info(c, a, false)
}

pub fn counters_info(c: &mut Ctx, a: &[Term]) -> R {
    info(c, a, true)
}
