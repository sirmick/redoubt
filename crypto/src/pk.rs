//! Public-key operations: key agreement, signatures, RSA encryption, key generation.

use alloc::vec::Vec;

use beamlet_vm::bif::Ctx;
use beamlet_vm::{Exception, Term};
use num_bigint::BigUint;
use num_traits::Zero;

use crate::hash::{self, Alg};
use crate::{atom_name, badarg, bin, bytes, nif_error, notsup, random_bytes, uint, KeystreamRng, R};

type E = Exception;

// ---- X25519 and Ed25519 (the "evp" keys) ----

fn key32(c: &mut Ctx, b: &[u8], arg: i64) -> Result<[u8; 32], E> {
    <[u8; 32]>::try_from(b).map_err(|_| badarg(c, arg, "Bad key length"))
}

/// A private key argument: a binary, or `undefined` to generate one.
fn private_or_random(c: &mut Ctx, a: &[Term], i: usize, len: usize) -> Result<Vec<u8>, E> {
    if a[i].is_atom(&c.sys.atoms.undefined) {
        random_bytes(c, len)
    } else {
        bytes(c, a, i, "private key")
    }
}

/// `evp_generate_key_nif(Curve, PrivKey)` → `{Public, Private}`.
pub fn evp_generate_key(c: &mut Ctx, a: &[Term]) -> R {
    match atom_name(&a[0]) {
        Some("x25519") => {
            let private = private_or_random(c, a, 1, 32)?;
            let secret = x25519_dalek::StaticSecret::from(key32(c, &private, 1)?);
            let public = x25519_dalek::PublicKey::from(&secret);
            Ok(Term::tuple(alloc::vec![bin(public.as_bytes()), bin(&private)]))
        }
        Some("ed25519") => {
            let private = private_or_random(c, a, 1, 32)?;
            let signing = ed25519_dalek::SigningKey::from_bytes(&key32(c, &private, 1)?);
            Ok(Term::tuple(alloc::vec![bin(signing.verifying_key().as_bytes()), bin(&private)]))
        }
        Some("x448" | "ed448") => Err(notsup(c, 0, "Unsupported curve")),
        _ => Err(badarg(c, 0, "Bad curve")),
    }
}

/// `evp_compute_key_nif(Curve, OthersPublic, MyPrivate)` → shared secret.
pub fn evp_compute_key(c: &mut Ctx, a: &[Term]) -> R {
    if atom_name(&a[0]) != Some("x25519") {
        return Err(notsup(c, 0, "Unsupported curve"));
    }
    let (theirs, mine) = (bytes(c, a, 1, "public key")?, bytes(c, a, 2, "private key")?);
    let theirs = x25519_dalek::PublicKey::from(key32(c, &theirs, 1)?);
    let secret = x25519_dalek::StaticSecret::from(key32(c, &mine, 2)?);
    let shared = secret.diffie_hellman(&theirs);
    // An all-zero result means the peer sent a low-order point; OpenSSL refuses those too.
    if !shared.was_contributory() {
        return Err(nif_error(c, "error", -1, "Can't derive secret"));
    }
    Ok(bin(shared.as_bytes()))
}

// ---- NIST curves: ECDH and ECDSA on P-256 and P-384 ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Curve {
    P256,
    P384,
}

/// The curve in `{Params, Name}` (crypto.erl's `nif_curve_params/1`), by name.
fn curve(c: &mut Ctx, t: &Term, arg: i64) -> Result<Curve, E> {
    let name = t.as_tuple().and_then(|p| p.get(1)).and_then(atom_name);
    match name {
        Some("secp256r1" | "prime256v1") => Ok(Curve::P256),
        Some("secp384r1") => Ok(Curve::P384),
        _ => Err(notsup(c, arg, "Unsupported curve")),
    }
}

