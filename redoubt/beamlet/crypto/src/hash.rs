//! Hashes, HMAC, Poly1305, PBKDF2 and constant-time comparison.

use alloc::vec::Vec;
use beamlet_vm::sync::Lock;

use beamlet_vm::bif::Ctx;
use beamlet_vm::Term;
use hmac::{KeyInit, Mac, SimpleHmac};
use sha2::Digest;

use crate::{atom_name, badarg, bin, bytes, notsup, resource, resource_ref, R};

/// A hash function by OTP name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Alg {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Sha3_224,
    Sha3_256,
    Sha3_384,
    Sha3_512,
}

/// `(OTP name, algorithm, OpenSSL NID, digest size, block size)`.
pub(crate) const ALGS: &[(&str, Alg, i64, usize, usize)] = &[
    ("md5", Alg::Md5, 4, 16, 64),
    ("sha", Alg::Sha1, 64, 20, 64),
    ("sha224", Alg::Sha224, 675, 28, 64),
    ("sha256", Alg::Sha256, 672, 32, 64),
    ("sha384", Alg::Sha384, 673, 48, 128),
    ("sha512", Alg::Sha512, 674, 64, 128),
    ("sha3_224", Alg::Sha3_224, 1096, 28, 144),
    ("sha3_256", Alg::Sha3_256, 1097, 32, 136),
    ("sha3_384", Alg::Sha3_384, 1098, 48, 104),
    ("sha3_512", Alg::Sha3_512, 1099, 64, 72),
];

pub(crate) fn alg(t: &Term) -> Option<Alg> {
    let name = atom_name(t)?;
    ALGS.iter().find(|(n, ..)| *n == name).map(|(_, a, ..)| *a)
}

/// Run `$body` with `$H` bound to the RustCrypto type for `$alg`.
macro_rules! with_hash {
    ($alg:expr, $H:ident => $body:expr) => {
        match $alg {
            Alg::Md5 => {
                type $H = md5::Md5;
                $body
            }
            Alg::Sha1 => {
                type $H = sha1::Sha1;
                $body
            }
            Alg::Sha224 => {
                type $H = sha2::Sha224;
                $body
            }
            Alg::Sha256 => {
                type $H = sha2::Sha256;
                $body
            }
            Alg::Sha384 => {
                type $H = sha2::Sha384;
                $body
            }
            Alg::Sha512 => {
                type $H = sha2::Sha512;
                $body
            }
            Alg::Sha3_224 => {
                type $H = sha3::Sha3_224;
                $body
            }
            Alg::Sha3_256 => {
                type $H = sha3::Sha3_256;
                $body
            }
            Alg::Sha3_384 => {
                type $H = sha3::Sha3_384;
                $body
            }
            Alg::Sha3_512 => {
                type $H = sha3::Sha3_512;
                $body
            }
        }
    };
}

pub(crate) fn digest(a: Alg, data: &[u8]) -> Vec<u8> {
    with_hash!(a, H => H::digest(data).to_vec())
}

fn hash_arg(c: &mut Ctx, a: &[Term], i: usize) -> Result<Alg, beamlet_vm::Exception> {
    match alg(&a[i]) {
        Some(h) => Ok(h),
        None => Err(badarg(c, i as i64, "Bad digest type")),
    }
}

pub fn hash_info(c: &mut Ctx, a: &[Term]) -> R {
    let name = atom_name(&a[0]).unwrap_or("");
    let Some(&(_, _, nid, size, block)) = ALGS.iter().find(|(n, ..)| *n == name) else {
        return Err(badarg(c, 0, "Bad digest type"));
    };
    let mut m: Vec<(Term, Term)> = Vec::new();
    {
        let k = c.atom("type");
        let v = Term::Int(nid);
        m.push((k, v));
    }
    {
        let k = c.atom("size");
        let v = Term::Int(size as i64);
        m.push((k, v));
    }
    {
        let k = c.atom("block_size");
        let v = Term::Int(block as i64);
        m.push((k, v));
    }
    Ok(c.map_from(m))
}

