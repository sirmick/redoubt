# TOOL1: one prerequisites list, one install script, a dev image that is only that

Tier B (tooling: implementer plus the editor), size S-M. Needs BENCHENV1 (it touches
`tools/testbench/src/ssh.rs` and the guest's QEMU line). Worktree `.worktrees/TOOL1` on
`wp-TOOL1`, from `main` after BENCHENV1's merge. Every cargo and bench command runs as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>` from the worktree, **except** the native
script's own test, below.

## The owner's decisions (2026-10-04)

> i want to make sure the tooling & dir layout is clean. should be minimal tooling, easy to
> install with scripts that work well on ubuntu. [...] debian as image. remove all the agent
> stuff. [drop the QEMU/OpenSSH 10.1 flags:] yes

## What is wrong today

- `GETTING-STARTED.md` lists what "your own machine instead needs" but nothing installs it; the
  only working route is Docker. The list and the `Dockerfile`'s `apt-get`/`rustup`/`cargo install`
  lines are two sources of truth and have drifted (the page omits `mdbook-svgbob`,
  `cargo-binutils`, `llvm-tools`, `gdb-multiarch`).
- The `Dockerfile` (184 lines) and `dev.sh` (165) are mostly not Redoubt: Node 22, the agent CLIs
  (pi, Claude Code, Codex), pi extensions, `sudo`, a hard-coded git identity, GitHub host-key
  seeding, an SSH identity, `scripts/pi-ensure.sh`, `scripts/ssh-key-ensure.sh`, the `/config`
  mount. All of it goes: the owner keeps agent tooling outside the tree.
- The bench needs QEMU >= 10.1 (`-run-with exit-with-parent=on`, `qemu.rs` `EXIT_WITH_PARENT`)
  and OpenSSH >= 10.1 (`-o WarnWeakCrypto=no-pq-kex`, `ssh.rs` `REDOUBT_OPTIONS`), so the image
  pulls trixie-backports and stock Ubuntu LTS (24.04: QEMU 8.2, OpenSSH 9.6) cannot run it. Both
  are conveniences the bench can provide itself.

## Deliverables

### 1. `scripts/setup.sh`: the one list

Idempotent, no Docker, **any apt-based system** (Debian, Ubuntu and their derivatives: Mint,
Pop, Raspberry Pi OS...). Owner 2026-10-04: "debian/ubuntu vaguely, work on most". Do not
allowlist distributions or versions by name: require `apt-get` (refuse without it, naming it),
install, then check each tool against the minimum the bench proves (step 6 and section 3), and
refuse by naming the tool, its version and the minimum, never the distro. Package names are the
same across the family; everything Rust-side comes from rustup and cargo. Test on `debian:trixie`
(the image) and `ubuntu:24.04` containers at least; `ubuntu:22.04` (QEMU 6.2, OpenSSH 8.9) tells
you where the real minimums are, and is the right place to find them. Steps, each printed as it
runs and skipped when already done:

1. `apt-get install --no-install-recommends` of exactly: `build-essential pkg-config libssl-dev
   ca-certificates curl git xz-utils qemu-system-misc openssh-client openssh-server gdb-multiarch`
   (and whatever the bench's `-sys` crates need: check `cargo build` on a clean image and list
   each with its reason). Uses `sudo` only for this step, and only if not root.
2. `rustup` (user-local, `https://sh.rustup.rs`, profile minimal) with stable, the targets
   `riscv64imac-unknown-none-elf riscv32imac-unknown-none-elf riscv64gc-unknown-none-elf`, the
   components `rustfmt clippy llvm-tools`, nightly `rustfmt` (`rustfmt.toml` uses nightly
   options); honours `CARGO_HOME`/`RUSTUP_HOME` if set, else rustup's defaults.
3. `cargo install --locked` of `cargo-binutils@0.4.0 mdbook@0.5.4 mdbook-mermaid@0.17.1
   mdbook-svgbob@0.3.1` (the versions the Dockerfile pins today; keep them in one place at the
   top of the script).
4. `scripts/build-bios.sh` (the firmware, both widths).
5. `--with-beam`: OTP 28.5.0.6 from its release source and Elixir 1.20.4 from its release zip,
   each checked against the sha256 the Dockerfile pins today, built into `toolchains/otp-28.5.0.6`
   and `toolchains/elixir-1.20.4`, the layout `userland/otp/tools/env.sh` reads. Off by default:
   it is the price of beamlet's differential and Elixir suites, not of entry.
6. Verifies and prints: `cargo --version`, each target present, `qemu-system-riscv64 --version`,
   `ssh -V`, `mdbook --version`, the firmware ELFs' paths; then `./build --arch rv64` and
   `cargo testbench --list` succeed. Exit non-zero naming the first thing that does not.

### 2. The `Dockerfile` and `dev.sh`, reduced to Redoubt

`FROM debian:trixie`, a uid/gid-matched user, `COPY scripts/setup.sh` and `RUN` it with
`--with-beam` (so the image proves the script on every build), `CARGO_HOME`/`RUSTUP_HOME` under
`/opt` as today, `BEAMLET_TOOLCHAINS=/opt/toolchains` (the script takes a `--toolchains <dir>`
for this), `LANG=C.UTF-8`, `WORKDIR /work`, `CMD bash`. Nothing else: no Node, agent CLIs, pi,
`sudo`, git identity, SSH keys, `/config` mount, host-key seeding. `dev.sh`: build the image when
its revision label or uid/gid differ, seed `/work/.cargo` and `/work/.rustup` from the image
as today, run with `/work` mounted; `--rebuild`. Delete `scripts/pi-ensure.sh` and
`scripts/ssh-key-ensure.sh`. `.wash/local/in-dev` is the orchestrator's and not in the tree;
it uses `sudo` for a symlink: tell the orchestrator what replaces it (a second `-v` mount of the
checkout at its host path is the likely answer; propose, do not edit `.wash/local`).

### 3. The bench on stock packages

- **QEMU:** replace `EXIT_WITH_PARENT` with the same guarantee from the bench's side: a
  `pre_exec` hook on every QEMU `Command` that sets `prctl(PR_SET_PDEATHSIG, SIGKILL)` (the
  `libc` crate is already a dependency; check), with the usual re-check of `getppid()` after
  the call for the race where the parent died first. The QMP helper's `sh -c` script
  (`qemu.rs` ~line 624) and the reference guest's proxy (`ssh.rs`/`ssh_guest.rs`, run by `ssh`,
  not by the bench) drop the flag: `ssh` HUPs its proxy and BENCHENV1's keeper catches a guest
  that outlives its case. `usable()` then probes nothing version-specific; keep the probe
  machinery and its test for the next option that needs it, re-aimed at a real minimum if one
  exists (`-M virt` features the kernel needs, say), else at "`qemu-system-riscv64` runs
  `-version`". Minimum stated on the page becomes the oldest QEMU the whole bench passes on:
  find it by running the bench once under Ubuntu 24.04's 8.2 (a container `FROM ubuntu:24.04`
  running `scripts/setup.sh` is the test rig for this and for the script); report what fails,
  if anything, before working around it.