/// Run `$body` with `$C` the RustCrypto curve type.
macro_rules! with_curve {
    ($curve:expr, $C:ident => $body:expr) => {
        match $curve {
            Curve::P256 => { type $C = p256::NistP256; $body }
            Curve::P384 => { type $C = p384::NistP384; $body }
        }
    };
}

/// `ec_generate_key_nif(Curve, PrivKey)` → `{PublicPoint, Private}` (uncompressed point; the
/// private key padded to the size of the curve order, as OpenSSL does).
pub fn ec_generate_key(c: &mut Ctx, a: &[Term]) -> R {
    use elliptic_curve::sec1::ToSec1Point;
    let cv = curve(c, &a[0], 0)?;
    with_curve!(cv, C => {
        let size = <elliptic_curve::FieldBytes<C>>::default().len();
        let secret = if a[1].is_atom(&c.sys.atoms.undefined) {
            // A random scalar: retry the (astronomically rare) out-of-range draw.
            let mut found = None;
            for _ in 0..8 {
                let b = random_bytes(c, size)?;
                if let Ok(k) = elliptic_curve::SecretKey::<C>::from_slice(&b) {
                    found = Some(k);
                    break;
                }
            }
            found.ok_or_else(|| nif_error(c, "error", -1, "Couldn't generate EC key"))?
        } else {
            let b = bytes(c, a, 1, "private key")?;
            let mut padded = alloc::vec![0u8; size.saturating_sub(b.len())];
            padded.extend_from_slice(&b);
            elliptic_curve::SecretKey::<C>::from_slice(&padded).map_err(|_| badarg(c, 1, "Couldn't get EC key"))?
        };
        let point = secret.public_key().to_sec1_point(false);
        Ok(Term::tuple(alloc::vec![bin(point.as_bytes()), bin(&secret.to_bytes())]))
    })
}

/// `ecdh_compute_key_nif(OthersPoint, Curve, MyPrivate)` → the shared x-coordinate.
pub fn ecdh_compute_key(c: &mut Ctx, a: &[Term]) -> R {
    let cv = curve(c, &a[1], 1)?;
    let (theirs, mine) = (bytes(c, a, 0, "public key")?, bytes(c, a, 2, "private key")?);
    with_curve!(cv, C => {
        let public = elliptic_curve::PublicKey::<C>::from_sec1_bytes(&theirs).map_err(|_| badarg(c, 0, "Couldn't get ecpoint"))?;
        let secret = elliptic_curve::SecretKey::<C>::from_slice(&mine).map_err(|_| badarg(c, 2, "Couldn't get EC key"))?;
        let shared = elliptic_curve::ecdh::diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
        Ok(bin(shared.raw_secret_bytes()))
    })
}

// ---- finite-field Diffie-Hellman and modular exponentiation ----

fn dh_params(c: &mut Ctx, t: &Term, arg: i64) -> Result<(BigUint, BigUint), E> {
    let parts = t.to_vec().unwrap_or_default();
    let p = parts.first().and_then(|x| x.iodata_bytes()).map(|b| BigUint::from_bytes_be(&b));
    let g = parts.get(1).and_then(|x| x.iodata_bytes()).map(|b| BigUint::from_bytes_be(&b));
    match (p, g) {
        (Some(p), Some(g)) if p > BigUint::from(3u32) && !g.is_zero() => Ok((p, g)),
        _ => Err(badarg(c, arg, "Bad DH parameters")),
    }
}

/// `dh_generate_key_nif(PrivKey, [P, G], Mpint, Len)` → `{Public, Private}`.
pub fn dh_generate_key(c: &mut Ctx, a: &[Term]) -> R {
    let (p, g) = dh_params(c, &a[1], 1)?;
    let private = if a[0].is_atom(&c.sys.atoms.undefined) || a[0].iodata_bytes().is_some_and(|b| b.is_empty()) {
        // A private exponent in [2, p - 2], from as many random bytes as p has plus 8, so the
        // reduction bias is negligible.
        let r = BigUint::from_bytes_be(&random_bytes(c, p.to_bytes_be().len() + 8)?);
        r % (&p - 3u32) + 2u32
    } else {
        uint(c, a, 0, "private key")?
    };
    let public = g.modpow(&private, &p);
    Ok(Term::tuple(alloc::vec![bin(&public.to_bytes_be()), bin(&private.to_bytes_be())]))
}

