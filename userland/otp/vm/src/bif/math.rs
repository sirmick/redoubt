//! The `math` module's native functions, over `libm` (a pure-Rust port of musl's libm).

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// A number argument as a float (`math` functions accept integers too).
fn arg(c: &Ctx, t: &Term) -> Result<f64, Exception> {
    match t {
        Term::Float(f) => Ok(*f),
        Term::Int(i) => Ok(*i as f64),
        Term::Big(_) => c
            .heap()
            .as_big(*t)
            .and_then(num_traits::ToPrimitive::to_f64)
            .filter(|f| f.is_finite())
            .ok_or_else(|| c.badarg()),
        _ => Err(c.badarg()),
    }
}

/// A result, or `badarith` if it is not a finite number (Erlang has no NaN or infinity).
fn result(c: &Ctx, f: f64) -> R {
    if f.is_finite() {
        Ok(Term::Float(f))
    } else {
        Err(c.badarith())
    }
}

macro_rules! unary {
    ($($name:ident => $f:path),* $(,)?) => {
        $(pub fn $name(c: &mut Ctx, a: &[Term]) -> R {
            let x = arg(c, &a[0])?;
            result(c, $f(x))
        })*
    };
}

unary! {
    sin => libm::sin, cos => libm::cos, tan => libm::tan,
    asin => libm::asin, acos => libm::acos, atan => libm::atan,
    sinh => libm::sinh, cosh => libm::cosh, tanh => libm::tanh,
    asinh => libm::asinh, acosh => libm::acosh, atanh => libm::atanh,
    exp => libm::exp, log => libm::log, log2 => libm::log2, log10 => libm::log10,
    sqrt => libm::sqrt, erf => libm::erf, erfc => libm::erfc,
    floor => libm::floor, ceil => libm::ceil,
}

pub fn atan2(c: &mut Ctx, a: &[Term]) -> R {
    let (y, x) = (arg(c, &a[0])?, arg(c, &a[1])?);
    result(c, libm::atan2(y, x))
}

pub fn pow(c: &mut Ctx, a: &[Term]) -> R {
    let (x, y) = (arg(c, &a[0])?, arg(c, &a[1])?);
    result(c, libm::pow(x, y))
}

pub fn fmod(c: &mut Ctx, a: &[Term]) -> R {
    let (x, y) = (arg(c, &a[0])?, arg(c, &a[1])?);
    result(c, libm::fmod(x, y))
}
