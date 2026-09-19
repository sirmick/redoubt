//! Symmetric ciphers: `crypto_init/update/final`, `crypto_one_time`, and AEAD.
//!
//! Block modes (CBC, ECB) are driven block by block over the AES block cipher, buffering a
//! partial block between updates exactly as OpenSSL does, so that OTP's padding options
//! (`undefined`, `none`, `pkcs_padding`, `zero`, `random`) behave as in BEAM.

use alloc::vec::Vec;
use core::cell::RefCell;

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit, KeyIvInit, StreamCipher, StreamCipherSeek};
use beamlet_vm::bif::Ctx;
use beamlet_vm::{Exception, Term};

use crate::{atom_name, badarg, bin, bytes, is_true, nif_error, notsup, random_bytes, resource, resource_ref, R};

const BLOCK: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Cbc,
    Ecb,
    Ctr,
    /// CFB with 128-bit and with 8-bit feedback.
    Cfb128,
    Cfb8,
    Gcm,
    ChaCha20,
    ChaCha20Poly1305,
}

/// `(OTP name, mode, key length, OpenSSL NID)`.
const CIPHERS: &[(&str, Mode, usize, i64)] = &[
    ("aes_128_cbc", Mode::Cbc, 16, 419),
    ("aes_192_cbc", Mode::Cbc, 24, 423),
    ("aes_256_cbc", Mode::Cbc, 32, 427),
    ("aes_128_ecb", Mode::Ecb, 16, 418),
    ("aes_192_ecb", Mode::Ecb, 24, 422),
    ("aes_256_ecb", Mode::Ecb, 32, 426),
    ("aes_128_ctr", Mode::Ctr, 16, 904),
    ("aes_192_ctr", Mode::Ctr, 24, 905),
    ("aes_256_ctr", Mode::Ctr, 32, 906),
    ("aes_128_cfb128", Mode::Cfb128, 16, 421),
    ("aes_192_cfb128", Mode::Cfb128, 24, 425),
    ("aes_256_cfb128", Mode::Cfb128, 32, 429),
    // OTP reports the CFB-128 NIDs for CFB-8 too.
    ("aes_128_cfb8", Mode::Cfb8, 16, 421),
    ("aes_192_cfb8", Mode::Cfb8, 24, 425),
    ("aes_256_cfb8", Mode::Cfb8, 32, 429),
    ("aes_128_gcm", Mode::Gcm, 16, 895),
    ("aes_192_gcm", Mode::Gcm, 24, 898),
    ("aes_256_gcm", Mode::Gcm, 32, 901),
    ("chacha20", Mode::ChaCha20, 32, 1019),
    ("chacha20_poly1305", Mode::ChaCha20Poly1305, 32, 1018),
];

pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    CIPHERS.iter().map(|(n, ..)| *n)
}

fn lookup(t: &Term) -> Option<(Mode, usize, i64)> {
    let name = atom_name(t)?;
    CIPHERS.iter().find(|(n, ..)| *n == name).map(|&(_, m, k, nid)| (m, k, nid))
}

fn iv_len(m: Mode) -> usize {
    match m {
        Mode::Cbc | Mode::Ctr | Mode::Cfb128 | Mode::Cfb8 | Mode::ChaCha20 => 16,
        Mode::Ecb => 0,
        Mode::Gcm | Mode::ChaCha20Poly1305 => 12,
    }
}

