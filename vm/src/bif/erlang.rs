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
    Ok(c.bool(c.heap().bit_len(a[0]).is_some_and(|n| n.is_multiple_of(8))))
}
pub fn is_bitstring(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Bits(_))))
}
pub fn is_boolean(c: &mut Ctx, a: &[Term]) -> R {
    let b = a[0].is_atom(&c.atoms.true_) || a[0].is_atom(&c.atoms.false_);
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
    Ok(c.bool(
        c.heap()
            .as_fun(a[0])
            .is_some_and(|f| f.arity() as usize == arity),
    ))
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
    Ok(c.bool(matches!(a[0], Term::Pid(p) if !p.port)))
}
pub fn is_port(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Pid(p) if p.port)))
}
pub fn is_reference(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Ref(_) | Term::Resource(_))))
}
pub fn is_tuple(c: &mut Ctx, a: &[Term]) -> R {
    Ok(c.bool(matches!(a[0], Term::Tuple(_))))
}

// ---- tuples ----

fn tuple(c: &Ctx, t: &Term) -> Result<Vec<Term>, Exception> {
    c.tuple_elems(*t).ok_or_else(|| c.badarg())
}

/// A 1-based position within `len`.
fn position(c: &Ctx, t: &Term, len: usize) -> Result<usize, Exception> {
    match t.as_usize() {
        Some(i) if i >= 1 && i <= len => Ok(i - 1),
        _ => Err(c.badarg()),
    }
}

pub fn element(c: &mut Ctx, a: &[Term]) -> R {
    let t = c.heap().as_tuple(a[1]).ok_or_else(|| c.badarg())?;
    Ok(t[position(c, &a[0], t.len())?])
}

pub fn setelement(c: &mut Ctx, a: &[Term]) -> R {
    let mut v = tuple(c, &a[1])?;
    let i = position(c, &a[0], v.len())?;
    v[i] = a[2];
    Ok(c.tuple(&v))
}

pub fn tuple_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(
        c.heap().as_tuple(a[0]).ok_or_else(|| c.badarg())?.len() as i64,
    ))
}

pub fn size(c: &mut Ctx, a: &[Term]) -> R {
    match a[0] {
        Term::Tuple(_) => Ok(Term::Int(
            c.heap().as_tuple(a[0]).expect("a tuple").len() as i64
        )),
        Term::Bits(_) => Ok(Term::Int(
            (c.heap().bit_len(a[0]).expect("bits") / 8) as i64,
        )),
        _ => Err(c.badarg()),
    }
}

pub fn make_tuple(c: &mut Ctx, a: &[Term]) -> R {
    match a[0].as_usize() {
        Some(n) if n <= MAX_TUPLE => Ok(c.tuple(&alloc::vec![a[1]; n])),
        Some(_) => Err(c.system_limit()),
        None => Err(c.badarg()),
    }
}

