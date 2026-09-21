# Verified boot

Built and enforced. Tenet 2: "every byte that runs in a privileged mode is authenticated before it
runs." Owns: the signature and its container. Without it, the whole security stack is bypassable by
editing the boot bundle.

## Chain of trust
On a real device the chain is ROM -> firmware -> loader -> bundle, each link verifying the next,
rooted in a key in ROM or fuses. QEMU loads the loader directly with `-kernel`, so the loader itself
is not verified here. What is verified is the link that matters for running code: **the loader
authenticates the boot bundle before executing any of it.** A tampered bundle is refused.

## Signature
- **Algorithm:** Ed25519 (RFC 8032), via the pure-Rust, self-contained, `no_std` `ed25519-compact`.
- **Container:** the initrd is `signature (64 bytes) || bundle tar`.
- **What is signed is not the bare archive.** The signature covers the domain-separated preimage
  `"redoubt.bundle.v1\0" || u64_le(len) || tar`: 18 bytes of NUL-terminated ASCII domain, then the
  archive's byte count as a little-endian `u64`, then the archive. `len` is the number of bytes
  after the 64-byte signature in the initrd; the verifier takes it from the container it is reading,
  never from the signed bytes. **The loader builds that preimage and verifies over it**, before
  reading the tar, and on failure panics, which powers the machine off via SBI. No unsigned
  fallback, and no acceptance of a signature over the archive alone. **The signing tool builds the
  same preimage** (the bench's bundle builder today, any production signer later). Both sides get
  it from one crate, `redoubt/signing` (`no_std`, no dependencies, `forbid(unsafe_code)`, TCB and
  counted in the `unsafe` budget), so they cannot drift apart; a host test pins its bytes to the
  ones stated here, and the bench runs that test, so they cannot drift from this note either.
- **Domains are prefix-free**, so one key's signature can never be read as another protocol's.
  Every Redoubt signing domain is a NUL-terminated ASCII name followed by the `u64_le` length of
  what it covers: `"redoubt.audit.v1\0" || u64_le(len) || record` in `keyd` (CONTAINMENT.md),
  `"redoubt.pkg.v1\0"` for packages from milestone 2 (PACKAGES.md), `"redoubt.bundle.v1\0"` here.
  Without a domain on this one, a signature made elsewhere could be made to cover a valid bundle:
  a ustar header's name field is 100 bytes of arbitrary bytes, so another protocol's domain and
  length fit inside the first tar header, and its preimage is then a well-formed archive
  (question 120).
- **Key:** the loader embeds one Ed25519 public key (`loader/src/verify.rs`). No algorithm agility.
  `init` carries the same key as a compiled-in constant, so that it can refuse a manifest handing it
  to `keyd` (INIT.md).

## Development key
The bench signs with a key derived from a fixed, public seed (`[0x42; 32]`), so builds are
reproducible and anyone can rebuild. Its public key is compiled into the loader as `DEV_PUBLIC_KEY`.
It is **not for production**: a real deployment generates a secret key and replaces it.

## Testbench
Every bundle the bench builds is signed, so all boot tests exercise the verified path. A case may set
`tamper_bundle = true` to flip one payload byte after signing; `verified-boot-rejects-tamper` checks
the loader refuses to boot. A case also signs the bare archive, with no domain and no length, and
checks the loader refuses that too (BUILD-PLAN.md, WP-V1).

## Not covered
- Verifying the loader itself (needs firmware or ROM support; the FPGA's boot ROM can do it,
  PLATFORM-FPGA.md).
- M-of-N signatures, rollback protection and key rotation (PACKAGES.md, system updates).
- Encrypting the bundle (this is integrity and authenticity, not confidentiality).
