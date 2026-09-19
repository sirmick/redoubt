//! Arithmetic, comparison and boolean operators.
//!
//! Integers are `i64` until an operation overflows, then [`BigInt`]; results that fit go back to
//! `i64` (see [`Term::big`]). Bignums are capped at [`MAX_BIG_BITS`] so that `1 bsl (1 bsl 40)`
//! is a `system_limit` error rather than an attempt to allocate 128 GiB.

use core::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::float::FloatCore;
use num_traits::{FromPrimitive, Signed, ToPrimitive, Zero};

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

/// Largest integer, in bits, any operation may produce.
pub const MAX_BIG_BITS: u64 = 1 << 24;

type R = Result<Term, Exception>;

enum Num {
    I(BigInt),
    F(f64),
}

fn num(ctx: &Ctx, t: &Term) -> Result<Num, Exception> {
    match t {
        Term::Int(i) => Ok(Num::I(BigInt::from(*i))),
        Term::Big(b) => Ok(Num::I((**b).clone())),
        Term::Float(f) => Ok(Num::F(*f)),
        _ => Err(ctx.badarith()),
    }
}

fn to_f64(ctx: &Ctx, n: &Num) -> Result<f64, Exception> {
    match n {
        Num::F(f) => Ok(*f),
        Num::I(i) => i.to_f64().filter(|f| f.is_finite()).ok_or_else(|| ctx.badarith()),
    }
}

fn mk_float(ctx: &Ctx, f: f64) -> R {
    if f.is_finite() {
        Ok(Term::Float(f))
    } else {
        Err(ctx.badarith())
    }
}

fn big(ctx: &Ctx, b: BigInt) -> R {
    if b.bits() > MAX_BIG_BITS {
        return Err(ctx.system_limit());
    }
    Ok(Term::big(b))
}

/// Shared shape of `+`, `-`, `*`: an `i64` fast path, else bignum or float.
fn arith(
    ctx: &mut Ctx,
    a: &[Term],
    small: fn(i64, i64) -> Option<i64>,
    bigop: fn(BigInt, BigInt) -> BigInt,
    fop: fn(f64, f64) -> f64,
) -> R {
    if let (Term::Int(x), Term::Int(y)) = (&a[0], &a[1]) {
        if let Some(r) = small(*x, *y) {
            return Ok(Term::Int(r));
        }
    }
    match (num(ctx, &a[0])?, num(ctx, &a[1])?) {
        (Num::I(x), Num::I(y)) => {
            if x.bits() + y.bits() > MAX_BIG_BITS {
                return Err(ctx.system_limit());
            }
            big(ctx, bigop(x, y))
        }
        (x, y) => mk_float(ctx, fop(to_f64(ctx, &x)?, to_f64(ctx, &y)?)),
    }
}

pub fn add(ctx: &mut Ctx, a: &[Term]) -> R {
    arith(ctx, a, i64::checked_add, |x, y| x + y, |x, y| x + y)
}

pub fn sub(ctx: &mut Ctx, a: &[Term]) -> R {
    arith(ctx, a, i64::checked_sub, |x, y| x - y, |x, y| x - y)
}

pub fn mul(ctx: &mut Ctx, a: &[Term]) -> R {
    arith(ctx, a, i64::checked_mul, |x, y| x * y, |x, y| x * y)
}

pub fn fdiv(ctx: &mut Ctx, a: &[Term]) -> R {
    let x = to_f64(ctx, &num(ctx, &a[0])?)?;
    let y = to_f64(ctx, &num(ctx, &a[1])?)?;
    if y == 0.0 {
        return Err(ctx.badarith());
    }
    mk_float(ctx, x / y)
}

fn ints(ctx: &Ctx, a: &[Term]) -> Result<(BigInt, BigInt), Exception> {
    match (a[0].as_bigint(), a[1].as_bigint()) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(ctx.badarith()),
    }
}

