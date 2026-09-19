//! The `lists` functions BEAM implements natively. The rest of `lists` is the real OTP module.

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// `lists:reverse(List, Tail)`.
pub fn reverse(c: &mut Ctx, a: &[Term]) -> R {
    let items = c.list_arg(a[0])?;
    let mut acc = a[1];
    for item in items {
        acc = c.cons(item, acc);
    }
    Ok(acc)
}

pub fn member(c: &mut Ctx, a: &[Term]) -> R {
    for item in c.heap().list_iter(a[1]) {
        if c.heap().eq_exact(item.map_err(|_| c.badarg())?, a[0]) {
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
    let h = c.heap();
    for item in h.list_iter(a[2]) {
        let item = item.map_err(|_| c.badarg())?;
        if let Some(t) = h.as_tuple(item) {
            // keyfind compares with ==, so 1 matches 1.0.
            if t.get(n).is_some_and(|e| h.eq_arith(*e, a[0])) {
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
        Some(t) => {
            let value = c.atom("value");
            c.tuple(&[value, t])
        }
        None => c.bool(false),
    })
}