pub fn cipher_info(c: &mut Ctx, a: &[Term]) -> R {
    let Some((mode, key_len, nid)) = lookup(&a[0]) else { return Err(Exception::error(c.atom("notsup"))) };
    let block = if matches!(mode, Mode::Cbc | Mode::Ecb) { BLOCK } else { 1 };
    let aead = matches!(mode, Mode::Gcm | Mode::ChaCha20Poly1305);
    let mode_name = match mode {
        Mode::Cbc => "cbc_mode",
        Mode::Ecb => "ecb_mode",
        Mode::Ctr => "ctr_mode",
        Mode::Cfb128 | Mode::Cfb8 => "cfb_mode",
        Mode::Gcm => "gcm_mode",
        Mode::ChaCha20 | Mode::ChaCha20Poly1305 => "stream_cipher",
    };
    let mut m: Vec<(Term, Term)> = Vec::new();
    // OpenSSL 3 reports no NID for the CTR and ChaCha ciphers; neither do we.
    let ty = if matches!(mode, Mode::Ctr | Mode::ChaCha20 | Mode::ChaCha20Poly1305) { c.atom("undefined") } else { Term::Int(nid) };
    { let k = c.atom("type"); let v = ty; m.push((k, v)); }
    { let k = c.atom("key_length"); let v = Term::Int(key_len as i64); m.push((k, v)); }
    { let k = c.atom("iv_length"); let v = Term::Int(iv_len(mode) as i64); m.push((k, v)); }
    { let k = c.atom("block_size"); let v = Term::Int(block as i64); m.push((k, v)); }
    { let k = c.atom("prop_aead"); let v = c.bool(aead); m.push((k, v)); }
    { let k = c.atom("mode"); let v = c.atom(mode_name); m.push((k, v)); }
    Ok(c.map_from(m))
}

// ---- the block cipher ----

enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> Option<Aes> {
        Some(match key.len() {
            16 => Aes::A128(aes::Aes128::new_from_slice(key).ok()?),
            24 => Aes::A192(aes::Aes192::new_from_slice(key).ok()?),
            32 => Aes::A256(aes::Aes256::new_from_slice(key).ok()?),
            _ => return None,
        })
    }

    fn encrypt(&self, block: &mut [u8]) {
        let b = block.try_into().expect("a 16-byte block");
        match self {
            Aes::A128(k) => k.encrypt_block(b),
            Aes::A192(k) => k.encrypt_block(b),
            Aes::A256(k) => k.encrypt_block(b),
        }
    }

    fn decrypt(&self, block: &mut [u8]) {
        let b = block.try_into().expect("a 16-byte block");
        match self {
            Aes::A128(k) => k.decrypt_block(b),
            Aes::A192(k) => k.decrypt_block(b),
            Aes::A256(k) => k.decrypt_block(b),
        }
    }
}

enum Stream {
    Ctr128(ctr::Ctr128BE<aes::Aes128>),
    Ctr192(ctr::Ctr128BE<aes::Aes192>),
    Ctr256(ctr::Ctr128BE<aes::Aes256>),
    ChaCha(chacha20::ChaCha20),
    Cfb(Cfb),
}

impl Stream {
    fn apply(&mut self, data: &mut [u8]) {
        match self {
            Stream::Cfb(s) => s.apply(data),
            Stream::Ctr128(s) => s.apply_keystream(data),
            Stream::Ctr192(s) => s.apply_keystream(data),
            Stream::Ctr256(s) => s.apply_keystream(data),
            Stream::ChaCha(s) => s.apply_keystream(data),
        }
    }
}

/// AES in CFB mode: a stream cipher whose keystream is the encryption of the last ciphertext
/// (a whole block of it with 128-bit feedback, the last 16 bytes with 8-bit feedback), so it
/// must know which way it is going.
struct Cfb {
    aes: Aes,
    /// The feedback register: the IV, then ciphertext.
    register: [u8; BLOCK],
    /// With 128-bit feedback: the current block of keystream and how much of it is used.
    keystream: [u8; BLOCK],
    used: usize,
    bits8: bool,
    encrypt: bool,
}

impl Cfb {
    fn new(key: &[u8], iv: &[u8], bits8: bool, encrypt: bool) -> Option<Cfb> {
        Some(Cfb { aes: Aes::new(key)?, register: iv.try_into().ok()?, keystream: [0; BLOCK], used: BLOCK, bits8, encrypt })
    }

