//! The `lists` functions BEAM implements natively. The rest of `lists` is the real OTP module.

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// `lists:reverse(List, Tail)`.
pub fn reverse(c: &mut Ctx, a: &[Term]) -> R {
    let mut acc = a[1].clone();
    for item in a[0].list_iter() {
        acc = Term::cons(item.map_err(|_| c.badarg())?, acc);
    }
    Ok(acc)
}

pub fn member(c: &mut Ctx, a: &[Term]) -> R {
    for item in a[1].list_iter() {
        if item.map_err(|_| c.badarg())?.eq_exact(&a[0]) {
            return Ok(c.bool(true));
        }
    }
    Ok(c.bool(false))
}

/// The first tuple in `a[2]` whose element `a[1]` (1-based) equals `a[0]`.
fn keyfind_tuple(c: &Ctx, a: &[Term]) -> Result<Option<Term>, Exception> {
    let n = match a[1].as_usize() {
        Some(n) if n >= 1 => n - 1,
        _ => return Err(c.badarg()),
    };
    for item in a[2].list_iter() {
        let item = item.map_err(|_| c.badarg())?;
        if let Some(t) = item.as_tuple() {
            // keyfind compares with ==, so 1 matches 1.0.
            if t.get(n).is_some_and(|e| e.eq_arith(&a[0])) {
                return Ok(Some(item));
            }
        }
    }
    Ok(None)
}

pub fn keyfind(c: &mut Ctx, a: &[Term]) -> R {
    Ok(keyfind_tuple(c, a)?.unwrap_or_else(|| c.bool(false)))
}

pub fn keymember(c: &mut Ctx, a: &[Term]) -> R {
    let found = keyfind_tuple(c, a)?.is_some();
    Ok(c.bool(found))
}

pub fn keysearch(c: &mut Ctx, a: &[Term]) -> R {
    Ok(match keyfind_tuple(c, a)? {
        Some(t) => Term::tuple(alloc::vec![c.atom("value"), t]),
        None => c.bool(false),
    })
}
