//! `erlang` BIFs for types, tuples, lists, binaries and conversions.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use num_bigint::BigInt;
use num_traits::Num;

use super::Ctx;
use crate::process::Exception;
use crate::term::{Bits, Term};

type R = Result<Term, Exception>;

/// Longest list or tuple a BIF will build. Bounds memory use for things like `make_tuple/2`.
const MAX_TUPLE: usize = 1 << 24;

pub fn is_atom(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Atom(_))))
}
pub fn is_binary(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(&a[0], Term::Bits(b) if b.is_binary())))
}
pub fn is_bitstring(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Bits(_))))
}
pub fn is_boolean(c: &mut Ctx, a: &[Term]) -> R {
    let b = a[0].is_atom(&c.sys.atoms.true_) || a[0].is_atom(&c.sys.atoms.false_);
    Ok(c.bool(b))
}
pub fn is_float(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Float(_))))
}
pub fn is_function(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Fun(_))))
}
pub fn is_function2(c: &mut Ctx, a: &[Term]) -> R {
    let arity = a[1].as_usize().ok_or_else(|| c.badarg())?;
    Ok(c.bool(matches!(&a[0], Term::Fun(f) if f.arity() as usize == arity)))
}
pub fn is_integer(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(a[0].is_integer()))
}
pub fn is_list(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Nil | Term::Cons(_))))
}
pub fn is_map(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Map(_))))
}
pub fn is_number(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(a[0].is_number()))
}
pub fn is_pid(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Pid(_))))
}
pub fn is_port(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(false))
}
pub fn is_reference(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Ref(_))))
}
pub fn is_tuple(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Tuple(_))))
}

// ---- tuples ----

fn tuple<'t>(c: &Ctx, t: &'t Term) -> Result<&'t [Term], Exception> {
    t.as_tuple().ok_or_else(|| c.badarg())
}

/// A 1-based position within `len`.
fn position(c: &Ctx, t: &Term, len: usize) -> Result<usize, Exception> {
    match t.as_usize() {
        Some(i) if i >= 1 && i <= len => Ok(i - 1),
        _ => Err(c.badarg()),
    }
}

pub fn element(c: &mut Ctx, a: &[Term]) -> R {
    let t = tuple(c, &a[1])?;
    Ok(t[position(c, &a[0], t.len())?].clone())
}

pub fn setelement(c: &mut Ctx, a: &[Term]) -> R {
    let t = tuple(c, &a[1])?;
    let i = position(c, &a[0], t.len())?;
    let mut v = t.to_vec();
    v[i] = a[2].clone();
    Ok(Term::tuple(v))
}

pub fn tuple_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(tuple(c, &a[0])?.len() as i64))
}

pub fn size(c: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Tuple(t) => Ok(Term::Int(t.len() as i64)),
        Term::Bits(b) => Ok(Term::Int((b.len / 8) as i64)),
        _ => Err(c.badarg()),
    }
}

pub fn make_tuple(c: &mut Ctx, a: &[Term]) -> R {
    match a[0].as_usize() {
        Some(n) if n <= MAX_TUPLE => Ok(Term::tuple(alloc::vec![a[1].clone(); n])),
        Some(_) => Err(c.system_limit()),
        None => Err(c.badarg()),
    }
}

pub fn append_element(c: &mut Ctx, a: &[Term]) -> R {
    let mut v = tuple(c, &a[0])?.to_vec();
    v.push(a[1].clone());
    Ok(Term::tuple(v))
}

pub fn tuple_to_list(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::list(tuple(c, &a[0])?.to_vec()))
}

pub fn list_to_tuple(c: &mut Ctx, a: &[Term]) -> R {
    let v = a[0].to_vec().ok_or_else(|| c.badarg())?;
    Ok(Term::tuple(v))
}

// ---- lists ----

pub fn hd(c: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Cons(cell) => Ok(cell.head.clone()),
        _ => Err(c.badarg()),
    }
}

