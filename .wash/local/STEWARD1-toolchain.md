# STEWARD1 checkpoint 1: the pinned OTP and Elixir in the dev image

Nothing is built or changed yet. This is the proposal.

## Facts found

- The image is built by `./dev.sh` from `Dockerfile` (debian:trixie, rev 3, 4.53 GB today). It is
  rebuilt when its `org.redoubt.dev.revision` label differs from `IMAGE_REV` in dev.sh.
- The host's `toolchains/` cannot simply be used in the container. `in-dev` does make
  `/home/mcloonan/redoubt/toolchains` visible there, but `beam.smp` there needs `GLIBC_2.43` (it
  was built from source on this host), and trixie has glibc 2.41. It is also untracked, so a fresh
  checkout does not have it.
- The host OTP is a `make release` + `./Install` tree (`Install`, `bin`, `erts-16.4.0.6`, `lib`,
  `misc`, `releases`, `usr` at the root). setup.sh depends on that layout: it finds the libraries
  at `$(dirname $(dirname $(command -v erl)))/lib/<app>-*/ebin`. A plain `make install --prefix`
  tree puts them under `lib/erlang/lib`, which would break it. Its apps are the default set
  without wx, odbc, jinterface, debugger, observer or et.
- Upstream artifacts exist. I downloaded both and checked each against the checksum published
  with its release:
  - `https://github.com/erlang/otp/releases/download/OTP-28.5.0.6/otp_src_28.5.0.6.tar.gz`
    (105 MB), sha256 `49d7a75e906334af54ae336ba53fc4e6ad100645e8e7efd3be008de284dab3ba`, as
    listed in that release's `SHA256.txt`.
  - `https://github.com/elixir-lang/elixir/releases/download/v1.20.4/elixir-otp-28.zip`
    (8.6 MB), sha256 `ea7ff98bc1ed76c663a5d034c863c6fd37e65c9c855b6bbf6ccb2034d806673c`, as in
    its `.sha256sum`.
  - The host's `toolchains/elixir-1.20.4/lib` matches that zip byte for byte (`diff -rq`), so the
    container's Elixir would be the host's Elixir.
- builds.hex.pm (bob) has no trixie OTP build list at the obvious path. Prebuilt OTP would be a
  third-party binary anyway.

## Proposal

1. **OTP from the pinned source, in a builder stage of the Dockerfile.**
   - `FROM debian:trixie AS beam`.
   - Build deps in that stage only: build-essential, libssl-dev, libncurses-dev, zlib1g-dev, curl,
     ca-certificates, unzip.
   - `ARG OTP_VERSION=28.5.0.6`, `ARG OTP_SHA256=49d7…3ba`, `ARG ELIXIR_VERSION=1.20.4`,
     `ARG ELIXIR_SHA256=ea7f…73c`.
   - Fetch the source, then `echo "$OTP_SHA256  otp_src.tar.gz" | sha256sum -c` (a mismatch fails
     the build).
   - `./configure --without-wx --without-odbc --without-javac --without-debugger
     --without-observer --without-et`, then `make -j$(nproc)`, then
     `make release RELEASE_ROOT=/opt/toolchains/otp-$OTP_VERSION`, then `./Install -minimal` in
     that root. That is the host's layout and app set.
   - Fetch the Elixir zip, check its sha256 the same way, and unzip it into
     `/opt/toolchains/elixir-$ELIXIR_VERSION`.
2. **The final image** gets `COPY --from=beam /opt/toolchains /opt/toolchains` and
   `ENV BEAMLET_TOOLCHAINS=/opt/toolchains`. The runtime libraries libtinfo6, zlib1g and libssl3
   are already in the trixie base plus libssl-dev, so no new packages are needed there.
3. **How the tools find them.**
   - env.sh already honours `BEAMLET_TOOLCHAINS` before its `toolchains/` default.
   - setup.sh only sets it when it is unset, so the image's value wins over the host tree that
     in-dev exposes.
   - Docker `ENV` survives `bash -lc`, which resets PATH only.
   - The bench case sources env.sh and fails outright, not SKIP, if `erl` is missing or
     `releases/28/OTP_VERSION` is not 28.5.0.6. That catches drift between the image's pins and
     env.sh's.
