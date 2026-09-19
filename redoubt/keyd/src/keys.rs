//! The keys `keyd` holds, and the purposes they may be used for (INIT.md, keyd).
//!
//! **Where they come from.** The boot manifest, one key per argument of `keyd`'s `servers`
//! entry, which reaches the process in its startup block. Milestone 1 generates no key on the
//! box, and there is no operation to add, replace or remove one, so `keyd` holds exactly what
//! the signed bundle gave it and nothing else, for as long as it runs.
//!
//! **What a purpose is.** Not a label on a signature: the *one message shape* a badge may ask
//! for, built by `keyd`. A badge with [`Purpose::SshHost`] can ask only for a signature over an
//! SSH exchange hash that `keyd` computed itself from a transcript naming `keyd`'s own public
//! key; a badge with [`Purpose::Audit`] can ask only for a signature over an audit record under
//! a fixed domain string. Neither can ask for a signature over bytes of the caller's choosing,
//! which is what makes a stolen badge useless as a signature oracle (CAPABILITIES.md, Agents 7;
//! answer 95).
//!
//! **Constant time.** Nothing here branches or indexes on key material. Parsing decodes hex with
//! arithmetic rather than comparisons, comparisons of key bytes are accumulated, and the
//! signature itself is `ed25519-compact`'s (see [`Key::sign`]).

use alloc::string::String;
use alloc::vec::Vec;

use ed25519_compact::{KeyPair, Seed};
use redoubt_rt::startup::valid_name;

/// The most keys one `keyd` holds. The manifest names a handful (a host key, an audit key, a
/// signing key per principal); the bound is here so a hostile manifest cannot make `keyd`'s
/// badge space or its startup work unbounded.
pub const MAX_KEYS: usize = 16;

/// Ed25519 (RFC 8032): the one signature scheme (VERIFIED-BOOT.md, PACKAGES.md).
pub const ALGORITHM: &str = "ssh-ed25519";
/// A public key, in bytes.
pub const PUBLIC_KEY_LEN: usize = 32;
/// A secret seed, in bytes, and as hex digits in an argument.
pub const SEED_LEN: usize = 32;
pub const SEED_HEX_LEN: usize = SEED_LEN * 2;
/// A signature, in bytes.
pub const SIGNATURE_LEN: usize = 64;

/// The field separator in a key argument: outside the manifest's name rule
/// (`[a-z0-9_:+-]`), so no field can swallow the next one.
const SEPARATOR: char = ',';

/// What a badge on a key may ask for. One purpose, one message shape; there is no purpose that
/// signs a caller's bytes as they came, and none that authenticates a person to the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// The box's SSH host key: `sign_ssh_exchange` only.
    SshHost,
    /// The steward's audit key: `sign_record` only.
    Audit,
}

impl Purpose {
    pub fn from_name(name: &str) -> Option<Purpose> {
        match name {
            "ssh_host" => Some(Purpose::SshHost),
            "audit" => Some(Purpose::Audit),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Purpose::SshHost => "ssh_host",
            Purpose::Audit => "audit",
        }
    }
}

/// Why `keyd` refused a key argument. Every one of them stops the process starting: a key it
/// cannot read is a key it cannot sign with, and limping on would mean serving requests for a
/// key that is silently missing (TENETS.md 2, fail closed and loudly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// Not `name,purpose,seed`.
    BadArgument,
    /// The name breaks the manifest's rule (INIT.md, Names).
    BadName,
    /// A purpose outside [`Purpose`]. There is deliberately no purpose for a key that
    /// authenticates a person to the box (CAPABILITIES.md, approvals).
    UnknownPurpose,
    /// The seed is not exactly [`SEED_HEX_LEN`] lower-case hex digits.
    BadSeed,
    /// The seed is all zero bytes. `ed25519-compact`'s `KeyPair::from_seed` **panics** on that
    /// one value, so it is refused here before it reaches the crate: a dependency that panics
    /// on an input is a denial-of-service bug in us (TENETS.md 5), and a manifest typo must
    /// give a refusal with a reason, not a crash in a key derivation.
    ZeroSeed,
    /// Two keys with the same name, or two with the same public key.
    Duplicate,
    /// More than [`MAX_KEYS`], or no memory for one more.
    TooMany,
}

/// One key: what it is called, what it may be used for, and the key pair itself.
pub struct Key {
    name: String,
    purpose: Purpose,
    pair: KeyPair,
}

impl Key {
    pub fn name(&self) -> &str { &self.name }

    pub fn purpose(&self) -> Purpose { self.purpose }

    /// The public half. Public by definition: the host key goes to every client that connects.
    pub fn public(&self) -> &[u8; PUBLIC_KEY_LEN] { &self.pair.pk }

