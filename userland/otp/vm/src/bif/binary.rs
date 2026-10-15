//! The `binary` module's native functions. The rest of `binary` is the real OTP module.

use alloc::vec::Vec;

use num_bigint::{BigInt, Sign};

use super::Ctx;
use crate::process::Exception;
use crate::term::{Bits, Term};

type R = Result<Term, Exception>;

fn bin(c: &Ctx, t: &Term) -> Result<Bits, Exception> {
    c.heap().as_bits(*t).filter(Bits::is_binary).ok_or_else(|| c.badarg())
}

fn bytes_of(c: &Ctx, t: &Term) -> Result<Vec<u8>, Exception> { Ok(bin(c, t)?.to_bytes().into_owned()) }

/// A `{Start, Length}` part of `size` bytes; a negative length counts backwards.
fn part_range(c: &Ctx, start: &Term, len: &Term, size: usize) -> Result<(usize, usize), Exception> {
    let (Some(s), Some(l)) = (start.as_i64(), len.as_i64()) else {
        return Err(c.badarg());
    };
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
        2 => match c.heap().as_tuple(a[1]) {
            Some(&[s, l]) => (s, l),
            _ => return Err(c.badarg()),
        },
        _ => (a[1], a[2]),
    };
    let (lo, hi) = part_range(c, &start, &len, b.len / 8)?;
    Ok(c.bits(b.slice(lo * 8, (hi - lo) * 8)))
}

pub fn copy(c: &mut Ctx, a: &[Term]) -> R {
    let bytes = bytes_of(c, &a[0])?;
    let n = match a.get(1) {
        None => 1,
        Some(t) => t.as_usize().ok_or_else(|| c.badarg())?,
    };
    if bytes.len().saturating_mul(n).saturating_mul(8) > c.sys().limits.max_binary_bits {
        return Err(c.system_limit());
    }
    Ok(c.binary(&bytes.repeat(n)))
}

pub fn bin_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let size = b.len / 8;
    let (lo, hi) = match a.len() {
        1 => (0, size),
        2 => match c.heap().as_tuple(a[1]) {
            Some(&[s, l]) => part_range(c, &s, &l, size)?,
            _ => return Err(c.badarg()),
        },
        _ => part_range(c, &a[1], &a[2], size)?,
    };
    Ok(c.list((lo..hi).map(|i| Term::Int(b.byte(i) as i64)).collect::<Vec<_>>()))
}

pub fn list_to_bin(c: &mut Ctx, a: &[Term]) -> R { super::erlang::list_to_binary(c, a) }

pub fn encode_unsigned(c: &mut Ctx, a: &[Term]) -> R {
    let v = c.heap().as_bigint(a[0]).filter(|v| v.sign() != Sign::Minus).ok_or_else(|| c.badarg())?;
    let little = a.get(1).is_some_and(|e| e.is_atom(&c.atoms.little));
    if a.len() == 2 && !little && !a[1].is_atom(&c.atoms.big) {
        return Err(c.badarg());
    }
    let mut bytes = if little { v.to_bytes_le().1 } else { v.to_bytes_be().1 };
    if bytes.is_empty() {
        bytes.push(0);
    }
    Ok(c.binary(&bytes))
}

pub fn decode_unsigned(c: &mut Ctx, a: &[Term]) -> R {
    let bytes = bytes_of(c, &a[0])?;
    let little = a.get(1).is_some_and(|e| e.is_atom(&c.atoms.little));
    if a.len() == 2 && !little && !a[1].is_atom(&c.atoms.big) {
        return Err(c.badarg());
    }
    Ok(c.big(if little {
        BigInt::from_bytes_le(Sign::Plus, &bytes)
    } else {
        BigInt::from_bytes_be(Sign::Plus, &bytes)
    }))
}

// ---- searching ----

/// A compiled pattern: what `compile_pattern/1` builds and keeps in a resource, and what a search
/// over an uncompiled pattern builds for the call. The patterns are sorted by their first byte,
/// the longest first among those sharing one, and `starts` indexes the groups, so a byte of the
/// haystack is compared only against the patterns that start with it: a search costs the
/// haystack's length, not its length times the pattern count, and at the leftmost position that
/// matches the longest pattern still wins.
pub struct Matcher {
    pats: Vec<Vec<u8>>,
    /// The patterns starting with byte `b` are `pats[starts[b]..starts[b + 1]]`.
    starts: [u32; 257],
}

impl Matcher {
    /// From the patterns, none empty (the caller checks).
    pub fn new(mut pats: Vec<Vec<u8>>) -> Matcher {
        pats.sort_by(|a, b| a[0].cmp(&b[0]).then(b.len().cmp(&a.len())));
        let mut starts = [0u32; 257];
        for p in &pats {
            starts[p[0] as usize + 1] += 1;
        }
        for b in 0..256 {
            starts[b + 1] += starts[b];
        }
        Matcher { pats, starts }
    }

