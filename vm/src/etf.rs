//! The external term format (`term_to_binary/1`, `binary_to_term/1`).
//!
//! Decoding reads module literals and untrusted binaries, so it treats its input as hostile:
//! every length is checked against the bytes that remain, nothing is preallocated from an
//! untrusted count, and nesting depth is bounded so a crafted term cannot exhaust the stack.
//! In `safe` mode it also refuses to create atoms.
//!
//! Encoding writes what OTP 28 writes, byte for byte, except that maps are always in key order
//! (as with `term_to_binary(T, [deterministic])`). It uses a work list, not recursion.

use alloc::rc::Rc;
use alloc::vec::Vec;

use num_bigint::{BigInt, Sign};

use crate::atom::{AtomError, AtomTable};
use crate::term::{Bits, Fun, Map, MapKey, Term};

/// How deeply tuples, maps and list heads may nest. Lists nest along the tail without limit.
pub const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtfError {
    /// Input ended inside a term.
    Truncated,
    /// Missing version byte or an unknown or unsupported tag.
    BadTag(u8),
    /// Nesting deeper than [`MAX_DEPTH`].
    TooDeep,
    /// An atom that is not valid text or exceeds the atom limits.
    BadAtom,
    /// A float that is NaN or infinite, which Erlang does not have.
    BadFloat,
    /// A malformed value (bad bit count, a fun arity that is not an integer, ...).
    Malformed,
    /// Bytes left over after the term.
    TrailingBytes,
}

impl From<AtomError> for EtfError {
    fn from(_: AtomError) -> Self {
        EtfError::BadAtom
    }
}

const VERSION: u8 = 131;

/// Decode one complete term (with its version byte) that must fill all of `bytes`.
pub fn decode(bytes: &[u8], atoms: &mut AtomTable) -> Result<Term, EtfError> {
    let (t, used) = decode_prefix(bytes, atoms, false)?;
    if used != bytes.len() {
        return Err(EtfError::TrailingBytes);
    }
    Ok(t)
}

/// Decode the term at the start of `bytes`; also return how many bytes it used. With `safe`,
/// an atom that does not already exist is an error rather than a new atom.
pub fn decode_prefix(bytes: &[u8], atoms: &mut AtomTable, safe: bool) -> Result<(Term, usize), EtfError> {
    let mut r = Reader { bytes, pos: 0, atoms, safe };
    if r.u8()? != VERSION {
        return Err(EtfError::BadTag(bytes[0]));
    }
    let t = r.term(0)?;
    Ok((t, r.pos))
}

struct Reader<'a, 'b> {
    bytes: &'a [u8],
    pos: usize,
    atoms: &'b mut AtomTable,
    safe: bool,
}

