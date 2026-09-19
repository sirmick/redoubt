//! The `binary` module's native functions. The rest of `binary` is the real OTP module.

use alloc::vec::Vec;

use num_bigint::{BigInt, Sign};

use super::Ctx;
use crate::process::Exception;
use crate::term::{Bits, Term};

type R = Result<Term, Exception>;

fn bin<'t>(c: &Ctx, t: &'t Term) -> Result<&'t Bits, Exception> {
    match t {
        Term::Bits(b) if b.is_binary() => Ok(b),
        _ => Err(c.badarg()),
    }
}

fn bytes_of(c: &Ctx, t: &Term) -> Result<Vec<u8>, Exception> {
    Ok(bin(c, t)?.to_bytes().into_owned())
}

/// A `{Start, Length}` part of `size` bytes; a negative length counts backwards.
fn part_range(c: &Ctx, start: &Term, len: &Term, size: usize) -> Result<(usize, usize), Exception> {
    let (Some(s), Some(l)) = (start.as_i64(), len.as_i64()) else { return Err(c.badarg()) };
    let (lo, hi) = if l >= 0 { (s, s.checked_add(l)) } else { (s + l, Some(s)) };
    match hi {
        Some(hi) if lo >= 0 && hi as u64 <= size as u64 => Ok((lo as usize, hi as usize)),
        _ => Err(c.badarg()),
    }
}

pub fn at(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    match a[1].as_usize() {
        Some(i) if i < b.len / 8 => Ok(Term::Int(b.byte(i) as i64)),
        _ => Err(c.badarg()),
    }
}

pub fn first(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    if b.len == 0 {
        return Err(c.badarg());
    }
    Ok(Term::Int(b.byte(0) as i64))
}

pub fn last(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    if b.len == 0 {
        return Err(c.badarg());
    }
    Ok(Term::Int(b.byte(b.len / 8 - 1) as i64))
}

/// `part(Bin, {Start, Len})` and `part(Bin, Start, Len)`.
pub fn part(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let (start, len) = match a.len() {
        2 => match a[1].as_tuple() {
            Some([s, l]) => (s.clone(), l.clone()),
            _ => return Err(c.badarg()),
        },
        _ => (a[1].clone(), a[2].clone()),
    };
    let (lo, hi) = part_range(c, &start, &len, b.len / 8)?;
    Ok(Term::Bits(b.slice(lo * 8, (hi - lo) * 8)))
}

pub fn copy(c: &mut Ctx, a: &[Term]) -> R {
    let bytes = bytes_of(c, &a[0])?;
    let n = match a.get(1) {
        None => 1,
        Some(t) => t.as_usize().ok_or_else(|| c.badarg())?,
    };
    if bytes.len().saturating_mul(n).saturating_mul(8) > c.sys.limits.max_binary_bits {
        return Err(c.system_limit());
    }
    Ok(Term::binary(&bytes.repeat(n)))
}

pub fn bin_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let size = b.len / 8;
    let (lo, hi) = match a.len() {
        1 => (0, size),
        2 => match a[1].as_tuple() {
            Some([s, l]) => part_range(c, s, l, size)?,
            _ => return Err(c.badarg()),
        },
        _ => part_range(c, &a[1], &a[2], size)?,
    };
    Ok(Term::list((lo..hi).map(|i| Term::Int(b.byte(i) as i64)).collect::<Vec<_>>()))
}

pub fn list_to_bin(c: &mut Ctx, a: &[Term]) -> R {
    super::erlang::list_to_binary(c, a)
}

pub fn encode_unsigned(c: &mut Ctx, a: &[Term]) -> R {
    let v = a[0].as_bigint().filter(|v| v.sign() != Sign::Minus).ok_or_else(|| c.badarg())?;
    let little = a.get(1).is_some_and(|e| e.is_atom(&c.sys.atoms.little));
    if a.len() == 2 && !little && !a[1].is_atom(&c.sys.atoms.big) {
        return Err(c.badarg());
    }
    let mut bytes = if little { v.to_bytes_le().1 } else { v.to_bytes_be().1 };
    if bytes.is_empty() {
        bytes.push(0);
    }
    Ok(Term::binary(&bytes))
}

pub fn decode_unsigned(c: &mut Ctx, a: &[Term]) -> R {
    let bytes = bytes_of(c, &a[0])?;
    let little = a.get(1).is_some_and(|e| e.is_atom(&c.sys.atoms.little));
    if a.len() == 2 && !little && !a[1].is_atom(&c.sys.atoms.big) {
        return Err(c.badarg());
    }
    Ok(Term::big(if little {
        BigInt::from_bytes_le(Sign::Plus, &bytes)
    } else {
        BigInt::from_bytes_be(Sign::Plus, &bytes)
    }))
}

// ---- searching ----

/// A pattern: one binary or a list of them, none empty.
fn patterns(c: &Ctx, t: &Term) -> Result<Vec<Vec<u8>>, Exception> {
    let list = match t {
        Term::Bits(_) => alloc::vec![t.clone()],
        // A "compiled" pattern is just the pattern (see `compile_pattern/1`).
        Term::Tuple(_) => match t.as_tuple() {
            Some([_, p]) => return patterns(c, p),
            _ => return Err(c.badarg()),
        },
        _ => t.to_vec().ok_or_else(|| c.badarg())?,
    };
    let mut out = Vec::new();
    for p in &list {
        let b = bytes_of(c, p)?;
        if b.is_empty() {
            return Err(c.badarg());
        }
        out.push(b);
    }
    if out.is_empty() {
        return Err(c.badarg());
    }
    Ok(out)
}