/// `dh_compute_key_nif(OthersPublic, MyPrivate, [P, G])`.
pub fn dh_compute_key(c: &mut Ctx, a: &[Term]) -> R {
    let (p, _) = dh_params(c, &a[2], 2)?;
    let theirs = uint(c, a, 0, "public key")?;
    // Reject the trivial public values 0, 1 and p - 1, and anything outside [2, p - 2].
    if theirs < BigUint::from(2u32) || theirs > &p - 2u32 {
        return Err(nif_error(c, "error", -1, "Bad public key"));
    }
    let mine = uint(c, a, 1, "private key")?;
    Ok(bin(&theirs.modpow(&mine, &p).to_bytes_be()))
}

/// `mod_exp_nif(Base, Exponent, Modulus, BinHdr)`: with `BinHdr` 4, a 4-byte length prefix.
pub fn mod_exp(c: &mut Ctx, a: &[Term]) -> R {
    let (base, exp, m) = (uint(c, a, 0, "base")?, uint(c, a, 1, "exponent")?, uint(c, a, 2, "modulus")?);
    if m.is_zero() {
        return Err(badarg(c, 2, "Modulus is zero"));
    }
    let r = base.modpow(&exp, &m).to_bytes_be();
    Ok(match a[3].as_i64() {
        Some(4) => {
            let mut out = (r.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(&r);
            bin(&out)
        }
        _ => bin(&r),
    })
}

// ---- signatures ----

/// The message to sign: `Data` hashed with `Type`, or `{digest, Digest}` given directly.
/// Returns the digest and its algorithm (`None` for type `none`: raw data).
fn digest_of(c: &mut Ctx, a: &[Term], type_arg: usize, data_arg: usize) -> Result<(Vec<u8>, Option<Alg>), E> {
    let alg = if atom_name(&a[type_arg]) == Some("none") {
        None
    } else {
        Some(hash::alg(&a[type_arg]).ok_or_else(|| badarg(c, type_arg as i64, "Bad digest type"))?)
    };
    if let Some([tag, d]) = a[data_arg].as_tuple() {
        if atom_name(tag) == Some("digest") {
            return Ok((d.iodata_bytes().ok_or_else(|| badarg(c, data_arg as i64, "Bad digest"))?, alg));
        }
    }
    let data = bytes(c, a, data_arg, "data")?;
    Ok(match alg {
        Some(h) => (hash::digest(h, &data), alg),
        None => (data, None),
    })
}

/// `pkey_sign_nif(Algorithm, Type, Data, Key, Options)`.
pub fn sign(c: &mut Ctx, a: &[Term]) -> R {
    match atom_name(&a[0]) {
        Some("eddsa") => {
            let key = a[3].to_vec().unwrap_or_default();
            if key.get(1).and_then(atom_name) != Some("ed25519") {
                return Err(notsup(c, 3, "Unsupported curve"));
            }
            let private = key.first().and_then(|k| k.iodata_bytes()).ok_or_else(|| badarg(c, 3, "Bad key"))?;
            let signing = ed25519_dalek::SigningKey::from_bytes(&key32(c, &private, 3)?);
            let msg = bytes(c, a, 2, "data")?;
            use ed25519_dalek::Signer;
            Ok(bin(&signing.sign(&msg).to_bytes()))
        }
        Some("ecdsa") => {
            let (digest, _) = digest_of(c, a, 1, 2)?;
            let (cv, private) = ec_key(c, &a[3], 3)?;
            with_curve!(cv, C => {
                use ecdsa::signature::hazmat::PrehashSigner;
                let key = ecdsa::SigningKey::<C>::from_slice(&private).map_err(|_| badarg(c, 3, "Couldn't get EC key"))?;
                // RFC 6979 deterministic nonces: no randomness needed, so none can be misused.
                let sig: ecdsa::Signature<C> = key.sign_prehash(&prehash::<C>(&digest)).map_err(|_| nif_error(c, "error", -1, "Can't sign"))?;
                Ok(bin(sig.to_der().as_bytes()))
            })
        }
        Some("rsa") => {
            let (digest, alg) = digest_of(c, a, 1, 2)?;
            let key = rsa_private(c, &a[3], 3)?;
            let opts = RsaOpts::parse(c, &a[4], 4)?;
            let sig = if opts.pss {
                let alg = alg.ok_or_else(|| badarg(c, 1, "PSS needs a digest"))?;
                let mut rng = KeystreamRng::new(c)?;
                with_digest10(alg, &opts, |scheme| key.sign_with_rng(&mut rng, scheme, &digest))
                    .ok_or_else(|| notsup(c, 1, "Unsupported digest for PSS"))?
            } else {
                key.sign(rsa::Pkcs1v15Sign::new_unprefixed(), &digest_info(alg, &digest))
            };
            sig.map(|s| bin(&s)).map_err(|_| nif_error(c, "error", -1, "Can't sign"))
        }
        Some("dss") => Err(notsup(c, 0, "Unsupported algorithm")),
        _ => Err(badarg(c, 0, "Bad algorithm")),
    }
}

/// `pkey_verify_nif(Algorithm, Type, Data, Signature, Key, Options)` → `true | false`.
pub fn verify(c: &mut Ctx, a: &[Term]) -> R {
    let sig = bytes(c, a, 3, "signature")?;
    let ok = match atom_name(&a[0]) {
        Some("eddsa") => {
            let key = a[4].to_vec().unwrap_or_default();
            if key.get(1).and_then(atom_name) != Some("ed25519") {
                return Err(notsup(c, 4, "Unsupported curve"));
            }
            let public = key.first().and_then(|k| k.iodata_bytes()).ok_or_else(|| badarg(c, 4, "Bad key"))?;
            let msg = bytes(c, a, 2, "data")?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&key32(c, &public, 4)?);
            match (vk, <[u8; 64]>::try_from(&sig[..])) {
                (Ok(vk), Ok(s)) => vk.verify_strict(&msg, &ed25519_dalek::Signature::from_bytes(&s)).is_ok(),
                _ => false,
            }
        }
        Some("ecdsa") => {
            let (digest, _) = digest_of(c, a, 1, 2)?;
            let (cv, public) = ec_key(c, &a[4], 4)?;
            with_curve!(cv, C => {
                use ecdsa::signature::hazmat::PrehashVerifier;
                match (ecdsa::VerifyingKey::<C>::from_sec1_bytes(&public), ecdsa::Signature::<C>::from_der(&sig)) {
                    (Ok(vk), Ok(s)) => vk.verify_prehash(&prehash::<C>(&digest), &s).is_ok(),
                    _ => false,
                }
            })
        }
        Some("rsa") => {
            let (digest, alg) = digest_of(c, a, 1, 2)?;
            let key = rsa_public(c, &a[4], 4)?;
            let opts = RsaOpts::parse(c, &a[5], 5)?;
            if opts.pss {
                let alg = alg.ok_or_else(|| badarg(c, 1, "PSS needs a digest"))?;
                with_digest10(alg, &opts, |scheme| key.verify(scheme, &digest, &sig))
                    .ok_or_else(|| notsup(c, 1, "Unsupported digest for PSS"))?
                    .is_ok()
            } else {
                key.verify(rsa::Pkcs1v15Sign::new_unprefixed(), &digest_info(alg, &digest), &sig).is_ok()
            }
        }
        Some("dss") => return Err(notsup(c, 0, "Unsupported algorithm")),
        _ => return Err(badarg(c, 0, "Bad algorithm")),
    };
    Ok(c.bool(ok))
}