/// `make_tuple(Arity, Default, [{Position, Value}])`: later entries win.
pub fn make_tuple3(c: &mut Ctx, a: &[Term]) -> R {
    let n = match a[0].as_usize() {
        Some(n) if n <= MAX_TUPLE => n,
        Some(_) => return Err(c.system_limit()),
        None => return Err(c.badarg()),
    };
    let mut elems = alloc::vec![a[1]; n];
    for init in c.list_arg(a[2])? {
        match c.heap().as_tuple(init) {
            Some(&[pos, v]) => match pos.as_usize() {
                Some(p) if p >= 1 && p <= elems.len() => elems[p - 1] = v,
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    Ok(c.tuple(&elems))
}

pub fn append_element(c: &mut Ctx, a: &[Term]) -> R {
    let mut v = tuple(c, &a[0])?;
    v.push(a[1]);
    Ok(c.tuple(&v))
}

pub fn tuple_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let v = tuple(c, &a[0])?;
    Ok(c.list(v))
}

pub fn list_to_tuple(c: &mut Ctx, a: &[Term]) -> R {
    let v = c.list_arg(a[0])?;
    Ok(c.tuple(&v))
}

// ---- lists ----

pub fn hd(c: &mut Ctx, a: &[Term]) -> R {
    c.heap()
        .as_cons(a[0])
        .map(|(h, _)| h)
        .ok_or_else(|| c.badarg())
}

pub fn tl(c: &mut Ctx, a: &[Term]) -> R {
    c.heap()
        .as_cons(a[0])
        .map(|(_, t)| t)
        .ok_or_else(|| c.badarg())
}

pub fn length(c: &mut Ctx, a: &[Term]) -> R {
    let mut n = 0i64;
    for item in c.heap().list_iter(a[0]) {
        item.map_err(|_| c.badarg())?;
        n += 1;
    }
    Ok(Term::Int(n))
}

pub fn append(c: &mut Ctx, a: &[Term]) -> R {
    // The left side must be a proper list; the right side can be anything.
    let left = c.list_arg(a[0])?;
    Ok(c.list_with_tail(left, a[1]))
}

/// `A -- B`: remove the first occurrence in `A` of each element of `B`.
pub fn subtract(c: &mut Ctx, a: &[Term]) -> R {
    let mut left = c.list_arg(a[0])?;
    let right = c.list_arg(a[1])?;
    let h = c.heap();
    for r in &right {
        if let Some(i) = left.iter().position(|l| h.eq_exact(*l, *r)) {
            left.remove(i);
        }
    }
    Ok(c.list(left))
}

// ---- binaries ----

fn bits(c: &Ctx, t: &Term) -> Result<Bits, Exception> {
    c.heap().as_bits(*t).ok_or_else(|| c.badarg())
}

fn binary(c: &Ctx, t: &Term) -> Result<Bits, Exception> {
    c.heap()
        .as_bits(*t)
        .filter(Bits::is_binary)
        .ok_or_else(|| c.badarg())
}

pub fn byte_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(
        c.heap()
            .bit_len(a[0])
            .ok_or_else(|| c.badarg())?
            .div_ceil(8) as i64,
    ))
}

pub fn bit_size(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(
        c.heap().bit_len(a[0]).ok_or_else(|| c.badarg())? as i64
    ))
}

/// `binary_part(Bin, {Start, Length})`.
pub fn binary_part2(c: &mut Ctx, a: &[Term]) -> R {
    match c.heap().as_tuple(a[1]) {
        Some(&[s, l]) => binary_part(c, &[a[0], s, l]),
        _ => Err(c.badarg()),
    }
}

pub fn binary_part(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let (start, len) = match (a[1].as_i64(), a[2].as_i64()) {
        (Some(s), Some(l)) => (s, l),
        _ => return Err(c.badarg()),
    };
    // A negative length counts back from `start`.
    let (lo, hi) = if len >= 0 {
        (start, start.checked_add(len))
    } else {
        (start + len, Some(start))
    };
    let size = (b.len / 8) as i64;
    match hi {
        Some(hi) if lo >= 0 && hi <= size => {
            Ok(c.bits(b.slice(lo as usize * 8, (hi - lo) as usize * 8)))
        }
        _ => Err(c.badarg()),
    }
}

pub fn split_binary(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    match a[1].as_usize() {
        Some(n) if n * 8 <= b.len => {
            let x = c.bits(b.slice(0, n * 8));
            let y = c.bits(b.slice(n * 8, b.len - n * 8));
            Ok(c.tuple(&[x, y]))
        }
        _ => Err(c.badarg()),
    }
}

// ---- atoms ----

/// A string from a list of code points (not an iolist).
fn list_to_string(c: &Ctx, t: &Term) -> Result<String, Exception> {
    let mut s = String::new();
    for item in c.heap().list_iter(*t) {
        let ch = item
            .ok()
            .and_then(|x| x.as_i64())
            .and_then(|i| u32::try_from(i).ok())
            .and_then(char::from_u32);
        s.push(ch.ok_or_else(|| c.badarg())?);
    }
    Ok(s)
}

fn atom_arg(c: &Ctx, t: &Term) -> Result<crate::atom::Atom, Exception> {
    match t {
        Term::Atom(a) => Ok(*a),
        _ => Err(c.badarg()),
    }
}

pub fn atom_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let atom = atom_arg(c, &a[0])?;
    Ok(c.string(atom.as_str()))
}

