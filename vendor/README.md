# Vendored crates

Third-party source kept in the tree so that it is read and built from here, not fetched. Each
crate is used through a `[patch.crates-io]` path in the root `Cargo.toml`, or, for beamlet's,
in `userland/otp/Cargo.toml`, its workspace's.

## The crates

Each directory is the crate exactly as crates.io published it: the `.crate` file, unpacked,
with nothing added, removed or edited, except that a patched crate carries its one patch
([below](#patched-crates)). The checksum is the published `.crate` file's SHA-256, as the
crates.io index records it. There are four users:

- **`ipd`'s TCP/IP stack:** `smoltcp` and the five crates after it.
- **The loader's and `keyd`'s Ed25519:** `ed25519-compact`, which `sshd` links too
  ([below](#the-loaders-and-keyds-ed25519)).
- **`sshd`'s SSH library:** `sunset` and the rest of the table
  ([below](#sshds-ssh-library)).
- **beamlet, the Elixir VM:** every crate compiled into the VM for the target, the crypto,
  regex, compression and number crates, one version of each ([below](#beamlets-crates)).

| Crate | Version | License (ours to use under) | crates.io SHA-256 |
| --- | --- | --- | --- |
| `smoltcp` | 0.14.0 | 0BSD | `b6f8b28ad56c6e35524a37dd492af5d1a47e31e1a4d175cd12f89c075f01980f` |
| `managed` | 0.8.0 | 0BSD | `0ca88d725a0a943b096803bd34e73a4437208b6077654cc4ecb2947a5f91618d` |
| `heapless` | 0.9.3 | MIT (of MIT OR Apache-2.0) | `25ba4bd83f9415b58b4ed8dc5714c76e626a105be4646c02630ad730ad3b5aa4` |
| `hash32` | 0.3.1 | MIT (of MIT OR Apache-2.0) | `47d60b12902ba28e2730cd37e95b8c9223af2808df9e902d4df49588d1470606` |
| `stable_deref_trait` | 1.2.1 | MIT (of MIT OR Apache-2.0) | `6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596` |
| `byteorder` | 1.5.0 | MIT (of Unlicense OR MIT) | `1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b` |
| `ed25519-compact` | 2.4.2 | MIT | `f05391a505666bdf2b5d2626f41b7f0f49052b1e33cceac960eaa818008141da` |
| `sunset` | 0.6.0 | 0BSD | `3b52312b804ac95f10d3963f6ef5889117ad678bd6ce977be4aa4cf491bad78f` |
| `sunset-sshwire-derive` | 0.3.0 | 0BSD | `d40354cdc622342c11b742e91a5c151ebd59cbd3bc507f6146e9e6de6791f75f` |
| `aes` | 0.9.3 | MIT (of MIT OR Apache-2.0) | `35f0f96ce78e38c3dc6d8948aa8163d06385be74000f3c7a95bf1eef35d3ea32` |
| `ctr` | 0.10.1 | MIT (of MIT OR Apache-2.0) | `baaca1c4b237092596f64d571e9db6ce4109c4ef9742e27590f1709594461f21` |
| `chacha20` | 0.10.2 | MIT (of MIT OR Apache-2.0) | `65c35e4b699c7e15ccbe7ee35c005e4fc0a278d22238a2857e6ce2dadeda1b06` |
| `poly1305` | 0.9.1 | MIT (of Apache-2.0 OR MIT) | `6e2d0073b297041425c7c3df6eb4792d598a15323fe63346852b092eca02904c` |
| `universal-hash` | 0.6.1 | MIT (of MIT OR Apache-2.0) | `f4987bdc12753382e0bec4a65c50738ffaabc998b9cdd1f952fb5f39b0048a96` |
| `hmac` | 0.13.0 | MIT (of MIT OR Apache-2.0) | `6303bc9732ae41b04cb554b844a762b4115a61bfaa81e3e83050991eeb56863f` |
| `sha2` | 0.11.0 | MIT (of MIT OR Apache-2.0) | `446ba717509524cb3f22f17ecc096f10f4822d76ab5c0b9822c5f9c284e825f4` |
| `digest` | 0.11.3 | MIT (of MIT OR Apache-2.0) | `f1dd6dbb5841937940781866fa1281a1ff7bd3bf827091440879f9994983d5c2` |
| `cipher` | 0.5.2 | MIT (of MIT OR Apache-2.0) | `e8cf2a2c93cd704877c0858356ed03480ff301ee950b43f1cbe4573b088bfa6c` |
| `crypto-common` | 0.2.2 | MIT (of MIT OR Apache-2.0) | `ce6e4c961d6cd6c9a86db418387425e8bdeaf05b3c8bc1411e6dca4c252f1453` |
| `inout` | 0.2.2 | MIT (of MIT OR Apache-2.0) | `4250ce6452e92010fdf7268ccc5d14faa80bb12fc741938534c58f16804e03c7` |
| `block-buffer` | 0.12.1 | MIT (of MIT OR Apache-2.0) | `d2f6c7dbe95a6ed67ad9f18e57daf93a2f034c524b99fd2b76d18fdfeb6660aa` |
| `hybrid-array` | 0.4.15 | MIT (of MIT OR Apache-2.0) | `27f864f10dfb56725ce5ce5472bc52252c8f93a4ab86327122cebf62c5f59a17` |
| `typenum` | 1.20.1 | MIT (of MIT OR Apache-2.0) | `b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20` |
| `ctutils` | 0.4.2 | MIT (of Apache-2.0 OR MIT) | `7d5515a3834141de9eafb9717ad39eea8247b5674e6066c404e8c4b365d2a29e` |
| `cmov` | 0.5.4 | MIT (of Apache-2.0 OR MIT) | `0c9ea0ac24bc397ab3c98583a3c9ba74fa56b09a4449bbe172b9b1ddb016027a` |
| `cpubits` | 0.1.1 | MIT (of MIT OR Apache-2.0) | `15b85f9c39137c3a891689859392b1bd49812121d0d61c9caf00d46ed5ce06ae` |
| `subtle` | 2.6.1 | BSD-3-Clause | `13c2bddecc57b384dee18652358fb23172facb8a2c51ccc10d74c157bdea3292` |
| `zeroize` | 1.9.0 | MIT (of Apache-2.0 OR MIT) | `e13c156562582aa81c60cb29407084cdb54c4164760106ab78e6c5b0858cf64e` |
| `zeroize_derive` | 1.5.0 | MIT (of Apache-2.0 OR MIT) | `3c50655cbb0fe3fc43170059e702f1ce5e19b84cec58dc87b037a09935c2f328` |
| `ascii` | 1.1.0 | MIT (of Apache-2.0 OR MIT) | `d92bec98840b8f03a5ff5413de5293bfcd8bf96467cf5452609f939ec6f5de16` |
| `snafu` | 0.9.2 | MIT (of MIT OR Apache-2.0) | `e45cb604038abb7b926b679887b3226d8d0f23874b66623625a0454be425a4b7` |
| `snafu-derive` | 0.9.2 | MIT (of MIT OR Apache-2.0) | `287f59010008f0d7cf5e3b03196d666c1acc46c8d3e9cf34c28a1a7157601e72` |
| `virtue` | 0.0.17 | MIT | `7302ac74a033bf17b6e609ceec0f891ca9200d502d31f02dc7908d3d98767c9d` |
| `getrandom` | 0.4.3 | MIT (of MIT OR Apache-2.0) | `300e883d756b2e4ec94e02791f39b04b522276138852cfc41d9fb7e904106099` |
| `adler2` | 2.0.1 | 0BSD (of 0BSD OR MIT OR Apache-2.0) | `320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa` |
| `aead` | 0.6.1 | MIT (of MIT OR Apache-2.0) | `1973cfbc1a2daf9cf550e74e1f088c28e7f7d8c1e1418fb6c9dc5184b7e84c99` |
| `aes-gcm` | 0.11.1 | MIT (of Apache-2.0 OR MIT) | `7f2b8006a0c83f52b62ba44a97b58bf76fe2f70a329e588f67f89691d93d498f` |
| `base16ct` | 1.0.0 | MIT (of Apache-2.0 OR MIT) | `fd307490d624467aa6f74b0eabb77633d1f758a7b25f12bceb0b22e08d9726f6` |
| `cbc` | 0.2.1 | MIT (of MIT OR Apache-2.0) | `ce2dc9ee5f88d11e0beb842c88b33c8a5cf0d1329c4b19494af42b07dbfe8896` |
| `chacha20poly1305` | 0.11.0 | MIT (of Apache-2.0 OR MIT) | `9b89e1c441e926b9c82a8d023f6e1b7ae0adcfaa7d621814e4d60789bac751cb` |
| `const-oid` | 0.10.2 | MIT (of Apache-2.0 OR MIT) | `a6ef517f0926dd24a1582492c791b6a4818a4d94e789a334894aa15b0d12f55c` |
| `crypto-bigint` | 0.7.5 | MIT (of Apache-2.0 OR MIT) | `1a52aa3fcda4e6302a9f48734f234d35d4721b96f8fe07d073f07ce9df4f0271` |
| `crypto-primes` | 0.7.2 | MIT (of Apache-2.0 OR MIT) | `3633a51a39c69ebbaa4feaa694bd83d241e4093901c84a0963b19d9bb3f0cf8f` |
| `der` | 0.8.2 | MIT (of Apache-2.0 OR MIT) | `a878c850e9e421b20262e9b41f9c860e4785fa07541c266b62ff9d1ef998a80a` |
| `ecdsa` | 0.17.0 | MIT (of Apache-2.0 OR MIT) | `c0681a4fc24c767085329728d8dfba959af91228aa4610cca4f8ce317ba46ae0` |
| `elf` | 0.8.0 | MIT (of MIT OR Apache-2.0) | `55dd888a213fc57e957abf2aa305ee3e8a28dbe05687a251f33b637cd46b0070` |
| `elliptic-curve` | 0.14.1 | MIT (of Apache-2.0 OR MIT) | `9d65aa39b3a5c1c9c1b745c9a019234bb7a21b77abcb4f4d266d706e2d577d65` |
| `ff` | 0.14.0 | MIT (of MIT OR Apache-2.0) | `a1f686ab92a9fb0eaf188f6c6c87b89490baa6fdb0db4544ba4dc47f7942489f` |
| `ghash` | 0.6.0 | MIT (of Apache-2.0 OR MIT) | `2eecf2d5dc9b66b732b97707a0210906b1d30523eb773193ab777c0c84b3e8d5` |
| `group` | 0.14.0 | MIT (of MIT OR Apache-2.0) | `7fd1a1c7a5206c5b7a3f5a0d7ccd3ff85d0c8f5133d62a02680255b0004af5f4` |
| `hkdf` | 0.13.0 | MIT (of MIT OR Apache-2.0) | `4aaa26c720c68b866f2c96ef5c1264b3e6f473fe5d4ce61cd44bbe913e553018` |
| `keccak` | 0.2.2 | MIT (of Apache-2.0 OR MIT) | `d8f198d1db720e4940b5a493201d199d9f24f568f8f746bd13706243a2f71598` |
| `libm` | 0.2.16 | MIT | `b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981` |
| `md-5` | 0.11.0 | MIT (of MIT OR Apache-2.0) | `69b6441f590336821bb897fb28fc622898ccceb1d6cea3fde5ea86b090c4de98` |
| `miniz_oxide` | 0.9.1 | MIT (of MIT OR Zlib OR Apache-2.0) | `b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c` |
| `num-bigint` | 0.4.8 | MIT (of MIT OR Apache-2.0) | `c89e69e7e0f03bea5ef08013795c25018e101932225a656383bd384495ecc367` |
| `num-integer` | 0.1.47 | MIT (of MIT OR Apache-2.0) | `7ce2d95d4b3734dc35aa2f45e1aa22cd416814592a4f9d9205e11affd5b8e10b` |
| `num-traits` | 0.2.19 | MIT (of MIT OR Apache-2.0) | `071dfc062690e90b734c0b2273ce72ad0ffa95f0c74596bc250dcfd960262841` |
| `p256` | 0.14.0 | MIT (of Apache-2.0 OR MIT) | `d2c9239b2dbc807adbbe147e8cf72ea7450c3a0aabe62cb8e75ff4ec22e1f72a` |
| `p384` | 0.14.0 | MIT (of Apache-2.0 OR MIT) | `d17b851e6b3e378ab4ecb07fa2ed23f4d15f075735f8fec9fa1e7bdce5f8301f` |
| `pbkdf2` | 0.13.0 | MIT (of MIT OR Apache-2.0) | `112d82ceb8c5bf524d9af484d4e4970c9fd5a0cc15ba14ad93dccd28873b0629` |
| `polyval` | 0.7.3 | MIT (of Apache-2.0 OR MIT) | `f0fa31d631f2b2cb2a544d0aa321ce847a94764d701ca2becc411138b93d49cd` |
| `primefield` | 0.14.0 | MIT (of Apache-2.0 OR MIT) | `c555a6e4eb7d4e158fcb028c835c3b8642206ddc279b5c6b202ef9a8bdb592f4` |
| `primeorder` | 0.14.0 | MIT (of Apache-2.0 OR MIT) | `5c9f42978c78a00e3d68f69fc03e57a234debae69da4020a4fb588fcdcd07b06` |
| `rand_core` | 0.10.1 | MIT (of MIT OR Apache-2.0) | `63b8176103e19a2643978565ca18b50549f6101881c443590420e4dc998a3c69` |
| `regex-automata` | 0.4.18 | MIT (of MIT OR Apache-2.0) | `ad8553b9b26413251cbf30e620595c7a41b3887f03da04579c0e6b0d6a06b4b2` |
| `regex-syntax` | 0.8.11 | MIT (of MIT OR Apache-2.0) | `d6f6ff9a378485b298a5286656da665ba74413d36db0979633275d2e708145d4` |
| `rfc6979` | 0.6.0 | MIT (of Apache-2.0 OR MIT) | `b4a459cddafb3fe76b31fd8f1108007566c40301feb64dc7b54656eb7388172b` |
| `rsa` | 0.10.0-rc.18 | MIT (of MIT OR Apache-2.0) | `30b2aa4ba0d89f73d1e332df05be0eeab8840351c36ca5654341dfdb57bb3caf` |
| `ryu` | 1.0.23 | BSL-1.0 (of Apache-2.0 OR BSL-1.0) | `9774ba4a74de5f7b1c1451ed6cd5285a32eddb5cccb8cc655a4e50009e06477f` |
| `sec1` | 0.8.1 | MIT (of Apache-2.0 OR MIT) | `d56d437c2f19203ce5f7122e507831de96f3d2d4d3be5af44a0b0a09d8a80e4d` |
| `sha1` | 0.11.0 | MIT (of MIT OR Apache-2.0) | `aacc4cc499359472b4abe1bf11d0b12e688af9a805fa5e3016f9a386dc2d0214` |
| `sha3` | 0.12.0 | MIT (of MIT OR Apache-2.0) | `bc9bad02c26382724b2d2692c6f179285e4b54eeecd7968f52a50059c3c11759` |
| `signature` | 3.0.0 | MIT (of Apache-2.0 OR MIT) | `28d567dcbaf0049cb8ac2608a76cd95ff9e4412e1899d389ee400918ca7537f5` |
| `sponge-cursor` | 0.1.0 | MIT (of MIT OR Apache-2.0) | `3a0219bd7d979d58245a4f41f695e1ac9f8befdffadd7f61f1bae9e39abc6620` |
| `wnaf` | 0.14.1 | MIT (of Apache-2.0 OR MIT) | `795ca18b3fdb5e62bf982199278341ddcf7ebf7d32e25e212ad05d496e95f6fa` |

Their licence texts are in `LICENSES/` (`smoltcp-0BSD.txt`, `heapless-MIT.txt`, ...), as well as
in each directory. `sunset-sshwire-derive` ships none; its author's is `sunset`'s
(`sunset-0BSD.txt`).

**Left to `Cargo.lock`, not vendored.** The crates also need these. Each is already locked from
crates.io for another package: `cfg-if` reaches the bench through `filetime`, `bitflags` the test
programs through `uart_16550`, `log` the loader through `tar-no-std`, and the proc-macro stack
(`proc-macro2`, `quote`, `syn`, `unicode-ident`, `heck`) the kernel through `riscv-macros` and the
bench through `clap_derive`. A patch applies to every user, so vendoring them would take those packages'
copies too. `log` is the only one compiled into a program, as its facade; the rest are
macro-only or run on the build host. They stay pinned by version and checksum instead.

| Crate | Version | License | crates.io SHA-256 |
| --- | --- | --- | --- |
| `cfg-if` | 1.0.0 | MIT OR Apache-2.0 | `baf1de4339761588bc0619e3cbc0120ee582ebb74b53b4efbf79117bd2da40fd` |
| `bitflags` | 1.3.2 | MIT OR Apache-2.0 | `bef38d45163c2f1dde094a7dfd33ccf595c92905c8f8f4fdc18d06fb1037718a` |
| `log` | 0.4.22 | MIT OR Apache-2.0 | `a7a70ba024b9dc04c27ea2f0c0548feb474ec5c54bba33a7f72f873a39d07b24` |
| `proc-macro2` | 1.0.86 | MIT OR Apache-2.0 | `5e719e8df665df0d1c8fbfd238015744736151d4445ec0836b8e628aae103b77` |
| `quote` | 1.0.35 | MIT OR Apache-2.0 | `291ec9ab5efd934aaf503a6466c5d5251535d108ee747472c3977cc5acc868ef` |
| `syn` | 2.0.87 | MIT OR Apache-2.0 | `25aa4ce346d03a6dcd68dd8b4010bcb74e54e62c90c573f394c46eae99aba32d` |
| `unicode-ident` | 1.0.12 | (MIT OR Apache-2.0) AND Unicode-DFS-2016 | `3354b9ac3fae1ff6755cb6db53683adb661634f67557942dea4facebec0fee4b` |
| `heck` | 0.5.0 | MIT OR Apache-2.0 | `2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea` |

**Locked, never built for the box.** `Cargo.lock` also lists `defmt`, `defmt-macros`,
`defmt-parser` and `thiserror` 2.0.0. smoltcp's `alloc` feature names `defmt?/alloc`, a weak
feature, and the lockfile resolves optional dependencies whatever the features. None of them is
compiled. `thiserror` is held at 2.0.0 so that no other package's locked `proc-macro2` moves. The
RustCrypto crates bring `cpufeatures` 0.3.1 (CPU feature detection on x86, ARM and LoongArch, so
compiled only for a host build) and `getrandom` brings `r-efi` 6.0.0 (UEFI only, never
compiled). `cargo tree -p redoubt-vendor-check -e normal --target riscv64imac-unknown-none-elf`
shows only the crates in the tables above.

## Checked

**The bench.** `tools/vendor-check` (the `vendor-check` bench case, `kind = "host-tests"`)
proves:
- every file matches `vendor/SHA256SUMS`, and no file has been added or removed; a patched
  crate is checked with its patch reversed, so the patch is the whole difference;
- every patch in `vendor/patches/` belongs to a vendored crate with a section below;
- git tracks every vendored file (a crate's own `.gitignore`, which often names `Cargo.lock`,
  applies inside `vendor/` too: add such a file with `git add -f`). `vendor/.gitattributes`
  turns off line-ending conversion, so git stores a crate's CRLF files (`virtue`'s) as published;
- `Cargo.lock` builds each vendored crate from its path, at its version, with no registry copy;
- `cargo metadata` resolves each one to `vendor/<name>/Cargo.toml` in this tree, so the patches
  point here, and no registry copy of it is in the graph;
- the crates left to the lockfile are locked at the versions and checksums above;
- what beamlet builds for the target, its build scripts' crates included, with the features
  `cargo tree` resolves, is vendored or in the table of crates left to its lockfile, so a new
  crates.io dependency fails;
- the first table agrees with the test's own.

`vendor-build` builds them all `no_std` for both widths, with the features their users ask for.

**Integrity, not provenance.** `vendor/SHA256SUMS` was taken from the published crates when
they were vendored (see "Updating"), and the table's `.crate` checksums are compared only with
the test's constants, never with the bytes. So the bench proves that nothing has changed since
the sums were written. It does not prove that the tree is what crates.io published: someone who
edits a file, regenerates the sums and updates both tables passes it. Provenance is checked
against crates.io itself, by

```
tools/vendor-check/provenance.sh
```

For each crate in the first table it downloads the `.crate` from `static.crates.io`, checks its
SHA-256 against the live crates.io index and against this table, unpacks it, applies the crate's
patch if it has one, and `diff -r`s it against `vendor/<name>`. It exits 0 only if all agree for
every crate. It needs the network, so the bench does not run it; **the review of any change
under `vendor/` must run it** and quote its output. It passed for 737a0a41a, in an independent
review run, and at the commit that added it.

**Not workspace members.** The vendored directories are in the root manifest's `exclude`, not in
`members`. Were they members, `Cargo.lock` would take in their dev-dependencies (test
frameworks, `rand`, `url`, ...), and `cargo test --workspace` would build their default `std`
and `libc` features. As path dependencies they build only with the features their users ask
for.

**Outside the unsafe ratchet and rustfmt.** `tests/unsafe-budget.toml` counts code this
repository owns, and these crates are third-party code pinned by these checksums. What each
user's crates do with `unsafe` is in its section below. rustfmt ignores `vendor/`.

**Warnings.** Cargo caps lints only for registry and git packages; a path package, patched or
not, is built as local code, and there is no per-package lint cap on stable (`profile-rustflags`
is nightly-only). A cold build prints warnings from three crates: `managed` (two
`mismatched_lifetime_syntaxes`, two `redundant_semicolons`), `ascii` (two
`mismatched_lifetime_syntaxes`) and `virtue`, a build-host macro helper (25, the same lint).
Nothing builds with `-D warnings`, and silencing them by a workspace-wide `-A` flag would hide
the same lints in our own code, so they are left. A lint that is an error by default is not
capped either: that is why `ascii` is patched ([below](#ascii)).

**Build scripts,** which run on the build host. All four were read.
- `smoltcp`'s reads `SMOLTCP_*` environment variables to size its buffers. `ipd`'s own build
  script refuses to build while any is set, so the compiled configuration is the one in the
  source.
- `heapless`'s sets a cfg for a few 32-bit targets that are not ours. It also compiles a
  one-line ARM `clrex` probe with the build's own `rustc`; on RISC-V the probe fails, so it sets
  nothing.
- `sunset`'s sets `SUNSET_SSH_IDENT` to `SSH-2.0-Sunset-<version>`, its identification string.
- `getrandom`'s sets `getrandom_msan` under the memory sanitizer, which nothing here uses.

**Updating.** Take the new `.crate` files from crates.io and check their SHA-256 against the
index. Unpack each into a scratch directory, `published/<name>/`, and regenerate the sums from
those published bytes, never from `vendor/`:

```
(cd published && find . -type f | sed 's|^\./||' | LC_ALL=C sort | xargs sha256sum) \
    > vendor/SHA256SUMS
```

Copy each over its emptied `vendor/<name>/`, then apply each patch
(`patch -p1 -d vendor/<name> < vendor/patches/<name>.patch`), redone by hand on the new release
if it no longer applies. Update both tables here and the constants in
`tools/vendor-check/tests/vendored.rs` in the same commit, run `tools/vendor-check/provenance.sh`,
and read the diff.

## ipd's TCP/IP stack

- `smoltcp` is `#![deny(unsafe_code)]`, apart from `rand.rs` and the host phy backends, which
  `ipd` does not compile.
- `heapless` does use `unsafe`. smoltcp uses it only through `Vec` and `LinearMap`. Those two
  were read with `ipd`'s stack commit, below.

**What of heapless `ipd` runs** (read for `ipd`'s stack commit). With `ipd`'s
features smoltcp compiles three heapless containers, all over `Copy` elements with no `Drop`:
`Vec<IpCidr, 2>` (the interface's addresses, `iface/interface/mod.rs`), `Vec<Route, 2>` (its
routes, `iface/route.rs`) and `LinearMap<IpAddress, Neighbor, 8>` (the neighbour cache,
`iface/neighbor.rs`). Multicast, SLAAC, RPL, DHCP, DNS and 6LoWPAN, which use more, are not
compiled.
- `vec/mod.rs` has 42 lines with `unsafe`. Its one invariant is that elements `0..len` are
  initialised and `len <= N`. The paths smoltcp reaches keep it:
  - `push` checks `len < capacity` before `push_unchecked` writes slot `len`;
  - `swap_remove` asserts `index < len`, then reads that slot, moves the last one into it and
    shortens `len`;
  - `remove` panics on `index >= len`, then reads the slot and shifts the tail down by one;
  - `as_slice`/`as_mut_slice` (and so `Deref`, `iter`) make a slice of exactly `0..len`;
  - `truncate`/`clear` and `Drop` shorten `len` before `drop_in_place`, so a panicking destructor
    cannot drop twice (and these elements have none).
  `LenT` is `usize` here, so its conversions cannot fail.
- `linear_map.rs` has 6 `unsafe` blocks, all in the `Entry` API (`OccupiedEntry`), which smoltcp
  never calls. What it does call (`new`, `get`, `get_mut`, `insert`, `remove`, `iter`, `keys`)
  is safe code over `Vec`'s `iter`, `iter_mut`, `push` and `swap_remove`, above.
- Found: nothing that needs changing. An update of heapless must redo this reading; the counts
  above make the diff easy to see.

## The loader's and keyd's Ed25519

The loader verifies the boot bundle's signature and `keyd` signs with `ed25519-compact`
([keyd](../docs/servers/keyd.md#keys-and-purposes)). They took it from crates.io through
`Cargo.lock`, pinned by checksum but not in the tree; it is vendored now because `sshd` will link
it too, for X25519 and for verifying login signatures
([sshd](../docs/servers/sshd.md#the-core-and-its-platforms)), and one copy read here serves all
three. The version is the one already locked, so the loader and `keyd` build the same code as
before, with no source change: only the `[patch.crates-io]` entry is new.

- **No dependencies and no build script.** Its three optional dependencies (`ct-codecs` for
  `pem`, `ed25519` for `traits`, `getrandom` for `random`) belong to features nobody here turns
  on; every user takes `default-features = false`, and `sunset` adds only `x25519`.
- **X25519** (`x25519.rs`, read for `sshd`): the ladder runs a fixed 255 steps whatever the
  scalar, swaps with a mask (`Fe::cswap2`), not a branch, and inverts by a fixed exponent chain;
  the one test on the result, that it is not zero (a peer's point of small order), is on public
  data. This is how `keyd`'s signing was read for R45. Secret keys and shared secrets wipe
  themselves on drop.
- **One `unsafe` block,** in `common.rs`: `Mem::wipe` writes `T::default()` over each element of
  a slice it was handed as `&mut [T]`, with `write_volatile` at `as_mut_ptr().add(i)` for
  `i < len`, so every write is in bounds; the fences keep the compiler from dropping the writes.
  It clears secret keys, seeds, signing state and X25519 shared secrets on drop.
- The crate ships its author's `AGENTS.md`, contributor notes for that repository; it is part of
  the published bytes, not guidance for this one.

## sshd's SSH library

`sshd`'s core runs `sunset` ([sshd](../docs/servers/sshd.md#the-core-and-its-platforms)), 0BSD,
`#![forbid(unsafe_code)]`, built with `default-features = false`: no ML-KEM, RSA or ECDSA, and
no `std`. What it brings:

- **Ciphers, MACs and hashes:** `aes` with `ctr` (`aes256-ctr`), `chacha20` with `poly1305`
  (`chacha20-poly1305@openssh.com`), `hmac` and `sha2` (`hmac-sha256`, and SHA-256 for the
  exchange hash), and the RustCrypto traits and containers under them (`digest`, `cipher`,
  `crypto-common`, `universal-hash`, `inout`, `block-buffer`, `hybrid-array`, `typenum`) with
  their constant-time helpers (`ctutils`, `cmov`, `cpubits`, `subtle`) and `zeroize`.
- **Curves:** none of its own. X25519 and Ed25519 are `ed25519-compact`'s, by its patch.
- **Parsing and errors:** `ascii` for algorithm names, `snafu` for its error type, and three
  macro crates that run on the build host: `sunset-sshwire-derive` (with `virtue`), which
  writes `sunset`'s wire encoders and decoders, `snafu-derive` and `zeroize_derive`. They are
  vendored though they never run on the box, because the code they write does, and the wire
  decoders parse bytes from before authentication.
- **Randomness:** `getrandom` 0.4. On bare metal it has no source but the program's own
  `__getrandom_v03_custom`, and only the cfg `getrandom_backend="custom"` selects it: the root
  `.cargo/config.toml` sets it for every `target_os = "none"` build, where no other crate reads
  it. Cargo joins those flags with any `[target.<triple>]` ones (checked with cargo 1.98.1),
  but a `RUSTFLAGS` environment variable replaces them all; nothing in the tree or the bench
  sets one. No program provides the function yet: `sshd` is a library until its box platform is
  built, and that platform must ([sshd](../docs/servers/sshd.md#sessions-over-ssh)).

**`unsafe` on RISC-V.** Most of these crates' `unsafe` is in SIMD and instruction backends for
x86, ARM, LoongArch and WebAssembly, and in `sha2`'s RISC-V `zknh` backend, which only an
explicit `sha2_backend` cfg selects: the box compiles none of them. What it does compile is the
software backends and the helpers: `inout`, `block-buffer`, `hybrid-array`, `cmov`'s portable
path, `zeroize`, `subtle`'s one barrier, `ascii`'s string conversions, and `getrandom`'s custom
backend and slice helpers. Their own tests ran under Miri, each with its software backend forced
([sshd under Miri](../docs/servers/sshd.md#under-miri)).

**Read for `sshd`.** On RISC-V, with no backend cfg set:
- `aes`, `chacha20`, `poly1305` and `sha2` fall through to their software backends. `aes`'s union
  is only ever written as `soft`, and each of its three reads is of `soft`; its `Drop` zeroes the
  whole struct. `chacha20` compiles none of its `unsafe`, and `poly1305` none (`sunset` does not
  turn on its `zeroize`). `sha2`'s soft `rk(i)` reads `K32[i]` (`K64[i]`) with `i` from its
  unrolled 0..64 (0..80): in bounds.
- `inout`: input and output are equal or disjoint by construction (one `&mut`, or a `&` and a
  `&mut`); `get` and `split_at` assert their bounds; chunks are cast to `Array`, which is
  `repr(transparent)`. `reserved.rs` is compiled, but its only callers are behind `cipher`'s
  `block-padding`, which is off.
- `block-buffer` (eager, for SHA-256): the position lives in the block's last byte and stays
  below the block size, under 256; bytes `0..pos` are initialised, and `ResetGuard` restores
  that if `compress` panics. `ReadBuffer` serves only XOFs, which nothing here uses.
- `hybrid-array`: every cast rests on `repr(transparent)` and on each `ArraySize`'s `USIZE`
  equalling its inner array's length, which its macro writes and its own test checks; `split`
  needs `U: Sub<N>`, so `N <= U` at compile time; `from_fn`'s guard drops only what it wrote.
- `cmov`: on RISC-V its mask is `seqz` then `addi -1` in inline assembly (`nomem`, `nostack`),
  all ones exactly when the condition is nonzero; Miri runs the portable Rust mask instead. Its
  slice casts are signed to unsigned of one width, and `NonZero` is rebuilt from a value `get`
  returned.
- `zeroize`: volatile writes over exactly the object's bytes, and zero is valid for each type
  written. On RISC-V the barrier is an empty `asm!` given the pointer, `readonly`; Miri runs its
  portable path instead. `zeroize_flat_type` hands the barrier `&data`, the local pointer, not
  the data: a weaker barrier than its name suggests.
- `subtle`: one volatile read of a local in `black_box`, and an `Ordering` rebuilt from an `i8`
  that is one of two valid `Ordering` values.
- `ascii`: `sunset` calls `as_ascii_str` (which checks `is_ascii`, then casts: `AsciiStr` is
  `repr(transparent)` over `[AsciiChar]`, which is `repr(u8)`), `as_str`, `chars` and `split`,
  whose unchecked slicing takes its bounds from `position`.
- `getrandom`: `fill` views `&mut [u8]` as `&mut [MaybeUninit<u8>]` and back once the backend
  returns `Ok`, so the custom backend's provider must write all of `dest` before it does.
- Found: nothing that needs changing. An update of any of these crates must redo this reading.

## Patched crates

A crate we must change is vendored from its published bytes plus exactly one patch file,
`vendor/patches/<crate>.patch`. The directory holds the published crate with the patch applied,
because that is what Cargo builds; `SHA256SUMS` records the published bytes, and `vendor-check`
reverses the patch on a copy and checks that copy against them, so the patch is the whole
difference. `provenance.sh` applies the patch to the downloaded crate and compares. The rule is
the book's: [patched crates](../docs/testbench.md#patched-crates).

Each patched crate has a section here saying what its patch changes and why. A new release
means taking the published bytes again and redoing the patch; a change the author has taken
leaves the patch with the release that carries it.

### `sunset`

- **X25519 and Ed25519 through `ed25519-compact`**, the loader's and `keyd`'s crate, in place of
  `x25519-dalek` and `ed25519-dalek` (and so `curve25519-dalek`, `ed25519` and `signature`, about
  30,000 lines). The ephemeral X25519 key and Ed25519 keys are made from `fill_random` seeds; a
  peer's X25519 point of small order fails the exchange. A stored Ed25519 key whose seed is all
  zeros is refused as a bad key, where `ed25519-compact` would panic. `chacha20` moves from 0.9
  to 0.10, whose `cipher` is the 0.5 the other ciphers use, so one copy of each trait crate
  builds. `rand_core`, which only RSA and ECDSA key generation still need, is optional behind
  those features.

- **A host key signed outside `sunset`.** Given a public-only host key
  (`SignKey::AgentEd25519`), the server raises `ServEvent::SignExchange` after `Hostkeys`, where
  published `sunset` would panic: it hands out `V_C`, `V_S`, `I_C`, `I_S`, `Q_C`, `Q_S` and `K`
  (as an `mpint` body), exactly `keyd`'s `sign_ssh_exchange` fields, and `signed()` sends
  `KEXDH_REPLY` once the signature checks against `sunset`'s own hash. A server keeps copies of
  both `KEXINIT` payloads for this, and hashes the copies, so the signer is given the bytes hashed;
  a peer's over 4 KiB is refused. Curve25519 only, the exchange `keyd` signs.
- **Channel requests.** A client's `window-change`, `signal` and `break` reach a server as
  `SessionWinChange`, `SessionSignal` and `SessionBreak` events, where published `sunset` drops
  them, and `SessionPty` gains `pty()`, the terminal's name (at most `MAX_TERM` bytes of ASCII)
  and starting size, where published `sunset` returns nothing. The client gains `term_signal`,
  beside its `term_break`, filling a slot `sunset` leaves for it. The server's runner gains
  `session_exit`, which sends `exit-status` (filling the slot `sunset` leaves for it), then EOF,
  then close, where published `sunset`'s server sends EOF and close only as echoes; on a full
  output it fails `BusySend`, and called again sends only what it has not sent. A channel that
  has sent its EOF takes no more writes. The server no longer echoes a client's EOF (RFC 4254,
  5.3: EOF is one direction), so a session's output goes on after it; close is still echoed.
  `ServPubkeyAuth` gains `signed()`, whether the request carried a verified signature or was a
  query.

`tools/vendor-check/tests/sunset_patch.rs` runs a `sunset` client against the patched server through
the public API: `keyd`'s `exchange_hash` over the handed-out parts is the hash the client checks, a
signature over another hash is refused, the 4 KiB bound holds at 4,096 and 4,097 bytes, a pty's
starting size and the three requests arrive, `signed()` tells the client's query from its signed
request, and `session_exit` delivers the status and then the close, with no write after, and on a
full output delivers the status once, when the output drains. The signing event and the channel
requests are offered to `sunset`'s author once they have been reviewed here; the curve change is
ours.

### `ascii`

`fn from(a: AsciiChar)` binds `a`, also the name of an `AsciiChar` variant. rustc 1.98 makes
that the error `bindings_with_variant_name`, and Cargo caps lints only for registry crates, so the
published crate does not build from `vendor/`. The patch renames the binding. It changes no
behaviour and can go upstream.

## beamlet's crates

beamlet builds everything it compiles for the target from `vendor/`, through the patches in
`userland/otp/Cargo.toml`: the twenty crates it shares with the servers, and the 42 of the first
table from `adler2` down, its own. One version of each: RSA is `rsa` 0.10.0-rc.18, pinned to that
release candidate, because it is the one on the digest 0.11 generation every other crate here
uses; the 0.9 line would bring a second copy of that generation, and with it the crates with the
most `unsafe` in the VM. What builds or tests only on the host (the CLI's `cap-std` and `rustix`,
`pcre2` for the differential tests, the proc-macro stack) comes from crates.io, pinned by
`userland/otp/Cargo.lock`, as the root's host tools' crates do.

**Left to `userland/otp/Cargo.lock`, not vendored.** `cfg-if` is macro-only, as it is for the root
(above), and `autocfg` is `num-traits`'s build dependency, which runs only on the build host; both
stay pinned by version and checksum. `serde`, `serde_core` and `serdect` are locked
and never built: `crypto-bigint` names `serdect` through a weak feature (`serdect?/alloc`), which
the lockfile resolves whatever the features, and nothing turns it on.

| Crate | Version | License | crates.io SHA-256 |
| --- | --- | --- | --- |
| `autocfg` | 1.5.1 | MIT OR Apache-2.0 | `f2032f911046de80f0a198e0901378627c33f59ea0ac00e363d481118bd70a53` |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 | `4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600` |

**What of them uses `unsafe`,** read for the vendoring commit, of what compiles for riscv64. The
SIMD and assembly backends for x86, ARM and LoongArch (`keccak`, `md-5`, `polyval`, `sha1`, `libm`'s
`arch`, `num-bigint`'s carry intrinsics) are behind `cfg(target_arch)` and are not compiled. None
of `aead`, `adler2`, `elf`, `ff`, `miniz_oxide`, `p384`, `primefield`, `regex-syntax`, `sec1`,
`sha3`, `signature` and `wnaf` has any (`forbid(unsafe_code)`), nor do `aes-gcm`, `cbc`,
`chacha20poly1305`, `crypto-primes`, `ecdsa`, `ghash`, `group`, `hkdf`, `num-integer`, `p256`,
`pbkdf2`, `primeorder`, `rand_core`, `rfc6979` and `rsa`. The rest:
- `const-oid`, `der`, `crypto-bigint` and `elliptic-curve`: casts of a reference to a
  `repr(transparent)` newtype (`ObjectIdentifierRef`, `BytesRef`, `Limb`, `UintRef`, `NonZero`,
  `Odd`, `NonIdentity`), each with its `SAFETY` note; `crypto-bigint` also views its limbs as
  bytes (a byte slice needs no alignment) and turns an `i8` into the `repr(i8)` `Ordering`.
- `base16ct` and `num-bigint`: `from_utf8_unchecked` over the ASCII digits they just wrote.
- `num-traits`: one float-to-integer conversion after the range check that makes it exact.
- `sponge-cursor` (under `sha3`): `unreachable_unchecked` where its type keeps the position below
  the rate, and a `[u64; N]` viewed as the first `RATE` bytes, where `RATE` is at most its size.
- `libm`: musl's algorithms, ported with unchecked indexing into fixed tables (`i!`), bit-for-bit
  float transmutes, a volatile read to force an evaluation, and `unreachable_unchecked` behind
  explicit checks in its narrowing division.
- `ryu`: formats a float into a buffer through raw pointers, its `Buffer` sized for the longest
  output, and indexes its power tables unchecked by exponents its arithmetic bounds.
- `regex-automata`: its pool of search caches (a mutex of its own and an owner fast path, with
  `Sync` asserted for them), its lazily initialised statics, and the deserialisation of a
  serialised automaton, which beamlet's `re` does not call. `re` turns on the lazy DFA
  (`hybrid`), whose search loop reads unchecked: `hybrid/search.rs`'s `next_unchecked!` reads
  the haystack byte with `get_unchecked` at a position the forward loop bounds by the search's
  end and the reverse loop by its start (reading down from one before its end),
  and `hybrid/dfa.rs`'s `next_state_untagged_unchecked` reads the transition table with
  `get_unchecked` at the state's offset plus the byte's class, which is in the table when the
  state is untagged, a state the cache built; the loop takes the checked path for a tagged one.

Found: nothing that needs changing. The patterns are the common ones, and each crate's use is
the one its authors document; an update must redo this reading, and the counts per crate (from
`grep -c unsafe`) make the diff easy to see.

**Build scripts,** which run on the build host; both were read. `num-traits`'s probes the compiler
for `f64::total_cmp` with `autocfg`. `libm`'s reads only Cargo's own variables and
`ENSURE_NO_PANIC`, and sets its configuration cfgs from the target.