    /// Signs `message` with the secret half, and returns the raw 64-byte signature.
    ///
    /// **Constant time** rests on `ed25519-compact` 2.4.2, read at this version
    /// (`src/ed25519.rs`, `src/edwards25519.rs`, `src/field25519.rs`, `src/sha512.rs`):
    /// - the only place a secret scalar meets a curve operation is `ge_scalarmult_base`, a fixed 64-iteration
    ///   loop that scans all sixteen precomputed points every time and selects one with an arithmetic mask
    ///   (`Fe::maybe_set`: `self ^= mask & (self ^ other)`), so there is no secret-dependent branch and no
    ///   secret-dependent memory index;
    /// - the two secret scalars (the expanded key and the nonce) are also operands of `sc_muladd`, and
    ///   `sc_muladd` and `sc_reduce` are straight-line 64-bit arithmetic over fixed limbs;
    /// - the field arithmetic is fiat-crypto's generated, formally verified code, which is branch-free by
    ///   construction;
    /// - SHA-512's only branches are on how many bytes are buffered, which is a length, and the lengths here
    ///   are public.
    ///
    /// The variable-time code in that crate (`slide`, `ge_double_scalarmult_vartime`, point
    /// decompression) is reached only from *verification*, and `keyd` never verifies: no
    /// operation in its protocol does.
    ///
    /// `None` for the noise argument means deterministic RFC 8032, which is what makes
    /// signatures testable against the RFC's own vectors. Noise guards against fault
    /// injection, which is a physical attack, and TENETS.md puts physical attacks out of scope
    /// for the software.
    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] { *self.pair.sk.sign(message, None) }
}

/// Every key `keyd` holds, in the order the manifest gave them. The key in position `i` has
/// root badge `i + 1` (INIT.md), so a restarted `keyd` gives the same badge the same meaning
/// without keeping anything across the restart.
pub struct Keys {
    keys: Vec<Key>,
}

impl Keys {
    /// Reads the key arguments, in order. Refuses the whole set on the first bad one.
    pub fn from_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Keys, KeyError> {
        let mut keys: Vec<Key> = Vec::new();
        for arg in args {
            if keys.len() >= MAX_KEYS {
                return Err(KeyError::TooMany);
            }
            let key = parse(arg)?;
            let clash = keys.iter().any(|k| k.name == key.name || ct_eq(k.public(), key.public()));
            if clash {
                return Err(KeyError::Duplicate);
            }
            keys.try_reserve(1).map_err(|_| KeyError::TooMany)?;
            keys.push(key);
        }
        Ok(Keys { keys })
    }

    pub fn len(&self) -> usize { self.keys.len() }

    pub fn is_empty(&self) -> bool { self.keys.is_empty() }

    pub fn get(&self, index: usize) -> Option<&Key> { self.keys.get(index) }

    /// The key a root badge names: badges 1..=n are the keys in argument order (INIT.md).
    /// Badge 0 is the receive right and names no key.
    pub fn by_root_badge(&self, badge: u64) -> Option<usize> {
        let index = badge.checked_sub(1)?;
        (index < self.keys.len() as u64).then_some(index as usize)
    }

    /// The labels of the key in `index`. Milestone 1 gives `keyd`'s keys no labels: the
    /// manifest has no field for them, so every key is unlabelled, which `check` turns into
    /// "anyone may read a public key, only an unlabelled caller may sign" (INIT.md).
    pub fn labels(&self, _index: usize) -> &[u64] { &[] }

    /// Whether any key here has this public key: what `sshd` asks before accepting a login key,
    /// and what `init` and the steward ask before enrolling one (CAPABILITIES.md, approvals).
    /// Every key is compared, without a short circuit, so the answer takes the same work
    /// whichever key matched.
    pub fn holds(&self, algorithm: &str, public: &[u8]) -> bool {
        if algorithm != ALGORITHM || public.len() != PUBLIC_KEY_LEN {
            return false;
        }
        let mut found = 0u8;
        for key in &self.keys {
            found |= u8::from(ct_eq(key.public(), public));
        }
        found != 0
    }
}

/// `name,purpose,seed`.
fn parse(arg: &str) -> Result<Key, KeyError> {
    let mut fields = arg.split(SEPARATOR);
    let (Some(name), Some(purpose), Some(seed), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(KeyError::BadArgument);
    };
    if !valid_name(name) {
        return Err(KeyError::BadName);
    }
    let purpose = Purpose::from_name(purpose).ok_or(KeyError::UnknownPurpose)?;
    let seed = hex_seed(seed)?;
    // Accumulated, not short-circuited: the seed is the secret.
    if seed.iter().fold(0u8, |all, byte| all | byte) == 0 {
        return Err(KeyError::ZeroSeed);
    }
    let mut owned = String::new();
    owned.try_reserve(name.len()).map_err(|_| KeyError::TooMany)?;
    owned.push_str(name);
    Ok(Key { name: owned, purpose, pair: KeyPair::from_seed(Seed::new(seed)) })
}