pub fn tl(c: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Cons(cell) => Ok(cell.tail.clone()),
        _ => Err(c.badarg()),
    }
}

pub fn length(c: &mut Ctx, a: &[Term]) -> R {
    let mut n = 0i64;
    for item in a[0].list_iter() {
        item.map_err(|_| c.badarg())?;
        n += 1;
    }
    Ok(Term::Int(n))
}

pub fn append(c: &mut Ctx, a: &[Term]) -> R {
    // The left side must be a proper list; the right side can be anything.
    let left = a[0].to_vec().ok_or_else(|| c.badarg())?;
    Ok(Term::list_with_tail(left, a[1].clone()))
}

/// `A -- B`: remove the first occurrence in `A` of each element of `B`.
pub fn subtract(c: &mut Ctx, a: &[Term]) -> R {
    let mut left = a[0].to_vec().ok_or_else(|| c.badarg())?;
    let right = a[1].to_vec().ok_or_else(|| c.badarg())?;
    for r in &right {
        if let Some(i) = left.iter().position(|l| l.eq_exact(r)) {
            left.remove(i);
        }
    }
    Ok(Term::list(left))
}

// ---- binaries ----

fn bits<'t>(c: &Ctx, t: &'t Term) -> Result<&'t Bits, Exception> {
    match t {
        Term::Bits(b) => Ok(b),
        _ => Err(c.badarg()),
    }
}

fn binary<'t>(c: &Ctx, t: &'t Term) -> Result<&'t Bits, Exception> {
    match t {
        Term::Bits(b) if b.is_binary() => Ok(b),
        _ => Err(c.badarg()),
    }
}

pub fn byte_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(bits(c, &a[0])?.len.div_ceil(8) as i64))
}

pub fn bit_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(bits(c, &a[0])?.len as i64))
}

pub fn binary_part(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let (start, len) = match (a[1].as_i64(), a[2].as_i64()) {
        (Some(s), Some(l)) => (s, l),
        _ => return Err(c.badarg()),
    };
    // A negative length counts back from `start`.
    let (lo, hi) = if len >= 0 { (start, start.checked_add(len)) } else { (start + len, Some(start)) };
    let size = (b.len / 8) as i64;
    match hi {
        Some(hi) if lo >= 0 && hi <= size => Ok(Term::Bits(b.slice(lo as usize * 8, (hi - lo) as usize * 8))),
        _ => Err(c.badarg()),
    }
}

pub fn split_binary(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    match a[1].as_usize() {
        Some(n) if n * 8 <= b.len => {
            let (x, y) = (b.slice(0, n * 8), b.slice(n * 8, b.len - n * 8));
            Ok(Term::tuple(alloc::vec![Term::Bits(x), Term::Bits(y)]))
        }
        _ => Err(c.badarg()),
    }
}

// ---- atoms ----

fn chars_to_list(s: &str) -> Term {
    Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>())
}

/// A string from a list of code points (not an iolist).
fn list_to_string(c: &Ctx, t: &Term) -> Result<String, Exception> {
    let mut s = String::new();
    for item in t.list_iter() {
        let ch = item.ok().and_then(|x| x.as_i64()).and_then(|i| u32::try_from(i).ok()).and_then(char::from_u32);
        s.push(ch.ok_or_else(|| c.badarg())?);
    }
    Ok(s)
}

fn atom_arg<'t>(c: &Ctx, t: &'t Term) -> Result<&'t crate::atom::Atom, Exception> {
    match t {
        Term::Atom(a) => Ok(a),
        _ => Err(c.badarg()),
    }
}

pub fn atom_to_list(c: &mut Ctx, a: &[Term]) -> R {
    Ok(chars_to_list(atom_arg(c, &a[0])?.as_str()))
}

