//! `unicode:characters_to_list/2` and `unicode:characters_to_binary/2`, the native half of the
//! `unicode` module. Supported input encodings: `latin1`, `unicode` and `utf8`.

use alloc::vec::Vec;

use super::Ctx;
use crate::process::Exception;
use crate::term::Term;

type R = Result<Term, Exception>;

/// Why conversion stopped early. The rest is shaped as BEAM shapes it (see [`collect`]).
enum Stop {
    /// An invalid code point or byte sequence, or an unfinished one with more input after it.
    Error(Term),
    /// Input ended inside a UTF-8 sequence: its bytes.
    Incomplete(Term),
}

/// The bytes a UTF-8 sequence starting with `lead` takes (a lead `from_utf8` left unfinished).
fn sequence_len(lead: u8) -> usize {
    match lead {
        0xF0.. => 4,
        0xE0.. => 3,
        _ => 2,
    }
}

/// What is left where conversion stopped: `cell`, the list cell (or a list's tail) at that
/// point, inside each enclosing list's remaining tail, innermost last in `enclosing`.
fn stopped_at(c: &mut Ctx, cell: Term, enclosing: &[Term]) -> Term {
    enclosing.iter().rev().fold(cell, |rest, &tail| c.cons(rest, tail))
}

/// Walk chardata (code points, binaries and nested lists) collecting code points. On a code
/// point that cannot be converted the rest is the list cell holding it, inside each enclosing
/// list's remaining tail; on a byte sequence that is not UTF-8, the same with the binary's
/// undecoded bytes in place of the binary. A sequence a binary's end cuts short takes its
/// missing bytes from the binaries that follow, skipping empty lists and binaries; if the
/// input ends first the result is incomplete, and if a code point or a bad byte comes first
/// it is an error whose rest is `[Bytes, Rest]`, `Rest` the rest at that point.
fn collect(c: &mut Ctx, data: &Term, latin1: bool, out: &mut Vec<char>) -> Result<Option<Stop>, Exception> {
    // What is left of each enclosing list, the input at the bottom: nesting costs no Rust stack.
    let mut levels = alloc::vec![*data];
    // The bytes of a UTF-8 sequence a binary's end cut short.
    let mut partial: Vec<u8> = Vec::new();
    while let Some(&top) = levels.last() {
        // A binary, and the tail after it if it is a list's element (none if it is the input or
        // a list's tail itself).
        let (bin, tail) = match top {
            Term::Nil => {
                levels.pop();
                continue;
            }
            Term::Bits(_) => {
                levels.pop();
                (top, None)
            }
            Term::Cons(_) => {
                let (head, tail) = c.heap().as_cons(top).expect("a list cell");
                *levels.last_mut().expect("the top") = tail;
                match head {
                    Term::Nil => continue,
                    Term::Cons(_) => {
                        levels.push(head);
                        continue;
                    }
                    Term::Bits(_) => (head, Some(tail)),
                    Term::Int(i) => {
                        let ch =
                            u32::try_from(i).ok().and_then(char::from_u32).filter(|_| !latin1 || i <= 255);
                        match ch {
                            Some(ch) if partial.is_empty() => {
                                out.push(ch);
                                continue;
                            }
                            _ => {
                                let cell = c.cons(head, tail);
                                let rest = stopped_at(c, cell, &levels[..levels.len() - 1]);
                                if partial.is_empty() {
                                    return Ok(Some(Stop::Error(rest)));
                                }
                                let bytes = c.binary(&partial);
                                return Ok(Some(Stop::Error(c.list([bytes, rest]))));
                            }
                        }
                    }
                    _ => return Err(c.badarg()),
                }
            }
            _ => return Err(c.badarg()),
        };
        let b = c.heap().as_bits(bin).filter(|b| b.is_binary()).ok_or_else(|| c.badarg())?;
        let bytes = b.to_bytes();
        if latin1 {
            out.extend(bytes.iter().map(|&x| x as char));
            continue;
        }
        // Where the binary's own characters start: past what an unfinished sequence takes.
        let mut start = 0;
        if !partial.is_empty() {
            let missing = sequence_len(partial[0]) - partial.len();
            if bytes.len() < missing {
                partial.extend_from_slice(&bytes);
                continue;
            }
            let mut sequence = partial.clone();
            sequence.extend_from_slice(&bytes[..missing]);
            match core::str::from_utf8(&sequence) {
                Ok(s) => {
                    out.extend(s.chars());
                    partial.clear();
                    start = missing;
                }
                Err(_) => {
                    let cell = match tail {
                        Some(tail) => c.cons(bin, tail),
                        None => bin,
                    };
                    let enclosing = if tail.is_some() { &levels[..levels.len() - 1] } else { &levels[..] };
                    let rest = stopped_at(c, cell, enclosing);
                    let done = c.binary(&partial);
                    return Ok(Some(Stop::Error(c.list([done, rest]))));
                }
            }
        }
        match core::str::from_utf8(&bytes[start..]) {
            Ok(s) => out.extend(s.chars()),
            Err(e) => {
                let good = start + e.valid_up_to();
                out.extend(core::str::from_utf8(&bytes[start..good]).unwrap_or("").chars());
                if e.error_len().is_none() {
                    partial.extend_from_slice(&bytes[good..]);
                    continue;
                }
                let left = c.bits(b.slice(good * 8, b.len - good * 8));
                let (cell, enclosing) = match tail {
                    Some(tail) => (c.cons(left, tail), &levels[..levels.len() - 1]),
                    None => (left, &levels[..]),
                };
                return Ok(Some(Stop::Error(stopped_at(c, cell, enclosing))));
            }
        }
    }
    if partial.is_empty() {
        return Ok(None);
    }
    // The input ended inside a sequence: unfinished if what came is a sequence's start.
    let unfinished = matches!(core::str::from_utf8(&partial), Err(e) if e.error_len().is_none());
    let bytes = c.binary(&partial);
    Ok(Some(if unfinished { Stop::Incomplete(bytes) } else { Stop::Error(bytes) }))
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
        Some(Stop::Error(rest)) => {
            let e = [Term::Atom(c.atoms.error), converted, rest];
            c.tuple(&e)
        }
        Some(Stop::Incomplete(rest)) => {
            let e = [c.atom("incomplete"), converted, rest];
            c.tuple(&e)
        }
    }
}