pub fn atom_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let atom = atom_arg(c, &a[0])?;
    if a.len() == 2
        && !(a[1].is_atom(&c.atoms.latin1)
            || a[1].is_atom(&c.atoms.unicode)
            || a[1].is_atom(&c.atoms.utf8))
    {
        return Err(c.badarg());
    }
    if a.len() == 2 && a[1].is_atom(&c.atoms.latin1) {
        let bytes: Option<Vec<u8>> = atom
            .as_str()
            .chars()
            .map(|ch| u8::try_from(ch as u32).ok())
            .collect();
        let bytes = bytes.ok_or_else(|| c.badarg())?;
        return Ok(c.binary(&bytes));
    }
    Ok(c.binary(atom.as_str().as_bytes()))
}

fn make_atom(c: &mut Ctx, s: &str, existing: bool) -> R {
    let atom = if existing {
        c.sys().atom_table.existing(s).ok_or_else(|| c.badarg())?
    } else {
        let found = c.sys().atom_table.intern(s);
        match found {
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
    if a.len() == 2 && a[1].is_atom(&c.atoms.latin1) {
        Ok(bytes.iter().map(|&b| b as char).collect())
    } else {
        core::str::from_utf8(&bytes)
            .map(|s| s.to_string())
            .map_err(|_| c.badarg())
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
    let i = c.heap().as_bigint(a[0]).ok_or_else(|| c.badarg())?;
    // Erlang prints digits above 9 in upper case.
    Ok(i.to_str_radix(r).to_uppercase())
}

pub fn integer_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let s = integer_text(c, a)?;
    Ok(c.string(&s))
}

pub fn integer_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let s = integer_text(c, a)?;
    Ok(c.binary(s.as_bytes()))
}

fn parse_integer(c: &mut Ctx, s: &str, r: u32) -> R {
    // An optional sign, then at least one digit; no whitespace, no `_`.
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(r)) {
        return Err(c.badarg());
    }
    if s.len() as u64 > super::arith::MAX_BIG_BITS / 3 {
        return Err(c.system_limit());
    }
    match BigInt::from_str_radix(s, r) {
        Ok(n) => Ok(c.big(n)),
        Err(_) => Err(c.badarg()),
    }
}

pub fn list_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    let s = list_to_string(c, &a[0])?;
    let r = radix(c, a)?;
    parse_integer(c, &s, r)
}

/// `erts_internal:list_to_integer(List, Base)`, the helper behind `string:to_integer/1`: the
/// integer at the start of `List` and the rest, `{Int, Rest}`, or `no_integer`, `not_a_list`,
/// `badarg` (a bad base) or `big` (past the bignum cap; the caller then raises `system_limit`).
pub fn internal_list_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    let atom = |c: &mut Ctx, s: &str| Ok(c.atom(s));
    let mut list = a[0];
    if matches!(list, Term::Nil) {
        return atom(c, "no_integer");
    }
    if !matches!(list, Term::Cons(_)) {
        return atom(c, "not_a_list");
    }
    let base = match a[1] {
        Term::Int(b @ 2..=36) => b as u32,
        _ => return atom(c, "badarg"),
    };
    let digit = |t: &Term| match t {
        Term::Int(ch @ 0..=255) => (*ch as u8 as char).to_digit(base),
        _ => None,
    };
    let mut neg = false;
    if let Some((Term::Int(s @ (43 | 45)), tail)) = c.heap().as_cons(list) {
        neg = s == 45;
        list = tail;
    }
    let mut digits = String::new();
    while let Some((head, tail)) = c.heap().as_cons(list) {
        let Some(d) = digit(&head) else { break };
        digits.push(char::from_digit(d, base).expect("a digit"));
        if digits.len() as u64 > super::arith::MAX_BIG_BITS / 3 {
            return atom(c, "big");
        }
        list = tail;
    }
    if digits.is_empty() {
        return atom(c, "no_integer");
    }
    let mut n = BigInt::from_str_radix(&digits, base).expect("digits");
    if neg {
        n = -n;
    }
    if n.bits() > super::arith::MAX_BIG_BITS {
        return atom(c, "big");
    }
    let n = c.big(n);
    Ok(c.tuple(&[n, list]))
}

/// `erts_internal:binary_to_integer(Bin, Base)`: the integer, or `badarg` or `big`.
pub fn internal_binary_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    match binary_to_integer(c, a) {
        Ok(n) => Ok(n),
        Err(e) if e.reason.is_atom(&c.atoms.system_limit) => Ok(c.atom("big")),
        Err(_) => Ok(c.atom("badarg")),
    }
}