    fn apply(&mut self, data: &mut [u8]) {
        for b in data {
            let input = *b;
            if self.bits8 {
                let mut ks = self.register;
                self.aes.encrypt(&mut ks);
                *b ^= ks[0];
                self.register.copy_within(1.., 0);
                self.register[BLOCK - 1] = if self.encrypt { *b } else { input };
            } else {
                if self.used == BLOCK {
                    self.keystream = self.register;
                    self.aes.encrypt(&mut self.keystream);
                    self.used = 0;
                }
                *b ^= self.keystream[self.used];
                self.register[self.used] = if self.encrypt { *b } else { input };
                self.used += 1;
            }
        }
    }
}

enum Engine {
    Stream(Stream),
    /// CBC keeps the previous ciphertext block as its chaining value.
    Cbc(Aes, [u8; BLOCK]),
    Ecb(Aes),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Padding {
    Undefined,
    None,
    Pkcs,
    Zero,
    Random,
}

impl Padding {
    fn name(self) -> &'static str {
        match self {
            Padding::Undefined => "undefined",
            Padding::None => "none",
            Padding::Pkcs => "pkcs_padding",
            Padding::Zero => "zero",
            Padding::Random => "random",
        }
    }
}

/// A cipher context (the resource behind `crypto_init`). Changed in place by updates.
struct CipherCtx {
    engine: Engine,
    encrypt: bool,
    padding: Padding,
    /// Input not yet processed: less than a block (or, when decrypting with PKCS padding, up
    /// to one whole block held back until `final`).
    pending: Vec<u8>,
    /// Bytes of input seen, and of padding added (for `crypto_get_data`).
    size: usize,
    padded_size: usize,
}

/// `crypto_init` options: a boolean (encrypt), or `[{encrypt, Bool}, {padding, P}]`.
fn options(c: &mut Ctx, t: &Term, arg: i64) -> Result<(bool, Padding), Exception> {
    if is_true(c, t) {
        return Ok((true, Padding::Undefined));
    }
    if t.is_atom(&c.sys.atoms.false_) {
        return Ok((false, Padding::Undefined));
    }
    let Some(opts) = c.heap().to_vec(*t) else { return Err(badarg(c, arg, "Options are not a boolean or a proper list")) };
    let (mut encrypt, mut padding) = (true, Padding::Undefined);
    for o in opts {
        match c.heap().as_tuple(o) {
            Some(&[k, v]) if atom_name(&k) == Some("encrypt") => {
                let v = &v;
                if is_true(c, v) {
                    encrypt = true;
                } else if v.is_atom(&c.sys.atoms.false_) {
                    encrypt = false;
                } else {
                    return Err(badarg(c, arg, "Bad encrypt option"));
                }
            }
            Some(&[k, v]) if atom_name(&k) == Some("padding") => {
                padding = match atom_name(&v) {
                    Some("undefined") => Padding::Undefined,
                    Some("none") => Padding::None,
                    Some("pkcs_padding") => Padding::Pkcs,
                    Some("zero") => Padding::Zero,
                    Some("random") => Padding::Random,
                    _ => return Err(badarg(c, arg, "Bad padding option")),
                };
            }
            _ => return Err(badarg(c, arg, "Bad option")),
        }
    }
    Ok((encrypt, padding))
}