/// A digest as the curve's field-sized prehash: truncated or left-padded, per SEC 1.
fn prehash<C: elliptic_curve::Curve>(digest: &[u8]) -> Vec<u8> {
    let size = <elliptic_curve::FieldBytes<C>>::default().len();
    if digest.len() >= size {
        digest[..size].to_vec()
    } else {
        let mut v = alloc::vec![0u8; size - digest.len()];
        v.extend_from_slice(digest);
        v
    }
}

/// `{Curve, KeyBin}` as crypto.erl's `format_pkey(ecdsa, ...)` makes it.
fn ec_key(c: &mut Ctx, t: &Term, arg: i64) -> Result<(Curve, Vec<u8>), E> {
    match t.as_tuple() {
        Some([params, key]) => {
            let cv = curve(c, params, arg)?;
            let k = key.iodata_bytes().ok_or_else(|| badarg(c, arg, "Bad key"))?;
            Ok((cv, k))
        }
        _ => Err(badarg(c, arg, "Bad key")),
    }
}

// ---- RSA ----

/// PKCS #1 v1.5 DigestInfo: the DER header naming the hash, then the digest. `None` (raw
/// data) has no header.
fn digest_info(alg: Option<Alg>, digest: &[u8]) -> Vec<u8> {
    let header: &[u8] = match alg {
        None => &[],
        Some(Alg::Md5) => &[0x30, 0x20, 0x30, 0x0c, 0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x05, 0x05, 0x00, 0x04, 0x10],
        Some(Alg::Sha1) => &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14],
        Some(Alg::Sha224) => &[0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x04, 0x05, 0x00, 0x04, 0x1c],
        Some(Alg::Sha256) => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
        Some(Alg::Sha384) => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30],
        Some(Alg::Sha512) => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40],
        Some(Alg::Sha3_224) => &[0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x07, 0x05, 0x00, 0x04, 0x1c],
        Some(Alg::Sha3_256) => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x08, 0x05, 0x00, 0x04, 0x20],
        Some(Alg::Sha3_384) => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x09, 0x05, 0x00, 0x04, 0x30],
        Some(Alg::Sha3_512) => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x0a, 0x05, 0x00, 0x04, 0x40],
    };
    let mut out = header.to_vec();
    out.extend_from_slice(digest);
    out
}