    /// The first match within `hay[from..until]`: at the leftmost position, the longest pattern.
    pub fn find(&self, hay: &[u8], from: usize, until: usize) -> Option<(usize, usize)> {
        let hay = &hay[..until];
        for i in from..until {
            let b = hay[i] as usize;
            let group = &self.pats[self.starts[b] as usize..self.starts[b + 1] as usize];
            if let Some(p) = group.iter().find(|p| hay[i..].starts_with(p)) {
                return Some((i, p.len()));
            }
        }
        None
    }

    /// What it holds, which counts toward its holders' heap limit.
    pub fn bytes(&self) -> usize {
        size_of::<Matcher>() + self.pats.iter().map(|p| size_of::<Vec<u8>>() + p.len()).sum::<usize>()
    }
}

/// The matcher a search runs: the one a compiled pattern holds, or one built for this call.
enum Pattern {
    Compiled(super::Held<Matcher>),
    Fresh(Matcher),
}

impl core::ops::Deref for Pattern {
    type Target = Matcher;

    fn deref(&self) -> &Matcher {
        match self {
            Pattern::Compiled(m) => m,
            Pattern::Fresh(m) => m,
        }
    }
}

/// A pattern: one binary or a list of them, none empty, or `compile_pattern/1`'s `{bm | ac, Ref}`.
fn pattern(c: &Ctx, t: &Term) -> Result<Pattern, Exception> {
    let list = match t {
        Term::Bits(_) => alloc::vec![*t],
        Term::Tuple(_) => {
            return match c.heap().as_tuple(*t) {
                Some(&[Term::Atom(tag), r]) if matches!(tag.as_str(), "bm" | "ac") => {
                    c.resource::<Matcher>(r).map(Pattern::Compiled)
                }
                _ => None,
            }
            .ok_or_else(|| c.badarg());
        }
        _ => c.heap().to_vec(*t).ok_or_else(|| c.badarg())?,
    };
    let mut pats = Vec::with_capacity(list.len());
    for p in &list {
        let b = bytes_of(c, p)?;
        if b.is_empty() {
            return Err(c.badarg());
        }
        pats.push(b);
    }
    if pats.is_empty() {
        return Err(c.badarg());
    }
    Ok(Pattern::Fresh(Matcher::new(pats)))
}

/// The elements of an options list. Like BEAM, an improper tail ends the list and is ignored
/// (`[global | foo]` means `[global]`).
fn options(c: &Ctx, t: &Term) -> Result<Vec<Term>, Exception> {
    if !matches!(t, Term::Nil | Term::Cons(_)) {
        return Err(c.badarg());
    }
    Ok(c.heap().list_iter(*t).map_while(|x| x.ok()).collect())
}

/// `{scope, {Start, Length}}` from an options list, or the whole binary.
fn scope(c: &Ctx, opts: Option<&Term>, size: usize) -> Result<(usize, usize), Exception> {
    let Some(opts) = opts else {
        return Ok((0, size));
    };
    let mut range = (0, size);
    for o in options(c, opts)? {
        match c.heap().as_tuple(o) {
            Some(&[Term::Atom(tag), part]) if tag.as_str() == "scope" => match c.heap().as_tuple(part) {
                Some(&[s, l]) => range = part_range(c, &s, &l, size)?,
                _ => return Err(c.badarg()),
            },
            _ => return Err(c.badarg()),
        }
    }
    Ok(range)
}

fn found(c: &mut Ctx, i: usize, len: usize) -> Term { c.tuple(&[Term::Int(i as i64), Term::Int(len as i64)]) }

/// `compile_pattern(Pattern)`: `{bm, Ref}` for one pattern and `{ac, Ref}` for several, as on BEAM,
/// the reference a resource holding the matcher, which counts toward its holders' heap limit.
pub fn compile_pattern(c: &mut Ctx, a: &[Term]) -> R {
    let Pattern::Fresh(m) = pattern(c, &a[0])? else {
        return Err(c.badarg());
    };
    let tag = c.atom(if m.pats.len() == 1 { "bm" } else { "ac" });
    let bytes = m.bytes();
    let r = c.new_resource_sized(m, bytes);
    Ok(c.tuple(&[tag, r]))
}

pub fn match_(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let hay = b.to_bytes();
    let m = pattern(c, &a[1])?;
    let (lo, hi) = scope(c, a.get(2), hay.len())?;
    Ok(match m.find(&hay, lo, hi) {
        Some((i, len)) => found(c, i, len),
        None => c.atom("nomatch"),
    })
}

pub fn matches(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let hay = b.to_bytes();
    let m = pattern(c, &a[1])?;
    let (mut at, hi) = scope(c, a.get(2), hay.len())?;
    let mut out = Vec::new();
    while let Some((i, len)) = m.find(&hay, at, hi) {
        out.push(found(c, i, len));
        at = i + len;
    }
    Ok(c.list(out))
}

