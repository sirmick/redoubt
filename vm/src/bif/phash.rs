//! `erlang:phash2/1,2`: BEAM's portable term hash (`make_hash2` in `erl_term_hashing.c`),
//! bit for bit, so hashes agree with BEAM's for every term that means the same on both
//! (everything but pids, references and ports, whose numbers differ between VMs).
//!
//! The walk order matters, since each value is mixed into the running hash: this follows
//! BEAM's explicit-stack traversal exactly, including its shortcut for lists of bytes and its
//! order-independent treatment of map pairs.

use num_bigint::Sign;

use super::Ctx;
use crate::atom::Atom;
use crate::process::Exception;
use crate::term::{Fun, Term};

type R = Result<Term, Exception>;

const HCONST: u32 = 0x9e37_79b9;
const HCONST_2: u32 = 0x3c6e_f372;
const HCONST_3: u32 = 0xdaa6_6d2b;
const HCONST_4: u32 = 0x78dd_e6e4;
const HCONST_5: u32 = 0x1715_609d;
const HCONST_7: u32 = 0x5384_540f;
const HCONST_9: u32 = 0x8ff3_4781;
const HCONST_10: u32 = 0x2e2a_c13a;
const HCONST_11: u32 = 0xcc62_3af3;
const HCONST_12: u32 = 0x6a99_b4ac;
const HCONST_13: u32 = 0x08d1_2e65;
const HCONST_14: u32 = 0xa708_a81e;
const HCONST_15: u32 = 0x4540_21d7;
const HCONST_16: u32 = 0xe377_9b90;
const HCONST_19: u32 = 0xbe1e_08bb;

/// BEAM's `NIL_DEF` (mixed in for a `[]` that follows something), and the hash of a lone `[]`.
const NIL_DEF: u32 = 0x02;
const NIL_HASH: u32 = 3_468_870_702;

/// Bob Jenkins' mix, as BEAM's `MIX` macro.
fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
}

/// `UINT32_HASH_2`: mix two words into `hash`.
fn hash2(hash: &mut u32, x: u32, y: u32, k: u32) {
    let (mut a, mut b) = (k.wrapping_add(x), k.wrapping_add(y));
    mix(&mut a, &mut b, hash);
}

fn hash1(hash: &mut u32, x: u32, k: u32) {
    hash2(hash, x, 0, k);
}

/// BEAM's `block_hash`: Jenkins' lookup2 over bytes, seeded with `init`.
fn block_hash(bytes: &[u8], init: u32) -> u32 {
    let word = |k: &[u8]| u32::from_le_bytes([k[0], k[1], k[2], k[3]]);
    let (mut a, mut b, mut c) = (HCONST, HCONST, init);
    let mut chunks = bytes.chunks_exact(12);
    for k in &mut chunks {
        a = a.wrapping_add(word(&k[0..4]));
        b = b.wrapping_add(word(&k[4..8]));
        c = c.wrapping_add(word(&k[8..12]));
        mix(&mut a, &mut b, &mut c);
    }
    let k = chunks.remainder();
    c = c.wrapping_add(bytes.len() as u32);
    // The first byte of `c` is taken by the length, so its bytes start at the second.
    for (i, &byte) in k.iter().enumerate() {
        let v = byte as u32;
        match i {
            0..=3 => a = a.wrapping_add(v << (8 * i)),
            4..=7 => b = b.wrapping_add(v << (8 * (i - 4))),
            _ => c = c.wrapping_add(v << (8 * (i - 7))),
        }
    }
    mix(&mut a, &mut b, &mut c);
    c
}

/// An atom's hash as BEAM's atom table stores it: `hashpjw` over the UTF-8 name, reading a
/// two-byte sequence for a Latin-1 character as that character (compatibility with R16).
pub fn atom_hash(a: &Atom) -> u32 {
    let bytes = a.as_str().as_bytes();
    let mut h: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let mut v = bytes[i];
        i += 1;
        if i < bytes.len() && (v & 0xfe) == 0xc2 && (bytes[i] & 0xc0) == 0x80 {
            v = (v << 6) | (bytes[i] & 0x3f);
            i += 1;
        }
        h = (h << 4).wrapping_add(v as u32);
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}

/// An integer that does not fit in 28 bits: its magnitude as 64-bit digits, in two halves each.
fn hash_digits(hash: &mut u32, negative: bool, digits: impl Iterator<Item = u64>) {
    let k = if negative { HCONST_10 } else { HCONST_11 };
    for d in digits {
        hash2(hash, d as u32, (d >> 32) as u32, k);
    }
}

enum Work<'a> {
    Term(&'a Term),
    /// End of a map: restore the enclosing hash and pair accumulator, mixing in this map's pairs.
    MapTail(u32, u32),
    /// End of one key-value pair of a map.
    MapPair,
}

