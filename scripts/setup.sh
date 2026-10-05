#!/usr/bin/env bash
#
# setup.sh -- install what Redoubt needs to build, boot, test and render its book, on any
# apt-based system (Debian, Ubuntu and their derivatives). This script is the one list of
# prerequisites; the dev image runs it too.
#
# Usage: scripts/setup.sh [--with-beam] [--toolchains <dir>] [--skip-firmware] [--help]
#   --with-beam         also build beamlet's pinned OTP and Elixir, for its differential and
#                       Elixir suites (a long build; off by default)
#   --toolchains <dir>  where --with-beam puts them (default: toolchains/ in this checkout);
#                       point BEAMLET_TOOLCHAINS at any other directory
#   --skip-firmware     do not build the RustSBI firmware (scripts/build-bios.sh)
#
# Steps, each printed, and skipped when already done:
#   1. apt-get install the system packages (with sudo, unless run as root)
#   2. rustup, user-local (CARGO_HOME and RUSTUP_HOME if set, else rustup's defaults), with
#      the Rust that rust-toolchain.toml pins, its targets and components, and nightly's
#      rustfmt and Miri
#   3. cargo install the pinned tools: cargo-binutils and mdbook with its preprocessors
#   4. the firmware, both widths
#   5. with --with-beam, OTP and Elixir, each checked against its pinned sha256
#   6. verify: each tool against the minimum version the bench needs, the firmware, then
#      `./build --arch rv64` and `cargo testbench --list`
#
# It exits non-zero naming the first thing that fails. Run outside a checkout (the dev image
# copies only this script and rust-toolchain.toml), it installs and checks the tools and skips
# the steps that need the tree: the firmware, `./build` and the bench.
set -euo pipefail

# ---- the versions ---------------------------------------------------------
# The oldest QEMU and OpenSSH the whole bench has passed on: Ubuntu 24.04's, on 2026-10-04.
# An older one may work; nothing has shown it.
QEMU_MIN=8.2
OPENSSH_MIN=9.6

CARGO_TOOLS=(cargo-binutils@0.4.0 mdbook@0.5.4 mdbook-mermaid@0.17.1 mdbook-svgbob@0.3.1)

OTP_VERSION=28.5.0.6
OTP_SHA256=49d7a75e906334af54ae336ba53fc4e6ad100645e8e7efd3be008de284dab3ba
ELIXIR_VERSION=1.20.4
ELIXIR_SHA256=ea7ff98bc1ed76c663a5d034c863c6fd37e65c9c855b6bbf6ccb2034d806673c

# build-essential: the linker for host binaries, and the C compiler for the crates that build C
# with cc (none links a system library). qemu-system-riscv64/32 come from qemu-system-riscv
# or qemu-system-misc (chosen below). curl and xz-utils, with dpkg's dpkg-deb: the bench builds
# OpenSSH's reference server guest from Debian's packages. openssh-client: the bench's ssh.
# gdb-multiarch: a GDB that knows RISC-V, for QEMU's GDB stub. No openssh-server: the bench
# runs OpenSSH's server only in that guest, never the host's.
APT_PACKAGES=(build-essential ca-certificates curl git xz-utils openssh-client gdb-multiarch)
# OTP's build: its crypto app (libssl-dev), its terminal (libncurses-dev) and zlib; unzip for
# Elixir's release zip; pkg-config, which beamlet's pcre2-sys tries before its bundled copy.
APT_PACKAGES_BEAM=(pkg-config libssl-dev libncurses-dev zlib1g-dev unzip)

# ---- arguments ------------------------------------------------------------
usage() { sed -n '3,13s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"; }

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
with_beam=0
skip_firmware=0
toolchains=""
while (($#)); do
    case "$1" in
        --with-beam)      with_beam=1; shift ;;
        --toolchains)     toolchains="${2:?--toolchains needs a directory}"; shift 2 ;;
        --skip-firmware)  skip_firmware=1; shift ;;
        -h|--help)        usage; exit 0 ;;
        *)                echo "setup.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done
if [ -n "$toolchains" ] && [ "$with_beam" = 0 ]; then
    echo "setup.sh: --toolchains only applies with --with-beam" >&2
    exit 2