- **OpenSSH:** pass `WarnWeakCrypto=no-pq-kex` only when `ssh -G` takes it (`takes()` exists);
  when it does not, drop OpenSSH's one warning line about the exchange from the session's
  stderr before judging, matched exactly and recorded in the transcript as dropped. Nothing
  else about the sessions changes. State the minimum as the oldest OpenSSH the whole bench
  passes on (24.04's 9.6 is the test).
- The reference guest and Redoubt's own `sshd` are unaffected.

### 4. Pages

`GETTING-STARTED.md` prerequisites: "Run `scripts/setup.sh` (any apt-based system: Debian,
Ubuntu and their derivatives; `--with-beam` for beamlet's suites), or `./dev.sh` for the same
in a container." The package list leaves the
page; the versions the bench needs are stated as minimums the script checks.
`docs/testbench.md` lines ~59-62 (the `exit-with-parent` paragraph: now the parent-death
signal, no version) and ~796-797 (`WarnWeakCrypto`: when taken; else the one line dropped).
`CONTRIBUTING.md` if it names the container or the tools. Grep `pi-ensure|ssh-key-ensure|
/config|redoubt-config|PI_CODING|codex|claude` across the tree outside `.wash/` and `vendor/`
and report every hit with its disposition.

## Owned paths

`scripts/setup.sh` (new), `scripts/pi-ensure.sh` and `scripts/ssh-key-ensure.sh` (deleted),
`Dockerfile`, `dev.sh`, `tools/testbench/src/{qemu.rs,ssh.rs,ssh_guest.rs}` (the two flags and
the probe only), `GETTING-STARTED.md` prerequisites, `docs/testbench.md`'s two paragraphs,
`CONTRIBUTING.md` if needed. Nothing in the kernel, servers, runtime or the cases.

## Gates (Tier B, tooling)

- The image builds from the new `Dockerfile` (`./dev.sh --rebuild`), and inside it the testbench
  host tests, `cargo +nightly fmt --all --check`, doccheck and `./build` on both widths pass.
- The script on a clean `ubuntu:24.04` container: exit 0, then `./build --arch rv64`,
  `cargo testbench --list`, and the whole bench on rv64 with that QEMU and OpenSSH (one bench
  at a time on this host; ask the orchestrator first). Report the counts and any case that
  fails on 8.2/9.6 before changing anything for it.
- The whole bench on both widths in the new image, no `--allow-skip`.
- `git diff --check`; the size budget's bench row if it has one.
- Report every command with its exit code; the new `Dockerfile` and `dev.sh` line counts.

## Context rules

Tail every build and bench to its verdict and failing lines; never read a whole log, a QA file,
or `.wash/local/BENCHENV1-*`. Read in full only what you commit. Report with `member_update`
(<= 2000 bytes; detail in `target/TOOL1-report.md` in your worktree), set waiting, end your turn.
At 70 % context stop at a safe checkpoint, commit by path, register a handoff. Never push;
never `git add -A`; never `git stash`.
