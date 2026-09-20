//! Redoubt signing domains: the one place a signature preimage is constructed.
//!
//! See `planning/redoubt/VERIFIED-BOOT.md`. A Redoubt signature never covers bare bytes it was
//! handed. It covers a domain-separated preimage: a NUL-terminated ASCII domain name, then the
//! `u64_le` byte count of what follows, then the bytes themselves. The names are prefix-free and
//! the length is fixed-width, so a signature made under one domain can never be re-read as
//! another protocol's message (question 120: 25 bytes of someone else's domain and length fit
//! inside a ustar header's name field, so without a domain of our own a foreign signature could
//! be presented as a valid bundle).
//!
//! The boot bundle's preimage is `"redoubt.bundle.v1\0" || u64_le(len) || tar`. Both sides of
//! the signature build it here and nowhere else: the loader
//! (`loader/src/verify.rs`) and every signer (today `redoubt/testbench`'s bundle builder). A
//! signer produced anywhere else, over any other bytes, stops working — which is the point.
//!
//! **`len` is measured, never read.** A verifier passes the number of archive bytes it found in
//! the container it is reading (the initrd, after the 64-byte signature). It must never take a
//! length out of the signed bytes: those are the attacker's.
//!
//! The other domains of VERIFIED-BOOT.md (`"redoubt.audit.v1\0"` in `keyd`, `"redoubt.pkg.v1\0"`
//! for packages in milestone 2) belong here too when they land, built the same way.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[cfg(any(test, feature = "alloc"))]
extern crate alloc;

/// The boot bundle signing domain, NUL-terminated (VERIFIED-BOOT.md, Signature).
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

/// The whole preimage for a signer that has the archive in memory: `bundle_preamble(tar.len())`
/// followed by `tar`. A verifier does not need this: it hashes the preamble and the archive in
/// turn, without a copy (`loader/src/verify.rs`).
#[cfg(any(test, feature = "alloc"))]
pub fn bundle_preimage(tar: &[u8]) -> alloc::vec::Vec<u8> {
    let mut preimage = alloc::vec::Vec::with_capacity(BUNDLE_PREAMBLE_LEN + tar.len());
    preimage.extend_from_slice(&bundle_preamble(tar.len() as u64));
    preimage.extend_from_slice(tar);
    preimage
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire form, spelled out by hand as VERIFIED-BOOT.md states it. If this test has to be
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
    }

    /// The domain is NUL-terminated and nothing else contains a NUL, so no domain name can be a
    /// prefix of another: the property the separation rests on.
    #[test]
    fn domain_is_prefix_free() {
        assert_eq!(BUNDLE_DOMAIN.iter().filter(|&&b| b == 0).count(), 1);
        assert_eq!(BUNDLE_DOMAIN.last(), Some(&0));
        assert!(BUNDLE_DOMAIN.iter().all(|b| b.is_ascii()));
    }

    /// What a signer builds in one buffer and what a verifier hashes in two pieces are the same
    /// bytes. The loader absorbs `bundle_preamble(len)` and then the archive; a signer calls
    /// `bundle_preimage`. If those two ever drift, this fails.
    #[test]
    fn preimage_is_preamble_then_archive() {
        for tar in [&b""[..], &b"x"[..], &[0xffu8; 1024][..]] {
            let preimage = bundle_preimage(tar);
            assert_eq!(preimage.len(), BUNDLE_PREAMBLE_LEN + tar.len());
            assert_eq!(&preimage[..BUNDLE_PREAMBLE_LEN], &bundle_preamble(tar.len() as u64));
            assert_eq!(&preimage[BUNDLE_PREAMBLE_LEN..], tar);
        }
    }

    /// The preimage of one archive is never the preimage of a different one, and never the bare
    /// archive: a signature over the bare bytes cannot be replayed as a domain-separated one.
    #[test]
    fn preimage_is_not_the_bare_archive() {
        let tar = &[0x42u8; 64][..];
        let preimage = bundle_preimage(tar);
        assert_ne!(preimage.as_slice(), tar);
        assert_ne!(bundle_preamble(tar.len() as u64), bundle_preamble(tar.len() as u64 + 1));
    }
}