/// The first match at or after `from`: at the leftmost position, the longest pattern.
fn find(hay: &[u8], pats: &[Vec<u8>], from: usize, until: usize) -> Option<(usize, usize)> {
    (from..until).find_map(|i| {
        pats.iter()
            .filter(|p| i + p.len() <= until && hay[i..i + p.len()] == p[..])
            .map(|p| p.len())
            .max()
            .map(|len| (i, len))
    })
}

/// `{scope, {Start, Length}}` from an options list, or the whole binary.
fn scope(c: &Ctx, opts: Option<&Term>, size: usize) -> Result<(usize, usize), Exception> {
    let Some(opts) = opts else { return Ok((0, size)) };
    let mut range = (0, size);
    for o in opts.to_vec().ok_or_else(|| c.badarg())? {
        match o.as_tuple() {
            Some([Term::Atom(tag), part]) if tag.as_str() == "scope" => match part.as_tuple() {
                Some([s, l]) => range = part_range(c, s, l, size)?,
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    Ok(range)
}

fn found(i: usize, len: usize) -> Term {
    Term::tuple(alloc::vec![Term::Int(i as i64), Term::Int(len as i64)])
}

pub fn compile_pattern(c: &mut Ctx, a: &[Term]) -> R {
    patterns(c, &a[0])?;
    Ok(Term::tuple(alloc::vec![c.atom("bm"), a[0].clone()]))
}

pub fn match_(c: &mut Ctx, a: &[Term]) -> R {
    let hay = bytes_of(c, &a[0])?;
    let pats = patterns(c, &a[1])?;
    let (lo, hi) = scope(c, a.get(2), hay.len())?;
    Ok(match find(&hay, &pats, lo, hi) {
        Some((i, len)) => found(i, len),
        None => c.atom("nomatch"),
    })
}

pub fn matches(c: &mut Ctx, a: &[Term]) -> R {
    let hay = bytes_of(c, &a[0])?;
    let pats = patterns(c, &a[1])?;
    let (mut at, hi) = scope(c, a.get(2), hay.len())?;
    let mut out = Vec::new();
    while let Some((i, len)) = find(&hay, &pats, at, hi) {
        out.push(found(i, len));
        at = i + len;
    }
    Ok(Term::list(out))
}

/// `split(Bin, Pattern, Options)` with `global`, `trim`, `trim_all` and `{scope, _}`.
pub fn split(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?.clone();
    let hay = b.to_bytes().into_owned();
    let pats = patterns(c, &a[1])?;
    let (mut global, mut trim, mut trim_all) = (false, false, false);
    let mut scope_opts = Vec::new();
    if let Some(opts) = a.get(2) {
        for o in opts.to_vec().ok_or_else(|| c.badarg())? {
            match &o {
                Term::Atom(x) if x.as_str() == "global" => global = true,
                Term::Atom(x) if x.as_str() == "trim" => trim = true,
                Term::Atom(x) if x.as_str() == "trim_all" => trim_all = true,
                Term::Tuple(_) => scope_opts.push(o.clone()),
                _ => return Err(c.badarg()),
            }
        }
    }
    let scope_list = Term::list(scope_opts);
    let (mut at, hi) = scope(c, Some(&scope_list), hay.len())?;
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while let Some((i, len)) = find(&hay, &pats, at, hi) {
        pieces.push((start, i));
        start = i + len;
        at = start;
        if !global {
            break;
        }
    }
    pieces.push((start, hay.len()));
    if trim_all {
        pieces.retain(|(s, e)| s != e);
    } else if trim {
        while pieces.last().is_some_and(|(s, e)| s == e) {
            pieces.pop();
        }
    }
    Ok(Term::list(pieces.into_iter().map(|(s, e)| Term::Bits(b.slice(s * 8, (e - s) * 8))).collect::<Vec<_>>()))
}

fn common(c: &Ctx, a: &Term, suffix: bool) -> R {
    let bins: Vec<Vec<u8>> = a.to_vec().ok_or_else(|| c.badarg())?.iter().map(|t| bytes_of(c, t)).collect::<Result<_, _>>()?;
    if bins.is_empty() {
        return Err(c.badarg());
    }
    let shortest = bins.iter().map(|b| b.len()).min().unwrap_or(0);
    let n = (0..shortest)
        .take_while(|&i| {
            let at = |b: &Vec<u8>| if suffix { b[b.len() - 1 - i] } else { b[i] };
            bins.iter().all(|b| at(b) == at(&bins[0]))
        })
        .count();
    Ok(Term::Int(n as i64))
}

pub fn longest_common_prefix(c: &mut Ctx, a: &[Term]) -> R {
    common(c, &a[0], false)
}

pub fn longest_common_suffix(c: &mut Ctx, a: &[Term]) -> R {
    common(c, &a[0], true)
}