fi
case "$toolchains" in
    ""|/*) ;;
    *) toolchains="$PWD/$toolchains" ;;
esac
# Everything below names its directory. From /, no rust-toolchain.toml applies, so rustup
# answers about what is installed instead of quietly installing the checkout's pin.
cd /

step() { printf '==> %s\n' "$*"; }
done_() { printf '    already done: %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
fail() { printf 'setup.sh: FAILED: %s\n' "$*" >&2; exit 1; }

# A checkout has the build script and the firmware next to this one.
in_tree=0
if [ -x "$root/build" ] && [ -f "$root/bios/Cargo.toml" ]; then
    in_tree=1
fi

# `ver_ge A B`: version A is B or later.
ver_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

# ---- 1. system packages ---------------------------------------------------
step "system packages (apt-get)"
command -v apt-get >/dev/null 2>&1 \
    || fail "apt-get not found: this script needs an apt-based system (Debian, Ubuntu or a derivative)"

as_root() {
    if [ "$(id -u)" = 0 ]; then
        env DEBIAN_FRONTEND=noninteractive "$@"
    else
        command -v sudo >/dev/null 2>&1 || fail "not root and no sudo: run this script as root, or install sudo"
        sudo env DEBIAN_FRONTEND=noninteractive "$@"
    fi
}

installed() { [ "$(dpkg-query -W -f='${db:Status-Status}' "$1" 2>/dev/null)" = installed ]; }

wanted=("${APT_PACKAGES[@]}")
[ "$with_beam" = 1 ] && wanted+=("${APT_PACKAGES_BEAM[@]}")
missing=()
for p in "${wanted[@]}"; do
    installed "$p" || missing+=("$p")
done
have_qemu=1
command -v qemu-system-riscv64 >/dev/null 2>&1 && command -v qemu-system-riscv32 >/dev/null 2>&1 \
    || have_qemu=0

if [ "${#missing[@]}" = 0 ] && [ "$have_qemu" = 1 ]; then
    done_ "${wanted[*]} and qemu-system-riscv64/32"
else
    as_root apt-get update
    if [ "$have_qemu" = 0 ]; then
        # Chosen by what the archive offers, never by the distribution's name: newer archives
        # (Debian trixie, Ubuntu 26.04) ship RISC-V in qemu-system-riscv, and their
        # qemu-system-misc lacks it; older ones (Ubuntu 24.04, 22.04) only in qemu-system-misc.
        if apt-cache show qemu-system-riscv >/dev/null 2>&1; then
            missing+=(qemu-system-riscv)
        else
            missing+=(qemu-system-misc)
        fi
    fi
    note "installing: ${missing[*]}"
    as_root apt-get install -y --no-install-recommends "${missing[@]}"
fi

# ---- 2. rustup and the Rust toolchains ------------------------------------
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
# The caller's PATH, before this script puts rustup's cargo first.
caller_path="$PATH"

step "rustup in $CARGO_HOME (toolchains in $RUSTUP_HOME)"
if [ -x "$CARGO_HOME/bin/rustup" ]; then
    done_ "rustup $("$CARGO_HOME/bin/rustup" --version 2>/dev/null | sed -n '1s/^rustup \([^ ]*\).*/\1/p')"
else
    # Over a distribution's Rust, the installer prints an error and carries on; the NOTE at
    # the end says instead whether that Rust shadows rustup's on the PATH.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | RUSTUP_INIT_SKIP_PATH_CHECK=yes sh -s -- -y --profile minimal --default-toolchain none
fi
export PATH="$CARGO_HOME/bin:$PATH"

# The checkout's rust-toolchain.toml pins the one Rust everything outside bios/ builds with,
# here and in the dev image; rustup selects it in the checkout. Its arrays are one line each.
toolchain_file="$root/rust-toolchain.toml"
[ -f "$toolchain_file" ] || fail "no $toolchain_file"
toml_list() {
    sed -n "s/^$1 = \[\(.*\)\]\$/\1/p" "$toolchain_file" | tr ',' '\n' | tr -d '" '
}
channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$toolchain_file")"
mapfile -t rust_targets < <(toml_list targets)
mapfile -t rust_components < <(toml_list components)
[ -n "$channel" ] && [ "${#rust_targets[@]}" -gt 0 ] && [ "${#rust_components[@]}" -gt 0 ] \
    || fail "$toolchain_file has no channel, targets or components line"

step "Rust $channel (rust-toolchain.toml)"
if grep -q "^$channel-" <<<"$(rustup toolchain list)"; then
    done_ "$(rustc "+$channel" --version)"
else
    rustup toolchain install "$channel" --profile minimal
fi
if ! rustup default >/dev/null 2>&1; then
    rustup default "$channel"
fi

step "targets: ${rust_targets[*]}"
have="$(rustup target list --installed --toolchain "$channel")"
add=()
for t in "${rust_targets[@]}"; do
    grep -qx "$t" <<<"$have" || add+=("$t")
done
if [ "${#add[@]}" = 0 ]; then done_ "all present"; else rustup target add --toolchain "$channel" "${add[@]}"; fi

step "components: ${rust_components[*]}"
have="$(rustup component list --installed --toolchain "$channel")"
add=()
for c in "${rust_components[@]}"; do
    grep -q "^$c-" <<<"$have" || add+=("$c")