/// Exactly [`SEED_HEX_LEN`] lower-case hex digits, decoded without branching on their values:
/// the seed is the secret itself, and a decoder that took a different path for `'a'` than for
/// `'0'` would be a timing channel on it, however small.
fn hex_seed(text: &str) -> Result<[u8; SEED_LEN], KeyError> {
    let digits = text.as_bytes();
    if digits.len() != SEED_HEX_LEN {
        return Err(KeyError::BadSeed);
    }
    let mut seed = [0u8; SEED_LEN];
    let mut bad = 0u8;
    for (byte, pair) in seed.iter_mut().zip(digits.chunks_exact(2)) {
        let (high, ok_high) = nibble(pair[0]);
        let (low, ok_low) = nibble(pair[1]);
        *byte = (high << 4) | low;
        bad |= !(ok_high & ok_low);
    }
    if bad != 0 {
        return Err(KeyError::BadSeed);
    }
    Ok(seed)
}

/// One hex digit's value, and 0xff if it was one. `'0'..='9'` and `'a'..='f'` only: upper case
/// is refused, so a seed has exactly one spelling.
fn nibble(c: u8) -> (u8, u8) {
    let digit = ge(c, b'0') & ge(b'9', c);
    let lower = ge(c, b'a') & ge(b'f', c);
    let value = (digit & c.wrapping_sub(b'0')) | (lower & c.wrapping_sub(b'a').wrapping_add(10));
    (value, digit | lower)
}

/// 0xff if `a >= b`, 0 otherwise, without a branch: the difference of the two as 16-bit numbers
/// borrows into the high byte exactly when `a < b`.
fn ge(a: u8, b: u8) -> u8 {
    let borrowed = (u16::from(a).wrapping_sub(u16::from(b)) >> 8) as u8;
    !borrowed
}