fn new_ctx(c: &mut Ctx, a: &[Term], data_opts: usize) -> Result<CipherCtx, Exception> {
    let Some((mode, key_len, _)) = lookup(&a[0]) else { return Err(badarg(c, 0, "Unknown cipher")) };
    if matches!(mode, Mode::Gcm | Mode::ChaCha20Poly1305) {
        return Err(badarg(c, 0, "Unknown cipher or invalid key size"));
    }
    let key = bytes(c, a, 1, "key")?;
    if key.len() != key_len {
        return Err(badarg(c, 1, "Bad key size"));
    }
    let iv = bytes(c, a, 2, "iv")?;
    // ECB has no IV; like OpenSSL, any given is ignored.
    if iv.len() != iv_len(mode) && mode != Mode::Ecb {
        return Err(badarg(c, 2, "Bad iv size"));
    }
    let (encrypt, padding) = options(c, &a[data_opts], data_opts as i64)?;
    let engine = match mode {
        Mode::Ctr => Engine::Stream(match key.len() {
            16 => Stream::Ctr128(ctr::Ctr128BE::new_from_slices(&key, &iv).expect("sizes checked")),
            24 => Stream::Ctr192(ctr::Ctr128BE::new_from_slices(&key, &iv).expect("sizes checked")),
            _ => Stream::Ctr256(ctr::Ctr128BE::new_from_slices(&key, &iv).expect("sizes checked")),
        }),
        Mode::ChaCha20 => {
            // OpenSSL's ChaCha20 IV: a 32-bit little-endian block counter, then a 96-bit nonce.
            let counter = u32::from_le_bytes(iv[..4].try_into().expect("16-byte iv"));
            let mut s = chacha20::ChaCha20::new_from_slices(&key, &iv[4..]).expect("sizes checked");
            s.seek(counter as u64 * 64);
            Engine::Stream(Stream::ChaCha(s))
        }
        Mode::Cfb128 | Mode::Cfb8 => {
            Engine::Stream(Stream::Cfb(Cfb::new(&key, &iv, mode == Mode::Cfb8, encrypt).expect("sizes checked")))
        }
        Mode::Cbc => Engine::Cbc(Aes::new(&key).expect("size checked"), iv.try_into().expect("size checked")),
        Mode::Ecb => Engine::Ecb(Aes::new(&key).expect("size checked")),
        Mode::Gcm | Mode::ChaCha20Poly1305 => unreachable!("rejected above"),
    };
    Ok(CipherCtx { engine, encrypt, padding, pending: Vec::new(), size: 0, padded_size: 0 })
}

impl CipherCtx {
    fn block(&mut self, block: &mut [u8]) {
        let encrypt = self.encrypt;
        match &mut self.engine {
            Engine::Ecb(k) => {
                if encrypt {
                    k.encrypt(block)
                } else {
                    k.decrypt(block)
                }
            }
            Engine::Cbc(k, chain) => {
                if encrypt {
                    block.iter_mut().zip(chain.iter()).for_each(|(b, c)| *b ^= c);
                    k.encrypt(block);
                    chain.copy_from_slice(block);
                } else {
                    let ciphertext: [u8; BLOCK] = (&*block).try_into().expect("16-byte block");
                    k.decrypt(block);
                    block.iter_mut().zip(chain.iter()).for_each(|(b, c)| *b ^= c);
                    *chain = ciphertext;
                }
            }
            Engine::Stream(_) => unreachable!("stream ciphers have no blocks"),
        }
    }

    fn update(&mut self, data: &[u8]) -> Vec<u8> {
        self.size += data.len();
        if let Engine::Stream(s) = &mut self.engine {
            let mut out = data.to_vec();
            s.apply(&mut out);
            return out;
        }
        let mut input = core::mem::take(&mut self.pending);
        input.extend_from_slice(data);
        let mut n = input.len() / BLOCK * BLOCK;
        // Decrypting with PKCS padding: keep the last whole block for `final` to unpad.
        if !self.encrypt && self.padding == Padding::Pkcs && n == input.len() && n > 0 {
            n -= BLOCK;
        }
        self.pending = input.split_off(n);
        for chunk in input.chunks_mut(BLOCK) {
            self.block(chunk);
        }
        input
    }

