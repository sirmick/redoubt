//! Boot bundle authentication. See `docs/kernel/boot.md`, "Verified boot".
//!
//! The initrd is `signature (64 bytes) || bundle-tar`. The loader verifies the signature
//! with one embedded Ed25519 public key and refuses to boot otherwise. What the signature
//! covers is not the bare archive: it is the domain-separated preimage
//! `"redoubt.bundle.v1\0" || u64_le(len) || tar`, built by `redoubt_signing` — the one place
//! either side of a Redoubt signature constructs a preimage.

use ed25519_compact::{PublicKey, Signature};
use redoubt_signing::DEV_PUBLIC_KEY;

const SIGNATURE_LEN: usize = 64;

/// Verify `initrd` (`signature || bundle`) and return the authenticated bundle, or panic.
///
/// The signature is checked over `"redoubt.bundle.v1\0" || u64_le(len) || bundle`. `len` is the
/// number of bytes of archive this loader found after the signature in the initrd it was
/// handed — measured here, never read out of the archive, which is the attacker's to write. The
/// preamble and the archive are hashed in turn, so nothing is copied; the result is identical to
/// verifying over the two concatenated (`redoubt_signing`, `preimage_is_preamble_then_archive`).
/// A signature over the bare archive does not verify here, and is refused like any other bad one.
pub fn authenticated_bundle(initrd: &[u8]) -> &[u8] {
    let (signature, bundle) =
        initrd.split_at_checked(SIGNATURE_LEN).expect("initrd is too small to be signed");

    let key = PublicKey::new(DEV_PUBLIC_KEY);
    let signature = Signature::new(signature.try_into().unwrap());
    let preamble = redoubt_signing::bundle_preamble(bundle.len() as u64);
    let verified = key.verify_incremental(&signature).and_then(|mut state| {
        state.absorb(preamble);
        state.absorb(bundle);
        state.verify()
    });
    match verified {
        Ok(()) => bundle,
        Err(_) => panic!("boot bundle signature is invalid; refusing to boot"),
    }
}