/// `unicode:bin_is_7bit(Bin)`: whether `Bin` is a binary of ASCII bytes (`false` for anything
/// that is not a binary).
pub fn bin_is_7bit(c: &mut Ctx, a: &[Term]) -> R {
    let ascii = c.heap().as_bits(a[0]).is_some_and(|b| b.is_binary() && b.to_bytes().is_ascii());
    Ok(c.bool(ascii))
}

pub fn characters_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let latin1 = in_encoding(c, &a[1])?;
    let mut chars = Vec::new();
    let stop = collect(c, &a[0], latin1, &mut chars)?;
    let list = {
        let v = chars.into_iter().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>();
        c.list(v)
    };
    Ok(finish(c, list, stop))
}

pub fn characters_to_binary(c: &mut Ctx, a: &[Term]) -> R {
    let latin1 = in_encoding(c, &a[1])?;
    // A list of one binary is that binary, as on BEAM: its rest is the binary's bytes alone.
    let data = match c.heap().as_cons(a[0]) {
        Some((only @ Term::Bits(_), Term::Nil)) => only,
        _ => a[0],
    };
    let mut chars = Vec::new();
    let stop = collect(c, &data, latin1, &mut chars)?;
    let s: alloc::string::String = chars.into_iter().collect();
    let bin = {
        let v = s.as_bytes();
        c.binary(v)
    };
    Ok(finish(c, bin, stop))
}