    fn finalize(&mut self, c: &mut Ctx) -> Result<Vec<u8>, Exception> {
        if matches!(self.engine, Engine::Stream(_)) {
            return Ok(Vec::new());
        }
        let pending = core::mem::take(&mut self.pending);
        let short = (BLOCK - pending.len() % BLOCK) % BLOCK;
        if self.encrypt {
            let fill = match self.padding {
                Padding::Undefined => {
                    self.padded_size = short;
                    return Ok(Vec::new());
                }
                Padding::None => {
                    if !pending.is_empty() {
                        return Err(nif_error(c, "error", -1, "Padding 'none' but unfilled last block"));
                    }
                    return Ok(Vec::new());
                }
                Padding::Pkcs => {
                    let n = BLOCK - pending.len();
                    alloc::vec![n as u8; n]
                }
                Padding::Zero => alloc::vec![0u8; short],
                Padding::Random => random_bytes(c, short)?,
            };
            self.padded_size = fill.len();
            let mut last = pending;
            last.extend_from_slice(&fill);
            for chunk in last.chunks_mut(BLOCK) {
                self.block(chunk);
            }
            return Ok(last);
        }
        match self.padding {
            Padding::Undefined => Ok(Vec::new()),
            Padding::Pkcs => {
                if pending.len() != BLOCK {
                    return Err(nif_error(c, "error", -1, "Can't finalize"));
                }
                let mut last = pending;
                self.block(&mut last);
                let n = last[BLOCK - 1] as usize;
                let valid = (1..=BLOCK).contains(&n) && last[BLOCK - n..].iter().all(|&b| b as usize == n);
                if !valid {
                    return Err(nif_error(c, "error", -1, "Can't finalize"));
                }
                last.truncate(BLOCK - n);
                Ok(last)
            }
            _ => {
                if !pending.is_empty() {
                    return Err(nif_error(c, "error", -1, "Can't finalize"));
                }
                Ok(Vec::new())
            }
        }
    }
}

fn ctx_arg(c: &mut Ctx, t: &Term) -> Result<beamlet_vm::bif::Held<RefCell<CipherCtx>>, Exception> {
    resource_ref::<RefCell<CipherCtx>>(c, t).ok_or_else(|| badarg(c, 0, "Bad State"))
}

/// `ng_crypto_init_nif(Cipher, Key, IVec, Options)`.
pub fn init(c: &mut Ctx, a: &[Term]) -> R {
    let ctx = new_ctx(c, a, 3)?;
    Ok(resource(c, RefCell::new(ctx)))
}

/// `ng_crypto_update_nif(State, Data)`: the output for `Data`; the state changes in place.
pub fn update(c: &mut Ctx, a: &[Term]) -> R {
    let data = bytes(c, a, 1, "data")?;
    let ctx = ctx_arg(c, &a[0])?;
    let out = ctx.borrow_mut().update(&data);
    Ok(bin(c, &out))
}

pub fn finalize(c: &mut Ctx, a: &[Term]) -> R {
    let ctx = ctx_arg(c, &a[0])?;
    let mut state = ctx.borrow_mut();
    let out = state.finalize(c)?;
    Ok(bin(c, &out))
}

pub fn get_data(c: &mut Ctx, a: &[Term]) -> R {
    let ctx = ctx_arg(c, &a[0])?;
    let (size, padded, padding, encrypt) = {
        let s = ctx.borrow();
        (s.size, s.padded_size, s.padding, s.encrypt)
    };
    let mut m: Vec<(Term, Term)> = Vec::new();
    { let k = c.atom("size"); let v = Term::Int(size as i64); m.push((k, v)); }
    { let k = c.atom("padding_size"); let v = Term::Int(padded as i64); m.push((k, v)); }
    { let k = c.atom("padding_type"); let v = c.atom(padding.name()); m.push((k, v)); }
    { let k = c.atom("encrypt"); let v = c.bool(encrypt); m.push((k, v)); }
    Ok(c.map_from(m))
}

/// `ng_crypto_one_time_nif(Cipher, Key, IVec, Data, Options)`: update and final in one.
pub fn one_time(c: &mut Ctx, a: &[Term]) -> R {
    let mut ctx = new_ctx(c, a, 4)?;
    let data = bytes(c, a, 3, "data")?;
    let mut out = ctx.update(&data);
    out.extend(ctx.finalize(c)?);
    Ok(bin(c, &out))
}

