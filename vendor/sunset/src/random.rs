#[cfg(any(feature = "rsa", feature = "_ecdsa"))]
use rand_core::{CryptoRng, UnwrapErr};
#[allow(unused_imports)]
use {
    crate::error::{Error, Result, TrapBug},
    log::{debug, error, info, log, trace, warn},
};

// Only RSA and ECDSA key generation need a `CryptoRng`; Ed25519 and X25519
// keys are made from `fill_random` seeds.
#[cfg(any(feature = "rsa", feature = "_ecdsa"))]
pub(crate) fn rng() -> impl CryptoRng {
    UnwrapErr(getrandom::SysRng)
}

pub fn fill_random(buf: &mut [u8]) -> Result<(), Error> {
    getrandom::fill(buf).map_err(|_| Error::msg("RNG failed"))
}