pub fn idiv(ctx: &mut Ctx, a: &[Term]) -> R {
    if let (Term::Int(x), Term::Int(y)) = (&a[0], &a[1]) {
        if *y == 0 {
            return Err(ctx.badarith());
        }
        if let Some(r) = x.checked_div(*y) {
            return Ok(Term::Int(r));
        }
    }
    let (x, y) = ints(ctx, a)?;
    if y.is_zero() {
        return Err(ctx.badarith());
    }
    big(ctx, x / y) // BigInt division truncates toward zero, like Erlang's div
}

pub fn rem(ctx: &mut Ctx, a: &[Term]) -> R {
    if let (Term::Int(x), Term::Int(y)) = (&a[0], &a[1]) {
        if *y == 0 {
            return Err(ctx.badarith());
        }
        return Ok(Term::Int(x.checked_rem(*y).unwrap_or(0)));
    }
    let (x, y) = ints(ctx, a)?;
    if y.is_zero() {
        return Err(ctx.badarith());
    }
    big(ctx, x % y) // sign follows the dividend, like Erlang's rem
}

pub fn neg(ctx: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Int(i) => Ok(i.checked_neg().map(Term::Int).unwrap_or_else(|| Term::big(-BigInt::from(*i)))),
        Term::Big(b) => Ok(Term::big(-(**b).clone())),
        Term::Float(f) => Ok(Term::Float(-f)),
        _ => Err(ctx.badarith()),
    }
}

pub fn plus(ctx: &mut Ctx, a: &[Term]) -> R {
    if a[0].is_number() {
        Ok(a[0].clone())
    } else {
        Err(ctx.badarith())
    }
}

fn bitwise(ctx: &mut Ctx, a: &[Term], small: fn(i64, i64) -> i64, bigop: fn(&BigInt, &BigInt) -> BigInt) -> R {
    if let (Term::Int(x), Term::Int(y)) = (&a[0], &a[1]) {
        return Ok(Term::Int(small(*x, *y)));
    }
    let (x, y) = ints(ctx, a)?;
    big(ctx, bigop(&x, &y))
}

pub fn band(ctx: &mut Ctx, a: &[Term]) -> R {
    bitwise(ctx, a, |x, y| x & y, |x, y| x & y)
}

pub fn bor(ctx: &mut Ctx, a: &[Term]) -> R {
    bitwise(ctx, a, |x, y| x | y, |x, y| x | y)
}

pub fn bxor(ctx: &mut Ctx, a: &[Term]) -> R {
    bitwise(ctx, a, |x, y| x ^ y, |x, y| x ^ y)
}

pub fn bnot(ctx: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Int(i) => Ok(Term::Int(!i)),
        Term::Big(b) => Ok(Term::big(-(**b).clone() - 1)),
        _ => Err(ctx.badarith()),
    }
}

/// `X bsl N`; a negative `N` shifts right.
fn shift(ctx: &mut Ctx, x: &Term, n: &Term, left: bool) -> R {
    let (Some(x), Some(n)) = (x.as_bigint(), n.as_bigint()) else {
        return Err(ctx.badarith());
    };
    let n = if left { n } else { -n };
    if x.is_zero() {
        return Ok(Term::Int(0));
    }
    match n.to_i64() {
        Some(n) if n >= 0 => {
            if x.bits().saturating_add(n as u64) > MAX_BIG_BITS {
                return Err(ctx.system_limit());
            }
            big(ctx, x << (n as u64))
        }
        Some(n) => {
            let n = n.unsigned_abs();
            if n >= x.bits() {
                // Arithmetic shift: everything shifted out leaves 0 or -1.
                Ok(Term::Int(if x.is_negative() { -1 } else { 0 }))
            } else {
                big(ctx, x >> n) // BigInt >> rounds toward negative infinity, like Erlang
            }
        }
        None if n.is_negative() => Ok(Term::Int(if x.is_negative() { -1 } else { 0 })),
        None => Err(ctx.system_limit()),
    }
}

pub fn bsl(ctx: &mut Ctx, a: &[Term]) -> R {
    shift(ctx, &a[0], &a[1], true)
}

pub fn bsr(ctx: &mut Ctx, a: &[Term]) -> R {
    shift(ctx, &a[0], &a[1], false)
}