// ---- AEAD ----

fn aead_seal(mode: Mode, key: &[u8], iv: &[u8], aad: &[u8], data: &mut [u8]) -> Option<[u8; 16]> {
    use aes_gcm::aead::AeadInOut;
    let tag = match (mode, key.len()) {
        (Mode::Gcm, 16) => aes_gcm::Aes128Gcm::new_from_slice(key).ok()?.encrypt_inout_detached(iv.try_into().ok()?, aad, data.into()).ok()?.into(),
        (Mode::Gcm, 24) => aes_gcm::AesGcm::<aes::Aes192, aes_gcm::aead::consts::U12>::new_from_slice(key)
            .ok()?
            .encrypt_inout_detached(iv.try_into().ok()?, aad, data.into())
            .ok()?
            .into(),
        (Mode::Gcm, 32) => aes_gcm::Aes256Gcm::new_from_slice(key).ok()?.encrypt_inout_detached(iv.try_into().ok()?, aad, data.into()).ok()?.into(),
        (Mode::ChaCha20Poly1305, 32) => chacha20poly1305::ChaCha20Poly1305::new_from_slice(key)
            .ok()?
            .encrypt_inout_detached(iv.try_into().ok()?, aad, data.into())
            .ok()?
            .into(),
        _ => return None,
    };
    Some(tag)
}

/// Decrypt in place; `false` if the tag does not authenticate.
fn aead_open(mode: Mode, key: &[u8], iv: &[u8], aad: &[u8], data: &mut [u8], tag: &[u8; 16]) -> bool {
    use aes_gcm::aead::AeadInOut;
    let (Ok(nonce), tag) = (<&[u8; 12]>::try_from(iv), tag.into()) else { return false };
    let r = match (mode, key.len()) {
        (Mode::Gcm, 16) => aes_gcm::Aes128Gcm::new_from_slice(key).map(|k| k.decrypt_inout_detached(nonce.into(), aad, data.into(), tag)),
        (Mode::Gcm, 24) => aes_gcm::AesGcm::<aes::Aes192, aes_gcm::aead::consts::U12>::new_from_slice(key)
            .map(|k| k.decrypt_inout_detached(nonce.into(), aad, data.into(), tag)),
        (Mode::Gcm, 32) => aes_gcm::Aes256Gcm::new_from_slice(key).map(|k| k.decrypt_inout_detached(nonce.into(), aad, data.into(), tag)),
        (Mode::ChaCha20Poly1305, 32) => {
            chacha20poly1305::ChaCha20Poly1305::new_from_slice(key).map(|k| k.decrypt_inout_detached(nonce.into(), aad, data.into(), tag))
        }
        _ => return false,
    };
    matches!(r, Ok(Ok(())))
}

fn aead_cipher(c: &mut Ctx, t: &Term) -> Result<Mode, Exception> {
    match lookup(t) {
        Some((m @ (Mode::Gcm | Mode::ChaCha20Poly1305), ..)) => Ok(m),
        Some(_) => Err(badarg(c, 0, "Not aead cipher")),
        None => Err(notsup(c, 0, "Unknown cipher or invalid key size")),
    }
}