done
if [ "${#add[@]}" = 0 ]; then done_ "all present"; else rustup component add --toolchain "$channel" "${add[@]}"; fi

# rustfmt.toml uses nightly options, and the bench's Miri cases run under nightly Miri, which
# needs the standard library's source.
NIGHTLY_COMPONENTS=(rustfmt miri rust-src)
step "nightly: ${NIGHTLY_COMPONENTS[*]}"
if ! grep -q '^nightly-' <<<"$(rustup toolchain list)"; then
    rustup toolchain install nightly --profile minimal
fi
have="$(rustup component list --installed --toolchain nightly)"
add=()
for c in "${NIGHTLY_COMPONENTS[@]}"; do
    grep -q "^$c\(-\|$\)" <<<"$have" || add+=("$c")
done
if [ "${#add[@]}" = 0 ]; then done_ "all present"; else rustup component add --toolchain nightly "${add[@]}"; fi

# ---- 3. pinned cargo tools ------------------------------------------------
step "cargo tools: ${CARGO_TOOLS[*]}"
have="$(cargo install --list)"
for spec in "${CARGO_TOOLS[@]}"; do
    name="${spec%@*}" version="${spec#*@}"
    if grep -q "^$name v$version:" <<<"$have"; then
        done_ "$spec"
    else
        cargo install --locked "$spec"
    fi
done

# ---- 4. the firmware ------------------------------------------------------
firmware=("$root/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper"
          "$root/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper")
step "firmware (scripts/build-bios.sh)"
if [ "$skip_firmware" = 1 ]; then
    note "skipped: --skip-firmware"
elif [ "$in_tree" = 0 ]; then
    note "skipped: not in a Redoubt checkout"
elif [ -f "${firmware[0]}" ] && [ -f "${firmware[1]}" ]; then
    done_ "both widths built (run scripts/build-bios.sh after changing bios/)"
else
    # bios/rust-toolchain.toml pins the firmware's own nightly; install it with its targets.
    ( cd "$root/bios" && rustup toolchain install )
    "$root/scripts/build-bios.sh"
fi

# ---- 5. beamlet's OTP and Elixir ------------------------------------------
if [ "$with_beam" = 1 ]; then
    toolchains="${toolchains:-$root/toolchains}"
    mkdir -p "$toolchains"
    toolchains="$(cd "$toolchains" && pwd)"
    otp="$toolchains/otp-$OTP_VERSION"
    elixir="$toolchains/elixir-$ELIXIR_VERSION"
    scratch="$(mktemp -d)"
    trap 'rm -rf "$scratch"' EXIT

    step "OTP $OTP_VERSION in $otp"
    if [ -x "$otp/bin/erl" ]; then
        done_ "$otp/bin/erl"
    else
        # Built as a release root, the layout userland/otp/tools/env.sh reads, without the
        # apps that need wx, ODBC or Java.
        curl -fsSL -o "$scratch/otp.tar.gz" \
            "https://github.com/erlang/otp/releases/download/OTP-${OTP_VERSION}/otp_src_${OTP_VERSION}.tar.gz"
        echo "${OTP_SHA256}  $scratch/otp.tar.gz" | sha256sum -c -
        mkdir "$scratch/otp"
        tar xzf "$scratch/otp.tar.gz" -C "$scratch/otp" --strip-components=1
        ( cd "$scratch/otp" \
            && ./configure --without-wx --without-odbc --without-javac --without-jinterface \
                 --without-debugger --without-observer --without-et \
            && make -j"$(nproc)" \
            && make release RELEASE_ROOT="$otp" )
        ( cd "$otp" && ./Install -minimal "$PWD" )
    fi

    step "Elixir $ELIXIR_VERSION in $elixir"
    if [ -x "$elixir/bin/elixir" ]; then
        done_ "$elixir/bin/elixir"
    else
        # The release's precompiled zip, the same on every machine.
        curl -fsSL -o "$scratch/elixir.zip" \
            "https://github.com/elixir-lang/elixir/releases/download/v${ELIXIR_VERSION}/elixir-otp-${OTP_VERSION%%.*}.zip"
        echo "${ELIXIR_SHA256}  $scratch/elixir.zip" | sha256sum -c -
        unzip -q "$scratch/elixir.zip" -d "$elixir"
    fi
fi

# ---- 6. verify ------------------------------------------------------------
step "verify"
cargo_v="$(cd "$root" && cargo --version)" || fail "cargo does not run"
note "$cargo_v ($(command -v cargo))"
rustc_v="$(cd "$root" && rustc --version)" || fail "rustc does not run"
case "$rustc_v" in
    "rustc $channel "*) note "$rustc_v, as rust-toolchain.toml pins" ;;
    *) fail "rustc in the checkout is $rustc_v; rust-toolchain.toml pins $channel" ;;
esac