struct RsaOpts {
    pss: bool,
    /// PSS salt length: -1 is the digest length, -2 the maximum; otherwise bytes.
    salt_len: i64,
    /// MGF1 digest for PSS; the signature digest if absent.
    mgf1: Option<Alg>,
    padding: Option<&'static str>,
}

impl RsaOpts {
    fn parse(c: &mut Ctx, t: &Term, arg: i64) -> Result<RsaOpts, E> {
        let mut o = RsaOpts { pss: false, salt_len: -1, mgf1: None, padding: None };
        for item in t.to_vec().ok_or_else(|| badarg(c, arg, "Bad options"))? {
            match item.as_tuple() {
                Some([k, v]) => match (atom_name(k), atom_name(v)) {
                    (Some("rsa_padding"), Some("rsa_pkcs1_pss_padding")) => o.pss = true,
                    (Some("rsa_padding"), Some("rsa_pkcs1_padding")) => o.padding = Some("pkcs1"),
                    (Some("rsa_padding"), Some("rsa_no_padding")) => o.padding = Some("none"),
                    (Some("rsa_padding"), Some("rsa_pkcs1_oaep_padding")) => o.padding = Some("oaep"),
                    (Some("rsa_padding"), _) => return Err(notsup(c, arg, "Unsupported padding")),
                    (Some("rsa_pss_saltlen"), _) => o.salt_len = v.as_i64().ok_or_else(|| badarg(c, arg, "Bad salt length"))?,
                    (Some("rsa_mgf1_md"), _) => o.mgf1 = Some(hash::alg(v).ok_or_else(|| badarg(c, arg, "Bad mgf1 digest"))?),
                    // OAEP label and digest: accepted for the parser's sake; OAEP is refused below.
                    (Some("rsa_oaep_md" | "rsa_oaep_label"), _) => {}
                    _ => return Err(badarg(c, arg, "Bad option")),
                },
                _ => return Err(badarg(c, arg, "Bad option")),
            }
        }
        Ok(o)
    }
}

