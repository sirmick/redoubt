//! OTP's `crypto` NIFs, implemented in pure Rust on the RustCrypto and dalek crates.
//!
//! The real `crypto.erl` from OTP runs unchanged: its NIF stub functions (`hash_nif/2`,
//! `ng_crypto_init_nif/4`, ...) are replaced by these natives when the module loads, as
//! `erlang:load_nif/2` would do in BEAM. The embedder opts in:
//!
//! ```ignore
//! Vm::with_config(platform, Config { natives: beamlet_crypto::NATIVES, ..Default::default() })
//! ```
//!
//! What is supported is what `ssl` and `ssh` need for modern cipher suites; everything else
//! raises `notsup`, and `crypto:supports/1` lists exactly what is here:
//! - hashes: MD5, SHA-1, SHA-2 (224/256/384/512), SHA-3 (224/256/384/512);
//! - MACs: HMAC over those hashes, Poly1305; PBKDF2-HMAC;
//! - ciphers: AES-128/192/256 in CTR, CBC, ECB and CFB (128- and 8-bit feedback) modes, ChaCha20;
//!   AEAD: AES-128/256-GCM, ChaCha20-Poly1305;
//! - key agreement: X25519, ECDH on P-256 and P-384, finite-field Diffie-Hellman;
//! - signatures: Ed25519, ECDSA on P-256 and P-384, RSA (PKCS #1 v1.5 and PSS);
//!   RSA encryption (PKCS #1 v1.5); RSA key generation.
//!
//! It also provides `asn1rt_nif`'s BER TLV splitter (`asn1.rs`), which `public_key` needs for
//! certificates and keys.
//!
//! Randomness comes only from the platform (`Platform::random`). If it fails, the operation
//! fails; nothing falls back to a weaker source.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::any::Any;

use beamlet_vm::bif::{Ctx, Held, NativeSpec};
use beamlet_vm::{Exception, Term};

mod asn1;
mod cipher;
mod hash;
mod info;
mod pk;

type R = Result<Term, Exception>;

/// Every native this crate provides, for [`beamlet_vm::vm::Config::natives`].
pub static NATIVES: &[NativeSpec] = &[
    // Information and algorithm lists.
    ("crypto", "info_lib", 0, info::info_lib),
    ("crypto", "info_nif", 0, info::info_nif),
    ("crypto", "info_fips", 0, info::info_fips),
    ("crypto", "enable_fips_mode_nif", 1, info::enable_fips_mode),
    ("crypto", "hash_algorithms", 0, info::hash_algorithms),
    ("crypto", "pubkey_algorithms", 0, info::pubkey_algorithms),
    ("crypto", "cipher_algorithms", 0, info::cipher_algorithms),
    ("crypto", "kem_algorithms_nif", 0, info::empty_list),
    ("crypto", "kdf_algorithms", 0, info::empty_list),
    ("crypto", "mac_algorithms", 0, info::mac_algorithms),
    ("crypto", "curve_algorithms", 0, info::curve_algorithms),
    ("crypto", "rsa_opts_algorithms", 0, info::rsa_opts_algorithms),
    ("crypto", "hash_info_nif", 1, hash::hash_info),
    ("crypto", "cipher_info_nif", 1, cipher::cipher_info),
    // Random numbers.
    ("crypto", "strong_rand_bytes_nif", 1, info::strong_rand_bytes),
    ("crypto", "strong_rand_range_nif", 1, info::strong_rand_range),
    ("crypto", "rand_uniform_nif", 2, info::rand_uniform),
    ("crypto", "rand_seed_nif", 1, info::rand_seed),
    // Hashes, MACs, key derivation.
    ("crypto", "hash_nif", 2, hash::hash),
    ("crypto", "hash_init_nif", 1, hash::hash_init),
    ("crypto", "hash_update_nif", 2, hash::hash_update),
    ("crypto", "hash_final_nif", 1, hash::hash_final),
    ("crypto", "mac_nif", 4, hash::mac),
    ("crypto", "mac_init_nif", 3, hash::mac_init),
    ("crypto", "mac_update_nif", 2, hash::mac_update),
    ("crypto", "mac_final_nif", 1, hash::mac_final),
    ("crypto", "pbkdf2_hmac_nif", 5, hash::pbkdf2_hmac),
    ("crypto", "hash_equals_nif", 2, hash::hash_equals),
    // Ciphers.
    ("crypto", "ng_crypto_init_nif", 4, cipher::init),
    ("crypto", "ng_crypto_update_nif", 2, cipher::update),
    ("crypto", "ng_crypto_final_nif", 1, cipher::finalize),
    ("crypto", "ng_crypto_get_data_nif", 1, cipher::get_data),
    ("crypto", "ng_crypto_one_time_nif", 5, cipher::one_time),
    ("crypto", "aead_cipher_nif", 7, cipher::aead_one_time),
    ("crypto", "aead_cipher_init_nif", 4, cipher::aead_init),
    ("crypto", "aead_cipher_nif", 4, cipher::aead_with_state),
    // Public keys.
    ("crypto", "evp_generate_key_nif", 2, pk::evp_generate_key),
    ("crypto", "evp_compute_key_nif", 3, pk::evp_compute_key),
    ("crypto", "ec_generate_key_nif", 2, pk::ec_generate_key),
    ("crypto", "ecdh_compute_key_nif", 3, pk::ecdh_compute_key),
    ("crypto", "dh_generate_key_nif", 4, pk::dh_generate_key),
    ("crypto", "dh_compute_key_nif", 3, pk::dh_compute_key),
    ("crypto", "mod_exp_nif", 4, pk::mod_exp),
    ("crypto", "pkey_sign_nif", 5, pk::sign),
    ("crypto", "pkey_verify_nif", 6, pk::verify),
    ("crypto", "pkey_crypt_nif", 6, pk::crypt),
    ("crypto", "privkey_to_pubkey_nif", 2, pk::privkey_to_pubkey),
    ("crypto", "rsa_generate_key_nif", 2, pk::rsa_generate_key),
    // ASN.1 BER splitting, for public_key's certificate and key codecs.
    ("asn1rt_nif", "decode_ber_tlv_raw", 1, asn1::decode_ber_tlv),
    ("asn1rt_nif", "encode_ber_tlv", 1, asn1::encode_ber_tlv),
];

