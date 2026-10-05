# TOOL1 report

## Checkpoint 1: scripts/setup.sh (commit 38c130a4a)

Checked only with `bash -n` and by reading. I have not run it (host rule). shellcheck is not
installed on the host. The argument handling did run: `--help` exits 0;
`--toolchains` without `--with-beam` exits 2; an unknown argument prints the usage and exits 2.

### apt list, each with its reason
- build-essential: the linker for host binaries (rustc links with cc), and the C compiler for
  the cc-built crates (pcre2-sys in userland/otp, littlefs diff), and OTP's build
- pkg-config: brief's list. The root workspace has no -sys crate that links a system C library
  (its Cargo.lock has no cc, pkg-config or openssl). pcre2-sys (userland/otp) probes pkg-config
  and falls back to its bundled copy.
- libssl-dev: brief's list. The bench workspace doesn't use it. OTP's crypto app does
  (--with-beam). Could move to the --with-beam list (orchestrator's call).
- ca-certificates, curl: rustup and the OTP/Elixir downloads
- git: the checkout
- xz-utils: brief's list (tar .xz; rustup's installer handles .xz itself)
- openssh-client: the bench's ssh
- openssh-server: the bench runs /usr/sbin/sshd -i. The package enables a system sshd on
  port 22. The script only prints how to disable it; it does not disable it.
- gdb-multiarch: GDB with RISC-V, for QEMU's stub
- QEMU: qemu-system-riscv if the archive has it (Debian trixie, Ubuntu 26.04: there
  qemu-system-misc does NOT contain riscv), else qemu-system-misc (Ubuntu 24.04/22.04). The
  script chooses by `apt-cache show`, never by distro name. This departs from the brief's
  literal list.