pub fn hash(c: &mut Ctx, a: &[Term]) -> R {
    let h = hash_arg(c, a, 0)?;
    let data = bytes(c, a, 1, "data")?;
    Ok(bin(c, &digest(h, &data)))
}

// ---- streaming hashes: immutable states, as in OTP (each update makes a new state) ----

#[derive(Clone)]
enum Hasher {
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha224(sha2::Sha224),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
    Sha3_224(sha3::Sha3_224),
    Sha3_256(sha3::Sha3_256),
    Sha3_384(sha3::Sha3_384),
    Sha3_512(sha3::Sha3_512),
}

macro_rules! each_hasher {
    ($h:expr, $x:ident => $body:expr) => {
        match $h {
            Hasher::Md5($x) => $body,
            Hasher::Sha1($x) => $body,
            Hasher::Sha224($x) => $body,
            Hasher::Sha256($x) => $body,
            Hasher::Sha384($x) => $body,
            Hasher::Sha512($x) => $body,
            Hasher::Sha3_224($x) => $body,
            Hasher::Sha3_256($x) => $body,
            Hasher::Sha3_384($x) => $body,
            Hasher::Sha3_512($x) => $body,
        }
    };
}

/// The resource behind a `hash_init` state.
struct HashState(Hasher);

pub fn hash_init(c: &mut Ctx, a: &[Term]) -> R {
    let h = hash_arg(c, a, 0)?;
    let hasher = match h {
        Alg::Md5 => Hasher::Md5(Default::default()),
        Alg::Sha1 => Hasher::Sha1(Default::default()),
        Alg::Sha224 => Hasher::Sha224(Default::default()),
        Alg::Sha256 => Hasher::Sha256(Default::default()),
        Alg::Sha384 => Hasher::Sha384(Default::default()),
        Alg::Sha512 => Hasher::Sha512(Default::default()),
        Alg::Sha3_224 => Hasher::Sha3_224(Default::default()),
        Alg::Sha3_256 => Hasher::Sha3_256(Default::default()),
        Alg::Sha3_384 => Hasher::Sha3_384(Default::default()),
        Alg::Sha3_512 => Hasher::Sha3_512(Default::default()),
    };
    Ok(resource(c, HashState(hasher)))
}

pub fn hash_update(c: &mut Ctx, a: &[Term]) -> R {
    let Some(state) = resource_ref::<HashState>(c, &a[0]) else {
        return Err(badarg(c, 0, "Bad state"));
    };
    let HashState(h) = &*state;
    let mut h = h.clone();
    let data = bytes(c, a, 1, "data")?;
    each_hasher!(&mut h, x => Digest::update(x, &data));
    Ok(resource(c, HashState(h)))
}

pub fn hash_final(c: &mut Ctx, a: &[Term]) -> R {
    let Some(state) = resource_ref::<HashState>(c, &a[0]) else {
        return Err(badarg(c, 0, "Bad state"));
    };
    let HashState(h) = &*state;
    let out: Vec<u8> = each_hasher!(h.clone(), x => x.finalize().to_vec());
    Ok(bin(c, &out))
}

// ---- MACs ----

/// A MAC in progress. Changed in place by `mac_update`, as OTP's MAC states are.
enum MacState {
    Hmac(Alg, Vec<u8>, Vec<u8>),
    Poly1305(Vec<u8>, Vec<u8>),
}

/// HMAC with key `key` over `data` using hash `h`.
pub(crate) fn hmac(h: Alg, key: &[u8], data: &[u8]) -> Vec<u8> {
    with_hash!(h, H => {
        let mut m = <SimpleHmac<H> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
        Mac::update(&mut m, data);
        m.finalize().into_bytes().to_vec()
    })
}

fn poly1305(c: &mut Ctx, key: &[u8], data: &[u8]) -> Result<Vec<u8>, beamlet_vm::Exception> {
    let k = poly1305::Key::try_from(key).map_err(|_| badarg(c, 2, "Bad key length"))?;
    Ok(poly1305::Poly1305::new(&k).compute_unpadded(data).to_vec())
}