/// Run `f` with the PSS scheme for digest `alg` (and `opts`' salt length), using the digest
/// 0.10 generation of SHA-1/SHA-2 that the `rsa` 0.9 crate works with.
fn with_digest10<T>(alg: Alg, opts: &RsaOpts, f: impl FnOnce(rsa::Pss) -> T) -> Option<T> {
    if opts.mgf1.is_some_and(|m| m != alg) {
        return None; // a different MGF1 digest is not supported
    }
    macro_rules! pss {
        ($D:ty, $len:expr) => {{
            let salt = if opts.salt_len >= 0 { opts.salt_len as usize } else { $len };
            f(rsa::Pss::new_with_salt::<$D>(salt))
        }};
    }
    Some(match alg {
        Alg::Sha1 => pss!(sha1_10::Sha1, 20),
        Alg::Sha224 => pss!(sha2_10::Sha224, 28),
        Alg::Sha256 => pss!(sha2_10::Sha256, 32),
        Alg::Sha384 => pss!(sha2_10::Sha384, 48),
        Alg::Sha512 => pss!(sha2_10::Sha512, 64),
        _ => return None,
    })
}

fn rsa_int(t: &Term) -> Option<rsa::BigUint> {
    t.iodata_bytes().map(|b| rsa::BigUint::from_bytes_be(&b))
}

/// `[E, N]`, or longer (a private key's list starts the same way).
fn rsa_public(c: &mut Ctx, t: &Term, arg: i64) -> Result<rsa::RsaPublicKey, E> {
    let parts = t.to_vec().unwrap_or_default();
    match (parts.first().and_then(rsa_int), parts.get(1).and_then(rsa_int)) {
        (Some(e), Some(n)) => rsa::RsaPublicKey::new(n, e).map_err(|_| badarg(c, arg, "Bad RSA public key")),
        _ => Err(badarg(c, arg, "Bad RSA public key")),
    }
}

/// `[E, N, D]` or `[E, N, D, P1, P2, E1, E2, C]`.
fn rsa_private(c: &mut Ctx, t: &Term, arg: i64) -> Result<rsa::RsaPrivateKey, E> {
    let parts: Vec<rsa::BigUint> = t.to_vec().unwrap_or_default().iter().filter_map(rsa_int).collect();
    if parts.len() < 3 {
        return Err(badarg(c, arg, "Bad RSA private key"));
    }
    let primes = if parts.len() >= 5 { alloc::vec![parts[3].clone(), parts[4].clone()] } else { Vec::new() };
    let key = rsa::RsaPrivateKey::from_components(parts[1].clone(), parts[0].clone(), parts[2].clone(), primes)
        .map_err(|_| badarg(c, arg, "Bad RSA private key"))?;
    key.validate().map_err(|_| badarg(c, arg, "Bad RSA private key"))?;
    Ok(key)
}

