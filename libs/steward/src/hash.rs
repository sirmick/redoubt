//! The request binding hash: SHA-256 over the request's canonical encoding (servers/steward.md,
//! "Guards and effects"). It binds content and signs nothing; it is the core's one cryptographic
//! function, and with a login key's id it uses the box's own SHA-256, `libs/sha256`.

use redoubt_sha256::{Sha256, hash};

use crate::domain::Domain;
use crate::event::Content;
use crate::store::Request;

const DOMAIN_SEPARATOR: &[u8] = b"redoubt.steward.request.v1\0";

fn u64s(h: &mut Sha256, words: &[u64]) {
    h.update(&(words.len() as u64).to_le_bytes());
    for w in words {
        h.update(&w.to_le_bytes());
    }
}

fn bytes(h: &mut Sha256, b: &[u8]) {
    h.update(&(b.len() as u64).to_le_bytes());
    h.update(b);
}

/// The hash of request `r` of `domain`: its requester's domain, its id, what it asks for, the
/// snapshot it froze and its reason. Any change makes another hash, so a new request.
pub fn binding(domain: &Domain, r: &Request) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN_SEPARATOR);
    h.update(&domain.account().get().to_le_bytes());
    u64s(&mut h, domain.labels().as_slice());
    h.update(&r.id.to_le_bytes());
    match &r.content {
        Content::Note { what } => {
            h.update(&[0]);
            bytes(&mut h, what.as_bytes());
        }
        Content::Agent { labels, lease } => {
            h.update(&[1]);
            u64s(&mut h, labels);
            h.update(&lease.to_le_bytes());
        }
        Content::Declassify { labels, item } => {
            h.update(&[2]);
            u64s(&mut h, labels);
            h.update(&item.to_le_bytes());
        }
        Content::Push { source, target, item } => {
            h.update(&[3]);
            h.update(&source.to_le_bytes());
            u64s(&mut h, target);
            h.update(&item.to_le_bytes());
        }
    }
    match &r.snapshot {
        None => h.update(&[0]),
        Some(s) => {
            h.update(&[1]);
            bytes(&mut h, s);
        }
    }
    bytes(&mut h, r.reason.as_bytes());
    h.finish()
}

/// An item's digest, as a push's screen shows it.
pub fn item(b: &[u8]) -> [u8; 32] { hash(b) }

/// The id the core knows an `ssh-ed25519` key by: the first eight bytes, little-endian, of
/// SHA-256 over its 32 raw bytes. `init` computes it for the manifest lines and the steward for a
/// login, so the core never sees a key.
pub fn key_id(key: &[u8; 32]) -> u64 {
    let d = hash(key);
    u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}
