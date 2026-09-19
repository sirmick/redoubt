//! Library information, the algorithm lists behind `crypto:supports/1`, and random numbers.

use alloc::vec::Vec;

use beamlet_vm::bif::Ctx;
use beamlet_vm::term::{Map, MapKey};
use beamlet_vm::Term;
use num_bigint::BigUint;
use num_traits::Zero;

use crate::{badarg, bin, bytes, random_bytes, string, R};

fn atoms(c: &mut Ctx, names: &[&str]) -> Term {
    Term::list(names.iter().map(|n| c.atom(n)).collect::<Vec<_>>())
}

/// `[{Name, VerNum, VerStr}]`, as for OpenSSL: here, this crate.
pub fn info_lib(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(alloc::vec![Term::tuple(alloc::vec![
        bin(b"beamlet-crypto (RustCrypto)"),
        Term::Int(0x0001_0000),
        bin(b"beamlet-crypto 0.1.0"),
    ])]))
}

pub fn info_nif(c: &mut Ctx, _a: &[Term]) -> R {
    let mut m = Map::new();
    m.insert(MapKey(c.atom("compile_type")), c.atom("normal"));
    m.insert(MapKey(c.atom("link_type")), c.atom("static"));
    m.insert(MapKey(c.atom("cryptolib_version_compiled")), string("beamlet-crypto 0.1.0"));
    m.insert(MapKey(c.atom("cryptolib_version_linked")), string("beamlet-crypto 0.1.0"));
    m.insert(MapKey(c.atom("fips_provider_available")), c.bool(false));
    Ok(Term::map(m))
}

pub fn info_fips(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.atom("not_supported"))
}

pub fn enable_fips_mode(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(false))
}

pub fn empty_list(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Nil)
}

pub fn hash_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    let names: Vec<&str> = crate::hash::ALGS.iter().map(|(n, ..)| *n).collect();
    Ok(atoms(c, &names))
}

pub fn pubkey_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(atoms(c, &["rsa", "ecdsa", "eddsa", "dh", "ecdh", "eddh"]))
}

pub fn cipher_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    let names: Vec<&str> = crate::cipher::names().collect();
    Ok(atoms(c, &names))
}

pub fn mac_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(atoms(c, &["hmac", "poly1305"]))
}

pub fn curve_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(atoms(c, &["secp256r1", "prime256v1", "secp384r1", "x25519", "ed25519"]))
}

pub fn rsa_opts_algorithms(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(atoms(c, &["rsa_pkcs1_pss_padding", "rsa_pss_saltlen", "rsa_mgf1_md", "rsa_pkcs1_padding"]))
}

/// `strong_rand_bytes_nif(N)`: `false` (which crypto.erl turns into `low_entropy`) if the
/// platform has no secure random source.
pub fn strong_rand_bytes(c: &mut Ctx, a: &[Term]) -> R {
    let Some(n) = a[0].as_usize().filter(|n| *n <= 1 << 24) else { return Err(badarg(c, 0, "Bad length")) };
    match random_bytes(c, n) {
        Ok(b) => Ok(bin(&b)),
        Err(_) => Ok(c.bool(false)),
    }
}

/// A uniform integer in `[0, range)`, by rejection sampling on the platform's random bytes.
fn uniform_below(c: &mut Ctx, range: &BigUint) -> Result<BigUint, beamlet_vm::Exception> {
    let bits = range.bits();
    let nbytes = bits.div_ceil(8) as usize;
    let excess = nbytes as u64 * 8 - bits;
    loop {
        let mut b = random_bytes(c, nbytes)?;
        if let Some(first) = b.first_mut() {
            *first &= 0xff >> excess;
        }
        let x = BigUint::from_bytes_be(&b);
        if &x < range {
            return Ok(x);
        }
    }
}

/// `strong_rand_range_nif(RangeBin)`: a random integer in `[0, Range)`, as a binary.
pub fn strong_rand_range(c: &mut Ctx, a: &[Term]) -> R {
    let range = BigUint::from_bytes_be(&bytes(c, a, 0, "range")?);
    if range.is_zero() {
        return Err(badarg(c, 0, "Bad range"));
    }
    Ok(bin(&uniform_below(c, &range)?.to_bytes_be()))
}

/// `rand_uniform_nif(From, To)` with both as binaries (mpint): an integer in `[From, To)`.
pub fn rand_uniform(c: &mut Ctx, a: &[Term]) -> R {
    // crypto.erl passes 4-byte-length-prefixed mpints here.
    fn mpint(b: &[u8]) -> Option<BigUint> {
        (b.len() >= 4).then(|| BigUint::from_bytes_be(&b[4..]))
    }
    let (from, to) = (bytes(c, a, 0, "from")?, bytes(c, a, 1, "to")?);
    let (Some(from), Some(to)) = (mpint(&from), mpint(&to)) else { return Err(badarg(c, 0, "Bad range")) };
    if to <= from {
        return Err(badarg(c, 1, "Bad range"));
    }
    let r = &from + uniform_below(c, &(&to - &from))?;
    let v = r.to_bytes_be();
    let mut out = (v.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&v);
    Ok(bin(&out))
}

/// `rand_seed_nif(Seed)`: the platform's source needs no seeding.
pub fn rand_seed(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.ok())
}
