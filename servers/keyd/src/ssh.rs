//! The SSH exchange hash (RFC 4253 §8, with `curve25519-sha256`: RFC 8731), which is also the
//! session identifier.
//!
//! This is the shape of the whole `keyd` design in one function. The SSH server is the caller;
//! it knows every part of the transcript and computes this same hash for its own key
//! derivation. It does **not** hand `keyd` the hash, and `keyd` does not sign what it is handed:
//! `keyd` builds the hash itself, from the parts, with the host key blob taken from its own key
//! rather than from the request. So a holder of the badge — a hijacked `sshd`, or an agent whose
//! lease named the key — cannot get a signature over bytes it chose, and in particular cannot
//! relay someone else's SSH user-authentication request and get it signed (servers/keyd.md R44).
//!
//! Why it cannot be one anyway: what is signed here is always exactly 32 bytes, the SHA-256
//! output. An SSH user-authentication signature is over `string session_id`, a byte, then the
//! user name, service and key — at least 36 bytes before the user name starts. No 32-byte
//! string is one, whatever the transcript was.
//!
//! What a holder of this badge *can* do, stated so the guarantee is not read as wider than it
//! is: complete an SSH key exchange as this box, with any peer, for as long as it holds the
//! capability. That is what a host-key capability is for, and it is why the steward grants one
//! only to `sshd`; `keys` in a lease names the key the approval named (servers/steward.md,
//! "Leases"), and an approval for the host key is an approval to speak as the box.

use crate::keys::{ALGORITHM, PUBLIC_KEY_LEN};
use crate::sha256::{self, Sha256};

/// The longest a single transcript part may be. Everything arrives in one lend, so the wire
/// already bounds the whole request; this bounds each part as well, so the work of one request
/// is bounded by something stated rather than by the buffer that happened to carry it.
pub const MAX_PART: usize = 16 * 1024;

/// The parts of the transcript the caller knows. `k_s`, the host key blob, is missing on
/// purpose: it is [`host_key_blob`], from `keyd`'s own key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transcript<'a> {
    /// The client's identification string.
    pub v_c: &'a [u8],
    /// The server's identification string.
    pub v_s: &'a [u8],
    /// The client's `SSH_MSG_KEXINIT` payload.
    pub i_c: &'a [u8],
    /// The server's `SSH_MSG_KEXINIT` payload.
    pub i_s: &'a [u8],
    /// The client's ephemeral public key.
    pub q_c: &'a [u8],
    /// The server's ephemeral public key.
    pub q_s: &'a [u8],
    /// The shared secret, already in `mpint` body form. The caller holds it and encodes it: an
    /// `mpint`'s length depends on how many leading zero bytes the secret has, so encoding it
    /// here would make `keyd`'s work depend on the secret's value.
    pub k: &'a [u8],
}

/// The ephemeral public keys of `curve25519-sha256` (RFC 8731), in bytes. The hash being
/// SHA-256 already pins the key exchange to that one; checking the length here means a
/// transcript that is not one of its is refused rather than hashed.
pub const EPHEMERAL_LEN: usize = 32;

/// Why a transcript was not hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadTranscript {
    /// A part was longer than [`MAX_PART`]: a bound on the work of one request, not a shape.
    TooLong,
    /// Not a shape RFC 8731 could have produced: an empty part, or an ephemeral key that is not
    /// [`EPHEMERAL_LEN`] bytes. Nothing in an SSH key exchange is empty — the identification
    /// strings begin `SSH-2.0-`, the `KEXINIT` payloads carry the algorithm lists, and `K` is a
    /// shared secret whose `mpint` body is empty only for zero, which no honest exchange
    /// produces. Refusing these is what stops a caller having a hash over nothing signed: the
    /// transcript `keyd` hashes is one a real exchange could have made.
    BadShape,
}

/// The host key blob SSH names `K_S`: `string "ssh-ed25519" || string <public key>`, 51 bytes.
pub const HOST_KEY_BLOB_LEN: usize = 4 + 11 + 4 + PUBLIC_KEY_LEN;