/// Whether two byte strings of the same length are equal, in time that depends only on that
/// length.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (x, y) in a.iter().zip(b) {
        difference |= x ^ y;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;

    use super::*;

    pub(crate) const HOST_SEED: &str = "000000000000000000000000000000000000000000000000000000000000004a";
    pub(crate) const AUDIT_SEED: &str = "00000000000000000000000000000000000000000000000000000000000000b5";

    fn keys() -> Keys {
        Keys::from_args(
            [format!("host,ssh_host,{HOST_SEED}"), format!("audit,audit,{AUDIT_SEED}")]
                .iter()
                .map(|s| s.as_str()),
        )
        .unwrap()
    }

    #[test]
    fn arguments_become_keys_with_root_badges_in_order() {
        let keys = keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.by_root_badge(1), Some(0));
        assert_eq!(keys.by_root_badge(2), Some(1));
        assert_eq!(keys.by_root_badge(3), None);
        assert_eq!(keys.by_root_badge(0), None, "badge 0 is the receive right");
        assert_eq!(keys.by_root_badge(u64::MAX), None);
        assert_eq!(keys.get(0).unwrap().name(), "host");
        assert_eq!(keys.get(0).unwrap().purpose(), Purpose::SshHost);
        assert_eq!(keys.get(1).unwrap().purpose(), Purpose::Audit);
    }

    /// Every way an argument can be wrong, and the reason given. A `keyd` that started anyway
    /// would be a `keyd` serving a key nobody put there.
    #[test]
    fn hostile_arguments_are_refused() {
        let one = |arg: &str| Keys::from_args([arg].into_iter()).err();
        assert_eq!(one(&format!("host,ssh_host,{HOST_SEED}")), None);
        assert_eq!(one(""), Some(KeyError::BadArgument));
        assert_eq!(one("host,ssh_host"), Some(KeyError::BadArgument));
        assert_eq!(one(&format!("host,ssh_host,{HOST_SEED},extra")), Some(KeyError::BadArgument));
        assert_eq!(one(&format!("Host,ssh_host,{HOST_SEED}")), Some(KeyError::BadName));
        assert_eq!(one(&format!(",ssh_host,{HOST_SEED}")), Some(KeyError::BadName));
        assert_eq!(one(&format!("a b,ssh_host,{HOST_SEED}")), Some(KeyError::BadName));
        // No purpose authenticates a person to the box, and nothing outside the table is a
        // purpose at all.
        for purpose in ["", "login", "ssh_user_auth", "approval", "SSH_HOST", "sign", "any"] {
            assert_eq!(
                one(&format!("host,{purpose},{HOST_SEED}")),
                Some(KeyError::UnknownPurpose),
                "{purpose}"
            );
        }
        // Seeds: exactly 64 lower-case hex digits, one spelling only.
        for seed in [
            "".to_string(),
            "00".to_string(),
            HOST_SEED[1..].to_string(),
            format!("{HOST_SEED}0"),
            HOST_SEED.to_uppercase(),
            format!("{}zz", &HOST_SEED[2..]),
            format!("{} ", &HOST_SEED[1..]),
        ] {
            assert_eq!(one(&format!("host,ssh_host,{seed}")), Some(KeyError::BadSeed), "{seed:?}");
        }
    }

    /// The one seed value `ed25519-compact` panics on, refused with a reason instead.
    #[test]
    fn an_all_zero_seed_is_refused_rather_than_panicking() {
        let zero = "0".repeat(SEED_HEX_LEN);
        assert_eq!(
            Keys::from_args([format!("k,audit,{zero}")].iter().map(|s| s.as_str())).err(),
            Some(KeyError::ZeroSeed)
        );
        // One bit anywhere is enough for a key.
        let mut nearly = alloc::vec![b'0'; SEED_HEX_LEN];
        nearly[SEED_HEX_LEN - 1] = b'1';
        let nearly = core::str::from_utf8(&nearly).unwrap();
        assert!(Keys::from_args([format!("k,audit,{nearly}")].iter().map(|s| s.as_str())).is_ok());
    }

    #[test]
    fn two_keys_may_not_share_a_name_or_a_public_key() {
        let same_name = [format!("k,ssh_host,{HOST_SEED}"), format!("k,audit,{AUDIT_SEED}")];
        assert_eq!(Keys::from_args(same_name.iter().map(|s| s.as_str())).err(), Some(KeyError::Duplicate));
        // The same seed under two names is the same key twice: one public key, two purposes,
        // which would let a badge for one purpose be checked against the other's key.
        let same_key = [format!("a,ssh_host,{HOST_SEED}"), format!("b,audit,{HOST_SEED}")];
        assert_eq!(Keys::from_args(same_key.iter().map(|s| s.as_str())).err(), Some(KeyError::Duplicate));
    }

    #[test]
    fn there_is_a_bound_on_how_many_keys_there_are() {
        let args: vec::Vec<String> = (0..=MAX_KEYS).map(|i| format!("k{i},audit,{:064x}", i + 1)).collect();
        assert_eq!(Keys::from_args(args.iter().map(|s| s.as_str())).err(), Some(KeyError::TooMany));
        let args = &args[..MAX_KEYS];
        assert!(Keys::from_args(args.iter().map(|s| s.as_str())).is_ok());
    }

    /// RFC 8032 §7.1, test vector 1: the seed, its public key and the signature over the empty
    /// message. This is the check that `keyd` really produces Ed25519 and not something that
    /// merely looks like it.
    #[test]
    fn signatures_are_rfc_8032_ed25519() {
        let seed = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
        let keys = Keys::from_args([format!("k,audit,{seed}")].iter().map(|s| s.as_str())).unwrap();
        let key = keys.get(0).unwrap();
        let hex = |b: &[u8]| {
            use core::fmt::Write as _;
            let mut s = String::new();
            for byte in b {
                let _ = write!(s, "{byte:02x}");
            }
            s
        };
        assert_eq!(hex(key.public()), "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        assert_eq!(
            hex(&key.sign(b"")),
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
    }

    #[test]
    fn holds_answers_only_about_keys_that_are_here() {
        let keys = keys();
        assert!(keys.holds(ALGORITHM, keys.get(0).unwrap().public()));
        assert!(keys.holds(ALGORITHM, keys.get(1).unwrap().public()));
        let mut other = *keys.get(0).unwrap().public();
        other[0] ^= 1;
        assert!(!keys.holds(ALGORITHM, &other));
        // Another algorithm, or the wrong length, is simply not held.
        assert!(!keys.holds("ssh-rsa", keys.get(0).unwrap().public()));
        assert!(!keys.holds(ALGORITHM, &other[..31]));
        assert!(!keys.holds(ALGORITHM, &[]));
    }

    /// The hex decoder is the one place a secret's *bytes* are read while parsing; it must
    /// agree with the obvious branching decoder on every input, including every bad one.
    #[test]
    fn hex_decoding_matches_the_obvious_decoder() {
        let plain = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        for c in 0..=255u8 {
            let (value, ok) = nibble(c);
            match plain(c) {
                Some(want) => assert_eq!((value, ok), (want, 0xff), "{c:#04x}"),
                None => assert_eq!(ok, 0, "{c:#04x}"),
            }
            assert_eq!(ge(c, c), 0xff);
        }
        assert_eq!(ge(0, 1), 0);
        assert_eq!(ge(255, 0), 0xff);
        assert_eq!(ge(0, 255), 0);
        let all = "0123456789abcdef".repeat(4);
        assert_eq!(hex_seed(&all).unwrap()[..8], [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
    }

    #[test]
    fn ct_eq_is_equality() {
        assert!(ct_eq(b"", b""));
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"", b"a"));
    }
}