impl<'a> Reader<'a, '_> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], EtfError> {
        let end = self.pos.checked_add(n).ok_or(EtfError::Truncated)?;
        let s = self.bytes.get(self.pos..end).ok_or(EtfError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, EtfError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<usize, EtfError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]) as usize)
    }

    fn u32(&mut self) -> Result<usize, EtfError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Reject a count of items that could not possibly fit in the remaining input, before
    /// allocating anything for it. Every item takes at least `min_bytes`.
    fn check_count(&self, count: usize, min_bytes: usize) -> Result<(), EtfError> {
        if count.saturating_mul(min_bytes) > self.remaining() {
            Err(EtfError::Truncated)
        } else {
            Ok(())
        }
    }

    fn term(&mut self, depth: usize) -> Result<Term, EtfError> {
        if depth > MAX_DEPTH {
            return Err(EtfError::TooDeep);
        }
        let tag = self.u8()?;
        Ok(match tag {
            97 => Term::Int(self.u8()? as i64),
            98 => {
                let b = self.take(4)?;
                Term::Int(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64)
            }
            70 => {
                let b = self.take(8)?;
                let f = f64::from_be_bytes(b.try_into().expect("8 bytes"));
                if !f.is_finite() {
                    return Err(EtfError::BadFloat);
                }
                Term::Float(f)
            }
            110 | 111 => {
                let n = if tag == 110 { self.u8()? as usize } else { self.u32()? };
                // Any non-zero sign byte means negative, as BEAM reads it.
                let sign = if self.u8()? == 0 { Sign::Plus } else { Sign::Minus };
                let digits = self.take(n)?;
                Term::big(BigInt::from_bytes_le(sign, digits))
            }
            118 | 119 | 100 | 115 => {
                let n = if tag == 119 || tag == 115 { self.u8()? as usize } else { self.u16()? };
                let raw = self.take(n)?;
                let text: alloc::string::String = if tag == 118 || tag == 119 {
                    core::str::from_utf8(raw).map_err(|_| EtfError::BadAtom)?.into()
                } else {
                    // Latin-1: each byte is one code point.
                    raw.iter().map(|&b| b as char).collect()
                };
                let atom = if self.safe {
                    self.atoms.existing(&text).ok_or(EtfError::BadAtom)?
                } else {
                    self.atoms.intern(&text)?
                };
                Term::Atom(atom)
            }
            104 | 105 => {
                let n = if tag == 104 { self.u8()? as usize } else { self.u32()? };
                self.check_count(n, 1)?;
                let mut elems = Vec::with_capacity(n);
                for _ in 0..n {
                    elems.push(self.term(depth + 1)?);
                }
                Term::tuple(elems)
            }
            106 => Term::Nil,
            107 => {
                let n = self.u16()?;
                let bytes = self.take(n)?;
                Term::list(bytes.iter().map(|&b| Term::Int(b as i64)).collect::<Vec<_>>())
            }
            108 => {
                let n = self.u32()?;
                self.check_count(n, 1)?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.term(depth + 1)?);
                }
                let tail = self.term(depth + 1)?;
                Term::list_with_tail(items, tail)
            }
            109 => {
                let n = self.u32()?;
                Term::binary(self.take(n)?)
            }
            77 => {
                let n = self.u32()?;
                let last_bits = self.u8()? as usize;
                if n == 0 || !(1..=8).contains(&last_bits) {
                    return Err(EtfError::Malformed);
                }
                let data = self.take(n)?;
                Term::bits(Bits { data: Rc::new(data.to_vec()), offset: 0, len: (n - 1) * 8 + last_bits })
            }
            116 => {
                let n = self.u32()?;
                self.check_count(n, 2)?;
                let mut map = Map::new();
                for _ in 0..n {
                    let k = self.term(depth + 1)?;
                    let v = self.term(depth + 1)?;
                    map.insert(MapKey(k), v);
                }
                Term::map(map)
            }
            113 => {
                let module = self.atom(depth)?;
                let function = self.atom(depth)?;
                let arity = match self.term(depth + 1)? {
                    Term::Int(a) if (0..=255).contains(&a) => a as u32,
                    _ => return Err(EtfError::Malformed),
                };
                Term::Fun(Rc::new(Fun::Export { module, function, arity }))
            }
            // Pids, ports, references, local funs, the old float format, compressed terms and
            // distribution headers are not accepted yet.
            other => return Err(EtfError::BadTag(other)),
        })
    }

    fn atom(&mut self, depth: usize) -> Result<crate::atom::Atom, EtfError> {
        match self.term(depth + 1)? {
            Term::Atom(a) => Ok(a),
            _ => Err(EtfError::Malformed),
        }
    }
}

// ---- encoding ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// Local funs, match contexts: things this VM cannot (yet) serialize.
    Unsupported,
}

/// The node name this VM reports; pids and references are encoded with it.
pub const NODE: &str = "nonode@nohost";