- with --with-beam only: libncurses-dev, zlib1g-dev, unzip (the Dockerfile's beam stage)
- dropped: python3 (nothing outside vendor/bios uses it), sudo, less, vim-tiny

### Not verifiable without running
- rustup-init with -y over a distro /usr/bin/cargo: I expect it to warn and continue
- `rustup default` exit status with no default set (rustup-init sets stable, so this only
  matters for a pre-existing rustup)
- `(cd bios && rustup toolchain install)` installs bios/rust-toolchain.toml's nightly with its
  targets and components. This form needs rustup >= 1.28, and an older existing rustup would
  fail at this step.
- whether `cargo install --list` prints `name vX.Y.Z:` (the skip check depends on it)
- the version parsers on real `qemu-system-riscv64 --version` and `ssh -V` output
- the OTP build under the script (same recipe as the Dockerfile's beam stage)
- `./build --arch rv64` and `cargo testbench --list` as the final checks; their output goes to
  target/setup/*.log, and the last 20 lines print on failure
- whether a fresh shell finds rustup's cargo first on 26.04. rustup's env file only prepends
  ~/.cargo/bin when it is absent from PATH. When the caller's PATH finds another cargo (e.g.
  /usr/bin/cargo), the script prints a NOTE naming both paths.
- Outside a checkout (the dev image copies only the script), the script skips the firmware,
  ./build and testbench --list and prints that it did.

## Sections 2 and 4 (commits f423732a6, 254c7cd12). Nothing built or run (host rule)

### f423732a6 workspace: the dev image is only what setup.sh installs
- Dockerfile is 50 lines (was 184): FROM debian:trixie; ENV RUSTUP_HOME=/opt/rustup,
  CARGO_HOME=/opt/cargo, PATH, BEAMLET_TOOLCHAINS=/opt/toolchains, LANG=C.UTF-8; COPY
  scripts/setup.sh, RUN with --with-beam --toolchains /opt/toolchains; a uid/gid user; WORKDIR
  /work; CMD bash. Gone: the separate beam stage (now the script, so the OTP build deps stay in
  the image), Node, the agent CLIs, pi, sudo, python3, less, vim-tiny, /etc/profile.d/rust.sh
  and /etc/environment (dev.sh no longer runs bash -l), the git identity, the SSH keys and
  /config.
- INTERIM, a departure from the brief: one RUN preinstalls openssh-client, openssh-server and
  qemu-system-riscv from trixie-backports, because trixie's own 10.0 packages fail the script's
  10.1 minimums. The script then sees them installed. Delete that block in the section 3 commit
  that lowers the minimums.
- Outside a checkout (/tmp/setup.sh), the script skips the firmware, ./build and testbench
  --list. The image proves the installs and the version checks, not the build.
- dev.sh is 91 lines (was 165): rebuilds when the revision/uid/gid labels differ or on a
  leading --rebuild (no longer taken from anywhere in the args), seeds /work/.cargo and
  /work/.rustup as before, and runs `docker run ... IMAGE "$@"` (no bash -lc wrapper). Mounts:
  the checkout at /work and at its own host path. IMAGE_REV is 5.
- Deleted scripts/pi-ensure.sh and scripts/ssh-key-ensure.sh.

### in-dev proposal (not edited)
dev.sh now mounts the checkout at its host path, so in-dev needs no sudo or symlink:
  exec "$ROOT/dev.sh" bash -c '
      export RUSTSBI_PROTOTYPER="$1/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper"
      export RUSTSBI_PROTOTYPER_RV32="$1/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper"
      cd "$2" && shift 2 && exec "$@"
  ' _ "$ROOT" "$PWD" "$@"
(i.e. drop the `sudo mkdir ... && sudo ln -sfn` line). in-dev must not pass --rebuild after
the command; dev.sh now takes it only as the first argument.

### 254c7cd12 docs: getting started points at the install script
GETTING-STARTED: Prerequisites rewritten (script or ./dev.sh; minimums QEMU/OpenSSH 10.1, which
the script checks; no package list; the mount sentence no longer names agent config). The
Firmware section says setup.sh builds it once. The beamlet section says the pins live in the
script and names --with-beam and --toolchains.

### Summaries checked
- README.md, docs/README.md: no prerequisites or container claims. No change.
- CONTRIBUTING.md: names neither the container nor the tools. No change.
- userland/otp/README.md:8 ("dev container provides both under /opt/toolchains ...; own
  machine toolchains/"): still true. Not an owned path. No change.
- docs/testbench.md:59-62 and :796-797 (exit-with-parent, WarnWeakCrypto): left for section 3,
  since they describe behaviour that is unchanged until then.
- docs/kernel/boot.md:61 and testbench.md:27 (build-bios.sh): still true.

### Grep `pi-ensure|ssh-key-ensure|/config|redoubt-config|PI_CODING|codex|claude` outside .wash/ and vendor/
- bios/.dockerignore:5 and bios/.gitignore:20 (.codex): upstream RustSBI, vendored. Left.
- bios/** firmware/prototyper/config paths, scripts/build-bios.sh:41: these match /config as a
  path component and are unrelated. Left.
- .gitignore:4, tests/no-cruft.toml:3 (.cargo/config.toml): unrelated.
- docs/theme/mermaid.min.js: minified vendor JS, matched by chance. Left.
No hit refers to the removed tooling.

### Not run
docker build, ./dev.sh, setup.sh, any cargo command, doccheck. `bash -n` passes on setup.sh and
dev.sh. git diff --check is clean on every commit.

## Amendment (orchestrator decisions): setup.sh is now de156a901 (was 38c130a4a)
Branch is now de156a901, d43bf2bcd, f21deda1b; the two later commits are unchanged, replayed.
- pkg-config and libssl-dev moved to --with-beam (APT_PACKAGES_BEAM), each with its reason.
  The base list is build-essential ca-certificates curl git xz-utils openssh-client
  openssh-server gdb-multiarch + qemu-system-riscv|misc.
- openssh-server note: one line, printed last (also before a FAILED line), only when this run
  installed the package: "NOTE: the openssh-server package enables a system sshd on port 22,
  which the bench does not need. To stop it: sudo systemctl disable --now ssh.socket
  ssh.service". Services are not touched.
- The qemu-system-riscv/-misc choice now has a comment: chosen by what the archive offers,
  never by distro name.

## Real-host evidence: owner's machine, 2026-10-04 (run by the owner, reported by the orchestrator)
Host Ubuntu 26.04.1 LTS with a distro /usr/bin/cargo present. One run of de156a901's
scripts/setup.sh (default flags), end to end, exit 0. Verify block: cargo 1.99.0 from
~/.cargo/bin; the three RISC-V targets; qemu-system-riscv64/32 QEMU 10.2.1; OpenSSH 10.2;
mdbook 0.5.4; both firmware ELFs; ./build --arch rv64 ok; cargo testbench --list ok. The PATH NOTE
correctly named /usr/bin/cargo ahead of ~/.cargo/bin/cargo. Owner: "lgtm".
One blemish: rustup-init printed "error: cannot install while Rust is installed" and then
carried on under -y. Fixed by amending the setup.sh commit again: the installer runs with
RUSTUP_INIT_SKIP_PATH_CHECK=yes, and the end-of-run NOTE still reports a shadowing cargo.
setup.sh is now 3f0fdb276 (branch: 3f0fdb276, 862dfc825, fdcc3f872).

## Runs after the host was freed (2026-10-04)
Scratch copies of the worktree live in /home/mcloonan/tool1-scratch/{dev,u2404,u2204}.
/home/mcloonan/tool1-scratch/indev runs a command in redoubt-dev-tool1, with the main checkout
mounted at its own path. Images: redoubt-dev-tool1 (new Dockerfile; the shared redoubt-dev is
untouched), tool1-u2404:setup / :pinned and tool1-u2204:setup (containers after the setup.sh
runs).

- ./dev.sh --rebuild (scratch copy, IMAGE=redoubt-dev-tool1): the first attempt failed at apt.
  Backports QEMU pulls libelf1t64 0.196, and trixie's gdb-multiarch needs 0.192. Fix: install
  gdb-multiarch from backports too (folded into the image commit). The rebuild then exited 0:
  QEMU 11.0.2, OpenSSH 10.3, cargo/rustc 1.99.0 (pinned), toolchains 1.99.0 + nightly.
- ubuntu:24.04, setup.sh (default flags): every install step OK. Exit 1, as designed, at
  "qemu-system-riscv64 is QEMU 8.2.2; the bench needs QEMU 10.1 or later". OpenSSH is 9.6p1
  (packages qemu-system-misc 1:8.2.2+ds-0ubuntu1.18, openssh-server 1:9.6p1-3ubuntu13.19).
  Then by hand with the pinned 1.99.0: ./build --arch rv64 exit 0, --arch rv32 exit 0,
  cargo testbench --list exit 0 (223 lines).
- ubuntu:22.04, setup.sh --skip-firmware: every install step OK. Exit 1 at "QEMU 6.2.0".
  OpenSSH is 8.9p1. ./build rv64 and rv32 exit 0, cargo testbench --list exit 0 (223 lines).
- shellcheck (koalaman/shellcheck-alpine) on setup.sh and dev.sh: exit 0.
- PQ warning evidence: an sshd -i ProxyCommand with KexAlgorithms=curve25519-sha256. OpenSSH
  9.6: no warning. 10.3: the 3-line "** WARNING: connection is not using a post-quantum key
  exchange algorithm..." without LogLevel=ERROR, and none with it.
- QEMU unknown-option message ("-no-such-option: invalid option") is the same on 11.0.2 and
  6.2.0.

## Section 3 (written; commit pending the host tests)
- qemu.rs: exit_with_parent() is a pre_exec doing prctl(PR_SET_PDEATHSIG, SIGKILL) plus a
  getppid() re-check. usable() probes `-version` only; probe(options, since) is kept for a
  future option. a_killed_bench_leaves_no_qemu re-runs the test binary as the stand-in bench.
- ssh.rs: WarnWeakCrypto is passed only when `ssh -G` takes it, with no stderr filter (the
  orchestrator agreed; the evidence is in the comment). main.rs drops the redoubt_usable() gate
  (main.rs is outside the owned paths, which is noted).
- docs/testbench.md: the exit-with-parent and WarnWeakCrypto paragraphs.

## Section 3 committed, then rebased onto b15ecfba6 (BENCHENV1)
- Before the rebase: the kill test first failed (exit 101). libtest prints the stand-in's
  "qemu-pid N" on the same line as "test ... ...", so the parent never matched it; it now uses
  rsplit_once. Mutation check: without exit_with_parent in the stand-in, the test fails with
  "QEMU 303 outlived the process that started it" (exit 101); with it, it passes.
  `indev cargo test -p testbench`: 83 passed, exit 0.
- Rebase: `git rebase --onto b15ecfba6 9212f6f60 wp-TOOL1`.
  - GETTING-STARTED had 2 conflicts; both were resolved by keeping BENCHENV1's reference-guest
    needs (curl, dpkg-deb, xz, network once) as a paragraph beside the script text and the pin.
  - main.rs: the comment conflict was resolved ("Only OpenSSH's server needs a guest.").
  - Section 3 also drops `-run-with exit-with-parent=on` from the reference guest's proxy (in
    ssh.rs proxy() and its test) and from the GUESTS_GO comment and testbench.md's "ssh ends each
    guest". The redoubt_options() probe still stands against the new ssh.rs.
- On the rebased tree: nightly rustfmt --check on tools/testbench/src/*.rs is ok, and
  `indev cargo test -p testbench` gives 87 passed, exit 0.
- Branch: 8fe5fcaea scripts, a32e522f6 workspace (image), 40f08bb0b docs, 14c825a83 workspace
  (pin), 117457aaf testbench (section 3), on b15ecfba6.
- Residual: ssh processes are not given a parent-death signal. A bench killed outright leaves
  its ssh running until its session ends, and the reference guest ends with that ssh. This is
  as before for ssh; the guest no longer has its own exit-with-parent.

## Image gates on the rebased branch (redoubt-dev-tool1, rustc 1.99.0 from rust-toolchain.toml)
Run via /home/mcloonan/tool1-scratch/indev, from the worktree:
- cargo +nightly fmt --all --check -> exit 0
- cargo run -q -p redoubt-doccheck -> exit 0
- ./build --arch rv64 -> exit 0
- ./build --arch rv32 -> exit 0
- cargo testbench size-budget -> exit 0
- cargo testbench unsafe-budget -> exit 0
- cargo test -p testbench -> 87 passed, exit 0 (above)
Logs: target/TOOL1-gate-*.log, target/TOOL1-hosttests.log.

## ubuntu:24.04 whole bench, rv64 (QEMU 8.2.2, OpenSSH 9.6p1, rustc 1.99.0)
Rig: a git clone of wp-TOOL1 at 117457aaf in /home/mcloonan/tool1-scratch/u2404git, with
firmware copied in and OTP/Elixir from `scripts/setup.sh --with-beam` (built in the container;
that run ended at the QEMU 10.1 check, as designed then). Image tool1-u2404:beam, run as a
uid-matched user.
- Three earlier attempts aborted on the rig, not the bench: root-owned target/, a copy without
  .git, and no erl/erlc.
- `cargo testbench --arch rv64` -> exit 1: 225 PASS, 1 FAIL.
  - The reference case bench-ssh-loopback-openssh PASSED (50.6s).
  - The FAIL was rt-miri: "nightly Miri not installed (rustup component add miri --toolchain
    nightly)". That is a setup.sh gap: the nightly step now installs rustfmt, miri and rust-src,
    and verify checks `cargo +nightly miri --version`.
  - After the fix: setup.sh --with-beam installed them, and `cargo testbench --arch rv64
    rt-miri` PASSED (92.1s), exit 0.
- So QEMU_MIN=8.2 and OPENSSH_MIN=9.6, the oldest the whole bench has passed on. 22.04's QEMU 6.2
  and OpenSSH 8.9 are untested by the bench.

## Last changes, then the fold
- openssh-server was dropped from the apt list, along with the system-sshd NOTE and the
  /usr/sbin/sshd check. Since BENCHENV1 the reference sshd runs in a QEMU guest built from
  Debian packages, and the bench no longer runs the host's sshd (no "sbin/sshd" in
  tools/testbench/src). Verify now checks curl, xz and dpkg-deb, which the guest build needs.
- The Dockerfile's backports block was dropped: trixie's own QEMU 10.0 and OpenSSH 10.0 clear
  8.2/9.6. The header comment was updated.
- GETTING-STARTED states the minimums 8.2/9.6, and says only rustfmt and the Miri cases run on
  nightly.
- docs/testbench.md: one sentence on ssh having no parent-death signal (residual accepted).
- The branch was folded with `git reset --soft b15ecfba6` and recommitted by path:
  a816082e8 testbench: run on stock QEMU and OpenSSH
  0b826b7b8 workspace: one pinned Rust for every machine
  ca00a1626 scripts: one install script for the prerequisites, on any apt system
  afceea891 workspace: the dev image is only what setup.sh installs
  4935534db docs: getting started points at the install script
- Not yet run on the folded tree: the image rebuild without backports (trixie's QEMU 10.0), the
  gates in it, and the brief's whole bench on both widths in the new image. These are pending
  the orchestrator's go. The tested code (qemu.rs, ssh.rs, main.rs) is unchanged since the host
  tests and gates. setup.sh changed after the gates (minimums, Miri, the sshd drop): bash -n and
  shellcheck pass on it.

## Round 1 (red OK with notes 1-4, editor OK with notes 1-7), folded; head 7b81c2ca7
Tag tool1-r1 = 4935534db (as reviewed). `git range-diff b15ecfba6 tool1-r1 HEAD`:
  1: a816082e8 ! ddec5a216 testbench: run on stock QEMU and OpenSSH
     (RED 1/2; RED 3/ED 4; ED 5 is in commit 5)
  2: 0b826b7b8 = e581aed9f workspace: one pinned Rust for every machine
  3: ca00a1626 ! 9e8c0f216 scripts: ... (ED 7; the openssh-server comment)
  4: afceea891 ! a8985e43b workspace: the dev image ... (ED 6)
  5: 4935534db ! 7b81c2ca7 docs: getting started ... (ED 3, ED 5, openssh-server sentence)
- RED 1: takes() runs `ssh -F /dev/null -G`. When the option is dropped, the run prints
  "note  ssh gets no `-o WarnWeakCrypto=no-pq-kex` against Redoubt's server: <ssh's complaint>".
- RED 2: REDOUBT_OPTIONS_SINCE and the `ssh -V` lookup are gone; takes() returns ssh's
  complaint, and its test was updated.
- RED 3 / ED 4: the "would be probed the same way" sentence was dropped; testbench.md's keeper
  paragraph was rewrapped. probe(options, since) is kept in qemu.rs.
- ED 2: the in-image counts are below.
- ED 3: "with `sudo` (or as root)".
- ED 5: testbench.md (oracle toolchain paragraph) and userland/otp/README.md name
  `scripts/setup.sh --with-beam`. README.md is outside the owned paths; the editor asked for it.
- ED 6: the Dockerfile says the firmware's nightly is fetched at run time and needs the network.
- ED 7: the `--toolchains=<dir>` form was dropped from the code; --help is unchanged.
- Orchestrator: no openssh-server. This is said in setup.sh's comment and on GETTING-STARTED
  ("The bench needs only OpenSSH's client on your machine").
Dockerfile 44 lines, dev.sh 91, setup.sh 380.

## All gates, with commands and exit codes
Ubuntu 24.04 container (QEMU 8.2.2, OpenSSH 9.6p1, rustc 1.99.0, uid-matched user, fresh git
clone, firmware, OTP from setup.sh --with-beam):
- rv64 at 117457aaf: `cargo testbench --arch rv64` -> exit 1. PASS 225, FAIL 1 (rt-miri: nightly
  Miri not installed, a setup.sh gap since fixed), SKIP 0. bench-ssh-loopback-openssh PASS
  50.6s. After the fix, `cargo testbench --arch rv64 rt-miri` -> PASS 92.1s, exit 0.
- rv32 at 7b81c2ca7: `scripts/setup.sh --with-beam` -> exit 0 (verify: QEMU 8.2.2 both widths,
  OpenSSH 9.6, rustc 1.99.0 pinned, firmware, ./build rv64 ok, testbench --list ok).
  `cargo testbench --arch rv32` -> exit 0: PASS 211, FAIL 0, SKIP 0. bench-ssh-loopback-openssh
  PASS 9.8s. The note line appeared ("Bad configuration option: warnweakcrypto").
In-image (redoubt-dev-tool1: trixie, no backports, QEMU 10.0.13, OpenSSH 10.0, rustc 1.99.0):
- `./dev.sh --rebuild` (scratch copy, IMAGE=redoubt-dev-tool1) -> exit 0.
- At 4935534db: `cargo testbench` (both widths) -> exit 1. PASS 397, FAIL 1, SKIP 0.
  bench-ssh-loopback-openssh PASS 9.9s. The FAIL is host-tests:
  ssh::tests::the_keeper_finds_what_names_the_case (BENCHENV1's test), left [18855, 18860]
  right [18855].
- Also at 4935534db, each exit 0: git diff --check; cargo +nightly fmt --all --check; doccheck;
  ./build rv64; ./build rv32; cargo test -p testbench (87 passed); cargo testbench size-budget;
  cargo testbench unsafe-budget.
- At 7b81c2ca7:
  - `git diff --check b15ecfba6..HEAD` -> 0
  - `cargo +nightly fmt --all --check` -> 0
  - `cargo run -q -p redoubt-doccheck` -> 0
  - `cargo testbench loopback` -> 0 (all 14: the 7 bench-ssh-loopback* and 7 sshd-loopback-*;
    the note line appeared)
  - `cargo test -p testbench` -> 101: 86 passed, 1 failed, the same keeper test, now
    left [] right [442]
- Keeper test alone, 30 runs of the test binary -> 0 of 30 failed. It looks like a race under
  load in BENCHENV1's test: it reads /proc before the child sh has exec'd, or while sh forks.
  TOOL1 does not change processes_naming or the test. It is a finding for BENCHENV1's owner.
- Host: Ubuntu 26.04.1 (the owner, de156a901's setup.sh) -> exit 0, as recorded above.

## Final-head gate: rv64 whole bench in the image at 7b81c2ca7
`cargo testbench --arch rv64` in redoubt-dev-tool1 (QEMU 10.0.13, OpenSSH 10.0, rustc 1.99.0),
no --allow-skip, alone on the host: exit 0. PASS 226, FAIL 0, SKIP 0.
bench-ssh-loopback-openssh PASS 3.3s. The WarnWeakCrypto note line printed (OpenSSH 10.0 lacks
the option). The keeper test passed this time. Log: target/TOOL1-gate4-rv64.log.
