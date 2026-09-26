//! Redoubt signing domains: the one place a signature preimage is constructed.
//!
//! See `docs/kernel/boot.md`, "Verified boot". A Redoubt signature never covers bare bytes a
//! caller handed the signer: it covers a domain-separated preimage the signer built itself, or
//! (`keyd`'s audit records) a digest of one that `keyd` computed itself. A preimage is a
//! NUL-terminated ASCII domain name, then the `u64_le` byte count of what follows, then the bytes
//! themselves. The names are prefix-free and the length is fixed-width, so a signature made under
//! one domain can never be re-read as another protocol's message (25 bytes of someone else's
//! domain and length fit inside a ustar header's name field, so without a domain of our own a
//! foreign signature could be presented as a valid bundle).
//!
//! The boot bundle's preimage is `"redoubt.bundle.v1\0" || u64_le(len) || tar`. Both sides of the
//! signature take its preamble from `bundle_preamble` here and nowhere else — the loader
//! (`loader/src/verify.rs`), which hashes the preamble and then the archive, and every signer
//! (today `tools/testbench`'s bundle builder), which writes the preamble and then the archive.
//! A signature produced anywhere else, over any other bytes, stops working — which is the point.
//!
//! **`len` is measured, never read.** A verifier passes the number of archive bytes it found in
//! the container it is reading (the initrd, after the 64-byte signature). It must never take a
//! length out of the signed bytes: those are the attacker's.
//!
//! The other domains (`"redoubt.audit.v1\0"`, today defined in `keyd` itself, see
//! `docs/servers/keyd.md`; `"redoubt.pkg.v1\0"` for packages, `docs/servers/pkg.md`, not built
//! yet) belong here too, built the same way.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// The boot bundle signing domain, NUL-terminated (kernel/boot.md, "Verified boot").
pub const BUNDLE_DOMAIN: &[u8] = b"redoubt.bundle.v1\0";

/// Bytes of preamble the bundle signature covers before the archive itself: the domain and the
/// `u64_le` length.
pub const BUNDLE_PREAMBLE_LEN: usize = BUNDLE_DOMAIN.len() + core::mem::size_of::<u64>();

/// The preamble of the boot bundle's signature preimage: `"redoubt.bundle.v1\0" || u64_le(len)`,
/// which the archive's `len` bytes follow.
///
/// `len` is the byte count of the archive the caller is holding: for a verifier, what it
/// measured in the container it is reading; for a signer, the archive it is about to ship.
/// A verifier that took `len` from the signed bytes would be letting the attacker choose it.
pub fn bundle_preamble(len: u64) -> [u8; BUNDLE_PREAMBLE_LEN] {
    let mut preamble = [0u8; BUNDLE_PREAMBLE_LEN];
    let (domain, length) = preamble.split_at_mut(BUNDLE_DOMAIN.len());
    domain.copy_from_slice(BUNDLE_DOMAIN);
    length.copy_from_slice(&len.to_le_bytes());
    preamble
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire form, spelled out by hand as kernel/boot.md states it. If this test has to be
    /// edited, the signature format has changed and every signed bundle in the world stops
    /// verifying: that is the change, not a detail of it.
    #[test]
    fn preamble_is_the_documented_bytes() {
        assert_eq!(BUNDLE_DOMAIN, b"redoubt.bundle.v1\x00");
        assert_eq!(BUNDLE_DOMAIN.len(), 18);
        assert_eq!(BUNDLE_PREAMBLE_LEN, 26);

        let mut expected = [0u8; 26];
        expected[..18].copy_from_slice(b"redoubt.bundle.v1\x00");
        expected[18..].copy_from_slice(&[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(bundle_preamble(0x0102_0304_0506_0708), expected);

        // An empty archive still carries its length, all zeroes.
        assert_eq!(&bundle_preamble(0)[18..], &[0u8; 8]);

        // And the length is part of what is signed: one archive's preamble is never another's.
        assert_ne!(bundle_preamble(64), bundle_preamble(65));
    }

    /// The domain is NUL-terminated and nothing else contains a NUL, so no domain name can be a
    /// prefix of another: the property the separation rests on.
    #[test]
    fn domain_is_prefix_free() {
        assert_eq!(BUNDLE_DOMAIN.iter().filter(|&&b| b == 0).count(), 1);
        assert_eq!(BUNDLE_DOMAIN.last(), Some(&0));
        assert!(BUNDLE_DOMAIN.iter().all(|b| b.is_ascii()));
    }
}