/// `K_S` for `public`. Built here, never taken from a request.
pub fn host_key_blob(public: &[u8; PUBLIC_KEY_LEN]) -> [u8; HOST_KEY_BLOB_LEN] {
    let mut blob = [0u8; HOST_KEY_BLOB_LEN];
    let name = ALGORITHM.as_bytes();
    blob[0..4].copy_from_slice(&(name.len() as u32).to_be_bytes());
    blob[4..4 + name.len()].copy_from_slice(name);
    let at = 4 + name.len();
    blob[at..at + 4].copy_from_slice(&(PUBLIC_KEY_LEN as u32).to_be_bytes());
    blob[at + 4..].copy_from_slice(public);
    blob
}

/// The exchange hash: `SHA-256(string V_C || string V_S || string I_C || string I_S ||
/// string K_S || string Q_C || string Q_S || mpint K)`.
///
/// The `mpint` is written as SSH writes one: a 32-bit length and the body the caller gave.
pub fn exchange_hash(
    transcript: &Transcript<'_>,
    public: &[u8; PUBLIC_KEY_LEN],
) -> Result<[u8; sha256::DIGEST], BadTranscript> {
    let parts = [
        transcript.v_c,
        transcript.v_s,
        transcript.i_c,
        transcript.i_s,
        transcript.q_c,
        transcript.q_s,
        transcript.k,
    ];
    if parts.iter().any(|part| part.len() > MAX_PART) {
        return Err(BadTranscript::TooLong);
    }
    let shape = parts.iter().all(|part| !part.is_empty())
        && transcript.q_c.len() == EPHEMERAL_LEN
        && transcript.q_s.len() == EPHEMERAL_LEN;
    if !shape {
        return Err(BadTranscript::BadShape);
    }
    let blob = host_key_blob(public);
    let mut hash = Sha256::new();
    // In RFC 4253's order: K_S sits fifth, between I_S and Q_C.
    for part in &parts[..4] {
        length_prefixed(&mut hash, part);
    }
    length_prefixed(&mut hash, &blob);
    for part in &parts[4..] {
        length_prefixed(&mut hash, part);
    }
    Ok(hash.finish())
}

