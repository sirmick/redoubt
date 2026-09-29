# Vendored crates

Third-party source kept in the tree so that it is read and built from here, not fetched. Each
crate is used through a `[patch.crates-io]` path in the root `Cargo.toml`.

## The crates

Each directory is the crate exactly as crates.io published it: the `.crate` file, unpacked,
with nothing added, removed or edited, except that a patched crate carries its one patch
([below](#patched-crates)). The checksum is the published `.crate` file's SHA-256, as the
crates.io index records it. There are three users:

- **`ipd`'s TCP/IP stack** (answer 174): `smoltcp` and the five crates after it. Mick chose to
  vendor them (2026-09-24).
- **The loader's and `keyd`'s Ed25519:** `ed25519-compact`, which `sshd` links too
  ([below](#the-loaders-and-keyds-ed25519)).
- **`sshd`'s SSH library:** `sunset` and the rest of the table
  ([below](#sshds-ssh-library)). The owner chose to vendor them with `sshd`'s core (2026-09-28).

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
under `vendor/` must run it** and quote its output. It passed for 737a0a41a (the red team's
independent run, QA D3-code-review-1) and at the provenance commit that added it.

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

**What of heapless `ipd` runs** (read for `ipd`'s stack commit, answer 174). With `ipd`'s
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
  sets one. `sshd`'s program provides the function.

**`unsafe` on RISC-V.** Most of these crates' `unsafe` is in SIMD and instruction backends for
x86, ARM, LoongArch and WebAssembly, and in `sha2`'s RISC-V `zknh` backend, which only an
explicit `sha2_backend` cfg selects: the box compiles none of them. What it does compile is the
software backends and the helpers: `inout`, `block-buffer`, `hybrid-array`, `cmov`'s portable
path, `zeroize`, `subtle`'s one barrier, `ascii`'s string conversions, and `getrandom`'s custom
backend and slice helpers. Their reading and their own tests under Miri, each with its software
backend forced, are a follow-up ([todo](../docs/todo/sshd-vendored-miri.md)).

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