/// `split(Bin, Pattern, Options)` with `global`, `trim`, `trim_all` and `{scope, _}`.
pub fn split(c: &mut Ctx, a: &[Term]) -> R {
    let b = bin(c, &a[0])?;
    let hay = b.to_bytes();
    let m = pattern(c, &a[1])?;
    let (mut global, mut trim, mut trim_all) = (false, false, false);
    let mut scope_opts = Vec::new();
    if let Some(opts) = a.get(2) {
        for o in options(c, opts)? {
            match &o {
                Term::Atom(x) if x.as_str() == "global" => global = true,
                Term::Atom(x) if x.as_str() == "trim" => trim = true,
                Term::Atom(x) if x.as_str() == "trim_all" => trim_all = true,
                Term::Tuple(_) => scope_opts.push(o),
                _ => return Err(c.badarg()),
            }
        }
    }
    let scope_list = c.list(scope_opts);
    let (mut at, hi) = scope(c, Some(&scope_list), hay.len())?;
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while let Some((i, len)) = m.find(&hay, at, hi) {
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
    let pieces: Vec<Term> = pieces.into_iter().map(|(s, e)| c.bits(b.slice(s * 8, (e - s) * 8))).collect();
    Ok(c.list(pieces))
}

fn common(c: &Ctx, a: &Term, suffix: bool) -> R {
    let bins: Vec<Vec<u8>> = c
        .heap()
        .to_vec(*a)
        .ok_or_else(|| c.badarg())?
        .iter()
        .map(|t| bytes_of(c, t))
        .collect::<Result<_, _>>()?;
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

pub fn longest_common_prefix(c: &mut Ctx, a: &[Term]) -> R { common(c, &a[0], false) }

pub fn longest_common_suffix(c: &mut Ctx, a: &[Term]) -> R { common(c, &a[0], true) }

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::Matcher;

    fn matcher(pats: &[&[u8]]) -> Matcher {
        Matcher::new(pats.iter().map(|p| p.to_vec()).collect::<Vec<_>>())
    }

    #[test]
    fn the_longest_pattern_wins_at_the_leftmost_position() {
        // One pattern a prefix of another, whichever order they are given in.
        for pats in [&[&b"ab"[..], &b"abc"[..]], &[&b"abc"[..], &b"ab"[..]]] {
            let m = matcher(pats);
            assert_eq!(m.find(b"xabcd", 0, 5), Some((1, 3)));
            assert_eq!(m.find(b"xabd", 0, 4), Some((1, 2)));
        }
        // A shorter pattern that starts earlier beats a longer one that starts later.
        let m = matcher(&[b"b", b"abcd"]);
        assert_eq!(m.find(b"xbabcd", 0, 6), Some((1, 1)));
        assert_eq!(m.find(b"xabcd", 0, 5), Some((1, 4)));
    }

    #[test]
    fn patterns_sharing_a_first_byte_are_each_tried() {
        let m = matcher(&[b"ab", b"ac", b"ad"]);
        assert_eq!(m.find(b"aaab", 0, 4), Some((2, 2)));
        assert_eq!(m.find(b"aaac", 0, 4), Some((2, 2)));
        assert_eq!(m.find(b"aaad", 0, 4), Some((2, 2)));
        assert_eq!(m.find(b"aaae", 0, 4), None);
    }

    #[test]
    fn a_match_at_the_last_byte_is_found_and_one_past_the_scope_is_not() {
        let m = matcher(&[b"z", b"yz"]);
        assert_eq!(m.find(b"abcz", 0, 4), Some((3, 1)));
        assert_eq!(m.find(b"abyz", 0, 4), Some((2, 2)));
        // The whole pattern must lie within the scope.
        assert_eq!(m.find(b"abyz", 0, 3), None);
        assert_eq!(matcher(&[b"abc"]).find(b"abc", 0, 2), None);
        assert_eq!(matcher(&[b"abc"]).find(b"ab", 0, 2), None);
    }

    #[test]
    fn an_empty_scope_at_the_end_matches_nothing() {
        let m = matcher(&[b"a"]);
        assert_eq!(m.find(b"aaa", 3, 3), None);
        assert_eq!(m.find(b"aaa", 2, 3), Some((2, 1)));
        assert_eq!(m.find(b"", 0, 0), None);
    }

    #[test]
    fn every_byte_value_is_a_first_byte() {
        let pats: Vec<Vec<u8>> = (0..=255u8).map(|b| alloc::vec![b, b]).collect();
        let m = Matcher::new(pats);
        assert_eq!(m.find(b"\x00\xff\xff", 0, 3), Some((1, 2)));
        assert_eq!(m.find(b"\x00\x00", 0, 2), Some((0, 2)));
        assert_eq!(m.find(b"\x01\x02\x03", 0, 3), None);
    }
}
