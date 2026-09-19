//! Building and reading bitstrings, for `<<...>>` construction and binary pattern matching.

use alloc::rc::Rc;
use alloc::vec::Vec;

use num_bigint::{BigInt, Sign};
use num_traits::float::FloatCore;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::term::{Bits, Term};

/// Accumulates bits, most significant first.
pub struct Builder {
    bytes: Vec<u8>,
    len: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        Builder { bytes: Vec::new(), len: 0 }
    }

    pub fn bit_len(&self) -> usize {
        self.len
    }

    pub fn push_bit(&mut self, bit: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit {
            let last = self.bytes.len() - 1;
            self.bytes[last] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    pub fn push_byte(&mut self, b: u8) {
        self.push_bytes(&[b]);
    }

    pub fn push_bytes(&mut self, bs: &[u8]) {
        let r = self.len % 8;
        if r == 0 {
            self.bytes.extend_from_slice(bs);
        } else {
            // Not byte-aligned: each byte straddles the last partial byte and a new one.
            self.bytes.reserve(bs.len());
            for &b in bs {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= b >> r;
                self.bytes.push(b << (8 - r));
            }
        }
        self.len += bs.len() * 8;
    }

    /// `n` copies of byte `b`.
    pub fn push_repeated(&mut self, b: u8, n: usize) {
        if self.len.is_multiple_of(8) {
            self.bytes.resize(self.bytes.len() + n, b);
            self.len += n * 8;
        } else {
            for _ in 0..n {
                self.push_byte(b);
            }
        }
    }

    pub fn push_bits(&mut self, b: &Bits) {
        self.push_bits_prefix(b, b.len);
    }

    /// The first `n` bits of `b` (caller checks `n <= b.len`).
    pub fn push_bits_prefix(&mut self, b: &Bits, n: usize) {
        let whole = n / 8;
        if self.len.is_multiple_of(8) && b.offset.is_multiple_of(8) {
            let start = b.offset / 8;
            self.push_bytes(&b.data[start..start + whole]);
        } else {
            for i in 0..whole {
                self.push_byte(b.byte(i));
            }
        }
        for i in whole * 8..n {
            self.push_bit(b.bit(i));
        }
    }

    /// `value` as a `size`-bit two's-complement integer, truncated to its low bits.
    pub fn push_integer(&mut self, value: &BigInt, size: usize, little: bool) {
        let nbytes = size.div_ceil(8);
        let mut le = value.to_signed_bytes_le();
        let fill = if value.is_negative() { 0xff } else { 0 };
        if !little && nbytes > le.len() + 1 {
            // A wide segment is mostly sign padding: write the padding in bulk, then the value
            // in the remaining bits. (`<<1:4000000>>` is legal and must not take a bit loop.)
            let value_bytes = le.len() + 1;
            let pad_bits = size - value_bytes * 8;
            for _ in 0..pad_bits % 8 {
                self.push_bit(fill != 0);
            }
            self.push_repeated(fill, pad_bits / 8);
            le.resize(value_bytes, fill);
            le.reverse();
            self.push_bytes(&le);
            return;
        }
        le.resize(nbytes.max(le.len()), fill);
        le.truncate(nbytes);
        if little {
            // Little-endian: whole bytes least significant first; a partial last unit holds the
            // top bits (as BEAM does for sizes that are not a multiple of 8).
            let rem = size % 8;
            for (i, &b) in le.iter().enumerate() {
                if i + 1 == nbytes && rem != 0 {
                    for k in (0..rem).rev() {
                        self.push_bit(b & (1 << k) != 0);
                    }
                } else {
                    self.push_byte(b);
                }
            }
        } else {
            // Big-endian: the top byte holds the leftover `size % 8` bits, then whole bytes.
            let top = size % 8;
            let mut rest = nbytes;
            if top != 0 {
                let b = le[nbytes - 1];
                for k in (0..top).rev() {
                    self.push_bit(b & (1 << k) != 0);
                }
                rest -= 1;
            }
            let be: Vec<u8> = le[..rest].iter().rev().copied().collect();
            self.push_bytes(&be);
        }
    }

    pub fn push_small(&mut self, value: i64, size: usize, little: bool) {
        self.push_integer(&BigInt::from(value), size, little)
    }

    pub fn finish(self) -> Term {
        Term::Bits(Bits { data: Rc::from(self.bytes), offset: 0, len: self.len })
    }
}

/// Read `size` bits at `pos` of `b` as an integer.
pub fn read_integer(b: &Bits, pos: usize, size: usize, signed: bool, little: bool) -> Term {
    if size == 0 {
        return Term::Int(0);
    }
    // Gather the bits into big-endian bytes, right-aligned.
    let nbytes = size.div_ceil(8);
    let mut be = alloc::vec![0u8; nbytes];
    let pad = nbytes * 8 - size;
    for i in 0..size {
        if b.bit(pos + i) {
            let k = pad + i;
            be[k / 8] |= 0x80 >> (k % 8);
        }
    }
    if little {
        // Undo the little-endian layout: the bytes as read are least significant first, with a
        // partial final unit holding the top bits.
        let rem = size % 8;
        let mut v = BigInt::zero();
        let mut shift = 0usize;
        for i in 0..size / 8 {
            let mut byte = 0u8;
            for k in 0..8 {
                byte = (byte << 1) | b.bit(pos + i * 8 + k) as u8;
            }
            v += BigInt::from(byte) << shift;
            shift += 8;
        }
        if rem != 0 {
            let mut top = 0u8;
            for k in 0..rem {
                top = (top << 1) | b.bit(pos + (size / 8) * 8 + k) as u8;
            }
            v += BigInt::from(top) << shift;
        }
        return finish_int(v, size, signed);
    }
    let v = BigInt::from_bytes_be(Sign::Plus, &be);
    finish_int(v, size, signed)
}

fn finish_int(v: BigInt, size: usize, signed: bool) -> Term {
    if signed && size > 0 && (&v >> (size - 1)) & BigInt::from(1) == BigInt::from(1) {
        return Term::big(v - (BigInt::from(1) << size));
    }
    match v.to_i64() {
        Some(i) => Term::Int(i),
        None => Term::big(v),
    }
}

/// Encode `f` in `size` bits (16, 32 or 64).
pub fn push_float(out: &mut Builder, f: f64, size: usize, little: bool) -> bool {
    let bytes: Vec<u8> = match size {
        64 => f.to_bits().to_be_bytes().to_vec(),
        32 => {
            let g = f as f32;
            if g.is_infinite() {
                return false;
            }
            g.to_bits().to_be_bytes().to_vec()
        }
        16 => match f16_bits(f) {
            Some(h) => h.to_be_bytes().to_vec(),
            None => return false,
        },
        _ => return false,
    };
    if little {
        for b in bytes.iter().rev() {
            out.push_byte(*b);
        }
    } else {
        out.push_bytes(&bytes);
    }
    true
}

/// Read a `size`-bit float at `pos`; `None` for a size other than 16/32/64 or a non-finite value.
pub fn read_float(b: &Bits, pos: usize, size: usize, little: bool) -> Option<f64> {
    let mut bytes: Vec<u8> = (0..size / 8)
        .map(|i| {
            let mut byte = 0u8;
            for k in 0..8 {
                byte = (byte << 1) | b.bit(pos + i * 8 + k) as u8;
            }
            byte
        })
        .collect();
    if little {
        bytes.reverse();
    }
    let f = match size {
        64 => f64::from_bits(u64::from_be_bytes(bytes.try_into().ok()?)),
        32 => f32::from_bits(u32::from_be_bytes(bytes.try_into().ok()?)) as f64,
        16 => f16_to_f64(u16::from_be_bytes(bytes.try_into().ok()?)),
        _ => return None,
    };
    f.is_finite().then_some(f)
}

/// IEEE 754 half precision, round to nearest even. `None` if it overflows.
fn f16_bits(f: f64) -> Option<u16> {
    let x = f as f32; // f64 -> f32 rounds once; f32 -> f16 below rounds again (as BEAM does)
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x7f_ffff;
    if exp == 0xff {
        return None;
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return None;
    }
    if e <= 0 {
        if e < -10 {
            return Some(sign);
        }
        let m = (man | 0x80_0000) >> (1 - e);
        let round = (m & 0x1fff) > 0x1000 || ((m & 0x1fff) == 0x1000 && (m & 0x2000) != 0);
        return Some(sign | ((m >> 13) as u16 + round as u16));
    }
    let round = (man & 0x1fff) > 0x1000 || ((man & 0x1fff) == 0x1000 && (man & 0x2000) != 0);
    let h = ((e as u32) << 10) | (man >> 13);
    let h = h + round as u32;
    if h >= 0x7c00 {
        return None;
    }
    Some(sign | h as u16)
}

fn f16_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let man = (h & 0x3ff) as f64;
    match exp {
        0 => sign * man * FloatCore::powi(2f64, -24),
        0x1f => f64::NAN,
        e => sign * (1.0 + man / 1024.0) * FloatCore::powi(2f64, e - 15),
    }
}