pub fn dt_true(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(true))
}

pub fn dt_undefined(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Atom(c.atoms.undefined))
}

pub fn dt_same(_c: &mut Ctx, a: &[Term]) -> R {
    Ok(a[0])
}

pub fn binary_to_integer(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let s = core::str::from_utf8(&b.to_bytes())
        .map_err(|_| c.badarg())?
        .to_string();
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
    let s = float_classic(float_arg(c, &a[0])?);
    Ok(c.string(&s))
}

pub fn float_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let s = float_classic(float_arg(c, &a[0])?);
    Ok(c.binary(s.as_bytes()))
}

// ---- lists and binaries ----

pub fn binary_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let v: Vec<Term> = b.to_bytes().iter().map(|&x| Term::Int(x as i64)).collect();
    Ok(c.list(v))
}

pub fn binary_to_list3(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let size = b.len / 8;
    match (a[1].as_usize(), a[2].as_usize()) {
        (Some(s), Some(e)) if s >= 1 && s <= e && e <= size => {
            let v: Vec<Term> = b.to_bytes()[s - 1..e]
                .iter()
                .map(|&x| Term::Int(x as i64))
                .collect();
            Ok(c.list(v))
        }
        _ => Err(c.badarg()),
    }
}

pub fn bitstring_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = bits(c, &a[0])?;
    let whole = b.len / 8;
    let mut items: Vec<Term> = (0..whole).map(|i| Term::Int(b.byte(i) as i64)).collect();
    // Leftover bits are the last element of a proper list: [1, 2, <<3:4>>].
    if b.len % 8 != 0 {
        items.push(c.bits(b.slice(whole * 8, b.len % 8)));
    }
    Ok(c.list(items))
}

/// Flatten an iolist (bytes 0..255, binaries and nested iolists; a binary may end a list) into
/// bytes, or a bitstring if `bits_ok` and the last element is one.
fn flatten_iolist(c: &Ctx, t: &Term, bits_ok: bool) -> Result<crate::bits::Builder, Exception> {
    let h = c.heap();
    let mut out = crate::bits::Builder::new();
    // An explicit work stack: iolists can nest arbitrarily deep.
    let mut stack: Vec<Term> = alloc::vec![*t];
    while let Some(t) = stack.pop() {
        match t {
            Term::Nil => {}
            Term::Bits(_) => {
                let b = h.as_bits(t).expect("bits");
                if !b.is_binary() && !bits_ok {
                    return Err(c.badarg());
                }
                out.push_bits(&b);
            }
            Term::Cons(_) => {
                let (head, tail) = h.as_cons(t).expect("a list cell");
                stack.push(tail);
                match head {
                    Term::Int(i) if (0..=255).contains(&i) => out.push_byte(i as u8),
                    Term::Cons(_) | Term::Nil | Term::Bits(_) => stack.push(head),
                    _ => return Err(c.badarg()),
                }
            }
            _ => return Err(c.badarg()),
        }
        if out.bit_len() > c.sys().limits.max_binary_bits {
            return Err(c.system_limit());
        }
    }
    Ok(out)
}

/// `list_to_binary(IoList)`: the argument must be a list.
pub fn list_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    if !matches!(a[0], Term::Cons(_) | Term::Nil) {
        return Err(c.badarg());
    }
    let b = flatten_iolist(c, &a[0], false)?;
    Ok(b.finish(c.heap_mut()))
}

/// `iolist_to_binary(IoData)`: like `list_to_binary/1`, but a binary is returned as it is.
pub fn iolist_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    if c.heap().bit_len(a[0]).is_some_and(|n| n.is_multiple_of(8)) {
        return Ok(a[0]);
    }
    list_to_binary(c, a)
}

pub fn list_to_bitstring(c: &mut Ctx, a: &[Term]) -> R {
    if let Term::Bits(_) = &a[0] {
        return Ok(a[0]);
    }
    let b = flatten_iolist(c, &a[0], true)?;
    Ok(b.finish(c.heap_mut()))
}

/// `iolist_to_iovec(IoData)`: a list of binaries with the same bytes. One binary suffices.
pub fn iolist_to_iovec(c: &mut Ctx, a: &[Term]) -> R {
    let bin = iolist_to_binary(c, a)?;
    Ok(if c.heap().bit_len(bin) == Some(0) {
        Term::Nil
    } else {
        c.list([bin])
    })
}

