//! The bench's test keys as OpenSSH writes them (`tests/keys/`): a public key line, and an
//! unencrypted private key file, Ed25519 only.

use anyhow::{Context, Result, bail, ensure};

/// The Ed25519 public key in an `authorized_keys` line, `ssh-ed25519 BASE64 [comment]`.
pub fn public(line: &str) -> Result<[u8; 32]> {
    let mut words = line.split_whitespace();
    ensure!(words.next() == Some("ssh-ed25519"), "not an ssh-ed25519 key");
    let blob = base64(words.next().context("no key")?)?;
    let mut r = Reader(&blob);
    ensure!(r.string()? == b"ssh-ed25519" && r.0.len() == 4 + 32, "not an ssh-ed25519 key blob");
    key(r.string()?)
}

/// The Ed25519 seed in an unencrypted OpenSSH private key file.
pub fn seed(text: &str) -> Result<[u8; 32]> {
    let body: String = text
        .lines()
        .skip_while(|l| *l != "-----BEGIN OPENSSH PRIVATE KEY-----")
        .skip(1)
        .take_while(|l| *l != "-----END OPENSSH PRIVATE KEY-----")
        .collect();
    let bytes = base64(&body)?;
    let rest = bytes.strip_prefix(b"openssh-key-v1\0").context("not an OpenSSH private key")?;
    let mut r = Reader(rest);
    ensure!(r.string()? == b"none" && r.string()? == b"none", "the key is encrypted");
    r.string()?;
    ensure!(r.u32()? == 1, "not one key");
    r.string()?;
    let mut private = Reader(r.string()?);
    ensure!(private.u32()? == private.u32()?, "the key's check words differ");
    ensure!(private.string()? == b"ssh-ed25519", "not an ssh-ed25519 key");
    let public = key(private.string()?)?;
    // Ed25519's private half as OpenSSH keeps it: the seed, then the public key.
    let pair = private.string()?;
    ensure!(pair.len() == 64 && pair[32..] == public, "a malformed ssh-ed25519 private key");
    key(&pair[..32])
}

/// `keyd`'s argument for a host key named `name` with this seed (servers/keyd.md).
pub fn host_key_arg(name: &str, seed: &[u8; 32]) -> String {
    let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    format!("{name},ssh_host,{hex}")
}

fn key(bytes: &[u8]) -> Result<[u8; 32]> { bytes.try_into().context("not a 32-byte key") }

/// SSH's wire encoding: big-endian lengths before strings.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Result<u32> {
        let (word, rest) = self.0.split_first_chunk::<4>().context("truncated")?;
        self.0 = rest;
        Ok(u32::from_be_bytes(*word))
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        ensure!(n <= self.0.len(), "truncated");
        let (s, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(s)
    }
}

/// Standard base64, padding optional.
fn base64(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let (mut bits, mut n) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => bail!("not base64"),
        };
        bits = bits << 6 | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/keys");

    /// Each test key's seed makes the public key its `.pub` file names.
    #[test]
    fn the_test_keys_read_as_their_pairs() {
        for name in ["alice", "bob", "mallory", "loopback-host"] {
            let seed = seed(&std::fs::read_to_string(format!("{KEYS}/{name}")).unwrap()).unwrap();
            let public = public(&std::fs::read_to_string(format!("{KEYS}/{name}.pub")).unwrap()).unwrap();
            let args = [host_key_arg("k", &seed)];
            let keys = redoubt_keyd::keys::Keys::from_args(args.iter().map(String::as_str)).unwrap();
            assert_eq!(*keys.get(0).unwrap().public(), public, "{name}");
        }
    }

    #[test]
    fn malformed_keys_are_refused() {
        assert!(public("ssh-rsa AAAA").is_err());
        assert!(public("ssh-ed25519 !!!!").is_err());
        assert!(public("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5").is_err());
        assert!(seed("").is_err());
    }
}