pub fn encode(t: &Term) -> Result<Vec<u8>, EncodeError> {
    let mut out = alloc::vec![VERSION];
    let mut work = alloc::vec![t.clone()];
    while let Some(t) = work.pop() {
        match &t {
            Term::Int(i) => encode_int(&mut out, *i),
            Term::Big(b) => encode_big(&mut out, b),
            Term::Float(f) => {
                out.push(70);
                out.extend_from_slice(&f.to_be_bytes());
            }
            Term::Atom(a) => encode_atom(&mut out, a.as_str()),
            Term::Nil => out.push(106),
            Term::Cons(_) => {
                // A proper list of bytes up to 65535 long is a STRING_EXT, as in BEAM.
                let mut items = Vec::new();
                let mut tail = Term::Nil;
                for item in t.list_iter() {
                    match item {
                        Ok(x) => items.push(x),
                        Err(x) => tail = x,
                    }
                }
                let bytes: Option<Vec<u8>> = items
                    .iter()
                    .map(|x| x.as_i64().and_then(|i| u8::try_from(i).ok()))
                    .collect();
                match bytes {
                    Some(b) if matches!(tail, Term::Nil) && b.len() <= 0xffff => {
                        out.push(107);
                        out.extend_from_slice(&(b.len() as u16).to_be_bytes());
                        out.extend_from_slice(&b);
                    }
                    _ => {
                        out.push(108);
                        out.extend_from_slice(&(items.len() as u32).to_be_bytes());
                        work.push(tail);
                        work.extend(items.into_iter().rev());
                    }
                }
            }
            Term::Tuple(elems) => {
                if elems.len() <= 255 {
                    out.extend_from_slice(&[104, elems.len() as u8]);
                } else {
                    out.push(105);
                    out.extend_from_slice(&(elems.len() as u32).to_be_bytes());
                }
                work.extend(elems.iter().rev().cloned());
            }
            Term::Map(m) => {
                out.push(116);
                out.extend_from_slice(&(m.len() as u32).to_be_bytes());
                for (k, v) in m.iter().rev() {
                    work.push(v.clone());
                    work.push(k.0.clone());
                }
            }
            Term::Bits(b) => {
                let bytes = b.to_bytes();
                if b.is_binary() {
                    out.push(109);
                    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                } else {
                    out.push(77);
                    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                    out.push((b.len % 8) as u8);
                }
                out.extend_from_slice(&bytes);
            }
            Term::Fun(f) => match &**f {
                Fun::Export { module, function, arity } => {
                    out.push(113);
                    encode_atom(&mut out, module.as_str());
                    encode_atom(&mut out, function.as_str());
                    encode_int(&mut out, *arity as i64);
                }
                Fun::Local { .. } => return Err(EncodeError::Unsupported),
            },
            Term::Pid(p) => {
                out.push(88);
                encode_atom(&mut out, NODE);
                out.extend_from_slice(&p.index.to_be_bytes());
                out.extend_from_slice(&p.serial.to_be_bytes());
                out.extend_from_slice(&0u32.to_be_bytes());
            }
            Term::Ref(r) => {
                out.push(90);
                out.extend_from_slice(&3u16.to_be_bytes());
                encode_atom(&mut out, NODE);
                out.extend_from_slice(&0u32.to_be_bytes());
                out.extend_from_slice(&((r.0 & 0x3ffff) as u32).to_be_bytes());
                out.extend_from_slice(&((r.0 >> 18) as u32).to_be_bytes());
                out.extend_from_slice(&((r.0 >> 50) as u32).to_be_bytes());
            }
            Term::Match(_) => return Err(EncodeError::Unsupported),
        }
    }
    Ok(out)
}

fn encode_int(out: &mut Vec<u8>, i: i64) {
    if (0..=255).contains(&i) {
        out.extend_from_slice(&[97, i as u8]);
    } else if let Ok(i) = i32::try_from(i) {
        out.push(98);
        out.extend_from_slice(&i.to_be_bytes());
    } else {
        encode_big(out, &BigInt::from(i));
    }
}

fn encode_big(out: &mut Vec<u8>, b: &BigInt) {
    let (sign, digits) = b.to_bytes_le();
    if digits.len() <= 255 {
        out.extend_from_slice(&[110, digits.len() as u8]);
    } else {
        out.push(111);
        out.extend_from_slice(&(digits.len() as u32).to_be_bytes());
    }
    out.push(if sign == Sign::Minus { 1 } else { 0 });
    out.extend_from_slice(&digits);
}