pub fn iolist_size(c: &mut Ctx, a: &[Term]) -> R {
    let b = flatten_iolist(c, &a[0], false)?;
    Ok(Term::Int((b.bit_len() / 8) as i64))
}

pub fn display(c: &mut Ctx, a: &[Term]) -> R {
    let text = alloc::format!("{}\n", c.show(a[0]));
    c.sys().platform.console_write(text.as_bytes());
    Ok(Term::Atom(c.atoms.true_))
}

// ---- records and tuples ----

/// `is_record(Term, Tag)` and `is_record(Term, Tag, Size)`.
pub fn is_record(c: &mut Ctx, a: &[Term]) -> R {
    if !matches!(a[1], Term::Atom(_)) {
        return Err(c.badarg());
    }
    let size = match a.get(2) {
        None => None,
        Some(t) => Some(t.as_usize().ok_or_else(|| c.badarg())?),
    };
    let ok = match c.heap().as_tuple(a[0]) {
        Some(t) => {
            !t.is_empty() && c.heap().eq_exact(t[0], a[1]) && size.is_none_or(|s| s == t.len())
        }
        None => false,
    };
    Ok(c.bool(ok))
}

pub fn insert_element(c: &mut Ctx, a: &[Term]) -> R {
    let mut v = tuple(c, &a[1])?;
    let i = position(c, &a[0], v.len() + 1)?;
    v.insert(i, a[2]);
    Ok(c.tuple(&v))
}

pub fn delete_element(c: &mut Ctx, a: &[Term]) -> R {
    let mut v = tuple(c, &a[1])?;
    let i = position(c, &a[0], v.len())?;
    v.remove(i);
    Ok(c.tuple(&v))
}

// ---- floats as text, with options ----