/// `pkey_crypt_nif(rsa, In, Key, Options, IsPrivate, IsEncrypt)`: RSA with PKCS #1 v1.5
/// padding (type 2 for public-key encryption, type 1 for private-key "encryption").
pub fn crypt(c: &mut Ctx, a: &[Term]) -> R {
    use rsa::traits::PublicKeyParts;
    if atom_name(&a[0]) != Some("rsa") {
        return Err(notsup(c, 0, "Unsupported algorithm"));
    }
    let input = bytes(c, a, 1, "data")?;
    let opts = RsaOpts::parse(c, &a[3], 3)?;
    if !matches!(opts.padding, None | Some("pkcs1")) {
        return Err(notsup(c, 3, "Unsupported padding"));
    }
    let (private, encrypt) = (crate::is_true(c, &a[4]), crate::is_true(c, &a[5]));
    let failed = |c: &mut Ctx| nif_error(c, "error", -1, "Couldn't crypt");
    let out = match (private, encrypt) {
        (false, true) => {
            let key = rsa_public(c, &a[2], 2)?;
            let mut rng = KeystreamRng::new(c)?;
            key.encrypt(&mut rng, rsa::Pkcs1v15Encrypt, &input).map_err(|_| failed(c))?
        }
        (true, false) => {
            let key = rsa_private(c, &a[2], 2)?;
            let mut rng = KeystreamRng::new(c)?;
            key.decrypt_blinded(&mut rng, rsa::Pkcs1v15Encrypt, &input).map_err(|_| failed(c))?
        }
        (true, true) => {
            let key = rsa_private(c, &a[2], 2)?;
            key.sign(rsa::Pkcs1v15Sign::new_unprefixed(), &input).map_err(|_| failed(c))?
        }
        (false, false) => {
            // Undo a type 1 padding: m = s^e mod n, then 00 01 FF...FF 00 data.
            let key = rsa_public(c, &a[2], 2)?;
            let k = key.size();
            let m = rsa::BigUint::from_bytes_be(&input).modpow(key.e(), key.n()).to_bytes_be();
            let mut em = alloc::vec![0u8; k.saturating_sub(m.len())];
            em.extend_from_slice(&m);
            let sep = em.iter().skip(2).position(|&b| b != 0xff).map(|i| i + 2);
            match sep {
                Some(i) if em.len() == k && em[0] == 0 && em[1] == 1 && em[i] == 0 && i >= 10 => em[i + 1..].to_vec(),
                _ => return Err(failed(c)),
            }
        }
    };
    Ok(bin(&out))
}

/// `privkey_to_pubkey_nif(rsa, [E, N, D | _])` → `[E, N]`.
pub fn privkey_to_pubkey(c: &mut Ctx, a: &[Term]) -> R {
    use rsa::traits::PublicKeyParts;
    if atom_name(&a[0]) != Some("rsa") {
        return Err(notsup(c, 0, "Unsupported algorithm"));
    }
    let key = rsa_private(c, &a[1], 1)?;
    Ok(Term::list(alloc::vec![bin(&key.e().to_bytes_be()), bin(&key.n().to_bytes_be())]))
}

/// `rsa_generate_key_nif(Bits, PublicExponent)` → `[E, N, D, P1, P2, E1, E2, C]`.
pub fn rsa_generate_key(c: &mut Ctx, a: &[Term]) -> R {
    use rsa::traits::{PrivateKeyParts, PublicKeyParts};
    let bits = a[0].as_usize().filter(|b| (512..=8192).contains(b)).ok_or_else(|| badarg(c, 0, "Bad modulus size"))?;
    let e = rsa_int(&a[1]).ok_or_else(|| badarg(c, 1, "Bad exponent"))?;
    let mut rng = KeystreamRng::new(c)?;
    let key = rsa::RsaPrivateKey::new_with_exp(&mut rng, bits, &e).map_err(|_| nif_error(c, "error", -1, "Key generation failed"))?;
    let primes = key.primes();
    let (p, q) = (&primes[0], &primes[1]);
    let d = key.d();
    let one = rsa::BigUint::from(1u32);
    let dp = d % (p - &one);
    let dq = d % (q - &one);
    let qinv = key.crt_coefficient().ok_or_else(|| nif_error(c, "error", -1, "Key generation failed"))?;
    let parts = [key.e(), key.n(), d, p, q, &dp, &dq, &qinv];
    Ok(Term::list(parts.iter().map(|x| bin(&x.to_bytes_be())).collect::<Vec<_>>()))
}
