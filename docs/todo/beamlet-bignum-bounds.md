# beamlet's modular exponentiation takes operands of any size

## What

Three of `beamlet-crypto`'s natives run `num-bigint`'s `modpow` on numbers the caller gives,
bounded only by the VM's bignum limit (2^24 bits, [limits](../userland/beamlet.md#limits-inside-one-vm)):

- `crypto:mod_pow/3` (`mod_exp_nif`): the base, the exponent and the modulus, all three.
- `crypto:generate_key(dh, ...)` (`dh_generate_key_nif`): the prime `P` and generator `G`, and a
  private exponent the caller gives.
- `crypto:compute_key(dh, ...)` (`dh_compute_key_nif`): the prime `P` and the private exponent.
  The peer's public value is held to `[2, P - 2]`, but `P` is not held to anything.

`dh_params` checks only that `P` is over 3 and `G` is not zero.

## Why it matters

A native is not preempted by reductions: a `modpow` over a megabyte modulus and exponent holds its
scheduler thread for as long as it takes, minutes or more, and every Erlang process on that thread
waits. This is the stall that RSA private keys had before their sizes were checked ahead of
`from_components`. It stays inside the one session (the kernel shares CPU between VMs by budget
weight), but a hostile key file or peer parameter the session reads can hang the session's tools.

## Where

- `userland/otp/crypto/src/pk.rs:155`, `dh_params`: `P` and `G` unbounded.
- `userland/otp/crypto/src/pk.rs:166`, `dh_generate_key`: a given private exponent unbounded
  (`modpow` at line 177).
- `userland/otp/crypto/src/pk.rs:185`, `dh_compute_key`: the private exponent unbounded
  (`modpow` at line 193).
- `userland/otp/crypto/src/pk.rs:197`, `mod_exp`: base, exponent and modulus unbounded (`modpow`
  at line 202).

## Done when

`P` and the modulus are held to a stated width (OpenSSL's `OPENSSL_DH_MAX_MODULUS_BITS` is 10,000
bits; RSA's limit here is 8,192), and each exponent and base to the modulus's width, before any
`modpow` runs; an oversized operand is an error, never a panic. Host tests with an oversized
modulus and exponent for each of the three show the refusal comes at once, and the crypto
difftests still agree with BEAM.
