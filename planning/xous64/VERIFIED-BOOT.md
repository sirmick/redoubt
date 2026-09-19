# Verified boot

Status: implementing, 2026-09-18. Tenet 2: "every byte that runs in a privileged mode is
authenticated before it runs." Without this, the whole security stack is bypassable by
editing the boot bundle — the device-grant manifest, the code, everything.

## Threat and chain of trust
On a real device the chain is ROM -> firmware -> loader -> bundle, each link verifying the
next, rooted in a key in ROM/fuses. QEMU loads `loader64` directly with `-kernel`, so we
cannot verify the loader itself here (that needs firmware support on real hardware). What
we can and do verify is the link that matters for running code: **the loader authenticates
the boot bundle before executing any of it.** A tampered bundle is refused, fail-closed.

## Signature
- **Algorithm:** Ed25519 (RFC 8032), via the pure-Rust, self-contained, `no_std`
  `ed25519-compact` crate. Not `ed25519-dalek`: the workspace patches `curve25519-dalek`
  to a VexRiscv-specific backend that does not build for rv64.
- **What is signed:** the entire bundle tar.
- **Container:** the initrd is `signature (64 bytes) || bundle-tar`. The loader takes the
  first 64 bytes as the signature and verifies them over the rest.
- **Key:** `loader64` embeds one Ed25519 public key. It verifies with that key alone; there
  is no algorithm agility and no key list, to keep the trusted path tiny.

## Development key
The bench signs with a **development** key derived from a fixed, public seed
(`[0x42; 32]`), so builds are reproducible and anyone can rebuild. Its public key is
compiled into `loader64` as `DEV_PUBLIC_KEY`. This key is **not for production**: it is
published in this repo. A real deployment generates a secret key, keeps it secret, and
replaces `DEV_PUBLIC_KEY`. The seed being public is the point — it proves the mechanism
without pretending the dev key is a secret.

## Loader behaviour
1. Split the initrd into `signature` and `bundle`.
2. Verify `signature` over `bundle` with `DEV_PUBLIC_KEY`.
3. On failure: panic (which powers the machine off via SBI). No unsigned fallback.
4. On success: proceed exactly as before, on `bundle`.

## Testbench
`bundle()` signs every bundle it builds, so all boot tests exercise the verified path. A
case may set `tamper_bundle = true` to flip one payload byte after signing; the loader
must then refuse to boot. Attack test `verified-boot-rejects-tamper` checks that.

## Not covered
- Verifying the loader itself (needs firmware / ROM support; out of scope on QEMU).
- Rollback protection (version pinning), key rotation, and revocation.
- Encrypting the bundle (this is integrity/authenticity, not confidentiality).
