//! Boot bundle authentication. See `planning/xous64/VERIFIED-BOOT.md`.
//!
//! The initrd is `signature (64 bytes) || bundle-tar`. The loader verifies the signature
//! over the bundle with one embedded Ed25519 public key and refuses to boot otherwise.

use ed25519_compact::{PublicKey, Signature};

/// Development public key, derived from the public seed `[0x42; 32]`. NOT FOR PRODUCTION:
/// a real build replaces this with the public half of a secret key. See VERIFIED-BOOT.md.
pub const DEV_PUBLIC_KEY: [u8; 32] = [
    0x21, 0x52, 0xf8, 0xd1, 0x9b, 0x79, 0x1d, 0x24, 0x45, 0x32, 0x42, 0xe1, 0x5f, 0x2e, 0xab, 0x6c, 0xb7,
    0xcf, 0xfa, 0x7b, 0x6a, 0x5e, 0xd3, 0x00, 0x97, 0x96, 0x0e, 0x06, 0x98, 0x81, 0xdb, 0x12,
];

const SIGNATURE_LEN: usize = 64;

/// Verify `initrd` (`signature || bundle`) and return the authenticated bundle, or panic.
pub fn authenticated_bundle(initrd: &[u8]) -> &[u8] {
    let (signature, bundle) = initrd.split_at_checked(SIGNATURE_LEN).expect("initrd is too small to be signed");

    let key = PublicKey::new(DEV_PUBLIC_KEY);
    let signature = Signature::new(signature.try_into().unwrap());
    match key.verify(bundle, &signature) {
        Ok(()) => bundle,
        Err(_) => panic!("boot bundle signature is invalid; refusing to boot"),
    }
}