have="$(rustup target list --installed --toolchain "$channel")"
for t in "${rust_targets[@]}"; do
    grep -qx "$t" <<<"$have" || fail "the Rust target $t is not installed"
    note "target $t"
done
rustup run nightly rustfmt --version >/dev/null 2>&1 || fail "nightly rustfmt does not run"
cargo +nightly miri --version >/dev/null 2>&1 || fail "nightly Miri does not run"

for w in 64 32; do
    q="qemu-system-riscv$w"
    command -v "$q" >/dev/null 2>&1 || fail "$q not found"
    v="$("$q" --version | sed -n '1s/^QEMU emulator version \([0-9][0-9.]*\).*/\1/p')"
    [ -n "$v" ] || fail "$q --version printed no version"
    ver_ge "$v" "$QEMU_MIN" || fail "$q is QEMU $v; the bench needs QEMU $QEMU_MIN or later"
    note "$q: QEMU $v"
done

command -v ssh >/dev/null 2>&1 || fail "ssh not found"
v="$(ssh -V 2>&1 | sed -n '1s/^OpenSSH_\([0-9][0-9.]*\).*/\1/p')"
[ -n "$v" ] || fail "ssh -V printed no OpenSSH version"
ver_ge "$v" "$OPENSSH_MIN" || fail "ssh is OpenSSH $v; the bench needs OpenSSH $OPENSSH_MIN or later"
note "ssh: OpenSSH $v"
for t in curl xz dpkg-deb; do
    command -v "$t" >/dev/null 2>&1 || fail "$t not found (the bench builds its reference guest with it)"
done

v="$(mdbook --version)" || fail "mdbook does not run"
note "$v"

if [ "$with_beam" = 1 ]; then
    "$otp/bin/erl" -noshell -eval 'halt().' || fail "$otp/bin/erl does not run"
    v="$(PATH="$otp/bin:$PATH" "$elixir/bin/elixir" --version | sed -n 's/^Elixir \([^ ]*\).*/\1/p')"
    [ "$v" = "$ELIXIR_VERSION" ] || fail "$elixir/bin/elixir does not run as Elixir $ELIXIR_VERSION"
    note "OTP $OTP_VERSION and Elixir $v in $toolchains"
fi

if [ "$in_tree" = 1 ]; then
    if [ "$skip_firmware" = 0 ]; then
        for f in "${firmware[@]}"; do
            [ -f "$f" ] || fail "firmware missing: $f"
            note "firmware $f"
        done
    fi

    logs="$root/target/setup"
    mkdir -p "$logs"
    # `run <label> <log> <command...>`: the output to a log, its tail on failure.
    run() {
        local label="$1" log="$2"
        shift 2
        if ( cd "$root" && "$@" ) >"$log" 2>&1; then
            note "$label: ok"
        else
            tail -n 20 "$log" >&2
            fail "$label (log: $log)"
        fi
    }
    run "./build --arch rv64" "$logs/build-rv64.log" ./build --arch rv64
    run "cargo testbench --list" "$logs/testbench-list.log" cargo testbench --list
else
    note "skipped ./build and cargo testbench --list: not in a Redoubt checkout"
fi

# ---- done -----------------------------------------------------------------
step "done"
# rustup's installer adds `. "$HOME/.cargo/env"` to the shell profiles, and that file puts
# its cargo first on PATH; this script never edits a profile, it only says what a new shell
# will find.
path_cargo="$(PATH="$caller_path"; hash -r; command -v cargo || true)"
if [ "$path_cargo" != "$CARGO_HOME/bin/cargo" ]; then
    env_file="$CARGO_HOME/env"
    sourced=""
    for f in "$HOME/.profile" "$HOME/.bash_profile" "$HOME/.bashrc" "$HOME/.zshenv"; do
        if [ -f "$f" ] && grep -qF -e "$env_file" -e "\$HOME${env_file#"$HOME"}" "$f"; then
            sourced="$f"
            break
        fi
    done
    if [ -n "$sourced" ]; then
        note "NOTE: open a new shell: rustup put \`. \"$env_file\"\` in $sourced, which puts its cargo first."
    elif [ -f "$env_file" ]; then
        note "NOTE: no shell profile sources $env_file; add this one line to yours, then open a new shell: . \"$env_file\""
    else
        note "NOTE: put $CARGO_HOME/bin first on PATH in your shell profile, then open a new shell."
    fi
fi
if installed cargo || installed rustc; then
    note "NOTE: the distribution's Rust in /usr/bin is unused by this project; \`sudo apt-get remove cargo rustc\` if you want one Rust."
fi
if [ "$with_beam" = 1 ] && [ "$toolchains" != "$root/toolchains" ]; then
    note "beamlet's suites find OTP and Elixir with: export BEAMLET_TOOLCHAINS=$toolchains"
fi