// ---- errors, in the shape crypto.erl's ?nif_call expects ----

/// `error:{Id, #{c_file_name, c_file_line_num, c_function_arg_num}, Msg}`, which `crypto.erl`
/// turns into `error:{Id, {File, Line}, Msg}` with the offending argument marked. `arg` is
/// 0-based, as in the C NIFs; `-1` means no particular argument.
fn nif_error(c: &mut Ctx, id: &str, arg: i64, msg: &str) -> Exception {
    let [file, line, argn, id] = ["c_file_name", "c_file_line_num", "c_function_arg_num", id].map(|n| c.atom(n));
    let name = c.string("beamlet_crypto");
    let info = c.map_from([(file, name), (line, Term::Int(0)), (argn, Term::Int(arg))]);
    let msg = c.string(msg);
    Exception::error(c.tuple(&[id, info, msg]))
}

fn badarg(c: &mut Ctx, arg: i64, msg: &str) -> Exception {
    nif_error(c, "badarg", arg, msg)
}

fn notsup(c: &mut Ctx, arg: i64, msg: &str) -> Exception {
    nif_error(c, "notsup", arg, msg)
}

// ---- arguments ----

/// Argument `i` as bytes: a binary or iodata, as `enif_inspect_iolist_as_binary` accepts.
fn bytes(c: &mut Ctx, a: &[Term], i: usize, what: &str) -> Result<Vec<u8>, Exception> {
    match c.heap().iodata_bytes(a[i]) {
        Some(b) => Ok(b),
        None => Err(badarg(c, i as i64, &alloc::format!("Bad {what}"))),
    }
}

fn atom_name(t: &Term) -> Option<&str> {
    match t {
        Term::Atom(a) => Some(a.as_str()),
        _ => None,
    }
}

fn is_true(c: &Ctx, t: &Term) -> bool {
    t.is_atom(&c.sys.atoms.true_)
}

// ---- resources ----

/// Wrap a native value as a resource term.
fn resource<T: Any>(c: &mut Ctx, value: T) -> Term {
    c.new_resource(value)
}

/// The `T` inside a resource argument, held so the caller's heap stays free.
fn resource_ref<T: Any>(c: &Ctx, t: &Term) -> Option<Held<T>> {
    c.resource::<T>(*t)
}

// ---- randomness ----

/// `n` bytes from the platform's secure source, or an error: never a weaker fallback.
fn random_bytes(c: &mut Ctx, n: usize) -> Result<Vec<u8>, Exception> {
    let mut buf = alloc::vec![0u8; n];
    match c.sys.platform.random(&mut buf) {
        Ok(()) => Ok(buf),
        Err(_) => Err(nif_error(c, "error", -1, "No secure random source")),
    }
}

/// A CSPRNG for the few algorithms that draw randomness as they go (RSA key generation and
/// padding): a ChaCha20 keystream keyed with 32 bytes from the platform. It is created only
/// after the platform supplied the key, so drawing from it cannot fail.
struct KeystreamRng(chacha20::ChaCha20);

impl KeystreamRng {
    fn new(c: &mut Ctx) -> Result<KeystreamRng, Exception> {
        use chacha20::cipher::KeyIvInit;
        let key = random_bytes(c, 32)?;
        let rng = chacha20::ChaCha20::new_from_slices(&key, &[0u8; 12]).expect("fixed key and nonce sizes");
        Ok(KeystreamRng(rng))
    }
}

impl rand_core_06::RngCore for KeystreamRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use chacha20::cipher::StreamCipher;
        dest.fill(0);
        self.0.apply_keystream(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core_06::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rand_core_06::CryptoRng for KeystreamRng {}

/// An integer argument given as a big-endian binary (crypto.erl's `ensure_int_as_bin`).
fn uint(c: &mut Ctx, a: &[Term], i: usize, what: &str) -> Result<num_bigint::BigUint, Exception> {
    let b = bytes(c, a, i, what)?;
    Ok(num_bigint::BigUint::from_bytes_be(&b))
}

fn bin(c: &mut Ctx, b: &[u8]) -> Term {
    c.binary(b)
}