4. **dev.sh `IMAGE_REV` 3 → 4** (and the Dockerfile default), so every dev.sh run rebuilds once.
   GETTING-STARTED "beamlet" says the container provides them, and the host keeps `toolchains/`.

## Cost (estimates; I have not run the build)

- **Size:** about +150 MB, measured from the host tree (OTP 153 MB, Elixir 17 MB). That is about
  3% of 4.5 GB. Build deps stay in the builder stage.
- **Build time:** about 5 to 8 minutes for OTP on 24 cores with docs skipped, cached afterwards.
  Downloads are about 115 MB.

## Rollout hazard: needs a ruling

`in-dev` always runs the main checkout's `dev.sh`, and every worktree shares one `redoubt-dev`
image. If I build rev 4 from my branch, the next run of main's rev-3 dev.sh by any agent rebuilds
rev 3, and the two keep rebuilding each other. Options:

- **(a) Recommended.** The Dockerfile and dev.sh change is my first commit. The orchestrator lands
  just that commit on main early, and the image is rebuilt once. Rev 4 is a strict superset, so
  nobody else is affected.
- **(b)** I build a private tag (`redoubt-dev:steward1`) and run it with a private copy of in-dev
  until the merge. That needs dev.sh to accept an `IMAGE` override (a one-line change).
- **(c)** Leave the image alone until the merge. The new case cannot then be shown RUNNING before
  acceptance, so I advise against it.

## Alternatives rejected

- A Debian OTP package: trixie ships 27.x, not the pin.
- bob/hexpm prebuilt binaries: third-party, and no trixie build of the patch level was found.
- Mounting the host's `toolchains/`: wrong glibc, and absent from a fresh checkout.

## Ruling applied: commit 17455f2d69fffe4ff08e0d6464fe9d9b6271a96b (wp-steward1)

Built and verified under the private tag redoubt-dev:steward1 only (redoubt-dev:latest untouched, rev 3).

- Build: full rebuild of every layer but the cached debian base, 135 s wall (OTP stage 40 s,
  24 cores); rebuilds after are cached (3 s).
- Size: 4,793,032,779 B vs rev 3's 4,530,007,084 B. The toolchain layer (COPY /opt/toolchains)
  is 178 MB; the remaining ~85 MB of difference is not ours — the `latest` agent CLIs and rust
  stable drifted since rev 3 was built (npm layer 853 -> 859 MB, etc., per `docker history`).
  No builder package is in the final image: `dpkg -l libncurses-dev zlib1g-dev unzip` finds none.
- App set: lib/ in the image lists exactly the host toolchains/otp-28.5.0.6 apps.
- Locale: added `LANG=C.UTF-8` (Elixir warned the VM ran with latin1 file-name encoding).
- Evidence (docker run mirroring in-dev: main checkout at /work, /home/mcloonan/redoubt -> /work,
  so the host toolchains/ was visible; cwd the worktree), log /tmp/steward1-verify.log:
    LANG=C.UTF-8
    BEAMLET_TOOLCHAINS=/opt/toolchains
    /opt/toolchains/otp-28.5.0.6/bin/erl
    /opt/toolchains/elixir-1.20.4/bin/elixir
    /opt/toolchains/elixir-1.20.4/bin/elixirc
    28.5.0.6                                  (releases/28/OTP_VERSION)
    Elixir 1.20.4 (compiled with Erlang/OTP 28)
    == setup.sh   -> mix compile ok, beamlet built (userland/otp/target/release/beamlet), fake-redoubt built
    == run-vectors (BEAMLET_DIR=userland/otp; its default ../beamlet is still stale, fixed later in the package)
    BEAM: example.txt failures=0; example-generated.txt failures=0; hostile encodes 13 failures=0; ok
    beamlet: identical / PASS          exit 0
- Conditions: (1) four ARGs, sha256sum -c, no fallback; the Elixir zip name derives from
  OTP_VERSION's major. (2) is the case's job, later in the package. (3) shown above. (4) header
  line and GETTING-STARTED "beamlet" in the same commit. (5) above.