fn encode_atom(out: &mut Vec<u8>, a: &str) {
    if a.len() <= 255 {
        out.extend_from_slice(&[119, a.len() as u8]);
    } else {
        out.push(118);
        out.extend_from_slice(&(a.len() as u16).to_be_bytes());
    }
    out.extend_from_slice(a.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    fn dec(bytes: &[u8]) -> Result<Term, EtfError> {
        decode(bytes, &mut AtomTable::new())
    }

    /// Byte strings are `term_to_binary/1` output from OTP 28.
    #[test]
    fn decodes_otp_output() {
        let cases: &[(&[u8], &str)] = &[
            (&[131, 97, 42], "42"),
            (&[131, 98, 255, 255, 255, 255], "-1"),
            (&[131, 110, 8, 0, 0, 0, 0, 0, 0, 0, 0, 128], "9223372036854775808"),
            (&[131, 70, 63, 248, 0, 0, 0, 0, 0, 0], "1.5"),
            (&[131, 119, 2, 111, 107], "ok"),
            (&[131, 106], "[]"),
            (&[131, 107, 0, 2, 104, 105], "[104,105]"),
            (&[131, 108, 0, 0, 0, 1, 97, 1, 97, 2], "[1|2]"),
            (&[131, 104, 2, 97, 1, 119, 1, 97], "{1,a}"),
            (&[131, 109, 0, 0, 0, 2, 1, 2], "<<1,2>>"),
            (&[131, 77, 0, 0, 0, 1, 3, 160], "<<5:3>>"),
            (&[131, 116, 0, 0, 0, 1, 119, 1, 107, 97, 1], "#{k => 1}"),
            (&[131, 113, 119, 5, 108, 105, 115, 116, 115, 119, 3, 109, 97, 112, 97, 2], "fun lists:map/2"),
        ];
        for (bytes, want) in cases {
            assert_eq!(dec(bytes).unwrap().to_string(), *want, "decoding {bytes:?}");
        }
    }

    /// Encoding reproduces OTP's bytes for every decodable test vector.
    #[test]
    fn encodes_like_otp() {
        let cases: &[&[u8]] = &[
            &[131, 97, 42],
            &[131, 98, 255, 255, 255, 255],
            &[131, 110, 8, 0, 0, 0, 0, 0, 0, 0, 0, 128],
            &[131, 70, 63, 248, 0, 0, 0, 0, 0, 0],
            &[131, 119, 2, 111, 107],
            &[131, 106],
            &[131, 107, 0, 2, 104, 105],
            &[131, 108, 0, 0, 0, 1, 97, 1, 97, 2],
            &[131, 104, 2, 97, 1, 119, 1, 97],
            &[131, 109, 0, 0, 0, 2, 1, 2],
            &[131, 77, 0, 0, 0, 1, 3, 160],
            &[131, 116, 0, 0, 0, 1, 119, 1, 107, 97, 1],
            &[131, 113, 119, 5, 108, 105, 115, 116, 115, 119, 3, 109, 97, 112, 97, 2],
        ];
        for bytes in cases {
            let t = dec(bytes).unwrap();
            assert_eq!(encode(&t).unwrap(), *bytes, "re-encoding {t}");
        }
    }

    #[test]
    fn safe_mode_creates_no_atoms() {
        let mut atoms = AtomTable::new();
        let bytes = [131, 119, 3, 110, 101, 119];
        assert_eq!(decode_prefix(&bytes, &mut atoms, true).err(), Some(EtfError::BadAtom));
        assert!(atoms.existing("new").is_none());
        assert!(decode_prefix(&bytes, &mut atoms, false).is_ok());
        assert!(decode_prefix(&bytes, &mut atoms, true).is_ok());
    }

    #[test]
    fn rejects_hostile_input() {
        // No version byte, unknown tag, truncation, trailing bytes.
        assert_eq!(dec(&[97, 1]).err(), Some(EtfError::BadTag(97)));
        assert_eq!(dec(&[131, 200]).err(), Some(EtfError::BadTag(200)));
        assert_eq!(dec(&[131, 98, 0, 0]).err(), Some(EtfError::Truncated));
        assert_eq!(dec(&[131, 97, 1, 0]).err(), Some(EtfError::TrailingBytes));
        // A list claiming 4 billion elements must fail before allocating.
        assert_eq!(dec(&[131, 108, 255, 255, 255, 255, 106]).err(), Some(EtfError::Truncated));
        // NaN is not an Erlang float.
        assert_eq!(dec(&[131, 70, 127, 248, 0, 0, 0, 0, 0, 0]).err(), Some(EtfError::BadFloat));
        // Invalid UTF-8 in an atom.
        assert_eq!(dec(&[131, 119, 1, 0xff]).err(), Some(EtfError::BadAtom));
        // Bit binary with 0 or 9 bits in the last byte.
        assert_eq!(dec(&[131, 77, 0, 0, 0, 1, 0, 0]).err(), Some(EtfError::Malformed));
        assert_eq!(dec(&[131, 77, 0, 0, 0, 1, 9, 0]).err(), Some(EtfError::Malformed));
    }

    #[test]
    fn nesting_is_bounded() {
        let mut ok = alloc::vec![131];
        for _ in 0..MAX_DEPTH {
            ok.extend([104, 1]);
        }
        ok.push(106);
        assert!(dec(&ok).is_ok());
        let mut deep = alloc::vec![131];
        for _ in 0..100_000 {
            deep.extend([104, 1]);
        }
        deep.push(106);
        assert_eq!(dec(&deep).err(), Some(EtfError::TooDeep));
    }

    #[test]
    fn long_lists_do_not_recurse() {
        // A million-element string decodes and drops without exhausting the stack.
        let mut b = alloc::vec![131, 108, 0, 15, 66, 64];
        for _ in 0..1_000_000 {
            b.extend([97, 7]);
        }
        b.push(106);
        let t = dec(&b).unwrap();
        assert_eq!(t.list_iter().count(), 1_000_000);
        drop(t);
        let _ = format!("{}", 1);
    }
}