pub fn abs(ctx: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Int(i) => Ok(i.checked_abs().map(Term::Int).unwrap_or_else(|| Term::big(BigInt::from(*i).abs()))),
        Term::Big(b) => Ok(Term::big(b.abs())),
        Term::Float(f) => Ok(Term::Float(f.abs())),
        _ => Err(ctx.badarg()),
    }
}

pub fn float(ctx: &mut Ctx, a: &[Term]) -> R {
    match num(ctx, &a[0]) {
        Ok(n) => to_f64(ctx, &n).map(Term::Float).map_err(|_| ctx.badarg()),
        Err(_) => Err(ctx.badarg()),
    }
}

/// Integer from a float that has already been rounded to a whole number.
fn whole(ctx: &Ctx, f: f64) -> R {
    BigInt::from_f64(f).map(Term::big).ok_or_else(|| ctx.badarg())
}

fn rounding(ctx: &mut Ctx, a: &[Term], op: fn(f64) -> f64) -> R {
    match &a[0] {
        Term::Int(_) | Term::Big(_) => Ok(a[0].clone()),
        Term::Float(f) => whole(ctx, op(*f)),
        _ => Err(ctx.badarg()),
    }
}

pub fn trunc(ctx: &mut Ctx, a: &[Term]) -> R {
    rounding(ctx, a, FloatCore::trunc)
}

/// Rounds half away from zero, as Erlang's `round/1` does.
pub fn round(ctx: &mut Ctx, a: &[Term]) -> R {
    rounding(ctx, a, FloatCore::round)
}

pub fn floor(ctx: &mut Ctx, a: &[Term]) -> R {
    rounding(ctx, a, FloatCore::floor)
}

pub fn ceil(ctx: &mut Ctx, a: &[Term]) -> R {
    rounding(ctx, a, FloatCore::ceil)
}

pub fn max(_ctx: &mut Ctx, a: &[Term]) -> R {
    // On equal values the first argument wins, as in Erlang.
    Ok(if a[1].cmp_term(&a[0]) == Ordering::Greater { a[1].clone() } else { a[0].clone() })
}

pub fn min(_ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(if a[1].cmp_term(&a[0]) == Ordering::Less { a[1].clone() } else { a[0].clone() })
}

pub fn eq(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].eq_arith(&a[1])))
}

pub fn ne(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(!a[0].eq_arith(&a[1])))
}

pub fn eq_exact(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].eq_exact(&a[1])))
}

pub fn ne_exact(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(!a[0].eq_exact(&a[1])))
}

pub fn lt(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].cmp_term(&a[1]) == Ordering::Less))
}

pub fn gt(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].cmp_term(&a[1]) == Ordering::Greater))
}

pub fn le(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].cmp_term(&a[1]) != Ordering::Greater))
}

pub fn ge(ctx: &mut Ctx, a: &[Term]) -> R {
    Ok(ctx.bool(a[0].cmp_term(&a[1]) != Ordering::Less))
}

fn boolean(ctx: &Ctx, t: &Term) -> Result<bool, Exception> {
    if t.is_atom(&ctx.sys.atoms.true_) {
        Ok(true)
    } else if t.is_atom(&ctx.sys.atoms.false_) {
        Ok(false)
    } else {
        Err(ctx.badarg())
    }
}

pub fn and(ctx: &mut Ctx, a: &[Term]) -> R {
    let (x, y) = (boolean(ctx, &a[0])?, boolean(ctx, &a[1])?);
    Ok(ctx.bool(x && y))
}

pub fn or(ctx: &mut Ctx, a: &[Term]) -> R {
    let (x, y) = (boolean(ctx, &a[0])?, boolean(ctx, &a[1])?);
    Ok(ctx.bool(x || y))
}

pub fn xor(ctx: &mut Ctx, a: &[Term]) -> R {
    let (x, y) = (boolean(ctx, &a[0])?, boolean(ctx, &a[1])?);
    Ok(ctx.bool(x ^ y))
}

pub fn not(ctx: &mut Ctx, a: &[Term]) -> R {
    let x = boolean(ctx, &a[0])?;
    Ok(ctx.bool(!x))
}