/// Append `cp` as UTF-8. `false` if it is not a Unicode scalar value.
pub fn push_utf8(out: &mut Builder, cp: i64) -> bool {
    match u32::try_from(cp).ok().and_then(char::from_u32) {
        Some(ch) => {
            let mut buf = [0u8; 4];
            out.push_bytes(ch.encode_utf8(&mut buf).as_bytes());
            true
        }
        None => false,
    }
}

pub fn push_utf16(out: &mut Builder, cp: i64, little: bool) -> bool {
    match u32::try_from(cp).ok().and_then(char::from_u32) {
        Some(ch) => {
            let mut buf = [0u16; 2];
            for unit in ch.encode_utf16(&mut buf) {
                out.push_small(*unit as i64, 16, little);
            }
            true
        }
        None => false,
    }
}

pub fn push_utf32(out: &mut Builder, cp: i64, little: bool) -> bool {
    match u32::try_from(cp).ok().and_then(char::from_u32) {
        Some(ch) => {
            out.push_small(ch as i64, 32, little);
            true
        }
        None => false,
    }
}

/// Decode one UTF-8 character at bit `pos`: `(code point, bits used)`.
pub fn read_utf8(b: &Bits, pos: usize) -> Option<(u32, usize)> {
    let avail = (b.len - pos) / 8;
    let byte = |i: usize| -> u8 {
        let mut v = 0u8;
        for k in 0..8 {
            v = (v << 1) | b.bit(pos + i * 8 + k) as u8;
        }
        v
    };
    if avail == 0 {
        return None;
    }
    let first = byte(0);
    let n = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    if avail < n {
        return None;
    }
    let bytes: Vec<u8> = (0..n).map(byte).collect();
    let s = core::str::from_utf8(&bytes).ok()?;
    let ch = s.chars().next()?;
    Some((ch as u32, n * 8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn build(f: impl FnOnce(&mut Builder)) -> alloc::string::String {
        let mut b = Builder::new();
        f(&mut b);
        b.finish().to_string()
    }

    /// Expected strings are what OTP 28 prints for the same construction.
    #[test]
    fn integers() {
        assert_eq!(build(|b| b.push_small(1, 8, false)), "<<1>>");
        assert_eq!(build(|b| b.push_small(258, 16, false)), "<<1,2>>");
        assert_eq!(build(|b| b.push_small(258, 16, true)), "<<2,1>>");
        assert_eq!(build(|b| b.push_small(-1, 12, false)), "<<255,15:4>>");
        assert_eq!(build(|b| b.push_small(5, 3, false)), "<<5:3>>");
        assert_eq!(build(|b| b.push_small(0x1234, 12, true)), "<<52,2:4>>");
        assert_eq!(build(|b| b.push_small(300, 8, false)), "<<44>>");
    }

    #[test]
    fn integers_round_trip() {
        for (v, size, signed, little) in [
            (5i64, 3, false, false),
            (-3, 5, true, false),
            (0x1234, 16, false, true),
            (0x234, 12, false, true),
            (-2, 12, true, true),
            (i64::MAX, 64, true, false),
            (-1, 64, true, true),
        ] {
            let mut b = Builder::new();
            b.push_small(v, size, little);
            let Term::Bits(bits) = b.finish() else { panic!() };
            assert_eq!(read_integer(&bits, 0, size, signed, little).as_i64(), Some(v), "{v} {size} {signed} {little}");
        }
    }

    #[test]
    fn floats() {
        let mut b = Builder::new();
        assert!(push_float(&mut b, 1.5, 64, false));
        assert_eq!(b.finish().to_string(), "<<63,248,0,0,0,0,0,0>>");
        let mut b = Builder::new();
        assert!(push_float(&mut b, 1.5, 16, false));
        assert_eq!(b.finish().to_string(), "<<62,0>>");
        let mut b = Builder::new();
        assert!(!push_float(&mut b, 1.0e300, 32, false));
    }
}