/// `mac_nif(Type, SubType, Key, Data)`.
pub fn mac(c: &mut Ctx, a: &[Term]) -> R {
    let key = bytes(c, a, 2, "key")?;
    let data = bytes(c, a, 3, "text")?;
    match atom_name(&a[0]) {
        Some("hmac") => {
            let h = hash_arg(c, a, 1)?;
            Ok(bin(c, &hmac(h, &key, &data)))
        }
        Some("poly1305") => {
            let tag = poly1305(c, &key, &data)?;
            Ok(bin(c, &tag))
        }
        Some("cmac" | "siphash") => Err(notsup(c, 0, "Unsupported mac algorithm")),
        _ => Err(badarg(c, 0, "Unknown mac algorithm")),
    }
}

/// Streaming MACs keep the data and compute at the end: simple, and the messages MACed
/// incrementally (SSH packets) are small.
pub fn mac_init(c: &mut Ctx, a: &[Term]) -> R {
    let key = bytes(c, a, 2, "key")?;
    let state = match atom_name(&a[0]) {
        Some("hmac") => MacState::Hmac(hash_arg(c, a, 1)?, key, Vec::new()),
        Some("poly1305") => {
            if key.len() != 32 {
                return Err(badarg(c, 2, "Bad key length"));
            }
            MacState::Poly1305(key, Vec::new())
        }
        Some("cmac" | "siphash") => return Err(notsup(c, 0, "Unsupported mac algorithm")),
        _ => return Err(badarg(c, 0, "Unknown mac algorithm")),
    };
    Ok(resource(c, Lock::new(state)))
}

pub fn mac_update(c: &mut Ctx, a: &[Term]) -> R {
    let data = bytes(c, a, 1, "text")?;
    let Some(state) = resource_ref::<Lock<MacState>>(c, &a[0]) else {
        return Err(badarg(c, 0, "Bad ref"));
    };
    match &mut *state.lock() {
        MacState::Hmac(_, _, buf) | MacState::Poly1305(_, buf) => buf.extend_from_slice(&data),
    }
    Ok(a[0])
}

pub fn mac_final(c: &mut Ctx, a: &[Term]) -> R {
    let Some(state) = resource_ref::<Lock<MacState>>(c, &a[0]) else {
        return Err(badarg(c, 0, "Bad ref"));
    };
    let out = match &*state.lock() {
        MacState::Hmac(h, key, buf) => hmac(*h, key, buf),
        MacState::Poly1305(key, buf) => {
            let k = poly1305::Key::try_from(&key[..]).expect("checked at init");
            poly1305::Poly1305::new(&k).compute_unpadded(buf).to_vec()
        }
    };
    Ok(bin(c, &out))
}

/// `pbkdf2_hmac_nif(Digest, Pass, Salt, Iter, KeyLen)`.
pub fn pbkdf2_hmac(c: &mut Ctx, a: &[Term]) -> R {
    let h = hash_arg(c, a, 0)?;
    let pass = bytes(c, a, 1, "password")?;
    let salt = bytes(c, a, 2, "salt")?;
    let iter = a[3]
        .as_i64()
        .and_then(|i| u32::try_from(i).ok())
        .filter(|i| *i > 0);
    let Some(iter) = iter else {
        return Err(badarg(c, 3, "Bad iteration count"));
    };
    let len = a[4].as_usize().filter(|l| *l <= 1 << 20);
    let Some(len) = len else {
        return Err(badarg(c, 4, "Bad key length"));
    };
    let mut out = alloc::vec![0u8; len];
    with_hash!(h, H => pbkdf2::pbkdf2::<SimpleHmac<H>>(&pass, &salt, iter, &mut out).expect("HMAC takes any key"));
    Ok(bin(c, &out))
}

/// Constant-time equality of two binaries of the same size.
pub fn hash_equals(c: &mut Ctx, a: &[Term]) -> R {
    use subtle::ConstantTimeEq;
    let (x, y) = (bytes(c, a, 0, "binary")?, bytes(c, a, 1, "binary")?);
    if x.len() != y.len() {
        return Err(badarg(c, 1, "Binaries of different size"));
    }
    Ok(c.bool(bool::from(x.ct_eq(&y))))
}
