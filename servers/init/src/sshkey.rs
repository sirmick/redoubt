//! A principal's SSH public key, as the manifest writes it: `ssh-ed25519 BASE64`, one space
//! between, nothing after. The base64 is the key's SSH blob (RFC 4253 section 6.6, RFC 8709): the
//! string `ssh-ed25519`, then the 32-byte key as a string. `init` decodes the blob to the 32 bytes
//! it asks `keyd` about (servers/init.md, "The key-separation check"). This is framing, not
//! cryptography: `init` holds none.

use alloc::vec::Vec;

/// The one algorithm on the box (servers/keyd.md).
const ALGORITHM: &str = "ssh-ed25519";
/// An Ed25519 public key's length.
pub const KEY_LEN: usize = 32;

/// The 32-byte public key in `text`, or `None` if `text` is not exactly one Ed25519 key in the
/// form above, with canonical base64.
pub fn ed25519(text: &str) -> Option<[u8; KEY_LEN]> {
    let (algorithm, encoded) = text.split_once(' ')?;
    if algorithm != ALGORITHM {
        return None;
    }
    let blob = base64(encoded)?;
    let (name, rest) = ssh_string(&blob)?;
    let (key, rest) = ssh_string(rest)?;
    if name != ALGORITHM.as_bytes() || !rest.is_empty() {
        return None;
    }
    key.try_into().ok()
}

/// One SSH `string`: a big-endian `u32` length, then that many bytes.
fn ssh_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, rest) = bytes.split_first_chunk::<4>()?;
    let len = usize::try_from(u32::from_be_bytes(*len)).ok()?;
    (len <= rest.len()).then(|| rest.split_at(len))
}

/// Standard base64 with padding (RFC 4648 section 4), canonical: whole quanta, `=` only at the
/// end, and no bits set past the data in the last quantum, so each blob has one spelling.
fn base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quanta = bytes.len() / 4;
    for (i, quantum) in bytes.chunks_exact(4).enumerate() {
        let pad = quantum.iter().rev().take_while(|b| **b == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != quanta) {
            return None;
        }
        let mut word = 0u32;
        for b in &quantum[..4 - pad] {
            word = word << 6 | u32::from(sextet(*b)?);
        }
        word <<= 6 * pad as u32;
        let data = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        let keep = 3 - pad;
        // The bits the padding stands in for must be zero.
        if data[keep..].iter().any(|b| *b != 0) {
            return None;
        }
        out.extend_from_slice(&data[..keep]);
    }
    Some(out)
}

fn sextet(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    /// The blob of the key 0x01 0x02 ... 0x20, base64-encoded by Python's `base64` module.
    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g";

    fn counting() -> [u8; KEY_LEN] { core::array::from_fn(|i| i as u8 + 1) }

    #[test]
    fn an_openssh_key_decodes_to_its_32_bytes() {
        assert_eq!(ed25519(KEY), Some(counting()));
    }

    #[test]
    fn anything_else_is_refused() {
        let (_, encoded) = KEY.split_once(' ').unwrap();
        for bad in [
            "",
            encoded,
            &std::format!("ssh-rsa {encoded}"),
            &std::format!("{KEY} comment"),
            &std::format!("ssh-ed25519  {encoded}"),
            // A blob one byte short, and one with a byte after the key.
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gIQ==",
            // Non-canonical: a set bit in the padding.
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gIR==",
            // Padding in the middle, and a character outside the alphabet.
            "ssh-ed25519 AAAAC3Nza=1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8g",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8_",
        ] {
            assert_eq!(ed25519(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn base64_round_trips_every_length() {
        // Against an encoder written here the obvious way.
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for len in 1..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let mut text = std::string::String::new();
            for chunk in data.chunks(3) {
                let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
                for i in 0..4 {
                    if i <= chunk.len() {
                        text.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                    } else {
                        text.push('=');
                    }
                }
            }
            assert_eq!(base64(&text), Some(data), "{text}");
        }
    }
}
