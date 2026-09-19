//! `unicode:characters_to_list/2` and `unicode:characters_to_binary/2`, the native half of the
//! `unicode` module. Supported input encodings: `latin1`, `unicode` and `utf8`.

use alloc::vec::Vec;

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// Why conversion stopped early.
enum Stop {
    /// An invalid code point or byte sequence; the rest of the input is returned as is.
    Error(Term),
    /// Input ended inside a UTF-8 sequence.
    Incomplete(Term),
}

/// Walk chardata (code points, binaries and nested lists) collecting code points.
fn collect(c: &Ctx, data: &Term, latin1: bool, out: &mut Vec<char>) -> Result<Option<Stop>, Exception> {
    // An explicit stack of what is left to convert, so nesting depth costs no Rust stack.
    let mut work = alloc::vec![data.clone()];
    while let Some(t) = work.pop() {
        match &t {
            Term::Nil => {}
            Term::Bits(b) if b.is_binary() => {
                let bytes = b.to_bytes();
                if latin1 {
                    out.extend(bytes.iter().map(|&x| x as char));
                    continue;
                }
                match core::str::from_utf8(&bytes) {
                    Ok(s) => out.extend(s.chars()),
                    Err(e) => {
                        let good = e.valid_up_to();
                        out.extend(core::str::from_utf8(&bytes[..good]).unwrap_or("").chars());
                        let rest = Term::bits(b.slice(good * 8, b.len - good * 8));
                        return Ok(Some(if e.error_len().is_none() { Stop::Incomplete(rest) } else { Stop::Error(rest) }));
                    }
                }
            }
            Term::Cons(cell) => {
                match &cell.head {
                    Term::Int(i) => match u32::try_from(*i).ok().and_then(char::from_u32) {
                        Some(ch) if !latin1 || *i <= 255 => out.push(ch),
                        _ => {
                            let rest = Term::list_with_tail(core::iter::once(cell.head.clone()).collect::<Vec<_>>(), cell.tail.clone());
                            let mut pending = Vec::new();
                            pending.push(rest);
                            pending.extend(work.into_iter().rev());
                            return Ok(Some(Stop::Error(Term::list(pending))));
                        }
                    },
                    h @ (Term::Bits(_) | Term::Cons(_) | Term::Nil) => {
                        work.push(cell.tail.clone());
                        work.push(h.clone());
                        continue;
                    }
                    _ => return Err(c.badarg()),
                }
                work.push(cell.tail.clone());
            }
            _ => return Err(c.badarg()),
        }
    }
    Ok(None)
}

fn in_encoding(c: &Ctx, t: &Term) -> Result<bool, Exception> {
    match t {
        Term::Atom(a) if a.as_str() == "latin1" => Ok(true),
        Term::Atom(a) if a.as_str() == "unicode" || a.as_str() == "utf8" => Ok(false),
        _ => Err(c.badarg()),
    }
}

fn finish(c: &mut Ctx, converted: Term, stop: Option<Stop>) -> Term {
    match stop {
        None => converted,
        Some(Stop::Error(rest)) => Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), converted, rest]),
        Some(Stop::Incomplete(rest)) => Term::tuple(alloc::vec![c.atom("incomplete"), converted, rest]),
    }
}

pub fn characters_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let latin1 = in_encoding(c, &a[1])?;
    let mut chars = Vec::new();
    let stop = collect(c, &a[0], latin1, &mut chars)?;
    let list = Term::list(chars.into_iter().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>());
    Ok(finish(c, list, stop))
}

pub fn characters_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let latin1 = in_encoding(c, &a[1])?;
    let mut chars = Vec::new();
    let stop = collect(c, &a[0], latin1, &mut chars)?;
    let s: alloc::string::String = chars.into_iter().collect();
    let bin = Term::binary(s.as_bytes());
    Ok(finish(c, bin, stop))
}
