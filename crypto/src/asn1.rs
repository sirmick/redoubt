//! The `asn1rt_nif` NIFs behind OTP's BER/DER codecs (used by `public_key` for certificates,
//! keys and PKCS structures): splitting bytes into `{Tag, Value}` TLVs and back.
//!
//! This parses untrusted input (certificates from the network), so nesting is bounded, every
//! length is checked against the bytes that remain, and nothing recurses on the Rust stack
//! beyond [`MAX_DEPTH`] levels.

use alloc::vec::Vec;

use beamlet_vm::bif::Ctx;
use beamlet_vm::Term;

use crate::R;

/// Deepest nesting of constructed values. Certificates nest about ten levels.
const MAX_DEPTH: usize = 64;

/// A decode error: its reason (as `make_ber_error_term` names it) and where it happened.
struct Error(&'static str, usize);

/// `decode_ber_tlv_raw(Bin)` → `{{Tag, Value}, Rest}` or `{error, {Reason, Position}}`.
/// `Tag` is the class in bits 16-17 and the tag number below; `Value` is a binary (primitive)
/// or a list of TLVs (constructed, including indefinite length).
pub fn decode_ber_tlv(c: &mut Ctx, a: &[Term]) -> R {
    let Some(input) = a[0].iodata_bytes() else { return Err(c.badarg()) };
    let mut pos = 0;
    match decode(&input, &mut pos, input.len(), 0) {
        Ok(t) => Ok(Term::tuple(alloc::vec![t, Term::binary(&input[pos..])])),
        Err(Error(reason, at)) => {
            let e = Term::tuple(alloc::vec![c.atom(reason), Term::Int(at as i64)]);
            Ok(Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), e]))
        }
    }
}

/// Decode one TLV starting at `*pos`, not reading past `end`.
fn decode(b: &[u8], pos: &mut usize, end: usize, depth: usize) -> Result<Term, Error> {
    if depth > MAX_DEPTH {
        return Err(Error("unknown", *pos));
    }
    if *pos + 2 > end {
        return Err(Error("invalid_value", *pos));
    }
    // Tag: class and form in the top three bits; numbers above 30 in up to two more bytes.
    let first = b[*pos];
    let constructed = first & 0x20 != 0;
    let mut tag = ((first & 0xc0) as u32) << 10;
    if first & 0x1f < 31 {
        tag |= (first & 0x1f) as u32;
        *pos += 1;
    } else {
        if *pos + 3 > end {
            return Err(Error("invalid_value", *pos));
        }
        *pos += 1;
        if b[*pos] >= 128 {
            tag |= ((b[*pos] & 0x7f) as u32) << 7;
            *pos += 1;
        }
        if b[*pos] >= 128 {
            return Err(Error("invalid_tag", *pos)); // tag numbers above 16K
        }
        tag |= b[*pos] as u32;
        *pos += 1;
    }
    if *pos >= end {
        return Err(Error("invalid_tag", *pos));
    }
    let tag = Term::Int(tag as i64);
    // Length.
    let l0 = b[*pos];
    if l0 == 0x80 {
        // Indefinite length: TLVs until two zero bytes. Only for constructed values.
        *pos += 1;
        if *pos + 1 >= end || !constructed {
            return Err(Error("invalid_length", *pos));
        }
        let mut items = Vec::new();
        while !(b[*pos] == 0 && b[*pos + 1] == 0) {
            items.push(decode(b, pos, end, depth + 1)?);
            if *pos + 1 >= end {
                return Err(Error("invalid_length", *pos));
            }
        }
        *pos += 2;
        return Ok(Term::tuple(alloc::vec![tag, Term::list(items)]));
    }
    let len = if l0 < 0x80 {
        l0 as usize
    } else {
        let n = (l0 & 0x7f) as usize;
        if n > end - (*pos + 1) || n > 4 {
            return Err(Error("invalid_length", *pos));
        }
        let mut len = 0usize;
        for _ in 0..n {
            *pos += 1;
            len = (len << 8) | b[*pos] as usize;
        }
        len
    };
    if len > end - (*pos + 1) {
        return Err(Error("invalid_value", *pos));
    }
    *pos += 1;
    let value_end = *pos + len;
    let value = if constructed {
        let mut items = Vec::new();
        while *pos < value_end {
            items.push(decode(b, pos, value_end, depth + 1)?);
        }
        Term::list(items)
    } else {
        let v = Term::binary(&b[*pos..value_end]);
        *pos = value_end;
        v
    };
    Ok(Term::tuple(alloc::vec![tag, value]))
}