/// Encrypt (tag of `tag_len` bytes) or decrypt (checking `tag`). Decryption failure is the
/// atom `error`, as in OTP.
fn aead_run(c: &mut Ctx, mode: Mode, key: &[u8], iv: &[u8], data: Vec<u8>, aad: &[u8], enc: Result<usize, Vec<u8>>) -> Result<(Vec<u8>, Option<Vec<u8>>), Exception> {
    if iv.len() != 12 {
        return Err(notsup(c, 2, "Unsupported IV length"));
    }
    let mut data = data;
    match enc {
        Ok(tag_len) => {
            if !(1..=16).contains(&tag_len) {
                return Err(badarg(c, 5, "Bad tag length"));
            }
            let tag = aead_seal(mode, key, iv, aad, &mut data).ok_or_else(|| badarg(c, 1, "Bad key size"))?;
            Ok((data, Some(tag[..tag_len].to_vec())))
        }
        Err(tag) => {
            let Ok(full) = <[u8; 16]>::try_from(&tag[..]) else {
                return Err(notsup(c, 5, "Only 16-byte tags can be verified"));
            };
            if aead_open(mode, key, iv, aad, &mut data, &full) {
                Ok((data, None))
            } else {
                Ok((Vec::new(), Some(Vec::new())))
            }
        }
    }
}

/// `aead_cipher_nif(Type, Key, IV, In, AAD, TagOrTagLength, EncFlg)`.
pub fn aead_one_time(c: &mut Ctx, a: &[Term]) -> R {
    let mode = aead_cipher(c, &a[0])?;
    let key = bytes(c, a, 1, "key")?;
    let iv = bytes(c, a, 2, "iv")?;
    let input = bytes(c, a, 3, "text")?;
    let aad = bytes(c, a, 4, "AAD")?;
    let encrypt = is_true(c, &a[6]);
    let enc = if encrypt {
        Ok(a[5].as_usize().ok_or_else(|| badarg(c, 5, "Bad tag length"))?)
    } else {
        Err(bytes(c, a, 5, "tag")?)
    };
    match aead_run(c, mode, &key, &iv, input, &aad, enc)? {
        (out, Some(tag)) if encrypt => Ok({ let e = [bin(c, &out), bin(c, &tag)]; c.tuple(&e) }),
        (_, Some(_)) => Ok(Term::Atom(c.sys.atoms.error.clone())),
        (out, None) => Ok(bin(c, &out)),
    }
}

struct AeadState {
    mode: Mode,
    key: Vec<u8>,
    tag_len: usize,
    encrypt: bool,
}

/// `aead_cipher_init_nif(Type, Key, TagLength, EncFlg)`.
pub fn aead_init(c: &mut Ctx, a: &[Term]) -> R {
    let mode = aead_cipher(c, &a[0])?;
    let key = bytes(c, a, 1, "key")?;
    let tag_len = a[2].as_usize().filter(|l| (1..=16).contains(l)).ok_or_else(|| badarg(c, 2, "Bad tag length"))?;
    let encrypt = is_true(c, &a[3]);
    Ok(resource(c, AeadState { mode, key, tag_len, encrypt }))
}

/// `aead_cipher_nif(State, IV, In, AAD)`: encrypting returns ciphertext followed by the tag;
/// decrypting expects the tag at the end of `In`.
pub fn aead_with_state(c: &mut Ctx, a: &[Term]) -> R {
    let Some(st) = resource_ref::<AeadState>(c, &a[0]) else { return Err(badarg(c, 0, "Bad state")) };
    let (mode, key, tag_len, encrypt) = (st.mode, st.key.clone(), st.tag_len, st.encrypt);
    let iv = bytes(c, a, 1, "iv")?;
    let mut input = bytes(c, a, 2, "text")?;
    let aad = bytes(c, a, 3, "AAD")?;
    if encrypt {
        let (mut out, tag) = aead_run(c, mode, &key, &iv, input, &aad, Ok(tag_len))?;
        out.extend(tag.unwrap_or_default());
        return Ok(bin(c, &out));
    }
    if input.len() < tag_len {
        return Ok(Term::Atom(c.sys.atoms.error.clone()));
    }
    let tag = input.split_off(input.len() - tag_len);
    match aead_run(c, mode, &key, &iv, input, &aad, Err(tag))? {
        (out, None) => Ok(bin(c, &out)),
        _ => Ok(Term::Atom(c.sys.atoms.error.clone())),
    }
}