pub fn atom_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let atom = atom_arg(c, &a[0])?;
    if a.len() == 2 && a[1].is_atom(&c.sys.atoms.latin1) {
        let bytes: Option<Vec<u8>> = atom.as_str().chars().map(|ch| u8::try_from(ch as u32).ok()).collect();
        return Ok(Term::binary(&bytes.ok_or_else(|| c.badarg())?));
    }
    Ok(Term::binary(atom.as_str().as_bytes()))
}

fn make_atom(c: &mut Ctx, s: &str, existing: bool) -> R {
    let atom = if existing {
        c.sys.atom_table.existing(s).ok_or_else(|| c.badarg())?
    } else {
        match c.sys.atom_table.intern(s) {
            Ok(a) => a,
            Err(crate::atom::AtomError::TooLong) => return Err(c.system_limit()),
            Err(crate::atom::AtomError::TableFull) => return Err(c.system_limit()),
        }
    };
    Ok(Term::Atom(atom))
}

pub fn list_to_atom(c: &mut Ctx, a: &[Term]) -> R {
    let s = list_to_string(c, &a[0])?;
    make_atom(c, &s, false)
}

pub fn list_to_existing_atom(c: &mut Ctx, a: &[Term]) -> R {
    let s = list_to_string(c, &a[0])?;
    make_atom(c, &s, true)
}

fn binary_text(c: &Ctx, a: &[Term]) -> Result<String, Exception> {
    let b = binary(c, &a[0])?;
    let bytes = b.to_bytes();
    if a.len() == 2 && a[1].is_atom(&c.sys.atoms.latin1) {
        Ok(bytes.iter().map(|&b| b as char).collect())
    } else {
        core::str::from_utf8(&bytes).map(|s| s.to_string()).map_err(|_| c.badarg())
    }
}

pub fn binary_to_atom(c: &mut Ctx, a: &[Term]) -> R {
    let s = binary_text(c, a)?;
    make_atom(c, &s, false)
}

pub fn binary_to_existing_atom(c: &mut Ctx, a: &[Term]) -> R {
    let s = binary_text(c, a)?;
    make_atom(c, &s, true)
}

// ---- integers and floats as text ----

fn radix(c: &Ctx, a: &[Term]) -> Result<u32, Exception> {
    match a.get(1) {
        None => Ok(10),
        Some(t) => match t.as_i64() {
            Some(r @ 2..=36) => Ok(r as u32),
            _ => Err(c.badarg()),
        },
    }
}

fn integer_text(c: &Ctx, a: &[Term]) -> Result<String, Exception> {
    let r = radix(c, a)?;
    let i = a[0].as_bigint().ok_or_else(|| c.badarg())?;
    // Erlang prints digits above 9 in upper case.
    Ok(i.to_str_radix(r).to_uppercase())
}

pub fn integer_to_list(c: &mut Ctx, a: &[Term]) -> R {
    Ok(chars_to_list(&integer_text(c, a)?))
}

pub fn integer_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::binary(integer_text(c, a)?.as_bytes()))
}

fn parse_integer(c: &Ctx, s: &str, r: u32) -> R {
    // An optional sign, then at least one digit; no whitespace, no `_`.
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(r)) {
        return Err(c.badarg());
    }
    if s.len() as u64 > super::arith::MAX_BIG_BITS / 3 {
        return Err(c.system_limit());
    }
    BigInt::from_str_radix(s, r).map(Term::big).map_err(|_| c.badarg())
}

pub fn list_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    let s = list_to_string(c, &a[0])?;
    let r = radix(c, a)?;
    parse_integer(c, &s, r)
}

pub fn binary_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let s = core::str::from_utf8(&b.to_bytes()).map_err(|_| c.badarg())?.to_string();
    let r = radix(c, a)?;
    parse_integer(c, &s, r)
}