/// `encode_ber_tlv({Tag, Value})` → the BER bytes, or `{error, Code}`. A binary value is
/// primitive, a list of TLVs constructed.
pub fn encode_ber_tlv(c: &mut Ctx, a: &[Term]) -> R {
    let mut out = Vec::new();
    if encode(&a[0], &mut out, 0).is_none() {
        return Ok(Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), Term::Int(-1)]));
    }
    Ok(Term::binary(&out))
}

fn encode(t: &Term, out: &mut Vec<u8>, depth: usize) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    let [tag, value] = t.as_tuple()? else { return None };
    let tag = u32::try_from(tag.as_i64()?).ok()?;
    let (constructed, content) = match value {
        Term::Bits(b) if b.is_binary() => (false, b.to_bytes().into_owned()),
        Term::Nil | Term::Cons(_) => {
            let mut inner = Vec::new();
            for item in value.list_iter() {
                encode(&item.ok()?, &mut inner, depth + 1)?;
            }
            (true, inner)
        }
        _ => return None,
    };
    // Tag.
    let head = ((tag & 0x30000) >> 10) as u8 | if constructed { 0x20 } else { 0 };
    let number = tag & 0xffff;
    if number <= 30 {
        out.push(head | number as u8);
    } else {
        out.push(head | 0x1f);
        let mut groups = alloc::vec![(number & 0x7f) as u8];
        let mut rest = number >> 7;
        while rest > 0 {
            groups.push(0x80 | (rest & 0x7f) as u8);
            rest >>= 7;
        }
        out.extend(groups.iter().rev());
    }
    // Length.
    let len = content.len();
    if len < 128 {
        out.push(len as u8);
    } else {
        let bytes: Vec<u8> = (len as u64).to_be_bytes().into_iter().skip_while(|b| *b == 0).collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend(content);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERTS: &[&[u8]] = &[include_bytes!("../tests/fixtures/rsa-root.der"), include_bytes!("../tests/fixtures/ec-root.der")];

    fn roundtrip(bytes: &[u8]) {
        let mut pos = 0;
        let t = decode(bytes, &mut pos, bytes.len(), 0).unwrap_or_else(|_| panic!("decodes"));
        assert_eq!(pos, bytes.len());
        let mut out = Vec::new();
        encode(&t, &mut out, 0).expect("encodes");
        assert_eq!(out, bytes, "DER re-encodes to the same bytes");
    }

    #[test]
    fn certificates_round_trip() {
        for c in CERTS {
            roundtrip(c);
        }
    }

    #[test]
    fn nesting_is_bounded() {
        // 10000 nested SEQUENCEs of indefinite length, then the end-of-contents markers.
        let mut deep = alloc::vec![0x30u8, 0x80].repeat(10_000);
        deep.extend(alloc::vec![0u8; 20_000]);
        let mut pos = 0;
        assert!(decode(&deep, &mut pos, deep.len(), 0).is_err());
    }

    /// Mutated certificates: decoding may fail but must never panic or read out of bounds.
    #[test]
    fn mutants_never_panic() {
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for round in 0..200_000 {
            let mut b = CERTS[round % CERTS.len()].to_vec();
            for _ in 0..1 + next() % 4 {
                let i = (next() as usize) % b.len();
                match next() % 3 {
                    0 => b[i] ^= 1 << (next() % 8),
                    1 => b[i] = next() as u8,
                    _ => b.truncate(i.max(1)),
                }
            }
            let mut pos = 0;
            let _ = decode(&b, &mut pos, b.len(), 0);
        }
    }
}