/// BEAM's `make_hash2`.
pub fn make_hash2(t: &Term) -> u32 {
    let mut hash: u32 = 0;
    let mut xor_pairs: u32 = 0;
    let mut work = alloc::vec![Work::Term(t)];
    while let Some(w) = work.pop() {
        let t = match w {
            Work::Term(t) => t,
            Work::MapTail(h, x) => {
                let pairs = xor_pairs;
                hash = h;
                hash1(&mut hash, pairs, HCONST_19);
                xor_pairs = x;
                continue;
            }
            Work::MapPair => {
                xor_pairs ^= hash;
                hash = 0;
                continue;
            }
        };
        match t {
            Term::Atom(a) => {
                if hash == 0 {
                    hash = atom_hash(a);
                } else {
                    hash1(&mut hash, atom_hash(a), HCONST_3);
                }
            }
            Term::Nil => {
                if hash == 0 {
                    hash = NIL_HASH;
                } else {
                    hash1(&mut hash, NIL_DEF, HCONST_2);
                }
            }
            Term::Int(n) => {
                if (-(1 << 27)..(1 << 27)).contains(n) {
                    let y = *n as i32;
                    if y < 0 {
                        // Negative numbers are mixed twice, as in BEAM.
                        hash1(&mut hash, y.wrapping_neg() as u32, HCONST);
                    }
                    hash1(&mut hash, y as u32, HCONST);
                } else {
                    hash_digits(&mut hash, *n < 0, core::iter::once(n.unsigned_abs()));
                }
            }
            Term::Big(b) => hash_digits(&mut hash, b.sign() == Sign::Minus, b.magnitude().iter_u64_digits()),
            Term::Float(f) => {
                // -0.0 hashes as 0.0.
                let bits = if *f == 0.0 { 0 } else { f.to_bits() };
                hash2(&mut hash, (bits >> 32) as u32, bits as u32, HCONST_12);
            }
            Term::Cons(cell) => {
                // Bytes at the head of a list are packed four to a word.
                let (mut c, mut sh) = (0u32, 0u32);
                let mut cur = cell;
                let rest = loop {
                    match cur.head {
                        Term::Int(b @ 0..=255) => {
                            sh = (sh << 8).wrapping_add(b as u32);
                            if c == 3 {
                                hash1(&mut hash, sh, HCONST_4);
                                c = 0;
                                sh = 0;
                            } else {
                                c += 1;
                            }
                            match &cur.tail {
                                Term::Cons(next) => cur = next,
                                tail => break alloc::vec![tail],
                            }
                        }
                        // A head that is not a byte: the tail waits while it is hashed.
                        _ => break alloc::vec![&cur.tail, &cur.head],
                    }
                };
                if c > 0 {
                    hash1(&mut hash, sh, HCONST_4);
                }
                work.extend(rest.into_iter().map(Work::Term));
            }
            Term::Tuple(e) => {
                hash1(&mut hash, e.len() as u32, HCONST_9);
                work.extend(e.iter().rev().map(Work::Term));
            }
            Term::Map(m) => {
                hash1(&mut hash, m.len() as u32, HCONST_16);
                if !m.is_empty() {
                    work.push(Work::MapTail(hash, xor_pairs));
                    hash = 0;
                    xor_pairs = 0;
                    for (k, v) in m.iter() {
                        work.push(Work::MapPair);
                        work.push(Work::Term(v));
                        work.push(Work::Term(&k.0));
                    }
                }
            }
            Term::Bits(b) => {
                let k = HCONST_13.wrapping_add(hash);
                let whole = b.len / 8;
                let rest_bits = (b.len % 8) as u32;
                if b.len == 0 {
                    hash = k;
                } else {
                    let bytes = b.to_bytes();
                    // Sizes are truncated to 32 bits, as BEAM does for compatibility.
                    hash = block_hash(&bytes[..whole], k);
                    if rest_bits > 0 {
                        hash2(&mut hash, rest_bits, (bytes[whole] >> (8 - rest_bits)) as u32, HCONST_15);
                    }
                }
            }
            Term::Fun(f) => match &**f {
                Fun::Export { module, function, arity } => {
                    hash2(&mut hash, *arity, atom_hash(module), HCONST);
                    hash1(&mut hash, atom_hash(function), HCONST_14);
                }
                Fun::Local { module, index, env, uniq, .. } => {
                    hash2(&mut hash, env.len() as u32, atom_hash(module), HCONST);
                    hash2(&mut hash, *index, *uniq, HCONST);
                    work.extend(env.iter().rev().map(Work::Term));
                }
            },
            Term::Pid(p) => hash1(&mut hash, p.index, HCONST_5),
            Term::Ref(r) => hash1(&mut hash, r.0 as u32, HCONST_7),
            Term::Resource(r) => hash1(&mut hash, r.id as u32, HCONST_7),
            Term::Match(_) => {}
        }
    }
    hash
}

/// `phash2(Term)`: a hash in `0..2^27`.
pub fn phash2_1(_c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int((make_hash2(&a[0]) & ((1 << 27) - 1)) as i64))
}

/// `phash2(Term, Range)`: a hash in `0..Range`, for `Range` in `1..=2^32`.
pub fn phash2_2(c: &mut Ctx, a: &[Term]) -> R {
    let range = match a[1] {
        Term::Int(r @ 1..=0x1_0000_0000) => r as u64,
        _ => return Err(c.badarg()),
    };
    Ok(Term::Int((make_hash2(&a[0]) as u64 % range) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn block_hash_handles_every_tail_length() {
        // Only checks it is total and length-sensitive; exact values come from the difftest.
        let data: Vec<u8> = (0..40).collect();
        let hashes: Vec<u32> = (0..data.len()).map(|n| block_hash(&data[..n], 7)).collect();
        for w in hashes.windows(2) {
            assert_ne!(w[0], w[1]);
        }
    }
}