/// `float_to_list/1`: the classic 20-significant-digit scientific format, e.g.
/// `"1.50000000000000000000e+00"`.
fn float_classic(f: f64) -> String {
    let s = alloc::format!("{f:.20e}");
    let (m, e) = s.split_once('e').expect("{:e} has an exponent");
    let e: i32 = e.parse().expect("exponent");
    alloc::format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

fn float_arg(c: &Ctx, t: &Term) -> Result<f64, Exception> {
    match t {
        Term::Float(f) => Ok(*f),
        _ => Err(c.badarg()),
    }
}

pub fn float_to_list(c: &mut Ctx, a: &[Term]) -> R {
    Ok(chars_to_list(&float_classic(float_arg(c, &a[0])?)))
}

pub fn float_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::binary(float_classic(float_arg(c, &a[0])?).as_bytes()))
}

// ---- lists and binaries ----

pub fn binary_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    Ok(Term::list(b.to_bytes().iter().map(|&x| Term::Int(x as i64)).collect::<Vec<_>>()))
}

pub fn binary_to_list3(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let size = b.len / 8;
    match (a[1].as_usize(), a[2].as_usize()) {
        (Some(s), Some(e)) if s >= 1 && s <= e && e <= size => {
            let bytes = b.to_bytes();
            Ok(Term::list(bytes[s - 1..e].iter().map(|&x| Term::Int(x as i64)).collect::<Vec<_>>()))
        }
        _ => Err(c.badarg()),
    }
}

pub fn bitstring_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = bits(c, &a[0])?;
    let whole = b.len / 8;
    let items: Vec<Term> = (0..whole).map(|i| Term::Int(b.byte(i) as i64)).collect();
    let tail = if b.len % 8 == 0 { Term::Nil } else { Term::Bits(b.slice(whole * 8, b.len % 8)) };
    Ok(Term::list_with_tail(items, tail))
}

/// Flatten an iolist (bytes 0..255, binaries and nested iolists; a binary may end a list) into
/// bytes, or a bitstring if `bits_ok` and the last element is one.
fn flatten_iolist(c: &Ctx, t: &Term, bits_ok: bool) -> Result<crate::bits::Builder, Exception> {
    let mut out = crate::bits::Builder::new();
    // An explicit work stack: iolists can nest arbitrarily deep.
    let mut stack: Vec<Term> = alloc::vec![t.clone()];
    while let Some(t) = stack.pop() {
        match t {
            Term::Nil => {}
            Term::Bits(b) => {
                if !b.is_binary() && !bits_ok {
                    return Err(c.badarg());
                }
                out.push_bits(&b);
            }
            Term::Cons(cell) => {
                stack.push(cell.tail.clone());
                match &cell.head {
                    Term::Int(i) if (0..=255).contains(i) => out.push_byte(*i as u8),
                    h @ (Term::Cons(_) | Term::Nil | Term::Bits(_)) => stack.push(h.clone()),
                    _ => return Err(c.badarg()),
                }
            }
            _ => return Err(c.badarg()),
        }
        if out.bit_len() > c.sys.limits.max_binary_bits {
            return Err(c.system_limit());
        }
    }
    Ok(out)
}

pub fn list_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    if let Term::Bits(b) = &a[0] {
        return if b.is_binary() { Ok(a[0].clone()) } else { Err(c.badarg()) };
    }
    if !matches!(a[0], Term::Cons(_) | Term::Nil) {
        return Err(c.badarg());
    }
    Ok(flatten_iolist(c, &a[0], false)?.finish())
}

pub fn list_to_bitstring(c: &mut Ctx, a: &[Term]) -> R {
    if let Term::Bits(_) = &a[0] {
        return Ok(a[0].clone());
    }
    Ok(flatten_iolist(c, &a[0], true)?.finish())
}

pub fn iolist_size(c: &mut Ctx, a: &[Term]) -> R {
    let b = flatten_iolist(c, &a[0], false)?;
    Ok(Term::Int((b.bit_len() / 8) as i64))
}

pub fn display(c: &mut Ctx, a: &[Term]) -> R {
    let text = alloc::format!("{}\n", a[0]);
    c.sys.platform.console_write(text.as_bytes());
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}