/// `float_to_list(F, Options)` / `float_to_binary(F, Options)`: `{decimals, N}`, `compact`,
/// `{scientific, N}`, `short`. Later options override earlier ones, as in BEAM.
fn float_text(c: &Ctx, f: f64, opts: &Term) -> Result<String, Exception> {
    enum Fmt {
        Scientific(usize),
        Decimals(usize),
        Short,
    }
    let mut fmt = Fmt::Scientific(20);
    let mut compact = false;
    for o in c.list_arg(*opts)? {
        match (o, c.heap().as_tuple(o)) {
            (Term::Atom(a), _) if a.as_str() == "compact" => compact = true,
            (Term::Atom(a), _) if a.as_str() == "short" => fmt = Fmt::Short,
            (_, Some(&[Term::Atom(a), n])) => match n.as_usize() {
                Some(n) if a.as_str() == "decimals" && n <= 253 => fmt = Fmt::Decimals(n),
                Some(n) if a.as_str() == "scientific" && n <= 249 => fmt = Fmt::Scientific(n),
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    Ok(match fmt {
        Fmt::Short => crate::float::format_short(f),
        Fmt::Scientific(n) => {
            let s = alloc::format!("{f:.n$e}");
            let (m, e) = s.split_once('e').expect("{:e} has an exponent");
            let e: i32 = e.parse().expect("exponent");
            alloc::format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
        }
        Fmt::Decimals(n) => {
            let mut s = alloc::format!("{f:.n$}");
            if compact && s.contains('.') {
                // Drop trailing zeros, but keep one digit after the point.
                while s.ends_with('0') && !s.ends_with(".0") {
                    s.pop();
                }
            }
            s
        }
    })
}

pub fn float_to_list2(c: &mut Ctx, a: &[Term]) -> R {
    let f = float_arg(c, &a[0])?;
    let s = float_text(c, f, &a[1])?;
    Ok(c.string(&s))
}

pub fn float_to_binary2(c: &mut Ctx, a: &[Term]) -> R {
    let f = float_arg(c, &a[0])?;
    let s = float_text(c, f, &a[1])?;
    Ok(c.binary(s.as_bytes()))
}

/// Parse Erlang float syntax: digits, `.`, digits, optional exponent. `"1"` and `"1."` are not
/// floats; `"1.0e5"` and `"-2.5E-3"` are. Like BEAM (whose parser allows it), a `,` may stand for
/// the decimal point: `list_to_float("1,0")` is `1.0`.
fn parse_float(c: &Ctx, s: &str) -> R {
    let owned = s.replacen(',', ".", 1);
    let s = owned.as_str();
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let ok_mantissa = mantissa.split_once('.').is_some_and(|(w, f)| {
        !w.is_empty() && !f.is_empty() && (w.chars().chain(f.chars())).all(|ch| ch.is_ascii_digit())
    });
    let ok_exponent = exponent.is_none_or(|e| {
        let d = e.strip_prefix(['+', '-']).unwrap_or(e);
        !d.is_empty() && d.chars().all(|ch| ch.is_ascii_digit())
    });
    if !ok_mantissa || !ok_exponent {
        return Err(c.badarg());
    }
    match s.parse::<f64>() {
        Ok(f) if f.is_finite() => Ok(Term::Float(f)),
        _ => Err(c.badarg()),
    }
}

pub fn list_to_float(c: &mut Ctx, a: &[Term]) -> R {
    let s = list_to_string(c, &a[0])?;
    parse_float(c, &s)
}

pub fn binary_to_float(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let s = core::str::from_utf8(&b.to_bytes())
        .map_err(|_| c.badarg())?
        .to_string();
    parse_float(c, &s)
}

// ---- the external term format ----

/// `term_to_binary(Term)` and `term_to_binary(Term, Options)`. `compressed` and
/// `{compressed, Level}` compress (when that is smaller); `minor_version`, `deterministic` and
/// `local` change nothing, since maps are always written in key order.
pub fn term_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let mut level = 0u8;
    if let Some(opts) = a.get(1) {
        for o in c.list_arg(*opts)? {
            match (o, c.heap().as_tuple(o)) {
                (Term::Atom(x), _) if x.as_str() == "compressed" => level = 6,
                (Term::Atom(x), _) if x.as_str() == "deterministic" || x.as_str() == "local" => {}
                (_, Some(&[Term::Atom(k), Term::Int(l @ 0..=9)])) if k.as_str() == "compressed" => {
                    level = l as u8
                }
                (_, Some(&[Term::Atom(k), Term::Int(0..=2)])) if k.as_str() == "minor_version" => {}
                _ => return Err(c.badarg()),
            }
        }
    }
    // The lock only for looking up a fun's module: encoding runs alongside other schedulers.
    let bytes = crate::etf::encode_compressed(&c.p.heap, a[0], level, &|m| c.sys().loaded_md5(m))
        .map_err(|_| c.badarg())?;
    Ok(c.binary(&bytes))
}

/// `term_to_iovec(Term)`: the external format as a list of binaries (one, here).
pub fn term_to_iovec(c: &mut Ctx, a: &[Term]) -> R {
    let b = term_to_binary(c, a)?;
    Ok(c.list([b]))
}

/// `external_size(Term)`: how many bytes `term_to_binary` would produce.
pub fn external_size(c: &mut Ctx, a: &[Term]) -> R {
    let b = term_to_binary(c, a)?;
    Ok(Term::Int(
        (c.heap().bit_len(b).expect("a binary") / 8) as i64,
    ))
}

/// `string:list_to_float(String)`: the float at the start of `String` and the rest, as
/// `{Float, Rest}`, or `{error, no_float}`. The float needs digits on both sides of the point.
pub fn string_list_to_float(c: &mut Ctx, a: &[Term]) -> R {
    let mut chars = Vec::new();
    let mut rest = a[0];
    while let Some((head, tail)) = c.heap().as_cons(rest) {
        match head {
            Term::Int(ch) if (0..128).contains(&ch) => chars.push(ch as u8),
            _ => break,
        }
        rest = tail;
    }
    let digits = |s: &[u8], i: usize| i + s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    let mut i = if matches!(chars.first(), Some(b'+' | b'-')) {
        1
    } else {
        0
    };
    let int_end = digits(&chars, i);
    let mut end = None;
    if int_end > i && chars.get(int_end) == Some(&b'.') {
        let frac_end = digits(&chars, int_end + 1);
        if frac_end > int_end + 1 {
            end = Some(frac_end);
            i = frac_end;
            if matches!(chars.get(i), Some(b'e' | b'E')) {
                let j = if matches!(chars.get(i + 1), Some(b'+' | b'-')) {
                    i + 2
                } else {
                    i + 1
                };
                let exp_end = digits(&chars, j);
                if exp_end > j {
                    end = Some(exp_end);
                }
            }
        }
    }
    let no_float = |c: &mut Ctx| {
        let (error, no_float) = (Term::Atom(c.atoms.error), c.atom("no_float"));
        Ok(c.tuple(&[error, no_float]))
    };
    let Some(end) = end else { return no_float(c) };
    let text = core::str::from_utf8(&chars[..end]).expect("ASCII");
    let Ok(f) = text.parse::<f64>() else {
        return no_float(c);
    };
    if !f.is_finite() {
        return no_float(c);
    }
    let mut tail = a[0];
    for _ in 0..end {
        tail = c.heap().as_cons(tail).expect("counted").1;
    }
    Ok(c.tuple(&[Term::Float(f), tail]))
}

/// `binary_to_term(Bin)` and `binary_to_term(Bin, Options)` with `safe` (create no atoms) and
/// `used` (also return how many bytes were read).
pub fn binary_to_term(c: &mut Ctx, a: &[Term]) -> R {
    let b = binary(c, &a[0])?;
    let bytes = b.to_bytes().into_owned();
    let (mut safe, mut used) = (false, false);
    if let Some(opts) = a.get(1) {
        for o in c.list_arg(*opts)? {
            match &o {
                Term::Atom(x) if x.as_str() == "safe" => safe = true,
                Term::Atom(x) if x.as_str() == "used" => used = true,
                _ => return Err(c.badarg()),
            }
        }
    }
    let (t, n) = crate::etf::decode_prefix(&bytes, &mut c.sys().atom_table, &mut c.p.heap, safe)
        .map_err(|_| c.badarg())?;
    if used {
        return Ok(c.tuple(&[t, Term::Int(n as i64)]));
    }
    if n != bytes.len() {
        return Err(c.badarg());
    }
    Ok(t)
}

/// `erts_debug:flat_size(Term)`: the heap words BEAM would use to copy `Term` (64-bit, OTP 28),
/// ignoring sharing. Calibrated against the real BEAM; see `tests/erlang/flat_size.erl`. Maps
/// above 32 keys are counted as flat maps, which BEAM does not use for them. A term whose flat
/// size passes `Limits::max_heap_words` (possible for one built with sharing, whose flattened
/// copy could be exponentially larger) raises `system_limit` rather than walking it all.
pub fn flat_size(c: &mut Ctx, a: &[Term]) -> R {
    let h = c.heap();
    let mut words: u64 = 0;
    let mut work = alloc::vec![a[0]];
    while let Some(t) = work.pop() {
        if words > c.sys().limits.max_heap_words {
            return Err(c.system_limit());
        }
        words += match t {
            Term::Int(_) | Term::Atom(_) | Term::Nil | Term::Pid(_) => 0,
            Term::Big(_) => 1 + h.as_big(t).expect("a bignum").bits().div_ceil(64),
            Term::Float(_) => 2,
            Term::Cons(_) => {
                let (head, tail) = h.as_cons(t).expect("a list cell");
                work.push(head);
                work.push(tail);
                2
            }
            Term::Tuple(_) => {
                let e = h.as_tuple(t).expect("a tuple");
                work.extend(e.iter().copied());
                if e.is_empty() {
                    0
                } else {
                    1 + e.len() as u64
                }
            }
            Term::Map(_) => {
                let entries = h.map_entries(t).expect("a map");
                let n = entries.len() as u64;
                for (k, v) in entries {
                    work.push(k);
                    work.push(v);
                }
                3 + n + if n > 0 { 1 + n } else { 0 }
            }
            Term::Bits(_) => {
                let bytes = h.bit_len(t).expect("bits").div_ceil(8) as u64;
                if bytes <= 64 {
                    2 + bytes.div_ceil(8)
                } else {
                    8
                }
            }
            Term::Fun(_) => match h.as_fun(t).expect("a fun") {
                crate::term::FunView::Export { .. } => 2,
                crate::term::FunView::Local { env, .. } => {
                    work.extend(env.iter().copied());
                    2 + env.len() as u64
                }
            },
            Term::Ref(_) | Term::Resource(_) => 3,
            Term::Match(_) | Term::Node(_) | Term::Header(_) | Term::OffHeap(_) => 0,
        };
    }
    Ok(Term::Int(words as i64))
}