/// One SSH `string` (and one `mpint`, which has the same framing once its body is settled): a
/// 32-bit big-endian length, then the bytes. The length fits: every part was checked against
/// [`MAX_PART`], and the blob is [`HOST_KEY_BLOB_LEN`].
fn length_prefixed(hash: &mut Sha256, bytes: &[u8]) {
    hash.update(&(bytes.len() as u32).to_be_bytes());
    hash.update(bytes);
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn sample() -> Transcript<'static> {
        Transcript {
            v_c: b"SSH-2.0-client",
            v_s: b"SSH-2.0-redoubt",
            i_c: b"client kexinit",
            i_s: b"server kexinit",
            q_c: &[1; 32],
            q_s: &[2; 32],
            k: &[3; 32],
        }
    }

    const PUBLIC: [u8; PUBLIC_KEY_LEN] = [0xab; PUBLIC_KEY_LEN];

    /// The bytes hashed are exactly RFC 4253 §8's, in its order, with `K_S` from the key: spelt
    /// out here by hand, so a change to the layout has to be a change to this test too.
    #[test]
    fn the_preimage_is_the_rfc_4253_transcript() {
        let t = sample();
        let mut want: Vec<u8> = Vec::new();
        let mut put = |bytes: &[u8]| {
            want.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            want.extend_from_slice(bytes);
        };
        put(t.v_c);
        put(t.v_s);
        put(t.i_c);
        put(t.i_s);
        put(&host_key_blob(&PUBLIC));
        put(t.q_c);
        put(t.q_s);
        put(t.k);
        assert_eq!(exchange_hash(&t, &PUBLIC).unwrap(), crate::sha256::hash(&want));
        // And the blob itself.
        let blob = host_key_blob(&PUBLIC);
        assert_eq!(&blob[..4], &[0, 0, 0, 11]);
        assert_eq!(&blob[4..15], b"ssh-ed25519");
        assert_eq!(&blob[15..19], &[0, 0, 0, 32]);
        assert_eq!(&blob[19..], &PUBLIC);
    }

    /// A vector computed by an implementation that is not this one: Python's `hashlib.sha256`
    /// over the RFC 4253 §8 transcript, assembled by hand there. It pins the framing and the
    /// hash together, so neither can drift without this failing.
    #[test]
    fn the_hash_matches_an_independent_implementation() {
        let want = "eb4eee0e48a9748231a85d02dcf2f56f394ede4a1a13599b0aae288724af202e";
        let got = exchange_hash(&sample(), &PUBLIC).unwrap();
        let digits = b"0123456789abcdef";
        let hex: alloc::string::String = got
            .iter()
            .flat_map(|b| [digits[usize::from(b >> 4)] as char, digits[usize::from(b & 15)] as char])
            .collect();
        assert_eq!(hex, want);
    }

    /// The host key is `keyd`'s, not the caller's: two keys never produce the same hash from
    /// the same transcript, so nobody can have a transcript naming another host signed.
    #[test]
    fn the_host_key_comes_from_keyd() {
        let t = sample();
        let mut other = PUBLIC;
        other[0] ^= 1;
        assert_ne!(exchange_hash(&t, &PUBLIC).unwrap(), exchange_hash(&t, &other).unwrap());
    }

    /// Every part is length-prefixed, so moving bytes from the end of one into the start of the
    /// next changes the hash: a caller cannot slide the boundary to forge a transcript.
    #[test]
    fn parts_cannot_be_slid_into_each_other() {
        let base = Transcript { v_c: b"abcd", v_s: b"efgh", ..sample() };
        let slid = Transcript { v_c: b"abc", v_s: b"defgh", ..sample() };
        assert_ne!(exchange_hash(&base, &PUBLIC).unwrap(), exchange_hash(&slid, &PUBLIC).unwrap());
        // The same across the boundary `K_S` sits on, which the caller does not supply: moving
        // bytes from `I_S` into `Q_C` is not open to it, but shortening `I_S` still shows.
        let short = Transcript { i_s: b"server kexini", ..sample() };
        assert_ne!(exchange_hash(&short, &PUBLIC).unwrap(), exchange_hash(&sample(), &PUBLIC).unwrap());
    }

    #[test]
    fn one_requests_work_is_bounded() {
        let long = vec![0u8; MAX_PART + 1];
        assert_eq!(
            exchange_hash(&Transcript { i_c: &long, ..sample() }, &PUBLIC),
            Err(BadTranscript::TooLong)
        );
        let ok = vec![0u8; MAX_PART];
        assert!(exchange_hash(&Transcript { i_c: &ok, ..sample() }, &PUBLIC).is_ok());
    }

    /// A transcript no key exchange could have produced is refused rather than hashed: the red
    /// team's all-empty one, and an ephemeral key that is not a Curve25519 point's length.
    #[test]
    fn a_transcript_no_exchange_could_make_is_refused() {
        let empty = Transcript { v_c: b"", v_s: b"", i_c: b"", i_s: b"", q_c: b"", q_s: b"", k: b"" };
        assert_eq!(exchange_hash(&empty, &PUBLIC), Err(BadTranscript::BadShape));
        // Each part on its own: every one of the seven must be there.
        let s = sample();
        let blanked: [Transcript; 7] = [
            Transcript { v_c: b"", ..s },
            Transcript { v_s: b"", ..s },
            Transcript { i_c: b"", ..s },
            Transcript { i_s: b"", ..s },
            Transcript { q_c: b"", ..s },
            Transcript { q_s: b"", ..s },
            Transcript { k: b"", ..s },
        ];
        for (i, t) in blanked.iter().enumerate() {
            assert_eq!(exchange_hash(t, &PUBLIC), Err(BadTranscript::BadShape), "part {i}");
        }
        // The ephemeral keys are Curve25519 points: 32 bytes, no more and no less.
        for len in [1, 31, 33, 64] {
            let wrong = vec![0xcdu8; len];
            assert_eq!(
                exchange_hash(&Transcript { q_c: &wrong, ..s }, &PUBLIC),
                Err(BadTranscript::BadShape)
            );
            assert_eq!(
                exchange_hash(&Transcript { q_s: &wrong, ..s }, &PUBLIC),
                Err(BadTranscript::BadShape)
            );
        }
        // `K` is an mpint body, whose length an exchange does vary: any non-empty one is taken.
        for len in [1, 31, 32, 33] {
            let k = vec![0x01u8; len];
            assert!(exchange_hash(&Transcript { k: &k, ..s }, &PUBLIC).is_ok(), "K of {len}");
        }
    }
}
